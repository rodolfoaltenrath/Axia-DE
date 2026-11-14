/* rules_json.c — the firewall's file (~/.config/axia/firewall.json): fw_to_json, fw_from_json,
 * fw_save and fw_load, on top of the public API of the rule store and decider, which are Rust
 * (rust/axia-codecs/src/fwrules.rs; see rules.h and docs/FIREWALL.md). */
#ifndef _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#if defined(__APPLE__) && !defined(_DARWIN_C_SOURCE)
#define _DARWIN_C_SOURCE                /* O_NOFOLLOW with _POSIX_C_SOURCE on BSD-libc hosts (tests) */
#endif
#include "fw/rules.h"
#include "sec/json.h"

#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static const char *const FLAG_NAMES[] = {
    "no-internet", "no-lan", "no-network", "no-incoming", "no-camera", "no-mic", "no-screen",
    "no-usb", "no-bluetooth", "no-files", "no-notifications", "no-clipboard",
};
#define NFLAGS (int)(sizeof FLAG_NAMES / sizeof *FLAG_NAMES)

static int parse_uint(const char *s, size_t n, unsigned max, unsigned *out) {
    if (!n || n > 5 || (n > 1 && s[0] == '0')) return -1;
    unsigned v = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] < '0' || s[i] > '9') return -1;
        v = v * 10 + (unsigned)(s[i] - '0');
    }
    if (v > max) return -1;
    *out = v;
    return 0;
}

/* ---- JSON -------------------------------------------------------------------------------------- */

typedef struct { char *p; size_t n, cap; int err; } Buf;

static void put(Buf *b, const char *s, size_t n) {
    if (b->err) return;
    if (b->n + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 1024;
        while (cap < b->n + n + 1) cap *= 2;
        char *p = realloc(b->p, cap);
        if (!p) { b->err = 1; return; }
        b->p = p;
        b->cap = cap;
    }
    memcpy(b->p + b->n, s, n);
    b->n += n;
    b->p[b->n] = 0;
}

static void puts_(Buf *b, const char *s) { put(b, s, strlen(s)); }

static void quote(Buf *b, const char *s) {
    char q[6 * FW_NAME_MAX + 8];
    size_t n = json_quote(q, sizeof q, s);
    put(b, q, n);
}

static void putf(Buf *b, const char *fmt, ...) {
    char t[128];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(t, sizeof t, fmt, ap);
    va_end(ap);
    if (n < 0 || (size_t)n >= sizeof t) { b->err = 1; return; }
    put(b, t, (size_t)n);
}

static const char *const MODE_NAMES[] = { "silent", "ask", "off", "deny" };

typedef struct { char id[FW_APP_MAX]; int trusted; uint32_t restricted; } AppRow;
typedef struct { AppRow *v; size_t n, cap; int err; } AppRows;

static int collect_app(void *u, const char *app, int trusted, uint32_t flags) {
    AppRows *r = u;
    if (r->n == r->cap) {
        size_t cap = r->cap ? r->cap * 2 : 64;
        AppRow *v = realloc(r->v, cap * sizeof *v);
        if (!v) { r->err = 1; return 1; }
        r->v = v;
        r->cap = cap;
    }
    snprintf(r->v[r->n].id, sizeof r->v[r->n].id, "%s", app);
    r->v[r->n].trusted = trusted;
    r->v[r->n].restricted = flags;
    r->n++;
    return 0;
}

static int cmp_app(const void *x, const void *y) { return strcmp(((const AppRow *)x)->id, ((const AppRow *)y)->id); }

char *fw_to_json(const FwStore *s, size_t *len) {
    if (!s) return NULL;
    Buf b = { 0 };
    AppRows apps = { 0 };
    fw_app_each(s, collect_app, &apps);
    size_t total = fw_rule_count(s);
    FwRule *rules = malloc((total ? total : 1) * sizeof *rules);
    if (apps.err || !rules) { free(apps.v); free(rules); return NULL; }
    size_t nr = (size_t)fw_rule_list(s, NULL, rules, (int)total);
    if (nr > total) nr = total;
    size_t na = apps.n;
    if (na) qsort(apps.v, na, sizeof *apps.v, cmp_app);

    putf(&b, "{\"v\":1,\"mode\":\"%s\",\"trusted\":[", MODE_NAMES[fw_mode(s)]);
    int first = 1;
    for (size_t i = 0; i < na; i++) {
        if (!apps.v[i].trusted) continue;
        if (!first) puts_(&b, ",");
        first = 0;
        quote(&b, apps.v[i].id);
    }
    puts_(&b, "],\"restricted\":{");
    first = 1;
    for (size_t i = 0; i < na; i++) {
        if (!apps.v[i].restricted) continue;
        if (!first) puts_(&b, ",");
        first = 0;
        quote(&b, apps.v[i].id);
        puts_(&b, ":[");
        int f1 = 1;
        for (int k = 0; k < NFLAGS; k++)
            if (apps.v[i].restricted & (1u << k)) { putf(&b, "%s\"%s\"", f1 ? "" : ",", FLAG_NAMES[k]); f1 = 0; }
        puts_(&b, "]");
    }
    puts_(&b, "},\"rules\":[");
    first = 1;
    for (size_t i = 0; i < nr; i++) {
        const FwRule *r = &rules[i];
        if (r->life != FW_LIFE_FOREVER) continue;     /* the others don't survive the session */
        char dst[FW_NAME_MAX + 8];
        if (fw_format_dest(r, dst, sizeof dst)) { b.err = 1; break; }
        puts_(&b, first ? "{\"app\":" : ",{\"app\":");
        first = 0;
        quote(&b, r->app);
        putf(&b, ",\"dir\":\"%s\",\"dst\":", r->dir == FW_IN ? "in" : "out");
        quote(&b, dst);
        if (r->port_lo != 0 || r->port_hi != 65535) {
            if (r->port_lo == r->port_hi) putf(&b, ",\"port\":\"%u\"", r->port_lo);
            else putf(&b, ",\"port\":\"%u-%u\"", r->port_lo, r->port_hi);
        }
        if (r->proto != FW_PROTO_ANY) putf(&b, ",\"proto\":\"%s\"", r->proto == FW_PROTO_TCP ? "tcp" : "udp");
        putf(&b, ",\"action\":\"%s\",\"when\":%lld}", r->action == FW_DENY ? "deny" : "allow", r->when);
    }
    puts_(&b, "]}\n");
    free(apps.v);
    free(rules);
    if (b.err) { free(b.p); return NULL; }
    if (len) *len = b.n;
    return b.p;
}

static int jerr(char *err, size_t n, const char *fmt, ...) {
    if (err && n) {
        va_list ap;
        va_start(ap, fmt);
        vsnprintf(err, n, fmt, ap);
        va_end(ap);
    }
    return -1;
}

static int only_keys(const JVal *o, const char *const *keys, const char **bad) {
    for (const JVal *k = o->kid; k; k = k->next) {
        int ok = 0;
        for (int i = 0; keys[i]; i++) if (!strcmp(k->key, keys[i])) { ok = 1; break; }
        if (!ok) { *bad = k->key; return -1; }
    }
    return 0;
}

/* A JSON text with no 0 inside and of reasonable length. */
static const char *jstr(const JVal *v, size_t max) {
    if (!v || v->type != J_STR || v->len >= max || strlen(v->str) != v->len) return NULL;
    return v->str;
}

static int parse_ports(const JVal *v, FwRule *r) {
    unsigned lo, hi;
    if (v->type == J_NUM) {
        if (v->num < 1 || v->num > 65535 || v->num != (double)(unsigned)v->num) return -1;
        r->port_lo = r->port_hi = (uint16_t)v->num;
        return 0;
    }
    const char *s = jstr(v, 16);
    if (!s) return -1;
    const char *dash = strchr(s, '-');
    if (dash) {
        if (parse_uint(s, (size_t)(dash - s), 65535, &lo) || parse_uint(dash + 1, strlen(dash + 1), 65535, &hi) || lo > hi) return -1;
    } else {
        if (parse_uint(s, strlen(s), 65535, &lo)) return -1;
        hi = lo;
    }
    if (!lo) return -1;                     /* port 0 doesn't exist; "any" = no "port" */
    r->port_lo = (uint16_t)lo;
    r->port_hi = (uint16_t)hi;
    return 0;
}

static int load_rule(FwStore *t, const JVal *o, int idx, char *err, size_t errlen) {
    static const char *const keys[] = { "app", "dir", "dst", "port", "proto", "action", "when", NULL };
    const char *bad;
    if (o->type != J_OBJ) return jerr(err, errlen, "rule %d: not an object", idx);
    if (only_keys(o, keys, &bad)) return jerr(err, errlen, "rule %d: unknown key \"%.32s\"", idx, bad);
    FwRule r;
    memset(&r, 0, sizeof r);
    r.port_hi = 65535;
    const char *app = jstr(json_get(o, "app"), FW_APP_MAX);
    if (!app || fw_app_canon(app, r.app, sizeof r.app) || strcmp(app, r.app))
        return jerr(err, errlen, "rule %d: invalid app", idx);
    const JVal *v;
    if ((v = json_get(o, "dir"))) {
        const char *d = jstr(v, 8);
        if (d && !strcmp(d, "out")) r.dir = FW_OUT;
        else if (d && !strcmp(d, "in")) r.dir = FW_IN;
        else return jerr(err, errlen, "rule %d: invalid dir", idx);
    }
    const char *dst = jstr(json_get(o, "dst"), FW_NAME_MAX + 4);
    if (!dst || fw_parse_dest(dst, &r)) return jerr(err, errlen, "rule %d: invalid dst", idx);
    if ((v = json_get(o, "port")) && parse_ports(v, &r)) return jerr(err, errlen, "rule %d: invalid port", idx);
    if ((v = json_get(o, "proto"))) {
        const char *p = jstr(v, 8);
        if (p && !strcmp(p, "tcp")) r.proto = FW_PROTO_TCP;
        else if (p && !strcmp(p, "udp")) r.proto = FW_PROTO_UDP;
        else if (p && !strcmp(p, "any")) r.proto = FW_PROTO_ANY;
        else return jerr(err, errlen, "rule %d: invalid proto", idx);
    }
    const char *act = jstr(json_get(o, "action"), 8);
    if (act && !strcmp(act, "allow")) r.action = FW_ALLOW;
    else if (act && !strcmp(act, "deny")) r.action = FW_DENY;
    else return jerr(err, errlen, "rule %d: invalid action", idx);
    if ((v = json_get(o, "when"))) {
        if (v->type != J_NUM || v->num < 0 || v->num > 9007199254740992.0 || v->num != (double)(long long)v->num)
            return jerr(err, errlen, "rule %d: invalid when", idx);
        r.when = (long long)v->num;
    }
    r.life = FW_LIFE_FOREVER;
    if (fw_rule_add(t, &r, NULL)) return jerr(err, errlen, "rule %d: doesn't fit", idx);
    return 0;
}

static int load_doc(FwStore *t, const JVal *root, char *err, size_t errlen) {
    static const char *const keys[] = { "v", "mode", "trusted", "restricted", "rules", NULL };
    const char *bad;
    if (!root || root->type != J_OBJ) return jerr(err, errlen, "the root is not an object");
    if (only_keys(root, keys, &bad)) return jerr(err, errlen, "unknown key \"%.32s\"", bad);
    const JVal *v = json_get(root, "v");
    if (!v || v->type != J_NUM || v->num != 1) return jerr(err, errlen, "unsupported version");

    if ((v = json_get(root, "mode"))) {
        const char *m = jstr(v, 8);
        int k = -1;
        for (int i = 0; m && i < 4; i++) if (!strcmp(m, MODE_NAMES[i])) k = i;
        if (k < 0) return jerr(err, errlen, "invalid mode");
        fw_set_mode(t, (FwMode)k);
    }
    if ((v = json_get(root, "trusted"))) {
        if (v->type != J_ARR) return jerr(err, errlen, "trusted is not a list");
        if (v->len > FW_MAX_APPS) return jerr(err, errlen, "trusted: too many apps");
        int i = 0;
        for (const JVal *k = v->kid; k; k = k->next, i++) {
            const char *app = jstr(k, FW_APP_MAX);
            char c[FW_APP_MAX];
            if (!app || fw_app_canon(app, c, sizeof c) || strcmp(app, c)) return jerr(err, errlen, "trusted %d: invalid app", i);
            if (fw_is_trusted(t, c)) return jerr(err, errlen, "trusted %d: repeated", i);
            if (fw_set_trusted(t, c, 1)) return jerr(err, errlen, "trusted %d: doesn't fit", i);
        }
    }
    if ((v = json_get(root, "restricted"))) {
        if (v->type != J_OBJ) return jerr(err, errlen, "restricted is not an object");
        if (v->len > FW_MAX_APPS) return jerr(err, errlen, "restricted: too many apps");
        for (const JVal *k = v->kid; k; k = k->next) {
            char c[FW_APP_MAX];
            if (strlen(k->key) >= FW_APP_MAX || fw_app_canon(k->key, c, sizeof c) || strcmp(k->key, c))
                return jerr(err, errlen, "restricted: invalid app");
            if (fw_is_trusted(t, c)) return jerr(err, errlen, "restricted: %.64s is also trusted", c);
            if (k->type != J_ARR || !k->len || k->len > NFLAGS) return jerr(err, errlen, "restricted %.64s: invalid list", c);
            uint32_t flags = 0;
            for (const JVal *f = k->kid; f; f = f->next) {
                uint32_t bit = fw_flag_from_name(jstr(f, 32));
                if (!bit || (flags & bit)) return jerr(err, errlen, "restricted %.64s: invalid or repeated restriction", c);
                flags |= bit;
            }
            if (fw_set_restricted(t, c, flags)) return jerr(err, errlen, "restricted %.64s: doesn't fit", c);
        }
    }
    if ((v = json_get(root, "rules"))) {
        if (v->type != J_ARR) return jerr(err, errlen, "rules is not a list");
        if (v->len > FW_MAX_RULES) return jerr(err, errlen, "too many rules");
        int i = 0;
        for (const JVal *k = v->kid; k; k = k->next, i++)
            if (load_rule(t, k, i, err, errlen)) return -1;
    }
    return 0;
}

int fw_from_json(FwStore *s, const char *text, size_t len, char *err, size_t errlen) {
    if (err && errlen) err[0] = 0;
    if (!s || !text) return jerr(err, errlen, "no data");
    if (len > FW_FILE_MAX) return jerr(err, errlen, "file too large");
    if (memchr(text, 0, len)) return jerr(err, errlen, "byte 0 in the file");
    JDoc doc;
    char jmsg[128];
    if (json_parse(text, len, &doc, jmsg, sizeof jmsg)) return jerr(err, errlen, "invalid JSON: %s", jmsg);
    FwStore *t = fw_store_new();
    if (!t) { json_free(&doc); return jerr(err, errlen, "out of memory"); }
    int r = load_doc(t, doc.root, err, errlen);
    json_free(&doc);
    if (r) { fw_store_free(t); return -1; }
    /* everything validated: the contents are swapped in one go */
    fw_store_swap(s, t);
    fw_store_free(t);
    return 0;
}

int fw_save(const FwStore *s, const char *path) {
    if (!s || !path) return -1;
    size_t len;
    char *text = fw_to_json(s, &len);
    if (!text) return -1;
    char tmp[4096];
    if ((size_t)snprintf(tmp, sizeof tmp, "%s.tmp", path) >= sizeof tmp) { free(text); return -1; }
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (fd < 0) { free(text); return -1; }
    size_t off = 0;
    while (off < len) {
        ssize_t w = write(fd, text + off, len - off);
        if (w < 0 && errno == EINTR) continue;
        if (w <= 0) break;
        off += (size_t)w;
    }
    free(text);
    int ok = off == len && !fsync(fd);
    if (close(fd)) ok = 0;
    if (!ok || rename(tmp, path)) { unlink(tmp); return -1; }
    return 0;
}

int fw_load(FwStore *s, const char *path, char *err, size_t errlen) {
    if (err && errlen) err[0] = 0;
    if (!s || !path) return jerr(err, errlen, "no path");
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) return 1;      /* there's no file yet: s stays as it is */
        return jerr(err, errlen, "could not open: %s", strerror(errno));
    }
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size < 0 || (unsigned long long)st.st_size > FW_FILE_MAX) {
        close(fd);
        return jerr(err, errlen, "not a regular file or too large");
    }
    size_t want = (size_t)st.st_size, got = 0;
    char *buf = malloc(want + 1);
    if (!buf) { close(fd); return jerr(err, errlen, "out of memory"); }
    while (got < want) {
        ssize_t r = read(fd, buf + got, want - got);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) break;
        got += (size_t)r;
    }
    close(fd);
    if (got != want) { free(buf); return jerr(err, errlen, "incomplete read"); }
    int r = fw_from_json(s, buf, got, err, errlen);
    free(buf);
    return r;
}
