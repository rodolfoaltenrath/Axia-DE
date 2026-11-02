/* wasm_run.c — instances, memories, tables, globals and the interpreter (see wasm.h).
 *
 * The interpreter is a "computed goto" loop (GCC/clang): each compiled instruction carries
 * the address of its handler. A single value stack (64 bits per value) and a frame
 * stack, reserved with mmap: real memory is used as it gets touched. Calls
 * wasm → wasm don't recurse in C; host (JS) calls can re-enter. */
#include "wasm_int.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

uint32_t wasm_max_call_depth = 20000;
uint32_t wasm_max_memory_pages = 65536;

#define VSTACK_SLOTS (1u << 20)         /* 8 MiB of values */

const void *const *wasm_op_labels;

/* ---- references ---- */

static void instance_free(WasmInstance *inst);

void wasm_instance_ref(WasmInstance *i) { i->rc++; }
void wasm_instance_unref(WasmInstance *i) { if (i && --i->rc == 0) instance_free(i); }
WasmModule *wasm_instance_module(const WasmInstance *i) { return i->m; }

void wasm_func_ref(WasmFunc *f) {
    if (f->inst) wasm_instance_ref(f->inst);
    else f->rc++;
}

void wasm_func_unref(WasmFunc *f) {
    if (!f) return;
    if (f->inst) { wasm_instance_unref(f->inst); return; }
    if (--f->rc == 0) {
        if (f->free_ud) f->free_ud(f->ud);
        free(f);
    }
}

void wasm_ref_hold(WasmInstance *owner, uint8_t type, WasmVal v) {
    if (type != WASM_FUNCREF || !v.ref) return;
    WasmFunc *f = v.ref;
    if (f->inst != owner || !owner) wasm_func_ref(f);
}

void wasm_ref_release(WasmInstance *owner, uint8_t type, WasmVal v) {
    if (type != WASM_FUNCREF || !v.ref) return;
    WasmFunc *f = v.ref;
    if (f->inst != owner || !owner) wasm_func_unref(f);
}

WasmFunc *wasm_func_new_host(const WasmFuncType *type, WasmHostFn fn, void *ud, void (*free_ud)(void *)) {
    WasmFunc *f = calloc(1, sizeof *f);
    if (!f) return NULL;
    f->type = type;
    f->host = fn;
    f->ud = ud;
    f->free_ud = free_ud;
    f->rc = 1;
    return f;
}

const WasmFuncType *wasm_func_type(const WasmFunc *f) { return f->type; }
int wasm_func_is_host(const WasmFunc *f) { return f->host != NULL; }
void *wasm_func_host_data(const WasmFunc *f) { return f->ud; }
void **wasm_func_slot(WasmFunc *f) { return &f->slot; }

/* ---- memories ---- */

WasmMemory *wasm_memory_new(uint32_t min, uint32_t max, WasmError *err) {
    uint32_t cap = wasm_max_memory_pages;
    if (min > cap) { wasm_err(err, WASM_ERR_NOMEM, "memory too large (%u pages)", min); return NULL; }
    uint32_t top = max == WASM_NO_MAX || max > cap ? cap : max;
    WasmMemory *m = calloc(1, sizeof *m);
    if (!m) { wasm_err(err, WASM_ERR_NOMEM, "out of memory"); return NULL; }
    m->rc = 1;
    m->max = max;
    m->reserved = (size_t)(top ? top : 1) * WASM_PAGE;
    void *p = mmap(NULL, m->reserved, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (p == MAP_FAILED) { free(m); wasm_err(err, WASM_ERR_NOMEM, "could not reserve the memory"); return NULL; }
    m->data = p;
    m->size = (size_t)min * WASM_PAGE;
    if (m->size && mprotect(p, m->size, PROT_READ | PROT_WRITE)) {
        munmap(p, m->reserved);
        free(m);
        wasm_err(err, WASM_ERR_NOMEM, "out of memory");
        return NULL;
    }
    return m;
}

void wasm_memory_ref(WasmMemory *m) { m->rc++; }
void wasm_memory_unref(WasmMemory *m) {
    if (m && --m->rc == 0) { munmap(m->data, m->reserved); free(m); }
}
uint8_t *wasm_memory_data(const WasmMemory *m) { return m->data; }
size_t wasm_memory_size(const WasmMemory *m) { return m->size; }
uint32_t wasm_memory_max(const WasmMemory *m) { return m->max; }

void wasm_memory_on_grow(WasmMemory *m, void (*fn)(void *, WasmMemory *), void *ud) {
    m->on_grow = fn;
    m->on_grow_ud = ud;
}

void **wasm_memory_slots(WasmMemory *m) { return m->slot; }
void **wasm_table_slot(WasmTable *t) { return &t->slot; }
void **wasm_global_slot(WasmGlobal *g) { return &g->slot; }

int64_t wasm_memory_grow(WasmMemory *m, uint32_t delta) {
    uint64_t old = m->size / WASM_PAGE, n = old + delta;
    uint64_t limit = m->max == WASM_NO_MAX ? 65536 : m->max;
    if (n > limit || n * WASM_PAGE > m->reserved) return -1;
    if (delta) {
        if (mprotect(m->data + m->size, (size_t)delta * WASM_PAGE, PROT_READ | PROT_WRITE)) return -1;
        m->size = (size_t)n * WASM_PAGE;
    }
    if (m->on_grow) m->on_grow(m->on_grow_ud, m);
    return (int64_t)old;
}

/* ---- tables ---- */

static WasmTable *table_alloc(WasmInstance *owner, uint8_t type, uint32_t min, uint32_t max, WasmVal init, WasmError *err) {
    if (min > 10000000) { wasm_err(err, WASM_ERR_NOMEM, "table too large"); return NULL; }
    WasmTable *t = calloc(1, sizeof *t);
    if (t) t->elems = calloc(min ? min : 1, sizeof *t->elems);
    if (!t || !t->elems) { free(t); wasm_err(err, WASM_ERR_NOMEM, "out of memory"); return NULL; }
    t->owner = owner;
    t->type = type;
    t->size = min;
    t->max = max;
    for (uint32_t i = 0; i < min; i++) { t->elems[i] = init; wasm_ref_hold(owner, type, init); }
    return t;
}

static void table_destroy(WasmTable *t) {
    for (uint32_t i = 0; i < t->size; i++) wasm_ref_release(t->owner, t->type, t->elems[i]);
    free(t->elems);
    free(t);
}

WasmTable *wasm_table_new(uint8_t type, uint32_t min, uint32_t max, WasmVal init, WasmError *err) {
    WasmTable *t = table_alloc(NULL, type, min, max, init, err);
    if (t) t->rc = 1;
    return t;
}

void wasm_table_ref(WasmTable *t) {
    if (t->rc++ == 0 && t->owner) wasm_instance_ref(t->owner);
}

void wasm_table_unref(WasmTable *t) {
    if (!t || --t->rc) return;
    if (t->owner) wasm_instance_unref(t->owner);
    else table_destroy(t);
}

uint8_t wasm_table_elemtype(const WasmTable *t) { return t->type; }
uint32_t wasm_table_size(const WasmTable *t) { return t->size; }
uint32_t wasm_table_max(const WasmTable *t) { return t->max; }

int wasm_table_get(const WasmTable *t, uint32_t i, WasmVal *v) {
    if (i >= t->size) return -1;
    *v = t->elems[i];
    return 0;
}

int wasm_table_set(WasmTable *t, uint32_t i, WasmVal v) {
    if (i >= t->size) return -1;
    wasm_ref_hold(t->owner, t->type, v);
    WasmVal old = t->elems[i];
    t->elems[i] = v;
    wasm_ref_release(t->owner, t->type, old);
    return 0;
}

int64_t wasm_table_grow(WasmTable *t, uint32_t delta, WasmVal init) {
    uint64_t n = (uint64_t)t->size + delta;
    uint64_t limit = t->max == WASM_NO_MAX ? 10000000 : t->max;
    if (n > limit || n > 10000000) return -1;
    if (delta) {
        WasmVal *e = realloc(t->elems, (size_t)n * sizeof *e);
        if (!e) return -1;
        t->elems = e;
        for (uint64_t i = t->size; i < n; i++) { e[i] = init; wasm_ref_hold(t->owner, t->type, init); }
    }
    uint32_t old = t->size;
    t->size = (uint32_t)n;
    return old;
}

/* ---- globals ---- */

static WasmGlobal *global_alloc(WasmInstance *owner, uint8_t type, int mut, WasmVal v) {
    WasmGlobal *g = calloc(1, sizeof *g);
    if (!g) return NULL;
    g->owner = owner;
    g->type = type;
    g->mut = (uint8_t)mut;
    g->val = v;
    wasm_ref_hold(owner, type, v);
    return g;
}

WasmGlobal *wasm_global_new(uint8_t type, int mut, WasmVal v) {
    WasmGlobal *g = global_alloc(NULL, type, mut, v);
    if (g) g->rc = 1;
    return g;
}

static void global_destroy(WasmGlobal *g) {
    wasm_ref_release(g->owner, g->type, g->val);
    free(g);
}

void wasm_global_ref(WasmGlobal *g) {
    if (g->rc++ == 0 && g->owner) wasm_instance_ref(g->owner);
}

void wasm_global_unref(WasmGlobal *g) {
    if (!g || --g->rc) return;
    if (g->owner) wasm_instance_unref(g->owner);
    else global_destroy(g);
}

uint8_t wasm_global_type(const WasmGlobal *g) { return g->type; }
int wasm_global_mut(const WasmGlobal *g) { return g->mut; }
WasmVal wasm_global_get(const WasmGlobal *g) { return g->val; }

void wasm_global_set(WasmGlobal *g, WasmVal v) {
    wasm_ref_hold(g->owner, g->type, v);
    WasmVal old = g->val;
    g->val = v;
    wasm_ref_release(g->owner, g->type, old);
}

void wasm_extern_ref(WasmExtern e) {
    switch (e.kind) {
    case WASM_EXT_FUNC: wasm_func_ref(e.func); break;
    case WASM_EXT_TABLE: wasm_table_ref(e.table); break;
    case WASM_EXT_MEMORY: wasm_memory_ref(e.memory); break;
    case WASM_EXT_GLOBAL: wasm_global_ref(e.global); break;
    }
}

void wasm_extern_unref(WasmExtern e) {
    switch (e.kind) {
    case WASM_EXT_FUNC: wasm_func_unref(e.func); break;
    case WASM_EXT_TABLE: wasm_table_unref(e.table); break;
    case WASM_EXT_MEMORY: wasm_memory_unref(e.memory); break;
    case WASM_EXT_GLOBAL: wasm_global_unref(e.global); break;
    }
}

/* ---- constant expressions ---- */

static uint64_t rd_leb(const uint8_t **pp, int sign) {
    const uint8_t *p = *pp;
    uint64_t r = 0;
    unsigned shift = 0;
    uint8_t b;
    do {
        b = *p++;
        if (shift < 64) r |= (uint64_t)(b & 0x7f) << shift;
        shift += 7;
    } while (b & 0x80);
    if (sign && shift < 64 && (b & 0x40)) r |= ~0ull << shift;
    *pp = p;
    return r;
}

WasmVal wasm_const_eval(const WasmInstance *inst, ConstExpr e) {
    const uint8_t *p = e.p;
    WasmVal v = { .u64 = 0 };
    for (;;) {
        uint8_t op = *p++;
        switch (op) {
        case 0x0b: return v;
        case 0x41: v.u64 = 0; v.i32 = (int32_t)rd_leb(&p, 1); break;
        case 0x42: v.u64 = rd_leb(&p, 1); break;
        case 0x43: v.u64 = 0; memcpy(&v.u32, p, 4); p += 4; break;
        case 0x44: memcpy(&v.u64, p, 8); p += 8; break;
        case 0xd0: p++; v.u64 = 0; break;
        case 0xd2: v.u64 = 0; v.ref = inst->funcs[rd_leb(&p, 0)]; break;
        case 0x23: v = inst->globals[rd_leb(&p, 0)]->val; break;
        default: return v;
        }
    }
}

/* ---- execution stacks ---- */

typedef struct {
    const Cell *ip;                     /* NULL: return to the host */
    WasmVal *fp;
    WasmInstance *inst;
    uint8_t crossed;                    /* entered another instance (it has to be released) */
} Frame;

static WasmVal *vstack, *vstack_end, *cur_sp;
static Frame *frames;
static uint32_t nframes, frames_cap;

static int stacks_init(void) {
    if (vstack) return 0;
    size_t vs = (size_t)VSTACK_SLOTS * sizeof(WasmVal);
    void *p = mmap(NULL, vs, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (p == MAP_FAILED) return -1;
    frames_cap = wasm_max_call_depth + 16;
    void *f = mmap(NULL, (size_t)frames_cap * sizeof(Frame), PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (f == MAP_FAILED) { munmap(p, vs); return -1; }
    vstack = cur_sp = p;
    vstack_end = vstack + VSTACK_SLOTS;
    frames = f;
    return 0;
}

/* ---- arithmetic ---- */

static inline float fmin32(float a, float b) {
    if (a != a || b != b) return a + b;
    if (a == 0 && b == 0) return signbit(a) ? a : b;
    return a < b ? a : b;
}
static inline float fmax32(float a, float b) {
    if (a != a || b != b) return a + b;
    if (a == 0 && b == 0) return signbit(a) ? b : a;
    return a > b ? a : b;
}
static inline double fmin64(double a, double b) {
    if (a != a || b != b) return a + b;
    if (a == 0 && b == 0) return signbit(a) ? a : b;
    return a < b ? a : b;
}
static inline double fmax64(double a, double b) {
    if (a != a || b != b) return a + b;
    if (a == 0 && b == 0) return signbit(a) ? b : a;
    return a > b ? a : b;
}

/* Saturating conversion (trunc_sat): NaN → 0, out of range → the extreme. */
static inline int32_t sat_i32(double x) {
    if (x != x) return 0;
    if (x <= -2147483649.0) return INT32_MIN;
    if (x >= 2147483648.0) return INT32_MAX;
    return (int32_t)x;
}
static inline uint32_t sat_u32(double x) {
    if (x != x || x <= -1.0) return 0;
    if (x >= 4294967296.0) return UINT32_MAX;
    return (uint32_t)x;
}
static inline int64_t sat_i64(double x) {
    if (x != x) return 0;
    if (x < -9223372036854775808.0) return INT64_MIN;
    if (x >= 9223372036854775808.0) return INT64_MAX;
    return (int64_t)x;
}
static inline uint64_t sat_u64(double x) {
    if (x != x || x <= -1.0) return 0;
    if (x >= 18446744073709551616.0) return UINT64_MAX;
    return (uint64_t)x;
}

/* ---- interpreter ---- */

static int exec(WasmFunc *entry, WasmVal *base, WasmError *err) {
    static const void *const labels[OP__COUNT] = {
#define X(name, byte) [OP_##name] = &&L_##name,
        WASM_OPS(X)
#undef X
    };
    if (!entry) { wasm_op_labels = labels; return 0; }

    const char *trapmsg = NULL;
    WasmErrKind trapkind = WASM_ERR_TRAP;
    uint32_t entry_depth = nframes;
    WasmInstance *inst = entry->inst;
    WasmMemory *mem;
    uint8_t *mb;
    uint64_t msz;
    WasmVal *fp, *sp;
    const Cell *ip, *ret_ip;
    WasmFunc *callee;

#define RELOAD() do { mem = inst->mem; mb = mem ? mem->data : NULL; msz = mem ? mem->size : 0; } while (0)
#define DISPATCH() goto *ip->op
#define NEXT(n) do { ip += (n); DISPATCH(); } while (0)
#define TRAP(msg) do { trapmsg = (msg); goto trap; } while (0)

    if (nframes >= wasm_max_call_depth || base + entry->code->max_stack > vstack_end) {
        wasm_err(err, WASM_ERR_EXHAUSTED, "call stack exhausted");
        return -1;
    }
    frames[nframes++] = (Frame){ NULL, NULL, NULL, 0 };
    fp = base;
    memset(fp + entry->type->nparams, 0, (entry->code->nlocals - entry->type->nparams) * sizeof *fp);
    sp = fp + entry->code->nlocals;
    ip = entry->code->code;
    RELOAD();
    DISPATCH();

L_UNREACHABLE: TRAP("unreachable");
L_BR: ip += ip[1].i; DISPATCH();
L_BR_N: {
    WasmVal *dst = fp + ip[2].u;
    uint64_t a = ip[3].u;
    memmove(dst, sp - a, a * sizeof *sp);
    sp = dst + a;
    ip += ip[1].i;
    DISPATCH();
}
L_BR_IF: if ((--sp)->u32) { ip += ip[1].i; DISPATCH(); } NEXT(2);
L_BR_IF_N:
    if ((--sp)->u32) {
        WasmVal *dst = fp + ip[2].u;
        uint64_t a = ip[3].u;
        memmove(dst, sp - a, a * sizeof *sp);
        sp = dst + a;
        ip += ip[1].i;
        DISPATCH();
    }
    NEXT(4);
L_BR_UNLESS: if (!(--sp)->u32) { ip += ip[1].i; DISPATCH(); } NEXT(2);
L_BR_TABLE: {
    uint64_t n = ip[1].u, a = ip[2].u;
    uint32_t i = (--sp)->u32;
    if (i > n) i = (uint32_t)n;
    const Cell *e = ip + 3 + 2 * (size_t)i;
    WasmVal *dst = fp + e[1].u;
    if (dst != sp - a) { memmove(dst, sp - a, a * sizeof *sp); sp = dst + a; }
    ip += e[0].i;
    DISPATCH();
}
L_RETURN: {
    uint64_t n = ip[1].u;
    memmove(fp, sp - n, n * sizeof *sp);
    sp = fp + n;
    goto do_return;
}
L_RETURN0: sp = fp; goto do_return;
L_RETURN1: fp[0] = sp[-1]; sp = fp + 1; goto do_return;
do_return: {
    Frame *fr = &frames[--nframes];
    if (fr->crossed) wasm_instance_unref(inst);
    if (!fr->ip) { cur_sp = sp; return 0; }
    inst = fr->inst;
    fp = fr->fp;
    ip = fr->ip;
    RELOAD();
    DISPATCH();
}
L_CALL:
    callee = inst->funcs[ip[1].u];
    ret_ip = ip + 2;
    goto do_call;
L_CALL_INDIRECT: {
    WasmTable *t = inst->tables[ip[1].u];
    uint32_t i = (--sp)->u32;
    if (i >= t->size) TRAP("undefined element");
    callee = t->elems[i].ref;
    if (!callee) TRAP("uninitialized element");
    if (callee->type != (const WasmFuncType *)(uintptr_t)ip[2].u) TRAP("indirect call type mismatch");
    ret_ip = ip + 3;
    goto do_call;
}
do_call: {
    const WasmFuncType *ft = callee->type;
    uint32_t np = ft->nparams, nr = ft->nresults;
    if (callee->host) {
        if (sp + nr + 16 > vstack_end) { trapkind = WASM_ERR_EXHAUSTED; TRAP("call stack exhausted"); }
        cur_sp = sp + nr;
        WasmError herr = { WASM_OK, "" };
        int r = callee->host(callee->ud, sp - np, sp, &herr);
        if (r) {
            if (err) *err = herr;
            if (herr.kind == WASM_OK && err) wasm_err(err, WASM_ERR_HOST, "host error");
            goto unwind;
        }
        memmove(sp - np, sp, nr * sizeof *sp);
        sp = sp - np + nr;
        ip = ret_ip;
        RELOAD();
        DISPATCH();
    }
    const WasmCode *c = callee->code;
    if (nframes >= wasm_max_call_depth || sp + c->max_stack > vstack_end) { trapkind = WASM_ERR_EXHAUSTED; TRAP("call stack exhausted"); }
    uint8_t crossed = callee->inst != inst;
    frames[nframes++] = (Frame){ ret_ip, fp, inst, crossed };
    fp = sp - np;
    memset(sp, 0, (c->nlocals - np) * sizeof *sp);
    sp = fp + c->nlocals;
    if (crossed) { inst = callee->inst; wasm_instance_ref(inst); RELOAD(); }
    ip = c->code;
    DISPATCH();
}
L_DROP: sp--; NEXT(1);
L_SELECT: {
    uint32_t c = sp[-1].u32;
    sp -= 2;
    if (!c) sp[-1] = sp[0];
    NEXT(1);
}
L_LOCAL_GET: *sp++ = fp[ip[1].u]; NEXT(2);
L_LOCAL_SET: fp[ip[1].u] = *--sp; NEXT(2);
L_LOCAL_TEE: fp[ip[1].u] = sp[-1]; NEXT(2);
L_GLOBAL_GET: *sp++ = inst->globals[ip[1].u]->val; NEXT(2);
L_GLOBAL_SET: {
    WasmGlobal *g = inst->globals[ip[1].u];
    sp--;
    if (g->type == WASM_FUNCREF) wasm_global_set(g, *sp);
    else g->val = *sp;
    NEXT(2);
}
L_TABLE_GET: {
    WasmTable *t = inst->tables[ip[1].u];
    uint32_t i = sp[-1].u32;
    if (i >= t->size) TRAP("out of bounds table access");
    sp[-1] = t->elems[i];
    NEXT(2);
}
L_TABLE_SET: {
    WasmTable *t = inst->tables[ip[1].u];
    sp -= 2;
    if (wasm_table_set(t, sp[0].u32, sp[1])) TRAP("out of bounds table access");
    NEXT(2);
}

#define EA(n) uint64_t ea = (uint64_t)sp[-1].u32 + ip[1].u; if (ea + (n) > msz) TRAP("out of bounds memory access")
#define LOAD(name, T, field, cast) L_##name: { EA(sizeof(T)); T v; memcpy(&v, mb + ea, sizeof v); sp[-1].u64 = 0; sp[-1].field = (cast)v; NEXT(2); }
LOAD(I32_LOAD, uint32_t, u32, uint32_t)
LOAD(I64_LOAD, uint64_t, u64, uint64_t)
LOAD(F32_LOAD, uint32_t, u32, uint32_t)
LOAD(F64_LOAD, uint64_t, u64, uint64_t)
LOAD(I32_LOAD8_S, int8_t, i32, int32_t)
LOAD(I32_LOAD8_U, uint8_t, u32, uint32_t)
LOAD(I32_LOAD16_S, int16_t, i32, int32_t)
LOAD(I32_LOAD16_U, uint16_t, u32, uint32_t)
LOAD(I64_LOAD8_S, int8_t, i64, int64_t)
LOAD(I64_LOAD8_U, uint8_t, u64, uint64_t)
LOAD(I64_LOAD16_S, int16_t, i64, int64_t)
LOAD(I64_LOAD16_U, uint16_t, u64, uint64_t)
LOAD(I64_LOAD32_S, int32_t, i64, int64_t)
LOAD(I64_LOAD32_U, uint32_t, u64, uint64_t)
#undef LOAD
#define STORE(name, T, field) L_##name: { \
    uint64_t ea = (uint64_t)sp[-2].u32 + ip[1].u; \
    if (ea + sizeof(T) > msz) TRAP("out of bounds memory access"); \
    T v = (T)sp[-1].field; memcpy(mb + ea, &v, sizeof v); sp -= 2; NEXT(2); }
STORE(I32_STORE, uint32_t, u32)
STORE(I64_STORE, uint64_t, u64)
STORE(F32_STORE, uint32_t, u32)
STORE(F64_STORE, uint64_t, u64)
STORE(I32_STORE8, uint8_t, u32)
STORE(I32_STORE16, uint16_t, u32)
STORE(I64_STORE8, uint8_t, u64)
STORE(I64_STORE16, uint16_t, u64)
STORE(I64_STORE32, uint32_t, u64)
#undef STORE
L_MEMORY_SIZE: sp->u64 = 0; sp->u32 = (uint32_t)(msz / WASM_PAGE); sp++; NEXT(1);
L_MEMORY_GROW: {
    int64_t r = wasm_memory_grow(mem, sp[-1].u32);
    sp[-1].i32 = (int32_t)r;
    RELOAD();
    NEXT(1);
}
L_CONST: sp->u64 = ip[1].u; sp++; NEXT(2);

#define CMP(name, field, op) L_##name: sp[-2].u32 = sp[-2].field op sp[-1].field; sp--; NEXT(1);
L_I32_EQZ: sp[-1].u32 = sp[-1].u32 == 0; NEXT(1);
CMP(I32_EQ, u32, ==) CMP(I32_NE, u32, !=) CMP(I32_LT_S, i32, <) CMP(I32_LT_U, u32, <)
CMP(I32_GT_S, i32, >) CMP(I32_GT_U, u32, >) CMP(I32_LE_S, i32, <=) CMP(I32_LE_U, u32, <=)
CMP(I32_GE_S, i32, >=) CMP(I32_GE_U, u32, >=)
L_I64_EQZ: sp[-1].u32 = sp[-1].u64 == 0; NEXT(1);
CMP(I64_EQ, u64, ==) CMP(I64_NE, u64, !=) CMP(I64_LT_S, i64, <) CMP(I64_LT_U, u64, <)
CMP(I64_GT_S, i64, >) CMP(I64_GT_U, u64, >) CMP(I64_LE_S, i64, <=) CMP(I64_LE_U, u64, <=)
CMP(I64_GE_S, i64, >=) CMP(I64_GE_U, u64, >=)
CMP(F32_EQ, f32, ==) CMP(F32_NE, f32, !=) CMP(F32_LT, f32, <) CMP(F32_GT, f32, >) CMP(F32_LE, f32, <=) CMP(F32_GE, f32, >=)
CMP(F64_EQ, f64, ==) CMP(F64_NE, f64, !=) CMP(F64_LT, f64, <) CMP(F64_GT, f64, >) CMP(F64_LE, f64, <=) CMP(F64_GE, f64, >=)
#undef CMP

#define BIN32(name, expr) L_##name: { uint32_t a = sp[-2].u32, b = sp[-1].u32; (void)a; (void)b; sp[-2].u32 = (expr); sp--; NEXT(1); }
#define BIN64(name, expr) L_##name: { uint64_t a = sp[-2].u64, b = sp[-1].u64; (void)a; (void)b; sp[-2].u64 = (expr); sp--; NEXT(1); }
L_I32_CLZ: sp[-1].u32 = sp[-1].u32 ? (uint32_t)__builtin_clz(sp[-1].u32) : 32; NEXT(1);
L_I32_CTZ: sp[-1].u32 = sp[-1].u32 ? (uint32_t)__builtin_ctz(sp[-1].u32) : 32; NEXT(1);
L_I32_POPCNT: sp[-1].u32 = (uint32_t)__builtin_popcount(sp[-1].u32); NEXT(1);
BIN32(I32_ADD, a + b) BIN32(I32_SUB, a - b) BIN32(I32_MUL, a * b)
L_I32_DIV_S: {
    int32_t a = sp[-2].i32, b = sp[-1].i32;
    if (!b) TRAP("integer divide by zero");
    if (a == INT32_MIN && b == -1) TRAP("integer overflow");
    sp[-2].i32 = a / b; sp--; NEXT(1);
}
L_I32_DIV_U: { uint32_t a = sp[-2].u32, b = sp[-1].u32; if (!b) TRAP("integer divide by zero"); sp[-2].u32 = a / b; sp--; NEXT(1); }
L_I32_REM_S: {
    int32_t a = sp[-2].i32, b = sp[-1].i32;
    if (!b) TRAP("integer divide by zero");
    sp[-2].i32 = b == -1 ? 0 : a % b; sp--; NEXT(1);
}
L_I32_REM_U: { uint32_t a = sp[-2].u32, b = sp[-1].u32; if (!b) TRAP("integer divide by zero"); sp[-2].u32 = a % b; sp--; NEXT(1); }
BIN32(I32_AND, a & b) BIN32(I32_OR, a | b) BIN32(I32_XOR, a ^ b) BIN32(I32_SHL, a << (b & 31))
L_I32_SHR_S: sp[-2].i32 = sp[-2].i32 >> (sp[-1].u32 & 31); sp--; NEXT(1);
BIN32(I32_SHR_U, a >> (b & 31))
BIN32(I32_ROTL, (a << (b & 31)) | (a >> ((32 - (b & 31)) & 31)))
BIN32(I32_ROTR, (a >> (b & 31)) | (a << ((32 - (b & 31)) & 31)))
L_I64_CLZ: sp[-1].u64 = sp[-1].u64 ? (uint64_t)__builtin_clzll(sp[-1].u64) : 64; NEXT(1);
L_I64_CTZ: sp[-1].u64 = sp[-1].u64 ? (uint64_t)__builtin_ctzll(sp[-1].u64) : 64; NEXT(1);
L_I64_POPCNT: sp[-1].u64 = (uint64_t)__builtin_popcountll(sp[-1].u64); NEXT(1);
BIN64(I64_ADD, a + b) BIN64(I64_SUB, a - b) BIN64(I64_MUL, a * b)
L_I64_DIV_S: {
    int64_t a = sp[-2].i64, b = sp[-1].i64;
    if (!b) TRAP("integer divide by zero");
    if (a == INT64_MIN && b == -1) TRAP("integer overflow");
    sp[-2].i64 = a / b; sp--; NEXT(1);
}
L_I64_DIV_U: { uint64_t a = sp[-2].u64, b = sp[-1].u64; if (!b) TRAP("integer divide by zero"); sp[-2].u64 = a / b; sp--; NEXT(1); }
L_I64_REM_S: {
    int64_t a = sp[-2].i64, b = sp[-1].i64;
    if (!b) TRAP("integer divide by zero");
    sp[-2].i64 = b == -1 ? 0 : a % b; sp--; NEXT(1);
}
L_I64_REM_U: { uint64_t a = sp[-2].u64, b = sp[-1].u64; if (!b) TRAP("integer divide by zero"); sp[-2].u64 = a % b; sp--; NEXT(1); }
BIN64(I64_AND, a & b) BIN64(I64_OR, a | b) BIN64(I64_XOR, a ^ b) BIN64(I64_SHL, a << (b & 63))
L_I64_SHR_S: sp[-2].i64 = sp[-2].i64 >> (sp[-1].u64 & 63); sp--; NEXT(1);
BIN64(I64_SHR_U, a >> (b & 63))
BIN64(I64_ROTL, (a << (b & 63)) | (a >> ((64 - (b & 63)) & 63)))
BIN64(I64_ROTR, (a >> (b & 63)) | (a << ((64 - (b & 63)) & 63)))
#undef BIN32
#undef BIN64

L_F32_ABS: sp[-1].u32 &= 0x7fffffffu; NEXT(1);
L_F32_NEG: sp[-1].u32 ^= 0x80000000u; NEXT(1);
/* rounding: glibc returns a signaling NaN as-is; wasm requires a quiet one */
#define QNAN32(x, e) ((x) != (x) ? (x) + (x) : (e))
#define QNAN64(x, e) ((x) != (x) ? (x) + (x) : (e))
L_F32_CEIL: sp[-1].f32 = QNAN32(sp[-1].f32, ceilf(sp[-1].f32)); NEXT(1);
L_F32_FLOOR: sp[-1].f32 = QNAN32(sp[-1].f32, floorf(sp[-1].f32)); NEXT(1);
L_F32_TRUNC: sp[-1].f32 = QNAN32(sp[-1].f32, truncf(sp[-1].f32)); NEXT(1);
L_F32_NEAREST: sp[-1].f32 = QNAN32(sp[-1].f32, nearbyintf(sp[-1].f32)); NEXT(1);
L_F32_SQRT: sp[-1].f32 = sqrtf(sp[-1].f32); NEXT(1);
L_F32_ADD: sp[-2].f32 = sp[-2].f32 + sp[-1].f32; sp--; NEXT(1);
L_F32_SUB: sp[-2].f32 = sp[-2].f32 - sp[-1].f32; sp--; NEXT(1);
L_F32_MUL: sp[-2].f32 = sp[-2].f32 * sp[-1].f32; sp--; NEXT(1);
L_F32_DIV: sp[-2].f32 = sp[-2].f32 / sp[-1].f32; sp--; NEXT(1);
L_F32_MIN: sp[-2].f32 = fmin32(sp[-2].f32, sp[-1].f32); sp--; NEXT(1);
L_F32_MAX: sp[-2].f32 = fmax32(sp[-2].f32, sp[-1].f32); sp--; NEXT(1);
L_F32_COPYSIGN: sp[-2].u32 = (sp[-2].u32 & 0x7fffffffu) | (sp[-1].u32 & 0x80000000u); sp--; NEXT(1);
L_F64_ABS: sp[-1].u64 &= 0x7fffffffffffffffull; NEXT(1);
L_F64_NEG: sp[-1].u64 ^= 0x8000000000000000ull; NEXT(1);
L_F64_CEIL: sp[-1].f64 = QNAN64(sp[-1].f64, ceil(sp[-1].f64)); NEXT(1);
L_F64_FLOOR: sp[-1].f64 = QNAN64(sp[-1].f64, floor(sp[-1].f64)); NEXT(1);
L_F64_TRUNC: sp[-1].f64 = QNAN64(sp[-1].f64, trunc(sp[-1].f64)); NEXT(1);
L_F64_NEAREST: sp[-1].f64 = QNAN64(sp[-1].f64, nearbyint(sp[-1].f64)); NEXT(1);
L_F64_SQRT: sp[-1].f64 = sqrt(sp[-1].f64); NEXT(1);
L_F64_ADD: sp[-2].f64 = sp[-2].f64 + sp[-1].f64; sp--; NEXT(1);
L_F64_SUB: sp[-2].f64 = sp[-2].f64 - sp[-1].f64; sp--; NEXT(1);
L_F64_MUL: sp[-2].f64 = sp[-2].f64 * sp[-1].f64; sp--; NEXT(1);
L_F64_DIV: sp[-2].f64 = sp[-2].f64 / sp[-1].f64; sp--; NEXT(1);
L_F64_MIN: sp[-2].f64 = fmin64(sp[-2].f64, sp[-1].f64); sp--; NEXT(1);
L_F64_MAX: sp[-2].f64 = fmax64(sp[-2].f64, sp[-1].f64); sp--; NEXT(1);
L_F64_COPYSIGN: sp[-2].u64 = (sp[-2].u64 & 0x7fffffffffffffffull) | (sp[-1].u64 & 0x8000000000000000ull); sp--; NEXT(1);

L_I32_WRAP_I64: NEXT(1);
#define TRUNC(name, src, lo_ok, hi, dst, T) L_##name: { \
    double x = sp[-1].src; \
    if (x != x) TRAP("invalid conversion to integer"); \
    if (!(lo_ok && x < (hi))) TRAP("integer overflow"); \
    T v = (T)x; sp[-1].u64 = 0; sp[-1].dst = v; NEXT(1); }
TRUNC(I32_TRUNC_F32_S, f32, x >= -2147483648.0, 2147483648.0, i32, int32_t)
TRUNC(I32_TRUNC_F32_U, f32, x > -1.0, 4294967296.0, u32, uint32_t)
TRUNC(I32_TRUNC_F64_S, f64, x > -2147483649.0, 2147483648.0, i32, int32_t)
TRUNC(I32_TRUNC_F64_U, f64, x > -1.0, 4294967296.0, u32, uint32_t)
TRUNC(I64_TRUNC_F32_S, f32, x >= -9223372036854775808.0, 9223372036854775808.0, i64, int64_t)
TRUNC(I64_TRUNC_F32_U, f32, x > -1.0, 18446744073709551616.0, u64, uint64_t)
TRUNC(I64_TRUNC_F64_S, f64, x >= -9223372036854775808.0, 9223372036854775808.0, i64, int64_t)
TRUNC(I64_TRUNC_F64_U, f64, x > -1.0, 18446744073709551616.0, u64, uint64_t)
#undef TRUNC
L_I64_EXTEND_I32_S: sp[-1].i64 = sp[-1].i32; NEXT(1);
L_I64_EXTEND_I32_U: sp[-1].u64 = sp[-1].u32; NEXT(1);
L_F32_CONVERT_I32_S: { float f = (float)sp[-1].i32; sp[-1].u64 = 0; sp[-1].f32 = f; NEXT(1); }
L_F32_CONVERT_I32_U: { float f = (float)sp[-1].u32; sp[-1].u64 = 0; sp[-1].f32 = f; NEXT(1); }
L_F32_CONVERT_I64_S: { float f = (float)sp[-1].i64; sp[-1].u64 = 0; sp[-1].f32 = f; NEXT(1); }
L_F32_CONVERT_I64_U: { float f = (float)sp[-1].u64; sp[-1].u64 = 0; sp[-1].f32 = f; NEXT(1); }
L_F32_DEMOTE_F64: { float f = (float)sp[-1].f64; sp[-1].u64 = 0; sp[-1].f32 = f; NEXT(1); }
L_F64_CONVERT_I32_S: sp[-1].f64 = (double)sp[-1].i32; NEXT(1);
L_F64_CONVERT_I32_U: sp[-1].f64 = (double)sp[-1].u32; NEXT(1);
L_F64_CONVERT_I64_S: sp[-1].f64 = (double)sp[-1].i64; NEXT(1);
L_F64_CONVERT_I64_U: sp[-1].f64 = (double)sp[-1].u64; NEXT(1);
L_F64_PROMOTE_F32: sp[-1].f64 = (double)sp[-1].f32; NEXT(1);
L_I32_EXTEND8_S: sp[-1].i32 = (int8_t)sp[-1].u32; NEXT(1);
L_I32_EXTEND16_S: sp[-1].i32 = (int16_t)sp[-1].u32; NEXT(1);
L_I64_EXTEND8_S: sp[-1].i64 = (int8_t)sp[-1].u64; NEXT(1);
L_I64_EXTEND16_S: sp[-1].i64 = (int16_t)sp[-1].u64; NEXT(1);
L_I64_EXTEND32_S: sp[-1].i64 = (int32_t)sp[-1].u64; NEXT(1);
L_REF_IS_NULL: sp[-1].u64 = sp[-1].ref == NULL; NEXT(1);
L_REF_FUNC: sp->ref = inst->funcs[ip[1].u]; sp++; NEXT(2);
L_I32_TRUNC_SAT_F32_S: { int32_t v = sat_i32(sp[-1].f32); sp[-1].u64 = 0; sp[-1].i32 = v; NEXT(1); }
L_I32_TRUNC_SAT_F32_U: { uint32_t v = sat_u32(sp[-1].f32); sp[-1].u64 = 0; sp[-1].u32 = v; NEXT(1); }
L_I32_TRUNC_SAT_F64_S: { int32_t v = sat_i32(sp[-1].f64); sp[-1].u64 = 0; sp[-1].i32 = v; NEXT(1); }
L_I32_TRUNC_SAT_F64_U: { uint32_t v = sat_u32(sp[-1].f64); sp[-1].u64 = 0; sp[-1].u32 = v; NEXT(1); }
L_I64_TRUNC_SAT_F32_S: sp[-1].i64 = sat_i64(sp[-1].f32); NEXT(1);
L_I64_TRUNC_SAT_F32_U: sp[-1].u64 = sat_u64(sp[-1].f32); NEXT(1);
L_I64_TRUNC_SAT_F64_S: sp[-1].i64 = sat_i64(sp[-1].f64); NEXT(1);
L_I64_TRUNC_SAT_F64_U: sp[-1].u64 = sat_u64(sp[-1].f64); NEXT(1);

L_MEMORY_INIT: {
    const DataSeg *s = &inst->m->data[ip[1].u];
    uint64_t d = sp[-3].u32, src = sp[-2].u32, n = sp[-1].u32;
    uint64_t len = inst->data_dropped[ip[1].u] ? 0 : s->len;
    sp -= 3;
    if (src + n > len || d + n > msz) TRAP("out of bounds memory access");
    memcpy(mb + d, s->bytes + src, n);
    NEXT(2);
}
L_DATA_DROP: inst->data_dropped[ip[1].u] = 1; NEXT(2);
L_MEMORY_COPY: {
    uint64_t d = sp[-3].u32, s = sp[-2].u32, n = sp[-1].u32;
    sp -= 3;
    if (s + n > msz || d + n > msz) TRAP("out of bounds memory access");
    memmove(mb + d, mb + s, n);
    NEXT(1);
}
L_MEMORY_FILL: {
    uint64_t d = sp[-3].u32, n = sp[-1].u32;
    uint8_t v = (uint8_t)sp[-2].u32;
    sp -= 3;
    if (d + n > msz) TRAP("out of bounds memory access");
    memset(mb + d, v, n);
    NEXT(1);
}
L_TABLE_INIT: {
    const ElemSeg *s = &inst->m->elems[ip[1].u];
    WasmTable *t = inst->tables[ip[2].u];
    uint64_t d = sp[-3].u32, src = sp[-2].u32, n = sp[-1].u32;
    uint64_t len = inst->elem_dropped[ip[1].u] ? 0 : s->n;
    sp -= 3;
    if (src + n > len || d + n > t->size) TRAP("out of bounds table access");
    for (uint64_t i = 0; i < n; i++) {
        const ElemItem *it = &s->items[src + i];
        WasmVal v = { .u64 = 0 };
        if (it->is_expr) v = wasm_const_eval(inst, it->expr);
        else v.ref = inst->funcs[it->func];
        wasm_table_set(t, (uint32_t)(d + i), v);
    }
    NEXT(3);
}
L_ELEM_DROP: inst->elem_dropped[ip[1].u] = 1; NEXT(2);
L_TABLE_COPY: {
    WasmTable *td = inst->tables[ip[1].u], *ts = inst->tables[ip[2].u];
    uint64_t d = sp[-3].u32, s = sp[-2].u32, n = sp[-1].u32;
    sp -= 3;
    if (s + n > ts->size || d + n > td->size) TRAP("out of bounds table access");
    if (d <= s) for (uint64_t i = 0; i < n; i++) wasm_table_set(td, (uint32_t)(d + i), ts->elems[s + i]);
    else for (uint64_t i = n; i-- > 0;) wasm_table_set(td, (uint32_t)(d + i), ts->elems[s + i]);
    NEXT(3);
}
L_TABLE_GROW: {
    WasmTable *t = inst->tables[ip[1].u];
    int64_t r = wasm_table_grow(t, sp[-1].u32, sp[-2]);
    sp--;
    sp[-1].u64 = 0;
    sp[-1].i32 = (int32_t)r;
    NEXT(2);
}
L_TABLE_SIZE: sp->u64 = inst->tables[ip[1].u]->size; sp++; NEXT(2);
L_TABLE_FILL: {
    WasmTable *t = inst->tables[ip[1].u];
    uint64_t i = sp[-3].u32, n = sp[-1].u32;
    WasmVal v = sp[-2];
    sp -= 3;
    if (i + n > t->size) TRAP("out of bounds table access");
    for (uint64_t k = 0; k < n; k++) wasm_table_set(t, (uint32_t)(i + k), v);
    NEXT(2);
}

trap:
    wasm_err(err, trapkind, "%s", trapmsg);
unwind:
    while (nframes > entry_depth) {
        Frame *fr = &frames[--nframes];
        if (fr->crossed) wasm_instance_unref(inst);
        inst = fr->inst;
    }
    cur_sp = base;
    return -1;
#undef RELOAD
#undef DISPATCH
#undef NEXT
#undef TRAP
#undef EA
}

void wasm_init_labels(void) {
    if (!wasm_op_labels) exec(NULL, NULL, NULL);
}

int wasm_call(WasmFunc *f, const WasmVal *args, WasmVal *results, WasmError *err) {
    if (err) err->kind = WASM_OK;
    if (stacks_init()) { wasm_err(err, WASM_ERR_NOMEM, "out of memory for the stack"); return -1; }
    const WasmFuncType *t = f->type;
    WasmVal *base = cur_sp;
    if (base + t->nparams + t->nresults + 16 > vstack_end) { wasm_err(err, WASM_ERR_EXHAUSTED, "call stack exhausted"); return -1; }
    if (t->nparams) memcpy(base, args, t->nparams * sizeof *args);
    int r;
    if (f->host) {
        cur_sp = base + t->nparams + t->nresults;
        WasmError herr = { WASM_OK, "" };
        r = f->host(f->ud, base, base + t->nparams, &herr);
        if (r && err) *err = herr;
        if (!r) memmove(base, base + t->nparams, t->nresults * sizeof *base);
    } else {
        WasmInstance *inst = f->inst;
        wasm_instance_ref(inst);
        r = exec(f, base, err);
        wasm_instance_unref(inst);
    }
    if (!r && t->nresults) memcpy(results, base, t->nresults * sizeof *results);
    cur_sp = base;
    return r;
}

/* ---- instantiation ---- */

static int limits_match(uint32_t min, uint32_t max, uint32_t want_min, uint32_t want_max) {
    if (min < want_min) return 0;
    if (want_max != WASM_NO_MAX && (max == WASM_NO_MAX || max > want_max)) return 0;
    return 1;
}

static void instance_free(WasmInstance *inst) {
    WasmModule *m = inst->m;
    if (inst->tables)
        for (uint32_t i = 0; i < m->ntables; i++) {
            WasmTable *t = inst->tables[i];
            if (!t) continue;
            if (i < m->ntable_imports) wasm_table_unref(t);
            else table_destroy(t);
        }
    if (inst->globals)
        for (uint32_t i = 0; i < m->nglobals; i++) {
            WasmGlobal *g = inst->globals[i];
            if (!g) continue;
            if (i < m->nglobal_imports) wasm_global_unref(g);
            else global_destroy(g);
        }
    wasm_memory_unref(inst->mem);
    if (inst->funcs)
        for (uint32_t i = 0; i < m->nfunc_imports; i++) if (inst->funcs[i]) wasm_func_unref(inst->funcs[i]);
    free(inst->funcs);
    free(inst->own);
    free(inst->tables);
    free(inst->globals);
    free(inst->elem_dropped);
    free(inst->data_dropped);
    free(inst->exports);
    wasm_module_unref(m);
    free(inst);
}

WasmInstance *wasm_instantiate(WasmModule *m, const WasmExtern *imports, uint32_t nimports, WasmError *err) {
    wasm_init_labels();
    if (err) err->kind = WASM_OK;
    if (nimports != m->nimports) { wasm_err(err, WASM_ERR_LINK, "se esperaban %u imports", m->nimports); return NULL; }
    for (uint32_t i = 0; i < nimports; i++) {
        const Import *im = &m->imports[i];
        const WasmExtern *e = &imports[i];
        const char *why = NULL;
        if (e->kind != im->type.kind || !e->ptr) why = "incompatible import type";
        else switch (e->kind) {
        case WASM_EXT_FUNC: if (e->func->type != im->type.func) why = "incompatible import type"; break;
        case WASM_EXT_TABLE:
            if (e->table->type != im->type.valtype || !limits_match(e->table->size, e->table->max, im->type.min, im->type.max))
                why = "incompatible import type";
            break;
        case WASM_EXT_MEMORY:
            if (!limits_match((uint32_t)(e->memory->size / WASM_PAGE), e->memory->max, im->type.min, im->type.max))
                why = "incompatible import type";
            break;
        case WASM_EXT_GLOBAL:
            if (e->global->type != im->type.valtype || e->global->mut != im->type.mut) why = "incompatible import type";
            break;
        }
        if (why) { wasm_err(err, WASM_ERR_LINK, "%s (%s.%s)", why, im->module, im->name); return NULL; }
    }

    WasmInstance *inst = calloc(1, sizeof *inst);
    if (!inst) { wasm_err(err, WASM_ERR_NOMEM, "out of memory"); return NULL; }
    inst->rc = 1;
    inst->m = m;
    wasm_module_ref(m);
    uint32_t ndef = m->nfuncs - m->nfunc_imports;
    inst->nfuncs = m->nfuncs;
    inst->funcs = calloc(m->nfuncs + 1, sizeof *inst->funcs);
    inst->own = calloc(ndef + 1, sizeof *inst->own);
    inst->tables = calloc(m->ntables + 1, sizeof *inst->tables);
    inst->globals = calloc(m->nglobals + 1, sizeof *inst->globals);
    inst->elem_dropped = calloc(m->nelems + 1, 1);
    inst->data_dropped = calloc(m->ndata + 1, 1);
    inst->exports = calloc(m->nexports + 1, sizeof *inst->exports);
    if (!inst->funcs || !inst->own || !inst->tables || !inst->globals || !inst->elem_dropped || !inst->data_dropped || !inst->exports) {
        wasm_err(err, WASM_ERR_NOMEM, "out of memory");
        goto fail;
    }
    /* imports */
    uint32_t fi = 0, ti = 0, gi = 0;
    for (uint32_t i = 0; i < nimports; i++) {
        const WasmExtern *e = &imports[i];
        switch (e->kind) {
        case WASM_EXT_FUNC: inst->funcs[fi++] = e->func; wasm_func_ref(e->func); break;
        case WASM_EXT_TABLE: inst->tables[ti++] = e->table; wasm_table_ref(e->table); break;
        case WASM_EXT_MEMORY: inst->mem = e->memory; wasm_memory_ref(e->memory); break;
        case WASM_EXT_GLOBAL: inst->globals[gi++] = e->global; wasm_global_ref(e->global); break;
        }
    }
    for (uint32_t i = 0; i < ndef; i++) {
        WasmFunc *f = &inst->own[i];
        f->type = m->functypes[m->nfunc_imports + i];
        f->inst = inst;
        f->code = &m->code[i];
        inst->funcs[m->nfunc_imports + i] = f;
    }
    for (uint32_t i = m->ntable_imports; i < m->ntables; i++) {
        inst->tables[i] = table_alloc(inst, m->tables[i].type, m->tables[i].min, m->tables[i].max, (WasmVal){ .u64 = 0 }, err);
        if (!inst->tables[i]) goto fail;
    }
    if (m->nmems && !m->nmem_imports) {
        inst->mem = wasm_memory_new(m->mem_min, m->mem_max, err);
        if (!inst->mem) goto fail;
    }
    for (uint32_t i = m->nglobal_imports; i < m->nglobals; i++) {
        WasmVal v = wasm_const_eval(inst, m->globals[i].init);
        inst->globals[i] = global_alloc(inst, m->globals[i].type, m->globals[i].mut, v);
        if (!inst->globals[i]) { wasm_err(err, WASM_ERR_NOMEM, "out of memory"); goto fail; }
    }
    for (uint32_t i = 0; i < m->nexports; i++) {
        const Export *x = &m->exports[i];
        WasmExtern *e = &inst->exports[i];
        e->kind = x->kind;
        switch (x->kind) {
        case WASM_EXT_FUNC: e->func = inst->funcs[x->idx]; break;
        case WASM_EXT_TABLE: e->table = inst->tables[x->idx]; break;
        case WASM_EXT_MEMORY: e->memory = inst->mem; break;
        case WASM_EXT_GLOBAL: e->global = inst->globals[x->idx]; break;
        }
    }
    /* element segments: active ones in order; declarative ones are discarded */
    for (uint32_t i = 0; i < m->nelems; i++) {
        const ElemSeg *s = &m->elems[i];
        if (s->mode == 1) continue;
        inst->elem_dropped[i] = 1;
        if (s->mode == 2) continue;
        uint64_t off = (uint32_t)wasm_const_eval(inst, s->offset).u32;
        WasmTable *t = inst->tables[s->table];
        if (off + s->n > t->size) { wasm_err(err, WASM_ERR_TRAP, "out of bounds table access"); goto fail; }
        for (uint32_t k = 0; k < s->n; k++) {
            const ElemItem *it = &s->items[k];
            WasmVal v = { .u64 = 0 };
            if (it->is_expr) v = wasm_const_eval(inst, it->expr);
            else v.ref = inst->funcs[it->func];
            wasm_table_set(t, (uint32_t)(off + k), v);
        }
    }
    for (uint32_t i = 0; i < m->ndata; i++) {
        const DataSeg *s = &m->data[i];
        if (!s->active) continue;
        inst->data_dropped[i] = 1;
        uint64_t off = (uint32_t)wasm_const_eval(inst, s->offset).u32;
        if (off + s->len > inst->mem->size) { wasm_err(err, WASM_ERR_TRAP, "out of bounds memory access"); goto fail; }
        memcpy(inst->mem->data + off, s->bytes, s->len);
    }
    if (m->start >= 0 && wasm_call(inst->funcs[m->start], NULL, NULL, err)) goto fail;
    return inst;
fail:
    wasm_instance_unref(inst);
    return NULL;
}

WasmExtern wasm_instance_export(const WasmInstance *i, uint32_t idx) {
    if (idx >= i->m->nexports) return (WasmExtern){ .kind = -1 };
    return i->exports[idx];
}

WasmExtern wasm_instance_export_find_len(const WasmInstance *i, const char *name, size_t len) {
    for (uint32_t k = 0; k < i->m->nexports; k++)
        if (i->m->exports[k].len == len && !memcmp(i->m->exports[k].name, name, len)) return i->exports[k];
    return (WasmExtern){ .kind = -1 };
}

WasmExtern wasm_instance_export_find(const WasmInstance *i, const char *name) {
    return wasm_instance_export_find_len(i, name, strlen(name));
}
