/* Transport only. Each QuickJS context owns its Rust catalog and finalizer. */
#include "js_launcher.h"
#include "js_sys.h"
#include "../fsops.h"
#include <string.h>
typedef struct AxiaLauncher AxiaLauncher;
extern AxiaLauncher *axia_launcher_new(void);
extern void axia_launcher_free(AxiaLauncher *);
extern char *axia_launcher_request(AxiaLauncher *, const char *);
extern void axia_launcher_reply_free(char *);
extern void axia_launcher_enable_search(AxiaLauncher *, const char *, const char *);
static JSClassID class_id;
static void finalize(JSRuntime *rt, JSValue value) {
    (void)rt;
    axia_launcher_free(JS_GetOpaque(value, class_id));
}
static JSValue request(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaLauncher *state = JS_GetOpaque2(ctx, self, class_id);
    if (!state) return JS_EXCEPTION;
    if (!argc || !JS_IsString(argv[0])) return JS_ThrowTypeError(ctx, "launcher request requires JSON");
    size_t len;
    const char *input = JS_ToCStringLen(ctx, &len, argv[0]);
    if (!input) return JS_EXCEPTION;
    if (len > 8 * 1024 * 1024 || memchr(input, 0, len)) {
        JS_FreeCString(ctx, input);
        return JS_ThrowRangeError(ctx, "launcher request limit exceeded or contains NUL");
    }
    char *reply = axia_launcher_request(state, input);
    JS_FreeCString(ctx, input);
    if (!reply) return JS_ThrowOutOfMemory(ctx);
    JSValue value = JS_ParseJSON(ctx, reply, strlen(reply), "<axia-launcher>");
    axia_launcher_reply_free(reply);
    return value;
}
void js_launcher_register(JSContext *ctx, JSValue lord, int session) {
    if (!class_id) JS_NewClassID(&class_id);
    JSClassDef def = { .class_name = "AxiaLauncher", .finalizer = finalize };
    JS_NewClass(JS_GetRuntime(ctx), class_id, &def);
    JSValue object = JS_NewObjectClass(ctx, class_id);
    if (JS_IsException(object)) return;
    AxiaLauncher *state = axia_launcher_new();
    if (session) axia_launcher_enable_search(state, fsops_home(), js_sys_assets());
    JS_SetOpaque(object, state);
    JS_SetPropertyStr(ctx, object, "request", JS_NewCFunction(ctx, request, "request", 1));
    JS_SetPropertyStr(ctx, lord, "launcher", object);
}
