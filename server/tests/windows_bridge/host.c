/* windows_bridge/host.c — runs a script in QuickJS with lord.winmodel and lord.windowList
 * (server/src/ui/js_winmodel.c), the bridges sdk/windows.js uses. server/tests/windows_bridge_test.mjs
 * bundles the script and runs it with this host (make -C server build/windows-bridge-host). */
#include "quickjs.h"
#include "js_winmodel.h"
#include <stdio.h>
#include <stdlib.h>
static JSValue print(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    for (int i = 0; i < argc; i++) { const char *s = JS_ToCString(c, argv[i]); fputs(s ? s : "?", stdout); if (s) JS_FreeCString(c, s); fputc(i + 1 < argc ? ' ' : '\n', stdout); }
    return JS_UNDEFINED;
}
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    FILE *f = fopen(argv[1], "rb"); if (!f) return 2;
    fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
    char *src = malloc((size_t)n + 1); if (fread(src, 1, (size_t)n, f) != (size_t)n) return 2; src[n] = 0; fclose(f);
    JSRuntime *rt = JS_NewRuntime(); JSContext *c = JS_NewContext(rt);
    JSValue g = JS_GetGlobalObject(c), lord = JS_NewObject(c), console = JS_NewObject(c);
    js_winmodel_register(c, lord);
    js_windows_register(c, lord);
    JS_SetPropertyStr(c, g, "lord", lord);
    JS_SetPropertyStr(c, console, "log", JS_NewCFunction(c, print, "log", 1));
    JS_SetPropertyStr(c, console, "error", JS_NewCFunction(c, print, "error", 1));
    JS_SetPropertyStr(c, console, "warn", JS_NewCFunction(c, print, "warn", 1));
    JS_SetPropertyStr(c, g, "console", console);
    JSValue r = JS_Eval(c, src, (size_t)n, argv[1], JS_EVAL_TYPE_GLOBAL);
    int bad = JS_IsException(r);
    if (bad) {
        JSValue e = JS_GetException(c); const char *m = JS_ToCString(c, e);
        JSValue st = JS_GetPropertyStr(c, e, "stack"); const char *sm = JS_ToCString(c, st);
        fprintf(stderr, "%s\n%s\n", m ? m : "?", sm ? sm : "");
    }
    JSContext *jc; while (JS_ExecutePendingJob(rt, &jc) > 0) {}
    JS_FreeValue(c, r); JS_FreeValue(c, g);
    JS_FreeContext(c); JS_FreeRuntime(rt);
    return bad;
}
