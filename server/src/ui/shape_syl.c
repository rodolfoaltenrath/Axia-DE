/* shape_syl.c — what the syllable-based shapers have in common (shape_khmer.c, shape_myanmar.c,
 * shape_use.c):
 *
 *   - syllable grammars: HarfBuzz writes them in Ragel and compiles them to automata; here they're
 *     written the same way (as text) and compiled on first use to a nondeterministic automaton
 *     (Thompson) that is walked with state sets. Inputs are short syllables, so
 *     that is more than enough;
 *   - normalization (decompose, reorder marks, recompose) like hb-ot-shape-normalize.cc;
 *   - ◌ in broken syllables and merging each syllable into a single cluster. */
#include "shape_syl.h"

#include <stdlib.h>
#include <string.h>

#include "unicode.h"

/* ---- automaton ---- */

#define MAX_ALTS 12

typedef struct { uint64_t mask; int out, out1; uint8_t sym; } St;

struct SylNfa {
    St *st;
    int n, cap;
    int start[MAX_ALTS], accept[MAX_ALTS];
    int *stamp, *cur, *nxt, gen;
    int bad;
};

typedef struct { int s, e; } Frag;

typedef struct {
    SylGrammar *g;
    SylNfa *a;
    const char *p, *end;
    int depth;
} Parser;

static int new_state(SylNfa *a, int sym, uint64_t mask) {
    if (a->n == a->cap) {
        int cap = a->cap ? a->cap * 2 : 256;
        St *ns = realloc(a->st, sizeof *ns * (size_t)cap);
        if (!ns) { a->bad = 1; return 0; }
        a->st = ns;
        a->cap = cap;
    }
    a->st[a->n] = (St){ mask, -1, -1, (uint8_t)sym };
    return a->n++;
}

static void skip_ws(Parser *ps) { while (ps->p < ps->end && (*ps->p == ' ' || *ps->p == '\t' || *ps->p == '\n' || *ps->p == '.')) ps->p++; }
static int is_name_char(char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_'; }

static Frag parse_alt(Parser *ps);

/* Expression of the def `name` (or NULL); *len = its length. */
static const char *find_def(const char *defs, const char *name, int nlen, int *len) {
    for (const char *p = defs; p && *p;) {
        while (*p == ' ' || *p == '\n' || *p == '\t') p++;
        const char *ns = p;
        while (is_name_char(*p)) p++;
        int match = p - ns == nlen && !memcmp(ns, name, (size_t)nlen);
        while (*p == ' ') p++;
        if (*p != '=') return NULL;
        const char *e = strchr(p, ';');
        if (!e) return NULL;
        if (match) { *len = (int)(e - p - 1); return p + 1; }
        p = e + 1;
    }
    return NULL;
}

static Frag parse_atom(Parser *ps) {
    skip_ws(ps);
    SylNfa *a = ps->a;
    if (ps->p < ps->end && *ps->p == '(') {
        ps->p++;
        Frag f = parse_alt(ps);
        skip_ws(ps);
        if (ps->p < ps->end && *ps->p == ')') ps->p++; else a->bad = 1;
        return f;
    }
    const char *ns = ps->p;
    while (ps->p < ps->end && is_name_char(*ps->p)) ps->p++;
    int nlen = (int)(ps->p - ns);
    Frag f = { 0, 0 };
    if (!nlen) { a->bad = 1; return f; }
    if (nlen == 3 && !memcmp(ns, "any", 3)) {
        f.s = new_state(a, 1, ~(uint64_t)0);
        f.e = new_state(a, 0, 0);
        if (!a->bad) a->st[f.s].out = f.e;
        return f;
    }
    for (int i = 0; i < ps->g->ncats; i++)
        if ((int)strlen(ps->g->cats[i]) == nlen && !memcmp(ps->g->cats[i], ns, (size_t)nlen)) {
            f.s = new_state(a, 1, (uint64_t)1 << i);
            f.e = new_state(a, 0, 0);
            if (!a->bad) a->st[f.s].out = f.e;
            return f;
        }
    int len;
    const char *def = find_def(ps->g->defs, ns, nlen, &len);
    if (!def || ps->depth > 32) { a->bad = 1; return f; }
    Parser sub = { ps->g, a, def, def + len, ps->depth + 1 };
    return parse_alt(&sub);
}

static Frag parse_post(Parser *ps) {
    Frag f = parse_atom(ps);
    SylNfa *a = ps->a;
    for (;;) {
        skip_ws(ps);
        if (ps->p >= ps->end || a->bad) return f;
        char c = *ps->p;
        if (c != '*' && c != '+' && c != '?') return f;
        ps->p++;
        int s = new_state(a, 0, 0), e = new_state(a, 0, 0);
        if (a->bad) return f;
        if (c == '*') {                 /* s → f | e; f.e → f | e */
            a->st[s].out = f.s; a->st[s].out1 = e;
            a->st[f.e].out = f.s; a->st[f.e].out1 = e;
        } else if (c == '+') {          /* f.e → f | e */
            a->st[s].out = f.s;
            a->st[f.e].out = f.s; a->st[f.e].out1 = e;
        } else {                        /* s → f | e; f.e → e */
            a->st[s].out = f.s; a->st[s].out1 = e;
            a->st[f.e].out = e;
        }
        f = (Frag){ s, e };
    }
}

static Frag parse_cat(Parser *ps) {
    Frag f = parse_post(ps);
    for (;;) {
        skip_ws(ps);
        if (ps->p >= ps->end || *ps->p == '|' || *ps->p == ')' || ps->a->bad) return f;
        Frag g = parse_post(ps);
        if (ps->a->bad) return f;
        ps->a->st[f.e].out = g.s;
        f.e = g.e;
    }
}

static Frag parse_alt(Parser *ps) {
    Frag f = parse_cat(ps);
    for (;;) {
        skip_ws(ps);
        if (ps->p >= ps->end || *ps->p != '|' || ps->a->bad) return f;
        ps->p++;
        Frag g = parse_cat(ps);
        int s = new_state(ps->a, 0, 0), e = new_state(ps->a, 0, 0);
        if (ps->a->bad) return f;
        ps->a->st[s].out = f.s; ps->a->st[s].out1 = g.s;
        ps->a->st[f.e].out = e;
        ps->a->st[g.e].out = e;
        f = (Frag){ s, e };
    }
}

static SylNfa *compile(SylGrammar *g) {
    SylNfa *a = calloc(1, sizeof *a);
    if (!a || g->nalts > MAX_ALTS || g->ncats > 64) { free(a); return NULL; }
    for (int i = 0; i < g->nalts && !a->bad; i++) {
        Parser ps = { g, a, g->alts[i], g->alts[i] + strlen(g->alts[i]), 0 };
        Frag f = parse_alt(&ps);
        skip_ws(&ps);
        if (ps.p != ps.end) a->bad = 1;
        a->start[i] = f.s;
        a->accept[i] = f.e;
    }
    if (!a->bad) {
        a->stamp = calloc((size_t)a->n, sizeof *a->stamp);
        a->cur = malloc(sizeof *a->cur * (size_t)a->n * 2);
        a->nxt = malloc(sizeof *a->nxt * (size_t)a->n * 2);
        if (!a->stamp || !a->cur || !a->nxt) a->bad = 1;
    }
    if (a->bad) { free(a->st); free(a->stamp); free(a->cur); free(a->nxt); free(a); return NULL; }
    return a;
}

/* Adds the state and its epsilon closure; the consuming ones go to list. Returns 1 if it reached acc. */
static int add(SylNfa *a, int s, int acc, int *list, int *nl) {
    int hit = 0, stack[512], sp = 0;
    stack[sp++] = s;
    while (sp) {
        int t = stack[--sp];
        if (t < 0 || a->stamp[t] == a->gen) continue;
        a->stamp[t] = a->gen;
        if (t == acc) hit = 1;
        if (a->st[t].sym) { list[(*nl)++] = t; continue; }
        if (sp + 2 <= 512) { stack[sp++] = a->st[t].out1; stack[sp++] = a->st[t].out; }
    }
    return hit;
}

static int longest(SylNfa *a, int alt, const uint8_t *cat, int n) {
    int nc = 0, best = 0;
    a->gen++;
    int acc = add(a, a->start[alt], a->accept[alt], a->cur, &nc);
    (void)acc;
    for (int i = 0; i < n && nc; i++) {
        int nn = 0, hit = 0;
        a->gen++;
        uint64_t bit = cat[i] < 64 ? (uint64_t)1 << cat[i] : 0;
        for (int k = 0; k < nc; k++) {
            const St *t = &a->st[a->cur[k]];
            if (t->mask & bit) hit |= add(a, t->out, a->accept[alt], a->nxt, &nn);
        }
        if (hit) best = i + 1;
        int *tmp = a->cur; a->cur = a->nxt; a->nxt = tmp;
        nc = nn;
    }
    return best;
}

int syl_next(SylGrammar *g, const uint8_t *cat, int n, int *alt) {
    if (!g->nfa) g->nfa = compile(g);
    *alt = g->nalts - 1;
    if (!g->nfa || n <= 0) return n > 0 ? 1 : 0;
    int best = 0;
    for (int i = 0; i < g->nalts; i++) {
        int l = longest(g->nfa, i, cat, n);
        if (l > best) { best = l; *alt = i; }
    }
    return best ? best : 1;
}

/* ---- normalization ---- */

int syl_modified_ccc(uint32_t cp) {
    static const uint8_t hebrew[17] = { 22, 15, 16, 17, 23, 18, 19, 20, 21, 14, 24, 12, 25, 13, 10, 11, 26 };   /* 10..26 */
    static const uint8_t arabic[10] = { 28, 29, 30, 31, 32, 33, 27, 34, 35, 36 };                             /* 27..36 */
    int c = uc_ccc(cp);
    if (c >= 10 && c <= 26) return hebrew[c - 10];
    if (c >= 27 && c <= 36) return arabic[c - 27];
    switch (c) {
    case 84: return 4;
    case 91: return 5;
    case 103: return 3;
    case 130: return 132;
    case 132: return 131;
    default: return c;
    }
}

typedef struct { uint32_t *cp; int *order; int n, cap; } Out;

static void emit(Out *o, uint32_t c, int from) {
    if (o->n < o->cap) { o->cp[o->n] = c; o->order[o->n++] = from; }
}

static int decompose(Otf *o, uint32_t ab, int (*split)(uint32_t, uint32_t *, uint32_t *), Out *out, int from, int depth) {
    uint32_t a, b;
    int ex;
    if (depth > 8 || !((split && split(ab, &a, &b)) || uc_decompose(ab, &a, &b, &ex))) return 0;
    if (b && !otf_cmap(o, b)) return 0;
    int has_a = otf_cmap(o, a) != 0, r;
    if (a != ab && (r = decompose(o, a, split, out, from, depth + 1))) { if (b) emit(out, b, from); return r + 1; }
    if (has_a) { emit(out, a, from); if (b) emit(out, b, from); return 2; }
    return 0;
}

int syl_normalize(Shaper *s, Otf *o, const uint32_t *in, int n, int (*split)(uint32_t, uint32_t *, uint32_t *), int compose_after_mark) {
    int cap = n * 4 + 8;
    if (cap > s->cp_cap) {
        uint32_t *nc = realloc(s->cp, sizeof *nc * (size_t)cap);
        if (!nc) return -1;
        s->cp = nc;
        int *no = realloc(s->order, sizeof *no * (size_t)cap);
        if (!no) return -1;
        s->order = no;
        s->cp_cap = cap;
    }
    Out out = { s->cp, s->order, 0, cap };
    for (int i = 0; i < n; i++) if (!decompose(o, in[i], split, &out, i, 0)) emit(&out, in[i], i);
    uint32_t *cp = s->cp;
    int *ord = s->order, m = out.n;
    /* consecutive marks in class order (stable) */
    for (int i = 0; i < m; i++) {
        if (!syl_modified_ccc(cp[i])) continue;
        int e = i;
        while (e < m && syl_modified_ccc(cp[e])) e++;
        if (e - i >= 2 && e - i <= 32)
            for (int a = i + 1; a < e; a++) {
                uint32_t c = cp[a];
                int k = syl_modified_ccc(c), oa = ord[a], b = a - 1;
                while (b >= i && syl_modified_ccc(cp[b]) > k) { cp[b + 1] = cp[b]; ord[b + 1] = ord[b]; b--; }
                cp[b + 1] = c;
                ord[b + 1] = oa;
            }
        i = e - 1;
    }
    /* recomposition: a mark with the preceding base, if nothing in between blocks it */
    if (m <= 1) return m;
    int w = 1, starter = 0;
    for (int i = 1; i < m; i++) {
        uint32_t c = cp[i];
        if (uc_mark(c)) {
            int ok = starter == w - 1 || syl_modified_ccc(cp[w - 1]) < syl_modified_ccc(c);
            uint32_t ab = ok && (compose_after_mark || !uc_mark(cp[starter])) ? uc_compose(cp[starter], c) : 0;
            if (ab && otf_cmap(o, ab)) {
                cp[starter] = ab;
                if (ord[i] < ord[starter]) ord[starter] = ord[i];
                continue;
            }
        }
        cp[w] = c;
        ord[w++] = ord[i];
        if (!syl_modified_ccc(c)) starter = w - 1;
    }
    return w;
}

/* ---- run ---- */

static void insert_at(GlyphRun *r, int at) {
    int tail = r->len - at;
    memmove(r->g + at + 1, r->g + at, sizeof *r->g * (size_t)tail);
    memmove(r->cl + at + 1, r->cl + at, sizeof *r->cl * (size_t)tail);
    memmove(r->mask + at + 1, r->mask + at, sizeof *r->mask * (size_t)tail);
    memmove(r->lig + at + 1, r->lig + at, (size_t)tail);
    memmove(r->aux + at + 1, r->aux + at, sizeof *r->aux * (size_t)tail);
    r->len++;
}

int syl_dotted_circles(Shaper *s, Otf *o, int broken_type, int cat_dc, int repha, int (*reserve)(Shaper *, int)) {
    GlyphRun *r = &s->run;
    uint16_t dc = otf_cmap(o, 0x25CC);
    if (!dc) return 0;
    int count = 0;
    for (int i = 0; i < r->len; i++)
        if (SYL_TYPE(r->aux[i]) == broken_type && (i == 0 || OTF_AUX_SYL(r->aux[i - 1]) != OTF_AUX_SYL(r->aux[i]))) count++;
    if (!count || reserve(s, r->len + count + 8)) return 0;
    for (int i = 0; i < r->len; i++) {
        if (SYL_TYPE(r->aux[i]) != broken_type || (i > 0 && OTF_AUX_SYL(r->aux[i - 1]) == OTF_AUX_SYL(r->aux[i]))) continue;
        uint32_t syl = r->aux[i] & 0xf0ff0000u;            /* syllable and type */
        int cl = r->cl[i];
        uint32_t mask = r->mask[i];
        int at = i;
        if (repha >= 0)
            while (at < r->len && (r->aux[at] & 0xf0ff0000u) == syl && (int)OTF_AUX_CAT(r->aux[at]) == repha) at++;
        insert_at(r, at);
        r->g[at] = dc;
        r->cl[at] = cl;
        r->mask[at] = mask;
        r->lig[at] = 0;
        r->aux[at] = syl | (uint32_t)cat_dc;
        i = at;
    }
    return 1;
}

void syl_merge_clusters(GlyphRun *r) {
    for (int i = 0; i < r->len;) {
        int e = i + 1, mn = r->cl[i];
        while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) { if (r->cl[e] < mn) mn = r->cl[e]; e++; }
        for (int j = i; j < e; j++) r->cl[j] = mn;
        i = e;
    }
}

void syl_move(GlyphRun *r, int from, int to) {
    if (from == to) return;
    uint16_t g = r->g[from];
    int cl = r->cl[from];
    uint32_t m = r->mask[from], x = r->aux[from];
    uint8_t l = r->lig[from];
    if (from < to) {
        int k = to - from;
        memmove(r->g + from, r->g + from + 1, sizeof *r->g * (size_t)k);
        memmove(r->cl + from, r->cl + from + 1, sizeof *r->cl * (size_t)k);
        memmove(r->mask + from, r->mask + from + 1, sizeof *r->mask * (size_t)k);
        memmove(r->lig + from, r->lig + from + 1, (size_t)k);
        memmove(r->aux + from, r->aux + from + 1, sizeof *r->aux * (size_t)k);
    } else {
        int k = from - to;
        memmove(r->g + to + 1, r->g + to, sizeof *r->g * (size_t)k);
        memmove(r->cl + to + 1, r->cl + to, sizeof *r->cl * (size_t)k);
        memmove(r->mask + to + 1, r->mask + to, sizeof *r->mask * (size_t)k);
        memmove(r->lig + to + 1, r->lig + to, (size_t)k);
        memmove(r->aux + to + 1, r->aux + to, sizeof *r->aux * (size_t)k);
    }
    r->g[to] = g; r->cl[to] = cl; r->mask[to] = m; r->aux[to] = x; r->lig[to] = l;
}
