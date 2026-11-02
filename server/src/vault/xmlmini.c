/* xmlmini.c — minimal XML reader (see xmlmini.h). */
#include "xmlmini.h"
#include "smem.h"

#include <stdint.h>
#include <string.h>

#define MAX_DEPTH 256
#define MAX_NODES (4 * 1024 * 1024)

typedef struct { const char *s; size_t len, p; const char *err; long nodes; } X;

static int is_ws(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r'; }
static int is_name(char c) { return !is_ws(c) && c != '>' && c != '/' && c != '=' && c != '<' && c != '"' && c != '\'' && c; }

static size_t put_utf8(char *d, uint32_t cp) {
    if (cp < 0x80) { d[0] = (char)cp; return 1; }
    if (cp < 0x800) { d[0] = (char)(0xc0 | cp >> 6); d[1] = (char)(0x80 | (cp & 63)); return 2; }
    if (cp < 0x10000) { d[0] = (char)(0xe0 | cp >> 12); d[1] = (char)(0x80 | ((cp >> 6) & 63)); d[2] = (char)(0x80 | (cp & 63)); return 3; }
    d[0] = (char)(0xf0 | cp >> 18); d[1] = (char)(0x80 | ((cp >> 12) & 63)); d[2] = (char)(0x80 | ((cp >> 6) & 63)); d[3] = (char)(0x80 | (cp & 63));
    return 4;
}

/* Appends s[a..b) to *buf (decoding entities if decode). The result is never longer than the
 * input (an entity takes at least as much space as its UTF-8). */
static int append(char **buf, size_t *n, const char *s, size_t a, size_t b, int decode) {
    char *d = sm_realloc(*buf, *n + (b - a) + 1);
    if (!d) return -1;
    *buf = d;
    size_t o = *n;
    for (size_t i = a; i < b; i++) {
        if (!decode || s[i] != '&') { d[o++] = s[i]; continue; }
        size_t e = i + 1;
        while (e < b && e - i < 12 && s[e] != ';') e++;
        if (e >= b || s[e] != ';') return -1;
        const char *ent = s + i + 1;
        size_t el = e - i - 1;
        if (el == 2 && !memcmp(ent, "lt", 2)) d[o++] = '<';
        else if (el == 2 && !memcmp(ent, "gt", 2)) d[o++] = '>';
        else if (el == 3 && !memcmp(ent, "amp", 3)) d[o++] = '&';
        else if (el == 4 && !memcmp(ent, "quot", 4)) d[o++] = '"';
        else if (el == 4 && !memcmp(ent, "apos", 4)) d[o++] = '\'';
        else if (el >= 2 && ent[0] == '#') {
            uint32_t cp = 0;
            int hex = ent[1] == 'x' || ent[1] == 'X';
            size_t k = hex ? 2 : 1;
            if (k >= el) return -1;
            for (; k < el; k++) {
                char c = ent[k];
                int v = c >= '0' && c <= '9' ? c - '0' : hex && c >= 'a' && c <= 'f' ? c - 'a' + 10 : hex && c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
                if (v < 0) return -1;
                cp = cp * (hex ? 16u : 10u) + (uint32_t)v;
                if (cp > 0x10ffff) return -1;
            }
            if (cp == 0 || (cp >= 0xd800 && cp < 0xe000)) cp = 0xfffd;
            o += put_utf8(d + o, cp);
        } else return -1;
        i = e;
    }
    d[o] = 0;
    *n = o;
    return 0;
}

static XNode *node_new(X *x, XNode *parent) {
    if (++x->nodes > MAX_NODES) { x->err = "too-big"; return NULL; }
    XNode *n = sm_alloc(sizeof *n);
    if (!n) { x->err = "memory"; return NULL; }
    n->parent = parent;
    if (parent) {
        if (parent->nkids == parent->cap) {
            int nc = parent->cap ? parent->cap * 2 : 4;
            XNode **k = sm_realloc(parent->kids, sizeof(XNode *) * (size_t)nc);
            if (!k) { sm_free(n); x->err = "memory"; return NULL; }
            parent->kids = k;
            parent->cap = nc;
        }
        parent->kids[parent->nkids++] = n;
    }
    return n;
}

static int starts(X *x, const char *w) { size_t n = strlen(w); return x->p + n <= x->len && !memcmp(x->s + x->p, w, n); }
static int skip_to(X *x, const char *w) {
    size_t n = strlen(w);
    while (x->p + n <= x->len) { if (!memcmp(x->s + x->p, w, n)) { x->p += n; return 0; } x->p++; }
    return -1;
}

/* <name attributes> or </name>: x->p en '<' */
static int parse_tag(X *x, XNode **cur, XNode **root, int *depth) {
    const char *s = x->s;
    x->p++;
    if (x->p < x->len && s[x->p] == '/') {
        x->p++;
        size_t a = x->p;
        while (x->p < x->len && is_name(s[x->p])) x->p++;
        size_t nl = x->p - a;
        while (x->p < x->len && is_ws(s[x->p])) x->p++;
        if (x->p >= x->len || s[x->p] != '>') return -1;
        x->p++;
        if (!*cur || strlen((*cur)->name) != nl || memcmp((*cur)->name, s + a, nl)) return -1;
        *cur = (*cur)->parent;
        (*depth)--;
        return 0;
    }
    if (!*cur && *root) return -1;                 /* a second root element */
    size_t a = x->p;
    while (x->p < x->len && is_name(s[x->p])) x->p++;
    if (x->p == a) return -1;
    XNode *n = node_new(x, *cur);
    if (!n) return -1;
    if (!*root) *root = n;
    n->name = sm_strndup(s + a, x->p - a);
    if (!n->name) { x->err = "memory"; return -1; }
    for (;;) {
        while (x->p < x->len && is_ws(s[x->p])) x->p++;
        if (x->p >= x->len) return -1;
        if (s[x->p] == '>') { x->p++; break; }
        if (s[x->p] == '/') {
            if (x->p + 1 >= x->len || s[x->p + 1] != '>') return -1;
            x->p += 2;
            return 0;                              /* empty: not entered */
        }
        size_t na = x->p;
        while (x->p < x->len && is_name(s[x->p])) x->p++;
        if (x->p == na) return -1;
        size_t nb = x->p;
        while (x->p < x->len && is_ws(s[x->p])) x->p++;
        if (x->p >= x->len || s[x->p] != '=') return -1;
        x->p++;
        while (x->p < x->len && is_ws(s[x->p])) x->p++;
        if (x->p >= x->len || (s[x->p] != '"' && s[x->p] != '\'')) return -1;
        char q = s[x->p++];
        size_t va = x->p;
        while (x->p < x->len && s[x->p] != q) { if (s[x->p] == '<') return -1; x->p++; }
        if (x->p >= x->len) return -1;
        size_t vb = x->p++;
        if (n->nattrs >= 64) return -1;
        XAttr *at = sm_realloc(n->attrs, sizeof(XAttr) * (size_t)(n->nattrs + 1));
        if (!at) { x->err = "memory"; return -1; }
        n->attrs = at;
        XAttr *t = &at[n->nattrs];
        t->name = sm_strndup(s + na, nb - na);
        t->value = NULL;
        size_t vl = 0;
        n->nattrs++;
        if (!t->name || append(&t->value, &vl, s, va, vb, 1)) return -1;
    }
    if (++(*depth) > MAX_DEPTH) { x->err = "too-deep"; return -1; }
    *cur = n;
    return 0;
}

XNode *xml_parse(const char *s, size_t len, const char **err) {
    X x = { s, len, 0, NULL, 0 };
    XNode *root = NULL, *cur = NULL;
    int depth = 0;
    if (len >= 3 && !memcmp(s, "\xef\xbb\xbf", 3)) x.p = 3;
    while (x.p < len) {
        if (s[x.p] == '<') {
            if (starts(&x, "<?")) { if (skip_to(&x, "?>")) goto bad; continue; }
            if (starts(&x, "<!--")) { if (skip_to(&x, "-->")) goto bad; continue; }
            if (starts(&x, "<![CDATA[")) {
                if (!cur) goto bad;
                size_t a = x.p + 9;
                x.p = a;
                if (skip_to(&x, "]]>")) goto bad;
                if (append(&cur->text, &cur->tlen, s, a, x.p - 3, 0)) goto bad;
                continue;
            }
            if (starts(&x, "<!")) { x.err = "doctype"; goto bad; }   /* DOCTYPE/ENTITY: never */
            if (parse_tag(&x, &cur, &root, &depth)) goto bad;
            if (root && !cur && depth == 0) {
                /* root closed: only whitespace, comments or PIs may follow */
                while (x.p < len) {
                    if (is_ws(s[x.p])) { x.p++; continue; }
                    if (starts(&x, "<!--")) { if (skip_to(&x, "-->")) goto bad; continue; }
                    if (starts(&x, "<?")) { if (skip_to(&x, "?>")) goto bad; continue; }
                    goto bad;
                }
                if (err) *err = NULL;
                return root;
            }
            continue;
        }
        size_t a = x.p;
        while (x.p < len && s[x.p] != '<') x.p++;
        if (!cur) {
            for (size_t i = a; i < x.p; i++) if (!is_ws(s[i])) goto bad;
            continue;
        }
        if (append(&cur->text, &cur->tlen, s, a, x.p, 1)) goto bad;
    }
bad:
    if (err) *err = x.err ? x.err : "corrupt";
    xml_free(root);
    return NULL;
}

void xml_free(XNode *n) {
    if (!n) return;
    /* recursive: depth is already bounded (MAX_DEPTH) */
    for (int i = 0; i < n->nkids; i++) xml_free(n->kids[i]);
    for (int i = 0; i < n->nattrs; i++) { sm_free(n->attrs[i].name); sm_free(n->attrs[i].value); }
    sm_free(n->attrs);
    sm_free(n->kids);
    sm_free(n->name);
    sm_free(n->text);
    sm_free(n);
}

XNode *xml_child(const XNode *n, const char *name) {
    if (!n) return NULL;
    for (int i = 0; i < n->nkids; i++) if (!strcmp(n->kids[i]->name, name)) return n->kids[i];
    return NULL;
}

const char *xml_attr(const XNode *n, const char *name) {
    if (!n) return NULL;
    for (int i = 0; i < n->nattrs; i++) if (!strcmp(n->attrs[i].name, name)) return n->attrs[i].value ? n->attrs[i].value : "";
    return NULL;
}

const char *xml_text(const XNode *n) { return n && n->text ? n->text : ""; }

void xml_set_text(XNode *n, char *text, size_t len) {
    sm_free(n->text);
    n->text = text;
    n->tlen = len;
}
