#ifndef AXIA_ANIM_FFI_H
#define AXIA_ANIM_FFI_H
#include <stddef.h>
#include <stdint.h>
/* Native animations (rust/axia-shell/src/anim.rs). UI thread only. Origins are pixels from the
 * node's border box; origin_set 0 = the default origin (the centre). */
typedef struct { float scale_x, scale_y, opacity, origin_x, origin_y, translate_x, translate_y; uint8_t origin_set; } AnimProps;
typedef struct { int32_t node; uint32_t id; AnimProps props; uint8_t done; } AnimFrame;
uint32_t axia_anim_start(int32_t node, double now, double ms, uint8_t easing, const AnimProps *from, const AnimProps *to, const AnimProps *base);
uint32_t axia_anim_minimize(int32_t node, double now, float x, float y, float w, float h, float dock_x, float dock_y, const AnimProps *base);
uint32_t axia_anim_slide(int32_t node, double now, int32_t step, _Bool rtl, const AnimProps *base);
uint32_t axia_anim_dock_visibility(int32_t node, double now, uint8_t edge, float distance, _Bool shown, _Bool immediate, const AnimProps *current, AnimProps *initial);
uint32_t axia_anim_dock_poof(int32_t node, double now, float size, uint8_t part, const AnimProps *base, AnimProps *initial);
size_t   axia_anim_step(double now, AnimFrame *out, size_t max);
int      axia_anim_clear(int32_t node, AnimProps *out);
void     axia_anim_forget(int32_t node);
int      axia_anim_active(void);
double   axia_anim_timeout(double now, double next_frame, _Bool software_paced, _Bool waiting_for_flip, uint32_t refresh_hz, _Bool js_frames);
#endif
