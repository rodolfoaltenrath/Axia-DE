/* imapp.c — see imapp.h. */
#include "imapp.h"

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

typedef struct {
    const char *s;
    size_t len, i;
    IVal *vals;
    size_t nvals, maxvals;
    char *strs;
    size_t nstr;
} P;

static IVal *newval(P *p, IvType t) {
    if (p->nvals >= p->maxvals) return NULL;
    IVal *v = &p->vals[p->nvals++];
    memset(v, 0, sizeof *v);
    v->type = t;
    return v;
}

static int atom_char(unsigned char c) {
    return c > 0x20 && c < 0x7f && c != '(' && c != ')' && c != '{' && c != '"' && c != '%';
}

/* One value at p->i. Returns NULL at end of the current list or on error (*err set). */
static IVal *value(P *p, int *err) {
    while (p->i < p->len && p->s[p->i] == ' ') p->i++;
    if (p->i >= p->len) return NULL;
    char c = p->s[p->i];
    if (c == '\r' || c == '\n') { p->i = p->len; return NULL; }
    if (c == ')') return NULL;
    if (c == '"') {
        IVal *v = newval(p, IV_STR);
        if (!v) { *err = 1; return NULL; }
        p->i++;
        char *o = p->strs + p->nstr;
        size_t n = 0;
        while (p->i < p->len && p->s[p->i] != '"') {
            char ch = p->s[p->i];
            if (ch == '\\' && p->i + 1 < p->len) { p->i++; ch = p->s[p->i]; }
            else if (ch == '\r' || ch == '\n') { *err = 1; return NULL; }
            o[n++] = ch;
            p->i++;
        }
        if (p->i >= p->len) { *err = 1; return NULL; }
        p->i++;
        o[n] = 0;
        p->nstr += n + 1;
        v->s = o; v->n = n;
        return v;
    }
    if (c == '{') {
        size_t k = p->i + 1;
        unsigned long long n = 0;
        size_t d0 = k;
        while (k < p->len && isdigit((unsigned char)p->s[k])) { n = n * 10 + (unsigned)(p->s[k] - '0'); if (n > (1ull << 31)) { *err = 1; return NULL; } k++; }
        if (k == d0) { *err = 1; return NULL; }
        if (k < p->len && p->s[k] == '+') k++;
        if (k >= p->len || p->s[k] != '}') { *err = 1; return NULL; }
        k++;
        if (k + 1 < p->len && p->s[k] == '\r' && p->s[k + 1] == '\n') k += 2;
        else if (k < p->len && p->s[k] == '\n') k += 1;
        else { *err = 1; return NULL; }
        if (n > p->len - k) { *err = 1; return NULL; }
        IVal *v = newval(p, IV_STR);
        if (!v) { *err = 1; return NULL; }
        v->s = p->s + k; v->n = (size_t)n;
        p->i = k + (size_t)n;
        return v;
    }
    if (c == '(') {
        IVal *v = newval(p, IV_LIST);
        if (!v) { *err = 1; return NULL; }
        p->i++;
        IVal *last = NULL;
        for (;;) {
            IVal *k = value(p, err);
            if (*err) return NULL;
            if (!k) break;
            if (last) last->next = k; else v->kid = k;
            last = k;
        }
        if (p->i >= p->len || p->s[p->i] != ')') { *err = 1; return NULL; }
        p->i++;
        return v;
    }
    /* atom, possibly with [section] (spaces and parens allowed inside) and <partial> */
    size_t a = p->i;
    int depth = 0;
    while (p->i < p->len) {
        unsigned char ch = (unsigned char)p->s[p->i];
        if (ch == '[') { depth++; p->i++; continue; }
        if (ch == ']' && depth) { depth--; p->i++; continue; }
        if (depth) { if (ch == '\r' || ch == '\n') break; p->i++; continue; }
        if (!atom_char(ch) && ch != '%') break;
        p->i++;
    }
    if (depth) { *err = 1; return NULL; }
    if (p->i == a) { *err = 1; return NULL; }
    IVal *v = newval(p, IV_ATOM);
    if (!v) { *err = 1; return NULL; }
    v->s = p->s + a; v->n = p->i - a;
    if (v->n == 3 && !strncasecmp(v->s, "NIL", 3)) { v->type = IV_NIL; }
    return v;
}

int imap_parse(const char *buf, size_t len, IResp *out) {
    memset(out, 0, sizeof *out);
    P p = { 0 };
    p.s = buf; p.len = len;
    p.maxvals = len + 2 < IMAP_MAX_VALUES ? len + 2 : IMAP_MAX_VALUES;
    size_t vbytes = p.maxvals * sizeof(IVal);
    out->arena = malloc(vbytes + len + 2);
    if (!out->arena) return -1;
    p.vals = (IVal *)(void *)out->arena;
    p.strs = out->arena + vbytes;
    /* tag */
    size_t k = 0;
    while (k < len && buf[k] != ' ' && buf[k] != '\r' && buf[k] != '\n' && k + 1 < sizeof out->tag) { out->tag[k] = buf[k]; k++; }
    out->tag[k] = 0;
    if (!k) { imap_resp_free(out); return -1; }
    p.i = k;
    IVal *last = NULL;
    int err = 0;
    for (;;) {
        IVal *v = value(&p, &err);
        if (err) { imap_resp_free(out); return -1; }
        if (!v) {
            if (p.i < p.len && p.s[p.i] == ')') { imap_resp_free(out); return -1; }   /* stray ')' */
            break;
        }
        if (last) last->next = v; else out->first = v;
        last = v;
    }
    return 0;
}

void imap_resp_free(IResp *r) { free(r->arena); r->arena = NULL; r->first = NULL; }

int iv_is(const IVal *v, const char *atom) {
    size_t n = strlen(atom);
    return v && v->type == IV_ATOM && v->n == n && !strncasecmp(v->s, atom, n);
}

long long iv_num(const IVal *v, long long def) {
    if (!v || (v->type != IV_ATOM && v->type != IV_STR) || !v->n || v->n > 20) return def;
    long long x = 0;
    for (size_t i = 0; i < v->n; i++) { if (!isdigit((unsigned char)v->s[i])) return def; x = x * 10 + (v->s[i] - '0'); }
    return x;
}

IVal *iv_at(IVal *first, int idx) {
    while (first && idx-- > 0) first = first->next;
    return first;
}

int iv_count(const IVal *first) { int n = 0; while (first) { n++; first = first->next; } return n; }

IVal *imap_fetch_item(IVal *list, const char *key) {
    if (!list) return NULL;
    IVal *k = list->type == IV_LIST ? list->kid : list;
    size_t kl = strlen(key);
    int prefix = memchr(key, '[', kl) != NULL;      /* BODY[...] sections: prefix match */
    for (; k && k->next; k = k->next->next) {
        if (k->type != IV_ATOM) return NULL;
        if (prefix ? (k->n >= kl && !strncasecmp(k->s, key, kl)) : iv_is(k, key)) return k->next;
    }
    return NULL;
}

size_t iv_copy(const IVal *v, char *out, size_t cap) {
    if (!cap) return 0;
    out[0] = 0;
    if (!v || v->type == IV_NIL || v->type == IV_LIST) return 0;
    size_t n = v->n < cap - 1 ? v->n : cap - 1;
    memcpy(out, v->s, n);
    out[n] = 0;
    return n;
}

int imap_code_num(const IResp *r, const char *code, unsigned long long *out) {
    size_t cl = strlen(code);
    for (const IVal *v = r->first; v; v = v->next) {
        if (v->type != IV_ATOM || v->n < cl + 2 || v->s[0] != '[') continue;
        if (strncasecmp(v->s + 1, code, cl) || (v->s[1 + cl] != ' ' && v->s[1 + cl] != ']')) continue;
        const char *p = v->s + 1 + cl;
        while (*p == ' ') p++;
        unsigned long long x = 0;
        int any = 0;
        while (p < v->s + v->n && isdigit((unsigned char)*p)) { x = x * 10 + (unsigned)(*p - '0'); p++; any = 1; }
        if (!any) return 0;
        *out = x;
        return 1;
    }
    return 0;
}

char *imap_quote(const char *s, int nonsync, int *is_literal) {
    if (!s) s = "";
    size_t n = strlen(s);
    int lit = 0;
    for (size_t i = 0; i < n; i++) { unsigned char c = (unsigned char)s[i]; if (c == '\r' || c == '\n' || c >= 0x80 || c == 0) lit = 1; }
    if (is_literal) *is_literal = lit;
    if (lit) {
        char *o = malloc(n + 32);
        if (!o) return NULL;
        int h = snprintf(o, 32, "{%zu%s}\r\n", n, nonsync ? "+" : "");
        memcpy(o + h, s, n);
        o[(size_t)h + n] = 0;
        return o;
    }
    char *o = malloc(n * 2 + 3);
    if (!o) return NULL;
    size_t k = 0;
    o[k++] = '"';
    for (size_t i = 0; i < n; i++) { if (s[i] == '"' || s[i] == '\\') o[k++] = '\\'; o[k++] = s[i]; }
    o[k++] = '"';
    o[k] = 0;
    return o;
}

/* ---- modified UTF-7 ---- */

static const char b64m[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

static int put_utf8(char *o, size_t room, unsigned cp) {
    char t[4];
    int k;
    if (cp < 0x80) { t[0] = (char)cp; k = 1; }
    else if (cp < 0x800) { t[0] = (char)(0xc0 | (cp >> 6)); t[1] = (char)(0x80 | (cp & 0x3f)); k = 2; }
    else if (cp < 0x10000) { t[0] = (char)(0xe0 | (cp >> 12)); t[1] = (char)(0x80 | ((cp >> 6) & 0x3f)); t[2] = (char)(0x80 | (cp & 0x3f)); k = 3; }
    else { t[0] = (char)(0xf0 | (cp >> 18)); t[1] = (char)(0x80 | ((cp >> 12) & 0x3f)); t[2] = (char)(0x80 | ((cp >> 6) & 0x3f)); t[3] = (char)(0x80 | (cp & 0x3f)); k = 4; }
    if ((size_t)k > room) return 0;
    memcpy(o, t, (size_t)k);
    return k;
}

void imap_utf7_decode(const char *in, char *out, size_t cap) {
    if (!cap) return;
    size_t o = 0;
    for (size_t i = 0; in[i] && o + 1 < cap;) {
        if (in[i] != '&') { out[o++] = in[i++]; continue; }
        i++;
        if (in[i] == '-') { out[o++] = '&'; i++; continue; }
        unsigned acc = 0, bits = 0, hi = 0;
        while (in[i] && in[i] != '-') {
            const char *p = strchr(b64m, in[i]);
            if (!p) break;
            acc = (acc << 6) | (unsigned)(p - b64m);
            bits += 6;
            if (bits >= 16) {
                bits -= 16;
                unsigned u = (acc >> bits) & 0xffff;
                if (u >= 0xd800 && u <= 0xdbff) hi = u;
                else {
                    unsigned cp = u;
                    if (u >= 0xdc00 && u <= 0xdfff && hi) cp = 0x10000 + ((hi - 0xd800) << 10) + (u - 0xdc00);
                    else if (u >= 0xdc00 && u <= 0xdfff) cp = 0xfffd;
                    hi = 0;
                    o += (size_t)put_utf8(out + o, cap - 1 - o, cp);
                }
            }
            i++;
        }
        if (in[i] == '-') i++;
    }
    out[o] = 0;
}

void imap_utf7_encode(const char *in, char *out, size_t cap) {
    if (!cap) return;
    size_t o = 0;
    const unsigned char *s = (const unsigned char *)in;
    for (size_t i = 0; s[i] && o + 1 < cap;) {
        if (s[i] >= 0x20 && s[i] < 0x7f) {
            if (s[i] == '&') { if (o + 2 >= cap) break; out[o++] = '&'; out[o++] = '-'; }
            else out[o++] = (char)s[i];
            i++;
            continue;
        }
        if (o + 1 >= cap) break;
        out[o++] = '&';
        unsigned acc = 0, bits = 0;
        while (s[i] && (s[i] < 0x20 || s[i] >= 0x7f)) {
            unsigned cp = s[i], k = 1;
            if (cp >= 0xf0 && s[i + 1] && s[i + 2] && s[i + 3]) { cp = ((cp & 7u) << 18) | ((s[i + 1] & 0x3fu) << 12) | ((s[i + 2] & 0x3fu) << 6) | (s[i + 3] & 0x3fu); k = 4; }
            else if (cp >= 0xe0 && s[i + 1] && s[i + 2]) { cp = ((cp & 15u) << 12) | ((s[i + 1] & 0x3fu) << 6) | (s[i + 2] & 0x3fu); k = 3; }
            else if (cp >= 0xc0 && s[i + 1]) { cp = ((cp & 31u) << 6) | (s[i + 1] & 0x3fu); k = 2; }
            i += k;
            unsigned units[2], nu = 0;
            if (cp >= 0x10000) { cp -= 0x10000; units[nu++] = 0xd800 + (cp >> 10); units[nu++] = 0xdc00 + (cp & 0x3ff); }
            else units[nu++] = cp;
            for (unsigned u = 0; u < nu; u++) {
                acc = (acc << 16) | units[u];
                bits += 16;
                while (bits >= 6) { bits -= 6; if (o + 2 < cap) out[o++] = b64m[(acc >> bits) & 63]; }
            }
        }
        if (bits && o + 2 < cap) out[o++] = b64m[(acc << (6 - bits)) & 63];
        if (o + 1 < cap) out[o++] = '-';
    }
    out[o] = 0;
}

size_t imap_uidset(const uint32_t *u, size_t n, char *out, size_t cap) {
    size_t o = 0;
    if (!cap) return 0;
    out[0] = 0;
    for (size_t i = 0; i < n;) {
        size_t j = i;
        while (j + 1 < n && u[j + 1] == u[j] + 1) j++;
        char t[32];
        int k = j > i ? snprintf(t, sizeof t, "%s%u:%u", o ? "," : "", u[i], u[j]) : snprintf(t, sizeof t, "%s%u", o ? "," : "", u[i]);
        if (o + (size_t)k + 1 > cap) break;
        memcpy(out + o, t, (size_t)k);
        o += (size_t)k;
        out[o] = 0;
        i = j + 1;
    }
    return o;
}

size_t imap_parse_uidset(const char *s, uint32_t *out, size_t max) {
    size_t n = 0;
    while (*s && n < max) {
        char *e;
        unsigned long a = strtoul(s, &e, 10);
        if (e == s) { while (*s && *s != ',') s++; if (*s) s++; continue; }
        unsigned long b = a;
        s = e;
        if (*s == ':') {
            s++;
            b = strtoul(s, &e, 10);
            if (e == s) { b = a; while (*s && *s != ',') s++; }
            else s = e;
        }
        if (b < a) { unsigned long t = a; a = b; b = t; }
        for (unsigned long x = a; x <= b && n < max; x++) out[n++] = (uint32_t)x;
        if (*s == ',') s++;
    }
    return n;
}
