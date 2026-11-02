/* ipp.c — see ipp.h. */
#include "ipp.h"

#include <stdlib.h>
#include <string.h>

#define IPP_MAX_ATTRS  20000
#define IPP_MAX_VALUES 60000
#define IPP_MAX_DEPTH  8

/* ---- encoding ---- */

static void put(IppBuf *b, const void *d, size_t n) {
    if (b->err) return;
    if (b->n + n > b->cap) {
        size_t cap = b->cap ? b->cap * 2 : 512;
        while (cap < b->n + n) cap *= 2;
        if (cap > (64u << 20)) { b->err = 1; return; }
        uint8_t *p = realloc(b->p, cap);
        if (!p) { b->err = 1; return; }
        b->p = p;
        b->cap = cap;
    }
    memcpy(b->p + b->n, d, n);
    b->n += n;
}
static void put8(IppBuf *b, uint8_t v) { put(b, &v, 1); }
static void put16(IppBuf *b, uint16_t v) { uint8_t x[2] = { (uint8_t)(v >> 8), (uint8_t)v }; put(b, x, 2); }
static void put32(IppBuf *b, uint32_t v) { uint8_t x[4] = { (uint8_t)(v >> 24), (uint8_t)(v >> 16), (uint8_t)(v >> 8), (uint8_t)v }; put(b, x, 4); }

void ipp_init(IppBuf *b, uint16_t op, uint32_t id) {
    memset(b, 0, sizeof *b);
    put8(b, 1);
    put8(b, 1);
    put16(b, op);
    put32(b, id);
}
void ipp_free(IppBuf *b) { free(b->p); memset(b, 0, sizeof *b); }
void ipp_group(IppBuf *b, uint8_t tag) { put8(b, tag); }

static void put_name(IppBuf *b, uint8_t tag, const char *name) {
    size_t n = name ? strlen(name) : 0;
    if (n > 0x7fff) { b->err = 1; return; }
    put8(b, tag);
    put16(b, (uint16_t)n);
    if (n) put(b, name, n);
}

void ipp_add_str(IppBuf *b, uint8_t tag, const char *name, const char *value) {
    size_t n = value ? strlen(value) : 0;
    if (n > 0x7fff) { b->err = 1; return; }
    put_name(b, tag, name);
    put16(b, (uint16_t)n);
    if (n) put(b, value, n);
}

void ipp_add_int(IppBuf *b, uint8_t tag, const char *name, int32_t v) {
    put_name(b, tag, name);
    put16(b, 4);
    put32(b, (uint32_t)v);
}

void ipp_add_bool(IppBuf *b, const char *name, int v) {
    put_name(b, IPP_T_BOOLEAN, name);
    put16(b, 1);
    put8(b, v ? 1 : 0);
}

void ipp_add_range(IppBuf *b, const char *name, int32_t lo, int32_t hi) {
    put_name(b, IPP_T_RANGE, name);
    put16(b, 8);
    put32(b, (uint32_t)lo);
    put32(b, (uint32_t)hi);
}

void ipp_end(IppBuf *b) { put8(b, IPP_G_END); }

void ipp_std(IppBuf *b, const char *printer_uri, const char *user) {
    ipp_group(b, IPP_G_OPERATION);
    ipp_add_str(b, IPP_T_CHARSET, "attributes-charset", "utf-8");
    ipp_add_str(b, IPP_T_LANGUAGE, "attributes-natural-language", "en");
    if (printer_uri) ipp_add_str(b, IPP_T_URI, "printer-uri", printer_uri);
    if (user) ipp_add_str(b, IPP_T_NAME, "requesting-user-name", user);
}

/* ---- decoding ---- */

typedef struct { const uint8_t *p; size_t n, i; int attrs, vals; } Rd;

static int rd16(Rd *r, uint16_t *v) { if (r->i + 2 > r->n) return -1; *v = (uint16_t)(r->p[r->i] << 8 | r->p[r->i + 1]); r->i += 2; return 0; }
static int32_t be32(const uint8_t *p) { return (int32_t)((uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]); }

static int is_text(uint8_t t) { return t >= 0x40 && t <= 0x5f; }

static void vals_free(IppVal *v);
static void attrs_free(IppAttr *a) {
    while (a) {
        IppAttr *n = a->next;
        free(a->name);
        vals_free(a->vals);
        free(a);
        a = n;
    }
}
static void vals_free(IppVal *v) {
    while (v) {
        IppVal *n = v->next;
        free(v->str);
        attrs_free(v->col);
        free(v);
        v = n;
    }
}

static char *dupn(const uint8_t *p, size_t n) {
    char *s = malloc(n + 1);
    if (!s) return NULL;
    memcpy(s, p, n);
    s[n] = 0;
    /* a 0 in the middle would cut the text: it's replaced */
    for (size_t i = 0; i < n; i++) if (!s[i]) s[i] = '?';
    return s;
}

static int read_value(Rd *r, uint8_t tag, IppVal *v, int depth);

/* Reads members of a collection up to endCollection. */
static int read_collection(Rd *r, IppVal *v, int depth) {
    if (depth > IPP_MAX_DEPTH) return -1;
    IppAttr **tail = &v->col, *cur = NULL;
    for (;;) {
        if (r->i >= r->n) return -1;
        uint8_t tag = r->p[r->i++];
        uint16_t nl, vl;
        if (rd16(r, &nl) || r->i + nl > r->n) return -1;
        r->i += nl;                                  /* in a collection the name is empty */
        if (rd16(r, &vl) || r->i + vl > r->n) return -1;
        if (tag == IPP_T_ENDCOL) { r->i += vl; return 0; }
        if (tag == IPP_T_MEMBERNAME) {
            if (++r->attrs > IPP_MAX_ATTRS) return -1;
            cur = calloc(1, sizeof *cur);
            if (!cur) return -1;
            cur->name = dupn(r->p + r->i, vl);
            r->i += vl;
            *tail = cur;
            tail = &cur->next;
            if (!cur->name) return -1;
            continue;
        }
        if (!cur) return -1;                         /* a value without a member name */
        r->i -= 2;                                   /* read_value reads the length again */
        IppVal *nv = calloc(1, sizeof *nv);
        if (!nv) return -1;
        IppVal **vt = &cur->vals;
        while (*vt) vt = &(*vt)->next;
        *vt = nv;
        cur->nvals++;
        if (read_value(r, tag, nv, depth + 1)) return -1;
    }
}

static int read_value(Rd *r, uint8_t tag, IppVal *v, int depth) {
    uint16_t vl;
    if (++r->vals > IPP_MAX_VALUES) return -1;
    if (rd16(r, &vl) || r->i + vl > r->n) return -1;
    const uint8_t *d = r->p + r->i;
    r->i += vl;
    v->tag = tag;
    v->data = d;
    v->len = vl;
    switch (tag) {
    case IPP_T_INTEGER: case IPP_T_ENUM:
        if (vl != 4) return -1;
        v->i = be32(d);
        break;
    case IPP_T_BOOLEAN:
        if (vl != 1) return -1;
        v->i = d[0] != 0;
        break;
    case IPP_T_RANGE:
        if (vl != 8) return -1;
        v->i = be32(d); v->i2 = be32(d + 4);
        break;
    case IPP_T_RESOLUTION:
        if (vl != 9) return -1;
        v->i = be32(d); v->i2 = be32(d + 4);
        break;
    case IPP_T_BEGCOL:
        return read_collection(r, v, depth);
    case IPP_T_TEXTLANG: case IPP_T_NAMELANG: {
        /* language (length + text) and the text (length + text) */
        if (vl < 4) return -1;
        size_t ll = (size_t)(d[0] << 8 | d[1]);
        if (2 + ll + 2 > vl) return -1;
        size_t tl = (size_t)(d[2 + ll] << 8 | d[3 + ll]);
        if (4 + ll + tl != vl) return -1;
        v->str = dupn(d + 4 + ll, tl);
        if (!v->str) return -1;
        break;
    }
    default:
        if (is_text(tag) || tag == IPP_T_STRING) { v->str = dupn(d, vl); if (!v->str) return -1; }
        break;
    }
    return 0;
}

int ipp_parse(const uint8_t *data, size_t len, IppMsg *m) {
    memset(m, 0, sizeof *m);
    if (!data || len < 9) return -1;
    m->major = data[0];
    m->minor = data[1];
    m->code = (uint16_t)(data[2] << 8 | data[3]);
    m->request_id = (uint32_t)be32(data + 4);
    if (m->major < 1 || m->major > 2) return -1;
    Rd r = { data, len, 8, 0, 0 };
    IppAttr **tail = &m->attrs, *cur = NULL;
    uint8_t group = 0;
    int gseq = 0;
    for (;;) {
        if (r.i >= r.n) goto bad;
        uint8_t tag = data[r.i++];
        if (tag == IPP_G_END) break;
        if (tag < 0x10) {                            /* group tag */
            if (tag == 0) goto bad;
            group = tag;
            gseq++;
            cur = NULL;
            continue;
        }
        if (!group) goto bad;
        if (tag == 0x7f) goto bad;                   /* extended tags: not used */
        uint16_t nl;
        if (rd16(&r, &nl) || r.i + nl > r.n) goto bad;
        if (nl) {
            if (++r.attrs > IPP_MAX_ATTRS) goto bad;
            cur = calloc(1, sizeof *cur);
            if (!cur) goto bad;
            *tail = cur;
            tail = &cur->next;
            cur->group = group;
            cur->gseq = gseq;
            cur->name = dupn(data + r.i, nl);
            if (!cur->name) goto bad;
            r.i += nl;
        } else if (!cur) goto bad;                   /* extra value without an attribute */
        IppVal *v = calloc(1, sizeof *v);
        if (!v) goto bad;
        IppVal **vt = &cur->vals;
        while (*vt) vt = &(*vt)->next;
        *vt = v;
        cur->nvals++;
        if (read_value(&r, tag, v, 0)) goto bad;
    }
    m->used = r.i;
    return 0;
bad:
    ipp_msg_free(m);
    return -1;
}

void ipp_msg_free(IppMsg *m) {
    attrs_free(m->attrs);
    m->attrs = NULL;
}

const IppAttr *ipp_find(const IppMsg *m, const IppAttr *from, uint8_t group, const char *name) {
    for (const IppAttr *a = from ? from->next : m->attrs; a; a = a->next)
        if ((!group || a->group == group) && !strcmp(a->name, name)) return a;
    return NULL;
}

const IppAttr *ipp_find_in(const IppAttr *start, const char *name) {
    /* forward within the same group (the attributes are in order) */
    for (const IppAttr *a = start; a && a->gseq == start->gseq; a = a->next)
        if (!strcmp(a->name, name)) return a;
    return NULL;
}

const char *ipp_str(const IppAttr *a) { return a && a->vals ? a->vals->str : NULL; }
int32_t ipp_int(const IppAttr *a, int32_t def) {
    if (!a || !a->vals) return def;
    uint8_t t = a->vals->tag;
    return t == IPP_T_INTEGER || t == IPP_T_ENUM || t == IPP_T_BOOLEAN ? a->vals->i : def;
}

const char *ipp_tag_name(uint8_t tag) {
    switch (tag) {
    case IPP_T_INTEGER: return "integer";
    case IPP_T_BOOLEAN: return "boolean";
    case IPP_T_ENUM: return "enum";
    case IPP_T_TEXT: return "text";
    case IPP_T_NAME: return "name";
    case IPP_T_KEYWORD: return "keyword";
    case IPP_T_URI: return "uri";
    case IPP_T_RANGE: return "rangeOfInteger";
    case IPP_T_BEGCOL: return "collection";
    default: return "other";
    }
}

const char *ipp_status_name(uint16_t s) {
    switch (s) {
    case 0x0000: return "successful-ok";
    case 0x0001: return "successful-ok-ignored-or-substituted-attributes";
    case 0x0002: return "successful-ok-conflicting-attributes";
    case 0x0400: return "client-error-bad-request";
    case 0x0401: return "client-error-forbidden";
    case 0x0402: return "client-error-not-authenticated";
    case 0x0403: return "client-error-not-authorized";
    case 0x0404: return "client-error-not-possible";
    case 0x0406: return "client-error-not-found";
    case 0x040a: return "client-error-document-format-not-supported";
    case 0x040b: return "client-error-attributes-or-values-not-supported";
    case 0x0500: return "server-error-internal-error";
    case 0x0501: return "server-error-operation-not-supported";
    case 0x0502: return "server-error-service-unavailable";
    case 0x0507: return "server-error-busy";
    default: return s < 0x100 ? "successful-ok" : s < 0x500 ? "client-error" : "server-error";
    }
}
