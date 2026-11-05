/* QuickJS transport only; file operations and shared native styling live in Rust. */
#include "js_files.h"
#include <string.h>
extern char *axia_files_request(const char *input);
extern char *axia_native_style_request(const char *input);
extern void axia_files_free(char *value);
static JSValue decode(JSContext *ctx, int argc, JSValueConst *argv, char *(*call)(const char *)) {
    if (!argc) return JS_ThrowTypeError(ctx, "native request requires JSON");
    size_t len;
    const char *input = JS_ToCStringLen(ctx, &len, argv[0]);
    if (!input) return JS_EXCEPTION;
    if (len > 1024 * 1024 || memchr(input, 0, len)) {
        JS_FreeCString(ctx, input);
        return JS_ThrowRangeError(ctx, "native request is too large or contains NUL");
    }
    char *result = call(input);
    JS_FreeCString(ctx, input);
    if (!result) return JS_ThrowOutOfMemory(ctx);
    JSValue value = JS_ParseJSON(ctx, result, strlen(result), "<axia-files>");
    axia_files_free(result); /* Both replies are Rust-owned CString allocations. */
    return value;
}
static JSValue request(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    return decode(ctx, argc, argv, axia_files_request);
}
static JSValue appearance(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    return decode(ctx, argc, argv, axia_native_style_request);
}
void js_files_register(JSContext *ctx, JSValue lord) {
    JSValue files = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, files, "request", JS_NewCFunction(ctx, request, "request", 1));
    JS_SetPropertyStr(ctx, lord, "files", files);
    JSValue style = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, style, "request", JS_NewCFunction(ctx, appearance, "request", 1));
    JS_SetPropertyStr(ctx, lord, "appearance", style);
}
