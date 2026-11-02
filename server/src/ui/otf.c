/* otf.c — substitutions (GSUB), positioning (GPOS) and color glyphs (CBDT). See otf.h.
 *
 * GSUB: lookups 1 single, 2 multiple, 4 ligature, 5 contextual, 6 chained contextual
 * (formats 1, 2 and 3), 7 extension and 8 reverse chained. GPOS: 1 single adjustment, 2 pairs
 * (kerning), 3 cursive, 4 mark-to-base, 5 mark-to-ligature, 6 mark-to-mark, 7/8
 * contextual and 9 extension. Lookup flags are honored (ignore bases, ligatures,
 * marks, mark attachment class and GDEF mark sets).
 *
 * The terminal uses two fixed feature groups (otf_shape); the interface, a plan per
 * script (otf_plan) with Arabic forms per glyph and stages like HarfBuzz. */
#include "otf.h"

#include <stdlib.h>
#include <string.h>

#define MAX_DEPTH 6
#define MAX_RUN 4096
#define MAX_SEQ 64

static unsigned u8(const Otf *o, uint32_t off) { return off < o->n ? o->d[off] : 0; }
static unsigned u16(const Otf *o, uint32_t off) {
    return off + 2 <= o->n ? (unsigned)(o->d[off] << 8 | o->d[off + 1]) : 0;
}
static int s16(const Otf *o, uint32_t off) { return (int16_t)u16(o, off); }
static uint32_t u32(const Otf *o, uint32_t off) {
    return off + 4 <= o->n ? (uint32_t)o->d[off] << 24 | (uint32_t)o->d[off + 1] << 16 | (uint32_t)o->d[off + 2] << 8 | o->d[off + 3] : 0;
}
static int tag_is(const Otf *o, uint32_t off, const char *tag) { return off + 4 <= o->n && !memcmp(o->d + off, tag, 4); }

static uint32_t find_table(const Otf *o, uint32_t font, const char *tag, uint32_t *len) {
    unsigned n = u16(o, font + 4);
    for (unsigned i = 0; i < n; i++) {
        uint32_t r = font + 12 + 16 * i;
        if (r + 16 > o->n) break;
        if (!memcmp(o->d + r, tag, 4)) {
            uint32_t off = u32(o, r + 8), l = u32(o, r + 12);
            if (off >= o->n || l > o->n - off) return 0;
            *len = l;
            return off;
        }
    }
    return 0;
}

/* ---- coverage and classdef ---- */

static int coverage(const Otf *o, uint32_t cov, unsigned g) {
    unsigned fmt = u16(o, cov), n = u16(o, cov + 2);
    int lo = 0, hi = (int)n - 1;
    if (fmt == 1) {
        while (lo <= hi) {
            int mid = (lo + hi) / 2;
            unsigned v = u16(o, cov + 4 + 2 * (uint32_t)mid);
            if (v == g) return mid;
            if (v < g) lo = mid + 1; else hi = mid - 1;
        }
    } else if (fmt == 2) {
        while (lo <= hi) {
            int mid = (lo + hi) / 2;
            uint32_t r = cov + 4 + 6 * (uint32_t)mid;
            unsigned s = u16(o, r), e = u16(o, r + 2);
            if (g < s) hi = mid - 1;
            else if (g > e) lo = mid + 1;
            else return (int)(u16(o, r + 4) + g - s);
        }
    }
    return -1;
}

static unsigned class_of(const Otf *o, uint32_t cd, unsigned g) {
    if (!cd) return 0;
    unsigned fmt = u16(o, cd);
    if (fmt == 1) {
        unsigned start = u16(o, cd + 2), n = u16(o, cd + 4);
        return g >= start && g < start + n ? u16(o, cd + 6 + 2 * (g - start)) : 0;
    }
    if (fmt == 2) {
        int lo = 0, hi = (int)u16(o, cd + 2) - 1;
        while (lo <= hi) {
            int mid = (lo + hi) / 2;
            uint32_t r = cd + 4 + 6 * (uint32_t)mid;
            unsigned s = u16(o, r), e = u16(o, r + 2);
            if (g < s) hi = mid - 1;
            else if (g > e) lo = mid + 1;
            else return u16(o, r + 4);
        }
    }
    return 0;
}

/* Marks the glyphs of a coverage in the bitset. */
static void coverage_bits(const Otf *o, uint32_t cov, uint8_t *bits, int nglyphs) {
    unsigned fmt = u16(o, cov), n = u16(o, cov + 2);
    if (fmt == 1)
        for (unsigned i = 0; i < n; i++) {
            unsigned g = u16(o, cov + 4 + 2 * i);
            if ((int)g < nglyphs) bits[g >> 3] |= (uint8_t)(1 << (g & 7));
        }
    else if (fmt == 2)
        for (unsigned i = 0; i < n; i++) {
            uint32_t r = cov + 4 + 6 * i;
            unsigned s = u16(o, r), e = u16(o, r + 2);
            for (unsigned g = s; g <= e && (int)g < nglyphs; g++) bits[g >> 3] |= (uint8_t)(1 << (g & 7));
        }
}

/* ---- GDEF ---- */

enum { GC_BASE = 1, GC_LIGATURE = 2, GC_MARK = 3, GC_COMPONENT = 4 };
enum { LF_IGNORE_BASE = 2, LF_IGNORE_LIG = 4, LF_IGNORE_MARKS = 8, LF_MARK_SET = 0x10 };

static unsigned glyph_class(const Otf *o, unsigned g) { return class_of(o, o->glyph_class, g); }
int otf_is_mark(const Otf *o, unsigned g) { return glyph_class(o, g) == GC_MARK; }

static int in_mark_set(const Otf *o, unsigned set, unsigned g) {
    if (!o->mark_sets || set >= u16(o, o->mark_sets + 2)) return 0;
    uint32_t cov = o->mark_sets + u32(o, o->mark_sets + 4 + 4 * set);
    return coverage(o, cov, g) >= 0;
}

/* ---- application context ---- */

enum { T_GSUB = 0, T_GPOS = 1 };

typedef struct {
    const Otf *o;
    GlyphRun *run;
    OtfPos *pos;                /* GPOS only */
    int table, ops;
    unsigned flag, mset;        /* flags of the current lookup */
    int per_syl;                /* Indic: the match doesn't leave syllable syl */
    unsigned syl;
    int rtl;                    /* GPOS: direction of the run (cursive) */
    const uint8_t *ign;         /* GPOS: invisibles (ZWJ, ZWNJ…) skipped when looking for pairs */
} Ctx;

static int ignored(const Ctx *c, unsigned g) {
    unsigned f = c->flag;
    if (!(f & 0xff1e)) return 0;
    unsigned cls = glyph_class(c->o, g);
    if (cls == GC_BASE && (f & LF_IGNORE_BASE)) return 1;
    if (cls == GC_LIGATURE && (f & LF_IGNORE_LIG)) return 1;
    if (cls == GC_MARK) {
        if (f & LF_IGNORE_MARKS) return 1;
        if ((f & LF_MARK_SET) && !in_mark_set(c->o, c->mset, g)) return 1;
        if ((f >> 8) && class_of(c->o, c->o->mark_class, g) != (f >> 8)) return 1;
    }
    return 0;
}
static int skip_at(const Ctx *c, int p) { return ignored(c, c->run->g[p]) || (c->ign && c->ign[p]); }
static int skip_fwd(const Ctx *c, int p) { while (p < c->run->len && skip_at(c, p)) p++; return p; }
static int skip_back(const Ctx *c, int p) { while (p >= 0 && skip_at(c, p)) p--; return p; }

static uint32_t table_base(const Ctx *c) { return c->table == T_GPOS ? c->o->gpos : c->o->gsub; }

static uint32_t lookup_at(const Otf *o, uint32_t tab, unsigned index) {
    uint32_t ll = tab + u16(o, tab + 8);
    if (index >= u16(o, ll)) return 0;
    return ll + u16(o, ll + 2 + 2 * index);
}

/* Subtable k of the lookup, resolving extension ones (GSUB 7, GPOS 9). */
static int subtable(const Otf *o, int table, uint32_t lk, unsigned k, uint32_t *sub) {
    unsigned type = u16(o, lk);
    uint32_t s = lk + u16(o, lk + 6 + 2 * k);
    if (type == (table == T_GPOS ? 9u : 7u)) {
        type = u16(o, s + 2);
        s += u32(o, s + 4);
    }
    *sub = s;
    return (int)type;
}

static int run_reserve(GlyphRun *r, int extra) {
    if (r->len + extra <= r->cap) return 0;
    int cap = r->cap * 2 > r->len + extra ? r->cap * 2 : r->len + extra + 16;
    if (cap > MAX_RUN) return -1;
    uint16_t *g = realloc(r->g, (size_t)cap * sizeof *g);
    if (!g) return -1;
    r->g = g;
    int *cl = realloc(r->cl, (size_t)cap * sizeof *cl);
    if (!cl) return -1;
    r->cl = cl;
    if (r->mask) { uint32_t *m = realloc(r->mask, (size_t)cap * sizeof *m); if (!m) return -1; r->mask = m; }
    if (r->lig) { uint8_t *l = realloc(r->lig, (size_t)cap); if (!l) return -1; r->lig = l; }
    if (r->aux) { uint32_t *a = realloc(r->aux, (size_t)cap * sizeof *a); if (!a) return -1; r->aux = a; }
    r->cap = cap;
    return 0;
}

/* Opens n-1 slots after i, copying cluster, mask and component from i. */
static void run_expand(GlyphRun *r, int i, int n) {
    int tail = r->len - i - 1;
    memmove(r->g + i + n, r->g + i + 1, (size_t)tail * sizeof *r->g);
    memmove(r->cl + i + n, r->cl + i + 1, (size_t)tail * sizeof *r->cl);
    if (r->mask) memmove(r->mask + i + n, r->mask + i + 1, (size_t)tail * sizeof *r->mask);
    if (r->lig) memmove(r->lig + i + n, r->lig + i + 1, (size_t)tail);
    if (r->aux) memmove(r->aux + i + n, r->aux + i + 1, (size_t)tail * sizeof *r->aux);
    for (int k = 1; k < n; k++) {
        r->cl[i + k] = r->cl[i];
        if (r->mask) r->mask[i + k] = r->mask[i];
        if (r->lig) r->lig[i + k] = r->lig[i];
        if (r->aux) r->aux[i + k] = r->aux[i];
    }
    r->len += n - 1;
}

static void run_remove(GlyphRun *r, int i) {
    int tail = r->len - i - 1;
    memmove(r->g + i, r->g + i + 1, (size_t)tail * sizeof *r->g);
    memmove(r->cl + i, r->cl + i + 1, (size_t)tail * sizeof *r->cl);
    if (r->mask) memmove(r->mask + i, r->mask + i + 1, (size_t)tail * sizeof *r->mask);
    if (r->lig) memmove(r->lig + i, r->lig + i + 1, (size_t)tail);
    if (r->aux) memmove(r->aux + i, r->aux + i + 1, (size_t)tail * sizeof *r->aux);
    r->len--;
}

static int apply_at(Ctx *c, unsigned lookup, int i, int depth, int *next);

/* Match of a sequence (glyph, class or coverage), skipping what the lookup ignores. */
typedef int (*MatchFn)(const Otf *o, uint32_t base, uint32_t arr, unsigned k, unsigned g);
static int m_glyph(const Otf *o, uint32_t base, uint32_t arr, unsigned k, unsigned g) {
    (void)base; return u16(o, arr + 2 * k) == g;
}
static int m_class(const Otf *o, uint32_t cd, uint32_t arr, unsigned k, unsigned g) {
    return u16(o, arr + 2 * k) == class_of(o, cd, g);
}
static int m_cover(const Otf *o, uint32_t base, uint32_t arr, unsigned k, unsigned g) {
    return coverage(o, base + u16(o, arr + 2 * k), g) >= 0;
}

/* From p (inclusive) in direction dir; out[k] = position of each match. */
static int match_seq(const Ctx *c, int p, unsigned count, MatchFn fn, uint32_t base, uint32_t arr, int dir, int *out) {
    for (unsigned k = 0; k < count; k++) {
        p = dir > 0 ? skip_fwd(c, p) : skip_back(c, p);
        if (p < 0 || p >= c->run->len) return 0;
        if (c->per_syl && c->run->aux && OTF_AUX_SYL(c->run->aux[p]) != c->syl) return 0;
        if (!fn(c->o, base, arr, k, c->run->g[p])) return 0;
        if (out) out[k] = p;
        p += dir;
    }
    return 1;
}

/* Records (input position, lookup) of a contextual rule that matched. */
static void apply_records(Ctx *c, int *pos, int in_count, uint32_t rec, unsigned nrec, int depth, int *next) {
    int end = pos[in_count - 1] + 1, total = 0;
    for (unsigned k = 0; k < nrec; k++) {
        unsigned seq = u16(c->o, rec + 4 * k), lk = u16(c->o, rec + 4 * k + 2);
        if ((int)seq >= in_count) continue;
        int p = pos[seq], before = c->run->len, dummy;
        if (p < c->run->len) apply_at(c, lk, p, depth + 1, &dummy);
        int delta = c->run->len - before;
        total += delta;
        if (delta) for (int j = (int)seq + 1; j < in_count; j++) if (pos[j] > p) pos[j] += delta;
    }
    /* continue after the input, counting what the nested lookups added or deleted
     * (otherwise a multiple substitution on the last glyph would be applied again) */
    end += total;
    *next = end > pos[0] ? end : pos[0] + 1;
}

/* Contextual (GSUB 5/6, GPOS 7/8: same format). type 5 = simple, 6 = chained. */
static int apply_context(Ctx *c, int type, uint32_t s, int i, int depth, int *next) {
    const Otf *o = c->o;
    unsigned g = c->run->g[i], fmt = u16(o, s);
    int pos[MAX_SEQ];
    pos[0] = i;
    if (type == 5) {
        if (fmt == 1 || fmt == 2) {
            int idx = coverage(o, s + u16(o, s + 2), g);
            if (idx < 0) return 0;
            uint32_t cd = fmt == 2 ? s + u16(o, s + 4) : 0;
            uint32_t arr = fmt == 2 ? s + 8 : s + 6;
            unsigned which = fmt == 2 ? class_of(o, cd, g) : (unsigned)idx;
            if (which >= u16(o, arr - 2) || !u16(o, arr + 2 * which)) return 0;
            uint32_t set = s + u16(o, arr + 2 * which);
            unsigned nr = u16(o, set);
            for (unsigned k = 0; k < nr; k++) {
                uint32_t rule = set + u16(o, set + 2 + 2 * k);
                unsigned gc = u16(o, rule), sc = u16(o, rule + 2);
                if (!gc || gc > MAX_SEQ) continue;
                if (!match_seq(c, i + 1, gc - 1, fmt == 2 ? m_class : m_glyph, cd, rule + 4, 1, pos + 1)) continue;
                apply_records(c, pos, (int)gc, rule + 4 + 2 * (gc - 1), sc, depth, next);
                return 1;
            }
            return 0;
        }
        if (fmt == 3) {
            unsigned gc = u16(o, s + 2), sc = u16(o, s + 4);
            if (!gc || gc > MAX_SEQ || !m_cover(o, s, s + 6, 0, g)) return 0;
            if (!match_seq(c, i + 1, gc - 1, m_cover, s, s + 8, 1, pos + 1)) return 0;
            apply_records(c, pos, (int)gc, s + 6 + 2 * gc, sc, depth, next);
            return 1;
        }
        return 0;
    }
    if (fmt == 1 || fmt == 2) {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0) return 0;
        uint32_t bcd = 0, icd = 0, lcd = 0, arr = s + 6;
        unsigned which = (unsigned)idx;
        if (fmt == 2) {
            bcd = s + u16(o, s + 4); icd = s + u16(o, s + 6); lcd = s + u16(o, s + 8);
            arr = s + 12;
            which = class_of(o, icd, g);
        }
        if (which >= u16(o, arr - 2) || !u16(o, arr + 2 * which)) return 0;
        uint32_t set = s + u16(o, arr + 2 * which);
        unsigned nr = u16(o, set);
        MatchFn fn = fmt == 2 ? m_class : m_glyph;
        for (unsigned k = 0; k < nr; k++) {
            uint32_t p = set + u16(o, set + 2 + 2 * k);
            unsigned bc = u16(o, p);
            uint32_t barr = p + 2;
            p = barr + 2 * bc;
            unsigned ic = u16(o, p);
            uint32_t iarr = p + 2;
            if (!ic || ic > MAX_SEQ) continue;
            p = iarr + 2 * (ic - 1);
            unsigned lc = u16(o, p);
            uint32_t larr = p + 2;
            p = larr + 2 * lc;
            unsigned sc = u16(o, p);
            if (!match_seq(c, i + 1, ic - 1, fn, icd, iarr, 1, pos + 1)) continue;
            if (!match_seq(c, i - 1, bc, fn, bcd, barr, -1, NULL)) continue;
            if (!match_seq(c, pos[ic - 1] + 1, lc, fn, lcd, larr, 1, NULL)) continue;
            apply_records(c, pos, (int)ic, p + 2, sc, depth, next);
            return 1;
        }
        return 0;
    }
    if (fmt == 3) {
        unsigned bc = u16(o, s + 2);
        uint32_t barr = s + 4, p = barr + 2 * bc;
        unsigned ic = u16(o, p);
        uint32_t iarr = p + 2;
        p = iarr + 2 * ic;
        unsigned lc = u16(o, p);
        uint32_t larr = p + 2;
        p = larr + 2 * lc;
        unsigned sc = u16(o, p);
        if (!ic || ic > MAX_SEQ || !m_cover(o, s, iarr, 0, g)) return 0;
        if (!match_seq(c, i + 1, ic - 1, m_cover, s, iarr + 2, 1, pos + 1)) return 0;
        if (!match_seq(c, i - 1, bc, m_cover, s, barr, -1, NULL)) return 0;
        if (!match_seq(c, pos[ic - 1] + 1, lc, m_cover, s, larr, 1, NULL)) return 0;
        apply_records(c, pos, (int)ic, p + 2, sc, depth, next);
        return 1;
    }
    return 0;
}

/* ---- GSUB ---- */

static int gsub_sub(Ctx *c, int type, uint32_t s, int i, int depth, int *next) {
    const Otf *o = c->o;
    GlyphRun *r = c->run;
    unsigned g = r->g[i], fmt = u16(o, s);
    switch (type) {
    case 1: {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0) return 0;
        if (fmt == 1) r->g[i] = (uint16_t)(g + u16(o, s + 4));
        else if (fmt == 2) { if ((unsigned)idx >= u16(o, s + 4)) return 0; r->g[i] = (uint16_t)u16(o, s + 6 + 2 * (uint32_t)idx); }
        else return 0;
        if (r->aux) r->aux[i] |= OTF_AUX_SUBST;
        *next = i + 1;
        return 1;
    }
    case 2: {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0 || fmt != 1 || (unsigned)idx >= u16(o, s + 4)) return 0;
        uint32_t seq = s + u16(o, s + 6 + 2 * (uint32_t)idx);
        unsigned n = u16(o, seq);
        if (!n) {               /* empty sequence: deletes the glyph */
            run_remove(r, i);
            *next = i;
            return 1;
        }
        if (n > 32 || run_reserve(r, (int)n - 1)) return 0;
        run_expand(r, i, (int)n);
        for (unsigned k = 0; k < n; k++) {
            r->g[i + k] = (uint16_t)u16(o, seq + 2 + 2 * k);
            if (r->aux) r->aux[i + k] |= OTF_AUX_SUBST | (n > 1 ? OTF_AUX_MULTIPLIED : 0) | (k ? OTF_AUX_NOTFIRST : 0);
        }
        *next = i + (int)n;
        return 1;
    }
    case 4: {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0 || fmt != 1 || (unsigned)idx >= u16(o, s + 4)) return 0;
        uint32_t set = s + u16(o, s + 6 + 2 * (uint32_t)idx);
        unsigned nl = u16(o, set);
        int pos[MAX_SEQ];
        for (unsigned k = 0; k < nl; k++) {
            uint32_t lig = set + u16(o, set + 2 + 2 * k);
            unsigned comp = u16(o, lig + 2);
            if (!comp || comp > MAX_SEQ) continue;
            if (!match_seq(c, i + 1, comp - 1, m_glyph, 0, lig + 4, 1, pos + 1)) continue;
            pos[0] = i;
            r->g[i] = (uint16_t)u16(o, lig);
            if (r->aux) r->aux[i] |= OTF_AUX_SUBST | OTF_AUX_LIGATED;
            /* the marks left between components follow the glyph and remember which
             * component they belong to (for GPOS mark-to-ligature) */
            if (r->lig) {
                r->lig[i] = (uint8_t)comp;
                int cpos = 0;
                for (int p = i + 1; p < r->len && cpos < (int)comp; p++) {
                    if (cpos + 1 < (int)comp && p == pos[cpos + 1]) { cpos++; continue; }
                    if (glyph_class(o, r->g[p]) == GC_MARK) r->lig[p] = (uint8_t)(cpos + 1);
                    else if (cpos + 1 >= (int)comp) break;
                }
            }
            for (int k2 = (int)comp - 1; k2 >= 1; k2--) run_remove(r, pos[k2]);
            *next = i + 1;
            return 1;
        }
        return 0;
    }
    case 5: case 6:
        return apply_context(c, type, s, i, depth, next);
    case 8: {
        if (fmt != 1) return 0;
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0) return 0;
        unsigned bc = u16(o, s + 4);
        uint32_t barr = s + 6, p = barr + 2 * bc;
        unsigned lc = u16(o, p);
        uint32_t larr = p + 2;
        p = larr + 2 * lc;
        unsigned gc = u16(o, p);
        if ((unsigned)idx >= gc) return 0;
        if (!match_seq(c, i - 1, bc, m_cover, s, barr, -1, NULL)) return 0;
        if (!match_seq(c, i + 1, lc, m_cover, s, larr, 1, NULL)) return 0;
        r->g[i] = (uint16_t)u16(o, p + 2 + 2 * (uint32_t)idx);
        if (r->aux) r->aux[i] |= OTF_AUX_SUBST;
        *next = i + 1;
        return 1;
    }
    default:
        return 0;
    }
}

/* ---- GPOS ---- */

static int vr_size(unsigned fmt) { int n = 0; for (unsigned b = fmt & 0xff; b; b >>= 1) n += b & 1; return n * 2; }

static void vr_apply(const Otf *o, uint32_t at, unsigned fmt, OtfPos *p) {
    if (fmt & 1) { p->dx += s16(o, at); at += 2; }
    if (fmt & 2) { p->dy += s16(o, at); at += 2; }
    if (fmt & 4) { p->adv += s16(o, at); }
}

static void anchor(const Otf *o, uint32_t at, int *x, int *y) { *x = s16(o, at + 2); *y = s16(o, at + 4); }

/* Anchor mark i to glyph j: (dx, dy) from j's origin. */
static void attach(Ctx *c, int i, int j, uint32_t mark_anchor, uint32_t base_anchor) {
    int mx, my, bx, by;
    anchor(c->o, mark_anchor, &mx, &my);
    anchor(c->o, base_anchor, &bx, &by);
    c->pos[i].dx = bx - mx;
    c->pos[i].dy = by - my;
    c->pos[i].att = j;
}

/* Base preceding a mark: the previous glyph that is not a mark (ignoring the rest according to flags). */
static int prev_non_mark(const Ctx *c, int i) {
    for (int j = i - 1; j >= 0; j--) {
        unsigned g = c->run->g[j];
        if (glyph_class(c->o, g) == GC_MARK) continue;
        if (ignored(c, g)) continue;
        return j;
    }
    return -1;
}

static int gpos_sub(Ctx *c, int type, uint32_t s, int i, int *next) {
    const Otf *o = c->o;
    GlyphRun *r = c->run;
    unsigned g = r->g[i], fmt = u16(o, s);
    switch (type) {
    case 1: {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0) return 0;
        unsigned vf = u16(o, s + 4);
        if (fmt == 1) vr_apply(o, s + 6, vf, &c->pos[i]);
        else if (fmt == 2) { if ((unsigned)idx >= u16(o, s + 6)) return 0; vr_apply(o, s + 8 + (uint32_t)(vr_size(vf) * idx), vf, &c->pos[i]); }
        else return 0;
        *next = i + 1;
        return 1;
    }
    case 2: {
        int idx = coverage(o, s + u16(o, s + 2), g);
        if (idx < 0) return 0;
        int j = skip_fwd(c, i + 1);
        if (j >= r->len) return 0;
        unsigned g2 = r->g[j], vf1 = u16(o, s + 4), vf2 = u16(o, s + 6);
        int l1 = vr_size(vf1), l2 = vr_size(vf2);
        uint32_t v1 = 0;
        if (fmt == 1) {
            if ((unsigned)idx >= u16(o, s + 8)) return 0;
            uint32_t set = s + u16(o, s + 10 + 2 * (uint32_t)idx);
            int lo = 0, hi = (int)u16(o, set) - 1, rec = 2 + l1 + l2;
            while (lo <= hi) {
                int mid = (lo + hi) / 2;
                uint32_t p = set + 2 + (uint32_t)(rec * mid);
                unsigned sg = u16(o, p);
                if (sg == g2) { v1 = p + 2; break; }
                if (sg < g2) lo = mid + 1; else hi = mid - 1;
            }
            if (!v1) return 0;
        } else if (fmt == 2) {
            uint32_t cd1 = s + u16(o, s + 8), cd2 = s + u16(o, s + 10);
            unsigned n1 = u16(o, s + 12), n2 = u16(o, s + 14);
            unsigned c1 = class_of(o, cd1, g), c2 = class_of(o, cd2, g2);
            if (c1 >= n1 || c2 >= n2) return 0;
            v1 = s + 16 + (uint32_t)((l1 + l2) * (int)(c1 * n2 + c2));
        } else return 0;
        vr_apply(o, v1, vf1, &c->pos[i]);
        vr_apply(o, v1 + (uint32_t)l1, vf2, &c->pos[j]);
        *next = vf2 ? j + 1 : j;
        return 1;
    }
    case 3: {                   /* cursive: the previous one's exit joins this one's entry */
        if (fmt != 1) return 0;
        uint32_t cov = s + u16(o, s + 2);
        int ci = coverage(o, cov, g);
        unsigned cnt = u16(o, s + 4);
        if (ci < 0 || (unsigned)ci >= cnt) return 0;
        unsigned entry = u16(o, s + 6 + 4 * (uint32_t)ci);
        if (!entry) return 0;
        int pi = skip_back(c, i - 1);
        if (pi < 0) return 0;
        int pc = coverage(o, cov, r->g[pi]);
        if (pc < 0 || (unsigned)pc >= cnt) return 0;
        unsigned exitp = u16(o, s + 6 + 4 * (uint32_t)pc + 2);
        if (!exitp) return 0;
        int ex, ey, nx, ny;
        anchor(o, s + exitp, &ex, &ey);
        anchor(o, s + entry, &nx, &ny);
        OtfPos *P = c->pos;
        int a = pi, b = i, d;            /* a: the previous one (exit), b: this one (entry) */
        if (!c->rtl) { P[a].adv = ex + P[a].dx; d = nx + P[b].dx; P[b].adv -= d; P[b].dx -= d; }
        else { d = ex + P[a].dx; P[a].adv -= d; P[a].dx -= d; P[b].adv = nx + P[b].dx; }
        int child = a, parent = b, yoff = ny - ey;
        if (!(c->flag & 1)) { child = b; parent = a; yoff = -yoff; }     /* without RightToLeft */
        P[child].dy = yoff;
        P[child].cur = parent;
        if (P[parent].cur == child) P[parent].cur = -1;
        *next = i + 1;
        return 1;
    }
    case 4: case 5: {
        if (fmt != 1) return 0;
        int mi = coverage(o, s + u16(o, s + 2), g);
        if (mi < 0) return 0;
        int j = prev_non_mark(c, i);
        if (j < 0) return 0;
        int bi = coverage(o, s + u16(o, s + 4), r->g[j]);
        if (bi < 0) return 0;
        unsigned ncls = u16(o, s + 6);
        uint32_t marks = s + u16(o, s + 8), bases = s + u16(o, s + 10);
        if ((unsigned)mi >= u16(o, marks)) return 0;
        unsigned mcls = u16(o, marks + 2 + 4 * (uint32_t)mi);
        uint32_t manc = marks + u16(o, marks + 4 + 4 * (uint32_t)mi);
        if (mcls >= ncls || (unsigned)bi >= u16(o, bases)) return 0;
        uint32_t banc;
        if (type == 4) {
            unsigned off = u16(o, bases + 2 + 2 * ((uint32_t)bi * ncls + mcls));
            if (!off) return 0;
            banc = bases + off;
        } else {
            uint32_t la = bases + u16(o, bases + 2 + 2 * (uint32_t)bi);
            unsigned ncomp = u16(o, la);
            if (!ncomp) return 0;
            unsigned comp = r->lig && r->lig[i] && r->lig[i] <= ncomp ? r->lig[i] - 1u : ncomp - 1;
            unsigned off = u16(o, la + 2 + 2 * (comp * ncls + mcls));
            if (!off) return 0;
            banc = la + off;
        }
        attach(c, i, j, manc, banc);
        *next = i + 1;
        return 1;
    }
    case 6: {
        if (fmt != 1) return 0;
        int mi = coverage(o, s + u16(o, s + 2), g);
        if (mi < 0) return 0;
        int j = skip_back(c, i - 1);
        if (j < 0 || glyph_class(o, r->g[j]) != GC_MARK) return 0;
        int bi = coverage(o, s + u16(o, s + 4), r->g[j]);
        if (bi < 0) return 0;
        unsigned ncls = u16(o, s + 6);
        uint32_t marks = s + u16(o, s + 8), m2 = s + u16(o, s + 10);
        if ((unsigned)mi >= u16(o, marks) || (unsigned)bi >= u16(o, m2)) return 0;
        unsigned mcls = u16(o, marks + 2 + 4 * (uint32_t)mi);
        if (mcls >= ncls) return 0;
        uint32_t manc = marks + u16(o, marks + 4 + 4 * (uint32_t)mi);
        unsigned off = u16(o, m2 + 2 + 2 * ((uint32_t)bi * ncls + mcls));
        if (!off) return 0;
        attach(c, i, j, manc, m2 + off);
        *next = i + 1;
        return 1;
    }
    default:
        return 0;
    }
}

static int apply_sub(Ctx *c, int type, uint32_t s, int i, int depth, int *next) {
    if (++c->ops > 400000) return 0;
    if (c->table == T_GPOS) {
        if (type == 7) return apply_context(c, 5, s, i, depth, next);
        if (type == 8) return apply_context(c, 6, s, i, depth, next);
        return gpos_sub(c, type, s, i, next);
    }
    return gsub_sub(c, type, s, i, depth, next);
}

/* Lookup flags (and mark set, if it uses one). */
static void lookup_flags(const Otf *o, uint32_t lk, unsigned *flag, unsigned *mset) {
    *flag = u16(o, lk + 2);
    *mset = (*flag & LF_MARK_SET) ? u16(o, lk + 6 + 2 * u16(o, lk + 4)) : 0;
}

static int apply_at(Ctx *c, unsigned lookup, int i, int depth, int *next) {
    if (depth > MAX_DEPTH || i >= c->run->len) return 0;
    uint32_t lk = lookup_at(c->o, table_base(c), lookup);
    if (!lk) return 0;
    unsigned sf = c->flag, sm = c->mset;
    lookup_flags(c->o, lk, &c->flag, &c->mset);
    unsigned n = u16(c->o, lk + 4);
    int done = 0;
    if (!ignored(c, c->run->g[i]))
        for (unsigned k = 0; k < n && !done; k++) {
            uint32_t s;
            int type = subtable(c->o, c->table, lk, k, &s);
            done = apply_sub(c, type, s, i, depth, next);
        }
    c->flag = sf;
    c->mset = sm;
    return done;
}

/* Applies a lookup to the whole run (back to front if it is GSUB type 8). */
static void run_lookup(Ctx *c, unsigned lookup, uint32_t mask, const uint8_t *bits, int per_syl) {
    const Otf *o = c->o;
    uint32_t lk = lookup_at(o, table_base(c), lookup);
    if (!lk) return;
    uint32_t s0;
    int reverse = c->table == T_GSUB && u16(o, lk + 4) && subtable(o, c->table, lk, 0, &s0) == 8;
    lookup_flags(o, lk, &c->flag, &c->mset);
    GlyphRun *r = c->run;
    for (int i = reverse ? r->len - 1 : 0; reverse ? i >= 0 : i < r->len;) {
        unsigned g = r->g[i];
        int next = i + 1;
        int ok = (!bits || ((int)g < o->nglyphs && (bits[g >> 3] >> (g & 7) & 1))) &&
                 (!mask || !r->mask || (r->mask[i] & mask)) && !ignored(c, g);
        if (ok) {
            c->per_syl = per_syl && r->aux;
            if (c->per_syl) c->syl = OTF_AUX_SYL(r->aux[i]);
            unsigned n = u16(o, lk + 4);
            int done = 0;
            for (unsigned k = 0; k < n && !done; k++) {
                uint32_t s;
                int type = subtable(o, c->table, lk, k, &s);
                done = apply_sub(c, type, s, i, 0, &next);
                lookup_flags(o, lk, &c->flag, &c->mset);    /* the nested ones change them */
            }
            if (!done) next = i + 1;
        }
        if (reverse) i--;
        else i = next > i ? next : (next == i && ok ? i : i + 1);
        if (c->ops > 400000) break;
    }
    c->per_syl = 0;
}

/* First possible glyph of each subtable → lookup bitset (fast rejection). */
static void lookup_bits(const Otf *o, unsigned lookup, uint8_t *bits) {
    uint32_t lk = lookup_at(o, o->gsub, lookup);
    unsigned n = lk ? u16(o, lk + 4) : 0;
    for (unsigned k = 0; k < n; k++) {
        uint32_t s;
        int type = subtable(o, T_GSUB, lk, k, &s);
        unsigned fmt = u16(o, s);
        if (type == 5 && fmt == 3) coverage_bits(o, s + u16(o, s + 6), bits, o->nglyphs);
        else if (type == 6 && fmt == 3) {
            uint32_t p = s + 4 + 2 * u16(o, s + 2);
            if (u16(o, p)) coverage_bits(o, s + u16(o, p + 2), bits, o->nglyphs);
        } else if ((type >= 1 && type <= 6) || type == 8) coverage_bits(o, s + u16(o, s + 2), bits, o->nglyphs);
    }
}

static int cmp_u16(const void *a, const void *b) { return (int)*(const uint16_t *)a - (int)*(const uint16_t *)b; }

static const char *const group_tags[2][3] = { { "calt", "liga", "clig" }, { "ccmp", "rlig", NULL } };

int otf_init(Otf *o, const uint8_t *data, size_t size, int font_offset) {
    memset(o, 0, sizeof *o);
    o->d = data;
    o->n = size;
    uint32_t f = (uint32_t)font_offset, len;
    uint32_t maxp = find_table(o, f, "maxp", &len);
    o->nglyphs = maxp ? (int)u16(o, maxp + 4) : 0;
    o->gsub = find_table(o, f, "GSUB", &o->gsub_len);
    o->gpos = find_table(o, f, "GPOS", &o->gpos_len);
    o->cblc = find_table(o, f, "CBLC", &o->cblc_len);
    o->cbdt = find_table(o, f, "CBDT", &o->cbdt_len);
    uint32_t hhea = find_table(o, f, "hhea", &len);
    o->hmtx = find_table(o, f, "hmtx", &len);
    o->nhmetrics = hhea ? u16(o, hhea + 34) : 0;
    if (o->nhmetrics * 4u > len) o->nhmetrics = len / 4;
    uint32_t gdef = find_table(o, f, "GDEF", &len);
    if (gdef) {
        unsigned gc = u16(o, gdef + 4), mc = u16(o, gdef + 10);
        if (gc) o->glyph_class = gdef + gc;
        if (mc) o->mark_class = gdef + mc;
        if (u16(o, gdef + 2) >= 2 && u16(o, gdef + 12)) o->mark_sets = gdef + u16(o, gdef + 12);
    }
    uint32_t cm = find_table(o, f, "cmap", &len);
    if (cm) {
        unsigned n = u16(o, cm + 2);
        int best = 0;
        for (unsigned i = 0; i < n && i < 64; i++) {
            uint32_t r = cm + 4 + 8 * i, sub = cm + u32(o, r + 4);
            unsigned plat = u16(o, r), enc = u16(o, r + 2), fmt = u16(o, sub);
            int score = fmt == 12 && (plat == 0 || (plat == 3 && enc == 10)) ? 3 : fmt == 4 && (plat == 0 || (plat == 3 && enc == 1)) ? 2 : 0;
            if (score > best) { best = score; o->cmap = sub; }
        }
    }
    if (o->nglyphs <= 0) { o->gsub = o->gpos = 0; return 0; }
    if (!o->gsub) return 0;

    uint32_t fl = o->gsub + u16(o, o->gsub + 6);
    unsigned nf = u16(o, fl), nlk = u16(o, o->gsub + u16(o, o->gsub + 8));
    uint16_t *tmp[2] = { NULL, NULL };
    int cnt[2] = { 0, 0 };
    for (int grp = 0; grp < 2; grp++) {
        tmp[grp] = malloc(sizeof(uint16_t) * (nlk + 1));
        if (!tmp[grp]) break;
        for (unsigned i = 0; i < nf; i++) {
            uint32_t rec = fl + 2 + 6 * i;
            int want = 0;
            for (int t = 0; t < 3; t++) if (group_tags[grp][t] && tag_is(o, rec, group_tags[grp][t])) want = 1;
            if (!want) continue;
            uint32_t feat = fl + u16(o, rec + 4);
            unsigned n = u16(o, feat + 2);
            for (unsigned k = 0; k < n; k++) {
                unsigned li = u16(o, feat + 4 + 2 * k);
                if (li >= nlk) continue;
                int dup = 0;
                for (int j = 0; j < cnt[grp]; j++) if (tmp[grp][j] == li) { dup = 1; break; }
                if (!dup && cnt[grp] < (int)nlk) tmp[grp][cnt[grp]++] = (uint16_t)li;
            }
        }
        qsort(tmp[grp], (size_t)cnt[grp], sizeof(uint16_t), cmp_u16);
    }
    int total = cnt[0] + cnt[1];
    size_t bytes = (size_t)(o->nglyphs + 7) / 8;
    o->lookups = malloc(sizeof(uint16_t) * (size_t)(total + 1));
    o->first[0] = calloc((size_t)(cnt[0] + 1), bytes);
    o->first[1] = calloc((size_t)(cnt[1] + 1), bytes);
    if (!o->lookups || !o->first[0] || !o->first[1] || !tmp[0] || !tmp[1]) {
        free(tmp[0]); free(tmp[1]);
        free(o->lookups); free(o->first[0]); free(o->first[1]);
        o->lookups = NULL; o->first[0] = o->first[1] = NULL;
        o->nlookups[0] = o->nlookups[1] = 0;
        return 0;
    }
    memcpy(o->lookups, tmp[0], sizeof(uint16_t) * (size_t)cnt[0]);
    memcpy(o->lookups + cnt[0], tmp[1], sizeof(uint16_t) * (size_t)cnt[1]);
    o->nlookups[0] = cnt[0];
    o->nlookups[1] = cnt[1];
    for (int grp = 0; grp < 2; grp++) {
        const uint16_t *ls = o->lookups + (grp ? cnt[0] : 0);
        for (int j = 0; j < cnt[grp]; j++) lookup_bits(o, ls[j], o->first[grp] + (size_t)j * bytes);
    }
    free(tmp[0]);
    free(tmp[1]);
    return 0;
}

void otf_free(Otf *o) {
    free(o->lookups);
    free(o->first[0]);
    free(o->first[1]);
    for (int i = 0; i < o->nplans; i++) { free(o->plans[i].gsub); free(o->plans[i].gpos); free(o->plans[i].first); }
    memset(o, 0, sizeof *o);
}

int otf_has_gsub(const Otf *o, int group) { return o->gsub && o->nlookups[group] > 0; }

void otf_shape(const Otf *o, int group, GlyphRun *run) {
    if (!otf_has_gsub(o, group)) return;
    Ctx c = { o, run, NULL, T_GSUB, 0, 0, 0, 0, 0, 0, NULL };
    size_t bytes = (size_t)(o->nglyphs + 7) / 8;
    const uint16_t *ls = o->lookups + (group ? o->nlookups[0] : 0);
    for (int j = 0; j < o->nlookups[group] && c.ops <= 400000; j++)
        run_lookup(&c, ls[j], 0, o->first[group] + (size_t)j * bytes, 0);
}

/* ---- per-script plans (interface) ---- */

typedef struct { const char *tag; uint8_t stage; uint32_t mask; uint8_t per_syl; } FeatureSpec;

/* Arabic stages as in HarfBuzz: ccmp/locl, isol, fina, medi, init, rlig, calt, rclt, liga… */
static const FeatureSpec gsub_features[] = {
    { "ccmp", 0, OTF_F_GLOBAL, 0 }, { "locl", 0, OTF_F_GLOBAL, 0 },
    { "isol", 1, OTF_F_ISOL, 0 }, { "fina", 2, OTF_F_FINA, 0 }, { "medi", 3, OTF_F_MEDI, 0 }, { "init", 4, OTF_F_INIT, 0 },
    { "rlig", 5, OTF_F_GLOBAL, 0 }, { "calt", 6, OTF_F_GLOBAL, 0 }, { "rclt", 7, OTF_F_GLOBAL, 0 },
    { "liga", 8, OTF_F_GLOBAL, 0 }, { "clig", 8, OTF_F_GLOBAL, 0 }, { "mset", 8, OTF_F_GLOBAL, 0 },
};
/* Indic (like HarfBuzz's Indic shaper): locl/ccmp; initial reordering; nukt, akhn,
 * rphf, rkrf, pref, blwf, abvf, half, pstf, vatu, cjct each in its own stage; final
 * reordering; and the rest together. None crosses syllables except the common horizontal ones. */
static const FeatureSpec indic_features[] = {
    { "locl", 0, OTF_F_GLOBAL, 1 }, { "ccmp", 0, OTF_F_GLOBAL, 1 },
    { "nukt", 1, OTF_F_GLOBAL, 1 }, { "akhn", 2, OTF_F_GLOBAL, 1 }, { "rphf", 3, OTF_F_RPHF, 1 },
    { "rkrf", 4, OTF_F_GLOBAL, 1 }, { "pref", 5, OTF_F_PREF, 1 }, { "blwf", 6, OTF_F_BLWF, 1 },
    { "abvf", 7, OTF_F_ABVF, 1 }, { "half", 8, OTF_F_HALF, 1 }, { "pstf", 9, OTF_F_PSTF, 1 },
    { "vatu", 10, OTF_F_GLOBAL, 1 }, { "cjct", 11, OTF_F_GLOBAL, 1 },
    { "init", 12, OTF_F_INIT, 1 }, { "pres", 12, OTF_F_GLOBAL, 1 }, { "abvs", 12, OTF_F_GLOBAL, 1 },
    { "blws", 12, OTF_F_GLOBAL, 1 }, { "psts", 12, OTF_F_GLOBAL, 1 }, { "haln", 12, OTF_F_GLOBAL, 1 },
    { "rlig", 12, OTF_F_GLOBAL, 0 }, { "calt", 12, OTF_F_GLOBAL, 0 }, { "clig", 12, OTF_F_GLOBAL, 0 },
    { "liga", 12, OTF_F_GLOBAL, 0 }, { "rclt", 12, OTF_F_GLOBAL, 0 },
};
/* Khmer: no pauses between the basic ones (like Uniscribe); clig instead of liga. */
static const FeatureSpec khmer_features[] = {
    { "locl", 0, OTF_F_GLOBAL, 1 }, { "ccmp", 0, OTF_F_GLOBAL, 1 }, { "pref", 0, OTF_F_PREF, 1 }, { "blwf", 0, OTF_F_BLWF, 1 },
    { "abvf", 0, OTF_F_ABVF, 1 }, { "pstf", 0, OTF_F_PSTF, 1 }, { "cfar", 0, OTF_F_CFAR, 1 },
    { "pres", 1, OTF_F_GLOBAL, 0 }, { "abvs", 1, OTF_F_GLOBAL, 0 }, { "blws", 1, OTF_F_GLOBAL, 0 }, { "psts", 1, OTF_F_GLOBAL, 0 },
    { "rlig", 1, OTF_F_GLOBAL, 0 }, { "calt", 1, OTF_F_GLOBAL, 0 }, { "clig", 1, OTF_F_GLOBAL, 0 }, { "rclt", 1, OTF_F_GLOBAL, 0 },
};
/* Myanmar: locl/ccmp; reordering; rphf, pref, blwf, pstf one at a time; the rest together. */
static const FeatureSpec myanmar_features[] = {
    { "locl", 0, OTF_F_GLOBAL, 1 }, { "ccmp", 0, OTF_F_GLOBAL, 1 },
    { "rphf", 1, OTF_F_GLOBAL, 1 }, { "pref", 2, OTF_F_GLOBAL, 1 }, { "blwf", 3, OTF_F_GLOBAL, 1 }, { "pstf", 4, OTF_F_GLOBAL, 1 },
    { "pres", 5, OTF_F_GLOBAL, 0 }, { "abvs", 5, OTF_F_GLOBAL, 0 }, { "blws", 5, OTF_F_GLOBAL, 0 }, { "psts", 5, OTF_F_GLOBAL, 0 },
    { "rlig", 5, OTF_F_GLOBAL, 0 }, { "calt", 5, OTF_F_GLOBAL, 0 }, { "clig", 5, OTF_F_GLOBAL, 0 }, { "liga", 5, OTF_F_GLOBAL, 0 },
    { "rclt", 5, OTF_F_GLOBAL, 0 },
};
/* Universal Shaping Engine: preprocessing; rphf; pref; basic; reordering; joining forms;
 * presentation. */
static const FeatureSpec use_features[] = {
    { "locl", 0, OTF_F_GLOBAL, 1 }, { "ccmp", 0, OTF_F_GLOBAL, 1 }, { "nukt", 0, OTF_F_GLOBAL, 1 }, { "akhn", 0, OTF_F_GLOBAL, 1 },
    { "rphf", 1, OTF_F_RPHF, 1 }, { "pref", 2, OTF_F_GLOBAL, 1 },
    { "rkrf", 3, OTF_F_GLOBAL, 1 }, { "abvf", 3, OTF_F_GLOBAL, 1 }, { "blwf", 3, OTF_F_GLOBAL, 1 }, { "half", 3, OTF_F_GLOBAL, 1 },
    { "pstf", 3, OTF_F_GLOBAL, 1 }, { "vatu", 3, OTF_F_GLOBAL, 1 }, { "cjct", 3, OTF_F_GLOBAL, 1 },
    { "isol", 4, OTF_F_ISOL, 0 }, { "init", 4, OTF_F_INIT, 0 }, { "medi", 4, OTF_F_MEDI, 0 }, { "fina", 4, OTF_F_FINA, 0 },
    { "abvs", 5, OTF_F_GLOBAL, 0 }, { "blws", 5, OTF_F_GLOBAL, 0 }, { "haln", 5, OTF_F_GLOBAL, 0 }, { "pres", 5, OTF_F_GLOBAL, 0 },
    { "psts", 5, OTF_F_GLOBAL, 0 }, { "rlig", 5, OTF_F_GLOBAL, 0 }, { "calt", 5, OTF_F_GLOBAL, 0 }, { "clig", 5, OTF_F_GLOBAL, 0 },
    { "liga", 5, OTF_F_GLOBAL, 0 }, { "rclt", 5, OTF_F_GLOBAL, 0 },
};
static const FeatureSpec gpos_features[] = {
    { "kern", 0, OTF_F_GLOBAL, 0 }, { "mark", 0, OTF_F_GLOBAL, 0 }, { "mkmk", 0, OTF_F_GLOBAL, 0 },
    { "dist", 0, OTF_F_GLOBAL, 0 }, { "abvm", 0, OTF_F_GLOBAL, 0 }, { "blwm", 0, OTF_F_GLOBAL, 0 },
    { "curs", 0, OTF_F_GLOBAL, 0 },
};

static int indic_script(const char *s) {
    static const char *const j[] = { "dev2", "bng2", "gur2", "gjr2", "ory2", "tml2", "tel2", "knd2", "mlm2",
                                     "deva", "beng", "guru", "gujr", "orya", "taml", "telu", "knda", "mlym" };
    for (size_t i = 0; i < sizeof j / sizeof *j; i++) if (!memcmp(s, j[i], 4)) return 1;
    return 0;
}

static int joining_script(const char *s) {
    static const char *const j[] = { "arab", "syrc", "nko ", "mong", "adlm", "rohg", "mand", "mani", "phag" };
    for (size_t i = 0; i < sizeof j / sizeof *j; i++) if (!memcmp(s, j[i], 4)) return 1;
    return 0;
}

static int cmp_ref(const void *a, const void *b) {
    const OtfLookupRef *x = a, *y = b;
    if (x->stage != y->stage) return x->stage - y->stage;
    return (int)x->lookup - (int)y->lookup;
}

/* Default LangSys of the script (or DFLT, or latn). */
static uint32_t langsys(const Otf *o, uint32_t tab, const char *script) {
    uint32_t sl = tab + u16(o, tab + 4);
    unsigned n = u16(o, sl);
    const char *try[3] = { script, "DFLT", "latn" };
    for (int t = 0; t < 3; t++)
        for (unsigned i = 0; i < n && i < 512; i++) {
            uint32_t rec = sl + 2 + 6 * i;
            if (!tag_is(o, rec, try[t])) continue;
            uint32_t sc = sl + u16(o, rec + 4);
            unsigned dl = u16(o, sc);
            if (dl) return sc + dl;
            if (u16(o, sc + 2)) return sc + u16(o, sc + 6);     /* no default: the first language */
            return 0;
        }
    return 0;
}

static int collect(const Otf *o, uint32_t tab, const char *script, const FeatureSpec *spec, int nspec, int staged,
                   OtfLookupRef **out, int *gpos_kern) {
    *out = NULL;
    if (!tab) return 0;
    uint32_t ls = langsys(o, tab, script);
    if (!ls) return 0;
    uint32_t fl = tab + u16(o, tab + 6);
    unsigned nfeat = u16(o, fl), nlk = u16(o, tab + u16(o, tab + 8));
    unsigned req = u16(o, ls + 2), cnt = u16(o, ls + 4);
    int cap = 64, n = 0;
    OtfLookupRef *refs = malloc(sizeof *refs * (size_t)cap);
    if (!refs) return 0;
    for (unsigned k = 0; k <= cnt; k++) {
        unsigned fi = k < cnt ? u16(o, ls + 6 + 2 * k) : req;
        if (fi >= nfeat) continue;
        uint32_t rec = fl + 2 + 6 * fi;
        const FeatureSpec *f = NULL;
        for (int s = 0; s < nspec; s++) if (tag_is(o, rec, spec[s].tag)) { f = &spec[s]; break; }
        if (!f && k == cnt) f = &spec[0];       /* the required feature always applies */
        if (!f) continue;
        if (gpos_kern && !memcmp(f->tag, "kern", 4)) *gpos_kern = 1;
        uint32_t feat = fl + u16(o, rec + 4);
        unsigned nl = u16(o, feat + 2);
        uint8_t stage = staged ? f->stage : 0;
        for (unsigned j = 0; j < nl; j++) {
            unsigned li = u16(o, feat + 4 + 2 * j);
            if (li >= nlk) continue;
            int dup = -1;
            for (int q = 0; q < n; q++) if (refs[q].lookup == li && refs[q].stage == stage) { dup = q; break; }
            if (dup >= 0) { refs[dup].mask |= f->mask; continue; }
            if (n == cap) {
                OtfLookupRef *nr = realloc(refs, sizeof *refs * (size_t)(cap *= 2));
                if (!nr) break;
                refs = nr;
            }
            refs[n].lookup = (uint16_t)li;
            refs[n].stage = stage;
            refs[n].mask = f->mask;
            refs[n].per_syllable = f->per_syl;
            n++;
        }
    }
    qsort(refs, (size_t)n, sizeof *refs, cmp_ref);
    *out = refs;
    return n;
}

const OtfPlan *otf_plan_kind(Otf *o, const char *script, int kind) {
    for (int i = 0; i < o->nplans; i++) if (!memcmp(o->plans[i].script, script, 4) && o->plans[i].kind == kind) return &o->plans[i];
    if (o->nplans == OTF_MAX_PLANS) {           /* the oldest one is replaced */
        OtfPlan *old = &o->plans[0];
        free(old->gsub); free(old->gpos); free(old->first);
        memmove(o->plans, o->plans + 1, sizeof *o->plans * (OTF_MAX_PLANS - 1));
        o->nplans--;
    }
    OtfPlan *p = &o->plans[o->nplans++];
    memset(p, 0, sizeof *p);
    memcpy(p->script, script, 4);
    p->kind = kind;
    p->indic = kind == OTF_KIND_INDIC;
    p->joining = kind == OTF_KIND_DEFAULT && joining_script(script);
    p->zero_marks = kind == OTF_KIND_INDIC || kind == OTF_KIND_KHMER ? OTF_ZERO_NONE :
                    kind == OTF_KIND_MYANMAR || kind == OTF_KIND_USE ? OTF_ZERO_EARLY : OTF_ZERO_LATE;
    if (o->nglyphs > 0) {
#define SPEC(a) a, (int)(sizeof a / sizeof *a)
        switch (kind) {
        case OTF_KIND_INDIC: p->ngsub = collect(o, o->gsub, script, SPEC(indic_features), 1, &p->gsub, NULL); break;
        case OTF_KIND_KHMER: p->ngsub = collect(o, o->gsub, script, SPEC(khmer_features), 1, &p->gsub, NULL); break;
        case OTF_KIND_MYANMAR: p->ngsub = collect(o, o->gsub, script, SPEC(myanmar_features), 1, &p->gsub, NULL); break;
        case OTF_KIND_USE: p->ngsub = collect(o, o->gsub, script, SPEC(use_features), 1, &p->gsub, NULL); break;
        default: p->ngsub = collect(o, o->gsub, script, SPEC(gsub_features), p->joining, &p->gsub, NULL);
        }
        p->ngpos = collect(o, o->gpos, script, SPEC(gpos_features), 0, &p->gpos, &p->gpos_kern);
#undef SPEC
        size_t bytes = (size_t)(o->nglyphs + 7) / 8;
        p->first = p->ngsub ? calloc((size_t)p->ngsub, bytes) : NULL;
        if (p->first) for (int j = 0; j < p->ngsub; j++) lookup_bits(o, p->gsub[j].lookup, p->first + (size_t)j * bytes);
        else p->ngsub = 0;
    }
    return p;
}

const OtfPlan *otf_plan(Otf *o, const char *script) {
    return otf_plan_kind(o, script, indic_script(script) ? OTF_KIND_INDIC : OTF_KIND_DEFAULT);
}

void otf_substitute_stages(const Otf *o, const OtfPlan *p, GlyphRun *run, int from, int to) {
    if (!p || !p->ngsub) return;
    Ctx c = { o, run, NULL, T_GSUB, 0, 0, 0, 0, 0, 0, NULL };
    size_t bytes = (size_t)(o->nglyphs + 7) / 8;
    for (int j = 0; j < p->ngsub && c.ops <= 400000; j++)
        if (p->gsub[j].stage >= from && p->gsub[j].stage <= to)
            run_lookup(&c, p->gsub[j].lookup, p->gsub[j].mask, p->first + (size_t)j * bytes, p->gsub[j].per_syllable);
}

void otf_substitute(const Otf *o, const OtfPlan *p, GlyphRun *run) { otf_substitute_stages(o, p, run, 0, 255); }

int otf_would_substitute(const Otf *o, const OtfPlan *p, int stage, const uint16_t *g, int n) {
    if (!p || n <= 0 || n > 8) return 0;
    uint16_t gg[16];
    int cl[16];
    for (int k = 0; k < p->ngsub; k++) {
        if (p->gsub[k].stage != stage) continue;
        memcpy(gg, g, sizeof *g * (size_t)n);
        for (int i = 0; i < n; i++) cl[i] = i;
        GlyphRun r = { gg, cl, n, 16, NULL, NULL, NULL };
        Ctx c = { o, &r, NULL, T_GSUB, 0, 0, 0, 0, 0, 0, NULL };
        int next;
        if (apply_at(&c, p->gsub[k].lookup, 0, 0, &next) && next >= n) return 1;
        if (r.len != n || memcmp(gg, g, sizeof *g * (size_t)n)) return 1;   /* something changed */
    }
    return 0;
}

int otf_has_gsub_script(const Otf *o, const char *tag) {
    if (!o->gsub) return 0;
    uint32_t sl = o->gsub + u16(o, o->gsub + 4);
    unsigned n = u16(o, sl);
    for (unsigned i = 0; i < n && i < 512; i++) if (tag_is(o, sl + 2 + 6 * i, tag)) return 1;
    return 0;
}

int otf_has_script(const Otf *o, const char *tag) {
    uint32_t tabs[2] = { o->gsub, o->gpos };
    for (int t = 0; t < 2; t++) {
        if (!tabs[t]) continue;
        uint32_t sl = tabs[t] + u16(o, tabs[t] + 4);
        unsigned n = u16(o, sl);
        for (unsigned i = 0; i < n && i < 512; i++) if (tag_is(o, sl + 2 + 6 * i, tag)) return 1;
    }
    return 0;
}

static int hadvance(const Otf *o, unsigned g) {
    if (!o->hmtx || !o->nhmetrics) return 0;
    unsigned k = g < o->nhmetrics ? g : o->nhmetrics - 1;
    return (int)u16(o, o->hmtx + 4 * k);
}

static int cursive_y(OtfPos *pos, int i, int depth) {
    if (pos[i].cur < 0 || depth > 64) return pos[i].dy;
    int py = cursive_y(pos, pos[i].cur, depth + 1);
    pos[i].dy += py;
    pos[i].cur = -1;
    return pos[i].dy;
}

void otf_position(const Otf *o, const OtfPlan *p, const GlyphRun *run, OtfPos *pos, int rtl) {
    otf_position_ign(o, p, run, pos, rtl, NULL);
}

void otf_position_ign(const Otf *o, const OtfPlan *p, const GlyphRun *run, OtfPos *pos, int rtl, const uint8_t *ign) {
    for (int i = 0; i < run->len; i++) { pos[i].adv = hadvance(o, run->g[i]); pos[i].dx = pos[i].dy = 0; pos[i].att = pos[i].cur = -1; }
    int zero = p ? p->zero_marks : OTF_ZERO_LATE;
    /* marks zero-width by GDEF: before GPOS (Myanmar, USE), after (the rest) or never
     * (Indic, Khmer), like HarfBuzz */
    if (o->glyph_class && zero == OTF_ZERO_EARLY)
        for (int i = 0; i < run->len; i++) if (glyph_class(o, run->g[i]) == GC_MARK) pos[i].adv = 0;
    if (p && p->ngpos) {
        Ctx c = { o, (GlyphRun *)run, pos, T_GPOS, 0, 0, 0, 0, 0, rtl, ign };
        for (int j = 0; j < p->ngpos && c.ops <= 400000; j++) run_lookup(&c, p->gpos[j].lookup, 0, NULL, 0);
        for (int i = 0; i < run->len; i++) if (pos[i].cur >= 0) cursive_y(pos, i, 0);    /* cursive chains */
    }
    if (o->glyph_class && zero == OTF_ZERO_LATE)
        for (int i = 0; i < run->len; i++) if (glyph_class(o, run->g[i]) == GC_MARK) pos[i].adv = 0;
}

/* ---- cmap ---- */

uint16_t otf_cmap(const Otf *o, uint32_t cp) {
    uint32_t c = o->cmap;
    if (!c) return 0;
    unsigned fmt = u16(o, c);
    if (fmt == 12) {
        int lo = 0, hi = (int)u32(o, c + 12) - 1;
        while (lo <= hi) {
            int mid = lo + (hi - lo) / 2;
            uint32_t g = c + 16 + 12 * (uint32_t)mid, s = u32(o, g), e = u32(o, g + 4);
            if (cp < s) hi = mid - 1;
            else if (cp > e) lo = mid + 1;
            else return (uint16_t)(u32(o, g + 8) + cp - s);
        }
        return 0;
    }
    if (fmt == 4 && cp <= 0xffff) {
        unsigned seg2 = u16(o, c + 6), segs = seg2 / 2;
        uint32_t ends = c + 14, starts = ends + seg2 + 2, deltas = starts + seg2, ranges = deltas + seg2;
        int lo = 0, hi = (int)segs - 1;
        while (lo <= hi) {
            int mid = (lo + hi) / 2;
            unsigned e = u16(o, ends + 2 * (uint32_t)mid), s = u16(o, starts + 2 * (uint32_t)mid);
            if (cp > e) lo = mid + 1;
            else if (cp < s) hi = mid - 1;
            else {
                unsigned delta = u16(o, deltas + 2 * (uint32_t)mid), ro = u16(o, ranges + 2 * (uint32_t)mid);
                if (!ro) return (uint16_t)(cp + delta);
                unsigned g = u16(o, ranges + 2 * (uint32_t)mid + ro + 2 * (cp - s));
                return g ? (uint16_t)(g + delta) : 0;
            }
        }
    }
    return 0;
}

/* ---- CBDT / CBLC ---- */

const uint8_t *otf_color_png(const Otf *o, uint16_t glyph, int ppem, size_t *len, int *strike_ppem) {
    if (!o->cblc || !o->cbdt) return NULL;
    uint32_t cb = o->cblc;
    unsigned nsizes = (unsigned)u32(o, cb + 4);
    int best = -1, best_ppem = 0;
    for (unsigned k = 0; k < nsizes && k < 64; k++) {
        uint32_t bs = cb + 8 + 48 * k;
        int pp = (int)u8(o, bs + 45);
        if (glyph < u16(o, bs + 40) || glyph > u16(o, bs + 42)) continue;
        if (best < 0 || (best_ppem < ppem && pp > best_ppem) || (pp >= ppem && pp < best_ppem)) { best = (int)k; best_ppem = pp; }
    }
    if (best < 0) return NULL;
    uint32_t bs = cb + 8 + 48 * (uint32_t)best;
    uint32_t arr = cb + u32(o, bs);
    unsigned nsub = (unsigned)u32(o, bs + 8);
    for (unsigned k = 0; k < nsub && k < 4096; k++) {
        uint32_t e = arr + 8 * k;
        unsigned first = u16(o, e), last = u16(o, e + 2);
        if (glyph < first || glyph > last) continue;
        uint32_t h = arr + u32(o, e + 4);
        unsigned ifmt = u16(o, h), imgfmt = u16(o, h + 2);
        uint32_t data = o->cbdt + u32(o, h + 4), off, end;
        unsigned gi = glyph - first;
        if (ifmt == 1) { off = u32(o, h + 8 + 4 * gi); end = u32(o, h + 12 + 4 * gi); }
        else if (ifmt == 3) { off = u16(o, h + 8 + 2 * gi); end = u16(o, h + 10 + 2 * gi); }
        else if (ifmt == 2) { uint32_t sz = u32(o, h + 8); off = sz * gi; end = off + sz; }
        else return NULL;
        if (end <= off) return NULL;
        uint32_t p = data + off;
        uint32_t hdr = imgfmt == 17 ? 5 : imgfmt == 18 ? 8 : imgfmt == 19 ? 0 : 0xffffffffu;
        if (hdr == 0xffffffffu) return NULL;
        uint32_t dl = u32(o, p + hdr), png = p + hdr + 4;
        if (png >= o->n || dl > o->n - png || dl > end - off) return NULL;
        *len = dl;
        *strike_ppem = best_ppem;
        return o->d + png;
    }
    return NULL;
}
