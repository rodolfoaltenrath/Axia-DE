/* xml.c — see xml.h. A single pass over the input, without recursion. */
#include "xml.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    MxNode node;
    char *qname;              /* prefixed name, to compare against the closing tag */
} Elem;

typedef struct { char *prefix, *uri; int depth; } Bind;
#define MAX_BINDS 512

typedef struct {
    const char *s;
    size_t len, i;
    const char *err;
    Bind binds[MAX_BINDS];
    int nbinds;
    long nodes;
} P;

static int is_ws(char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; }
static int is_name(unsigned char c) { return c > 0x7f || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.' || c == ':'; }

static int starts(const P *p, const char *lit) {
    size_t n = strlen(lit);
    return p->len - p->i >= n && !memcmp(p->s + p->i, lit, n);
}

/* finds lit starting at i; returns the position or (size_t)-1 */
static size_t find(const P *p, size_t from, const char *lit) {
    size_t n = strlen(lit);
    if (from > p->len || p->len - from < n) return (size_t)-1;
    for (size_t k = from; k + n <= p->len; k++) if (p->s[k] == lit[0] && !memcmp(p->s + k, lit, n)) return k;
    return (size_t)-1;
}

static int put_utf8(char *o, uint32_t cp) {
    if (cp < 0x80) { o[0] = (char)cp; return 1; }
    if (cp < 0x800) { o[0] = (char)(0xc0 | (cp >> 6)); o[1] = (char)(0x80 | (cp & 0x3f)); return 2; }
    if (cp < 0x10000) { o[0] = (char)(0xe0 | (cp >> 12)); o[1] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[2] = (char)(0x80 | (cp & 0x3f)); return 3; }
    o[0] = (char)(0xf0 | (cp >> 18)); o[1] = (char)(0x80 | ((cp >> 12) & 0x3f)); o[2] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[3] = (char)(0x80 | (cp & 0x3f));
    return 4;
}

/* Copies s[a..b) to out, decoding entities. out must hold at least b-a bytes (an entity
 * never takes more space decoded than encoded). Returns the length or -1 on a bad one. */
static long decode(const char *s, size_t a, size_t b, char *out) {
    size_t o = 0;
    for (size_t k = a; k < b;) {
        if (s[k] != '&') { out[o++] = s[k++]; continue; }
        size_t e = k + 1;
        while (e < b && e - k < 12 && s[e] != ';') e++;
        if (e >= b || s[e] != ';') return -1;
        const char *ent = s + k + 1;
        size_t el = e - k - 1;
        if (el == 2 && !memcmp(ent, "lt", 2)) out[o++] = '<';
        else if (el == 2 && !memcmp(ent, "gt", 2)) out[o++] = '>';
        else if (el == 3 && !memcmp(ent, "amp", 3)) out[o++] = '&';
        else if (el == 4 && !memcmp(ent, "quot", 4)) out[o++] = '"';
        else if (el == 4 && !memcmp(ent, "apos", 4)) out[o++] = '\'';
        else if (el >= 2 && ent[0] == '#') {
            uint32_t cp = 0;
            int hex = ent[1] == 'x' || ent[1] == 'X';
            size_t d0 = hex ? 2 : 1;
            if (el <= d0) return -1;
            for (size_t d = d0; d < el; d++) {
                char c = ent[d];
                uint32_t v;
                if (c >= '0' && c <= '9') v = (uint32_t)(c - '0');
                else if (hex && c >= 'a' && c <= 'f') v = (uint32_t)(c - 'a' + 10);
                else if (hex && c >= 'A' && c <= 'F') v = (uint32_t)(c - 'A' + 10);
                else return -1;
                cp = cp * (hex ? 16u : 10u) + v;
                if (cp > 0x10ffff) return -1;
            }
            if (!cp || (cp >= 0xd800 && cp <= 0xdfff)) return -1;
            /* "&#N;" is ≥ 4 bytes and the UTF-8 of N takes ≤ 4: it always fits */
            o += (size_t)put_utf8(out + o, cp);
        } else return -1;
        k = e + 1;
    }
    return (long)o;
}

static void append_text(MxNode *n, const char *t, size_t len) {
    if (!n || !len) return;
    char *nt = realloc(n->text, n->tlen + len + 1);
    if (!nt) return;
    memcpy(nt + n->tlen, t, len);
    n->tlen += len;
    nt[n->tlen] = 0;
    n->text = nt;
}

static const char *resolve(P *p, const char *qname, size_t plen, int is_attr) {
    if (plen == 3 && !memcmp(qname, "xml", 3)) return "http://www.w3.org/XML/1998/namespace";
    for (int k = p->nbinds - 1; k >= 0; k--) {
        const char *pf = p->binds[k].prefix;
        if (strlen(pf) == plen && !memcmp(pf, qname, plen)) return p->binds[k].uri;
    }
    (void)is_attr;
    return "";
}

void mx_free(MxNode *root) {
    MxNode *n = root;
    while (n) {
        if (n->kid) { MxNode *k = n->kid; n->kid = NULL; n = k; continue; }
        MxNode *next = n->next, *parent = n->parent;
        int last = n == root;
        free(n->text);
        free(((Elem *)n)->qname);
        free(n);
        if (last) break;
        n = next ? next : parent;
    }
}

static void pop_binds(P *p, int depth) {
    while (p->nbinds && p->binds[p->nbinds - 1].depth >= depth) {
        p->nbinds--;
        free(p->binds[p->nbinds].prefix);
        free(p->binds[p->nbinds].uri);
    }
}

MxNode *mx_parse(const char *s, size_t len, const char **err) {
    P p;
    memset(&p, 0, sizeof p);
    p.s = s; p.len = len;
    MxNode *root = NULL, *cur = NULL;
    int depth = 0;
    char *tmp = NULL;
#define FAIL(m) do { p.err = (m); goto fail; } while (0)
    if (!s || len > MX_MAX_INPUT) FAIL("input");
    if (len >= 3 && (unsigned char)s[0] == 0xef && (unsigned char)s[1] == 0xbb && (unsigned char)s[2] == 0xbf) p.i = 3;
    while (p.i < len) {
        if (s[p.i] != '<') {
            size_t a = p.i;
            while (p.i < len && s[p.i] != '<') p.i++;
            if (!cur) {
                for (size_t k = a; k < p.i; k++) if (!is_ws(s[k])) FAIL("text outside element");
                continue;
            }
            tmp = malloc(p.i - a + 1);
            if (!tmp) FAIL("memory");
            long n = decode(s, a, p.i, tmp);
            if (n < 0) FAIL("entity");
            append_text(cur, tmp, (size_t)n);
            free(tmp); tmp = NULL;
            continue;
        }
        if (starts(&p, "<?")) {
            size_t e = find(&p, p.i + 2, "?>");
            if (e == (size_t)-1) FAIL("unclosed instruction");
            p.i = e + 2;
            continue;
        }
        if (starts(&p, "<!--")) {
            size_t e = find(&p, p.i + 4, "-->");
            if (e == (size_t)-1) FAIL("unclosed comment");
            p.i = e + 3;
            continue;
        }
        if (starts(&p, "<![CDATA[")) {
            if (!cur) FAIL("CDATA outside element");
            size_t e = find(&p, p.i + 9, "]]>");
            if (e == (size_t)-1) FAIL("unclosed CDATA");
            append_text(cur, s + p.i + 9, e - (p.i + 9));
            p.i = e + 3;
            continue;
        }
        if (starts(&p, "<!")) FAIL("DOCTYPE not allowed");
        if (starts(&p, "</")) {
            p.i += 2;
            size_t a = p.i;
            while (p.i < len && is_name((unsigned char)s[p.i])) p.i++;
            size_t nl = p.i - a;
            while (p.i < len && is_ws(s[p.i])) p.i++;
            if (p.i >= len || s[p.i] != '>') FAIL("malformed closing tag");
            p.i++;
            if (!cur) FAIL("closing tag without opening");
            const char *q = ((Elem *)cur)->qname;
            if (strlen(q) != nl || memcmp(q, s + a, nl)) FAIL("mismatched closing tag");
            pop_binds(&p, depth);
            depth--;
            cur = cur->parent;
            continue;
        }
        /* opening tag */
        p.i++;
        size_t a = p.i;
        while (p.i < len && is_name((unsigned char)s[p.i])) p.i++;
        size_t ql = p.i - a;
        if (!ql) FAIL("empty name");
        if (cur == NULL && root) FAIL("two roots");
        if (depth >= MX_MAX_DEPTH) FAIL("too deep");
        if (++p.nodes > MX_MAX_NODES) FAIL("too many elements");
        size_t qa = a;
        int selfclose = 0;
        for (;;) {
            while (p.i < len && is_ws(s[p.i])) p.i++;
            if (p.i >= len) FAIL("unclosed tag");
            if (s[p.i] == '>') { p.i++; break; }
            if (s[p.i] == '/') {
                if (p.i + 1 >= len || s[p.i + 1] != '>') FAIL("stray /");
                p.i += 2; selfclose = 1; break;
            }
            size_t an = p.i;
            while (p.i < len && is_name((unsigned char)s[p.i])) p.i++;
            size_t anl = p.i - an;
            if (!anl) FAIL("attribute");
            while (p.i < len && is_ws(s[p.i])) p.i++;
            if (p.i >= len || s[p.i] != '=') FAIL("attribute without =");
            p.i++;
            while (p.i < len && is_ws(s[p.i])) p.i++;
            if (p.i >= len || (s[p.i] != '"' && s[p.i] != '\'')) FAIL("unquoted value");
            char qc = s[p.i++];
            size_t va = p.i;
            while (p.i < len && s[p.i] != qc) { if (s[p.i] == '<') FAIL("< in an attribute"); p.i++; }
            if (p.i >= len) FAIL("unclosed value");
            size_t vb = p.i++;
            int is_def = anl == 5 && !memcmp(s + an, "xmlns", 5);
            int is_pre = anl > 6 && !memcmp(s + an, "xmlns:", 6);
            if (is_def || is_pre) {
                if (p.nbinds >= MAX_BINDS) FAIL("too many namespaces");
                char *uri = malloc(vb - va + 1);
                if (!uri) FAIL("memory");
                long ul = decode(s, va, vb, uri);
                if (ul < 0) { free(uri); FAIL("entity"); }
                uri[ul] = 0;
                size_t pl = is_def ? 0 : anl - 6;
                char *pf = malloc(pl + 1);
                if (!pf) { free(uri); FAIL("memory"); }
                memcpy(pf, s + an + (is_def ? 0 : 6), pl);
                pf[pl] = 0;
                p.binds[p.nbinds++] = (Bind){ pf, uri, depth + 1 };
            } else {
                /* other attributes are validated (entities) and discarded: DAV does not use them */
                tmp = malloc(vb - va + 1);
                if (!tmp) FAIL("memory");
                if (decode(s, va, vb, tmp) < 0) FAIL("entity");
                free(tmp); tmp = NULL;
            }
        }
        /* resolved name */
        const char *colon = memchr(s + qa, ':', ql);
        size_t plen = colon ? (size_t)(colon - (s + qa)) : 0;
        const char *local = colon ? colon + 1 : s + qa;
        size_t ll = ql - (size_t)(local - (s + qa));
        if (!ll) FAIL("empty local name");
        const char *ns = resolve(&p, s + qa, plen, 0);
        size_t nsl = strlen(ns);
        Elem *e = calloc(1, sizeof *e + nsl + 1 + ll + 1);
        if (!e) FAIL("memory");
        e->qname = malloc(ql + 1);
        if (!e->qname) { free(e); FAIL("memory"); }
        memcpy(e->qname, s + qa, ql);
        e->qname[ql] = 0;
        char *store = (char *)(e + 1);
        memcpy(store, ns, nsl + 1);
        memcpy(store + nsl + 1, local, ll);
        store[nsl + 1 + ll] = 0;
        e->node.ns = store;
        e->node.name = store + nsl + 1;
        MxNode *n = &e->node;
        n->parent = cur;
        if (cur) { if (cur->last) cur->last->next = n; else cur->kid = n; cur->last = n; }
        else root = n;
        if (selfclose) pop_binds(&p, depth + 1);
        else { cur = n; depth++; }
    }
    if (cur || !root) FAIL("incomplete document");
    pop_binds(&p, 0);
    if (err) *err = NULL;
    return root;
fail:
    free(tmp);
    pop_binds(&p, 0);
    if (root) mx_free(root);
    if (err) *err = p.err ? p.err : "error";
    return NULL;
#undef FAIL
}

static int match(const MxNode *n, const char *ns, const char *name) {
    return (!ns || !strcmp(n->ns, ns)) && !strcmp(n->name, name);
}

MxNode *mx_child(const MxNode *n, const char *ns, const char *name) {
    for (MxNode *k = n ? n->kid : NULL; k; k = k->next) if (match(k, ns, name)) return k;
    return NULL;
}

MxNode *mx_next(const MxNode *n, const char *ns, const char *name) {
    for (MxNode *k = n ? n->next : NULL; k; k = k->next) if (match(k, ns, name)) return k;
    return NULL;
}

MxNode *mx_find(const MxNode *top, const char *ns, const char *name) {
    if (!top) return NULL;
    const MxNode *n = top->kid;
    while (n) {
        if (match(n, ns, name)) return (MxNode *)n;
        if (n->kid) { n = n->kid; continue; }
        while (n && n != top && !n->next) n = n->parent;
        if (!n || n == top) break;
        n = n->next;
    }
    return NULL;
}

const char *mx_text(const MxNode *n) { return n && n->text ? n->text : ""; }

void mx_child_text(const MxNode *n, const char *ns, const char *name, char *out, size_t cap) {
    if (!cap) return;
    out[0] = 0;
    const MxNode *k = mx_child(n, ns, name);
    if (!k || !k->text) return;
    const char *t = k->text;
    size_t l = k->tlen;
    while (l && is_ws(*t)) { t++; l--; }
    while (l && is_ws(t[l - 1])) l--;
    if (l >= cap) l = cap - 1;
    memcpy(out, t, l);
    out[l] = 0;
}

size_t mx_escape(char *out, size_t cap, const char *s) {
    size_t o = 0;
    if (!cap) return 0;
    for (; s && *s; s++) {
        const char *r = NULL;
        char one[2] = { *s, 0 };
        switch (*s) {
        case '&': r = "&amp;"; break;
        case '<': r = "&lt;"; break;
        case '>': r = "&gt;"; break;
        case '"': r = "&quot;"; break;
        default: r = one;
        }
        size_t rl = strlen(r);
        if (o + rl >= cap) break;
        memcpy(out + o, r, rl);
        o += rl;
    }
    out[o] = 0;
    return o;
}
