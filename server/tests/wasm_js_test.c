/* wasm_js_test.c — runs tests/wasm/api_test.js with QuickJS and the WebAssembly global. */
#include "ui/js_wasm.h"

#include <quickjs.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static JSValue js_print(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    for (int i = 0; i < argc; i++) {
        const char *s = JS_ToCString(c, argv[i]);
        if (s) { fputs(s, stdout); JS_FreeCString(c, s); }
    }
    putchar('\n');
    return JS_UNDEFINED;
}

static char *slurp(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *b = malloc((size_t)n + 1);
    if (b && fread(b, 1, (size_t)n, f) != (size_t)n) { free(b); b = NULL; }
    fclose(f);
    if (b) b[n] = 0;
    *len = (size_t)n;
    return b;
}

static JSValue js_read(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val; (void)argc;
    const char *p = JS_ToCString(c, argv[0]);
    size_t n;
    char *b = p ? slurp(p, &n) : NULL;
    if (p) JS_FreeCString(c, p);
    if (!b) return JS_ThrowReferenceError(c, "could not read");
    JSValue ab = JS_NewArrayBufferCopy(c, (uint8_t *)b, n);
    free(b);
    return ab;
}

int main(int argc, char **argv) {
    const char *script = argc > 1 ? argv[1] : "tests/wasm/api_test.js";
    JSRuntime *rt = JS_NewRuntime();
    JSContext *c = JS_NewContext(rt);
    JSValue g = JS_GetGlobalObject(c);
    JS_SetPropertyStr(c, g, "print", JS_NewCFunction(c, js_print, "print", 1));
    JS_SetPropertyStr(c, g, "readFile", JS_NewCFunction(c, js_read, "readFile", 1));
    js_wasm_register(c, g);
    JS_FreeValue(c, g);
    size_t n;
    char *src = slurp(script, &n);
    if (!src) { fprintf(stderr, "could not read %s\n", script); return 1; }
    JSValue r = JS_Eval(c, src, n, script, JS_EVAL_TYPE_GLOBAL);
    int fail = 0;
    if (JS_IsException(r)) {
        JSValue e = JS_GetException(c);
        const char *s = JS_ToCString(c, e);
        fprintf(stderr, "exception: %s\n", s ? s : "?");
        if (s) JS_FreeCString(c, s);
        JS_FreeValue(c, e);
        fail = 1;
    }
    JS_FreeValue(c, r);
    JSContext *jc;
    while (JS_ExecutePendingJob(rt, &jc) > 0) {}
    free(src);
    js_wasm_shutdown(c);
    JS_FreeContext(c);
    JS_FreeRuntime(rt);                  /* with assert: nothing may be left alive */
    puts("context freed without leaks");
    return fail;
}
