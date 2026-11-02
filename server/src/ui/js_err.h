/* js_err.h — throwing translatable errors to JS (header only; see ../errcode.h).
 *
 *   return js_throw_code(c, "fs.outsideHome", "path", p, NULL);
 *   if (lpk_open(path, &pk, err, sizeof err)) return js_throw_line(c, err);   // err holds a code line
 *
 * The thrown Error has `code` ("fs.outsideHome") and `params` ({ path }) and, as its message, the
 * text in the user's language: the SDK registers globalThis.__lordErrorText(code, params)
 * (sdk/errors.js), so existing `catch (e) { show(e.message) }` code keeps working. Without the
 * SDK (tests) the message is the code line itself. A line that is not a code (prose from an older
 * path) is thrown as is. */
#ifndef LORD_JS_ERR_H
#define LORD_JS_ERR_H

#include "quickjs.h"
#include "../errcode.h"

static inline JSValue js_err_params(JSContext *c, const char *line)
{
    JSValue o = JS_NewObject(c);
    for (const char *p = strchr(line, ' '); p; p = strchr(p + 1, ' ')) {
        const char *eq = strchr(p + 1, '=');
        const char *end = strchr(p + 1, ' ');
        if (!eq || (end && eq > end)) continue;
        char key[64];
        size_t kl = (size_t)(eq - p - 1);
        if (!kl || kl >= sizeof key) continue;
        memcpy(key, p + 1, kl);
        key[kl] = 0;
        size_t vl = end ? (size_t)(end - eq - 1) : strlen(eq + 1);
        char *v = js_malloc(c, vl + 1);
        if (!v) break;
        size_t o2 = 0;
        for (size_t i = 0; i < vl; i++) {
            const char *s = eq + 1 + i;
            if (s[0] == '%' && i + 2 < vl) {
                char h[3] = { s[1], s[2], 0 };
                v[o2++] = (char)strtol(h, NULL, 16);
                i += 2;
            } else v[o2++] = *s;
        }
        v[o2] = 0;
        JS_SetPropertyStr(c, o, key, JS_NewStringLen(c, v, o2));
        js_free(c, v);
    }
    return o;
}

/* Throws the code line `line` (or prose, as is) as a translated Error. */
static inline JSValue js_throw_line(JSContext *c, const char *line)
{
    if (!line || !line[0]) line = "sys.unknown";
    JSValue e = JS_NewError(c);
    if (JS_IsException(e)) return e;
    if (!lerr_is(line)) {
        JS_SetPropertyStr(c, e, "message", JS_NewString(c, line));
        return JS_Throw(c, e);
    }
    size_t n = strcspn(line, " ");
    JSValue code = JS_NewStringLen(c, line, n);
    JSValue params = js_err_params(c, line);
    JSValue msg = JS_UNDEFINED;
    JSValue g = JS_GetGlobalObject(c);
    JSValue fn = JS_GetPropertyStr(c, g, "__lordErrorText");
    if (JS_IsFunction(c, fn)) {
        JSValue argv[2] = { code, params };
        msg = JS_Call(c, fn, JS_UNDEFINED, 2, argv);
        if (JS_IsException(msg)) { JS_FreeValue(c, JS_GetException(c)); msg = JS_UNDEFINED; }
        else if (!JS_IsString(msg)) { JS_FreeValue(c, msg); msg = JS_UNDEFINED; }
    }
    JS_FreeValue(c, fn);
    JS_FreeValue(c, g);
    if (JS_IsUndefined(msg)) msg = JS_NewString(c, line);
    JS_SetPropertyStr(c, e, "message", msg);
    JS_SetPropertyStr(c, e, "code", code);
    JS_SetPropertyStr(c, e, "params", params);
    return JS_Throw(c, e);
}

/* Throws code with (key, value) pairs ending in NULL. */
__attribute__((sentinel))
static inline JSValue js_throw_code(JSContext *c, const char *code, ...)
{
    char line[1024];
    va_list ap;
    va_start(ap, code);
    lerr_vset(line, sizeof line, code, ap);
    va_end(ap);
    return js_throw_line(c, line);
}

#endif
