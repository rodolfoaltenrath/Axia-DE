/* mime.c — see mime.h. */
#include "mime.h"
#include "charset_tables.h"

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

/* ---- utilities ---- */

static int lc(int c) { return (c >= 'A' && c <= 'Z') ? c + 32 : c; }

static void copy_trim(char *out, size_t cap, const char *s, size_t n) {
    if (!cap) return;
    while (n && (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n')) { s++; n--; }
    while (n && (s[n - 1] == ' ' || s[n - 1] == '\t' || s[n - 1] == '\r' || s[n - 1] == '\n')) n--;
    if (n >= cap) n = cap - 1;
    memcpy(out, s, n);
    out[n] = 0;
}

/* Bounded copy that always terminates (no truncation warnings from snprintf). */
static void scpy(char *d, size_t cap, const char *s) {
    if (!cap) return;
    size_t n = strlen(s);
    if (n >= cap) n = cap - 1;
    memcpy(d, s, n);
    d[n] = 0;
}

static void lower_ascii(char *s) { for (; *s; s++) *s = (char)lc((unsigned char)*s); }

/* Truncates UTF-8 text to cap-1 bytes without splitting a character. */
static void utf8_fit(char *s, size_t cap) {
    size_t n = strlen(s);
    if (n < cap) return;
    n = cap - 1;
    while (n && ((unsigned char)s[n] & 0xc0) == 0x80) n--;
    s[n] = 0;
}

typedef struct { char *p; size_t n, cap; int oom; } Buf;
static void bput(Buf *b, const void *d, size_t n) {
    if (b->oom) return;
    if (b->n + n + 1 > b->cap) {
        size_t nc = b->cap ? b->cap : 256;
        while (nc < b->n + n + 1) nc *= 2;
        char *np = realloc(b->p, nc);
        if (!np) { b->oom = 1; return; }
        b->p = np; b->cap = nc;
    }
    memcpy(b->p + b->n, d, n);
    b->n += n;
    b->p[b->n] = 0;
}
static void bputc(Buf *b, char c) { bput(b, &c, 1); }
static void bputs(Buf *b, const char *s) { bput(b, s, strlen(s)); }
static char *bdone(Buf *b, size_t *olen) {
    if (b->oom) { free(b->p); if (olen) *olen = 0; return NULL; }
    if (!b->p) { b->p = malloc(1); if (!b->p) return NULL; b->p[0] = 0; }
    if (olen) *olen = b->n;
    return b->p;
}

static int put_cp(char *o, uint32_t cp) {
    if (cp < 0x80) { o[0] = (char)cp; return 1; }
    if (cp < 0x800) { o[0] = (char)(0xc0 | (cp >> 6)); o[1] = (char)(0x80 | (cp & 0x3f)); return 2; }
    if (cp < 0x10000) { o[0] = (char)(0xe0 | (cp >> 12)); o[1] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[2] = (char)(0x80 | (cp & 0x3f)); return 3; }
    o[0] = (char)(0xf0 | (cp >> 18)); o[1] = (char)(0x80 | ((cp >> 12) & 0x3f)); o[2] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[3] = (char)(0x80 | (cp & 0x3f));
    return 4;
}

/* Length of the valid UTF-8 sequence at s (0 if invalid). */
static int utf8_len(const unsigned char *s, size_t n) {
    if (!n) return 0;
    unsigned c = s[0];
    if (c < 0x80) return 1;
    int k;
    uint32_t cp;
    if (c >= 0xc2 && c <= 0xdf) { k = 2; cp = c & 0x1f; }
    else if (c >= 0xe0 && c <= 0xef) { k = 3; cp = c & 0x0f; }
    else if (c >= 0xf0 && c <= 0xf4) { k = 4; cp = c & 0x07; }
    else return 0;
    if ((size_t)k > n) return 0;
    for (int i = 1; i < k; i++) { if ((s[i] & 0xc0) != 0x80) return 0; cp = (cp << 6) | (s[i] & 0x3f); }
    if ((k == 3 && cp < 0x800) || (k == 4 && (cp < 0x10000 || cp > 0x10ffff)) || (cp >= 0xd800 && cp <= 0xdfff)) return 0;
    return k;
}

static int utf8_valid(const char *s, size_t n) {
    for (size_t i = 0; i < n;) { int k = utf8_len((const unsigned char *)s + i, n - i); if (!k) return 0; i += (size_t)k; }
    return 1;
}

char *utf8_clean(const char *in, size_t len, size_t *olen) {
    Buf b = { 0 };
    for (size_t i = 0; i < len;) {
        int k = utf8_len((const unsigned char *)in + i, len - i);
        if (!k) { bput(&b, "\xef\xbf\xbd", 3); i++; continue; }
        if (k == 1 && in[i] == 0) { i++; continue; }      /* no embedded NUL */
        bput(&b, in + i, (size_t)k);
        i += (size_t)k;
    }
    return bdone(&b, olen);
}

/* ---- character sets ---- */

static void cs_norm(const char *cs, char *out, size_t cap) {
    size_t o = 0;
    for (; cs && *cs && o + 1 < cap; cs++) {
        unsigned char c = (unsigned char)*cs;
        if (c == '"' || c == '\'' || c == '-' || c == '_' || c == ' ' || c == '.') continue;
        if (c == '*') break;                          /* RFC 2231 language */
        out[o++] = (char)lc(c);
    }
    out[o] = 0;
    /* common aliases */
    if (!strncmp(out, "cp125", 5)) { char t[64]; snprintf(t, sizeof t, "windows%s", out + 2); snprintf(out, cap, "%s", t); }
    else if (!strncmp(out, "xcp125", 6)) { char t[64]; snprintf(t, sizeof t, "windows%s", out + 3); snprintf(out, cap, "%s", t); }
    else if (!strcmp(out, "cp850")) snprintf(out, cap, "ibm850");
    else if (!strcmp(out, "cp437")) snprintf(out, cap, "ibm437");
    else if (!strcmp(out, "mac") || !strcmp(out, "macroman") || !strcmp(out, "xmacroman")) snprintf(out, cap, "macintosh");
    else if (!strncmp(out, "iso8859", 7) && out[7] && out[7] != '-') { /* "iso88591" is already fine */ }
}

static const CsTable *cs_table(const char *norm) {
    for (size_t i = 0; i < sizeof cs_tables / sizeof cs_tables[0]; i++) {
        char n[64];
        cs_norm(cs_tables[i].name, n, sizeof n);
        if (!strcmp(n, norm)) return &cs_tables[i];
        if (cs_tables[i].alias[0]) { cs_norm(cs_tables[i].alias, n, sizeof n); if (!strcmp(n, norm)) return &cs_tables[i]; }
    }
    return NULL;
}

static int is_utf8_name(const char *n) { return !strcmp(n, "utf8") || !strcmp(n, "unicode11utf8") || !strcmp(n, "xunicode20utf8"); }
static int is_ascii_name(const char *n) { return !*n || !strcmp(n, "usascii") || !strcmp(n, "ascii") || !strcmp(n, "ansix341968") || !strcmp(n, "us"); }
static int is_latin1_name(const char *n) { return !strcmp(n, "iso88591") || !strcmp(n, "latin1") || !strcmp(n, "l1") || !strcmp(n, "iso885911987") || !strcmp(n, "isoir100"); }

int charset_known(const char *cs) {
    char n[64];
    cs_norm(cs, n, sizeof n);
    return is_utf8_name(n) || is_ascii_name(n) || is_latin1_name(n) || cs_table(n) || !strncmp(n, "utf16", 5) || !strncmp(n, "utf32", 5);
}

static char *from_table(const uint16_t *hi, const char *in, size_t len, size_t *olen) {
    Buf b = { 0 };
    char t[4];
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)in[i];
        if (!c) continue;
        uint32_t cp = c < 0x80 ? c : (hi ? hi[c - 0x80] : c);
        bput(&b, t, (size_t)put_cp(t, cp));
    }
    return bdone(&b, olen);
}

static char *from_utf16(const char *in, size_t len, int be, int wide, size_t *olen) {
    Buf b = { 0 };
    char t[4];
    size_t step = wide ? 4 : 2;
    for (size_t i = 0; i + step <= len; i += step) {
        const unsigned char *u = (const unsigned char *)in + i;
        uint32_t cp;
        if (wide) cp = be ? ((uint32_t)u[0] << 24 | (uint32_t)u[1] << 16 | (uint32_t)u[2] << 8 | u[3]) : ((uint32_t)u[3] << 24 | (uint32_t)u[2] << 16 | (uint32_t)u[1] << 8 | u[0]);
        else {
            cp = be ? (uint32_t)(u[0] << 8 | u[1]) : (uint32_t)(u[1] << 8 | u[0]);
            if (cp >= 0xd800 && cp <= 0xdbff && i + 4 <= len) {
                uint32_t lo = be ? (uint32_t)(u[2] << 8 | u[3]) : (uint32_t)(u[3] << 8 | u[2]);
                if (lo >= 0xdc00 && lo <= 0xdfff) { cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00); i += 2; }
                else cp = 0xfffd;
            } else if (cp >= 0xd800 && cp <= 0xdfff) cp = 0xfffd;
        }
        if (cp == 0xfeff && i == 0) continue;
        if (!cp) continue;
        if (cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) cp = 0xfffd;
        bput(&b, t, (size_t)put_cp(t, cp));
    }
    return bdone(&b, olen);
}

char *charset_to_utf8(const char *cs, const char *in, size_t len, size_t *olen) {
    char n[64];
    cs_norm(cs, n, sizeof n);
    if (is_utf8_name(n)) return utf8_clean(in, len, olen);
    if (is_ascii_name(n)) {
        /* "us-ascii" with 8-bit bytes: almost always mislabeled UTF-8 or windows-1252 */
        if (utf8_valid(in, len)) return utf8_clean(in, len, olen);
        const CsTable *w = cs_table("windows1252");
        return from_table(w ? w->hi : NULL, in, len, olen);
    }
    if (is_latin1_name(n)) return from_table(NULL, in, len, olen);
    const CsTable *t = cs_table(n);
    if (t) return from_table(t->hi, in, len, olen);
    if (!strncmp(n, "utf16", 5) || !strncmp(n, "utf32", 5)) {
        int wide = n[3] == '3';
        int be = 1;
        const char *suf = n + 5;
        if (!strcmp(suf, "le")) be = 0;
        else if (!strcmp(suf, "be")) be = 1;
        else if (len >= 2) {          /* with BOM */
            unsigned char a = (unsigned char)in[0], c = (unsigned char)in[1];
            if (a == 0xff && c == 0xfe) be = 0;
        }
        return from_utf16(in, len, be, wide, olen);
    }
    /* unknown (GB2312, Shift_JIS, …): UTF-8 if it is, otherwise replacement */
    return utf8_clean(in, len, olen);
}

/* ---- base64 and quoted-printable ---- */

static int b64v(unsigned char c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+' || c == '-') return 62;
    if (c == '/' || c == '_') return 63;
    return -1;
}

size_t b64_decode(const char *in, size_t len, unsigned char *out) {
    uint32_t acc = 0;
    int bits = 0;
    size_t o = 0;
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)in[i];
        if (c == '=') break;
        int v = b64v(c);
        if (v < 0) continue;
        acc = (acc << 6) | (uint32_t)v;
        bits += 6;
        if (bits >= 8) { bits -= 8; out[o++] = (unsigned char)(acc >> bits); acc &= (1u << bits) - 1; }
    }
    return o;
}

char *b64_encode(const unsigned char *in, size_t len, int wrap, size_t *olen) {
    static const char tbl[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    Buf b = { 0 };
    int col = 0;
    for (size_t i = 0; i < len; i += 3) {
        uint32_t v = (uint32_t)in[i] << 16;
        if (i + 1 < len) v |= (uint32_t)in[i + 1] << 8;
        if (i + 2 < len) v |= in[i + 2];
        char q[4] = { tbl[(v >> 18) & 63], tbl[(v >> 12) & 63], i + 1 < len ? tbl[(v >> 6) & 63] : '=', i + 2 < len ? tbl[v & 63] : '=' };
        bput(&b, q, 4);
        col += 4;
        if (wrap && col >= 76) { bput(&b, "\r\n", 2); col = 0; }
    }
    if (wrap && col) bput(&b, "\r\n", 2);
    return bdone(&b, olen);
}

static int hexv(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

size_t qp_decode(const char *in, size_t len, char *out, int header) {
    size_t o = 0;
    for (size_t i = 0; i < len; i++) {
        char c = in[i];
        if (header && c == '_') { out[o++] = ' '; continue; }
        if (c != '=') { out[o++] = c; continue; }
        if (i + 1 < len && in[i + 1] == '\n') { i += 1; continue; }
        if (i + 2 < len && in[i + 1] == '\r' && in[i + 2] == '\n') { i += 2; continue; }
        /* "=" with whitespace before the line break (transport padding) */
        size_t k = i + 1;
        while (k < len && (in[k] == ' ' || in[k] == '\t')) k++;
        if (k < len && k > i + 1 && (in[k] == '\n' || in[k] == '\r')) { i = (in[k] == '\r' && k + 1 < len && in[k + 1] == '\n') ? k + 1 : k; continue; }
        if (i + 2 < len) {
            int h = hexv((unsigned char)in[i + 1]), l = hexv((unsigned char)in[i + 2]);
            if (h >= 0 && l >= 0) { out[o++] = (char)(h * 16 + l); i += 2; continue; }
        }
        out[o++] = '=';
    }
    return o;
}

char *qp_encode(const char *in, size_t len, size_t *olen) {
    Buf b = { 0 };
    int col = 0;
    static const char hx[] = "0123456789ABCDEF";
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)in[i];
        if (c == '\r' && i + 1 < len && in[i + 1] == '\n') continue;
        if (c == '\n') { bput(&b, "\r\n", 2); col = 0; continue; }
        int at_eol = i + 1 >= len || in[i + 1] == '\n' || in[i + 1] == '\r';
        char enc[3];
        size_t el;
        if ((c >= 33 && c <= 126 && c != '=') || ((c == ' ' || c == '\t') && !at_eol)) { enc[0] = (char)c; el = 1; }
        else { enc[0] = '='; enc[1] = hx[c >> 4]; enc[2] = hx[c & 15]; el = 3; }
        /* "." at the start of a line is also encoded (some servers mangle it) */
        if (col == 0 && c == '.') { enc[0] = '='; enc[1] = '2'; enc[2] = 'E'; el = 3; }
        if (col + (int)el > 75) { bput(&b, "=\r\n", 3); col = 0; }
        bput(&b, enc, el);
        col += (int)el;
    }
    return bdone(&b, olen);
}

/* ---- headers ---- */

void mime_split(const char *m, size_t len, const char **hdr, size_t *hlen, const char **body, size_t *blen) {
    *hdr = m; *hlen = 0; *body = m + len; *blen = 0;
    if (!len) return;
    if (m[0] == '\n') { *body = m + 1; *blen = len - 1; return; }
    if (len >= 2 && m[0] == '\r' && m[1] == '\n') { *body = m + 2; *blen = len - 2; return; }
    for (size_t i = 0; i + 1 < len; i++) {
        if (m[i] != '\n') continue;
        if (m[i + 1] == '\n') { *hlen = i + 1; *body = m + i + 2; *blen = len - i - 2; return; }
        if (m[i + 1] == '\r' && i + 2 < len && m[i + 2] == '\n') { *hlen = i + 1; *body = m + i + 3; *blen = len - i - 3; return; }
    }
    *hlen = len;
}

/* Finds the header and calls put with each piece of the unfolded value. */
static int header_find(const char *h, size_t n, const char *name, Buf *b, char *out, size_t cap) {
    size_t nl = strlen(name);
    size_t o = 0;
    for (size_t i = 0; i < n;) {
        size_t e = i;
        while (e < n && h[e] != '\n') e++;
        if (e - i > nl && h[i] != ' ' && h[i] != '\t' && h[i + nl] == ':' && !strncasecmp(h + i, name, nl)) {
            size_t v = i + nl + 1;
            for (;;) {
                size_t le = e;
                if (le > v && h[le - 1] == '\r') le--;
                for (size_t k = v; k < le; k++) {
                    if (b) bputc(b, h[k]);
                    else if (o + 1 < cap) out[o++] = h[k];
                }
                if (e + 1 < n && (h[e + 1] == ' ' || h[e + 1] == '\t')) {
                    v = e + 1;
                    e = v;
                    while (e < n && h[e] != '\n') e++;
                    continue;
                }
                break;
            }
            if (!b && cap) {
                out[o] = 0;
                /* trimmed */
                size_t s = 0;
                while (out[s] == ' ' || out[s] == '\t') s++;
                if (s) memmove(out, out + s, o - s + 1);
                o -= s;
                while (o && (out[o - 1] == ' ' || out[o - 1] == '\t')) out[--o] = 0;
            }
            return 1;
        }
        i = e + 1;
    }
    if (!b && cap) out[0] = 0;
    return 0;
}

int mime_header(const char *hdr, size_t hlen, const char *name, char *out, size_t cap) {
    return header_find(hdr, hlen, name, NULL, out, cap);
}

char *mime_header_dup(const char *hdr, size_t hlen, const char *name) {
    Buf b = { 0 };
    if (!header_find(hdr, hlen, name, &b, NULL, 0)) { free(b.p); return NULL; }
    char *r = bdone(&b, NULL);
    if (!r) return NULL;
    char *s = r;
    while (*s == ' ' || *s == '\t') s++;
    if (s != r) memmove(r, s, strlen(s) + 1);
    return r;
}

/* Raw header text (without encoded words) to UTF-8. */
static void raw_header_utf8(Buf *b, const char *s, size_t n) {
    if (utf8_valid(s, n)) { bput(b, s, n); return; }
    size_t ol;
    const CsTable *w = cs_table("windows1252");
    char *u = from_table(w ? w->hi : NULL, s, n, &ol);
    if (u) { bput(b, u, ol); free(u); }
}

void mime_decode_words(const char *in, char *out, size_t cap) {
    if (!cap) return;
    Buf b = { 0 };
    size_t n = in ? strlen(in) : 0;
    size_t i = 0, lit = 0;
    int last_was_word = 0;
    while (i < n) {
        if (in[i] == '=' && i + 1 < n && in[i + 1] == '?') {
            /* =?charset?E?text?= */
            size_t q1 = i + 2;
            while (q1 < n && in[q1] != '?' && q1 - i < 70) q1++;
            if (q1 + 3 < n && in[q1] == '?' && in[q1 + 2] == '?') {
                char enc = (char)lc((unsigned char)in[q1 + 1]);
                size_t t = q1 + 3, te = t;
                while (te + 1 < n && !(in[te] == '?' && in[te + 1] == '=')) te++;
                if ((enc == 'b' || enc == 'q') && te + 1 < n) {
                    /* whitespace between two encoded words is ignored */
                    size_t ws = lit;
                    int only_ws = 1;
                    for (size_t k = lit; k < i; k++) if (in[k] != ' ' && in[k] != '\t' && in[k] != '\r' && in[k] != '\n') only_ws = 0;
                    if (!(last_was_word && only_ws)) raw_header_utf8(&b, in + ws, i - ws);
                    char cs[64];
                    size_t cl = q1 - (i + 2);
                    if (cl >= sizeof cs) cl = sizeof cs - 1;
                    memcpy(cs, in + i + 2, cl);
                    cs[cl] = 0;
                    size_t tl = te - t;
                    char *raw = malloc(tl + 4);
                    if (raw) {
                        size_t rl = enc == 'b' ? b64_decode(in + t, tl, (unsigned char *)raw) : qp_decode(in + t, tl, raw, 1);
                        size_t ul;
                        char *u = charset_to_utf8(cs, raw, rl, &ul);
                        if (u) { bput(&b, u, ul); free(u); }
                        free(raw);
                    }
                    i = te + 2;
                    lit = i;
                    last_was_word = 1;
                    continue;
                }
            }
        }
        i++;
        if (i - lit > 0 && in[i - 1] != ' ' && in[i - 1] != '\t' && in[i - 1] != '\r' && in[i - 1] != '\n') last_was_word = last_was_word && 0;
    }
    if (lit < n) raw_header_utf8(&b, in + lit, n - lit);
    char *r = bdone(&b, NULL);
    size_t o = 0;
    if (r) {
        for (size_t k = 0; r[k] && o + 1 < cap; k++) {
            unsigned char c = (unsigned char)r[k];
            if (c < 0x20 || c == 0x7f) { if (o && out[o - 1] != ' ') out[o++] = ' '; continue; }
            out[o++] = (char)c;
        }
        free(r);
    }
    out[o] = 0;
    utf8_fit(out, cap);
    /* trimmed */
    size_t s = 0;
    while (out[s] == ' ') s++;
    if (s) memmove(out, out + s, strlen(out + s) + 1);
    size_t l = strlen(out);
    while (l && out[l - 1] == ' ') out[--l] = 0;
}

/* RFC 2231: %XX with charset'language' at the start of the first piece */
static void pct_decode(Buf *b, const char *s, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (s[i] == '%' && i + 2 < n) {
            int h = hexv((unsigned char)s[i + 1]), l = hexv((unsigned char)s[i + 2]);
            if (h >= 0 && l >= 0) { bputc(b, (char)(h * 16 + l)); i += 2; continue; }
        }
        bputc(b, s[i]);
    }
}

int mime_param(const char *value, const char *name, char *out, size_t cap) {
    if (!cap) return 0;
    out[0] = 0;
    if (!value) return 0;
    size_t nl = strlen(name);
    const char *p = value;
    /* skip the main value */
    int inq = 0;
    while (*p && (inq || *p != ';')) { if (*p == '"') inq = !inq; else if (*p == '\\' && inq && p[1]) p++; p++; }
    struct { int idx, ext; char *val; } seg[32];
    int nseg = 0, found_plain = 0;
    char plain[1024] = "";
    while (*p == ';') {
        p++;
        while (*p == ' ' || *p == '\t' || *p == '\r' || *p == '\n') p++;
        const char *ka = p;
        while (*p && *p != '=' && *p != ';') p++;
        const char *ke = p;
        while (ke > ka && (ke[-1] == ' ' || ke[-1] == '\t')) ke--;
        if (*p != '=') continue;
        p++;
        while (*p == ' ' || *p == '\t') p++;
        Buf v = { 0 };
        if (*p == '"') {
            p++;
            while (*p && *p != '"') { if (*p == '\\' && p[1]) p++; bputc(&v, *p); p++; }
            if (*p == '"') p++;
            while (*p && *p != ';') p++;
        } else {
            const char *va = p;
            while (*p && *p != ';') p++;
            const char *ve = p;
            while (ve > va && (ve[-1] == ' ' || ve[-1] == '\t' || ve[-1] == '\r' || ve[-1] == '\n')) ve--;
            bput(&v, va, (size_t)(ve - va));
        }
        char *val = bdone(&v, NULL);
        if (!val) continue;
        size_t kl = (size_t)(ke - ka);
        if (kl == nl && !strncasecmp(ka, name, nl)) {
            snprintf(plain, sizeof plain, "%s", val);
            found_plain = 1;
            free(val);
        } else if (kl > nl && !strncasecmp(ka, name, nl) && ka[nl] == '*') {
            const char *r = ka + nl + 1;
            int idx = 0, ext = 0;
            if (r == ke) { ext = 1; idx = 0; }
            else {
                while (r < ke && *r >= '0' && *r <= '9') { idx = idx * 10 + (*r - '0'); r++; if (idx > 1000) break; }
                if (r < ke && *r == '*') ext = 1;
            }
            if (nseg < 32 && idx < 32) { seg[nseg].idx = idx; seg[nseg].ext = ext; seg[nseg].val = val; nseg++; }
            else free(val);
        } else free(val);
    }
    if (nseg) {
        Buf raw = { 0 };
        char cs[64] = "us-ascii";
        for (int want = 0; want < 32; want++) {
            int k;
            for (k = 0; k < nseg && seg[k].idx != want; k++) {}
            if (k == nseg) break;
            const char *v = seg[k].val;
            if (seg[k].ext) {
                if (want == 0) {
                    const char *q1 = strchr(v, '\'');
                    const char *q2 = q1 ? strchr(q1 + 1, '\'') : NULL;
                    if (q1 && q2) {
                        size_t cl = (size_t)(q1 - v);
                        if (cl && cl < sizeof cs) { memcpy(cs, v, cl); cs[cl] = 0; }
                        v = q2 + 1;
                    }
                }
                pct_decode(&raw, v, strlen(v));
            } else bputs(&raw, v);
        }
        for (int k = 0; k < nseg; k++) free(seg[k].val);
        size_t rl;
        char *r = bdone(&raw, &rl);
        if (r) {
            size_t ul;
            char *u = charset_to_utf8(cs, r, rl, &ul);
            free(r);
            if (u) {
                char tmp[1024];
                snprintf(tmp, sizeof tmp, "%s", u);
                free(u);
                mime_decode_words(tmp, out, cap);
                return 1;
            }
        }
    }
    if (found_plain) { mime_decode_words(plain, out, cap); return 1; }
    return 0;
}

/* ---- parts ---- */

static void parse_ctype(const char *hdr, size_t hlen, MimePart *p, const MimePart *parent) {
    char ct[1024], v[256];
    snprintf(p->type, sizeof p->type, "text");
    snprintf(p->subtype, sizeof p->subtype, "plain");
    if (parent && !strcmp(parent->type, "multipart") && !strcmp(parent->subtype, "digest")) {
        snprintf(p->type, sizeof p->type, "message");
        snprintf(p->subtype, sizeof p->subtype, "rfc822");
    }
    if (mime_header(hdr, hlen, "Content-Type", ct, sizeof ct)) {
        size_t k = 0;
        while (ct[k] && ct[k] != ';') k++;
        char main[160];
        copy_trim(main, sizeof main, ct, k);
        lower_ascii(main);
        char *slash = strchr(main, '/');
        if (slash && slash != main && slash[1]) {
            *slash = 0;
            copy_trim(p->type, sizeof p->type, main, strlen(main));
            copy_trim(p->subtype, sizeof p->subtype, slash + 1, strlen(slash + 1));
        }
        if (mime_param(ct, "charset", v, sizeof v)) { scpy(p->charset, sizeof p->charset, v); lower_ascii(p->charset); }
        if (mime_param(ct, "boundary", v, sizeof v)) scpy(p->boundary, sizeof p->boundary, v);
        if (mime_param(ct, "name", v, sizeof v)) snprintf(p->filename, sizeof p->filename, "%s", v);
        if (mime_param(ct, "method", v, sizeof v)) { scpy(p->method, sizeof p->method, v); for (char *c = p->method; *c; c++) *c = (char)toupper((unsigned char)*c); }
    }
    if (mime_header(hdr, hlen, "Content-Transfer-Encoding", v, sizeof v)) { copy_trim(p->encoding, sizeof p->encoding, v, strlen(v)); lower_ascii(p->encoding); }
    char cd[1024];
    if (mime_header(hdr, hlen, "Content-Disposition", cd, sizeof cd)) {
        size_t k = 0;
        while (cd[k] && cd[k] != ';') k++;
        copy_trim(p->disposition, sizeof p->disposition, cd, k);
        lower_ascii(p->disposition);
        if (mime_param(cd, "filename", v, sizeof v) && v[0]) snprintf(p->filename, sizeof p->filename, "%s", v);
    }
    if (mime_header(hdr, hlen, "Content-ID", v, sizeof v)) {
        char *a = v;
        while (*a == ' ' || *a == '<') a++;
        size_t l = strlen(a);
        while (l && (a[l - 1] == '>' || a[l - 1] == ' ')) l--;
        copy_trim(p->cid, sizeof p->cid, a, l);
    }
    /* file names: decoded first (RFC 2231/2047 above), then reduced to a safe basename */
    { char safe[256]; mime_safe_filename(p->filename, safe, sizeof safe); scpy(p->filename, sizeof p->filename, p->filename[0] ? safe : ""); }
}

/* Finds the next "--boundary" line starting at from. Returns the start of the line or (size_t)-1. */
static size_t find_boundary(const char *b, size_t len, size_t from, const char *bnd, size_t bl) {
    for (size_t i = from; i + 2 + bl <= len; i++) {
        if ((i == 0 || b[i - 1] == '\n') && b[i] == '-' && b[i + 1] == '-' && !memcmp(b + i + 2, bnd, bl)) return i;
        const char *nx = memchr(b + i, '\n', len - i);
        if (!nx) break;
        i = (size_t)(nx - b);
    }
    return (size_t)-1;
}

static void parse_part(MimeMsg *m, const char *data, size_t len, int parent, int depth) {
    if (m->nparts >= MIME_MAX_PARTS) { m->truncated = 1; return; }
    int idx = m->nparts++;
    MimePart *p = &m->parts[idx];
    memset(p, 0, sizeof *p);
    p->parent = parent;
    p->depth = depth;
    mime_split(data, len, &p->hdr, &p->hlen, &p->body, &p->blen);
    parse_ctype(p->hdr, p->hlen, p, parent >= 0 ? &m->parts[parent] : NULL);
    if (!strcmp(p->type, "multipart") && p->boundary[0] && depth + 1 < MIME_MAX_DEPTH) {
        const char *b = p->body;
        size_t bl = strlen(p->boundary), blen = p->blen;
        size_t pos = find_boundary(b, blen, 0, p->boundary, bl);
        while (pos != (size_t)-1) {
            size_t after = pos + 2 + bl;
            if (after + 1 < blen + 1 && after + 2 <= blen && b[after] == '-' && b[after + 1] == '-') break;   /* closing */
            const char *nl = memchr(b + after, '\n', blen - after);
            if (!nl) break;
            size_t start = (size_t)(nl - b) + 1;
            size_t next = find_boundary(b, blen, start, p->boundary, bl);
            size_t end = next == (size_t)-1 ? blen : next;
            /* the line break before the delimiter belongs to the delimiter */
            if (end > start && b[end - 1] == '\n') end--;
            if (end > start && b[end - 1] == '\r') end--;
            if (end < start) end = start;
            parse_part(m, b + start, end - start, idx, depth + 1);
            if (next == (size_t)-1 || m->nparts >= MIME_MAX_PARTS) break;
            pos = next;
        }
    } else if (!strcmp(p->type, "message") && !strcmp(p->subtype, "rfc822") && strcmp(p->disposition, "attachment") &&
               (!p->encoding[0] || !strcmp(p->encoding, "7bit") || !strcmp(p->encoding, "8bit") || !strcmp(p->encoding, "binary")) &&
               depth + 1 < MIME_MAX_DEPTH) {
        parse_part(m, p->body, p->blen, idx, depth + 1);
    }
}

void mime_parse(const char *msg, size_t len, MimeMsg *out) {
    out->nparts = 0;
    out->truncated = 0;
    parse_part(out, msg ? msg : "", msg ? len : 0, -1, 0);
}

char *mime_decode_body(const MimePart *p, size_t *olen) {
    char *out = malloc(p->blen + 4);
    if (!out) return NULL;
    size_t n;
    if (!strcmp(p->encoding, "base64")) n = b64_decode(p->body, p->blen, (unsigned char *)out);
    else if (!strcmp(p->encoding, "quoted-printable")) n = qp_decode(p->body, p->blen, out, 0);
    else { memcpy(out, p->body, p->blen); n = p->blen; }
    out[n] = 0;
    if (olen) *olen = n;
    return out;
}

char *mime_part_text(const MimePart *p, size_t *olen) {
    size_t n;
    char *raw = mime_decode_body(p, &n);
    if (!raw) return NULL;
    char *u = charset_to_utf8(p->charset[0] ? p->charset : "us-ascii", raw, n, olen);
    free(raw);
    return u;
}

/* ---- addresses ---- */

static int addrs_raw(const char *value, MimeAddr *out, int max);

int mime_addrs(const char *value, MimeAddr *out, int max) {
    if (!value) return 0;
    int n = addrs_raw(value, out, max);
    /* broken mailers encode the whole "Name <addr>" as one encoded word: decode, then parse */
    if (!n && strstr(value, "=?")) {
        char dec[2048];
        mime_decode_words(value, dec, sizeof dec);
        n = addrs_raw(dec, out, max);
    }
    return n;
}

static int addrs_raw(const char *value, MimeAddr *out, int max) {
    int n = 0;
    const char *s = value;
    while (*s && n < max) {
        /* one element up to the top-level comma */
        Buf name = { 0 }, email = { 0 }, comment = { 0 }, bare = { 0 };
        int inq = 0, angle = 0, par = 0, had_angle = 0;
        for (; *s; s++) {
            char c = *s;
            if (inq) {
                if (c == '\\' && s[1]) { s++; bputc(&name, *s); continue; }
                if (c == '"') { inq = 0; continue; }
                bputc(&name, c);
                continue;
            }
            if (par) {
                if (c == '\\' && s[1]) { s++; bputc(&comment, *s); continue; }
                if (c == '(') par++;
                else if (c == ')') { par--; if (!par) continue; }
                bputc(&comment, c);
                continue;
            }
            if (angle) { if (c == '>') { angle = 0; continue; } bputc(&email, c); continue; }
            if (c == '"') { inq = 1; continue; }
            if (c == '(') { par = 1; continue; }
            if (c == '<') { angle = 1; had_angle = 1; continue; }
            if (c == ',' || c == ';') { s++; break; }
            if (c == ':') {                       /* group name: discarded */
                free(name.p); free(bare.p);
                memset(&name, 0, sizeof name); memset(&bare, 0, sizeof bare);
                continue;
            }
            bputc(&bare, c);
            bputc(&name, c);
        }
        char *nm = bdone(&name, NULL), *em = bdone(&email, NULL), *cm = bdone(&comment, NULL), *br = bdone(&bare, NULL);
        MimeAddr a;
        memset(&a, 0, sizeof a);
        if (had_angle && em) {
            copy_trim(a.email, sizeof a.email, em, strlen(em));
            char tmp[512];
            copy_trim(tmp, sizeof tmp, nm ? nm : "", nm ? strlen(nm) : 0);
            if (!tmp[0] && cm) copy_trim(tmp, sizeof tmp, cm, strlen(cm));
            mime_decode_words(tmp, a.name, sizeof a.name);
        } else if (br) {
            copy_trim(a.email, sizeof a.email, br, strlen(br));
            /* unquoted "a@b" with spaces: keep the word containing @ */
            if (strchr(a.email, ' ')) {
                char *at = strchr(a.email, '@');
                if (at) {
                    char *st = at, *en = at;
                    while (st > a.email && st[-1] != ' ') st--;
                    while (*en && *en != ' ') en++;
                    memmove(a.email, st, (size_t)(en - st));
                    a.email[en - st] = 0;
                }
            }
            if (cm) { char tmp[512]; copy_trim(tmp, sizeof tmp, cm, strlen(cm)); mime_decode_words(tmp, a.name, sizeof a.name); }
        }
        free(nm); free(em); free(cm); free(br);
        /* only address-shaped addresses */
        for (char *c = a.email; *c; c++) if ((unsigned char)*c < 0x21 || *c == '<' || *c == '>' || *c == '"') { *c = 0; break; }
        if (a.email[0] && strchr(a.email, '@')) out[n++] = a;
        while (*s == ' ' || *s == '\t' || *s == ',' || *s == '\r' || *s == '\n') s++;
    }
    return n;
}

/* ---- dates ---- */

static long long days_from_civil(long long y, unsigned m, unsigned d) {
    y -= m <= 2;
    long long era = (y >= 0 ? y : y - 399) / 400;
    unsigned yoe = (unsigned)(y - era * 400);
    unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (long long)doe - 719468;
}

time_t mime_date(const char *v) {
    static const char *mon[] = { "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec" };
    if (!v) return 0;
    const char *s = v;
    int day = -1, month = -1, year = -1, hh = 0, mm = 0, ss = 0, have_time = 0;
    long zone = 0;
    /* tokens */
    while (*s) {
        while (*s == ' ' || *s == '\t' || *s == ',' || *s == '\r' || *s == '\n') s++;
        if (!*s) break;
        if (*s == '(') { int d = 0; while (*s) { if (*s == '(') d++; else if (*s == ')' && !--d) { s++; break; } s++; } continue; }
        const char *a = s;
        while (*s && *s != ' ' && *s != '\t' && *s != ',' && *s != '(') s++;
        size_t n = (size_t)(s - a);
        char tok[32];
        if (n >= sizeof tok) continue;
        memcpy(tok, a, n);
        tok[n] = 0;
        if (strchr(tok, ':') && !have_time) {
            if (sscanf(tok, "%d:%d:%d", &hh, &mm, &ss) >= 2) have_time = 1;
            continue;
        }
        if ((tok[0] == '+' || tok[0] == '-') && n == 5 && isdigit((unsigned char)tok[1])) {
            long z = atol(tok + 1);
            zone = (z / 100) * 3600 + (z % 100) * 60;
            if (tok[0] == '-') zone = -zone;
            continue;
        }
        if (isdigit((unsigned char)tok[0])) {
            long x = atol(tok);
            if (day < 0 && x >= 1 && x <= 31 && n <= 2) day = (int)x;
            else if (year < 0) year = (int)(n <= 2 ? (x < 50 ? 2000 + x : 1900 + x) : x);
            continue;
        }
        char low[32];
        for (size_t k = 0; k <= n; k++) low[k] = (char)lc((unsigned char)tok[k]);
        int found = 0;
        for (int k = 0; k < 12; k++) if (!strncmp(low, mon[k], 3)) { month = k + 1; found = 1; break; }
        if (found) continue;
        static const struct { const char *z; int h; } zones[] = { { "ut", 0 }, { "utc", 0 }, { "gmt", 0 }, { "z", 0 }, { "est", -5 }, { "edt", -4 },
            { "cst", -6 }, { "cdt", -5 }, { "mst", -7 }, { "mdt", -6 }, { "pst", -8 }, { "pdt", -7 } };
        for (size_t k = 0; k < sizeof zones / sizeof zones[0]; k++) if (!strcmp(low, zones[k].z)) zone = zones[k].h * 3600;
    }
    if (day < 1 || month < 1 || year < 1970 || year > 9999 || hh > 23 || mm > 59 || ss > 60) return 0;
    long long days = days_from_civil(year, (unsigned)month, (unsigned)day);
    long long t = days * 86400 + hh * 3600 + mm * 60 + ss - zone;
    return t > 0 ? (time_t)t : 0;
}

void mime_format_date(time_t t, char *out, size_t cap) {
    static const char *wd[] = { "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat" };
    static const char *mo[] = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" };
    struct tm tm;
    localtime_r(&t, &tm);
    long off = tm.tm_gmtoff;
    char sign = off < 0 ? '-' : '+';
    if (off < 0) off = -off;
    snprintf(out, cap, "%s, %d %s %d %02d:%02d:%02d %c%02ld%02ld", wd[tm.tm_wday % 7], tm.tm_mday, mo[tm.tm_mon % 12], tm.tm_year + 1900,
             tm.tm_hour, tm.tm_min, tm.tm_sec, sign, off / 3600, (off % 3600) / 60);
}

/* ---- building headers ---- */

static int needs_encoding(const char *s) {
    for (; *s; s++) if ((unsigned char)*s >= 0x7f || ((unsigned char)*s < 0x20 && *s != '\t')) return 1;
    return 0;
}

char *mime_encode_header(const char *u) {
    if (!u) u = "";
    if (!needs_encoding(u) && !strstr(u, "=?")) return strdup(u);
    Buf b = { 0 };
    size_t n = strlen(u), i = 0;
    int first = 1;
    while (i < n) {
        size_t take = 0;
        while (i + take < n && take < 42) {
            int k = utf8_len((const unsigned char *)u + i + take, n - i - take);
            if (!k) k = 1;
            if (take + (size_t)k > 42) break;
            take += (size_t)k;
        }
        size_t el;
        char *e = b64_encode((const unsigned char *)u + i, take, 0, &el);
        if (!e) break;
        if (!first) bputs(&b, "\r\n ");
        bputs(&b, "=?UTF-8?B?");
        bput(&b, e, el);
        bputs(&b, "?=");
        free(e);
        first = 0;
        i += take;
    }
    return bdone(&b, NULL);
}

char *mime_format_addr(const char *name, const char *email) {
    Buf b = { 0 };
    if (name && *name) {
        if (needs_encoding(name)) {
            char *e = mime_encode_header(name);
            if (e) { bputs(&b, e); free(e); }
        } else if (strpbrk(name, "()<>[]:;@\\,.\"")) {
            bputc(&b, '"');
            for (const char *c = name; *c; c++) { if (*c == '"' || *c == '\\') bputc(&b, '\\'); bputc(&b, *c); }
            bputc(&b, '"');
        } else bputs(&b, name);
        bputs(&b, " <");
        bputs(&b, email ? email : "");
        bputc(&b, '>');
    } else bputs(&b, email ? email : "");
    return bdone(&b, NULL);
}

void mime_base_subject(const char *subj, char *out, size_t cap) {
    static const char *pre[] = { "re", "fw", "fwd", "rv", "aw", "wg", "sv", "vs", "tr", "ref", "res", "enc", "antw", "odp", "r", NULL };
    const char *s = subj ? subj : "";
    for (int guard = 0; guard < 20; guard++) {
        while (*s == ' ' || *s == '\t') s++;
        int hit = 0;
        for (int k = 0; pre[k]; k++) {
            size_t l = strlen(pre[k]);
            if (strncasecmp(s, pre[k], l)) continue;
            const char *t = s + l;
            if (*t == '[' || *t == '(') { char close = *t == '[' ? ']' : ')'; t++; while (isdigit((unsigned char)*t)) t++; if (*t != close) continue; t++; }
            while (*t == ' ') t++;
            if (*t == ':' || (unsigned char)t[0] == 0xef) {         /* also the full-width colon (：) */
                if (*t == ':') t++;
                else if ((unsigned char)t[1] == 0xbc && (unsigned char)t[2] == 0x9a) t += 3;
                else continue;
                s = t;
                hit = 1;
                break;
            }
        }
        if (!hit) break;
    }
    copy_trim(out, cap, s, strlen(s));
}

/* A safe basename for a file name chosen by whoever sent the message: the last path component
 * (both / and \\), no leading dots or spaces, no control, bidi-override or FAT-reserved
 * characters (<>:"|?*), at most 200 bytes without cutting a UTF-8 sequence. Never empty. */
void mime_safe_filename(const char *in, char *out, size_t cap) {
    if (!cap) return;
    const char *s = in ? in : "";
    for (const char *p = s; *p; p++) if (*p == '/' || *p == '\\') s = p + 1;
    size_t o = 0, limit = cap - 1 < 200 ? cap - 1 : 200;
    while (*s == '.' || *s == ' ' || *s == '\t') s++;
    while (*s && o < limit) {
        unsigned char c = (unsigned char)*s;
        int k = utf8_len((const unsigned char *)s, strlen(s));
        if (!k) { if (o + 1 > limit) break; out[o++] = '_'; s++; continue; }
        uint32_t cp = c;
        if (k == 2) cp = ((c & 0x1fu) << 6) | (s[1] & 0x3fu);
        else if (k == 3) cp = ((c & 0x0fu) << 12) | ((s[1] & 0x3fu) << 6) | (s[2] & 0x3fu);
        else if (k == 4) cp = ((c & 0x07u) << 18) | ((s[1] & 0x3fu) << 12) | ((s[2] & 0x3fu) << 6) | (s[3] & 0x3fu);
        int bad = cp < 0x20 || cp == 0x7f || (cp >= 0x80 && cp < 0xa0) || strchr("<>:\"|?*", (int)(cp < 0x80 ? cp : 1)) != NULL ||
                  (cp >= 0x202a && cp <= 0x202e) || (cp >= 0x2066 && cp <= 0x2069) || cp == 0x200e || cp == 0x200f || cp == 0x061c || cp == 0xfeff;
        if (bad) { out[o++] = '_'; s += k; continue; }
        if (o + (size_t)k > limit) break;
        memcpy(out + o, s, (size_t)k);
        o += (size_t)k;
        s += k;
    }
    while (o && (out[o - 1] == ' ' || out[o - 1] == '.')) o--;      /* "name." / "name " are trouble on FAT */
    out[o] = 0;
    if (!o || strspn(out, "._ ") == o) snprintf(out, cap, "attachment");
}
