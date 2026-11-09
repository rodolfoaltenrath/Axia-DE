/* js_winmodel.c — lord.winmodel(op, json) → json: the window manager's pure rules in Rust
 * (winmodel_ffi.h, rust/axia-shell/src/winmodel). sdk/winmodel.js wraps it; a refused call throws a
 * TypeError with Rust's reason. Stateless, so isolated apps' processes have it too. */
#include "js_winmodel.h"
#include "winmodel_ffi.h"

#include <string.h>

static JSValue f_call(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 2 || !JS_IsString(argv[0]) || !JS_IsString(argv[1]))
        return JS_ThrowTypeError(c, "lord.winmodel expects an op and a JSON array");
    size_t oplen = 0, len = 0;
    const char *op = JS_ToCStringLen(c, &oplen, argv[0]);
    if (!op) return JS_EXCEPTION;
    const char *json = JS_ToCStringLen(c, &len, argv[1]);
    if (!json) { JS_FreeCString(c, op); return JS_EXCEPTION; }
    char *out = NULL;
    int rc = oplen < 64 && strlen(op) == oplen ? axia_winmodel_call(op, json, len, &out) : -1;
    JS_FreeCString(c, json);
    JS_FreeCString(c, op);
    JSValue r = rc == 0 && out ? JS_NewString(c, out)
              : JS_ThrowTypeError(c, "%s", out ? out : "lord.winmodel: invalid op");
    axia_winmodel_free(out);
    return r;
}

void js_winmodel_register(JSContext *c, JSValue lord) {
    JS_SetPropertyStr(c, lord, "winmodel", JS_NewCFunction(c, f_call, "winmodel", 2));
}
