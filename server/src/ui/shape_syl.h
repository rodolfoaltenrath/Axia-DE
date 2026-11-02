/* shape_syl.h — common pieces of the syllable-based shapers (Khmer, Myanmar, USE): syllable
 * grammars, HarfBuzz-style normalization and run assembly. See shape_syl.c. */
#ifndef LORD_SHAPE_SYL_H
#define LORD_SHAPE_SYL_H

#include <stdint.h>

#include "shape.h"

/* Syllable grammar: regular expressions over categories, like HarfBuzz's Ragel
 * machines. defs = "name = expression;" (may use the previous ones); alts = the
 * alternatives of the main machine, in order (the longest wins; on equal length, the
 * first). Syntax: category or def names, `any`, ( ) | * + ? and concatenation by
 * juxtaposition or with `.`. */
typedef struct SylNfa SylNfa;
typedef struct {
    const char *const *cats;    /* category names (index = value) */
    int ncats;
    const char *defs;
    const char *const *alts;
    int nalts;
    SylNfa *nfa;                /* compiled the first time */
} SylGrammar;

/* Length of the syllable starting at cat[0] (at least 1) and which alternative won (*alt). */
int syl_next(SylGrammar *g, const uint8_t *cat, int n, int *alt);

/* Combining class with HarfBuzz's changes (Hebrew, Arabic, Telugu, Thai, Tibetan). */
int syl_modified_ccc(uint32_t cp);

/* HarfBuzz normalization for these shapers ("composed diacritics, no short circuit"):
 * decomposes what the font can show separately, sorts marks by class and recomposes
 * a mark with its base if the font has the composite and compose() allows it.
 * split(cp, &a, &b) = the shaper's own decompositions (Khmer split matras).
 * Leaves the result in s->cp / s->order; returns the length or -1. */
int syl_normalize(Shaper *s, Otf *o, const uint32_t *in, int n,
                  int (*split)(uint32_t, uint32_t *, uint32_t *), int compose_after_mark);

/* aux: bits 28..31 = syllable type (the shapers in this group); bit 27 = a part that is not the
 * first of a multiple substitution. */
#define SYL_TYPE(a) ((int)((a) >> 28 & 0xf))

/* Inserts ◌ at the start of each broken syllable (after the repha-category ones if repha >= 0).
 * cat_dc = category of ◌. Returns 1 if it inserted any. */
int syl_dotted_circles(Shaper *s, Otf *o, int broken_type, int cat_dc, int repha, int (*reserve)(Shaper *, int));

/* At the end: each syllable is a single cluster (drawing neither separates nor breaks it). */
void syl_merge_clusters(GlyphRun *r);

/* Moves glyph from to position to, shifting the rest (g, cl, mask, lig, aux). */
void syl_move(GlyphRun *r, int from, int to);

#endif
