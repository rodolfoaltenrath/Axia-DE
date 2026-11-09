/* js_wm.h — lord.wm: window dragging in C (see js_wm.c and grab.h). */
#ifndef LORD_JS_WM_H
#define LORD_JS_WM_H

#include "quickjs.h"
#include "grab.h"

/* Transport for the public file chooser portal; the picker remains Rust-owned. */
int js_portal_pick(unsigned id, const char *mode, const char *title, const char *folder,
                   const char *name, int multiple, const char *filters);
void js_portal_close(unsigned id);
void js_wm_register(JSContext *c, JSValue lord);
void js_wm_end(const GrabEnd *e);                  /* → __lordWm('end', …) */
/* → __lordWm('zone', { zone, x, y, w, h }) when the snapping zone under a moved window changed
 * (grab_zone_take); cheap when it didn't. main.c after pointer input, js.c after keys. */
void js_wm_zone_flush(void);

#endif
