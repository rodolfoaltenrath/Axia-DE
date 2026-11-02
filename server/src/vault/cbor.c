/* cbor.c — strict CBOR for CTAP2 (see cbor.h). */
#include "cbor.h"

#include <string.h>

/* ================= writer ================= */

void cbw_init(CbW *w, uint8_t *buf, size_t cap) { w->p = buf; w->n = 0; w->cap = cap; w->err = 0; }

void cbw_raw(CbW *w, const void *d, size_t n) {
    if (w->err || n > w->cap - w->n) { w->err = 1; return; }
    memcpy(w->p + w->n, d, n);
    w->n += n;
}

static void head(CbW *w, int major, uint64_t v) {
    uint8_t b[9];
    size_t n;
    uint8_t m = (uint8_t)(major << 5);
    if (v < 24) { b[0] = m | (uint8_t)v; n = 1; }
    else if (v <= 0xff) { b[0] = m | 24; b[1] = (uint8_t)v; n = 2; }
    else if (v <= 0xffff) { b[0] = m | 25; b[1] = (uint8_t)(v >> 8); b[2] = (uint8_t)v; n = 3; }
    else if (v <= 0xffffffffu) { b[0] = m | 26; for (int i = 0; i < 4; i++) b[1 + i] = (uint8_t)(v >> (24 - 8 * i)); n = 5; }
    else { b[0] = m | 27; for (int i = 0; i < 8; i++) b[1 + i] = (uint8_t)(v >> (56 - 8 * i)); n = 9; }
    cbw_raw(w, b, n);
}

void cbw_uint(CbW *w, uint64_t v) { head(w, 0, v); }
void cbw_int(CbW *w, int64_t v) { if (v >= 0) head(w, 0, (uint64_t)v); else head(w, 1, (uint64_t)(-1 - v)); }
void cbw_bytes(CbW *w, const void *d, size_t n) { head(w, 2, n); cbw_raw(w, d, n); }
void cbw_textn(CbW *w, const char *s, size_t n) { head(w, 3, n); cbw_raw(w, s, n); }
void cbw_text(CbW *w, const char *s) { cbw_textn(w, s, strlen(s)); }
void cbw_array(CbW *w, size_t n) { head(w, 4, n); }
void cbw_map(CbW *w, size_t n) { head(w, 5, n); }
void cbw_bool(CbW *w, int v) { uint8_t b = v ? 0xf5 : 0xf4; cbw_raw(w, &b, 1); }
void cbw_null(CbW *w) { uint8_t b = 0xf6; cbw_raw(w, &b, 1); }

/* ================= reader ================= */

typedef struct { const uint8_t *d; size_t len, p; CbItem *it; int n, max; } R;

static int utf8_ok(const uint8_t *s, size_t n) {
    for (size_t i = 0; i < n;) {
        uint8_t c = s[i];
        if (c < 0x80) { i++; continue; }
        size_t k;
        uint32_t cp;
        if (c >= 0xc2 && c <= 0xdf) { k = 1; cp = c & 0x1f; }
        else if (c >= 0xe0 && c <= 0xef) { k = 2; cp = c & 0x0f; }
        else if (c >= 0xf0 && c <= 0xf4) { k = 3; cp = c & 0x07; }
        else return 0;
        for (size_t j = 1; j <= k; j++) { if (i + j >= n || (s[i + j] & 0xc0) != 0x80) return 0; cp = cp << 6 | (s[i + j] & 0x3f); }
        if ((k == 2 && cp < 0x800) || (k == 3 && (cp < 0x10000 || cp > 0x10ffff)) || (cp >= 0xd800 && cp < 0xe000)) return 0;
        i += k + 1;
    }
    return 1;
}

static int item(R *r, int depth) {
    if (depth > 16 || r->n >= r->max || r->p >= r->len) return -1;
    uint8_t ib = r->d[r->p++];
    int major = ib >> 5, ai = ib & 31;
    uint64_t v;
    if (ai < 24) v = (uint64_t)ai;
    else if (ai <= 27) {
        size_t k = (size_t)1 << (ai - 24);
        if (k > r->len - r->p) return -1;
        v = 0;
        for (size_t i = 0; i < k; i++) v = v << 8 | r->d[r->p + i];
        r->p += k;
        /* minimal (canonical) encoding: if it fit in fewer bytes, it is invalid */
        if ((ai == 24 && v < 24) || (ai == 25 && v <= 0xff) || (ai == 26 && v <= 0xffff) || (ai == 27 && v <= 0xffffffffu)) return -1;
    } else return -1;                                  /* 28-30 reserved, 31 indefinite */
    int k = r->n++;
    CbItem *it = &r->it[k];
    memset(it, 0, sizeof *it);
    switch (major) {
    case 0: it->type = CB_UINT; it->v = v; break;
    case 1: it->type = CB_NINT; it->v = v; break;
    case 2: case 3:
        if (v > r->len - r->p) return -1;
        it->type = major == 2 ? CB_BYTES : CB_TEXT;
        it->ptr = r->d + r->p;
        it->len = (size_t)v;
        r->p += (size_t)v;
        if (major == 3 && !utf8_ok(it->ptr, it->len)) return -1;
        break;
    case 4: case 5: {
        it->type = major == 4 ? CB_ARRAY : CB_MAP;
        it->v = v;
        uint64_t sub = major == 4 ? v : v * 2;
        if (v > (uint64_t)r->max || sub > r->len - r->p) return -1;   /* each item takes >= 1 byte */
        for (uint64_t i = 0; i < sub; i++) if (item(r, depth + 1)) return -1;
        break;
    }
    case 6: return -1;                                 /* tags: not in CTAP2 */
    default:
        if (ai >= 24) return -1;                       /* floats and 1-byte simple values */
        if (v == 20) it->type = CB_FALSE;
        else if (v == 21) it->type = CB_TRUE;
        else if (v == 22) it->type = CB_NULL;
        else if (v == 23) it->type = CB_UNDEF;
        else return -1;
    }
    r->it[k].next = r->n;
    return 0;
}

int cb_parse(Cb *c, const uint8_t *d, size_t len, CbItem *items, int max, int allow_trailing) {
    R r = { d, len, 0, items, 0, max };
    c->it = items;
    c->n = 0;
    c->used = 0;
    if (item(&r, 0)) return -1;
    if (!allow_trailing && r.p != len) return -1;
    c->n = r.n;
    c->used = r.p;
    return 0;
}

int cb_type(const Cb *c, int i) { return i >= 0 && i < c->n ? c->it[i].type : 0; }

int cb_int(const Cb *c, int i, int64_t *out) {
    int t = cb_type(c, i);
    if (t == CB_UINT && c->it[i].v <= (uint64_t)INT64_MAX) { *out = (int64_t)c->it[i].v; return 0; }
    if (t == CB_NINT && c->it[i].v <= (uint64_t)INT64_MAX) { *out = -1 - (int64_t)c->it[i].v; return 0; }
    return -1;
}

int cb_text_eq(const Cb *c, int i, const char *s) {
    if (cb_type(c, i) != CB_TEXT) return 0;
    size_t n = strlen(s);
    return c->it[i].len == n && !memcmp(c->it[i].ptr, s, n);
}

int cb_map_int(const Cb *c, int map, int64_t key) {
    if (cb_type(c, map) != CB_MAP) return -1;
    int k = map + 1;
    for (uint64_t i = 0; i < c->it[map].v; i++) {
        int64_t v;
        if (cb_int(c, k, &v) == 0 && v == key) return k + 1;
        k = c->it[k + 1].next;
    }
    return -1;
}

int cb_map_text(const Cb *c, int map, const char *key) {
    if (cb_type(c, map) != CB_MAP) return -1;
    int k = map + 1;
    for (uint64_t i = 0; i < c->it[map].v; i++) {
        if (cb_text_eq(c, k, key)) return k + 1;
        k = c->it[k + 1].next;
    }
    return -1;
}

int cb_at(const Cb *c, int arr, int k) {
    if (cb_type(c, arr) != CB_ARRAY || k < 0 || (uint64_t)k >= c->it[arr].v) return -1;
    int i = arr + 1;
    while (k-- > 0) i = c->it[i].next;
    return i;
}
