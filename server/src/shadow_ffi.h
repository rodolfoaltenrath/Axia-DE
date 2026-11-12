#ifndef AXIA_SHADOW_FFI_H
#define AXIA_SHADOW_FFI_H
/* Box shadows of the canvas (rust/axia-shell/src/canvas_shadow.rs). */
#include <stdint.h>
/* A cached coverage mask: mw×mh bytes whose (0, 0) is (x0, y0) from the box's corner; per row,
 * spans[4j..4j+4] = a0, a1, b0, b1: coverage only in [a0, a1) and [b0, b1). */
typedef struct { int32_t x0, y0, mw, mh; const uint8_t *a; const int32_t *spans; } AxiaShadowMask;
/* UI thread. NULL for an empty box or blur < 1. Valid until the next call. */
const AxiaShadowMask *axia_shadow_mask(int32_t w, int32_t h, int32_t r, int32_t blur, int32_t dx, int32_t dy);
/* Blends it with its corner at (x, y) into the rows of px inside [cx0, cx1) × [cy0, cy1); any thread
 * while the mask is valid (the canvas' row bands). */
void axia_shadow_blend(uint32_t *px, int32_t stride, int32_t cx0, int32_t cy0, int32_t cx1, int32_t cy1,
                       int32_t x, int32_t y, const AxiaShadowMask *m, uint32_t argb);
/* canvas_clear (rust/axia-shell/src/canvas_ops.rs): fills [x0, x1) × [y0, y1) with value. */
void axia_canvas_fill(uint32_t *px, int32_t stride, int32_t x0, int32_t y0, int32_t x1, int32_t y1, uint32_t value);
#endif
