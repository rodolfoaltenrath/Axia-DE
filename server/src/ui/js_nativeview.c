/* lord.nativeViews: the shell's side of native views (see js_nativeview.h). The shell gives a
 * view its mount node, its facts (settings, theme, services) and runs the effects that belong
 * to the shell; drawing, input and captions are Rust's. */
#include "js_nativeview.h"
#include "js.h"
#include "js_winmodel.h"
#include "ui.h"
#include "../tp/tp_route.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned arg_handle(JSContext *c, JSValueConst v) {
    uint32_t h = 0;
    return JS_ToUint32(c, &h, v) ? 0 : h;
}

/* JSON text of a value (a string is passed as is). */
static const char *arg_json(JSContext *c, JSValueConst v, JSValue *keep) {
    *keep = JS_IsString(v) ? JS_DupValue(c, v) : JS_JSONStringify(c, v, JS_UNDEFINED, JS_UNDEFINED);
    if (JS_IsException(*keep) || JS_IsUndefined(*keep)) { *keep = JS_UNDEFINED; return NULL; }
    return JS_ToCString(c, *keep);
}

static JSValue owned_string(JSContext *c, char *s) {
    if (!s) return JS_NULL;
    JSValue r = JS_NewString(c, s);
    axia_view_free(s);
    return r;
}

/* In an isolated app's process (axia-server --app <id>): the one view it may mount, its own. */
static char only_app[64];

/* mount(nodeId, appId, facts) → handle, or 0 when the app has no native view */
static JSValue f_mount(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t node = 0;
    if (argc < 3 || JS_ToInt32(c, &node, argv[0])) return JS_NewUint32(c, 0);
    const char *app = JS_ToCString(c, argv[1]);
    JSValue keep;
    const char *facts = arg_json(c, argv[2], &keep);
    int allowed = app && (!only_app[0] || !strcmp(app, only_app));
    AxiaWindows *windows = argc > 3 ? js_windows_borrow(c, argv[3]) : NULL;
    unsigned h = 0;
    if (allowed && facts && (argc <= 3 || windows))
        h = windows ? (only_app[0] ? axia_view_mount_windows(node, app, facts, windows)
                                  : axia_view_mount_session(node, app, facts, windows))
                    : axia_view_mount(node, app, facts);
    if (app) JS_FreeCString(c, app);
    if (facts) JS_FreeCString(c, facts);
    JS_FreeValue(c, keep);
    return argc > 3 && !windows ? JS_EXCEPTION : JS_NewUint32(c, h);
}

static JSValue f_unmount(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc) axia_view_unmount(arg_handle(c, argv[0]));
    return JS_UNDEFINED;
}

/* facts(handle, json) · result(handle, json) */
static JSValue f_json(JSContext *c, JSValueConst t, int argc, JSValueConst *argv, int magic) {
    (void)t;
    if (argc < 2) return JS_UNDEFINED;
    JSValue keep;
    const char *json = arg_json(c, argv[1], &keep);
    if (json) {
        if (magic == 2) axia_view_action(arg_handle(c, argv[0]), json);
        else if (magic) axia_view_result(arg_handle(c, argv[0]), json);
        else axia_view_facts(arg_handle(c, argv[0]), json);
        JS_FreeCString(c, json);
    }
    JS_FreeValue(c, keep);
    return JS_UNDEFINED;
}

static JSValue f_focus(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc >= 2) axia_view_focus(arg_handle(c, argv[0]), JS_ToBool(c, argv[1]));
    return JS_UNDEFINED;
}

/* take(handle) → the effects waiting for the shell, or null */
static JSValue f_take(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (!argc) return JS_NULL;
    char *s = axia_view_take(arg_handle(c, argv[0]));
    if (!s) return JS_NULL;
    JSValue r = JS_ParseJSON(c, s, strlen(s), "<native-view>");
    axia_view_free(s);
    return r;
}

static JSValue f_paste(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 2) return JS_UNDEFINED;
    const char *s = JS_ToCString(c, argv[1]);
    if (s) { axia_view_paste(arg_handle(c, argv[0]), s); JS_FreeCString(c, s); }
    return JS_UNDEFINED;
}

/* key(key, char, flags) → true if a focused native text field took it */
static JSValue f_key(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 3) return JS_FALSE;
    const char *k = JS_ToCString(c, argv[0]), *ch = JS_ToCString(c, argv[1]);
    int32_t flags = 0;
    JS_ToInt32(c, &flags, argv[2]);
    int used = k && ch ? axia_view_key(k, ch, flags) : 0;
    if (k) JS_FreeCString(c, k);
    if (ch) JS_FreeCString(c, ch);
    return JS_NewBool(c, used);
}

static JSValue f_text(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    return argc ? owned_string(c, axia_view_text(arg_handle(c, argv[0]))) : JS_NULL;
}

/* Caption lookup returns a node owned by this view; it does not dispatch input. */
static JSValue f_find_text(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 2) return JS_NewInt32(c, 0);
    const char *label = JS_ToCString(c, argv[1]);
    int node = label ? axia_view_find_text(arg_handle(c, argv[0]), label) : 0;
    if (label) JS_FreeCString(c, label);
    return JS_NewInt32(c, node);
}

/* Read-only lookup: coordinate probes use the compositor's real scene target. */
static JSValue f_hit_test(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    double x = 0, y = 0;
    if (argc < 2) return JS_NewInt32(c, 0);
    if (JS_ToFloat64(c, &x, argv[0]) || JS_ToFloat64(c, &y, argv[1])) return JS_EXCEPTION;
    if (!(x > -1e9 && x < 1e9 && y > -1e9 && y < 1e9)) return JS_NewInt32(c, 0);
    Node *target = ui.root ? hit_test(ui.root, (float)x, (float)y) : NULL;
    return JS_NewInt32(c, target ? target->id : 0);
}

/* Synthetic input uses the same native pre-dispatch as js_pointer, returning
 * the shell mount target. The caller then dispatches its shell event normally. */
static JSValue f_pointer(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 5) return JS_NewInt32(c, 0);
    int32_t target = 0, button = 0;
    double x = 0, y = 0;
    if (JS_ToInt32(c, &target, argv[1]) || JS_ToFloat64(c, &x, argv[2]) ||
        JS_ToFloat64(c, &y, argv[3]) || JS_ToInt32(c, &button, argv[4])) return JS_EXCEPTION;
    if (!(x > -1e9 && x < 1e9 && y > -1e9 && y < 1e9)) return JS_NewInt32(c, target);
    const char *kind = JS_ToCString(c, argv[0]);
    if (!kind) return JS_EXCEPTION;
    int mount = axia_view_pointer(kind, target, (float)x, (float)y, button);
    JS_FreeCString(c, kind);
    return JS_NewInt32(c, mount);
}

/* Session-only synthetic evdev input for accessibility and compositor probes.
 * It uses the physical translation/ownership path, not __lordKey. */
static JSValue f_input_key(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t code = 0, value = 0;
    if (only_app[0] || argc < 2 || JS_ToInt32(c, &code, argv[0]) || JS_ToInt32(c, &value, argv[1]) ||
        code < 1 || code >= 256 || value < 0 || value > 2) return JS_FALSE;
    js_key(code, value);
    return JS_TRUE;
}
static JSValue f_modifiers(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return JS_NewInt32(c, axia_view_get_modifiers());
}
static JSValue f_input_swipe(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t phase=0, fingers=0; double dx=0, dy=0;
    if (only_app[0] || argc<4 || JS_ToInt32(c,&phase,argv[0]) || JS_ToInt32(c,&fingers,argv[1]) ||
        JS_ToFloat64(c,&dx,argv[2]) || JS_ToFloat64(c,&dy,argv[3]) || phase<1 || phase>4 ||
        fingers<2 || fingers>5 || !isfinite(dx) || !isfinite(dy)) return JS_FALSE;
    tp_route_swipe(phase,fingers,dx,dy);
    return JS_TRUE;
}
static const JSCFunctionListEntry funcs[] = {
    JS_CFUNC_DEF("inputKey", 2, f_input_key),
    JS_CFUNC_DEF("inputSwipe", 4, f_input_swipe),
    JS_CFUNC_DEF("modifiers", 0, f_modifiers),
    JS_CFUNC_DEF("mount", 3, f_mount),
    JS_CFUNC_DEF("unmount", 1, f_unmount),
    JS_CFUNC_MAGIC_DEF("facts", 2, f_json, 0),
    JS_CFUNC_MAGIC_DEF("result", 2, f_json, 1),
    JS_CFUNC_MAGIC_DEF("action", 2, f_json, 2),
    JS_CFUNC_DEF("focus", 2, f_focus),
    JS_CFUNC_DEF("take", 1, f_take),
    JS_CFUNC_DEF("paste", 2, f_paste),
    JS_CFUNC_DEF("key", 3, f_key),
    JS_CFUNC_DEF("text", 1, f_text),
    JS_CFUNC_DEF("findText", 2, f_find_text),
    JS_CFUNC_DEF("hitTest", 2, f_hit_test),
    JS_CFUNC_DEF("pointer", 5, f_pointer),
};

/* Session clock model: all scheduling and state transitions are Rust-owned. */
static JSValue f_clock_request(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (!argc) return JS_NULL;
    JSValue keep;
    const char *input = arg_json(c, argv[0], &keep);
    char *out = input ? axia_clock_request(input) : NULL;
    if (input) JS_FreeCString(c, input);
    JS_FreeValue(c, keep);
    if (!out) return JS_NULL;
    JSValue result = JS_ParseJSON(c, out, strlen(out), "<clock>");
    axia_view_free(out);
    return result;
}

/* An isolated app's process: lord.nativeViews for its own view only (no session clock). The app's
 * loop drives the views through js_tick (js.c) once this is on. */
void js_nativeview_register_isolated(JSContext *ctx, JSValue lord, const char *assets_dir, const char *id) {
    if (!id || !*id || strlen(id) >= sizeof only_app) return;
    snprintf(only_app, sizeof only_app, "%s", id);
    const char *home = getenv("HOME");
    axia_view_init(assets_dir ? assets_dir : "", home ? home : "");
    JSValue o = JS_NewObject(ctx);
    JS_SetPropertyFunctionList(ctx, o, funcs, sizeof funcs / sizeof funcs[0]);
    JS_SetPropertyStr(ctx, lord, "nativeViews", o);
}

int js_nativeview_isolated(void) { return only_app[0] != 0; }

void js_nativeview_register(JSContext *ctx, JSValue lord, const char *assets_dir) {
    JSValue clock = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, clock, "request", JS_NewCFunction(ctx, f_clock_request, "request", 1));
    JS_SetPropertyStr(ctx, lord, "clock", clock);
    const char *home = getenv("HOME");
    axia_view_init(assets_dir ? assets_dir : "", home ? home : "");
    JSValue o = JS_NewObject(ctx);
    JS_SetPropertyFunctionList(ctx, o, funcs, sizeof funcs / sizeof funcs[0]);
    JS_SetPropertyStr(ctx, lord, "nativeViews", o);
}
