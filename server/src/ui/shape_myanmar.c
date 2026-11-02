/* shape_myanmar.c — Myanmar shaping (and Mon, Shan, Karen…), like HarfBuzz's Myanmar shaper
 * (fonts with the mym2 tag; the old "mymr" ones go through the common shaper):
 *
 *   1. normalization and syllables (kinzi, consonant, medials, vowels, tones);
 *   2. locl and ccmp per syllable;
 *   3. ◌ in broken syllables; reordering: kinzi after the base, the medial ra (ြ) and the
 *      vowel ေ before it, the below ones in their order;
 *   4. rphf, pref, blwf, pstf one at a time per syllable; then pres, abvs, blws, psts… and GPOS
 *      (marks are zero-width before GPOS). */
#include "shape_syl.h"

#include <stdlib.h>
#include <string.h>

#include "unicode.h"

static const char *const cats[] = { SEA_NAMES };

/* HarfBuzz's IV, DB and GB categories are V, N and PLACEHOLDER here. */
static SylGrammar grammar = {
    cats, (int)(sizeof cats / sizeof *cats),
    "j = ZWJ | ZWNJ;"
    "k = (Ra As H);"
    "sm = SM | SMPst;"
    "c = C | Ra;"
    "medial_group = MY? As? MR? ((MW MH? ML? | MH ML? | ML) As?)?;"
    "main_vowel_group = (VPre VS?)* VAbv* VBlw* A* (N As?)?;"
    "post_vowel_group = VPst MH? ML? As* VAbv* A* (N As?)?;"
    "tone_group = sm | PT A* N? As?;"
    "complex_syllable_tail = As* medial_group main_vowel_group post_vowel_group* tone_group* j?;"
    "syllable_tail = (H (c | V) VS?)* (H | complex_syllable_tail);"
    "consonant_syllable = (k | CS)? (c | V | PLACEHOLDER | DOTTEDCIRCLE) VS? syllable_tail;"
    "broken_cluster = k? VS? syllable_tail;",
    (const char *const[]){ "consonant_syllable", "j | SMPst", "broken_cluster", "any" }, 4, NULL,
};
enum { A_CONSONANT, A_JOINER, A_BROKEN, A_OTHER };     /* alternatives (= syllable type) */

/* Positions like HarfBuzz's ot_position_t. */
enum { P_START, P_RA_TO_BECOME_REPH, P_PRE_M, P_PRE_C, P_BASE_C, P_AFTER_MAIN, P_ABOVE_C, P_BEFORE_SUB, P_BELOW_C,
       P_AFTER_SUB, P_BEFORE_POST, P_POST_C, P_AFTER_POST, P_SMVD, P_END };

#define CAT(i) ((int)OTF_AUX_CAT(r->aux[i]))
#define POS(i) ((int)OTF_AUX_POS(r->aux[i]))
static void set_pos(GlyphRun *r, int i, int p) { r->aux[i] = (r->aux[i] & ~0xff00u) | (uint32_t)p << 8; }

static int is_consonant(const GlyphRun *r, int i) {
    if (r->aux[i] & OTF_AUX_LIGATED) return 0;
    int c = CAT(i);
    return c == SEA_C || c == SEA_CS || c == SEA_Ra || c == SEA_V || c == SEA_PLACEHOLDER || c == SEA_DOTTEDCIRCLE;
}

static void reverse(GlyphRun *r, int a, int b) {      /* [a, b) */
    for (b--; a < b; a++, b--) {
        uint16_t g = r->g[a]; r->g[a] = r->g[b]; r->g[b] = g;
        int cl = r->cl[a]; r->cl[a] = r->cl[b]; r->cl[b] = cl;
        uint32_t m = r->mask[a]; r->mask[a] = r->mask[b]; r->mask[b] = m;
        uint32_t x = r->aux[a]; r->aux[a] = r->aux[b]; r->aux[b] = x;
        uint8_t l = r->lig[a]; r->lig[a] = r->lig[b]; r->lig[b] = l;
    }
}

static void reorder(GlyphRun *r, int start, int end) {
    int base = end, has_reph = 0, limit = start;
    if (start + 3 <= end && CAT(start) == SEA_Ra && CAT(start + 1) == SEA_As && CAT(start + 2) == SEA_H) {
        limit += 3;
        base = start;
        has_reph = 1;
    }
    if (!has_reph) base = limit;
    for (int i = limit; i < end; i++) if (is_consonant(r, i)) { base = i; break; }

    int i = start;
    for (; i < start + (has_reph ? 3 : 0); i++) set_pos(r, i, P_AFTER_MAIN);
    for (; i < base; i++) set_pos(r, i, P_PRE_C);
    if (i < end) { set_pos(r, i, P_BASE_C); i++; }
    int pos = P_AFTER_MAIN;
    for (; i < end; i++) {
        int c = CAT(i);
        if (c == SEA_MR) { set_pos(r, i, P_PRE_C); continue; }          /* medial ra: before the base */
        if (c == SEA_VPre) { set_pos(r, i, P_PRE_M); continue; }        /* left-side vowel */
        if (c == SEA_VS) { set_pos(r, i, POS(i - 1)); continue; }
        if (pos == P_AFTER_MAIN && c == SEA_VBlw) { pos = P_BELOW_C; set_pos(r, i, pos); continue; }
        if (pos == P_BELOW_C && c == SEA_A) { set_pos(r, i, P_BEFORE_SUB); continue; }
        if (pos == P_BELOW_C && c == SEA_VBlw) { set_pos(r, i, pos); continue; }
        if (pos == P_BELOW_C && c != SEA_A) { pos = P_AFTER_SUB; set_pos(r, i, pos); continue; }
        set_pos(r, i, pos);
    }
    /* stable order by position */
    for (int a = start + 1; a < end; a++) {
        int b = a;
        while (b > start && POS(b - 1) > POS(a)) b--;
        syl_move(r, a, b);
    }
    /* consecutive left vowels are reversed (with their variation selectors) */
    int first = end, last = end;
    for (int k = start; k < end; k++) if (POS(k) == P_PRE_M) { if (first == end) first = k; last = k; }
    if (first < last) {
        reverse(r, first, last + 1);
        int k = first;
        for (int j = k; j <= last; j++) if (CAT(j) == SEA_VPre) { reverse(r, k, j + 1); k = j + 1; }
    }
}

int shape_myanmar(Shaper *s, Otf *o, const uint32_t *in, int n, int (*reserve)(Shaper *, int)) {
    GlyphRun *r = &s->run;
    r->len = 0;
    int m = syl_normalize(s, o, in, n, NULL, 1);
    if (m <= 0 || reserve(s, m * 2 + 8)) return 0;
    uint8_t *cat = malloc((size_t)m + 1);
    if (!cat) return 0;
    for (int i = 0; i < m; i++) cat[i] = (uint8_t)uc_sea_cat(s->cp[i]);
    int syl = 0;
    for (int i = 0; i < m;) {
        int alt, l = syl_next(&grammar, cat + i, m - i, &alt);
        if (alt == A_JOINER) alt = A_OTHER;
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
    const OtfPlan *p = otf_plan_kind(o, "mym2", OTF_KIND_MYANMAR);
    s->plan = p;
    otf_substitute_stages(o, p, r, 0, 0);
    syl_dotted_circles(s, o, A_BROKEN, SEA_DOTTEDCIRCLE, -1, reserve);
    for (int i = 0; i < r->len;) {
        int e = i + 1;
        while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) e++;
        if (SYL_TYPE(r->aux[i]) == A_CONSONANT || SYL_TYPE(r->aux[i]) == A_BROKEN) reorder(r, i, e);
        i = e;
    }
    otf_substitute_stages(o, p, r, 1, 255);
    return r->len;
}
