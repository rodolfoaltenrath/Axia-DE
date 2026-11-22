/* Rust session app control and compiled-app metadata; QuickJS owns only handles. */
#include "js_native_apps.h"
#include "js_winmodel.h"
#include <stdio.h>
#include <string.h>
typedef struct AxiaNativeApp AxiaNativeApp;
extern AxiaNativeApp *axia_native_app_new(const char *);
extern AxiaNativeApp *axia_native_app_new_windows(const char *, const AxiaWindows *);
extern void axia_native_app_free(AxiaNativeApp *);
extern char *axia_native_app_request(AxiaNativeApp *, const char *);
extern void axia_native_app_reply_free(char *);
extern char *axia_native_app_take_services(const AxiaNativeApp *);
static JSClassID class_id;
static void finalize(JSRuntime *rt, JSValue value) {
    (void)rt;
    axia_native_app_free(JS_GetOpaque(value, class_id));
}
static JSValue request(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaNativeApp *state = JS_GetOpaque2(ctx, self, class_id);
    if (!state) return JS_EXCEPTION;
    if (!argc || !JS_IsString(argv[0])) return JS_ThrowTypeError(ctx, "native app request requires JSON");
    size_t len;
    const char *input = JS_ToCStringLen(ctx, &len, argv[0]);
    if (!input) return JS_EXCEPTION;
    if (len > 1024 * 1024 || memchr(input, 0, len)) {
        JS_FreeCString(ctx, input);
        return JS_ThrowRangeError(ctx, "native app request is too large or contains NUL");
    }
    char *reply = axia_native_app_request(state, input);
    JS_FreeCString(ctx, input);
    if (!reply) return JS_ThrowOutOfMemory(ctx);
    JSValue result = JS_ParseJSON(ctx, reply, strlen(reply), "<axia-native-app>");
    axia_native_app_reply_free(reply);
    return result;
}
/* Root-only service effects; NULL means quiet and does not allocate/parse JSON. */
static JSValue take_services(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaNativeApp *state = JS_GetOpaque2(ctx, self, class_id);
    if (!state) return JS_EXCEPTION;
    char *reply = axia_native_app_take_services(state);
    if (!reply) return JS_NULL;
    JSValue result = JS_ParseJSON(ctx, reply, strlen(reply), "<axia-service-effects>");
    axia_native_app_reply_free(reply);
    return result;
}
static JSValue make(JSContext *ctx, const char *id, const AxiaWindows *windows) {
    AxiaNativeApp *state = windows ? axia_native_app_new_windows(id, windows) : axia_native_app_new(id);
    if (!state) return JS_ThrowRangeError(ctx, "unknown native widget app");
    JSValue object = JS_NewObjectClass(ctx, class_id);
    if (JS_IsException(object)) { axia_native_app_free(state); return object; }
    JS_SetOpaque(object, state);
    JS_SetPropertyStr(ctx, object, "request", JS_NewCFunction(ctx, request, "request", 1));
    return object;
}
/* In an isolated app's process: the one model it may create (its own id), or "" in the shell. */
static char only_id[129];

static JSValue create(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    if (!argc || !JS_IsString(argv[0])) return JS_ThrowTypeError(ctx, "native app id required");
    size_t len;
    const char *id = JS_ToCStringLen(ctx, &len, argv[0]);
    if (!id) return JS_EXCEPTION;
    if (len > 128 || memchr(id, 0, len)) {
        JS_FreeCString(ctx, id);
        return JS_ThrowRangeError(ctx, "invalid native app id");
    }
    if (only_id[0] && strcmp(id, only_id)) {
        JS_FreeCString(ctx, id);
        return JS_ThrowRangeError(ctx, "an isolated app can only use its own native model");
    }
    JSValue result = make(ctx, id, NULL);
    JS_FreeCString(ctx, id);
    return result;
}
static int register_class(JSContext *ctx) {
    if (!class_id) JS_NewClassID(&class_id);
    if (!JS_IsRegisteredClass(JS_GetRuntime(ctx), class_id)) {
        JSClassDef def = { .class_name = "AxiaNativeApp", .finalizer = finalize };
        if (JS_NewClass(JS_GetRuntime(ctx), class_id, &def) < 0) return -1;
    }
    return 0;
}

/* An isolated app (js_broker.c) gets lord.nativeApps with create() only: its own model (the
 * Passwords category catalog, for instance), never the shell's session control. */
void js_native_apps_register_isolated(JSContext *ctx, JSValue lord, const char *id) {
    if (!id || strlen(id) >= sizeof only_id || register_class(ctx)) return;
    snprintf(only_id, sizeof only_id, "%s", id);
    JSValue object = JS_NewObject(ctx);
    if (JS_IsException(object)) return;
    JS_SetPropertyStr(ctx, object, "create", JS_NewCFunction(ctx, create, "create", 1));
    JS_SetPropertyStr(ctx, lord, "nativeApps", object);
}

void js_native_apps_register(JSContext *ctx, JSValue lord) {
    if (register_class(ctx)) return;
    JSValue source = JS_GetPropertyStr(ctx, lord, "windowList");
    AxiaWindows *windows = js_windows_borrow(ctx, source);
    if (!windows) { JS_FreeValue(ctx, source); return; }
    JSValue object = make(ctx, "forcequit", windows);
    JS_FreeValue(ctx, source);
    if (JS_IsException(object)) {                /* no compiled apps; the exception is not left pending */
        JS_FreeValue(ctx, JS_GetException(ctx));
        fprintf(stderr, "native apps: the shared host could not start\n");
        return;
    }
    JS_SetPropertyStr(ctx, object, "takeServices", JS_NewCFunction(ctx, take_services, "takeServices", 0));
    JS_SetPropertyStr(ctx, object, "create", JS_NewCFunction(ctx, create, "create", 1));
    JS_SetPropertyStr(ctx, lord, "nativeApps", object);
}
