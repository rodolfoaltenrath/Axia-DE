/* shape_use.c — Universal Shaping Engine (the OpenType shaping model, as HarfBuzz implements it): the
 * shaping of the South and Southeast Asian scripts that have no shaper of their own (Tibetan,
 * Sinhala, Balinese, Javanese, Sundanese, Buginese, Tai Tham, Chakma, Brahmi, Limbu, Lepcha…) and of
 * some joining ones (Mongolian, N'Ko, Adlam):
 *
 *   1. normalization; USE categories (tools/gen-unicode.mjs); syllables with the
 *      USE grammar (without seeing CGJ or the ZWNJ that goes before a mark);
 *   2. masks: rphf at the start of each syllable; joining forms per syllable (or the
 *      Arabic ones in the joining scripts);
 *   3. locl, ccmp, nukt, akhn; rphf (a substituted reph becomes R); pref (the substituted one
 *      becomes VPre); rkrf, abvf, blwf, half, pstf, vatu, cjct;
 *   4. ◌ in broken syllables; reordering: the reph before the first post-base glyph and the
 *      pre-base vowels at the start (or after the last halant);
 *   5. isol/init/medi/fina; abvs, blws, haln, pres, psts… and GPOS (marks zero-width before). */
#include "shape_syl.h"

#include <stdlib.h>
#include <string.h>

#include "unicode.h"

static const char *const cats[] = { USE_NAMES };

static SylGrammar grammar = {
    cats, (int)(sizeof cats / sizeof *cats),
    "h = H | HVM | IS | Sk;"
    "consonant_modifiers = CMAbv* CMBlw* ((h B | SUB) CMAbv* CMBlw*)*;"
    "medial_consonants = MPre? MAbv? MBlw? MPst?;"
    "dependent_vowels = VPre* VAbv* VBlw* VPst* | H;"
    "vowel_modifiers = HVM? VMPre* VMAbv* VMBlw* VMPst*;"
    "final_consonants = FAbv* FBlw* FPst*;"
    "final_modifiers = FMAbv* FMBlw* | FMPst?;"
    "complex_syllable_start = (R | CS)? (B | GB);"
    "complex_syllable_middle = consonant_modifiers medial_consonants dependent_vowels vowel_modifiers (Sk B)*;"
    "complex_syllable_tail = complex_syllable_middle final_consonants final_modifiers;"
    "number_joiner_terminated_cluster_tail = (HN N)* HN;"
    "numeral_cluster_tail = (HN N)+;"
    "symbol_cluster_tail = SMAbv+ SMBlw* | SMBlw+;"
    "virama_terminated_cluster_tail = consonant_modifiers (IS | RK);"
    "virama_terminated_cluster = complex_syllable_start virama_terminated_cluster_tail;"
    "sakot_terminated_cluster_tail = complex_syllable_middle Sk;"
    "sakot_terminated_cluster = complex_syllable_start sakot_terminated_cluster_tail;"
    "standard_cluster = complex_syllable_start complex_syllable_tail;"
    "tail = complex_syllable_tail | sakot_terminated_cluster_tail | symbol_cluster_tail | virama_terminated_cluster_tail;"
    "broken_cluster = R? (tail | number_joiner_terminated_cluster_tail | numeral_cluster_tail);"
    "number_joiner_terminated_cluster = N number_joiner_terminated_cluster_tail;"
    "numeral_cluster = N numeral_cluster_tail?;"
    "symbol_cluster = (O | GB | SB) tail?;"
    "hieroglyph_cluster = SB* G HR? HM? SE* (J SB* (G HR? HM? SE*)?)*;",
    (const char *const[]){
        "virama_terminated_cluster ZWNJ?", "sakot_terminated_cluster ZWNJ?", "standard_cluster ZWNJ?",
        "number_joiner_terminated_cluster ZWNJ?", "numeral_cluster ZWNJ?", "symbol_cluster ZWNJ?",
        "hieroglyph_cluster ZWNJ?", "FMPst", "broken_cluster ZWNJ?", "any" }, 10, NULL,
};
/* Syllable types (like use_syllable_type_t) and which alternative each one comes from. */
enum { T_VIRAMA, T_SAKOT, T_STANDARD, T_NUMBER_JOINER, T_NUMERAL, T_SYMBOL, T_HIEROGLYPH, T_BROKEN, T_NON };
static const uint8_t alt_type[10] = { T_VIRAMA, T_SAKOT, T_STANDARD, T_NUMBER_JOINER, T_NUMERAL, T_SYMBOL, T_HIEROGLYPH, T_NON, T_BROKEN, T_NON };

/* Scripts that go through the USE (those in hb_ot_shaper_categorize with their own tag in the font). */
static const char use_tags[][5] = {
    "tibt", "mong", "sinh", "buhd", "hano", "tglg", "tagb", "limb", "tale", "bugi", "khar", "sylo", "tfng", "bali", "nko ",
    "phag", "cham", "kali", "lepc", "rjng", "saur", "sund", "egyp", "java", "kthi", "mtei", "lana", "tavt", "batk", "brah",
    "mand", "cakm", "plrd", "shrd", "takr", "dupl", "gran", "khoj", "sind", "mahj", "mani", "modi", "hmng", "phlp", "sidd",
    "tirh", "ahom", "mult", "adlm", "bhks", "marc", "newa", "gonm", "soyo", "zanb", "dogr", "gong", "rohg", "maka", "medf",
    "sogo", "sogd", "elym", "nand", "hmnp", "wcho", "chrs", "diak", "kits", "yezi", "cpmn", "ougr", "tnsa", "toto", "vith",
    "kawi", "nagm",
};

int shape_use_script(const char *tag) {
    for (size_t i = 0; i < sizeof use_tags / sizeof *use_tags; i++) if (!memcmp(use_tags[i], tag, 4)) return 1;
    return 0;
}

/* The ones that join like Arabic (HarfBuzz's has_arabic_joining). */
static int arabic_joining(const char *tag) {
    static const char j[][5] = { "mong", "nko ", "phag", "mand", "mani", "phlp", "adlm", "rohg", "sogd", "chrs", "ougr" };
    for (size_t i = 0; i < sizeof j / sizeof *j; i++) if (!memcmp(j[i], tag, 4)) return 1;
    return 0;
}

#define CAT(i) ((int)OTF_AUX_CAT(r->aux[i]))
static void set_cat(GlyphRun *r, int i, int c) { r->aux[i] = (r->aux[i] & ~0xffu) | (uint32_t)c; }

static int syl_end(const GlyphRun *r, int i) {
    int e = i + 1;
    while (e < r->len && OTF_AUX_SYL(r->aux[e]) == OTF_AUX_SYL(r->aux[i])) e++;
    return e;
}

static void clear_subst(GlyphRun *r) { for (int i = 0; i < r->len; i++) r->aux[i] &= ~(uint32_t)OTF_AUX_SUBST; }

static int is_halant(const GlyphRun *r, int i) {
    int c = CAT(i);
    return (c == USE_H || c == USE_HVM || c == USE_IS) && !(r->aux[i] & OTF_AUX_LIGATED);
}

static int post_base(int c) {
    switch (c) {
    case USE_FAbv: case USE_FBlw: case USE_FPst: case USE_FMAbv: case USE_FMBlw: case USE_FMPst: case USE_MAbv:
    case USE_MBlw: case USE_MPst: case USE_MPre: case USE_VAbv: case USE_VBlw: case USE_VPst: case USE_VPre:
    case USE_VMAbv: case USE_VMBlw: case USE_VMPst: case USE_VMPre:
        return 1;
    default:
        return 0;
    }
}

static void reorder(GlyphRun *r, int start, int end) {
    int t = SYL_TYPE(r->aux[start]);
    if (!(t == T_VIRAMA || t == T_SAKOT || t == T_STANDARD || t == T_SYMBOL || t == T_BROKEN)) return;
    /* the reph goes before the first post-base glyph (or at the end) */
    if (CAT(start) == USE_R && end - start > 1)
        for (int i = start + 1; i < end; i++) {
            int pb = post_base(CAT(i)) || is_halant(r, i);
            if (pb || i == end - 1) {
                if (pb) i--;
                syl_move(r, start, i);
                break;
            }
        }
    /* the pre-base vowels, at the start or after the last halant */
    for (int i = start, j = start; i < end; i++) {
        int c = CAT(i);
        if (is_halant(r, i)) j = i + 1;
        else if ((c == USE_VPre || c == USE_VMPre) && !(r->aux[i] & OTF_AUX_NOTFIRST) && j < i) syl_move(r, i, j);
    }
}

int shape_use(Shaper *s, Otf *o, const uint32_t *in, int n, const char *tag, int (*reserve)(Shaper *, int)) {
    GlyphRun *r = &s->run;
    r->len = 0;
    int m = syl_normalize(s, o, in, n, NULL, 0);
    if (m <= 0 || reserve(s, m * 2 + 8)) return 0;
    uint8_t *cat = malloc((size_t)m * 2 + 2);
    int *idx = malloc(sizeof *idx * (size_t)(m + 1));
    if (!cat || !idx) { free(cat); free(idx); return 0; }
    uint8_t *fc = cat + m + 1;
    for (int i = 0; i < m; i++) cat[i] = (uint8_t)uc_use_cat(s->cp[i]);
    /* the grammar doesn't see CGJ (nor ZWJ nor selectors) nor the ZWNJ before a mark */
    int nf = 0;
    for (int i = 0; i < m; i++) {
        if (cat[i] == USE_CGJ) continue;
        if (cat[i] == USE_ZWNJ) {
            int j = i + 1;
            while (j < m && cat[j] == USE_CGJ) j++;
            if (j < m && uc_mark(s->cp[j])) continue;
        }
        idx[nf] = i;
        fc[nf++] = cat[i];
    }
    idx[nf] = m;
    for (int i = 0; i < m; i++) {
        r->g[i] = otf_cmap(o, s->cp[i]);
        r->cl[i] = s->order[i];
        r->mask[i] = OTF_F_GLOBAL;
        r->lig[i] = 0;
        r->aux[i] = cat[i] | (uint32_t)T_NON << 28 | 1u << 16;
    }
    int syl = 1;
    for (int k = 0; k < nf;) {
        int alt, l = syl_next(&grammar, fc + k, nf - k, &alt);
        syl = syl % 255 + 1;
        for (int i = idx[k]; i < idx[k + l]; i++)
            r->aux[i] = cat[i] | (uint32_t)syl << 16 | (uint32_t)alt_type[alt] << 28;
        k += l;
    }
    r->len = m;
    free(idx);

    const OtfPlan *p = otf_plan_kind(o, tag, OTF_KIND_USE);
    s->plan = p;
    /* rphf: the first glyph if it is R; otherwise, up to the first three */
    for (int i = 0; i < r->len;) {
        int e = syl_end(r, i), lim = CAT(i) == USE_R ? 1 : (e - i < 3 ? e - i : 3);
        for (int j = i; j < i + lim; j++) r->mask[j] |= OTF_F_RPHF;
        i = e;
    }
    /* joining forms */
    if (arabic_joining(tag)) shape_joining_masks(s->cp, m, r->mask);
    else {
        int last_start = 0, last_form = -1;     /* 0 isol, 1 init, 2 medi, 3 fina */
        static const uint32_t forms[4] = { OTF_F_ISOL, OTF_F_INIT, OTF_F_MEDI, OTF_F_FINA };
        const uint32_t all = OTF_F_ISOL | OTF_F_INIT | OTF_F_MEDI | OTF_F_FINA;
        for (int i = 0; i < r->len;) {
            int e = syl_end(r, i), t = SYL_TYPE(r->aux[i]);
            if (t == T_HIEROGLYPH || t == T_NON) last_form = -1;
            else {
                int join = last_form == 3 || last_form == 0;
                if (join) {
                    last_form = last_form == 3 ? 2 : 1;
                    for (int j = last_start; j < i; j++) r->mask[j] = (r->mask[j] & ~all) | forms[last_form];
                }
                last_form = join ? 3 : 0;
                for (int j = i; j < e; j++) r->mask[j] = (r->mask[j] & ~all) | forms[last_form];
            }
            last_start = i;
            i = e;
        }
    }
    free(cat);

    otf_substitute_stages(o, p, r, 0, 0);
    clear_subst(r);
    otf_substitute_stages(o, p, r, 1, 1);
    for (int i = 0; i < r->len;) {               /* a substituted reph is R */
        int e = syl_end(r, i);
        for (int j = i; j < e && (r->mask[j] & OTF_F_RPHF); j++)
            if (r->aux[j] & OTF_AUX_SUBST) { set_cat(r, j, USE_R); break; }
        i = e;
    }
    clear_subst(r);
    otf_substitute_stages(o, p, r, 2, 2);
    for (int i = 0; i < r->len;) {               /* a substituted pref moves like VPre */
        int e = syl_end(r, i);
        for (int j = i; j < e; j++) if (r->aux[j] & OTF_AUX_SUBST) { set_cat(r, j, USE_VPre); break; }
        i = e;
    }
    otf_substitute_stages(o, p, r, 3, 3);
    syl_dotted_circles(s, o, T_BROKEN, USE_B, USE_R, reserve);
    for (int i = 0; i < r->len;) {
        int e = syl_end(r, i);
        reorder(r, i, e);
        i = e;
    }
    otf_substitute_stages(o, p, r, 4, 255);
    return r->len;
}
