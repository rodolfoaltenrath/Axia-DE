/* shape_khmer.c — Khmer shaping, like HarfBuzz's Khmer shaper:
 *
 *   1. normalization; split vowels without a Unicode decomposition (ើ ឿ ៀ ោ ៅ) are
 *      separated into េ + the vowel;
 *   2. syllables (consonant with coeng, robat, vowels, signs);
 *   3. ◌ in broken syllables; reordering: coeng + ro goes before the base (with pref and the
 *      rest with cfar) and the left-side vowel ( េ ែ ៃ) at the start; masks
 *      blwf/abvf/pstf after the base;
 *   4. locl, ccmp, pref, blwf, abvf, pstf together per syllable; then pres, abvs, blws,
 *      psts, clig… and GPOS. */
#include "shape_syl.h"

#include <stdlib.h>
#include <string.h>

#include "unicode.h"

static const char *const cats[] = { SEA_NAMES };

static SylGrammar grammar = {
    cats, (int)(sizeof cats / sizeof *cats),
    "c = (C | Ra | V);"
    "cn = c ((ZWJ|ZWNJ)? Robatic)?;"
    "joiner = (ZWJ | ZWNJ);"
    "xgroup = (joiner* Xgroup)*;"
    "ygroup = Ygroup*;"
    "matra_group = VPre? xgroup VBlw? xgroup (joiner? VAbv)? xgroup VPst?;"
    "syllable_tail = xgroup matra_group xgroup (H c)? ygroup;"
    "broken_cluster = Robatic? (H cn)* (H | syllable_tail);"
    "consonant_syllable = (cn | PLACEHOLDER | DOTTEDCIRCLE) broken_cluster;",
    (const char *const[]){ "consonant_syllable", "broken_cluster", "any" }, 3, NULL,
};
enum { T_CONSONANT, T_BROKEN, T_OTHER };

static int split(uint32_t ab, uint32_t *a, uint32_t *b) {
    switch (ab) {
    case 0x17BE: case 0x17BF: case 0x17C0: case 0x17C4: case 0x17C5: *a = 0x17C1; *b = ab; return 1;
    default: return 0;
    }
}

#define CAT(i) ((int)OTF_AUX_CAT(r->aux[i]))

static void reorder(GlyphRun *r, int start, int end) {
    for (int i = start + 1; i < end; i++) r->mask[i] |= OTF_F_BLWF | OTF_F_ABVF | OTF_F_PSTF;
    int coengs = 0;
    for (int i = start + 1; i < end; i++) {
        if (CAT(i) == SEA_H && coengs <= 2 && i + 1 < end) {
            coengs++;
            if (CAT(i + 1) == SEA_Ra) {         /* coeng + ro: before the base, with pref */
                r->mask[i] |= OTF_F_PREF;
                r->mask[i + 1] |= OTF_F_PREF;
                syl_move(r, i, start);
                syl_move(r, i + 1, start + 1);
                for (int j = i + 2; j < end; j++) r->mask[j] |= OTF_F_CFAR;
                coengs = 2;
            }
        } else if (CAT(i) == SEA_VPre) syl_move(r, i, start);     /* left-side vowel */
    }
}

int shape_khmer(Shaper *s, Otf *o, const uint32_t *in, int n, int (*reserve)(Shaper *, int)) {
    GlyphRun *r = &s->run;
    r->len = 0;
    int m = syl_normalize(s, o, in, n, split, 0);
    if (m <= 0 || reserve(s, m * 2 + 8)) return 0;
    uint8_t *cat = malloc((size_t)m + 1);
    if (!cat) return 0;
    for (int i = 0; i < m; i++) cat[i] = (uint8_t)uc_sea_cat(s->cp[i]);
    int syl = 0;
    for (int i = 0; i < m;) {
        int alt, l = syl_next(&grammar, cat + i, m - i, &alt);
        syl = syl % 255 + 1;
        for (int j = i; j < i + l; j++) {
            r->g[j] = otf_cmap(o, s->cp[j]);
            r->cl[j] = s->order[j];
            r->mask[j] = OTF_F_GLOBAL;
            r->lig[j] = 0;
            r->aux[j] = cat[j] | (uint32_t)syl << 16 | (uint32_t)alt << 28;
        }
        i += l;
    }
    free(cat);
    r->len = m;
    const OtfPlan *p = otf_plan_kind(o, "khmr", OTF_KIND_KHMER);
    s->plan = p;
    syl_dotted_circles(s, o, T_BROKEN, SEA_DOTTEDCIRCLE, -1, reserve);
    for (int i = 0; i < r->len;) {
        int e = i + 1;
        while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) e++;
        if (SYL_TYPE(r->aux[i]) != T_OTHER) reorder(r, i, e);
        i = e;
    }
    otf_substitute(o, p, r);
    return r->len;
}
