/* shape.h — shaping of a text run with a single font, script and direction: mirroring
 * in RTL, Arabic joining forms, GSUB and GPOS (otf.c). See shape.c. */
#ifndef LORD_SHAPE_H
#define LORD_SHAPE_H

#include <stdint.h>

#include "otf.h"

typedef struct {
    GlyphRun run;               /* glyphs in logical order; run.cl = character index */
    OtfPos *pos;                /* font units */
    int pos_cap;
    int gpos_kern;              /* 0: the caller applies the kern table */
    uint32_t *cp;               /* copy with marks in canonical order */
    int *order, cp_cap;
    const OtfPlan *plan;        /* the one that was used (Indic: dev2, bng2…) */
} Shaper;

/* script: OpenType tag ("arab", "hebr", "latn"…). Returns the number of glyphs. */
int  shape_run(Shaper *s, Otf *o, const uint32_t *cp, int n, const char *script, int rtl);
void shape_free(Shaper *s);

/* shape_indic.c */
int  shape_indic_script(uint32_t cp);      /* Indic script index, or -1 */
int  shape_indic(Shaper *s, Otf *o, const uint32_t *in, int n, int script, int (*reserve)(Shaper *, int));

/* shape_khmer.c, shape_myanmar.c, shape_use.c (see shape_syl.h) */
int  shape_khmer(Shaper *s, Otf *o, const uint32_t *in, int n, int (*reserve)(Shaper *, int));
int  shape_myanmar(Shaper *s, Otf *o, const uint32_t *in, int n, int (*reserve)(Shaper *, int));
int  shape_use_script(const char *tag);    /* does the script go through the Universal Shaping Engine? */
int  shape_use(Shaper *s, Otf *o, const uint32_t *in, int n, const char *tag, int (*reserve)(Shaper *, int));
/* shape.c: isol/init/medi/fina forms of Arabic and the joining scripts */
void shape_joining_masks(const uint32_t *cp, int n, uint32_t *mask);

#endif
