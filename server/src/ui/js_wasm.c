/* js_wasm.c — the standard `WebAssembly` global on top of our own interpreter (src/wasm).
 *
 * The same as in browsers: validate, compile, instantiate (with a Module or with bytes),
 * Module (exports, imports, customSections), Instance, Memory, Table, Global and the errors
 * CompileError, LinkError and RuntimeError. Exported functions are real JS functions
 * (typeof "function", call/apply/bind) and the same wasm function always yields the same
 * object. i64 ↔ BigInt. Memory.buffer is an ArrayBuffer over the real memory (no copy);
 * on growth, the previous one becomes detached (as the specification requires).
 *
 * Known limitation: the QuickJS collector doesn't see the references the interpreter keeps
 * to JS functions (imports) or to externref values; a cycle that goes through there lives
 * until the context is closed (js_wasm_shutdown cuts them before freeing it). */
#include "js_wasm.h"

#include "../wasm/wasm.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

static JSContext *wctx;
static JSClassID cls_module, cls_instance, cls_memory, cls_table, cls_global, cls_func;
static JSValue ns = { 0 };              /* the WebAssembly object */
static JSValue err_compile, err_link, err_runtime;
static JSValue fn_freeze, fn_proto;
static JSValue ext_map;                 /* externref: value → index */
static JSValue *ext_vals;
static uint32_t next_ext, ext_cap;
static JSValue pending;                 /* JS exception inside an imported function */
static int have_pending, alive;

/* ---- functions imported from JS ---- */

typedef struct HostFn {
    JSValue fn;
    const WasmFuncType *type;
    struct HostFn *prev, *next;
} HostFn;

static HostFn hosts = { .prev = &hosts, .next = &hosts };

static void host_free(void *ud) {
    HostFn *h = ud;
    h->prev->next = h->next;
    h->next->prev = h->prev;
    if (wctx && !JS_IsUndefined(h->fn)) JS_FreeValueRT(JS_GetRuntime(wctx), h->fn);
    free(h);
}

/* ---- errores ---- */

static JSValue throw_cls(JSContext *c, JSValue cls, const char *msg) {
    JSValue m = JS_NewString(c, msg);
    JSValue e = JS_CallConstructor(c, cls, 1, (JSValueConst *)&m);
    JS_FreeValue(c, m);
    if (JS_IsException(e)) return e;
    return JS_Throw(c, e);
}

static JSValue throw_wasm(JSContext *c, const WasmError *err) {
    switch (err->kind) {
    case WASM_ERR_HOST:
        if (have_pending) { have_pending = 0; return JS_Throw(c, pending); }
        return throw_cls(c, err_runtime, err->msg);
    case WASM_ERR_MALFORMED: case WASM_ERR_INVALID: return throw_cls(c, err_compile, err->msg);
    case WASM_ERR_LINK: return throw_cls(c, err_link, err->msg);
    case WASM_ERR_EXHAUSTED: return JS_ThrowRangeError(c, "%s", err->msg); /* i18n-ok: WebAssembly API */
    case WASM_ERR_NOMEM: return JS_ThrowRangeError(c, "%s", err->msg); /* i18n-ok: WebAssembly API */
    default: return throw_cls(c, err_runtime, err->msg);
    }
}

static void set_pending(JSContext *c) {
    JSValue e = JS_GetException(c);
    if (have_pending) JS_FreeValue(c, pending);
    pending = e;
    have_pending = 1;
}

/* ---- conversiones ---- */

static JSValue func_to_js(JSContext *c, WasmFunc *f);

static uint64_t ext_from_js(JSContext *c, JSValueConst v) {
    if (JS_IsNull(v)) return 0;
    JSValue get = JS_GetPropertyStr(c, ext_map, "get");
    JSValue r = JS_Call(c, get, ext_map, 1, &v);
    JS_FreeValue(c, get);
    if (JS_IsNumber(r)) {
        uint32_t i;
        JS_ToUint32(c, &i, r);
        return (uint64_t)i + 1;
    }
    JS_FreeValue(c, r);
    if (next_ext == ext_cap) {
        uint32_t nc = ext_cap ? ext_cap * 2 : 64;
        JSValue *n = realloc(ext_vals, nc * sizeof *n);
        if (!n) return 0;
        ext_vals = n;
        ext_cap = nc;
    }
    uint32_t idx = next_ext++;
    ext_vals[idx] = JS_DupValue(c, v);
    JSValue set = JS_GetPropertyStr(c, ext_map, "set");
    JSValueConst args[2] = { v, JS_NewInt32(c, (int32_t)idx) };
    JS_FreeValue(c, JS_Call(c, set, ext_map, 2, args));
    JS_FreeValue(c, set);
    return (uint64_t)idx + 1;
}

static JSValue ext_to_js(JSContext *c, uint64_t v) {
    if (!v || v > next_ext) return JS_NULL;
    return JS_DupValue(c, ext_vals[v - 1]);
}

/* ToWebAssemblyValue. -1 with an exception. */
static int to_wasm(JSContext *c, JSValueConst v, uint8_t type, WasmVal *out) {
    out->u64 = 0;
    switch (type) {
    case WASM_I32: return JS_ToInt32(c, &out->i32, v);
    case WASM_I64: return JS_ToBigInt64(c, &out->i64, v);
    case WASM_F32: { double d; if (JS_ToFloat64(c, &d, v)) return -1; out->f32 = (float)d; return 0; }
    case WASM_F64: return JS_ToFloat64(c, &out->f64, v);
    case WASM_EXTERNREF: out->u64 = ext_from_js(c, v); return 0;
    case WASM_FUNCREF: {
        if (JS_IsNull(v)) return 0;
        WasmFunc *f = JS_GetOpaque(v, cls_func);
        if (!f) { JS_ThrowTypeError(c, "expected an exported WebAssembly function or null"); return -1; } /* i18n-ok: WebAssembly API */
        out->ref = f;
        return 0;
    }
    }
    JS_ThrowTypeError(c, "unsupported value type"); /* i18n-ok: WebAssembly API */
    return -1;
}

static JSValue from_wasm(JSContext *c, uint8_t type, WasmVal v) {
    switch (type) {
    case WASM_I32: return JS_NewInt32(c, v.i32);
    case WASM_I64: return JS_NewBigInt64(c, v.i64);
    case WASM_F32: return JS_NewFloat64(c, (double)v.f32);
    case WASM_F64: return JS_NewFloat64(c, v.f64);
    case WASM_EXTERNREF: return ext_to_js(c, v.u64);
    case WASM_FUNCREF: return v.ref ? func_to_js(c, v.ref) : JS_NULL;
    }
    return JS_UNDEFINED;
}

static int valtype_from_str(const char *s, uint8_t *t) {
    if (!strcmp(s, "i32")) *t = WASM_I32;
    else if (!strcmp(s, "i64")) *t = WASM_I64;
    else if (!strcmp(s, "f32")) *t = WASM_F32;
    else if (!strcmp(s, "f64")) *t = WASM_F64;
    else if (!strcmp(s, "anyfunc") || !strcmp(s, "funcref")) *t = WASM_FUNCREF;
    else if (!strcmp(s, "externref")) *t = WASM_EXTERNREF;
    else return -1;
    return 0;
}

/* Default value of a type when the argument is missing (DefaultValue in the specification):
 * 0 for numbers, null for funcref and undefined for externref. */
static int default_or(JSContext *c, int argc, JSValueConst *argv, int i, uint8_t type, WasmVal *out) {
    if (i >= argc || JS_IsUndefined(argv[i])) {
        out->u64 = 0;
        if (type == WASM_EXTERNREF) out->u64 = ext_from_js(c, JS_UNDEFINED);
        return 0;
    }
    return to_wasm(c, argv[i], type, out);
}

/* ---- calling wasm from JS ---- */

static JSValue call_wasm(JSContext *c, WasmFunc *f, int argc, JSValueConst *argv) {
    const WasmFuncType *t = wasm_func_type(f);
    WasmVal sa[16], sr[16];
    WasmVal *a = t->nparams > 16 ? calloc(t->nparams, sizeof *a) : sa;
    WasmVal *r = t->nresults > 16 ? calloc(t->nresults, sizeof *r) : sr;
    JSValue ret = JS_UNDEFINED;
    if (!a || !r) { ret = JS_ThrowOutOfMemory(c); goto out; }
    for (uint32_t i = 0; i < t->nparams; i++)
        if (to_wasm(c, (int)i < argc ? argv[i] : JS_UNDEFINED, t->params[i], &a[i])) { ret = JS_EXCEPTION; goto out; }
    WasmError err;
    if (wasm_call(f, a, r, &err)) { ret = throw_wasm(c, &err); goto out; }
    if (t->nresults == 1) ret = from_wasm(c, t->results[0], r[0]);
    else if (t->nresults > 1) {
        ret = JS_NewArray(c);
        for (uint32_t i = 0; i < t->nresults; i++) JS_SetPropertyUint32(c, ret, i, from_wasm(c, t->results[i], r[i]));
    }
out:
    if (a != sa) free(a);
    if (r != sr) free(r);
    return ret;
}

static JSValue fn_call(JSContext *c, JSValueConst func_obj, JSValueConst this_val, int argc, JSValueConst *argv, int flags) {
    (void)this_val; (void)flags;
    WasmFunc *f = JS_GetOpaque(func_obj, cls_func);
    if (!f) return JS_ThrowTypeError(c, "not a WebAssembly function"); /* i18n-ok: WebAssembly API */
    return call_wasm(c, f, argc, argv);
}

static void fn_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmFunc *f = JS_GetOpaque(val, cls_func);
    if (!f) return;
    void **slot = wasm_func_slot(f);
    if (*slot == JS_VALUE_GET_PTR(val)) *slot = NULL;
    wasm_func_unref(f);
}

static JSValue func_to_js(JSContext *c, WasmFunc *f) {
    void **slot = wasm_func_slot(f);
    if (*slot) return JS_DupValue(c, JS_MKPTR(JS_TAG_OBJECT, *slot));
    JSValue o = JS_NewObjectProtoClass(c, fn_proto, cls_func);
    if (JS_IsException(o)) return o;
    wasm_func_ref(f);
    JS_SetOpaque(o, f);
    const WasmFuncType *t = wasm_func_type(f);
    JS_DefinePropertyValueStr(c, o, "length", JS_NewInt32(c, (int32_t)t->nparams), JS_PROP_CONFIGURABLE);
    char nm[24];
    snprintf(nm, sizeof nm, "%p", (void *)f);
    JS_DefinePropertyValueStr(c, o, "name", JS_NewString(c, nm), JS_PROP_CONFIGURABLE);
    *slot = JS_VALUE_GET_PTR(o);
    return o;
}

/* ---- calling JS from wasm (imports) ---- */

static int host_call(void *ud, const WasmVal *args, WasmVal *results, WasmError *err) {
    HostFn *h = ud;
    JSContext *c = wctx;
    const WasmFuncType *t = h->type;
    if (!c || JS_IsUndefined(h->fn)) { wasm_err(err, WASM_ERR_HOST, "the JS function no longer exists"); return -1; }
    JSValue sv[16];
    JSValue *jv = t->nparams > 16 ? calloc(t->nparams, sizeof *jv) : sv;
    if (!jv) { wasm_err(err, WASM_ERR_NOMEM, "out of memory"); return -1; }
    for (uint32_t i = 0; i < t->nparams; i++) jv[i] = from_wasm(c, t->params[i], args[i]);
    JSValue r = JS_Call(c, h->fn, JS_UNDEFINED, (int)t->nparams, (JSValueConst *)jv);
    for (uint32_t i = 0; i < t->nparams; i++) JS_FreeValue(c, jv[i]);
    if (jv != sv) free(jv);
    int fail = JS_IsException(r);
    if (!fail && t->nresults == 1) fail = to_wasm(c, r, t->results[0], &results[0]) != 0;
    else if (!fail && t->nresults > 1) {
        for (uint32_t i = 0; !fail && i < t->nresults; i++) {
            JSValue x = JS_GetPropertyUint32(c, r, i);
            fail = JS_IsException(x) || to_wasm(c, x, t->results[i], &results[i]) != 0;
            JS_FreeValue(c, x);
        }
    }
    JS_FreeValue(c, r);
    if (fail) { set_pending(c); wasm_err(err, WASM_ERR_HOST, "JS exception"); return -1; }
    return 0;
}

static WasmFunc *host_func(JSContext *c, JSValueConst fn, const WasmFuncType *t) {
    HostFn *h = calloc(1, sizeof *h);
    if (!h) return NULL;
    h->fn = JS_DupValue(c, fn);
    h->type = t;
    h->next = hosts.next;
    h->prev = &hosts;
    hosts.next->prev = h;
    hosts.next = h;
    WasmFunc *f = wasm_func_new_host(t, host_call, h, host_free);
    if (!f) host_free(h);
    return f;
}

/* ---- bytes of a BufferSource ---- */

static uint8_t *get_bytes(JSContext *c, JSValueConst v, size_t *len) {
    size_t n;
    uint8_t *p = JS_GetArrayBuffer(c, &n, v);
    if (p) { *len = n; return p; }
    JS_FreeValue(c, JS_GetException(c));
    size_t off, blen, per;
    JSValue buf = JS_GetTypedArrayBuffer(c, v, &off, &blen, &per);
    if (JS_IsException(buf)) {
        JS_FreeValue(c, JS_GetException(c));
        JS_ThrowTypeError(c, "expected an ArrayBuffer or a view (Uint8Array...)"); /* i18n-ok: WebAssembly API */
        return NULL;
    }
    p = JS_GetArrayBuffer(c, &n, buf);
    JS_FreeValue(c, buf);
    if (!p) return NULL;
    *len = blen;
    return p + off;
}

/* ---- Module ---- */

static void module_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmModule *m = JS_GetOpaque(val, cls_module);
    if (m) wasm_module_unref(m);
}

static JSValue module_obj(JSContext *c, WasmModule *m) {
    JSValue o = JS_NewObjectClass(c, (int)cls_module);
    if (JS_IsException(o)) { wasm_module_unref(m); return o; }
    JS_SetOpaque(o, m);
    return o;
}

static JSValue compile_bytes(JSContext *c, JSValueConst src) {
    size_t len;
    uint8_t *p = get_bytes(c, src, &len);
    if (!p) return JS_EXCEPTION;
    WasmError err;
    WasmModule *m = wasm_module_new(p, len, &err);
    if (!m) return throw_wasm(c, &err);
    return module_obj(c, m);
}

static JSValue module_ctor(JSContext *c, JSValueConst new_target, int argc, JSValueConst *argv) {
    (void)new_target;
    return compile_bytes(c, argc ? argv[0] : JS_UNDEFINED);
}

static const char *kind_name(int k) {
    static const char *const n[] = { "function", "table", "memory", "global" };
    return k >= 0 && k < 4 ? n[k] : "unknown";
}

static JSValue module_exports(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    WasmModule *m = argc ? JS_GetOpaque(argv[0], cls_module) : NULL;
    if (!m) return JS_ThrowTypeError(c, "expected a WebAssembly.Module"); /* i18n-ok: WebAssembly API */
    JSValue arr = JS_NewArray(c);
    for (uint32_t i = 0; i < wasm_module_num_exports(m); i++) {
        size_t len;
        const char *nm = wasm_export_name(m, i, &len);
        JSValue o = JS_NewObject(c);
        JS_SetPropertyStr(c, o, "name", JS_NewStringLen(c, nm, len));
        JS_SetPropertyStr(c, o, "kind", JS_NewString(c, kind_name(wasm_export_type(m, i).kind)));
        JS_SetPropertyUint32(c, arr, i, o);
    }
    return arr;
}

static JSValue module_imports(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    WasmModule *m = argc ? JS_GetOpaque(argv[0], cls_module) : NULL;
    if (!m) return JS_ThrowTypeError(c, "expected a WebAssembly.Module"); /* i18n-ok: WebAssembly API */
    JSValue arr = JS_NewArray(c);
    for (uint32_t i = 0; i < wasm_module_num_imports(m); i++) {
        size_t ml, nl;
        const char *mod = wasm_import_module(m, i, &ml), *nm = wasm_import_name(m, i, &nl);
        JSValue o = JS_NewObject(c);
        JS_SetPropertyStr(c, o, "module", JS_NewStringLen(c, mod, ml));
        JS_SetPropertyStr(c, o, "name", JS_NewStringLen(c, nm, nl));
        JS_SetPropertyStr(c, o, "kind", JS_NewString(c, kind_name(wasm_import_type(m, i).kind)));
        JS_SetPropertyUint32(c, arr, i, o);
    }
    return arr;
}

static JSValue module_custom(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    WasmModule *m = argc ? JS_GetOpaque(argv[0], cls_module) : NULL;
    if (!m) return JS_ThrowTypeError(c, "expected a WebAssembly.Module"); /* i18n-ok: WebAssembly API */
    const char *nm = JS_ToCString(c, argc > 1 ? argv[1] : JS_UNDEFINED);
    if (!nm) return JS_EXCEPTION;
    JSValue arr = JS_NewArray(c);
    size_t len;
    const uint8_t *p;
    for (uint32_t i = 0; (p = wasm_custom_section(m, nm, i, &len)); i++)
        JS_SetPropertyUint32(c, arr, i, JS_NewArrayBufferCopy(c, p, len));
    JS_FreeCString(c, nm);
    return arr;
}

/* ---- Memory ---- */

static void buf_free(JSRuntime *rt, void *opaque, void *ptr) {
    (void)rt;
    if (!ptr) return;                   /* already detached */
    WasmMemory *m = opaque;
    wasm_memory_slots(m)[1] = NULL;
    wasm_memory_unref(m);
}

static void mem_grown(void *ud, WasmMemory *m) {
    (void)ud;
    void **s = wasm_memory_slots(m);
    if (s[1] && wctx) {
        JSValue b = JS_MKPTR(JS_TAG_OBJECT, s[1]);
        s[1] = NULL;
        JS_DetachArrayBuffer(wctx, b);  /* llama a buf_free */
    }
}

static void memory_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmMemory *m = JS_GetOpaque(val, cls_memory);
    if (!m) return;
    void **s = wasm_memory_slots(m);
    if (s[0] == JS_VALUE_GET_PTR(val)) s[0] = NULL;
    wasm_memory_unref(m);
}

static JSValue memory_obj(JSContext *c, WasmMemory *m) {
    void **s = wasm_memory_slots(m);
    if (s[0]) return JS_DupValue(c, JS_MKPTR(JS_TAG_OBJECT, s[0]));
    JSValue o = JS_NewObjectClass(c, (int)cls_memory);
    if (JS_IsException(o)) return o;
    wasm_memory_ref(m);
    JS_SetOpaque(o, m);
    wasm_memory_on_grow(m, mem_grown, NULL);
    s[0] = JS_VALUE_GET_PTR(o);
    return o;
}

/* Integer from a descriptor ({initial, maximum}); -1 with an exception, 1 if missing. */
static int desc_u32(JSContext *c, JSValueConst d, const char *key, uint32_t lim, uint32_t *out) {
    JSValue v = JS_GetPropertyStr(c, d, key);
    if (JS_IsException(v)) return -1;
    if (JS_IsUndefined(v)) return 1;
    double x;
    int r = JS_ToFloat64(c, &x, v);
    JS_FreeValue(c, v);
    if (r) return -1;
    if (x != x || x < 0 || x > lim || isinf(x)) { JS_ThrowTypeError(c, "%s out of range", key); return -1; } /* i18n-ok: WebAssembly API */
    *out = (uint32_t)x;
    return 0;
}

static int limits_desc(JSContext *c, JSValueConst d, uint32_t lim, uint32_t *min, uint32_t *max) {
    if (!JS_IsObject(d)) { JS_ThrowTypeError(c, "expected a descriptor"); return -1; } /* i18n-ok: WebAssembly API */
    int r = desc_u32(c, d, "initial", lim, min);
    if (r == 1) r = desc_u32(c, d, "minimum", lim, min);
    if (r) { if (r == 1) JS_ThrowTypeError(c, "missing initial"); return -1; } /* i18n-ok: WebAssembly API */
    *max = WASM_NO_MAX;
    r = desc_u32(c, d, "maximum", lim, max);
    if (r < 0) return -1;
    if (*max != WASM_NO_MAX && *max < *min) { JS_ThrowRangeError(c, "maximum less than initial"); return -1; } /* i18n-ok: WebAssembly API */
    return 0;
}

static JSValue memory_ctor(JSContext *c, JSValueConst new_target, int argc, JSValueConst *argv) {
    (void)new_target;
    uint32_t min, max;
    if (limits_desc(c, argc ? argv[0] : JS_UNDEFINED, 65536, &min, &max)) return JS_EXCEPTION;
    WasmError err;
    WasmMemory *m = wasm_memory_new(min, max, &err);
    if (!m) return JS_ThrowRangeError(c, "%s", err.msg); /* i18n-ok: WebAssembly API */
    JSValue o = memory_obj(c, m);
    wasm_memory_unref(m);
    return o;
}

static JSValue memory_buffer(JSContext *c, JSValueConst this_val) {
    WasmMemory *m = JS_GetOpaque2(c, this_val, cls_memory);
    if (!m) return JS_EXCEPTION;
    void **s = wasm_memory_slots(m);
    if (s[1]) return JS_DupValue(c, JS_MKPTR(JS_TAG_OBJECT, s[1]));
    wasm_memory_ref(m);
    JSValue b = JS_NewArrayBuffer(c, wasm_memory_data(m), wasm_memory_size(m), buf_free, m, 0);
    if (JS_IsException(b)) { wasm_memory_unref(m); return b; }
    wasm_memory_on_grow(m, mem_grown, NULL);
    s[1] = JS_VALUE_GET_PTR(b);
    return b;
}

static JSValue memory_grow(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    WasmMemory *m = JS_GetOpaque2(c, this_val, cls_memory);
    if (!m) return JS_EXCEPTION;
    double d;
    if (JS_ToFloat64(c, &d, argc ? argv[0] : JS_UNDEFINED)) return JS_EXCEPTION;
    if (d != d || d < 0 || d > 65536) return JS_ThrowTypeError(c, "delta out of range"); /* i18n-ok: WebAssembly API */
    int64_t r = wasm_memory_grow(m, (uint32_t)d);
    if (r < 0) return JS_ThrowRangeError(c, "could not grow the memory"); /* i18n-ok: WebAssembly API */
    return JS_NewInt64(c, r);
}

/* ---- Table ---- */

static void table_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmTable *t = JS_GetOpaque(val, cls_table);
    if (!t) return;
    void **s = wasm_table_slot(t);
    if (*s == JS_VALUE_GET_PTR(val)) *s = NULL;
    wasm_table_unref(t);
}

static JSValue table_obj(JSContext *c, WasmTable *t) {
    void **s = wasm_table_slot(t);
    if (*s) return JS_DupValue(c, JS_MKPTR(JS_TAG_OBJECT, *s));
    JSValue o = JS_NewObjectClass(c, (int)cls_table);
    if (JS_IsException(o)) return o;
    wasm_table_ref(t);
    JS_SetOpaque(o, t);
    *s = JS_VALUE_GET_PTR(o);
    return o;
}

static JSValue table_ctor(JSContext *c, JSValueConst new_target, int argc, JSValueConst *argv) {
    (void)new_target;
    JSValueConst d = argc ? argv[0] : JS_UNDEFINED;
    if (!JS_IsObject(d)) return JS_ThrowTypeError(c, "expected a descriptor"); /* i18n-ok: WebAssembly API */
    JSValue ev = JS_GetPropertyStr(c, d, "element");
    const char *es = JS_ToCString(c, ev);
    JS_FreeValue(c, ev);
    if (!es) return JS_EXCEPTION;
    uint8_t type;
    int bad = valtype_from_str(es, &type) || (type != WASM_FUNCREF && type != WASM_EXTERNREF);
    JS_FreeCString(c, es);
    if (bad) return JS_ThrowTypeError(c, "element must be 'anyfunc' or 'externref'"); /* i18n-ok: WebAssembly API */
    uint32_t min, max;
    if (limits_desc(c, d, 10000000, &min, &max)) return JS_EXCEPTION;
    WasmVal init;
    if (default_or(c, argc, argv, 1, type, &init)) return JS_EXCEPTION;
    WasmError err;
    WasmTable *t = wasm_table_new(type, min, max, init, &err);
    if (!t) return JS_ThrowRangeError(c, "%s", err.msg); /* i18n-ok: WebAssembly API */
    JSValue o = table_obj(c, t);
    wasm_table_unref(t);
    return o;
}

static JSValue table_length(JSContext *c, JSValueConst this_val) {
    WasmTable *t = JS_GetOpaque2(c, this_val, cls_table);
    if (!t) return JS_EXCEPTION;
    return JS_NewUint32(c, wasm_table_size(t));
}

static int index_arg(JSContext *c, int argc, JSValueConst *argv, int i, uint32_t *out) {
    double d;
    if (JS_ToFloat64(c, &d, i < argc ? argv[i] : JS_UNDEFINED)) return -1;
    if (d != d || d < 0 || d > 4294967295.0) { JS_ThrowTypeError(c, "index out of range"); return -1; } /* i18n-ok: WebAssembly API */
    *out = (uint32_t)d;
    return 0;
}

static JSValue table_get(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    WasmTable *t = JS_GetOpaque2(c, this_val, cls_table);
    uint32_t i;
    if (!t || index_arg(c, argc, argv, 0, &i)) return JS_EXCEPTION;
    WasmVal v;
    if (wasm_table_get(t, i, &v)) return JS_ThrowRangeError(c, "index outside the table"); /* i18n-ok: WebAssembly API */
    return from_wasm(c, wasm_table_elemtype(t), v);
}

static JSValue table_set(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    WasmTable *t = JS_GetOpaque2(c, this_val, cls_table);
    uint32_t i;
    if (!t || index_arg(c, argc, argv, 0, &i)) return JS_EXCEPTION;
    WasmVal v;
    if (default_or(c, argc, argv, 1, wasm_table_elemtype(t), &v)) return JS_EXCEPTION;
    if (wasm_table_set(t, i, v)) return JS_ThrowRangeError(c, "index outside the table"); /* i18n-ok: WebAssembly API */
    return JS_UNDEFINED;
}

static JSValue table_grow(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    WasmTable *t = JS_GetOpaque2(c, this_val, cls_table);
    uint32_t n;
    if (!t || index_arg(c, argc, argv, 0, &n)) return JS_EXCEPTION;
    WasmVal v;
    if (default_or(c, argc, argv, 1, wasm_table_elemtype(t), &v)) return JS_EXCEPTION;
    int64_t r = wasm_table_grow(t, n, v);
    if (r < 0) return JS_ThrowRangeError(c, "could not grow the table"); /* i18n-ok: WebAssembly API */
    return JS_NewInt64(c, r);
}

/* ---- Global ---- */

static void global_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmGlobal *g = JS_GetOpaque(val, cls_global);
    if (!g) return;
    void **s = wasm_global_slot(g);
    if (*s == JS_VALUE_GET_PTR(val)) *s = NULL;
    wasm_global_unref(g);
}

static JSValue global_obj(JSContext *c, WasmGlobal *g) {
    void **s = wasm_global_slot(g);
    if (*s) return JS_DupValue(c, JS_MKPTR(JS_TAG_OBJECT, *s));
    JSValue o = JS_NewObjectClass(c, (int)cls_global);
    if (JS_IsException(o)) return o;
    wasm_global_ref(g);
    JS_SetOpaque(o, g);
    *s = JS_VALUE_GET_PTR(o);
    return o;
}

static JSValue global_ctor(JSContext *c, JSValueConst new_target, int argc, JSValueConst *argv) {
    (void)new_target;
    JSValueConst d = argc ? argv[0] : JS_UNDEFINED;
    if (!JS_IsObject(d)) return JS_ThrowTypeError(c, "expected a descriptor"); /* i18n-ok: WebAssembly API */
    JSValue tv = JS_GetPropertyStr(c, d, "value"), mv = JS_GetPropertyStr(c, d, "mutable");
    const char *ts = JS_ToCString(c, tv);
    int mut = JS_ToBool(c, mv);
    JS_FreeValue(c, tv);
    JS_FreeValue(c, mv);
    if (!ts) return JS_EXCEPTION;
    uint8_t type;
    int bad = valtype_from_str(ts, &type);
    JS_FreeCString(c, ts);
    if (bad) return JS_ThrowTypeError(c, "unknown global type"); /* i18n-ok: WebAssembly API */
    WasmVal v;
    if (default_or(c, argc, argv, 1, type, &v)) return JS_EXCEPTION;
    WasmGlobal *g = wasm_global_new(type, mut, v);
    if (!g) return JS_ThrowOutOfMemory(c);
    JSValue o = global_obj(c, g);
    wasm_global_unref(g);
    return o;
}

static JSValue global_value(JSContext *c, JSValueConst this_val) {
    WasmGlobal *g = JS_GetOpaque2(c, this_val, cls_global);
    if (!g) return JS_EXCEPTION;
    return from_wasm(c, wasm_global_type(g), wasm_global_get(g));
}

static JSValue global_set_value(JSContext *c, JSValueConst this_val, JSValueConst val) {
    WasmGlobal *g = JS_GetOpaque2(c, this_val, cls_global);
    if (!g) return JS_EXCEPTION;
    if (!wasm_global_mut(g)) return JS_ThrowTypeError(c, "the global is not mutable"); /* i18n-ok: WebAssembly API */
    WasmVal v;
    if (to_wasm(c, val, wasm_global_type(g), &v)) return JS_EXCEPTION;
    wasm_global_set(g, v);
    return JS_UNDEFINED;
}

static JSValue global_value_of(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    return global_value(c, this_val);
}

/* ---- Instance ---- */

static void instance_finalizer(JSRuntime *rt, JSValue val) {
    (void)rt;
    WasmInstance *i = JS_GetOpaque(val, cls_instance);
    if (i) wasm_instance_unref(i);
}

static JSValue extern_to_js(JSContext *c, WasmExtern e) {
    switch (e.kind) {
    case WASM_EXT_FUNC: return func_to_js(c, e.func);
    case WASM_EXT_TABLE: return table_obj(c, e.table);
    case WASM_EXT_MEMORY: return memory_obj(c, e.memory);
    case WASM_EXT_GLOBAL: return global_obj(c, e.global);
    }
    return JS_UNDEFINED;
}

static JSValue get_prop_len(JSContext *c, JSValueConst o, const char *name, size_t len) {
    JSAtom a = JS_NewAtomLen(c, name, len);
    JSValue v = JS_GetProperty(c, o, a);
    JS_FreeAtom(c, a);
    return v;
}

static JSValue link_error(JSContext *c, const char *mod, const char *name, const char *why) {
    char msg[300];
    snprintf(msg, sizeof msg, "import %.100s.%.100s: %s", mod, name, why);
    return throw_cls(c, err_link, msg);
}

/* Instantiates m with the imports object: the JS Instance, or an exception. */
static JSValue instantiate(JSContext *c, WasmModule *m, JSValueConst imports) {
    uint32_t n = wasm_module_num_imports(m);
    if (n && !JS_IsObject(imports)) return JS_ThrowTypeError(c, "the module has imports: the imports object is missing"); /* i18n-ok: WebAssembly API */
    WasmExtern *ext = calloc(n + 1, sizeof *ext);
    uint8_t *temp = calloc(n + 1, 1);           /* created here (they must be released) */
    JSValue ret = JS_EXCEPTION;
    if (!ext || !temp) { ret = JS_ThrowOutOfMemory(c); goto out; }
    for (uint32_t i = 0; i < n; i++) {
        size_t ml, nl;
        const char *mod = wasm_import_module(m, i, &ml), *nm = wasm_import_name(m, i, &nl);
        WasmExternType t = wasm_import_type(m, i);
        JSValue o = get_prop_len(c, imports, mod, ml);
        if (JS_IsException(o)) goto out;
        if (!JS_IsObject(o)) { JS_FreeValue(c, o); ret = JS_ThrowTypeError(c, "import %s: not an object", mod); goto out; } /* i18n-ok: WebAssembly API */
        JSValue v = get_prop_len(c, o, nm, nl);
        JS_FreeValue(c, o);
        if (JS_IsException(v)) goto out;
        const char *why = NULL;
        ext[i].kind = t.kind;
        switch (t.kind) {
        case WASM_EXT_FUNC:
            if (!JS_IsFunction(c, v)) { why = "not a function"; break; }
            ext[i].func = JS_GetOpaque(v, cls_func);
            if (!ext[i].func) {
                ext[i].func = host_func(c, v, t.func);
                if (!ext[i].func) why = "out of memory";
                temp[i] = 1;
            }
            break;
        case WASM_EXT_GLOBAL:
            ext[i].global = JS_GetOpaque(v, cls_global);
            if (ext[i].global) break;
            if (t.mut) { why = "a mutable global must be a WebAssembly.Global"; break; }
            if (t.valtype == WASM_I64 ? !JS_IsBigInt(c, v) : (t.valtype != WASM_EXTERNREF && t.valtype != WASM_FUNCREF && !JS_IsNumber(v))) {
                why = "value of the wrong type";
                break;
            }
            {
                WasmVal gv;
                if (to_wasm(c, v, t.valtype, &gv)) { JS_FreeValue(c, v); goto out; }
                ext[i].global = wasm_global_new(t.valtype, 0, gv);
                temp[i] = 1;
            }
            break;
        case WASM_EXT_MEMORY:
            ext[i].memory = JS_GetOpaque(v, cls_memory);
            if (!ext[i].memory) why = "not a WebAssembly.Memory";
            break;
        case WASM_EXT_TABLE:
            ext[i].table = JS_GetOpaque(v, cls_table);
            if (!ext[i].table) why = "not a WebAssembly.Table";
            break;
        }
        JS_FreeValue(c, v);
        if (why) { ret = link_error(c, mod, nm, why); goto out; }
    }
    WasmError err;
    WasmInstance *inst = wasm_instantiate(m, ext, n, &err);
    if (!inst) { ret = throw_wasm(c, &err); goto out; }
    JSValue io = JS_NewObjectClass(c, (int)cls_instance);
    if (JS_IsException(io)) { wasm_instance_unref(inst); goto out; }
    JS_SetOpaque(io, inst);
    JSValue exports = JS_NewObjectProto(c, JS_NULL);
    for (uint32_t i = 0; i < wasm_module_num_exports(m); i++) {
        size_t len;
        const char *nm = wasm_export_name(m, i, &len);
        JSAtom a = JS_NewAtomLen(c, nm, len);
        JS_DefinePropertyValue(c, exports, a, extern_to_js(c, wasm_instance_export(inst, i)), JS_PROP_ENUMERABLE);
        JS_FreeAtom(c, a);
    }
    JS_FreeValue(c, JS_Call(c, fn_freeze, JS_UNDEFINED, 1, (JSValueConst *)&exports));
    JS_DefinePropertyValueStr(c, io, "exports", exports, JS_PROP_ENUMERABLE);
    ret = io;
out:
    for (uint32_t i = 0; ext && temp && i < n; i++) if (temp[i] && ext[i].ptr) wasm_extern_unref(ext[i]);
    free(ext);
    free(temp);
    return ret;
}

static JSValue instance_ctor(JSContext *c, JSValueConst new_target, int argc, JSValueConst *argv) {
    (void)new_target;
    WasmModule *m = argc ? JS_GetOpaque(argv[0], cls_module) : NULL;
    if (!m) return JS_ThrowTypeError(c, "expected a WebAssembly.Module"); /* i18n-ok: WebAssembly API */
    return instantiate(c, m, argc > 1 ? argv[1] : JS_UNDEFINED);
}

/* ---- namespace functions ---- */

static JSValue settle(JSContext *c, JSValue v) {
    JSValue fns[2];
    JSValue p = JS_NewPromiseCapability(c, fns);
    if (JS_IsException(p)) { JS_FreeValue(c, v); return p; }
    JSValue arg;
    int which = 0;
    if (JS_IsException(v)) { arg = JS_GetException(c); which = 1; }
    else arg = v;
    JS_FreeValue(c, JS_Call(c, fns[which], JS_UNDEFINED, 1, (JSValueConst *)&arg));
    JS_FreeValue(c, arg);
    JS_FreeValue(c, fns[0]);
    JS_FreeValue(c, fns[1]);
    return p;
}

static JSValue ns_validate(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    size_t len;
    uint8_t *p = get_bytes(c, argc ? argv[0] : JS_UNDEFINED, &len);
    if (!p) return JS_EXCEPTION;
    WasmError err;
    return JS_NewBool(c, wasm_validate(p, len, &err) == 0);
}

static JSValue ns_compile(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    return settle(c, compile_bytes(c, argc ? argv[0] : JS_UNDEFINED));
}

static JSValue ns_instantiate(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    JSValueConst src = argc ? argv[0] : JS_UNDEFINED, imports = argc > 1 ? argv[1] : JS_UNDEFINED;
    WasmModule *m = JS_GetOpaque(src, cls_module);
    if (m) return settle(c, instantiate(c, m, imports));
    JSValue mo = compile_bytes(c, src);
    if (JS_IsException(mo)) return settle(c, mo);
    JSValue io = instantiate(c, JS_GetOpaque(mo, cls_module), imports);
    if (JS_IsException(io)) { JS_FreeValue(c, mo); return settle(c, io); }
    JSValue r = JS_NewObject(c);
    JS_SetPropertyStr(c, r, "module", mo);
    JS_SetPropertyStr(c, r, "instance", io);
    return settle(c, r);
}

/* ---- registration ---- */

static const JSCFunctionListEntry ns_funcs[] = {
    JS_CFUNC_DEF("validate", 1, ns_validate),
    JS_CFUNC_DEF("compile", 1, ns_compile),
    JS_CFUNC_DEF("instantiate", 1, ns_instantiate),
};
static const JSCFunctionListEntry module_static[] = {
    JS_CFUNC_DEF("exports", 1, module_exports),
    JS_CFUNC_DEF("imports", 1, module_imports),
    JS_CFUNC_DEF("customSections", 2, module_custom),
};
static const JSCFunctionListEntry memory_proto[] = {
    JS_CGETSET_DEF("buffer", memory_buffer, NULL),
    JS_CFUNC_DEF("grow", 1, memory_grow),
};
static const JSCFunctionListEntry table_proto[] = {
    JS_CGETSET_DEF("length", table_length, NULL),
    JS_CFUNC_DEF("get", 1, table_get),
    JS_CFUNC_DEF("set", 1, table_set),
    JS_CFUNC_DEF("grow", 1, table_grow),
};
static const JSCFunctionListEntry global_proto[] = {
    JS_CGETSET_DEF("value", global_value, global_set_value),
    JS_CFUNC_DEF("valueOf", 0, global_value_of),
};

static void add_class(JSContext *c, JSClassID *id, const char *name, JSClassFinalizer *fin, JSClassCall *call,
                      JSCFunction *ctor, int nargs, const JSCFunctionListEntry *proto_funcs, int nproto) {
    JSRuntime *rt = JS_GetRuntime(c);
    if (!*id) JS_NewClassID(id);
    if (!JS_IsRegisteredClass(rt, *id)) {
        JSClassDef def = { .class_name = name, .finalizer = fin, .call = call };
        JS_NewClass(rt, *id, &def);
    }
    if (call) return;
    JSValue proto = JS_NewObject(c);
    if (proto_funcs) JS_SetPropertyFunctionList(c, proto, proto_funcs, nproto);
    JS_SetClassProto(c, *id, JS_DupValue(c, proto));
    JSValue f = JS_NewCFunction2(c, ctor, name, nargs, JS_CFUNC_constructor, 0);
    JS_SetConstructor(c, f, proto);
    if (*id == cls_module) JS_SetPropertyFunctionList(c, f, module_static, sizeof module_static / sizeof *module_static);
    JS_SetPropertyStr(c, ns, name, f);
    JS_FreeValue(c, proto);
}

void js_wasm_register(JSContext *c, JSValueConst global) {
    wctx = c;
    alive = 1;
    ns = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, ns, ns_funcs, sizeof ns_funcs / sizeof *ns_funcs);
    /* errors: subclasses of Error, as in browsers */
    static const char errs[] =
        "(() => { const mk = (n) => { const C = class extends Error { constructor(m) { super(m); } };"
        " Object.defineProperty(C, 'name', { value: n }); C.prototype.name = n; return C; };"
        " return [mk('CompileError'), mk('LinkError'), mk('RuntimeError'), Object.freeze,"
        " Object.getPrototypeOf(function () {}), new Map()]; })()";
    JSValue arr = JS_Eval(c, errs, sizeof errs - 1, "<wasm>", JS_EVAL_TYPE_GLOBAL);
    err_compile = JS_GetPropertyUint32(c, arr, 0);
    err_link = JS_GetPropertyUint32(c, arr, 1);
    err_runtime = JS_GetPropertyUint32(c, arr, 2);
    fn_freeze = JS_GetPropertyUint32(c, arr, 3);
    fn_proto = JS_GetPropertyUint32(c, arr, 4);
    ext_map = JS_GetPropertyUint32(c, arr, 5);
    JS_FreeValue(c, arr);
    JS_SetPropertyStr(c, ns, "CompileError", JS_DupValue(c, err_compile));
    JS_SetPropertyStr(c, ns, "LinkError", JS_DupValue(c, err_link));
    JS_SetPropertyStr(c, ns, "RuntimeError", JS_DupValue(c, err_runtime));

    add_class(c, &cls_module, "Module", module_finalizer, NULL, module_ctor, 1, NULL, 0);
    add_class(c, &cls_instance, "Instance", instance_finalizer, NULL, instance_ctor, 1, NULL, 0);
    add_class(c, &cls_memory, "Memory", memory_finalizer, NULL, memory_ctor, 1, memory_proto, sizeof memory_proto / sizeof *memory_proto);
    add_class(c, &cls_table, "Table", table_finalizer, NULL, table_ctor, 1, table_proto, sizeof table_proto / sizeof *table_proto);
    add_class(c, &cls_global, "Global", global_finalizer, NULL, global_ctor, 1, global_proto, sizeof global_proto / sizeof *global_proto);
    add_class(c, &cls_func, "Function", fn_finalizer, fn_call, NULL, 0, NULL, 0);
    JS_SetPropertyStr(c, global, "WebAssembly", JS_DupValue(c, ns));
}

/* Before freeing the context: cuts the references the interpreter holds to JS (the
 * imported functions and the externrefs), so the collector can free everything. */
void js_wasm_shutdown(JSContext *c) {
    if (!alive) return;
    alive = 0;
    for (HostFn *h = hosts.next; h != &hosts; h = h->next) {
        JS_FreeValue(c, h->fn);
        h->fn = JS_UNDEFINED;
    }
    for (uint32_t i = 0; i < next_ext; i++) JS_FreeValue(c, ext_vals[i]);
    free(ext_vals);
    ext_vals = NULL;
    next_ext = ext_cap = 0;
    if (have_pending) { JS_FreeValue(c, pending); have_pending = 0; }
    JS_FreeValue(c, err_compile);
    JS_FreeValue(c, err_link);
    JS_FreeValue(c, err_runtime);
    JS_FreeValue(c, fn_freeze);
    JS_FreeValue(c, fn_proto);
    JS_FreeValue(c, ext_map);
    JS_FreeValue(c, ns);
    ns = JS_UNDEFINED;
}
