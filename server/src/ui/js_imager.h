/* js_imager.h — lord.imager (the LordOS Imager; see js_imager.c, docs/IMAGER.md). */
#ifndef LORD_JS_IMAGER_H
#define LORD_JS_IMAGER_H

#include "quickjs.h"

void js_imager_register(JSContext *c, JSValue lord);

#endif
