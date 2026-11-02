/* vici.c — our own client for strongSwan's VICI protocol (the one swanctl uses), root side.
 *
 * We use it instead of swanctl because Debian's AppArmor profile for swanctl only lets it
 * talk to /run/charon.vici and read /etc/swanctl: with our own VICI, keys and certificates
 * travel over the socket and no program has to read our files.
 *
 * Packet: length (4 bytes, big endian) + type (1 byte) + [name (1 length byte + text)] +
 * message. Message: elements SECTION_START(name) / SECTION_END / KEY_VALUE(key, value with a
 * 2-byte length) / LIST_START(name) / LIST_ITEM(value) / LIST_END. */
#include "vici.h"

#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

enum { CMD_REQUEST, CMD_RESPONSE, CMD_UNKNOWN, EVENT_REGISTER, EVENT_UNREGISTER, EVENT_CONFIRM, EVENT_UNKNOWN, EVENT };
enum { SECTION_START = 1, SECTION_END, KEY_VALUE, LIST_START, LIST_ITEM, LIST_END };

int vici_connect(const char *path) {
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return -1;
    struct sockaddr_un su = { .sun_family = AF_UNIX };
    snprintf(su.sun_path, sizeof su.sun_path, "%s", path);
    if (connect(fd, (struct sockaddr *)&su, sizeof su)) { close(fd); return -1; }
    return fd;
}

/* ---- building ---- */

static void put(ViciMsg *m, const void *p, size_t n) {
    if (m->oom) return;
    if (m->len + n > m->cap) {
        size_t cap = m->cap ? m->cap * 2 : 1024;
        while (cap < m->len + n) cap *= 2;
        uint8_t *q = realloc(m->p, cap);
        if (!q) { m->oom = 1; return; }
        m->p = q;
        m->cap = cap;
    }
    memcpy(m->p + m->len, p, n);
    m->len += n;
}

static void put_name(ViciMsg *m, uint8_t type, const char *name) {
    uint8_t h[2] = { type, (uint8_t)strlen(name) };
    put(m, h, 2);
    put(m, name, strlen(name));
}

void vici_begin(ViciMsg *m, const char *name) { put_name(m, SECTION_START, name); }
void vici_end(ViciMsg *m) { uint8_t t = SECTION_END; put(m, &t, 1); }
void vici_kv_bin(ViciMsg *m, const char *key, const void *val, size_t n) {
    if (n > 65535) { m->oom = 1; return; }
    put_name(m, KEY_VALUE, key);
    uint8_t l[2] = { (uint8_t)(n >> 8), (uint8_t)n };
    put(m, l, 2);
    put(m, val, n);
}
void vici_kv(ViciMsg *m, const char *key, const char *val) { vici_kv_bin(m, key, val, strlen(val)); }
void vici_list_begin(ViciMsg *m, const char *name) { put_name(m, LIST_START, name); }
void vici_item(ViciMsg *m, const char *val) {
    size_t n = strlen(val);
    uint8_t h[3] = { LIST_ITEM, (uint8_t)(n >> 8), (uint8_t)n };
    put(m, h, 3);
    put(m, val, n);
}
void vici_list_end(ViciMsg *m) { uint8_t t = LIST_END; put(m, &t, 1); }
void vici_free(ViciMsg *m) { if (m->p) memset(m->p, 0, m->len); free(m->p); memset(m, 0, sizeof *m); }

/* ---- sending and receiving ---- */

static int send_packet(int fd, uint8_t type, const char *name, const ViciMsg *msg) {
    size_t nl = name ? strlen(name) : 0, body = 1 + (name ? 1 + nl : 0) + (msg ? msg->len : 0);
    uint8_t *p = malloc(4 + body);
    if (!p) return -1;
    p[0] = (uint8_t)(body >> 24); p[1] = (uint8_t)(body >> 16); p[2] = (uint8_t)(body >> 8); p[3] = (uint8_t)body;
    size_t o = 4;
    p[o++] = type;
    if (name) { p[o++] = (uint8_t)nl; memcpy(p + o, name, nl); o += nl; }
    if (msg && msg->len) { memcpy(p + o, msg->p, msg->len); o += msg->len; }
    int ok = 1;
    for (size_t w = 0; w < o;) {
        ssize_t r = write(fd, p + w, o - w);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) { ok = 0; break; }
        w += (size_t)r;
    }
    memset(p, 0, o);
    free(p);
    return ok ? 0 : -1;
}

static int read_full(int fd, uint8_t *p, size_t n, int timeout_ms) {
    size_t got = 0;
    while (got < n) {
        struct pollfd pf = { fd, POLLIN, 0 };
        int pr = poll(&pf, 1, timeout_ms);
        if (pr <= 0) return -1;
        ssize_t r = read(fd, p + got, n - got);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) return -1;
        got += (size_t)r;
    }
    return 0;
}

/* One packet: returns its type, and the rest in body and blen (malloc). */
static int recv_packet(int fd, uint8_t **body, size_t *blen, int timeout_ms) {
    uint8_t h[4];
    if (read_full(fd, h, 4, timeout_ms)) return -1;
    size_t n = (size_t)h[0] << 24 | (size_t)h[1] << 16 | (size_t)h[2] << 8 | h[3];
    if (!n || n > 512 * 1024) return -1;
    uint8_t *p = malloc(n);
    if (!p || read_full(fd, p, n, timeout_ms)) { free(p); return -1; }
    *body = p;
    *blen = n;
    return p[0];
}

/* Walks a message and calls fn(current section, key or list name, value) for each
 * KEY_VALUE and LIST_ITEM. The section is the path "a/b/c" (sections separated by slashes). */
void vici_walk(const uint8_t *m, size_t n, ViciFn fn, void *ud) {
    char path[512] = "", list[256] = "";
    size_t i = 0;
    while (i < n) {
        uint8_t t = m[i++];
        if (t == SECTION_START || t == LIST_START || t == KEY_VALUE) {
            if (i >= n) return;
            uint8_t l = m[i++];
            if (i + l > n) return;
            char name[256];
            memcpy(name, m + i, l);
            name[l] = 0;
            i += l;
            if (t == SECTION_START) {
                size_t pl = strlen(path);
                snprintf(path + pl, sizeof path - pl, "%s%s", pl ? "/" : "", name);
            } else if (t == LIST_START) snprintf(list, sizeof list, "%s", name);
            else {
                if (i + 2 > n) return;
                size_t vl = (size_t)m[i] << 8 | m[i + 1];
                i += 2;
                if (i + vl > n) return;
                char *v = malloc(vl + 1);
                if (!v) return;
                memcpy(v, m + i, vl);
                v[vl] = 0;
                i += vl;
                fn(path, name, v, ud);
                free(v);
            }
        } else if (t == SECTION_END) {
            char *s = strrchr(path, '/');
            if (s) *s = 0; else path[0] = 0;
        } else if (t == LIST_ITEM) {
            if (i + 2 > n) return;
            size_t vl = (size_t)m[i] << 8 | m[i + 1];
            i += 2;
            if (i + vl > n) return;
            char *v = malloc(vl + 1);
            if (!v) return;
            memcpy(v, m + i, vl);
            v[vl] = 0;
            i += vl;
            fn(path, list, v, ud);
            free(v);
        } else if (t == LIST_END) list[0] = 0;
        else return;
    }
}

static int event_register(int fd, const char *event, int reg) {
    if (send_packet(fd, reg ? EVENT_REGISTER : EVENT_UNREGISTER, event, NULL)) return -1;
    for (;;) {
        uint8_t *b;
        size_t n;
        int t = recv_packet(fd, &b, &n, 5000);
        if (t < 0) return -1;
        free(b);
        if (t == EVENT_CONFIRM) return 0;
        if (t == EVENT_UNKNOWN) return -1;
    }
}

int vici_call(int fd, const char *cmd, const ViciMsg *req, const char *event, ViciFn on_event, ViciFn on_reply, void *ud, int timeout_ms) {
    if (event && event_register(fd, event, 1)) return -1;
    int rc = -1;
    if (!send_packet(fd, CMD_REQUEST, cmd, req)) {
        for (;;) {
            uint8_t *b;
            size_t n;
            int t = recv_packet(fd, &b, &n, timeout_ms);
            if (t < 0) break;
            if (t == EVENT && n > 1) {
                size_t nl = b[1];
                if (2 + nl <= n && on_event) vici_walk(b + 2 + nl, n - 2 - nl, on_event, ud);
            } else if (t == CMD_RESPONSE) {
                if (on_reply) vici_walk(b + 1, n - 1, on_reply, ud);
                rc = 0;
                free(b);
                break;
            } else if (t == CMD_UNKNOWN) { free(b); break; }
            free(b);
        }
    }
    if (event) event_register(fd, event, 0);
    return rc;
}

/* ---- simple reply: success / errmsg ---- */

static void ok_cb(const char *path, const char *key, const char *val, void *ud) {
    ViciResult *r = ud;
    if (path[0]) return;
    if (!strcmp(key, "success")) r->success = !strcmp(val, "yes");
    else if (!strcmp(key, "errmsg")) snprintf(r->errmsg, sizeof r->errmsg, "%s", val);
}

int vici_simple(int fd, const char *cmd, const ViciMsg *req, ViciResult *res, int timeout_ms) {
    memset(res, 0, sizeof *res);
    if (vici_call(fd, cmd, req, NULL, NULL, ok_cb, res, timeout_ms)) { snprintf(res->errmsg, sizeof res->errmsg, "no answer from charon"); return -1; }
    return res->success ? 0 : -1;
}

void vici_result_cb(const char *path, const char *key, const char *val, void *ud) { ok_cb(path, key, val, ud); }
