/* otf.h — the OpenType parts stb_truetype lacks, without HarfBuzz:
 *   - GSUB substitutions: ligatures of programming fonts (JetBrains Mono, Fira Code),
 *     emoji sequences and, per script, Arabic contextual forms;
 *   - GPOS positioning: pair kerning, cursive (Nastaliq, Kannada) and marks (Arabic
 *     harakat, Hebrew niqqud, combining accents) anchored to the base, to ligatures or to other marks;
 *   - CBDT/CBLC color glyphs (PNG inside the font, like Noto Color Emoji).
 *
 * All reads validate bounds: fonts may come from ~/.fonts. */
#ifndef LORD_OTF_H
#define LORD_OTF_H

#include <stddef.h>
#include <stdint.h>

/* Per-glyph feature masks: Arabic forms go only where they apply; the rest
 * of the features (ccmp, locl, rlig, calt, liga, kern, mark…) is global. */
enum { OTF_F_ISOL = 1 << 0, OTF_F_FINA = 1 << 1, OTF_F_MEDI = 1 << 2, OTF_F_INIT = 1 << 3, OTF_F_GLOBAL = 1 << 4,
       /* Indic scripts: forms requested glyph by glyph according to their place in the syllable */
       OTF_F_RPHF = 1 << 5, OTF_F_PREF = 1 << 6, OTF_F_BLWF = 1 << 7, OTF_F_ABVF = 1 << 8, OTF_F_HALF = 1 << 9,
       OTF_F_PSTF = 1 << 10, OTF_F_CFAR = 1 << 11 };

/* GlyphRun.aux: Indic shaping properties that follow each glyph through substitutions */
#define OTF_AUX_CAT(a)   ((a) & 0xff)
#define OTF_AUX_POS(a)   ((a) >> 8 & 0xff)
#define OTF_AUX_SYL(a)   ((a) >> 16 & 0xff)
enum { OTF_AUX_SUBST = 1u << 24, OTF_AUX_LIGATED = 1u << 25, OTF_AUX_MULTIPLIED = 1u << 26,
       OTF_AUX_NOTFIRST = 1u << 27 };     /* part 2, 3… of a multiple substitution */

/* Stages of the Indic plan (like HarfBuzz): reordering goes between them. */
enum { OTF_ST_PRE = 0, OTF_ST_NUKT = 1, OTF_ST_RPHF = 3, OTF_ST_PREF = 5, OTF_ST_BLWF = 6, OTF_ST_PSTF = 9,
       OTF_ST_VATU = 10, OTF_ST_CJCT = 11, OTF_ST_FINAL = 12 };

typedef struct {
    uint16_t lookup;
    uint8_t stage;              /* the Arabic and Indic ones go by stages, as in HarfBuzz */
    uint8_t per_syllable;       /* Indic: does not cross syllables */
    uint32_t mask;
} OtfLookupRef;

/* Shaper type of the plan (each one requests its features by stages, as in HarfBuzz). */
enum { OTF_KIND_DEFAULT, OTF_KIND_INDIC, OTF_KIND_KHMER, OTF_KIND_MYANMAR, OTF_KIND_USE };
/* Mark widths (GDEF): zeroed after GPOS, never, or before GPOS. */
enum { OTF_ZERO_LATE, OTF_ZERO_NONE, OTF_ZERO_EARLY };

/* Lookups of the interface features for a script (OpenType tag). */
typedef struct {
    char script[4];
    int joining;                /* Arabic, Syriac, N'Ko, Mongolian…: uses isol/fina/medi/init */
    int indic;                  /* Devanagari … Malayalam (tags dev2, bng2…) */
    int kind;                   /* OTF_KIND_* */
    int zero_marks;             /* OTF_ZERO_* */
    OtfLookupRef *gsub, *gpos;
    int ngsub, ngpos;
    uint8_t *first;             /* bitset of the first glyph per GSUB lookup */
    int gpos_kern;              /* GPOS has kerning (otherwise the kern table is used) */
} OtfPlan;

#define OTF_MAX_PLANS 6

typedef struct {
    const uint8_t *d;
    size_t n;                   /* file size */
    uint32_t gsub, gsub_len;    /* absolute offsets (0 = absent) */
    uint32_t gpos, gpos_len;
    uint32_t cblc, cblc_len, cbdt, cbdt_len;
    uint32_t cmap;              /* chosen cmap subtable (format 4 or 12) */
    uint32_t hmtx;
    unsigned nhmetrics;
    uint32_t glyph_class, mark_class, mark_sets;    /* GDEF */
    uint16_t *lookups;          /* lookup indices per feature group, sorted */
    int nlookups[2];
    uint8_t *first[2];          /* bitset: glyphs that can start some substitution */
    int nglyphs;
    OtfPlan plans[OTF_MAX_PLANS];
    int nplans;
} Otf;

enum { OTF_LIGA = 0, OTF_CCMP = 1 };   /* terminal groups: calt+liga+clig / ccmp+rlig */

/* Glyph buffer: each glyph remembers which input character it came from (cluster).
 * mask and lig are optional (NULL): per-script shaping uses them. */
typedef struct {
    uint16_t *g;
    int *cl;
    int len, cap;
    uint32_t *mask;             /* OTF_F_* per glyph */
    uint8_t *lig;               /* ligature: number of components; mark: component + 1 */
    uint32_t *aux;              /* Indic: category, position, syllable and flags (OTF_AUX_*) */
} GlyphRun;

/* GPOS result, in font units. An anchored mark (att >= 0) is drawn at the
 * origin of glyph att plus (dx, dy); the others at the pen plus (dx, dy). */
typedef struct { int32_t adv, dx, dy; int att, cur; } OtfPos;    /* cur: cursive parent (GPOS 3), -1 */

int  otf_init(Otf *o, const uint8_t *data, size_t size, int font_offset);
void otf_free(Otf *o);
int  otf_has_gsub(const Otf *o, int group);
/* Character → glyph (for fonts stb_truetype doesn't open, like bitmap-only ones). */
uint16_t otf_cmap(const Otf *o, uint32_t cp);
/* Terminal: applies the group's features on the run (in place). */
void otf_shape(const Otf *o, int group, GlyphRun *run);

/* Interface: plan for the script `script` ("arab", "hebr", "latn", "hani"…). */
const OtfPlan *otf_plan(Otf *o, const char *script);
const OtfPlan *otf_plan_kind(Otf *o, const char *script, int kind);
void otf_substitute(const Otf *o, const OtfPlan *p, GlyphRun *run);
/* Only stages [from, to] (Indic reorders between stages). */
void otf_substitute_stages(const Otf *o, const OtfPlan *p, GlyphRun *run, int from, int to);
/* Does some lookup of that stage substitute exactly this sequence? (no context) */
int  otf_would_substitute(const Otf *o, const OtfPlan *p, int stage, const uint16_t *g, int n);
int  otf_has_script(const Otf *o, const char *tag);
int  otf_has_gsub_script(const Otf *o, const char *tag);       /* only in GSUB (like HarfBuzz picks the shaper) */
/* pos has run->len slots. */
void otf_position(const Otf *o, const OtfPlan *p, const GlyphRun *run, OtfPos *pos, int rtl);
/* Same, but glyphs with ign[i] (invisibles like ZWJ) don't break a kerning pair or a
 * GPOS sequence (HarfBuzz skips them). */
void otf_position_ign(const Otf *o, const OtfPlan *p, const GlyphRun *run, OtfPos *pos, int rtl, const uint8_t *ign);
int  otf_is_mark(const Otf *o, unsigned glyph);

/* Color glyph: PNG of the strike closest to `ppem`. Returns a pointer to the PNG inside the
 * font and its size, or NULL. `strike_ppem` = native size of the bitmap. */
const uint8_t *otf_color_png(const Otf *o, uint16_t glyph, int ppem, size_t *len, int *strike_ppem);

#endif
