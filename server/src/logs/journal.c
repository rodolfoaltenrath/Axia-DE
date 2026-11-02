/* journal.c — our own reader for systemd journal files (format from
 * systemd/src/libsystemd/sd-journal/journal-def.h, versions 187 to 257). Used by the Logs app
 * (lord.logs, as the user: it reads only the files that the kernel and journald's ACLs let it
 * read) and by the helper's logs-system action (root, for administrators).
 *
 * The format, in short (all little-endian, objects aligned to 8):
 *   "LPKSHHRH" header with incompatible flags (XZ=1, LZ4=2, KEYED_HASH=4, ZSTD=8, COMPACT=16),
 *   n_entries and entry_array_offset;
 *   ENTRY_ARRAY: [header 16][next array 8][entry offsets 8 (or 4 in compact mode)];
 *   ENTRY: [header 16][seqnum][realtime][monotonic][boot_id 16][xor_hash] + items: offset of
 *          the DATA (+ hash) — 16 bytes per item, or 4 in compact mode;
 *   DATA:  [header 16][hash][next_hash][next_field][entry][entry_array][n_entries]
 *          (+ tail_entry_array_offset and n in compact mode) and the "FIELD=value" payload,
 *          possibly compressed (XZ, LZ4 with the size in the first 8 bytes, or ZSTD).
 *
 * Safety: never mmap (if journald shrinks the file, mmap would raise SIGBUS): pread with a small
 * block cache. Every offset is checked against the file size, the array chain must always move
 * forward (no loops) and sizes are capped. ZSTD has no decoder of our own yet: those fields are
 * shown as «[zstd-compressed]». */
#include "journal.h"
#include "../pkg/xz.h"

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <unistd.h>

#define BS 65536u                       /* cache block */
#define NB 32                           /* blocks (2 MB per open file) */
#define DC 8192                         /* cache of short fields by offset */
#define HDR_MIN 208                     /* systemd ≥ 187 headers */
#define MAX_ENTRY_SIZE (1u << 20)
#define MAX_PAYLOAD (1u << 20)          /* compressed or not: anything bigger is not read */

enum { INC_XZ = 1, INC_LZ4 = 2, INC_KEYED = 4, INC_ZSTD = 8, INC_COMPACT = 16, INC_KNOWN = 31 };
enum { OBJ_DATA = 1, OBJ_ENTRY = 3, OBJ_ENTRY_ARRAY = 6 };
enum { OF_XZ = 1, OF_LZ4 = 2, OF_ZSTD = 4 };
enum { F_NONE = 0, F_MSG, F_PRIO, F_IDENT, F_COMM, F_UNIT, F_UUNIT, F_PID, F_UID, F_BOOT, F_TRANSPORT };

typedef struct { uint64_t base; uint32_t len; int valid; uint8_t *buf; } Blk;
typedef struct { uint64_t off; uint8_t id; char val[95]; } DCache;

struct JFile {
    int fd;
    uint64_t size;
    int compact;
    uint64_t *ent;                      /* entry offsets, in order */
    size_t nent;
    Blk blk[NB];
    DCache *dc;
};

static uint64_t le64(const uint8_t *p) { uint64_t v = 0; for (int i = 7; i >= 0; i--) v = v << 8 | p[i]; return v; }
static uint32_t le32(const uint8_t *p) { return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }

static int pread_all(int fd, void *buf, size_t len, uint64_t off) {
    size_t got = 0;
    while (got < len) {
        ssize_t r = pread(fd, (uint8_t *)buf + got, len - got, (off_t)(off + got));
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) return -1;
        got += (size_t)r;
    }
    return 0;
}

/* Reads len bytes at off, via the cache if small. -1 if it runs past the file (or it shrank). */
static int rd(JFile *f, uint64_t off, size_t len, void *out) {
    if (off > f->size || len > f->size - off) return -1;
    if (len > BS / 4) return pread_all(f->fd, out, len, off);
    uint8_t *o = out;
    while (len) {
        uint64_t b = off / BS;
        Blk *k = &f->blk[b % NB];
        if (!k->valid || k->base != b * BS) {
            if (!k->buf && !(k->buf = malloc(BS))) return -1;
            ssize_t r;
            do r = pread(f->fd, k->buf, BS, (off_t)(b * BS)); while (r < 0 && errno == EINTR);
            if (r <= 0) { k->valid = 0; return -1; }
            k->base = b * BS;
            k->len = (uint32_t)r;
            k->valid = 1;
        }
        uint64_t in = off - k->base;
        if (in >= k->len) return -1;
        size_t n = k->len - in < len ? k->len - in : len;
        memcpy(o, k->buf + in, n);
        o += n; off += n; len -= n;
    }
    return 0;
}

void jf_close(JFile *f) {
    if (!f) return;
    if (f->fd >= 0) close(f->fd);
    for (int i = 0; i < NB; i++) free(f->blk[i].buf);
    free(f->ent);
    free(f->dc);
    free(f);
}

size_t jf_count(const JFile *f) { return f ? f->nent : 0; }
int jf_compact(const JFile *f) { return f && f->compact; }

JFile *jf_open(const char *path) {
    JFile *f = calloc(1, sizeof *f);
    if (!f) return NULL;
    f->fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NOCTTY);
    struct stat st;
    uint8_t h[272];
    if (f->fd < 0 || fstat(f->fd, &st) || !S_ISREG(st.st_mode) || st.st_size < HDR_MIN) goto bad;
    f->size = (uint64_t)st.st_size;
    memset(h, 0, sizeof h);
    if (pread_all(f->fd, h, f->size < sizeof h ? (size_t)f->size : sizeof h, 0) || memcmp(h, "LPKSHHRH", 8)) goto bad;
    uint32_t inc = le32(h + 12);
    if (inc & ~(uint32_t)INC_KNOWN) goto bad;           /* something we don't know: don't guess */
    f->compact = (inc & INC_COMPACT) != 0;
    uint64_t hsize = le64(h + 88), n_entries = le64(h + 152), arr = le64(h + 176);
    if (hsize < HDR_MIN || hsize > f->size) goto bad;
    /* cap: no more entries than fit in the file (64 bytes minimum each) */
    uint64_t cap = f->size / 64;
    if (n_entries > cap) n_entries = cap;
    if (!(f->dc = calloc(DC, sizeof *f->dc))) goto bad;
    if (n_entries && !(f->ent = malloc(n_entries * sizeof *f->ent))) goto bad;
    size_t isz = f->compact ? 4 : 8;
    uint64_t prev = 0;
    int arrays = 0;
    uint8_t *items = NULL;
    while (arr && f->nent < n_entries) {
        uint8_t oh[24];
        if (arr <= prev || (arr & 7) || arr < hsize || ++arrays > 1 << 20 || rd(f, arr, 24, oh) || oh[0] != OBJ_ENTRY_ARRAY) break;
        uint64_t osize = le64(oh + 8);
        if (osize < 24 || osize > f->size - arr || osize > (64u << 20)) break;
        size_t n = (size_t)((osize - 24) / isz);
        uint8_t *ni = realloc(items, n * isz + 1);
        if (!ni) break;
        items = ni;
        if (n && rd(f, arr + 24, n * isz, items)) break;
        int end = 0;
        for (size_t i = 0; i < n && f->nent < n_entries; i++) {
            uint64_t e = f->compact ? le32(items + i * 4) : le64(items + i * 8);
            if (!e) { end = 1; break; }             /* preallocated array: the rest is empty */
            if ((e & 7) || e < hsize || e > f->size - 64) continue;
            f->ent[f->nent++] = e;
        }
        if (end) break;
        prev = arr;
        arr = le64(oh + 16);
    }
    free(items);
    return f;
bad:
    jf_close(f);
    return NULL;
}

static int entry_header(JFile *f, size_t idx, uint8_t eh[64], uint64_t *size) {
    if (!f || idx >= f->nent) return -1;
    uint64_t off = f->ent[idx];
    if (rd(f, off, 64, eh) || eh[0] != OBJ_ENTRY) return -1;
    *size = le64(eh + 8);
    if (*size < 64 || *size > MAX_ENTRY_SIZE || *size > f->size - off) return -1;
    return 0;
}

uint64_t jf_realtime(JFile *f, size_t idx) {
    uint8_t eh[64];
    uint64_t size;
    if (entry_header(f, idx, eh, &size)) return 0;
    return le64(eh + 24);
}

/* ---- LZ4 block ---- */

long lz4_block_decode(const uint8_t *in, size_t len, uint8_t *out, size_t cap) {
    const uint8_t *ip = in, *ie = in + len;
    size_t op = 0;
    while (ip < ie) {
        unsigned tok = *ip++;
        size_t lit = tok >> 4;
        if (lit == 15) {
            unsigned b;
            do { if (ip >= ie) return -1; b = *ip++; lit += b; } while (b == 255 && lit < (1u << 30));
        }
        if (lit > (size_t)(ie - ip) || lit > cap - op) return -1;
        memcpy(out + op, ip, lit);
        ip += lit;
        op += lit;
        if (ip >= ie) break;                 /* the last sequence has only literals */
        if (ie - ip < 2) return -1;
        size_t dist = (size_t)ip[0] | (size_t)ip[1] << 8;
        ip += 2;
        if (!dist || dist > op) return -1;
        size_t ml = tok & 15;
        if (ml == 15) {
            unsigned b;
            do { if (ip >= ie) return -1; b = *ip++; ml += b; } while (b == 255 && ml < (1u << 30));
        }
        ml += 4;
        if (ml > cap - op) return -1;
        for (size_t i = 0; i < ml; i++, op++) out[op] = out[op - dist];   /* may overlap */
    }
    return (long)op;
}

/* ---- fields ---- */

static int field_id(const char *p, size_t n, size_t *eq) {
    static const struct { const char *name; int id; } names[] = {
        { "MESSAGE", F_MSG }, { "PRIORITY", F_PRIO }, { "SYSLOG_IDENTIFIER", F_IDENT }, { "_COMM", F_COMM },
        { "_SYSTEMD_UNIT", F_UNIT }, { "_SYSTEMD_USER_UNIT", F_UUNIT }, { "_PID", F_PID }, { "_UID", F_UID },
        { "_BOOT_ID", F_BOOT }, { "_TRANSPORT", F_TRANSPORT },
    };
    const char *e = memchr(p, '=', n);
    if (!e) return -1;
    *eq = (size_t)(e - p);
    for (size_t i = 0; i < sizeof names / sizeof *names; i++)
        if (strlen(names[i].name) == *eq && !memcmp(p, names[i].name, *eq)) return names[i].id;
    return F_NONE;
}

static void copy_val(char *dst, size_t cap, const char *v, size_t n) {
    if (n >= cap) n = cap - 1;
    for (size_t i = 0; i < n; i++) dst[i] = (v[i] >= 32 && v[i] != 127) || (unsigned char)v[i] >= 128 ? v[i] : '?';
    dst[n] = 0;
}

static int parse_int(const char *s) {
    if (!*s) return -1;
    long v = 0;
    for (; *s; s++) { if (*s < '0' || *s > '9' || v > 100000000) return -1; v = v * 10 + (*s - '0'); }
    return (int)v;
}

static void set_field(JEntry *e, int id, const char *v) {
    switch (id) {
    case F_PRIO: e->priority = v[0] >= '0' && v[0] <= '7' && !v[1] ? v[0] - '0' : e->priority; break;
    case F_IDENT: snprintf(e->ident, sizeof e->ident, "%s", v); break;
    case F_COMM: snprintf(e->comm, sizeof e->comm, "%s", v); break;
    case F_UNIT: if (!e->unit[0]) snprintf(e->unit, sizeof e->unit, "%s", v); break;
    case F_UUNIT: snprintf(e->unit, sizeof e->unit, "%s", v); break;   /* the user unit wins */
    case F_PID: e->pid = parse_int(v); break;
    case F_UID: e->uid = parse_int(v); break;
    case F_BOOT: snprintf(e->boot, sizeof e->boot, "%s", v); break;
    case F_TRANSPORT: e->kernel = !strcmp(v, "kernel"); break;
    }
}

static void set_message(JEntry *e, const char *v, size_t n) {
    int cut = n > JMSG_MAX;
    if (cut) n = JMSG_MAX;
    char *m = malloc(n + 4);
    if (!m) return;
    memcpy(m, v, n);
    if (cut) { memcpy(m + n, "\xe2\x80\xa6", 3); n += 3; }       /* «…» */
    m[n] = 0;
    free(e->message);
    e->message = m;
}

/* A compressed field: it is decompressed whole (capped) and parsed. */
static void compressed_field(JFile *f, JEntry *e, uint64_t poff, uint64_t plen, int flags, int *zstd) {
    if (flags & OF_ZSTD) { (*zstd)++; return; }
    if (plen > MAX_PAYLOAD) return;
    uint8_t *in = malloc(plen ? plen : 1), *out = NULL;
    size_t outlen = 0;
    if (!in || rd(f, poff, plen, in)) { free(in); return; }
    if (flags & OF_XZ) {
        if (xz_decode(in, plen, &out, &outlen, MAX_PAYLOAD)) out = NULL;
    } else if ((flags & OF_LZ4) && plen > 8) {
        uint64_t want = le64(in);
        if (want && want <= MAX_PAYLOAD && (out = malloc(want))) {
            long r = lz4_block_decode(in + 8, plen - 8, out, want);
            if (r < 0) { free(out); out = NULL; } else outlen = (size_t)r;
        }
    }
    free(in);
    if (!out) return;
    size_t eq;
    int id = field_id((const char *)out, outlen, &eq);
    if (id == F_MSG) set_message(e, (const char *)out + eq + 1, outlen - eq - 1);
    else if (id > 0) { char v[96]; copy_val(v, sizeof v, (const char *)out + eq + 1, outlen - eq - 1); set_field(e, id, v); }
    free(out);
}

int jf_entry(JFile *f, size_t idx, JEntry *out) {
    memset(out, 0, sizeof *out);
    out->priority = out->pid = out->uid = -1;
    uint8_t eh[64];
    uint64_t size;
    if (entry_header(f, idx, eh, &size)) return -1;
    uint64_t off = f->ent[idx];
    out->seqnum = le64(eh + 16);
    out->realtime = le64(eh + 24);
    size_t isz = f->compact ? 4 : 16, n = (size_t)((size - 64) / isz);
    if (n > 4096) n = 4096;
    uint8_t *items = malloc(n * isz + 1);
    if (!items || (n && rd(f, off + 64, n * isz, items))) { free(items); return -1; }
    size_t pstart = f->compact ? 72 : 64;
    int zstd = 0;
    for (size_t i = 0; i < n; i++) {
        uint64_t d = f->compact ? le32(items + i * 4) : le64(items + i * 16);
        if (!d || (d & 7) || d > f->size - 16) continue;
        DCache *c = &f->dc[(d >> 3) % DC];
        if (c->off == d) { if (c->id) set_field(out, c->id, c->val); continue; }
        uint8_t dh[16];
        if (rd(f, d, 16, dh) || dh[0] != OBJ_DATA) continue;
        uint64_t osize = le64(dh + 8);
        if (osize < pstart || osize > f->size - d) continue;
        uint64_t plen = osize - pstart, poff = d + pstart;
        int flags = dh[1];
        if (flags & (OF_XZ | OF_LZ4 | OF_ZSTD)) { compressed_field(f, out, poff, plen, flags, &zstd); continue; }
        char head[128];
        size_t hl = plen < sizeof head ? (size_t)plen : sizeof head;
        if (rd(f, poff, hl, head)) continue;
        size_t eq;
        int id = field_id(head, hl, &eq);
        if (id < 0) continue;
        if (id == F_MSG) {
            size_t ml = plen - eq - 1 > JMSG_MAX + 1 ? JMSG_MAX + 1 : (size_t)(plen - eq - 1);
            char *m = malloc(ml + 1);
            if (m && !rd(f, poff + eq + 1, ml, m)) set_message(out, m, ml);
            free(m);
            continue;
        }
        c->off = d;
        c->id = (uint8_t)id;
        c->val[0] = 0;
        if (id) {
            copy_val(c->val, sizeof c->val, head + eq + 1, hl - eq - 1);
            set_field(out, id, c->val);
        }
    }
    free(items);
    if (!out->message) out->message = strdup(zstd ? "[zstd-compressed]" : "");
    return out->message ? 0 : -1;
}

void jentry_free(JEntry *e) { if (e) { free(e->message); e->message = NULL; } }

/* ---- JSON ---- */

typedef struct { char *b; size_t cap, n; int full; } Out;

static void oput(Out *o, const char *s, size_t n) {
    if (o->full || n > o->cap - o->n - 1) { o->full = 1; return; }
    memcpy(o->b + o->n, s, n);
    o->n += n;
}

/* Text as a JSON string: control characters escaped and invalid UTF-8 → U+FFFD. */
static void oesc(Out *o, const char *s, size_t max) {
    const unsigned char *p = (const unsigned char *)s;
    size_t used = 0;
    while (*p && used < max) {
        unsigned c = *p;
        size_t len = c < 0x80 ? 1 : (c & 0xe0) == 0xc0 ? 2 : (c & 0xf0) == 0xe0 ? 3 : (c & 0xf8) == 0xf0 ? 4 : 0;
        int ok = len > 0 && (len == 1 || c >= 0xc2) && !(c > 0xf4);
        for (size_t k = 1; ok && k < len; k++) if ((p[k] & 0xc0) != 0x80) ok = 0;
        if (ok && len == 3 && ((c == 0xe0 && p[1] < 0xa0) || (c == 0xed && p[1] >= 0xa0))) ok = 0;
        if (ok && len == 4 && ((c == 0xf0 && p[1] < 0x90) || (c == 0xf4 && p[1] >= 0x90))) ok = 0;
        if (!ok) { oput(o, "\xef\xbf\xbd", 3); p++; used++; continue; }
        if (len > 1) { oput(o, (const char *)p, len); p += len; used += len; continue; }
        char tmp[8];
        if (c == '"' || c == '\\') { tmp[0] = '\\'; tmp[1] = (char)c; oput(o, tmp, 2); }
        else if (c == '\n') oput(o, "\\n", 2);
        else if (c == '\t') oput(o, "\\t", 2);
        else if (c < 32 || c == 127) { snprintf(tmp, sizeof tmp, "\\u%04x", c); oput(o, tmp, 6); }
        else oput(o, (const char *)p, 1);
        p++;
        used++;
    }
    if (*p) oput(o, "\xe2\x80\xa6", 3);
}

size_t jentry_json(const JEntry *e, char *buf, size_t cap, size_t msgmax) {
    if (cap < 2) return 0;
    Out o = { buf, cap, 0, 0 };
    char tmp[96];
    int n = snprintf(tmp, sizeof tmp, "{\"t\":%llu,\"s\":%llu,\"p\":%d,\"pid\":%d,\"uid\":%d,\"k\":%d,\"id\":\"",
                     (unsigned long long)e->realtime, (unsigned long long)e->seqnum, e->priority, e->pid, e->uid, e->kernel);
    oput(&o, tmp, (size_t)n);
    oesc(&o, e->ident[0] ? e->ident : e->comm, sizeof e->ident);
    oput(&o, "\",\"u\":\"", 7);
    oesc(&o, e->unit, sizeof e->unit);
    oput(&o, "\",\"b\":\"", 7);
    oesc(&o, e->boot, 8);
    oput(&o, "\",\"m\":\"", 7);
    oesc(&o, e->message ? e->message : "", msgmax);
    oput(&o, "\"}", 2);
    if (o.full) return 0;
    buf[o.n] = 0;
    return o.n;
}

/* ---- queries ---- */

int jq_valid_match(const char *s) {
    size_t n = strlen(s);
    if (!n || n > 95) return 0;
    for (; *s; s++) if (!isalnum((unsigned char)*s) && !strchr("@._:-", *s)) return 0;
    return 1;
}

static int valid_grep(const char *s) {
    size_t n = strlen(s);
    if (n > 127) return 0;
    for (; *s; s++) if ((unsigned char)*s < 32 || *s == 127) return 0;
    return 1;
}

static int parse_u64(const char *s, uint64_t *out) {
    if (!*s || strlen(s) > 19) return -1;
    uint64_t v = 0;
    for (; *s; s++) { if (*s < '0' || *s > '9') return -1; v = v * 10 + (uint64_t)(*s - '0'); }
    *out = v;
    return 0;
}

int jq_parse(const char *text, JQuery *q, size_t max_limit) {
    memset(q, 0, sizeof *q);
    q->prio = -1;
    q->limit = max_limit < 1000 ? max_limit : 1000;
    char line[256];
    const char *p = text;
    while (*p) {
        const char *nl = strchr(p, '\n');
        size_t n = nl ? (size_t)(nl - p) : strlen(p);
        if (n >= sizeof line) return -1;
        memcpy(line, p, n);
        line[n] = 0;
        p += n + (nl ? 1 : 0);
        if (!n) continue;
        char *eq = strchr(line, '=');
        if (!eq) return -1;
        *eq = 0;
        const char *k = line, *v = eq + 1;
        uint64_t u;
        if (!strcmp(k, "since") || !strcmp(k, "until")) {
            if (parse_u64(v, &u)) return -1;
            if (u && u < 1000000000000ull) u *= 1000000ull;       /* seconds → µs */
            if (!strcmp(k, "since")) q->since = u; else q->until = u;
        } else if (!strcmp(k, "prio")) {
            if (strlen(v) != 1 || v[0] < '0' || v[0] > '7') return -1;
            q->prio = v[0] - '0';
        } else if (!strcmp(k, "kernel")) {
            if (strcmp(v, "0") && strcmp(v, "1")) return -1;
            q->kernel = v[0] == '1';
        } else if (!strcmp(k, "match")) {
            if (*v && !jq_valid_match(v)) return -1;
            snprintf(q->match, sizeof q->match, "%s", v);
        } else if (!strcmp(k, "grep")) {
            if (!valid_grep(v)) return -1;
            snprintf(q->grep, sizeof q->grep, "%s", v);
        } else if (!strcmp(k, "limit")) {
            if (parse_u64(v, &u) || !u || u > max_limit) return -1;
            q->limit = (size_t)u;
        } else return -1;
    }
    return 0;
}

static int matches(const JQuery *q, const JEntry *e) {
    if (q->prio >= 0 && (e->priority < 0 || e->priority > q->prio)) return 0;   /* no priority: no match */
    if (q->kernel && !e->kernel) return 0;
    if (q->match[0]) {
        size_t ml = strlen(q->match);
        int ok = !strcmp(e->ident, q->match) || !strcmp(e->comm, q->match) || !strcmp(e->unit, q->match) ||
                 (!strncmp(e->unit, q->match, ml) && !strcmp(e->unit + ml, ".service"));
        if (!ok) return 0;
    }
    if (q->grep[0] && !strcasestr(e->message ? e->message : "", q->grep)) return 0;
    return 1;
}

static int cmp_entry(const void *a, const void *b) {
    const JEntry *x = a, *y = b;
    if (x->realtime != y->realtime) return x->realtime < y->realtime ? -1 : 1;
    return x->seqnum < y->seqnum ? -1 : x->seqnum > y->seqnum;
}

void jq_free(JEntry *e, long n) {
    for (long i = 0; i < n; i++) free(e[i].message);
    free(e);
}

long jq_run(char *const *paths, int npaths, const JQuery *q, JEntry **out) {
    size_t limit = q->limit ? q->limit : 1000, n = 0, cap = 0;
    JEntry *all = NULL;
    for (int p = 0; p < npaths; p++) {
        JFile *f = jf_open(paths[p]);
        if (!f) continue;
        size_t found = 0;
        for (size_t i = f->nent; i-- > 0 && found < limit;) {
            uint64_t rt = jf_realtime(f, i);
            if (!rt) continue;
            if (q->until && rt >= q->until) continue;
            if (q->since && rt <= q->since) break;
            JEntry e;
            if (jf_entry(f, i, &e)) continue;
            if (!matches(q, &e)) { jentry_free(&e); continue; }
            if (n == cap) {
                size_t nc = cap ? cap * 2 : 256;
                JEntry *na = realloc(all, nc * sizeof *na);
                if (!na) { jentry_free(&e); break; }
                all = na;
                cap = nc;
            }
            all[n++] = e;
            found++;
        }
        jf_close(f);
    }
    if (n > 1) qsort(all, n, sizeof *all, cmp_entry);
    if (n > limit) {                         /* the newest ones */
        for (size_t i = 0; i < n - limit; i++) free(all[i].message);
        memmove(all, all + (n - limit), limit * sizeof *all);
        n = limit;
    }
    *out = all;
    return (long)n;
}

/* ---- files ---- */

static int name_ok(const char *name, const char *prefix) {
    size_t pl = strlen(prefix), nl = strlen(name);
    if (strncmp(name, prefix, pl)) return 0;
    const char *rest = name + pl;
    if (strcmp(rest, ".journal") && *rest != '@') return 0;
    return (nl > 8 && !strcmp(name + nl - 8, ".journal")) || (nl > 9 && !strcmp(name + nl - 9, ".journal~"));
}

int jq_files(int scope, uid_t uid, char ***paths) {
    *paths = NULL;
    char mid[64] = "";
    FILE *m = fopen("/etc/machine-id", "r");
    if (m) { if (!fgets(mid, sizeof mid, m)) mid[0] = 0; fclose(m); }
    mid[strcspn(mid, "\n")] = 0;
    if (strlen(mid) != 32) return 0;
    for (const char *c = mid; *c; c++) if (!isxdigit((unsigned char)*c)) return 0;
    char prefix[32];
    if (scope == JSCOPE_USER) snprintf(prefix, sizeof prefix, "user-%u", (unsigned)uid);
    else snprintf(prefix, sizeof prefix, "system");
    const char *roots[] = { "/var/log/journal", "/run/log/journal" };
    int n = 0, cap = 0;
    char **list = NULL;
    for (int r = 0; r < 2; r++) {
        char dir[128];
        snprintf(dir, sizeof dir, "%s/%s", roots[r], mid);
        DIR *d = opendir(dir);
        if (!d) continue;
        struct dirent *de;
        while ((de = readdir(d))) {
            if (!name_ok(de->d_name, prefix)) continue;
            char path[512];
            snprintf(path, sizeof path, "%s/%s", dir, de->d_name);
            struct stat st;
            if (lstat(path, &st) || !S_ISREG(st.st_mode) || access(path, R_OK)) continue;
            if (n == cap) {
                int nc = cap ? cap * 2 : 16;
                char **nl = realloc(list, (size_t)nc * sizeof *nl);
                if (!nl) break;
                list = nl;
                cap = nc;
            }
            if (!(list[n] = strdup(path))) break;
            n++;
            if (n >= 512) break;
        }
        closedir(d);
    }
    *paths = list;
    return n;
}

void jq_files_free(char **paths, int n) {
    for (int i = 0; i < n; i++) free(paths[i]);
    free(paths);
}
