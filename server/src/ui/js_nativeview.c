/* lord.nativeViews: the shell's side of native views (see js_nativeview.h). The shell gives a
 * view its mount node, its facts (settings, theme, services) and runs the effects that belong
 * to the shell; drawing, input and captions are Rust's. */
#include "js_nativeview.h"
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

/* mount(nodeId, appId, facts) → handle, or 0 when the app has no native view */
static JSValue f_mount(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t node = 0;
    if (argc < 3 || JS_ToInt32(c, &node, argv[0])) return JS_NewUint32(c, 0);
    const char *app = JS_ToCString(c, argv[1]);
    JSValue keep;
    const char *facts = arg_json(c, argv[2], &keep);
    unsigned h = app && facts ? axia_view_mount(node, app, facts) : 0;
    if (app) JS_FreeCString(c, app);
    if (facts) JS_FreeCString(c, facts);
    JS_FreeValue(c, keep);
    return JS_NewUint32(c, h);
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
        if (magic) axia_view_result(arg_handle(c, argv[0]), json);
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

static const JSCFunctionListEntry funcs[] = {
    JS_CFUNC_DEF("mount", 3, f_mount),
    JS_CFUNC_DEF("unmount", 1, f_unmount),
    JS_CFUNC_MAGIC_DEF("facts", 2, f_json, 0),
    JS_CFUNC_MAGIC_DEF("result", 2, f_json, 1),
    JS_CFUNC_DEF("focus", 2, f_focus),
    JS_CFUNC_DEF("take", 1, f_take),
    JS_CFUNC_DEF("paste", 2, f_paste),
    JS_CFUNC_DEF("key", 3, f_key),
    JS_CFUNC_DEF("text", 1, f_text),
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
