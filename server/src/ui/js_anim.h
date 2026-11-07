/* js_anim.h — lord.anim: native animations computed in Rust (rust/axia-shell/src/anim.rs). */
#ifndef LORD_JS_ANIM_H
#define LORD_JS_ANIM_H
#include "quickjs.h"
void js_anim_register(JSContext *c, JSValue lord);
int  js_anim_active(void);          /* an animation runs: the main loop wants frames */
double js_anim_timeout(double now, double next_frame, int software_paced, int waiting_for_flip, unsigned refresh_hz, int js_frames);
void js_anim_step(double now);      /* each frame, before layout: writes this frame's values */
#endif
