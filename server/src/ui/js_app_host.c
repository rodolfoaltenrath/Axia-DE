/* QuickJS value ABI and renderer structures only. Rust owns the isolated app,
 * its windows, input, sandbox order, broker launch and bytecode cache. */
#include "js_app_host.h"
#include "js_broker.h"
#include "ui.h"
#include <stdlib.h>
#include <string.h>

static JSValue window_open(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    const char *title = argc > 0 && JS_IsString(argv[0]) ? JS_ToCString(c, argv[0]) : NULL;
    int32_t w = 640, h = 480;
    if (argc > 1) JS_ToInt32(c, &w, argv[1]);
    if (argc > 2) JS_ToInt32(c, &h, argv[2]);
    int key = axia_app_window_open(title, w, h);
    if (title) JS_FreeCString(c, title);
    return JS_NewInt32(c, key);
}
static JSValue window_attach(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    int32_t key = -1, node = 0;
    if (argc < 2 || JS_ToInt32(c, &key, argv[0]) || JS_ToInt32(c, &node, argv[1])) return JS_FALSE;
    return JS_NewBool(c, axia_app_window_attach(key, node));
}
static JSValue window_close(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    int32_t key = -1;
    if (argc) JS_ToInt32(c, &key, argv[0]);
    axia_app_window_close(key);
    return JS_UNDEFINED;
}
static JSValue window_title(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    if (argc < 2) return JS_UNDEFINED;
    int32_t key = -1;
    JS_ToInt32(c, &key, argv[0]);
    const char *title = JS_ToCString(c, argv[1]);
    if (title) { axia_app_window_title(key, title); JS_FreeCString(c, title); }
    return JS_UNDEFINED;
}
void app_windows_register(JSContext *c, JSValue lord) {
    static const JSCFunctionListEntry methods[] = {
        JS_CFUNC_DEF("open", 3, window_open), JS_CFUNC_DEF("attach", 2, window_attach),
        JS_CFUNC_DEF("close", 1, window_close), JS_CFUNC_DEF("setTitle", 2, window_title),
    };
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, methods, sizeof methods / sizeof methods[0]);
    JS_SetPropertyStr(c, lord, "appWindows", o);
}
uint8_t *axia_app_qjs_compile(const char *src, size_t len, const char *name, size_t *outlen) {
    *outlen = 0;
    JSRuntime *rt = JS_NewRuntime();
    JSContext *ctx = rt ? JS_NewContext(rt) : NULL;
    uint8_t *copy = NULL;
    if (ctx) {
        JSValue fn = JS_Eval(ctx, src, len, name, JS_EVAL_TYPE_GLOBAL | JS_EVAL_FLAG_COMPILE_ONLY);
        if (!JS_IsException(fn)) {
            size_t size = 0;
            uint8_t *bc = JS_WriteObject(ctx, &size, fn, JS_WRITE_OBJ_BYTECODE);
            if (bc) {
                if (size && size <= (64u << 20) && (copy = malloc(size))) {
                    memcpy(copy, bc, size); *outlen = size;
                }
                js_free(ctx, bc);
            }
        } else JS_FreeValue(ctx, JS_GetException(ctx));
        JS_FreeValue(ctx, fn);
        JS_FreeContext(ctx);
    }
    if (rt) JS_FreeRuntime(rt);
    return copy;
}
int axia_app_qjs_eval(JSContext *ctx, const uint8_t *bytecode, size_t len, int *ok) {
    *ok = 0;
    JSValue fn = JS_ReadObject(ctx, bytecode, len, JS_READ_OBJ_BYTECODE);
    if (JS_IsException(fn)) { JS_FreeValue(ctx, JS_GetException(ctx)); return -1; }
    JSValue result = JS_EvalFunction(ctx, fn);
    *ok = !JS_IsException(result);
    JS_FreeValue(ctx, result);
    return *ok ? 0 : 1;
}
static Node *window_root(int node, int main) {
    Node *n = node ? node_get(node) : NULL;
    return n ? n : main ? ui.root : NULL;
}
int axia_app_paint(int node, int main, uint32_t *pixels, int w, int h) {
    Node *root = window_root(node, main);
    if (!root) return 0;
    Canvas canvas;
    canvas_init(&canvas, pixels, w, h, w);
    ui.width = (float)w; ui.height = (float)h;
    if (!layout_root(root, (float)w, (float)h)) return 0;
    ui.damage = (Box){0, 0, 0, 0};
    paint_root_clear(&canvas, root, 0x00000000);
    return 1;
}
int axia_app_hit(int node, int main, float x, float y) {
    Node *root = window_root(node, main);
    Node *target = root ? hit_test(root, x, y) : NULL;
    return target ? target->id : 0;
}
int axia_app_node_valid(int node) { return node_get(node) != NULL; }
int axia_app_dirty(void) { int dirty = ui.dirty; ui.dirty = 0; return dirty; }
int axia_app_quit(void) { return ui.quit_requested; }
