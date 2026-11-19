/* js_winmodel.h — lord.winmodel: the window manager's pure rules in Rust (js_winmodel.c). */
#ifndef AXIA_JS_WINMODEL_H
#define AXIA_JS_WINMODEL_H

#include "quickjs.h"
#include "winmodel_ffi.h"

void js_winmodel_register(JSContext *c, JSValue lord);
void js_windows_register(JSContext *c, JSValue lord);    /* one window list per context */
/* Valid only while the supplied QuickJS object is live; callers must not retain it. */
AxiaWindows *js_windows_borrow(JSContext *c, JSValueConst object);

#endif
