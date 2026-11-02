/* shape_indic.c — shaping of the Indic scripts (Devanagari, Bengali, Gurmukhi, Gujarati,
 * Oriya, Tamil, Telugu, Kannada, Malayalam), following HarfBuzz's Indic shaper:
 *
 *   1. normalization: split matras are separated (ো = ে + া) and so are nukta consonants
 *      excluded from composition (क़ = क + ़); whatever the font has is recomposed;
 *   2. category and position of each character (IndicSyllabicCategory/PositionalCategory) and the
 *      position of each consonant according to the font (does it have a below-base form, a post-base one?);
 *   3. syllables (consonant, vowel, standalone, broken: with dotted circle ◌);
 *   4. locl, ccmp; initial reordering: base, reph, matras that go before the consonant,
 *      rphf/half/blwf/abvf/pstf/pref masks;
 *   5. nukt, akhn, rphf, rkrf, pref, blwf, abvf, half, pstf, vatu, cjct (each in its own stage);
 *   6. final reordering: the pre-base matra goes after the half forms, the reph to its
 *      place according to the script, the pref consonant before the base, init at word start;
 *   7. pres, abvs, blws, psts, haln, calt, liga… and GPOS (abvm, blwm, dist, kern, mark, mkmk).
 *
 * Compared against HarfBuzz in server/tests/shape_test.c. */
#include "shape.h"

#include <stdlib.h>
#include <string.h>

#include "unicode.h"

enum { P_START, P_RA_TO_BECOME_REPH, P_PRE_M, P_PRE_C, P_BASE_C, P_AFTER_MAIN, P_ABOVE_C, P_BEFORE_SUB, P_BELOW_C,
       P_AFTER_SUB, P_BEFORE_POST, P_POST_C, P_AFTER_POST, P_SMVD, P_END };
enum { RP_AFTER_MAIN, RP_BEFORE_SUB, RP_AFTER_SUB, RP_BEFORE_POST, RP_AFTER_POST };
enum { RM_IMPLICIT, RM_EXPLICIT, RM_LOG_REPHA };
enum { BM_PRE_AND_POST, BM_POST_ONLY };
enum { S_DEVA, S_BENG, S_GURU, S_GUJR, S_ORYA, S_TAML, S_TELU, S_KNDA, S_MLYM };

typedef struct { const char *tag2, *tag; uint32_t lo, virama; int reph_pos, reph_mode, blwf_mode; } Cfg;
static const Cfg cfgs[] = {
    { "dev2", "deva", 0x0900, 0x094D, RP_BEFORE_POST, RM_IMPLICIT, BM_PRE_AND_POST },
    { "bng2", "beng", 0x0980, 0x09CD, RP_AFTER_SUB, RM_IMPLICIT, BM_PRE_AND_POST },
    { "gur2", "guru", 0x0A00, 0x0A4D, RP_BEFORE_SUB, RM_IMPLICIT, BM_PRE_AND_POST },
    { "gjr2", "gujr", 0x0A80, 0x0ACD, RP_BEFORE_POST, RM_IMPLICIT, BM_PRE_AND_POST },
    { "ory2", "orya", 0x0B00, 0x0B4D, RP_AFTER_MAIN, RM_IMPLICIT, BM_PRE_AND_POST },
    { "tml2", "taml", 0x0B80, 0x0BCD, RP_AFTER_POST, RM_IMPLICIT, BM_PRE_AND_POST },
    { "tel2", "telu", 0x0C00, 0x0C4D, RP_AFTER_POST, RM_EXPLICIT, BM_POST_ONLY },
    { "knd2", "knda", 0x0C80, 0x0CCD, RP_AFTER_POST, RM_IMPLICIT, BM_POST_ONLY },
    { "mlm2", "mlym", 0x0D00, 0x0D4D, RP_AFTER_MAIN, RM_LOG_REPHA, BM_PRE_AND_POST },
};

int shape_indic_script(uint32_t cp) {
    for (int i = 0; i < (int)(sizeof cfgs / sizeof *cfgs); i++) if (cp >= cfgs[i].lo && cp < cfgs[i].lo + 0x80) return i;
    return -1;
}

/* Position of a matra according to its side and the script (HarfBuzz's MATRA_POS_*). */
static int matra_pos(uint32_t u, int side, int sc) {
    switch (side) {
    case P_PRE_C: return P_PRE_M;
    case P_POST_C:
        switch (sc) {
        case S_DEVA: return P_AFTER_SUB;
        case S_TELU: return u <= 0x0C42 ? P_BEFORE_SUB : P_AFTER_SUB;
        case S_KNDA: return u < 0x0CC3 || u > 0x0CD6 ? P_BEFORE_SUB : P_AFTER_SUB;
        default: return P_AFTER_POST;
        }
    case P_ABOVE_C:
        switch (sc) {
        case S_GURU: return P_AFTER_POST;
        case S_ORYA: return P_AFTER_MAIN;
        case S_TELU: case S_KNDA: return P_BEFORE_SUB;
        default: return P_AFTER_SUB;
        }
    case P_BELOW_C:
        switch (sc) {
        case S_GURU: case S_GUJR: case S_TAML: case S_MLYM: return P_AFTER_POST;
        case S_TELU: case S_KNDA: return P_BEFORE_SUB;
        default: return P_AFTER_SUB;
        }
    default: return side;
    }
}

static int is_consonant_cat(int c) {
    return c == IC_C || c == IC_CS || c == IC_Ra || c == IC_CM || c == IC_V || c == IC_PLACEHOLDER || c == IC_DOTTEDCIRCLE;
}
static int is_joiner_cat(int c) { return c == IC_ZWJ || c == IC_ZWNJ; }

static void props(uint32_t u, int sc, int *cat, int *pos) {
    int c = uc_indic_cat(u), ip = uc_indic_pos(u);
    static const int map[] = { P_END, P_PRE_C, P_ABOVE_C, P_BELOW_C, P_POST_C, P_AFTER_MAIN, P_PRE_M };
    int p = ip >= 0 && ip < 7 ? map[ip] : P_END;
    if (is_consonant_cat(c)) p = P_BASE_C;
    else if (c == IC_M) p = matra_pos(u, p, sc);
    else if (c == IC_SM || c == IC_A || c == IC_Symbol) p = P_SMVD;
    if (u == 0x0B01) p = P_BEFORE_SUB;     /* Oriya bindu */
    *cat = c;
    *pos = p;
}

/* ---- run arrays (g, cl, mask, lig, aux go together) ---- */

typedef struct { uint16_t g; int cl; uint32_t mask; uint8_t lig; uint32_t aux; } Info;

static Info get(const GlyphRun *r, int i) { Info x = { r->g[i], r->cl[i], r->mask[i], r->lig[i], r->aux[i] }; return x; }
static void put(GlyphRun *r, int i, Info x) { r->g[i] = x.g; r->cl[i] = x.cl; r->mask[i] = x.mask; r->lig[i] = x.lig; r->aux[i] = x.aux; }

#define CAT(i) ((int)OTF_AUX_CAT(r->aux[i]))
#define POS(i) ((int)OTF_AUX_POS(r->aux[i]))
static void set_pos(GlyphRun *r, int i, int p) { r->aux[i] = (r->aux[i] & ~0xff00u) | (uint32_t)p << 8; }
static void set_cat(GlyphRun *r, int i, int c) { r->aux[i] = (r->aux[i] & ~0xffu) | (uint32_t)c; }
static int is_cons(const GlyphRun *r, int i) { return is_consonant_cat(CAT(i)); }
static int is_joiner(const GlyphRun *r, int i) { return is_joiner_cat(CAT(i)); }
static int is_halant(const GlyphRun *r, int i) { return CAT(i) == IC_H; }
static int ligated_not_multiplied(const GlyphRun *r, int i) { return (r->aux[i] & OTF_AUX_LIGATED) && !(r->aux[i] & OTF_AUX_MULTIPLIED); }

/* Moves the element from to position to (shifting the rest). */
static void move(GlyphRun *r, int from, int to) {
    if (from == to) return;
    Info x = get(r, from);
    if (from < to) for (int i = from; i < to; i++) put(r, i, get(r, i + 1));
    else for (int i = from; i > to; i--) put(r, i, get(r, i - 1));
    put(r, to, x);
}

static void reverse(GlyphRun *r, int a, int b) {      /* [a, b) */
    for (b--; a < b; a++, b--) { Info x = get(r, a); put(r, a, get(r, b)); put(r, b, x); }
}

/* ---- syllables (HarfBuzz's state machine, written by hand) ---- */

enum { SY_CONSONANT, SY_VOWEL, SY_STANDALONE, SY_SYMBOL, SY_BROKEN, SY_OTHER };

typedef struct { const int *cat; int n; } Cats;
#define C_(i) ((i) < k->n ? k->cat[i] : -1)

static int is_c(int c) { return c == IC_C || c == IC_Ra; }
static int p_n(const Cats *k, int j) {           /* n = ((ZWNJ? RS)? (N N?)?) */
    if (C_(j) == IC_ZWNJ && C_(j + 1) == IC_RS) j += 2;
    else if (C_(j) == IC_RS) j++;
    if (C_(j) == IC_N) { j++; if (C_(j) == IC_N) j++; }
    return j;
}
static int p_cn(const Cats *k, int j) {          /* cn = c ZWJ? n */
    j++;
    if (C_(j) == IC_ZWJ) j++;
    return p_n(k, j);
}
static int p_halant_group(const Cats *k, int j) {   /* z? H (ZWJ N?)? ; -1 if not */
    int s = j;
    if (is_joiner_cat(C_(j)) && C_(j + 1) == IC_H) j++;
    if (C_(j) != IC_H) return -1;
    j++;
    if (C_(j) == IC_ZWJ) { j++; if (C_(j) == IC_N) j++; }
    (void)s;
    return j;
}
static int p_matra_group(const Cats *k, int j) {    /* z* M N? H? ; -1 if not */
    int s = j;
    while (is_joiner_cat(C_(j))) j++;
    if (C_(j) != IC_M) return -1;
    j++;
    if (C_(j) == IC_N) j++;
    if (C_(j) == IC_H) j++;
    (void)s;
    return j;
}
static int p_tail(const Cats *k, int j) {           /* (z? SM SM? ZWNJ?)? (A)* */
    int t = j;
    if (is_joiner_cat(C_(t)) && C_(t + 1) == IC_SM) t++;
    if (C_(t) == IC_SM) {
        t++;
        if (C_(t) == IC_SM) t++;
        if (C_(t) == IC_ZWNJ) t++;
        j = t;
    }
    while (C_(j) == IC_A) j++;
    return j;
}
static int p_complex_tail(const Cats *k, int j) {
    for (;;) {                                       /* (halant_group cn)* */
        int h = p_halant_group(k, j);
        if (h < 0 || !is_c(C_(h))) break;
        j = p_cn(k, h);
    }
    if (C_(j) == IC_CM) j++;                         /* medial */
    int h = p_halant_group(k, j);                    /* final_halant_group | matra_group* */
    if (h >= 0) j = h;
    else if (C_(j) == IC_H && C_(j + 1) == IC_ZWNJ) j += 2;
    else for (int m; (m = p_matra_group(k, j)) >= 0;) j = m;
    if (h >= 0 && C_(j) == IC_ZWNJ && C_(j - 1) == IC_H) j++;
    return p_tail(k, j);
}

/* Length of the syllable starting at i and its type. */
static int syllable(const Cats *k, int i, int *type) {
    int best = 0;
    *type = SY_OTHER;
    /* consonante: (Repha|CS)? cn complex_tail */
    {
        int j = i;
        if (C_(j) == IC_Repha || C_(j) == IC_CS) j++;
        if (is_c(C_(j))) { j = p_complex_tail(k, p_cn(k, j)); if (j - i > best) { best = j - i; *type = SY_CONSONANT; } }
        else if (j != i && is_c(C_(i))) {}
    }
    /* vocal: reph? V n (ZWJ | complex_tail) */
    {
        int j = i;
        if (C_(j) == IC_Ra && C_(j + 1) == IC_H) j += 2;
        else if (C_(j) == IC_Repha) j++;
        if (C_(j) == IC_V) {
            j = p_n(k, j + 1);
            j = C_(j) == IC_ZWJ ? j + 1 : p_complex_tail(k, j);
            if (j - i > best) { best = j - i; *type = SY_VOWEL; }
        }
    }
    /* independiente: ((Repha|CS)? PLACEHOLDER | reph? DOTTEDCIRCLE) n complex_tail */
    {
        int j = i, ok = 0;
        if ((C_(j) == IC_Repha || C_(j) == IC_CS) && C_(j + 1) == IC_PLACEHOLDER) { j += 2; ok = 1; }
        else if (C_(j) == IC_PLACEHOLDER) { j++; ok = 1; }
        else {
            if (C_(j) == IC_Ra && C_(j + 1) == IC_H) j += 2;
            else if (C_(j) == IC_Repha) j++;
            if (C_(j) == IC_DOTTEDCIRCLE) { j++; ok = 1; } else j = i;
        }
        if (ok) { j = p_complex_tail(k, p_n(k, j)); if (j - i > best) { best = j - i; *type = SY_STANDALONE; } }
    }
    /* symbol: Symbol N? tail */
    if (C_(i) == IC_Symbol) {
        int j = i + 1;
        if (C_(j) == IC_N) j++;
        j = p_tail(k, j);
        if (j - i > best) { best = j - i; *type = SY_SYMBOL; }
    }
    /* broken: reph? n complex_tail (at least one character) */
    if (!best) {
        int j = i;
        if (C_(j) == IC_Ra && C_(j + 1) == IC_H) j += 2;
        else if (C_(j) == IC_Repha) j++;
        j = p_complex_tail(k, p_n(k, j));
        if (j > i) { best = j - i; *type = SY_BROKEN; }
    }
    if (!best) { best = 1; *type = SY_OTHER; }
    return best;
}

/* ---- reorderings ---- */

typedef struct {
    Otf *o;
    const OtfPlan *p;
    const Cfg *cfg;
    int sc;
    uint16_t virama;
    int has_rphf, has_pref;
} Indic;

static int would(const Indic *in, int stage, const uint16_t *g, int n) { return otf_would_substitute(in->o, in->p, stage, g, n); }

static int consonant_position(const Indic *in, uint16_t g) {
    if (!in->virama) return P_BASE_C;
    uint16_t a[3] = { in->virama, g, in->virama };
    if (would(in, OTF_ST_BLWF, a, 2) || would(in, OTF_ST_BLWF, a + 1, 2) || would(in, OTF_ST_VATU, a, 2) || would(in, OTF_ST_VATU, a + 1, 2))
        return P_BELOW_C;
    if (would(in, OTF_ST_PSTF, a, 2) || would(in, OTF_ST_PSTF, a + 1, 2)) return P_POST_C;
    if (would(in, OTF_ST_PREF, a, 2) || would(in, OTF_ST_PREF, a + 1, 2)) return P_POST_C;
    return P_BASE_C;
}

static void initial_reorder(const Indic *in, GlyphRun *r, int start, int end) {
    int base = end, has_reph = 0, limit = start;
    /* Kannada: Ra+H+ZWJ behaves like Ra+ZWJ+H (compatibility with old texts) */
    if (in->sc == S_KNDA && start + 3 <= end && CAT(start) == IC_Ra && CAT(start + 1) == IC_H && CAT(start + 2) == IC_ZWJ) {
        Info x = get(r, start + 1);
        put(r, start + 1, get(r, start + 2));
        put(r, start + 2, x);
    }
    if (in->has_rphf && start + 3 <= end &&
        ((in->cfg->reph_mode == RM_IMPLICIT && !is_joiner(r, start + 2)) ||
         (in->cfg->reph_mode == RM_EXPLICIT && CAT(start + 2) == IC_ZWJ))) {
        uint16_t g[3] = { r->g[start], r->g[start + 1], in->cfg->reph_mode == RM_EXPLICIT ? r->g[start + 2] : 0 };
        if (would(in, OTF_ST_RPHF, g, 2) || (in->cfg->reph_mode == RM_EXPLICIT && would(in, OTF_ST_RPHF, g, 3))) {
            limit += 2;
            while (limit < end && is_joiner(r, limit)) limit++;
            base = start;
            has_reph = 1;
        }
    } else if (in->cfg->reph_mode == RM_LOG_REPHA && CAT(start) == IC_Repha) {
        limit += 1;
        while (limit < end && is_joiner(r, limit)) limit++;
        base = start;
        has_reph = 1;
    }
    /* base: from the end, the first consonant with neither a below-base nor a post-base form */
    {
        int i = end, seen_below = 0;
        do {
            i--;
            if (is_cons(r, i)) {
                if (POS(i) != P_BELOW_C && (POS(i) != P_POST_C || seen_below)) { base = i; break; }
                if (POS(i) == P_BELOW_C) seen_below = 1;
                base = i;
            } else if (start < i && CAT(i) == IC_ZWJ && CAT(i - 1) == IC_H) break;
        } while (i > limit);
    }
    if (has_reph && base == start && limit - base <= 2) has_reph = 0;

    for (int i = start; i < base; i++) if (POS(i) > P_PRE_C) set_pos(r, i, P_PRE_C);
    if (base < end) set_pos(r, base, P_BASE_C);
    if (has_reph) set_pos(r, start, P_RA_TO_BECOME_REPH);

    /* marks, joiners and halants move with what precedes them */
    int last_pos = P_START;
    for (int i = start; i < end; i++) {
        int c = CAT(i);
        if (is_joiner_cat(c) || c == IC_N || c == IC_RS || c == IC_CM || c == IC_H) {
            set_pos(r, i, last_pos);
            if (c == IC_H && POS(i) == P_PRE_M)
                for (int j = i; j > start; j--) if (POS(j - 1) != P_PRE_M) { set_pos(r, i, POS(j - 1)); break; }
        } else if (POS(i) != P_SMVD) last_pos = POS(i);
    }
    /* post-base consonants take along what comes before them */
    {
        int last = base;
        for (int i = base + 1; i < end; i++)
            if (is_cons(r, i)) {
                for (int j = last + 1; j < i; j++) if (POS(j) < P_SMVD) set_pos(r, j, POS(i));
                last = i;
            } else if (CAT(i) == IC_M) last = i;
    }
    /* stable order by position */
    for (int i = start + 1; i < end; i++) {
        int j = i;
        while (j > start && POS(j - 1) > POS(i)) j--;
        move(r, i, j);
    }
    /* base again; consecutive left matras are reversed */
    int first_left = end, last_left = end;
    base = end;
    for (int i = start; i < end; i++) {
        if (POS(i) == P_BASE_C) { base = i; break; }
        if (POS(i) == P_PRE_M) { if (first_left == end) first_left = i; last_left = i; }
    }
    if (first_left < last_left) {
        reverse(r, first_left, last_left + 1);
        int i = first_left;
        for (int j = i; j <= last_left; j++) if (CAT(j) == IC_M) { reverse(r, i, j + 1); i = j + 1; }
    }

    /* masks */
    for (int i = start; i < end && POS(i) == P_RA_TO_BECOME_REPH; i++) r->mask[i] |= OTF_F_RPHF;
    uint32_t pre = OTF_F_HALF | (in->cfg->blwf_mode == BM_PRE_AND_POST ? OTF_F_BLWF : 0);
    for (int i = start; i < base && i < end; i++) r->mask[i] |= pre;
    for (int i = base + 1; i < end; i++) r->mask[i] |= OTF_F_BLWF | OTF_F_ABVF | OTF_F_PSTF;
    if (in->has_pref && base + 2 < end)
        for (int i = base + 1; i + 1 < end; i++) {
            uint16_t g[2] = { r->g[i], r->g[i + 1] };
            if (would(in, OTF_ST_PREF, g, 2)) { r->mask[i] |= OTF_F_PREF; r->mask[i + 1] |= OTF_F_PREF; break; }
        }
    /* ZWNJ breaks the half forms */
    for (int i = base + 1; i < end; i++)
        if (is_joiner(r, i)) {
            int nonj = CAT(i) == IC_ZWNJ, j = i;
            do { j--; if (nonj) r->mask[j] &= ~(uint32_t)OTF_F_HALF; } while (j > start && !is_cons(r, j));
        }
}

static void final_reorder(const Indic *in, GlyphRun *r, int start, int end, const uint32_t *src) {
    /* a halant that was ligated and split again is still a halant */
    if (in->virama)
        for (int i = start; i < end; i++)
            if (r->g[i] == in->virama && (r->aux[i] & OTF_AUX_LIGATED) && (r->aux[i] & OTF_AUX_MULTIPLIED)) {
                set_cat(r, i, IC_H);
                r->aux[i] &= ~(uint32_t)(OTF_AUX_LIGATED | OTF_AUX_MULTIPLIED);
            }
    int try_pref = in->has_pref, base;
    for (base = start; base < end; base++)
        if (POS(base) >= P_BASE_C) {
            if (try_pref && base + 1 < end) {
                for (int i = base + 1; i < end; i++)
                    if (r->mask[i] & OTF_F_PREF) {
                        if (!((r->aux[i] & OTF_AUX_SUBST) && ligated_not_multiplied(r, i))) {
                            base = i;
                            while (base < end && is_halant(r, base)) base++;
                            if (base < end) set_pos(r, base, P_BASE_C);
                            try_pref = 0;
                        }
                        break;
                    }
                if (base == end) break;
            }
            if (in->sc == S_MLYM)
                for (int i = base + 1; i < end; i++) {
                    while (i < end && is_joiner(r, i)) i++;
                    if (i == end || !is_halant(r, i)) break;
                    i++;
                    while (i < end && is_joiner(r, i)) i++;
                    if (i < end && is_cons(r, i) && POS(i) == P_BELOW_C) { base = i; set_pos(r, base, P_BASE_C); }
                }
            if (start < base && POS(base) > P_BASE_C) base--;
            break;
        }
    if (base == end && start < base && CAT(base - 1) == IC_ZWJ) base--;
    if (base < end) while (start < base && (CAT(base) == IC_N || CAT(base) == IC_H)) base--;

    /* pre-base matras: after the last half form (explicit halant) */
    if (start + 1 < end && start < base) {
        int new_pos = base == end ? base - 2 : base - 1;
        if (in->sc != S_MLYM && in->sc != S_TAML) {
        search:
            while (new_pos > start && !(CAT(new_pos) == IC_M || CAT(new_pos) == IC_H)) new_pos--;
            if (is_halant(r, new_pos) && POS(new_pos) != P_PRE_M) {
                if (new_pos + 1 < end && CAT(new_pos + 1) == IC_ZWJ && new_pos > start) { new_pos--; goto search; }
            } else new_pos = start;
        }
        if (start < new_pos && POS(new_pos) != P_PRE_M) {
            for (int i = new_pos; i > start; i--)
                if (POS(i - 1) == P_PRE_M) {
                    int old = i - 1;
                    if (old < base && base <= new_pos) base--;
                    move(r, old, new_pos);
                    new_pos--;
                }
        }
    }

    /* reph to its place */
    if (start + 1 < end && POS(start) == P_RA_TO_BECOME_REPH && ((CAT(start) == IC_Repha) ^ ligated_not_multiplied(r, start))) {
        int np, rp = in->cfg->reph_pos;
        if (rp == RP_AFTER_POST) goto step5;
        np = start + 1;
        while (np < base && !is_halant(r, np)) np++;
        if (np < base && is_halant(r, np)) {
            if (np + 1 < base && is_joiner(r, np + 1)) np++;
            goto do_move;
        }
        if (rp == RP_AFTER_MAIN) {
            np = base;
            while (np + 1 < end && POS(np + 1) <= P_AFTER_MAIN) np++;
            if (np < end) goto do_move;
        }
        if (rp == RP_AFTER_SUB) {
            np = base;
            while (np + 1 < end && !(POS(np + 1) == P_POST_C || POS(np + 1) == P_AFTER_POST || POS(np + 1) == P_SMVD)) np++;
            if (np < end) goto do_move;
        }
    step5:
        np = start + 1;
        while (np < base && !is_halant(r, np)) np++;
        if (np < base && is_halant(r, np)) {
            if (np + 1 < base && is_joiner(r, np + 1)) np++;
            goto do_move;
        }
        np = end - 1;
        while (np > start && POS(np) == P_SMVD) np--;
        if (is_halant(r, np))
            for (int i = base + 1; i < np; i++) if (CAT(i) == IC_M) np--;
    do_move:
        move(r, start, np);
        if (start < base && base <= np) base--;
    }

    /* consonante pre-base (pref) */
    if (try_pref && base + 1 < end)
        for (int i = base + 1; i < end; i++)
            if (r->mask[i] & OTF_F_PREF) {
                if (ligated_not_multiplied(r, i)) {
                    int np = base;
                    if (in->sc != S_MLYM && in->sc != S_TAML)
                        while (np > start && !(CAT(np - 1) == IC_M || CAT(np - 1) == IC_H)) np--;
                    if (np > start && is_halant(r, np - 1) && np < end && is_joiner(r, np)) np++;
                    move(r, i, np);
                    if (np <= base && base < i) base++;
                }
                break;
            }

    /* init: left matra at word start */
    if (POS(start) == P_PRE_M) {
        int word_start = start == 0;
        if (!word_start) {
            uint32_t prev = src[r->cl[start - 1]];
            word_start = uc_script(prev) <= SCRIPT_INHERITED && !uc_ccc(prev) && prev != 0x200C && prev != 0x200D;
        }
        if (word_start) r->mask[start] |= OTF_F_INIT;
    }
}

/* ---- normalization ---- */

static int decompose(uint32_t cp, uint32_t *out, int max) {
    uint32_t a, b;
    int ex;
    if (max >= 2 && uc_decompose(cp, &a, &b, &ex)) {
        int n = decompose(a, out, max - 1);
        out[n++] = b;
        return n;
    }
    out[0] = cp;
    return 1;
}

/* in → s->cp (and s->order: which input character each one came from). */
static int normalize(Shaper *s, Otf *o, const uint32_t *in, int n) {
    int cap = n * 3 + 8, m = 0;
    if (cap > s->cp_cap) {
        uint32_t *nc = realloc(s->cp, sizeof *nc * (size_t)cap);
        if (!nc) return -1;
        s->cp = nc;
        int *no = realloc(s->order, sizeof *no * (size_t)cap);
        if (!no) return -1;
        s->order = no;
        s->cp_cap = cap;
    }
    for (int i = 0; i < n; i++) {
        uint32_t d[4];
        int k = decompose(in[i], d, 4);
        for (int j = 0; j < k && m < cap; j++) { s->cp[m] = d[j]; s->order[m++] = i; }
    }
    /* recomposed if the first one is not a mark and the font has the composite */
    int w = 0;
    for (int i = 0; i < m; i++) {
        if (w > 0) {
            uint32_t a = s->cp[w - 1], b = s->cp[i], ab = uc_compose(a, b);
            if (a == 0x09AF && b == 0x09BC) ab = 0x09DF;
            if (ab && !uc_ccc(a) && uc_indic_cat(a) != IC_M && otf_cmap(o, ab)) { s->cp[w - 1] = ab; continue; }
        }
        s->cp[w] = s->cp[i];
        s->order[w++] = s->order[i];
    }
    return w;
}

/* ---- all together ---- */

int shape_indic(Shaper *s, Otf *o, const uint32_t *in, int n, int script, int (*reserve)(Shaper *, int)) {
    GlyphRun *r = &s->run;
    r->len = 0;
    int m = normalize(s, o, in, n);
    if (m <= 0 || reserve(s, m * 2 + 8)) return 0;
    Indic ind;
    ind.o = o;
    ind.cfg = &cfgs[script];
    ind.sc = script;
    ind.p = otf_plan(o, otf_has_script(o, ind.cfg->tag2) ? ind.cfg->tag2 : ind.cfg->tag);
    ind.virama = otf_cmap(o, ind.cfg->virama);
    ind.has_rphf = ind.has_pref = 0;
    for (int k = 0; k < ind.p->ngsub; k++) {
        if (ind.p->gsub[k].mask & OTF_F_RPHF) ind.has_rphf = 1;
        if (ind.p->gsub[k].mask & OTF_F_PREF) ind.has_pref = 1;
    }
    /* categories and syllables over the characters */
    int *cats = malloc(sizeof(int) * (size_t)(m + 1)), *poss = malloc(sizeof(int) * (size_t)(m + 1));
    if (!cats || !poss) { free(cats); free(poss); return 0; }
    for (int i = 0; i < m; i++) props(s->cp[i], script, &cats[i], &poss[i]);
    uint16_t dotted = otf_cmap(o, 0x25CC);
    int len = 0, syl = 0;
    Cats k = { cats, m };
    for (int i = 0; i < m;) {
        int type, l = syllable(&k, i, &type);
        syl++;
        int dc = type == SY_BROKEN && dotted;
        int reph = dc ? (cats[i] == IC_Ra && i + 1 < m && cats[i + 1] == IC_H ? 2 : cats[i] == IC_Repha ? 1 : 0) : 0;
        for (int j = 0; j < l; j++) {
            if (dc && j == reph) {          /* broken syllable: ◌ where the base would go */
                r->g[len] = dotted; r->cl[len] = s->order[i + j]; r->mask[len] = OTF_F_GLOBAL; r->lig[len] = 0;
                r->aux[len] = IC_DOTTEDCIRCLE | (uint32_t)P_BASE_C << 8 | (uint32_t)(syl & 0xff) << 16;
                len++;
            }
            int c = i + j, p = poss[c];
            uint16_t g = otf_cmap(o, s->cp[c]);
            if (is_consonant_cat(cats[c])) p = consonant_position(&ind, g);
            r->g[len] = g; r->cl[len] = s->order[c]; r->mask[len] = OTF_F_GLOBAL; r->lig[len] = 0;
            r->aux[len] = (uint32_t)cats[c] | (uint32_t)p << 8 | (uint32_t)(syl & 0xff) << 16 | (uint32_t)(type == SY_SYMBOL || type == SY_OTHER) << 31;
            len++;
        }
        i += l;
    }
    free(cats);
    free(poss);
    r->len = len;

    otf_substitute_stages(o, ind.p, r, 0, 0);
    for (int i = 0; i < r->len;) {               /* initial reordering per syllable */
        int e = i + 1;
        while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) e++;
        if (!(r->aux[i] >> 31)) initial_reorder(&ind, r, i, e);
        i = e;
    }
    otf_substitute_stages(o, ind.p, r, 1, OTF_ST_CJCT);
    for (int i = 0; i < r->len;) {               /* final reordering */
        int e = i + 1;
        while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) e++;
        if (!(r->aux[i] >> 31)) final_reorder(&ind, r, i, e, in);
        /* the syllable is a single cluster: drawing neither separates nor breaks it again */
        int mn = r->cl[i];
        for (int j = i; j < e; j++) if (r->cl[j] < mn) mn = r->cl[j];
        for (int j = i; j < e; j++) r->cl[j] = mn;
        i = e;
    }
    otf_substitute_stages(o, ind.p, r, OTF_ST_FINAL, 255);
    s->plan = ind.p;
    return r->len;
}
