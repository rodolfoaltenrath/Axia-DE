/* Help content is owned by apps/help/content.rs. C only marshals JSON and
 * supplies the trusted assets root; the legacy raw list/read calls remain available.
 * lord.time.zoneOffset still uses the compositor's TZif reader. */
#include "js_help.h"
#include "js_sys.h"
#include "js_nativeview.h"
#include "tzif.h"
#include <string.h>

static JSValue help_request(JSContext *c, JSValueConst request) {
    JSValue encoded = JS_JSONStringify(c, request, JS_UNDEFINED, JS_UNDEFINED);
    if (JS_IsException(encoded)) return encoded;
    const char *input = JS_ToCString(c, encoded);
    char *out = input ? axia_help_request(js_sys_assets(), input) : NULL;
    if (input) JS_FreeCString(c, input);
    JS_FreeValue(c, encoded);
    if (!out) return JS_NULL;
    JSValue result = JS_ParseJSON(c, out, strlen(out), "<help>");
    axia_view_free(out);
    return result;
}

static JSValue f_help_request(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    return argc ? help_request(c, argv[0]) : JS_NULL;
}

/* Compatibility with callers of the original raw API (no locale merging). */
static JSValue f_help_raw(JSContext *c, JSValueConst self, int argc, JSValueConst *argv, int read) {
    (void)self;
    JSValue input = JS_NewObject(c);
    JS_SetPropertyStr(c, input, "cmd", JS_NewString(c, read ? "rawRead" : "rawList"));
    JS_SetPropertyStr(c, input, "lang", argc > 0 ? JS_DupValue(c, argv[0]) : JS_UNDEFINED);
    if (read) JS_SetPropertyStr(c, input, "id", argc > 1 ? JS_DupValue(c, argv[1]) : JS_UNDEFINED);
    JSValue result = help_request(c, input);
    JS_FreeValue(c, input);
    return result;
}

static JSValue f_zone_offset(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    const char *zone = JS_ToCString(c, argv[0]);
    double t = 0;
    if (argc > 1) JS_ToFloat64(c, &t, argv[1]);
    JSValue r = JS_NULL;
    TzInfo z;
    if (zone && t > -1e12 && t < 1e12 && !tz_zone_offset(zone, (int64_t)t, &z)) {
        r = JS_NewObject(c);
        JS_SetPropertyStr(c, r, "offset", JS_NewInt32(c, z.utoff));
        JS_SetPropertyStr(c, r, "abbr", JS_NewString(c, z.abbr));
        JS_SetPropertyStr(c, r, "dst", JS_NewBool(c, z.isdst));
    }
    if (zone) JS_FreeCString(c, zone);
    return r;
}

static const JSCFunctionListEntry help_funcs[] = {
    JS_CFUNC_MAGIC_DEF("list", 1, f_help_raw, 0),
    JS_CFUNC_MAGIC_DEF("read", 2, f_help_raw, 1),
    JS_CFUNC_DEF("request", 1, f_help_request),
};

void js_help_register(JSContext *c, JSValue lord) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, help_funcs, sizeof help_funcs / sizeof help_funcs[0]);
    JS_SetPropertyStr(c, lord, "help", o);
    /* lord.time was already created by js_sys_register: zoneOffset is added to it */
    JSValue time = JS_GetPropertyStr(c, lord, "time");
    if (JS_IsObject(time)) JS_SetPropertyStr(c, time, "zoneOffset", JS_NewCFunction(c, f_zone_offset, "zoneOffset", 2));
    JS_FreeValue(c, time);
}
