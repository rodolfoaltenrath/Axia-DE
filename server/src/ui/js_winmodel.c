/* js_winmodel.c — the window manager in Rust (winmodel_ffi.h, rust/axia-shell/src/winmodel).
 *
 *   lord.winmodel(op, json) → json       the pure rules (sdk/winmodel.js). Stateless, so isolated apps'
 *                                        processes have it too.
 *   lord.windowList.call(op, json) → json  the windows of this context (sdk/windows.js): the session's,
 *                                        or an isolated app's own; freed with the context.
 * A refused call throws a TypeError with Rust's reason. */
#include "js_winmodel.h"
#include "winmodel_ffi.h"

#include <string.h>

static JSClassID list_class;

static void list_finalize(JSRuntime *rt, JSValue v) {
    (void)rt;
    axia_windows_free(JS_GetOpaque(v, list_class));
}

/* state: the session's windows, or NULL for the pure rules */
static JSValue run(JSContext *c, AxiaWindows *state, int argc, JSValueConst *argv) {
    if (argc < 2 || !JS_IsString(argv[0]) || !JS_IsString(argv[1]))
        return JS_ThrowTypeError(c, "lord.winmodel expects an op and a JSON array");
    size_t oplen = 0, len = 0;
    const char *op = JS_ToCStringLen(c, &oplen, argv[0]);
    if (!op) return JS_EXCEPTION;
    const char *json = JS_ToCStringLen(c, &len, argv[1]);
    if (!json) { JS_FreeCString(c, op); return JS_EXCEPTION; }
    char *out = NULL;
    int rc = oplen >= 64 || strlen(op) != oplen ? -1
           : state ? axia_windows_call(state, op, json, len, &out) : axia_winmodel_call(op, json, len, &out);
    JS_FreeCString(c, json);
    JS_FreeCString(c, op);
    JSValue r = rc == 0 && out ? JS_NewString(c, out)
              : JS_ThrowTypeError(c, "%s", out ? out : "lord.winmodel: invalid op");
    axia_winmodel_free(out);
    return r;
}

static JSValue f_call(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    return run(c, NULL, argc, argv);
}

static JSValue f_list_call(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaWindows *s = JS_GetOpaque2(c, self, list_class);
    return s ? run(c, s, argc, argv) : JS_EXCEPTION;
}

void js_winmodel_register(JSContext *c, JSValue lord) {
    JS_SetPropertyStr(c, lord, "winmodel", JS_NewCFunction(c, f_call, "winmodel", 2));
}

void js_windows_register(JSContext *c, JSValue lord) {
    if (!list_class) JS_NewClassID(&list_class);
    JSClassDef def = { .class_name = "AxiaWindows", .finalizer = list_finalize };
    JS_NewClass(JS_GetRuntime(c), list_class, &def);
    JSValue o = JS_NewObjectClass(c, list_class);
    JS_SetOpaque(o, axia_windows_new());
    JS_SetPropertyStr(c, o, "call", JS_NewCFunction(c, f_list_call, "call", 2));
    JS_SetPropertyStr(c, lord, "windowList", o);
}
