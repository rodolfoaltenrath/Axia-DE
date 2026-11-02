/* shape.c — shaping of a run (see shape.h). Equivalent to what HarfBuzz does for the
 * scripts that are neither Indic nor Southeast Asian: Latin, Greek, Cyrillic, Arabic,
 * Hebrew, CJK. Compared against HarfBuzz in server/tests/shape_test.c. */
#include "shape.h"

#include <stdlib.h>
#include <string.h>

#include "shape_syl.h"
#include "unicode.h"

static int reserve(Shaper *s, int n) {
    GlyphRun *r = &s->run;
    if (n > r->cap) {
        int cap = n + 32;
        uint16_t *g = realloc(r->g, sizeof *g * (size_t)cap);
        if (!g) return -1;
        r->g = g;
        int *cl = realloc(r->cl, sizeof *cl * (size_t)cap);
        if (!cl) return -1;
        r->cl = cl;
        uint32_t *m = realloc(r->mask, sizeof *m * (size_t)cap);
        if (!m) return -1;
        r->mask = m;
        uint8_t *l = realloc(r->lig, (size_t)cap);
        if (!l) return -1;
        r->lig = l;
        uint32_t *a = realloc(r->aux, sizeof *a * (size_t)cap);
        if (!a) return -1;
        r->aux = a;
        r->cap = cap;
    }
    return 0;
}

/* Joining type for shaping: C (tatweel, ZWJ) behaves like D, same as in HarfBuzz. */
static int join_type(uint32_t cp) {
    int t = uc_join(cp);
    return t == JOIN_C ? JOIN_D : t;
}
static int joins_next(int t) { return t == JOIN_D || t == JOIN_L; }
static int joins_prev(int t) { return t == JOIN_D || t == JOIN_R; }

/* isol/init/medi/fina forms according to the non-transparent neighbors (marks). */
void shape_joining_masks(const uint32_t *cp, int n, uint32_t *mask) {
    int prev = -1, prev_t = JOIN_U;
    for (int i = 0; i < n; i++) {
        int t = join_type(cp[i]);
        if (t == JOIN_T) continue;
        int next_t = JOIN_U;
        for (int j = i + 1; j < n; j++) { int u = join_type(cp[j]); if (u != JOIN_T) { next_t = u; break; } }
        int jp = prev >= 0 && joins_prev(t) && joins_next(prev_t);
        int jn = joins_next(t) && joins_prev(next_t);
        if (t == JOIN_D || t == JOIN_R || t == JOIN_L)
            mask[i] |= jp && jn ? OTF_F_MEDI : jp ? OTF_F_FINA : jn ? OTF_F_INIT : OTF_F_ISOL;
        prev = i;
        prev_t = t;
    }
}

/* Combining class with HarfBuzz's changes: in Hebrew, the SBL handbook order
 * (shin/sin dots and dagesh before the vowels); in Arabic, shadda before the rest. */
static int modified_ccc(uint32_t cp) {
    static const uint8_t hebrew[17] = { 22, 15, 16, 17, 23, 18, 19, 20, 21, 14, 24, 12, 25, 13, 10, 11, 26 };   /* 10..26 */
    static const uint8_t arabic[10] = { 28, 29, 30, 31, 32, 33, 27, 34, 35, 36 };                             /* 27..36 */
    int c = uc_ccc(cp);
    if (c >= 10 && c <= 26) return hebrew[c - 10];
    if (c >= 27 && c <= 36) return arabic[c - 27];
    return c;
}

/* Canonical order of consecutive marks (stable, by class): this way fonts receive the
 * sequences as HarfBuzz expects them. order[i] = original index. */
static void reorder_marks(uint32_t *cp, int *order, int n) {
    for (int i = 0; i < n; i++) {
        if (!uc_ccc(cp[i])) continue;
        int e = i;
        while (e < n && uc_ccc(cp[e])) e++;
        for (int a = i + 1; a < e; a++) {
            uint32_t c = cp[a];
            int o = order[a], k = modified_ccc(c), b = a - 1;
            while (b >= i && modified_ccc(cp[b]) > k) { cp[b + 1] = cp[b]; order[b + 1] = order[b]; b--; }
            cp[b + 1] = c;
            order[b + 1] = o;
        }
        i = e - 1;
    }
}

/* Thai and Lao: SARA AM (ำ) = NIKHAHIT + SARA AA, and NIKHAHIT goes before the tones that
 * precede it (like HarfBuzz). Works on s->cp/s->order; returns the new length. */
static int above_mark(uint32_t c) {
    return c == 0x0E31 || (c >= 0x0E34 && c <= 0x0E37) || (c >= 0x0E47 && c <= 0x0E4E) ||
           c == 0x0EB1 || (c >= 0x0EB4 && c <= 0x0EB7) || (c >= 0x0EBB && c <= 0x0EBC) || (c >= 0x0EC8 && c <= 0x0ECD);
}
static int thai_sara_am(Shaper *s, int n) {
    int extra = 0;
    for (int i = 0; i < n; i++) if (s->cp[i] == 0x0E33 || s->cp[i] == 0x0EB3) extra++;
    if (!extra || n + extra > s->cp_cap) return n;
    for (int i = n - 1, w = n + extra - 1; i >= 0; i--) {
        uint32_t c = s->cp[i];
        int o = s->order[i];
        if (c == 0x0E33 || c == 0x0EB3) {
            s->cp[w] = c == 0x0E33 ? 0x0E32 : 0x0EB2; s->order[w--] = o;
            s->cp[w] = c == 0x0E33 ? 0x0E4D : 0x0ECD; s->order[w--] = o;
        } else { s->cp[w] = c; s->order[w--] = o; }
    }
    n += extra;
    for (int i = 1; i < n; i++) {
        if (!(s->cp[i] == 0x0E4D || s->cp[i] == 0x0ECD) || i + 1 >= n || !(s->cp[i + 1] == 0x0E32 || s->cp[i + 1] == 0x0EB2)) continue;
        int st = i;
        while (st > 0 && above_mark(s->cp[st - 1]) && s->cp[st - 1] != 0x0E4D && s->cp[st - 1] != 0x0ECD) st--;
        if (st < i) {
            uint32_t c = s->cp[i];
            int o = s->order[i];
            memmove(s->cp + st + 1, s->cp + st, sizeof *s->cp * (size_t)(i - st));
            memmove(s->order + st + 1, s->order + st, sizeof *s->order * (size_t)(i - st));
            s->cp[st] = c;
            s->order[st] = o;
        }
    }
    return n;
}

int shape_run(Shaper *s, Otf *o, const uint32_t *in, int n, const char *script, int rtl) {
    GlyphRun *r = &s->run;
    r->len = 0;
    if (n <= 0 || reserve(s, n * 2 + 8)) return 0;
    if (n * 2 + 32 > s->cp_cap) {
        uint32_t *nc = realloc(s->cp, sizeof *nc * (size_t)(n * 2 + 32));
        if (!nc) return 0;
        s->cp = nc;
        int *no = realloc(s->order, sizeof *no * (size_t)(n * 2 + 32));
        if (!no) return 0;
        s->order = no;
        s->cp_cap = n * 2 + 32;
    }
    int is_indic = -1, syllabic = 0;
    if (!memcmp(script, "deva", 4) || !memcmp(script, "beng", 4) || !memcmp(script, "guru", 4) || !memcmp(script, "gujr", 4) ||
        !memcmp(script, "orya", 4) || !memcmp(script, "taml", 4) || !memcmp(script, "telu", 4) || !memcmp(script, "knda", 4) ||
        !memcmp(script, "mlym", 4))
        for (int i = 0; i < n && is_indic < 0; i++) is_indic = shape_indic_script(in[i]);
    if (is_indic >= 0) {
        if (!shape_indic(s, o, in, n, is_indic, reserve)) return 0;
        memset(r->lig, 0, (size_t)r->len);
        goto position;
    }
    /* Khmer, Myanmar (mym2 fonts) and the Universal Shaping Engine ones (if the font has them) */
    if (!memcmp(script, "khmr", 4)) syllabic = shape_khmer(s, o, in, n, reserve) ? 1 : -1;
    else if (!memcmp(script, "mymr", 4) && otf_has_gsub_script(o, "mym2")) syllabic = shape_myanmar(s, o, in, n, reserve) ? 1 : -1;
    else if (shape_use_script(script) && otf_has_gsub_script(o, script)) syllabic = shape_use(s, o, in, n, script, reserve) ? 1 : -1;
    if (syllabic < 0) return 0;
    if (syllabic) goto position;
    uint32_t *cp = s->cp;
    for (int i = 0; i < n; i++) { cp[i] = in[i]; s->order[i] = i; }
    reorder_marks(cp, s->order, n);
    if (!memcmp(script, "thai", 4) || !memcmp(script, "lao ", 4)) n = thai_sara_am(s, n);
    cp = s->cp;
    const OtfPlan *p = otf_plan(o, script);
    s->plan = p;
    for (int i = 0; i < n; i++) {
        uint32_t c = cp[i];
        if (rtl) {                  /* "(" is drawn as ")" in a right-to-left run */
            uint32_t m = uc_mirror(c);
            if (m && otf_cmap(o, m)) c = m;
        }
        r->g[i] = otf_cmap(o, c);
        r->cl[i] = s->order[i];
        r->mask[i] = OTF_F_GLOBAL;
        r->lig[i] = 0;
    }
    r->len = n;
    if (p->joining) shape_joining_masks(cp, n, r->mask);
    if (r->aux) memset(r->aux, 0, sizeof *r->aux * (size_t)n);
    otf_substitute(o, p, r);
position:;
    p = s->plan;
    if (r->len > s->pos_cap) {
        OtfPos *np = realloc(s->pos, sizeof *np * (size_t)(r->len + 32));
        if (!np) return 0;
        s->pos = np;
        s->pos_cap = r->len + 32;
    }
    /* invisibles that GPOS skips (not in Indic: there the cluster is the whole syllable) */
    uint8_t *ign = is_indic < 0 ? malloc((size_t)r->len + 1) : NULL;
    int any_ign = 0;
    if (ign) for (int i = 0; i < r->len; i++) any_ign |= ign[i] = uc_ignorable(in[r->cl[i]]) && !(r->aux[i] & OTF_AUX_SUBST);
    otf_position_ign(o, p, r, s->pos, rtl, any_ign ? ign : NULL);
    free(ign);
    /* invisibles (ZWJ, ZWNJ, direction marks, variation selectors): the glyph of the
     * zero-width space, like HarfBuzz */
    unsigned space = otf_cmap(o, ' ');
    for (int i = 0; i < r->len && syllabic; i++) {
        if (!uc_ignorable(in[r->cl[i]]) || (r->aux[i] & OTF_AUX_SUBST)) continue;
        r->g[i] = (uint16_t)space;
        s->pos[i].adv = s->pos[i].dx = s->pos[i].dy = 0;
        s->pos[i].att = -1;
    }
    if (syllabic) { syl_merge_clusters(r); s->gpos_kern = p->gpos_kern; return r->len; }
    for (int i = 0; i < r->len; i++) {
        if (is_indic >= 0) {                /* Indic: the syllable is a single cluster; look at the category */
            int c = (int)OTF_AUX_CAT(r->aux[i]);
            if (!((c == IC_ZWJ || c == IC_ZWNJ) && !(r->aux[i] & OTF_AUX_LIGATED))) continue;
        } else {
            if (!uc_ignorable(in[r->cl[i]])) continue;
            if (i + 1 < r->len && r->cl[i + 1] == r->cl[i]) continue;      /* part of something else */
            if (i > 0 && r->cl[i - 1] == r->cl[i]) continue;
        }
        r->g[i] = (uint16_t)space;
        s->pos[i].adv = s->pos[i].dx = s->pos[i].dy = 0;
        s->pos[i].att = -1;
    }
    s->gpos_kern = p->gpos_kern;
    return r->len;
}

void shape_free(Shaper *s) {
    free(s->run.g); free(s->run.cl); free(s->run.mask); free(s->run.lig); free(s->pos);
    free(s->cp); free(s->order); free(s->run.aux);
    memset(s, 0, sizeof *s);
}
