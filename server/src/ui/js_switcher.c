/* Session-only bridge to the Rust selector. Isolated apps never get this API. */
#include "js_switcher.h"
#include "switcher_ffi.h"
#include <stdint.h>
#include <string.h>

static JSClassID class_id;
static AxiaSwitcher *state(JSContext *c, JSValueConst self) {
    return JS_GetOpaque2(c, self, class_id);
}
static void finalize(JSRuntime *rt, JSValue value) {
    (void)rt;
    axia_switcher_free(JS_GetOpaque(value, class_id));
}
static JSValue snapshot(JSContext *c, AxiaSwitcher *s) {
    JSValue out = JS_NewObject(c), order = JS_NewArray(c);
    for (size_t i = 0; i < axia_switcher_len(s); i++)
        JS_SetPropertyUint32(c, order, (uint32_t)i, JS_NewInt32(c, axia_switcher_source(s, i)));
    JS_SetPropertyStr(c, out, "order", order);
    JS_SetPropertyStr(c, out, "index", JS_NewInt32(c, axia_switcher_index(s)));
    return out;
}
static JSValue begin(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaSwitcher *s = state(c, self);
    if (!s) return JS_EXCEPTION;
    if (argc < 3 || !JS_IsArray(c, argv[0]) || !JS_IsString(argv[1]))
        return JS_ThrowTypeError(c, "switcher.begin expects keys, focused key and direction");
    JSValue length = JS_GetPropertyStr(c, argv[0], "length");
    uint32_t count = 0;
    int bad = JS_ToUint32(c, &count, length);
    JS_FreeValue(c, length);
    if (bad) return JS_EXCEPTION;
    if (count > 512) return JS_ThrowRangeError(c, "too many switcher entries");
    const char *keys[512] = {0}, *focused = NULL;
    for (uint32_t i = 0; i < count; i++) {
        JSValue key = JS_GetPropertyUint32(c, argv[0], i);
        if (!JS_IsString(key)) { JS_FreeValue(c, key); goto invalid; }
        size_t size = 0;
        keys[i] = JS_ToCStringLen(c, &size, key);
        JS_FreeValue(c, key);
        if (!keys[i]) goto failed;
        if (size > 1024 || strlen(keys[i]) != size) goto invalid;
    }
    focused = JS_ToCString(c, argv[1]);
    if (!focused) goto failed;
    axia_switcher_begin(s, keys, count, focused, JS_ToBool(c, argv[2]) > 0);
    for (uint32_t i = 0; i < count; i++) JS_FreeCString(c, keys[i]);
    JS_FreeCString(c, focused);
    return snapshot(c, s);
invalid:
    JS_ThrowTypeError(c, "invalid switcher key");
failed:
    for (uint32_t i = 0; i < count; i++) if (keys[i]) JS_FreeCString(c, keys[i]);
    if (focused) JS_FreeCString(c, focused);
    return JS_EXCEPTION;
}
static JSValue text_action(JSContext *c, JSValueConst self, int argc, JSValueConst *argv, int magic) {
    AxiaSwitcher *s = state(c, self);
    if (!s) return JS_EXCEPTION;
    if (!argc || !JS_IsString(argv[0])) return JS_ThrowTypeError(c, "expected a switcher key");
    size_t size = 0;
    const char *key = JS_ToCStringLen(c, &size, argv[0]);
    if (!key) return JS_EXCEPTION;
    if (size > 1024 || strlen(key) != size) {
        JS_FreeCString(c, key);
        return JS_ThrowRangeError(c, "invalid switcher key");
    }
    if (magic) axia_switcher_remove(s, key); else axia_switcher_focus(s, key);
    JS_FreeCString(c, key);
    return JS_NewInt32(c, axia_switcher_index(s));
}
static JSValue step(JSContext *c, JSValueConst self, int argc, JSValueConst *argv, int magic) {
    AxiaSwitcher *s = state(c, self);
    int32_t value = 0;
    if (!s) return JS_EXCEPTION;
    if (!argc) return JS_ThrowTypeError(c, "expected a switcher direction or index");
    if (JS_ToInt32(c, &value, argv[0])) return JS_EXCEPTION;
    if (magic) { if (value >= 0) axia_switcher_select(s, (size_t)value); }
    else axia_switcher_step(s, value);
    return JS_NewInt32(c, axia_switcher_index(s));
}
static JSValue finish(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaSwitcher *s = state(c, self);
    if (!s) return JS_EXCEPTION;
    return JS_NewInt32(c, axia_switcher_finish(s, argc && JS_ToBool(c, argv[0]) > 0));
}
void js_switcher_register(JSContext *c, JSValue lord) {
    if (!class_id) JS_NewClassID(&class_id);
    JSClassDef def = { .class_name = "AxiaSwitcher", .finalizer = finalize };
    JS_NewClass(JS_GetRuntime(c), class_id, &def);
    JSValue object = JS_NewObjectClass(c, class_id);
    JS_SetOpaque(object, axia_switcher_new());
    JS_SetPropertyStr(c, object, "begin", JS_NewCFunction(c, begin, "begin", 3));
    JS_SetPropertyStr(c, object, "finish", JS_NewCFunction(c, finish, "finish", 1));
    JS_SetPropertyStr(c, object, "focus", JS_NewCFunctionMagic(c, text_action, "focus", 1, JS_CFUNC_generic_magic, 0));
    JS_SetPropertyStr(c, object, "remove", JS_NewCFunctionMagic(c, text_action, "remove", 1, JS_CFUNC_generic_magic, 1));
    JS_SetPropertyStr(c, object, "step", JS_NewCFunctionMagic(c, step, "step", 1, JS_CFUNC_generic_magic, 0));
    JS_SetPropertyStr(c, object, "select", JS_NewCFunctionMagic(c, step, "select", 1, JS_CFUNC_generic_magic, 1));
    JS_SetPropertyStr(c, lord, "switcher", object);
}
