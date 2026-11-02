/* js_sound.h — lord.sound (see js_sound.c and docs/SOUND.md). */
#ifndef LORD_JS_SOUND_H
#define LORD_JS_SOUND_H

#include "quickjs.h"

void js_sound_register(JSContext *c, JSValue lord);

#endif
