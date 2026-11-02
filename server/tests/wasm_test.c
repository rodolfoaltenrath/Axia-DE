/* wasm_test.c — tests of the WebAssembly interpreter (src/wasm).
 *
 *   build/wasm-test                      modules in tests/wasm/ (C compiled with clang) with their
 *                                        expected results
 *   build/wasm-test spec a.json b.json   official suite: wast2json (wabt) output for the .wast files
 *                                        of WebAssembly/spec (see docs/WASM.md)
 *   build/wasm-test bench x.wasm f n     time of f(n)
 */
#include "wasm/wasm.h"

#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static uint8_t *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = malloc(n > 0 ? (size_t)n + 1 : 1);
    if (b && fread(b, 1, (size_t)n, f) != (size_t)n) { free(b); b = NULL; }
    fclose(f);
    if (b) b[n] = 0;
    *len = (size_t)n;
    return b;
}

/* ---- minimal JSON ---- */

typedef struct J {
    int type;                           /* 'o' object, 'a' array, 's' string, 'n' number, 'l' literal */
    char *str;
    size_t len;                         /* of str (may contain 0 bytes) */
    struct J **kids;
    char **keys;
    int n;
} J;

static const char *jp;

static void jws(void) { while (*jp && isspace((unsigned char)*jp)) jp++; }

static size_t jlen;
static char *jstring(void) {
    jp++;
    size_t cap = 32, n = 0;
    char *s = malloc(cap);
    while (*jp && *jp != '"') {
        char c = *jp++;
        if (c == '\\') {
            c = *jp++;
            if (c == 'n') c = '\n';
            else if (c == 't') c = '\t';
            else if (c == 'u') {
                unsigned v = 0;
                sscanf(jp, "%4x", &v);
                jp += 4;
                /* UTF-8 (no surrogate pairs: the suite's names come escaped) */
                char tmp[4];
                int k = 0;
                if (v < 0x80) tmp[k++] = (char)v;
                else if (v < 0x800) { tmp[k++] = (char)(0xc0 | v >> 6); tmp[k++] = (char)(0x80 | (v & 0x3f)); }
                else { tmp[k++] = (char)(0xe0 | v >> 12); tmp[k++] = (char)(0x80 | ((v >> 6) & 0x3f)); tmp[k++] = (char)(0x80 | (v & 0x3f)); }
                for (int i = 0; i < k; i++) { if (n + 2 >= cap) s = realloc(s, cap *= 2); s[n++] = tmp[i]; }
                continue;
            }
        }
        if (n + 2 >= cap) s = realloc(s, cap *= 2);
        s[n++] = c;
    }
    if (*jp) jp++;
    s[n] = 0;
    jlen = n;
    return s;
}

static J *jvalue(void) {
    jws();
    J *j = calloc(1, sizeof *j);
    if (*jp == '{' || *jp == '[') {
        int obj = *jp == '{';
        j->type = obj ? 'o' : 'a';
        jp++;
        for (;;) {
            jws();
            if (*jp == '}' || *jp == ']') { jp++; break; }
            char *key = NULL;
            if (obj) { key = jstring(); jws(); jp++; }
            J *v = jvalue();
            j->kids = realloc(j->kids, (size_t)(j->n + 1) * sizeof *j->kids);
            j->keys = realloc(j->keys, (size_t)(j->n + 1) * sizeof *j->keys);
            j->kids[j->n] = v;
            j->keys[j->n++] = key;
            jws();
            if (*jp == ',') jp++;
        }
    } else if (*jp == '"') {
        j->type = 's';
        j->str = jstring();
        j->len = jlen;
    } else {
        const char *s = jp;
        while (*jp && !strchr(",}] \n\t\r", *jp)) jp++;
        j->type = 'n';
        j->str = strndup(s, (size_t)(jp - s));
    }
    return j;
}

static J *jget(const J *o, const char *key) {
    if (!o || o->type != 'o') return NULL;
    for (int i = 0; i < o->n; i++) if (!strcmp(o->keys[i], key)) return o->kids[i];
    return NULL;
}

static const char *jstr(const J *o, const char *key) {
    J *v = jget(o, key);
    return v && v->str ? v->str : NULL;
}

/* ---- spectest and registered modules ---- */

typedef struct { char *name; WasmInstance *inst; } Reg;
static Reg regs[256];
static int nregs;
static WasmInstance *current;
static Reg named[512];
static int nnamed;

static WasmFunc *spec_funcs[8];
static WasmGlobal *spec_globals[4];
static WasmTable *spec_table;
static WasmMemory *spec_memory;

static int host_print(void *ud, const WasmVal *a, WasmVal *r, WasmError *e) { (void)ud; (void)a; (void)r; (void)e; return 0; }

static void spectest_init(void) {
    static const uint8_t i32[] = { WASM_I32 }, i64[] = { WASM_I64 }, f32[] = { WASM_F32 }, f64[] = { WASM_F64 },
                         i32f32[] = { WASM_I32, WASM_F32 }, f64f64[] = { WASM_F64, WASM_F64 };
    spec_funcs[0] = wasm_func_new_host(wasm_functype(NULL, 0, NULL, 0), host_print, NULL, NULL);
    spec_funcs[1] = wasm_func_new_host(wasm_functype(i32, 1, NULL, 0), host_print, NULL, NULL);
    spec_funcs[2] = wasm_func_new_host(wasm_functype(i64, 1, NULL, 0), host_print, NULL, NULL);
    spec_funcs[3] = wasm_func_new_host(wasm_functype(f32, 1, NULL, 0), host_print, NULL, NULL);
    spec_funcs[4] = wasm_func_new_host(wasm_functype(f64, 1, NULL, 0), host_print, NULL, NULL);
    spec_funcs[5] = wasm_func_new_host(wasm_functype(i32f32, 2, NULL, 0), host_print, NULL, NULL);
    spec_funcs[6] = wasm_func_new_host(wasm_functype(f64f64, 2, NULL, 0), host_print, NULL, NULL);
    WasmVal v = { .u64 = 0 };
    v.i32 = 666; spec_globals[0] = wasm_global_new(WASM_I32, 0, v);
    v.i64 = 666; spec_globals[1] = wasm_global_new(WASM_I64, 0, v);
    v.u64 = 0; v.f32 = 666.6f; spec_globals[2] = wasm_global_new(WASM_F32, 0, v);
    v.f64 = 666.6; spec_globals[3] = wasm_global_new(WASM_F64, 0, v);
    WasmError err;
    spec_table = wasm_table_new(WASM_FUNCREF, 10, 20, (WasmVal){ .u64 = 0 }, &err);
    spec_memory = wasm_memory_new(1, 2, &err);
}

static WasmExtern spectest_lookup(const char *name) {
    static const char *const fnames[] = { "print", "print_i32", "print_i64", "print_f32", "print_f64", "print_i32_f32", "print_f64_f64" };
    for (int i = 0; i < 7; i++) if (!strcmp(name, fnames[i])) return (WasmExtern){ .kind = WASM_EXT_FUNC, .func = spec_funcs[i] };
    static const char *const gnames[] = { "global_i32", "global_i64", "global_f32", "global_f64" };
    for (int i = 0; i < 4; i++) if (!strcmp(name, gnames[i])) return (WasmExtern){ .kind = WASM_EXT_GLOBAL, .global = spec_globals[i] };
    if (!strcmp(name, "table")) return (WasmExtern){ .kind = WASM_EXT_TABLE, .table = spec_table };
    if (!strcmp(name, "memory")) return (WasmExtern){ .kind = WASM_EXT_MEMORY, .memory = spec_memory };
    return (WasmExtern){ .kind = -1 };
}

static WasmInstance *find_inst(const char *name) {
    if (!name) return current;
    for (int i = nnamed - 1; i >= 0; i--) if (!strcmp(named[i].name, name)) return named[i].inst;
    return NULL;
}

static WasmInstance *load(const char *dir, const char *file, WasmError *err, int *phase) {
    char path[1024];
    snprintf(path, sizeof path, "%s/%s", dir, file);
    size_t len;
    uint8_t *b = read_file(path, &len);
    *phase = 0;
    if (!b) { snprintf(err->msg, sizeof err->msg, "could not read %.120s", path); err->kind = WASM_ERR_MALFORMED; return NULL; }
    WasmModule *m = wasm_module_new(b, len, err);
    free(b);
    if (!m) return NULL;
    *phase = 1;
    uint32_t n = wasm_module_num_imports(m);
    WasmExtern *im = calloc(n + 1, sizeof *im);
    for (uint32_t i = 0; i < n; i++) {
        size_t nl;
        const char *mod = wasm_import_module(m, i, NULL), *nm = wasm_import_name(m, i, &nl);
        im[i].kind = -1;
        if (!strcmp(mod, "spectest")) im[i] = spectest_lookup(nm);
        else for (int r = nregs - 1; r >= 0; r--) if (!strcmp(regs[r].name, mod)) { im[i] = wasm_instance_export_find_len(regs[r].inst, nm, nl); break; }
        if (im[i].kind < 0) im[i].ptr = NULL;
        if (im[i].kind < 0) im[i].kind = wasm_import_type(m, i).kind == 0 ? 1 : 0;   /* does not exist: a type that does not match */
    }
    *phase = 2;
    WasmInstance *inst = wasm_instantiate(m, im, n, err);
    free(im);
    wasm_module_unref(m);
    return inst;
}

static int parse_val(const J *v, WasmVal *out, uint8_t *type) {
    const char *t = jstr(v, "type"), *s = jstr(v, "value");
    out->u64 = 0;
    if (!t) return -1;
    if (!strcmp(t, "i32")) { *type = WASM_I32; out->u32 = (uint32_t)strtoull(s, NULL, 10); }
    else if (!strcmp(t, "i64")) { *type = WASM_I64; out->u64 = strtoull(s, NULL, 10); }
    else if (!strcmp(t, "f32")) { *type = WASM_F32; if (s && strncmp(s, "nan", 3)) out->u32 = (uint32_t)strtoull(s, NULL, 10); }
    else if (!strcmp(t, "f64")) { *type = WASM_F64; if (s && strncmp(s, "nan", 3)) out->u64 = strtoull(s, NULL, 10); }
    else if (!strcmp(t, "externref")) { *type = WASM_EXTERNREF; out->u64 = !s || !strcmp(s, "null") ? 0 : strtoull(s, NULL, 10) + 1; }
    else if (!strcmp(t, "funcref")) { *type = WASM_FUNCREF; out->u64 = 0; }
    else return -1;                      /* v128: no soportado */
    return 0;
}

static int check_val(const J *v, WasmVal got) {
    const char *t = jstr(v, "type"), *s = jstr(v, "value");
    if (!strcmp(t, "f32") && s && !strncmp(s, "nan", 3)) {
        uint32_t u = got.u32;
        if ((u & 0x7f800000u) != 0x7f800000u || !(u & 0x7fffffu)) return 0;
        if (!strcmp(s, "nan:canonical")) return (u & 0x7fffffu) == 0x400000u;
        return (u & 0x400000u) != 0;
    }
    if (!strcmp(t, "f64") && s && !strncmp(s, "nan", 3)) {
        uint64_t u = got.u64;
        if ((u & 0x7ff0000000000000ull) != 0x7ff0000000000000ull || !(u & 0xfffffffffffffull)) return 0;
        if (!strcmp(s, "nan:canonical")) return (u & 0xfffffffffffffull) == 0x8000000000000ull;
        return (u & 0x8000000000000ull) != 0;
    }
    if (!strcmp(t, "funcref")) return s && !strcmp(s, "null") ? got.ref == NULL : got.ref != NULL;
    WasmVal want;
    uint8_t ty;
    if (parse_val(v, &want, &ty)) return 0;
    switch (ty) {
    case WASM_I32: case WASM_F32: return want.u32 == got.u32;
    default: return want.u64 == got.u64;
    }
}

/* Runs the action; returns 0, or -1 with err. */
static int action(const J *a, WasmVal *res, uint32_t *nres, WasmError *err) {
    const char *type = jstr(a, "type"), *field = jstr(a, "field"), *mod = jstr(a, "module");
    WasmInstance *inst = find_inst(mod);
    *nres = 0;
    if (!inst) { snprintf(err->msg, sizeof err->msg, "no module"); err->kind = WASM_ERR_LINK; return -1; }
    J *fj = jget(a, "field");
    WasmExtern e = wasm_instance_export_find_len(inst, field, fj ? fj->len : 0);
    if (!strcmp(type, "get")) {
        if (e.kind != WASM_EXT_GLOBAL) { snprintf(err->msg, sizeof err->msg, "no global %s", field); return -1; }
        res[0] = wasm_global_get(e.global);
        *nres = 1;
        return 0;
    }
    if (e.kind != WASM_EXT_FUNC) { snprintf(err->msg, sizeof err->msg, "no function %s", field); err->kind = WASM_ERR_LINK; return -1; }
    J *args = jget(a, "args");
    WasmVal av[64];
    uint8_t ty;
    int n = args ? args->n : 0;
    for (int i = 0; i < n && i < 64; i++) if (parse_val(args->kids[i], &av[i], &ty)) { snprintf(err->msg, sizeof err->msg, "skip"); err->kind = WASM_OK; return -2; }
    const WasmFuncType *ft = wasm_func_type(e.func);
    *nres = ft->nresults;
    return wasm_call(e.func, av, res, err);
}

typedef struct { int pass, fail, skip; } Score;

static void run_spec(const char *json, Score *total) {
    size_t len;
    char *text = (char *)read_file(json, &len);
    if (!text) { fprintf(stderr, "could not read %s\n", json); return; }
    char dir[1024];
    snprintf(dir, sizeof dir, "%s", json);
    char *slash = strrchr(dir, '/');
    if (slash) *slash = 0; else strcpy(dir, ".");
    jp = text;
    J *root = jvalue();
    J *cmds = jget(root, "commands");
    Score sc = { 0, 0, 0 };
    spectest_init();                     /* fresh for each file, like the reference interpreter */
    current = NULL;
    nregs = nnamed = 0;
    for (int i = 0; cmds && i < cmds->n; i++) {
        J *c = cmds->kids[i];
        const char *type = jstr(c, "type"), *line = jstr(c, "line");
        WasmError err = { WASM_OK, "" };
        int phase;
        WasmVal res[64];
        uint32_t nres;
        int ok = -1;                     /* 1 passed, 0 failed, -1 does not count */
        if (!strcmp(type, "module")) {
            current = load(dir, jstr(c, "filename"), &err, &phase);
            if (!current) { ok = 0; }
            else {
                const char *nm = jstr(c, "name");
                if (nm && nnamed < 512) named[nnamed++] = (Reg){ strdup(nm), current };
            }
        } else if (!strcmp(type, "register")) {
            WasmInstance *in = find_inst(jstr(c, "name"));
            if (in && nregs < 256) regs[nregs++] = (Reg){ strdup(jstr(c, "as")), in };
        } else if (!strcmp(type, "action")) {
            int r = action(jget(c, "action"), res, &nres, &err);
            if (r == -2) sc.skip++;
            else ok = r == 0;
        } else if (!strcmp(type, "assert_return")) {
            int r = action(jget(c, "action"), res, &nres, &err);
            if (r == -2) sc.skip++;
            else if (r) ok = 0;
            else {
                J *exp = jget(c, "expected");
                ok = 1;
                for (int k = 0; exp && k < exp->n; k++) {
                    const char *et = jstr(exp->kids[k], "type");
                    if (et && !strcmp(et, "v128")) { ok = -1; sc.skip++; break; }
                    if ((uint32_t)k >= nres || !check_val(exp->kids[k], res[k])) ok = 0;
                }
            }
        } else if (!strcmp(type, "assert_trap") || !strcmp(type, "assert_exhaustion")) {
            J *a = jget(c, "action");
            if (a) {
                int r = action(a, res, &nres, &err);
                if (r == -2) sc.skip++;
                else {
                    ok = r != 0 && (err.kind == WASM_ERR_TRAP || err.kind == WASM_ERR_EXHAUSTED);
                    if (ok && strcmp(err.msg, jstr(c, "text")) && strncmp(jstr(c, "text"), err.msg, strlen(err.msg)))
                        fprintf(stderr, "  %s:%s trampa distinta: «%s» (esperada «%s»)\n", json, line, err.msg, jstr(c, "text"));
                }
            } else {
                WasmInstance *in = load(dir, jstr(c, "filename"), &err, &phase);
                ok = !in && err.kind == WASM_ERR_TRAP;
                if (in) wasm_instance_unref(in);
            }
        } else if (!strcmp(type, "assert_malformed") || !strcmp(type, "assert_invalid")) {
            const char *mt = jstr(c, "module_type");
            if (mt && strcmp(mt, "binary")) { /* text: wabt tests it, not us */ }
            else {
                WasmInstance *in = load(dir, jstr(c, "filename"), &err, &phase);
                int want = !strcmp(type, "assert_malformed") ? WASM_ERR_MALFORMED : WASM_ERR_INVALID;
                ok = !in && phase == 0;
                if (ok && (int)err.kind != want)
                    fprintf(stderr, "  %s:%s %s but got %s: %s\n", json, line, type, err.kind == WASM_ERR_MALFORMED ? "malformed" : "invalid", err.msg);
                if (in) wasm_instance_unref(in);
            }
        } else if (!strcmp(type, "assert_unlinkable") || !strcmp(type, "assert_uninstantiable")) {
            WasmInstance *in = load(dir, jstr(c, "filename"), &err, &phase);
            if (!strcmp(type, "assert_unlinkable")) ok = !in && phase == 2 && err.kind == WASM_ERR_LINK;
            else ok = !in && phase == 2 && err.kind == WASM_ERR_TRAP;
            if (in) wasm_instance_unref(in);
        } else {
            sc.skip++;
        }
        if (ok == 1) sc.pass++;
        else if (ok == 0) {
            sc.fail++;
            fprintf(stderr, "  FAIL %s:%s %s %s\n", json, line, type, err.msg);
        }
    }
    printf("%-40s %5d pass %4d fail %4d skipped\n", strrchr(json, '/') ? strrchr(json, '/') + 1 : json, sc.pass, sc.fail, sc.skip);
    total->pass += sc.pass;
    total->fail += sc.fail;
    total->skip += sc.skip;
    free(text);
}

/* ---- tests/wasm modules ---- */

static WasmInstance *load_simple(const char *path, WasmExtern *imports, uint32_t n) {
    size_t len;
    uint8_t *b = read_file(path, &len);
    if (!b) { printf("FAIL: could not read %s\n", path); return NULL; }
    WasmError err;
    WasmModule *m = wasm_module_new(b, len, &err);
    free(b);
    if (!m) { printf("FAIL %s: %s\n", path, err.msg); return NULL; }
    WasmInstance *i = wasm_instantiate(m, imports, n, &err);
    wasm_module_unref(m);
    if (!i) printf("FAIL %s: %s\n", path, err.msg);
    return i;
}

static int fails;
static void expect_i64(const char *what, int64_t got, int64_t want) {
    if (got != want) { printf("FAIL %s: %lld (expected %lld)\n", what, (long long)got, (long long)want); fails++; }
    else printf("ok   %s = %lld\n", what, (long long)got);
}

static int64_t call1(WasmInstance *inst, const char *name, int64_t arg, int is64) {
    WasmExtern e = wasm_instance_export_find(inst, name);
    if (e.kind != WASM_EXT_FUNC) { printf("FAIL: no %s\n", name); fails++; return 0; }
    WasmVal a = { .u64 = 0 }, r = { .u64 = 0 };
    if (is64) a.i64 = arg; else a.i32 = (int32_t)arg;
    WasmError err;
    if (wasm_call(e.func, &a, &r, &err)) { printf("FAIL %s: %s\n", name, err.msg); fails++; return 0; }
    const WasmFuncType *t = wasm_func_type(e.func);
    return t->nresults && t->results[0] == WASM_I64 ? r.i64 : r.i32;
}

static char log_buf[256];
static int host_log(void *ud, const WasmVal *a, WasmVal *r, WasmError *e) {
    (void)r; (void)e;
    WasmInstance **inst = ud;
    WasmExtern mem = wasm_instance_export_find(*inst, "memory");
    uint32_t p = a[0].u32, n = a[1].u32;
    if (mem.kind != WASM_EXT_MEMORY || (uint64_t)p + n > wasm_memory_size(mem.memory) || n >= sizeof log_buf) {
        wasm_err(e, WASM_ERR_TRAP, "log outside memory");
        return -1;
    }
    memcpy(log_buf, wasm_memory_data(mem.memory) + p, n);
    log_buf[n] = 0;
    return 0;
}

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static int builtin_tests(const char *dir) {
    char path[1024];
    snprintf(path, sizeof path, "%s/demo.wasm", dir);
    static WasmInstance *demo;
    static const uint8_t ii[] = { WASM_I32, WASM_I32 };
    WasmFunc *logf = wasm_func_new_host(wasm_functype(ii, 2, NULL, 0), host_log, &demo, NULL);
    WasmExtern im[1] = { { .kind = WASM_EXT_FUNC, .func = logf } };
    demo = load_simple(path, im, 1);
    if (!demo) return 1;
    expect_i64("fib(20)", call1(demo, "fib", 20, 0), 6765);
    expect_i64("fact64(20)", call1(demo, "fact64", 20, 1), 2432902008176640000ll);
    expect_i64("primes(10000)", call1(demo, "primes", 10000, 0), 1229);
    expect_i64("sort_check(1000)", call1(demo, "sort_check", 1000, 0), 1);
    expect_i64("crc32 of 'LordOS'", call1(demo, "crc_demo", 0, 0), (int32_t)0xd951d347u);
    expect_i64("dispatch(3) (call_indirect)", call1(demo, "dispatch", 3, 0), 9);
    expect_i64("fdemo(10) (float)", call1(demo, "fdemo", 10, 0), 3628800);
    expect_i64("greet (host import)", call1(demo, "greet", 0, 0), 13);
    if (strcmp(log_buf, "hola desde C!")) { printf("FAIL greet: «%s»\n", log_buf); fails++; }
    else printf("ok   host log: «%s»\n", log_buf);
    expect_i64("grow (memory.grow)", call1(demo, "grow_test", 3, 0), 1);
    expect_i64("bulk (memory.copy/fill)", call1(demo, "bulk_test", 0, 0), 1);
    /* trap: division by zero */
    WasmExtern e = wasm_instance_export_find(demo, "divide");
    WasmVal a[2] = { { .i32 = 7 }, { .i32 = 0 } }, r;
    WasmError err;
    if (wasm_call(e.func, a, &r, &err) == 0 || err.kind != WASM_ERR_TRAP) { printf("FAIL divide: no trap\n"); fails++; }
    else printf("ok   7/0 traps: %s\n", err.msg);
    /* infinite recursion: stack exhausted, without crashing */
    e = wasm_instance_export_find(demo, "recurse");
    a[0].i32 = 1;
    if (wasm_call(e.func, a, &r, &err) == 0 || err.kind != WASM_ERR_EXHAUSTED) { printf("FAIL recurse: %s\n", err.msg); fails++; }
    else printf("ok   infinite recursion: %s\n", err.msg);
    expect_i64("fib(20) after the trap", call1(demo, "fib", 20, 0), 6765);
    /* timings */
    double t0 = now();
    int64_t v = call1(demo, "fib", 30, 0);
    double t1 = now();
    printf("time fib(30) = %lld: %.0f ms\n", (long long)v, (t1 - t0) * 1e3);
    t0 = now();
    v = call1(demo, "primes", 2000000, 0);
    t1 = now();
    printf("time primes(2000000) = %lld: %.0f ms\n", (long long)v, (t1 - t0) * 1e3);
    wasm_instance_unref(demo);
    wasm_func_unref(logf);
    printf(fails ? "%d FAILURES\n" : "all good\n", fails);
    return fails != 0;
}

int main(int argc, char **argv) {
    if (argc > 1 && !strcmp(argv[1], "spec")) {
        Score total = { 0, 0, 0 };
        for (int i = 2; i < argc; i++) run_spec(argv[i], &total);
        printf("TOTAL: %d pass, %d fail, %d skipped\n", total.pass, total.fail, total.skip);
        return total.fail != 0;
    }
    if (argc > 4 && !strcmp(argv[1], "bench")) {
        WasmInstance *i = load_simple(argv[2], NULL, 0);
        if (!i) return 1;
        double t0 = now();
        int64_t v = call1(i, argv[3], atoll(argv[4]), 0);
        printf("%s(%s) = %lld in %.1f ms\n", argv[3], argv[4], (long long)v, (now() - t0) * 1e3);
        return 0;
    }
    return builtin_tests(argc > 1 ? argv[1] : "tests/wasm");
}
