/* QuickJS transport only; Arquivos is implemented in Rust. */
#include "js_files.h"
#include <string.h>
extern char *axia_files_request(const char *input);
extern void axia_files_free(char *value);
static JSValue request(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    if (!argc) return JS_ThrowTypeError(ctx, "files.request requires JSON");
    size_t len;
    const char *input = JS_ToCStringLen(ctx, &len, argv[0]);
    if (!input) return JS_EXCEPTION;
    if (len > 1024 * 1024 || memchr(input, 0, len)) {
        JS_FreeCString(ctx, input);
        return JS_ThrowRangeError(ctx, "files request is too large or contains NUL");
    }
    char *result = axia_files_request(input);
    JS_FreeCString(ctx, input);
    if (!result) return JS_ThrowOutOfMemory(ctx);
    JSValue value = JS_ParseJSON(ctx, result, strlen(result), "<axia-files>");
    axia_files_free(result);
    return value;
}
void js_files_register(JSContext *ctx, JSValue lord) {
    JSValue files = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, files, "request", JS_NewCFunction(ctx, request, "request", 1));
    JS_SetPropertyStr(ctx, lord, "files", files);
}
