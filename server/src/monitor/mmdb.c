/* mmdb.c — our own reader for the MaxMind DB 2.0 format (https://maxmind.github.io/MaxMind-DB/),
 * only what's needed to look up country.iso_code. It works both for LordOS's small file
 * (assets/monitor/geoip-country.mmdb, built by tools/gen-monitor-data.py) and for the original
 * dbip-country-lite.mmdb.
 *
 * Every buffer access goes through need(): nothing is ever read outside the file, decoder
 * pointers are followed only once (the format doesn't allow pointers to pointers) and the
 * depth of maps/arrays is bounded. */
#include "mmdb.h"
#include "pkg/xz.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const uint8_t MARKER[] = { 0xAB, 0xCD, 0xEF, 'M', 'a', 'x', 'M', 'i', 'n', 'd', '.', 'c', 'o', 'm' };

enum { T_EXT = 0, T_PTR = 1, T_STR = 2, T_DBL = 3, T_BYTES = 4, T_U16 = 5, T_U32 = 6, T_MAP = 7,
       T_I32 = 8, T_U64 = 9, T_U128 = 10, T_ARR = 11, T_CONT = 12, T_END = 13, T_BOOL = 14, T_FLT = 15 };

/* A decodable section: [base, base+len) inside the file. */
typedef struct { const uint8_t *p; size_t len; } Sec;

static int need(const Sec *s, size_t off, size_t n) { return off <= s->len && n <= s->len - off; }

/* Reads the control byte at *off: type and size (or the target, if it's a pointer). */
static int ctrl(const Sec *s, size_t *off, int *type, uint32_t *size) {
    if (!need(s, *off, 1)) return -1;
    uint8_t c = s->p[(*off)++];
    int t = c >> 5;
    if (t == T_PTR) {
        int ss = (c >> 3) & 3;
        uint32_t v = c & 7, p;
        if (!need(s, *off, (size_t)ss + 1)) return -1;
        const uint8_t *b = s->p + *off;
        if (ss == 0) p = v << 8 | b[0];
        else if (ss == 1) p = (v << 16 | (uint32_t)b[0] << 8 | b[1]) + 2048;
        else if (ss == 2) p = (v << 24 | (uint32_t)b[0] << 16 | (uint32_t)b[1] << 8 | b[2]) + 526336;
        else p = (uint32_t)b[0] << 24 | (uint32_t)b[1] << 16 | (uint32_t)b[2] << 8 | b[3];
        *off += (size_t)ss + 1;
        *type = T_PTR;
        *size = p;
        return 0;
    }
    uint32_t sz = c & 0x1f;
    if (t == T_EXT) {
        if (!need(s, *off, 1)) return -1;
        t = 7 + s->p[(*off)++];
        if (t < 8 || t > 15) return -1;
    }
    if (sz >= 29) {
        int extra = (int)sz - 28;
        if (!need(s, *off, (size_t)extra)) return -1;
        const uint8_t *b = s->p + *off;
        if (sz == 29) sz = 29 + b[0];
        else if (sz == 30) sz = 285 + ((uint32_t)b[0] << 8 | b[1]);
        else sz = 65821 + ((uint32_t)b[0] << 16 | (uint32_t)b[1] << 8 | b[2]);
        *off += (size_t)extra;
    }
    *type = t;
    *size = sz;
    return 0;
}

/* If the value at *off is a pointer, *off moves to its target (only once). */
static int resolve(const Sec *s, size_t *off, int *type, uint32_t *size) {
    if (ctrl(s, off, type, size)) return -1;
    if (*type != T_PTR) return 0;
    size_t dst = *size;
    if (ctrl(s, &dst, type, size) || *type == T_PTR) return -1;
    *off = dst;
    return 0;
}

static int skip(const Sec *s, size_t *off, int depth);

/* Skips the body of a value whose type/size were already read. */
static int skip_body(const Sec *s, size_t *off, int type, uint32_t size, int depth) {
    if (depth > 16) return -1;
    switch (type) {
    case T_PTR: case T_BOOL: return 0;
    case T_DBL: size = 8; break;
    case T_FLT: size = 4; break;
    case T_STR: case T_BYTES: case T_U16: case T_U32: case T_I32: case T_U64: case T_U128: break;
    case T_MAP:
        for (uint32_t i = 0; i < size; i++) if (skip(s, off, depth + 1) || skip(s, off, depth + 1)) return -1;
        return 0;
    case T_ARR:
        for (uint32_t i = 0; i < size; i++) if (skip(s, off, depth + 1)) return -1;
        return 0;
    default: return -1;
    }
    if (!need(s, *off, size)) return -1;
    *off += size;
    return 0;
}

static int skip(const Sec *s, size_t *off, int depth) {
    int t;
    uint32_t sz;
    if (ctrl(s, off, &t, &sz)) return -1;
    return skip_body(s, off, t, sz, depth);
}

/* Scalar value at *off (following a pointer): *body and *n are its bytes, and *off moves to the
 * next value. */
static int scalar(const Sec *s, size_t *off, int *type, size_t *body, uint32_t *n) {
    size_t at = *off;
    if (ctrl(s, &at, type, n)) return -1;
    size_t next = at + *n;
    if (*type == T_PTR) {
        next = at;
        at = *n;
        if (ctrl(s, &at, type, n) || *type == T_PTR) return -1;
    }
    if (!need(s, at, *n)) return -1;
    *body = at;
    *off = next;
    return 0;
}

/* Reads a string into out (NUL-terminated, truncated to n-1). */
static int read_str(const Sec *s, size_t *off, char *out, size_t n) {
    int t;
    size_t body;
    uint32_t sz;
    if (scalar(s, off, &t, &body, &sz) || t != T_STR) return -1;
    size_t k = sz < n ? sz : n - 1;
    memcpy(out, s->p + body, k);
    out[k] = 0;
    return 0;
}

static int read_uint(const Sec *s, size_t *off, uint64_t *v) {
    int t;
    size_t body;
    uint32_t sz;
    if (scalar(s, off, &t, &body, &sz) || (t != T_U16 && t != T_U32 && t != T_U64 && t != T_U128) || sz > 8) return -1;
    uint64_t x = 0;
    for (uint32_t i = 0; i < sz; i++) x = x << 8 | s->p[body + i];
    *v = x;
    return 0;
}

/* Looks up the key in the map starting at off; leaves *val at its value. 1 = found. */
static int map_find(const Sec *s, size_t off, const char *key, size_t *val) {
    int t;
    uint32_t n;
    if (resolve(s, &off, &t, &n) || t != T_MAP) return -1;
    for (uint32_t i = 0; i < n; i++) {
        char k[64];
        if (read_str(s, &off, k, sizeof k)) return -1;
        if (!strcmp(k, key)) { *val = off; return 1; }
        if (skip(s, &off, 0)) return -1;
    }
    return 0;
}

static uint32_t record(const Mmdb *m, uint32_t node, int bit) {
    const uint8_t *b = m->buf + (size_t)node * (m->record_size / 4);
    if (m->record_size == 24) {
        b += bit ? 3 : 0;
        return (uint32_t)b[0] << 16 | (uint32_t)b[1] << 8 | b[2];
    }
    if (m->record_size == 28) {
        if (!bit) return ((uint32_t)(b[3] & 0xf0) << 20) | (uint32_t)b[0] << 16 | (uint32_t)b[1] << 8 | b[2];
        return ((uint32_t)(b[3] & 0x0f) << 24) | (uint32_t)b[4] << 16 | (uint32_t)b[5] << 8 | b[6];
    }
    b += bit ? 4 : 0;
    return (uint32_t)b[0] << 24 | (uint32_t)b[1] << 16 | (uint32_t)b[2] << 8 | b[3];
}

int mmdb_open_mem(Mmdb *m, const uint8_t *buf, size_t len) {
    memset(m, 0, sizeof *m);
    m->buf = (uint8_t *)buf;
    m->len = len;
    if (len < sizeof MARKER + 1) return -1;
    /* the metadata marker is in the last 128 KiB; the last one is taken */
    size_t lo = len > 131072 ? len - 131072 : 0, found = (size_t)-1;
    for (size_t i = len - sizeof MARKER + 1; i-- > lo;)
        if (buf[i] == MARKER[0] && !memcmp(buf + i, MARKER, sizeof MARKER)) { found = i; break; }
    if (found == (size_t)-1) return -1;
    Sec meta = { buf + found + sizeof MARKER, len - found - sizeof MARKER };
    size_t v;
    uint64_t x;
    if (map_find(&meta, 0, "node_count", &v) != 1 || read_uint(&meta, &v, &x) || x == 0 || x > 0xffffffffu) return -1;
    m->node_count = (uint32_t)x;
    if (map_find(&meta, 0, "record_size", &v) != 1 || read_uint(&meta, &v, &x) || (x != 24 && x != 28 && x != 32)) return -1;
    m->record_size = (uint32_t)x;
    if (map_find(&meta, 0, "ip_version", &v) != 1 || read_uint(&meta, &v, &x) || (x != 4 && x != 6)) return -1;
    m->ip_version = (uint32_t)x;
    if (map_find(&meta, 0, "build_epoch", &v) == 1 && !read_uint(&meta, &v, &x)) m->build_epoch = x;
    if (map_find(&meta, 0, "database_type", &v) == 1) read_str(&meta, &v, m->type, sizeof m->type);
    uint64_t tree = (uint64_t)m->node_count * m->record_size / 4;
    if (tree + 16 > found) return -1;
    m->tree_size = (size_t)tree;
    m->data_off = m->tree_size + 16;
    m->data_len = found - m->data_off;
    /* node of ::/96 (IPv4 inside an IPv6 tree): 96 zero bits */
    m->ipv4_start = 0;
    if (m->ip_version == 6) {
        uint32_t node = 0;
        for (int i = 0; i < 96 && node < m->node_count; i++) node = record(m, node, 0);
        m->ipv4_start = node;
    }
    return 0;
}

int mmdb_open(Mmdb *m, const char *path) {
    memset(m, 0, sizeof *m);
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    uint8_t *buf = NULL;
    long len = -1;
    if (!fseek(f, 0, SEEK_END)) len = ftell(f);
    int ok = len > 0 && (unsigned long)len <= MMDB_MAX_FILE && !fseek(f, 0, SEEK_SET) && (buf = malloc((size_t)len)) &&
             fread(buf, 1, (size_t)len, f) == (size_t)len;
    fclose(f);
    size_t n = ok ? (size_t)len : 0;
    /* .mmdb.xz (LordOS's): decompressed in memory with our own xz (src/pkg/xz.c) */
    size_t pl = strlen(path);
    if (ok && pl > 3 && !strcmp(path + pl - 3, ".xz")) {
        uint8_t *raw = NULL;
        size_t rlen = 0;
        ok = !xz_decode(buf, n, &raw, &rlen, MMDB_MAX_FILE);
        free(buf);
        buf = ok ? raw : NULL;
        n = rlen;
    }
    if (ok && !mmdb_open_mem(m, buf, n)) {
        m->owned = 1;
        return 0;
    }
    free(buf);
    memset(m, 0, sizeof *m);
    return -1;
}

void mmdb_close(Mmdb *m) {
    if (m->owned) free(m->buf);
    memset(m, 0, sizeof *m);
}

int mmdb_country(const Mmdb *m, const uint8_t *addr, int family, char cc[3]) {
    cc[0] = 0;
    if (!m || !m->buf || !addr || (family != 4 && family != 6)) return -1;
    if (family == 6 && m->ip_version == 4) return 0;
    int bits = family == 4 ? 32 : 128;
    uint32_t node = family == 4 && m->ip_version == 6 ? m->ipv4_start : 0;
    for (int i = 0; i < bits && node < m->node_count; i++) {
        if ((size_t)(node + 1) * (m->record_size / 4) > m->tree_size) return -1;
        node = record(m, node, (addr[i >> 3] >> (7 - (i & 7))) & 1);
    }
    if (node <= m->node_count) return 0;                     /* == node_count: no data */
    uint64_t off = (uint64_t)node - m->node_count - 16;
    if (off >= m->data_len) return -1;
    Sec data = { m->buf + m->data_off, m->data_len };
    size_t v, w;
    if (map_find(&data, (size_t)off, "country", &v) != 1) return 0;
    if (map_find(&data, v, "iso_code", &w) != 1) return 0;
    char s[8];
    if (read_str(&data, &w, s, sizeof s) || strlen(s) != 2) return 0;
    cc[0] = s[0];
    cc[1] = s[1];
    cc[2] = 0;
    return 1;
}
