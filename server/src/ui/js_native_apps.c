/* App-owned state and widget trees stay in Rust; this bridge owns only QuickJS handles. */
#include "js_native_apps.h"
#include <stdio.h>
#include <string.h>
typedef struct AxiaNativeApp AxiaNativeApp;
extern AxiaNativeApp *axia_native_app_new(const char *);
extern void axia_native_app_free(AxiaNativeApp *);
extern char *axia_native_app_request(AxiaNativeApp *, const char *);
extern void axia_native_app_reply_free(char *);
extern const unsigned char *axia_native_app_pixels(const AxiaNativeApp *, const char *, int *, int *);
static JSClassID class_id;
static JSValue pixels(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaNativeApp *state = JS_GetOpaque2(ctx, self, class_id);
    if (!state) return JS_EXCEPTION;
    if (!argc || !JS_IsString(argv[0])) return JS_NULL;
    const char *id = JS_ToCString(ctx, argv[0]);
    if (!id) return JS_EXCEPTION;
    int w = 0, h = 0;
    const unsigned char *data = axia_native_app_pixels(state, id, &w, &h);
    JS_FreeCString(ctx, id);
    if (!data || w < 1 || h < 1 || w > 2048 || h > 1024) return JS_NULL;
    JSValue buffer = JS_NewArrayBufferCopy(ctx, data, (size_t)w * h * 4);
    if (JS_IsException(buffer)) return buffer;
    JSValue object = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, object, "width", JS_NewInt32(ctx, w));
    JS_SetPropertyStr(ctx, object, "height", JS_NewInt32(ctx, h));
    JS_SetPropertyStr(ctx, object, "buffer", buffer);
    return object;
}
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
static JSValue make(JSContext *ctx, const char *id) {
    AxiaNativeApp *state = axia_native_app_new(id);
    if (!state) return JS_ThrowRangeError(ctx, "unknown native widget app");
    JSValue object = JS_NewObjectClass(ctx, class_id);
    if (JS_IsException(object)) { axia_native_app_free(state); return object; }
    JS_SetOpaque(object, state);
    JS_SetPropertyStr(ctx, object, "request", JS_NewCFunction(ctx, request, "request", 1));
    JS_SetPropertyStr(ctx, object, "pixels", JS_NewCFunction(ctx, pixels, "pixels", 1));
    return object;
}
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
    JSValue result = make(ctx, id);
    JS_FreeCString(ctx, id);
    return result;
}
void js_native_apps_register(JSContext *ctx, JSValue lord) {
    if (!class_id) JS_NewClassID(&class_id);
    if (!JS_IsRegisteredClass(JS_GetRuntime(ctx), class_id)) {
        JSClassDef def = { .class_name = "AxiaNativeApp", .finalizer = finalize };
        if (JS_NewClass(JS_GetRuntime(ctx), class_id, &def) < 0) return;
    }
    JSValue object = make(ctx, "forcequit");
    if (JS_IsException(object)) {                /* no compiled apps; the exception is not left pending */
        JS_FreeValue(ctx, JS_GetException(ctx));
        fprintf(stderr, "native apps: the shared host could not start\n");
        return;
    }
    JS_SetPropertyStr(ctx, object, "create", JS_NewCFunction(ctx, create, "create", 1));
    JS_SetPropertyStr(ctx, lord, "nativeApps", object);
}
