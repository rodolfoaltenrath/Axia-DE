/* js_snap.h — lord.snap: window snapping's geometry in Rust (js_snap.c, snap_ffi.h). */
#ifndef AXIA_JS_SNAP_H
#define AXIA_JS_SNAP_H

#include "quickjs.h"

void js_snap_register(JSContext *c, JSValue lord);

#endif
