/* Compatibility transport for shell clients. Collection and policy live in Rust. */
#include "js_monitor.h"
#include <string.h>
extern char *axia_monitor_request(const char *);
extern void axia_monitor_reply_free(char *);
static const char *methods[] = {
    "netTotals", "start", "stop", "want", "sample", "processes", "connections", "info",
    "map", "mapFrame", "detail", "detailInfo", "signal", "watchDisks", "diskTemps",
    "wantVoltage", "voltage", "historyEnable", "history", "historyClear"
};
static JSValue request(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv, int magic) {
    (void)self;
    JSValue input = JS_NewObject(ctx), args = JS_NewArray(ctx);
    JS_SetPropertyStr(ctx, input, "cmd", JS_NewString(ctx, methods[magic]));
    for (int i = 0; i < argc; i++) JS_SetPropertyUint32(ctx, args, i, JS_DupValue(ctx, argv[i]));
    JS_SetPropertyStr(ctx, input, "args", args);
    JSValue encoded = JS_JSONStringify(ctx, input, JS_UNDEFINED, JS_UNDEFINED);
    JS_FreeValue(ctx, input);
    if (JS_IsException(encoded)) return encoded;
    const char *json = JS_ToCString(ctx, encoded);
    JS_FreeValue(ctx, encoded);
    if (!json) return JS_EXCEPTION;
    char *reply = axia_monitor_request(json);
    JS_FreeCString(ctx, json);
    if (!reply) return JS_ThrowOutOfMemory(ctx);
    JSValue result = JS_ParseJSON(ctx, reply, strlen(reply), "<rust-monitor>");
    axia_monitor_reply_free(reply);
    return result;
}
void js_monitor_register(JSContext *ctx, JSValue lord, const char *assets_dir) {
    (void)assets_dir;
    char *reply = axia_monitor_request("{\"cmd\":\"reset\"}");
    axia_monitor_reply_free(reply);
    JSValue object = JS_NewObject(ctx);
    for (unsigned i = 0; i < sizeof methods / sizeof methods[0]; i++)
        JS_SetPropertyStr(ctx, object, methods[i], JS_NewCFunctionMagic(ctx, request, methods[i], 0, JS_CFUNC_generic_magic, i));
    JS_SetPropertyStr(ctx, lord, "monitor", object);
}
