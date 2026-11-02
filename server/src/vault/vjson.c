/* vjson.c — JSON in the secrets arena (see vjson.h). */
#include "vjson.h"
#include "smem.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ================= reader ================= */

typedef struct { Vj *j; size_t p; int depth; } P;

static void ws(P *q) {
    const char *s = q->j->s;
    while (q->p < q->j->len && (s[q->p] == ' ' || s[q->p] == '\t' || s[q->p] == '\n' || s[q->p] == '\r')) q->p++;
}

static int tok_new(P *q, int type) {
    Vj *j = q->j;
    if (j->n == j->cap) {
        int nc = j->cap ? j->cap * 2 : 64;
        if (nc > (1 << 24)) return -1;
        VjTok *t = sm_realloc(j->t, sizeof(VjTok) * (size_t)nc);
        if (!t) return -1;
        j->t = t;
        j->cap = nc;
    }
    VjTok *t = &j->t[j->n];
    memset(t, 0, sizeof *t);
    t->type = (uint8_t)type;
    t->start = (uint32_t)q->p;
    return j->n++;
}

static int hexv(int c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1; }

static int parse_value(P *q);

static int parse_string(P *q) {
    const char *s = q->j->s;
    size_t len = q->j->len;
    q->p++;                                   /* quote */
    int k = tok_new(q, VJ_STR);
    if (k < 0) return -1;
    while (q->p < len) {
        unsigned char c = (unsigned char)s[q->p];
        if (c == '"') { q->j->t[k].end = (uint32_t)q->p; q->p++; q->j->t[k].next = k + 1; return 0; }
        if (c < 0x20) return -1;
        if (c == '\\') {
            if (q->p + 1 >= len) return -1;
            char e = s[q->p + 1];
            if (e == 'u') {
                if (q->p + 6 > len) return -1;
                for (int i = 2; i < 6; i++) if (hexv(s[q->p + i]) < 0) return -1;
                q->p += 6;
                continue;
            }
            if (!strchr("\"\\/bfnrt", e)) return -1;
            q->p += 2;
            continue;
        }
        q->p++;
    }
    return -1;
}

static int parse_number(P *q) {
    const char *s = q->j->s;
    size_t len = q->j->len, p = q->p;
    int k = tok_new(q, VJ_NUM);
    if (k < 0) return -1;
    if (p < len && s[p] == '-') p++;
    if (p >= len) return -1;
    if (s[p] == '0') p++;
    else if (s[p] >= '1' && s[p] <= '9') while (p < len && s[p] >= '0' && s[p] <= '9') p++;
    else return -1;
    if (p < len && s[p] == '.') { p++; if (p >= len || s[p] < '0' || s[p] > '9') return -1; while (p < len && s[p] >= '0' && s[p] <= '9') p++; }
    if (p < len && (s[p] == 'e' || s[p] == 'E')) {
        p++;
        if (p < len && (s[p] == '+' || s[p] == '-')) p++;
        if (p >= len || s[p] < '0' || s[p] > '9') return -1;
        while (p < len && s[p] >= '0' && s[p] <= '9') p++;
    }
    q->j->t[k].end = (uint32_t)p;
    q->j->t[k].next = k + 1;
    q->p = p;
    return 0;
}

static int parse_lit(P *q, const char *w, int type) {
    size_t n = strlen(w);
    if (q->p + n > q->j->len || memcmp(q->j->s + q->p, w, n)) return -1;
    int k = tok_new(q, type);
    if (k < 0) return -1;
    q->p += n;
    q->j->t[k].end = (uint32_t)q->p;
    q->j->t[k].next = k + 1;
    return 0;
}

static int parse_container(P *q, int obj) {
    if (++q->depth > 64) return -1;
    int k = tok_new(q, obj ? VJ_OBJ : VJ_ARR);
    if (k < 0) return -1;
    q->p++;
    ws(q);
    int count = 0;
    char close = obj ? '}' : ']';
    if (q->p < q->j->len && q->j->s[q->p] == close) { q->p++; goto done; }
    for (;;) {
        ws(q);
        if (obj) {
            if (q->p >= q->j->len || q->j->s[q->p] != '"') return -1;
            if (parse_string(q)) return -1;
            ws(q);
            if (q->p >= q->j->len || q->j->s[q->p] != ':') return -1;
            q->p++;
            ws(q);
        }
        if (parse_value(q)) return -1;
        count++;
        ws(q);
        if (q->p >= q->j->len) return -1;
        char c = q->j->s[q->p++];
        if (c == ',') continue;
        if (c == close) break;
        return -1;
    }
done:
    q->j->t[k].end = (uint32_t)q->p;
    q->j->t[k].count = count;
    q->j->t[k].next = q->j->n;
    q->depth--;
    return 0;
}

static int parse_value(P *q) {
    ws(q);
    if (q->p >= q->j->len) return -1;
    char c = q->j->s[q->p];
    if (c == '{') return parse_container(q, 1);
    if (c == '[') return parse_container(q, 0);
    if (c == '"') return parse_string(q);
    if (c == 't') return parse_lit(q, "true", VJ_TRUE);
    if (c == 'f') return parse_lit(q, "false", VJ_FALSE);
    if (c == 'n') return parse_lit(q, "null", VJ_NULL);
    return parse_number(q);
}

int vj_parse(Vj *j, const char *s, size_t len) {
    memset(j, 0, sizeof *j);
    j->s = s;
    j->len = len;
    P q = { j, 0, 0 };
    if (parse_value(&q)) { vj_free(j); j->s = s; j->len = len; return -1; }
    ws(&q);
    if (q.p != len) { vj_free(j); return -1; }
    return 0;
}

void vj_free(Vj *j) {
    sm_free(j->t);
    j->t = NULL;
    j->n = j->cap = 0;
}

int vj_type(const Vj *j, int tok) { return tok >= 0 && tok < j->n ? j->t[tok].type : 0; }

int vj_streq(const Vj *j, int tok, const char *s) {
    if (vj_type(j, tok) != VJ_STR) return 0;
    const VjTok *t = &j->t[tok];
    size_t n = strlen(s);
    /* fast comparison if there are no escapes */
    if (t->end - t->start == n && !memchr(j->s + t->start, '\\', n)) return !memcmp(j->s + t->start, s, n);
    char *d = vj_str(j, tok);
    int r = d && !strcmp(d, s);
    sm_free(d);
    return r;
}

int vj_get(const Vj *j, int obj, const char *key) {
    if (vj_type(j, obj) != VJ_OBJ) return -1;
    for (int k = obj + 1; k < j->t[obj].next; k = j->t[k + 1].next)
        if (vj_streq(j, k, key)) return k + 1;
    return -1;
}

int vj_at(const Vj *j, int arr, int i) {
    if (vj_type(j, arr) != VJ_ARR || i < 0 || i >= j->t[arr].count) return -1;
    int k = arr + 1;
    while (i-- > 0) k = j->t[k].next;
    return k;
}

static size_t put_utf8(char *d, unsigned cp) {
    if (cp < 0x80) { d[0] = (char)cp; return 1; }
    if (cp < 0x800) { d[0] = (char)(0xc0 | cp >> 6); d[1] = (char)(0x80 | (cp & 63)); return 2; }
    if (cp < 0x10000) { d[0] = (char)(0xe0 | cp >> 12); d[1] = (char)(0x80 | ((cp >> 6) & 63)); d[2] = (char)(0x80 | (cp & 63)); return 3; }
    d[0] = (char)(0xf0 | cp >> 18); d[1] = (char)(0x80 | ((cp >> 12) & 63)); d[2] = (char)(0x80 | ((cp >> 6) & 63)); d[3] = (char)(0x80 | (cp & 63));
    return 4;
}

/* Unescapes s[a..b) into d (which has room for b-a bytes: it never grows). */
static size_t unescape(const char *s, size_t a, size_t b, char *d) {
    size_t o = 0;
    for (size_t i = a; i < b; i++) {
        if (s[i] != '\\') { d[o++] = s[i]; continue; }
        char e = s[++i];
        switch (e) {
        case 'b': d[o++] = '\b'; break;
        case 'f': d[o++] = '\f'; break;
        case 'n': d[o++] = '\n'; break;
        case 'r': d[o++] = '\r'; break;
        case 't': d[o++] = '\t'; break;
        case 'u': {
            unsigned cp = 0;
            for (int k = 1; k <= 4; k++) cp = cp << 4 | (unsigned)hexv(s[i + k]);
            i += 4;
            if (cp >= 0xd800 && cp < 0xdc00 && i + 6 < b && s[i + 1] == '\\' && s[i + 2] == 'u') {
                unsigned lo = 0;
                for (int k = 3; k <= 6; k++) lo = lo << 4 | (unsigned)hexv(s[i + k]);
                if (lo >= 0xdc00 && lo < 0xe000) { cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00); i += 6; }
                else cp = 0xfffd;
            } else if (cp >= 0xd800 && cp < 0xe000) cp = 0xfffd;
            if (cp == 0) cp = 0xfffd;                  /* no NUL inside texts */
            o += put_utf8(d + o, cp);
            break;
        }
        default: d[o++] = e; break;
        }
    }
    return o;
}

char *vj_str(const Vj *j, int tok) {
    if (vj_type(j, tok) != VJ_STR) return NULL;
    const VjTok *t = &j->t[tok];
    char *d = sm_alloc((size_t)(t->end - t->start) + 1);
    if (!d) return NULL;
    d[unescape(j->s, t->start, t->end, d)] = 0;
    return d;
}

size_t vj_strbuf(const Vj *j, int tok, char *buf, size_t n) {
    if (!n) return 0;
    buf[0] = 0;
    char *d = vj_str(j, tok);
    if (!d) return 0;
    size_t l = strlen(d);
    if (l >= n) {                                    /* truncate without splitting a UTF-8 character */
        l = n - 1;
        while (l > 0 && ((unsigned char)d[l] & 0xc0) == 0x80) l--;
    }
    memcpy(buf, d, l);
    buf[l] = 0;
    sm_free(d);
    return l;
}

double vj_num(const Vj *j, int tok, double def) {
    if (vj_type(j, tok) != VJ_NUM) return def;
    char b[64];
    size_t n = j->t[tok].end - j->t[tok].start;
    if (n >= sizeof b) return def;
    memcpy(b, j->s + j->t[tok].start, n);
    b[n] = 0;
    double v = strtod(b, NULL);
    return isfinite(v) ? v : def;
}

int vj_bool(const Vj *j, int tok, int def) {
    int t = vj_type(j, tok);
    return t == VJ_TRUE ? 1 : t == VJ_FALSE ? 0 : def;
}

char *vj_gets(const Vj *j, int obj, const char *key) { return vj_str(j, vj_get(j, obj, key)); }
size_t vj_getsbuf(const Vj *j, int obj, const char *key, char *buf, size_t n) {
    int k = vj_get(j, obj, key);
    if (k < 0) { if (n) buf[0] = 0; return 0; }
    return vj_strbuf(j, k, buf, n);
}
double vj_getn(const Vj *j, int obj, const char *key, double def) { return vj_num(j, vj_get(j, obj, key), def); }
int vj_getb(const Vj *j, int obj, const char *key, int def) { return vj_bool(j, vj_get(j, obj, key), def); }

/* ================= writer ================= */

void vb_init(VjB *b) { memset(b, 0, sizeof *b); b->first[0] = 1; }
void vb_free(VjB *b) { sm_free(b->p); memset(b, 0, sizeof *b); }

static int grow(VjB *b, size_t extra) {
    if (b->oom) return -1;
    if (b->n + extra + 1 <= b->cap) return 0;
    size_t nc = b->cap ? b->cap : 256;
    while (nc < b->n + extra + 1) nc *= 2;
    char *p = sm_realloc(b->p, nc);
    if (!p) { b->oom = 1; return -1; }
    b->p = p;
    b->cap = nc;
    return 0;
}

void vb_rawn(VjB *b, const char *s, size_t n) {
    if (grow(b, n)) return;
    memcpy(b->p + b->n, s, n);
    b->n += n;
    b->p[b->n] = 0;
}
void vb_raw(VjB *b, const char *s) { vb_rawn(b, s, strlen(s)); }

void vb_strn(VjB *b, const char *s, size_t n) {
    if (!s) { vb_raw(b, "null"); return; }
    if (grow(b, n * 6 + 2)) return;
    char *d = b->p + b->n;
    *d++ = '"';
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c == '"' || c == '\\') { *d++ = '\\'; *d++ = (char)c; }
        else if (c == '\n') { *d++ = '\\'; *d++ = 'n'; }
        else if (c == '\r') { *d++ = '\\'; *d++ = 'r'; }
        else if (c == '\t') { *d++ = '\\'; *d++ = 't'; }
        else if (c < 0x20 || c == 0x7f) { snprintf(d, 7, "\\u%04x", c); d += 6; }
        else if (c >= 0x80) {
            /* valid UTF-8 passes through as-is; a stray byte is replaced by U+FFFD */
            int len = c >= 0xf0 && c < 0xf5 ? 4 : c >= 0xe0 ? 3 : c >= 0xc2 && c < 0xe0 ? 2 : 0;
            int ok = len && i + (size_t)len <= n;
            for (int k = 1; ok && k < len; k++) if (((unsigned char)s[i + k] & 0xc0) != 0x80) ok = 0;
            if (ok) { memcpy(d, s + i, (size_t)len); d += len; i += (size_t)len - 1; }
            else { memcpy(d, "\xef\xbf\xbd", 3); d += 3; }
        } else *d++ = (char)c;
    }
    *d++ = '"';
    b->n = (size_t)(d - b->p);
    b->p[b->n] = 0;
}
void vb_str(VjB *b, const char *s) { vb_strn(b, s, s ? strlen(s) : 0); }

void vb_num(VjB *b, double v) {
    char t[40];
    if (!isfinite(v)) v = 0;
    if (v == (double)(long long)v && fabs(v) < 9e15) snprintf(t, sizeof t, "%lld", (long long)v);
    else snprintf(t, sizeof t, "%.17g", v);
    vb_raw(b, t);
}
void vb_int(VjB *b, long long v) { char t[24]; snprintf(t, sizeof t, "%lld", v); vb_raw(b, t); }

void vb_sep(VjB *b) {
    int d = b->depth < 63 ? b->depth : 63;
    if (!b->first[d]) vb_raw(b, ",");
    b->first[d] = 0;
}
void vb_open(VjB *b, char c) {
    char s[2] = { c, 0 };
    vb_raw(b, s);
    if (b->depth < 63) b->depth++;
    b->first[b->depth] = 1;
}
void vb_close(VjB *b, char c) {
    char s[2] = { c, 0 };
    vb_raw(b, s);
    if (b->depth > 0) b->depth--;
}
void vb_key(VjB *b, const char *k) { vb_sep(b); vb_str(b, k); vb_raw(b, ":"); }
void vb_ks(VjB *b, const char *k, const char *v) { vb_key(b, k); vb_str(b, v); }
void vb_kn(VjB *b, const char *k, double v) { vb_key(b, k); vb_num(b, v); }
void vb_ki(VjB *b, const char *k, long long v) { vb_key(b, k); vb_int(b, v); }
void vb_kb(VjB *b, const char *k, int v) { vb_key(b, k); vb_raw(b, v ? "true" : "false"); }

/* ================= base64 ================= */

static const char B64[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

char *b64_encode(const uint8_t *in, size_t n) {
    char *o = sm_alloc((n + 2) / 3 * 4 + 1);
    if (!o) return NULL;
    size_t k = 0;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)in[i] << 16 | (i + 1 < n ? (uint32_t)in[i + 1] << 8 : 0) | (i + 2 < n ? in[i + 2] : 0);
        o[k++] = B64[v >> 18 & 63];
        o[k++] = B64[v >> 12 & 63];
        o[k++] = i + 1 < n ? B64[v >> 6 & 63] : '=';
        o[k++] = i + 2 < n ? B64[v & 63] : '=';
    }
    o[k] = 0;
    return o;
}

uint8_t *b64_decode(const char *in, size_t n, size_t *outlen) {
    uint8_t *o = sm_alloc(n / 4 * 3 + 3);
    if (!o) return NULL;
    uint32_t acc = 0;
    int bits = 0;
    size_t k = 0;
    for (size_t i = 0; i < n; i++) {
        char c = in[i];
        int v;
        if (c >= 'A' && c <= 'Z') v = c - 'A';
        else if (c >= 'a' && c <= 'z') v = c - 'a' + 26;
        else if (c >= '0' && c <= '9') v = c - '0' + 52;
        else if (c == '+' || c == '-') v = 62;
        else if (c == '/' || c == '_') v = 63;
        else if (c == '=' || c == ' ' || c == '\n' || c == '\r' || c == '\t') continue;
        else { sm_free(o); return NULL; }
        acc = acc << 6 | (uint32_t)v;
        bits += 6;
        if (bits >= 8) { bits -= 8; o[k++] = (uint8_t)(acc >> bits); }
    }
    *outlen = k;
    return o;
}
