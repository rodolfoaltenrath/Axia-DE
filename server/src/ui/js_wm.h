/* js_wm.h — lord.wm: window dragging in C (see js_wm.c and grab.h). */
#ifndef LORD_JS_WM_H
#define LORD_JS_WM_H

#include "quickjs.h"

/* Transport for the public file chooser portal; the picker remains Rust-owned. */
int js_portal_pick(unsigned id, const char *mode, const char *title, const char *folder,
                   const char *name, int multiple, const char *filters);
void js_portal_close(unsigned id);
void js_wm_register(JSContext *c, JSValue lord);
void js_wm_end(int node, int edges, float dx, float dy, float dw, float dh);   /* → __lordWm('end', …) */

#endif
