/* js_nativeview.h — native views: compiled apps that Rust mounts straight into the scene
 * (rust/axia-shell/src/native_view). C converts arguments; Rust owns the views. */
#ifndef LORD_JS_NATIVEVIEW_H
#define LORD_JS_NATIVEVIEW_H
#include "quickjs.h"

/* Rust (native_view/mod.rs). UI thread only. */
void     axia_view_init(const char *assets, const char *home);
unsigned axia_view_mount(int node, const char *app, const char *facts);
void     axia_view_unmount(unsigned handle);
void     axia_view_facts(unsigned handle, const char *facts);
void     axia_view_focus(unsigned handle, int focused);
char    *axia_view_take(unsigned handle);
void     axia_view_free(char *p);
void     axia_view_result(unsigned handle, const char *result);
void     axia_view_paste(unsigned handle, const char *text);
char    *axia_view_text(unsigned handle);
int      axia_view_find_text(unsigned handle, const char *label);
/* A pointer event or a touchpad gesture: the view under it handles it; JS's target is returned
 * (the view's mount node, so the window and the shell still see the event). */
int      axia_view_pointer(const char *type, int target, float x, float y, int button);
int      axia_view_gesture(const char *kind, const char *phase, int target, double dx, double dy);
int      axia_view_key(const char *key, const char *chr, int flags);
void     axia_view_tick(double now);
double   axia_view_timeout(double now);
int      axia_view_pending(void);

/* lord.nativeViews */
void js_nativeview_register(JSContext *ctx, JSValue lord, const char *assets_dir);
#endif
