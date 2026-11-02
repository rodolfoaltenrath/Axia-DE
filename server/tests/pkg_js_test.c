/* pkg_js_test.c — tests lord.pkg (js_pkg.c) with QuickJS against the system's real index:
 *   build/pkg-js-test 'JS code'   (lord.pkg and print() available; measures the time) */
#include "ui/js_pkg.h"

#include <quickjs.h>
#include <stdio.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>

static JSValue js_print(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    for (int i = 0; i < argc; i++) {
        const char *s = JS_ToCString(c, argv[i]);
        if (s) { fputs(s, stdout); JS_FreeCString(c, s); }
        if (i + 1 < argc) putchar(' ');
    }
    putchar('\n');
    return JS_UNDEFINED;
}

static JSValue js_now(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val; (void)argc; (void)argv;
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return JS_NewFloat64(c, t.tv_sec * 1e3 + t.tv_nsec / 1e6);
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: pkg-js-test 'code'\n"); return 2; }
    JSRuntime *rt = JS_NewRuntime();
    JSContext *c = JS_NewContext(rt);
    JSValue g = JS_GetGlobalObject(c), lord = JS_NewObject(c);
    js_pkg_register(c, lord);
    JS_SetPropertyStr(c, g, "lord", lord);
    JS_SetPropertyStr(c, g, "print", JS_NewCFunction(c, js_print, "print", 1));
    JS_SetPropertyStr(c, g, "now", JS_NewCFunction(c, js_now, "now", 0));
    JS_FreeValue(c, g);
    JSValue r = JS_Eval(c, argv[1], strlen(argv[1]), "<test>", JS_EVAL_TYPE_GLOBAL);
    int bad = JS_IsException(r);
    if (bad) {
        JSValue e = JS_GetException(c);
        const char *s = JS_ToCString(c, e);
        fprintf(stderr, "error: %s\n", s ? s : "?");
        JS_FreeCString(c, s);
        JS_FreeValue(c, e);
    }
    JS_FreeValue(c, r);
    struct rusage ru;
    getrusage(RUSAGE_SELF, &ru);
    printf("peak memory: %ld KB\n", ru.ru_maxrss);
    JS_FreeContext(c);
    JS_FreeRuntime(rt);
    return bad;
}
