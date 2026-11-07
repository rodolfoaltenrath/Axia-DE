/* js_anim.h — lord.anim: native animations computed in Rust (rust/axia-shell/src/anim.rs). */
#ifndef LORD_JS_ANIM_H
#define LORD_JS_ANIM_H
#include "quickjs.h"
void js_anim_register(JSContext *c, JSValue lord);
int  js_anim_active(void);          /* an animation runs: the main loop wants frames */
void js_anim_step(double now);      /* each frame, before layout: writes this frame's values */
#endif
