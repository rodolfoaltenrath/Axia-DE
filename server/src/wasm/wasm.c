/* wasm.c — WebAssembly decoder, validator and compiler (see wasm.h).
 *
 * A single pass per function: it validates the types with the algorithm from the
 * specification's appendix and at the same time generates the internal code (wasm_int.h). Since the validator knows the
 * exact stack height at every point, jumps come out with their target already resolved and with
 * how many values to move, without looking up labels at run time. Unreachable code is validated
 * but not generated. */
#include "wasm_int.h"

#include <setjmp.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void wasm_err(WasmError *err, WasmErrKind kind, const char *fmt, ...) {
    if (!err) return;
    err->kind = kind;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(err->msg, sizeof err->msg, fmt, ap);
    va_end(ap);
}

/* ---- interned function types ---- */

typedef struct TypeNode { struct TypeNode *next; WasmFuncType t; uint8_t v[]; } TypeNode;
static TypeNode *type_list[64];

const WasmFuncType *wasm_functype(const uint8_t *params, uint32_t np, const uint8_t *results, uint32_t nr) {
    uint32_t h = np * 31u + nr;
    for (uint32_t i = 0; i < np; i++) h = h * 131u + params[i];
    for (uint32_t i = 0; i < nr; i++) h = h * 137u + results[i];
    TypeNode **b = &type_list[h % 64];
    for (TypeNode *n = *b; n; n = n->next)
        if (n->t.nparams == np && n->t.nresults == nr && !memcmp(n->v, params, np) && !memcmp(n->v + np, results, nr))
            return &n->t;
    TypeNode *n = malloc(sizeof *n + np + nr + 1);
    if (!n) return NULL;
    memcpy(n->v, params, np);
    memcpy(n->v + np, results, nr);
    n->t = (WasmFuncType){ np, nr, n->v, n->v + np };
    n->next = *b;
    *b = n;
    return &n->t;
}

/* ---- reading ---- */

typedef struct {
    uint8_t kind;                       /* 0 function, 1 block, 2 loop, 3 if, 4 else */
    uint32_t np, nr;
    const uint8_t *params, *results;
    uint32_t height;                    /* type stack height below the parameters */
    uint8_t unreachable, emit;
    size_t loop_pc;
    size_t else_op;                     /* the if's BR_UNLESS (SIZE_MAX if not generated) */
    int32_t patches;                    /* jumps to the end, still to be resolved */
} Ctrl;

typedef struct { size_t op, cell; int32_t next; } Patch;

typedef struct {
    jmp_buf jb;
    WasmError *err;
    WasmModule *m;
    const uint8_t *p, *end;
    /* function being compiled */
    uint8_t *vals; uint32_t nvals, capvals, maxvals;
    Ctrl *ctrls; uint32_t nctrls, capctrls;
    Cell *code; size_t ncode, capcode;
    Patch *patches; uint32_t npatches, cappatches;
    uint8_t *locals; uint32_t nlocals;
    const WasmFuncType *ftype;
    int uses_data;                      /* memory.init / data.drop (require datacount) */
} Dec;

#define UNKNOWN 0

static void fail(Dec *d, WasmErrKind kind, const char *fmt, ...) __attribute__((noreturn, format(printf, 3, 4)));
static void fail(Dec *d, WasmErrKind kind, const char *fmt, ...) {
    if (d->err) {
        d->err->kind = kind;
        va_list ap;
        va_start(ap, fmt);
        vsnprintf(d->err->msg, sizeof d->err->msg, fmt, ap);
        va_end(ap);
    }
    longjmp(d->jb, 1);
}
#define MALFORMED(...) fail(d, WASM_ERR_MALFORMED, __VA_ARGS__)
#define INVALID(...) fail(d, WASM_ERR_INVALID, __VA_ARGS__)

static void *xalloc(Dec *d, size_t n) {
    void *p = calloc(1, n ? n : 1);
    if (!p) fail(d, WASM_ERR_NOMEM, "out of memory");
    return p;
}

static void grow(Dec *d, void **p, uint32_t *cap, uint32_t need, size_t elem) {
    if (need <= *cap) return;
    uint32_t c = *cap ? *cap : 16;
    while (c < need) c *= 2;
    void *n = realloc(*p, (size_t)c * elem);
    if (!n) fail(d, WASM_ERR_NOMEM, "out of memory");
    *p = n;
    *cap = c;
}

static uint8_t u8(Dec *d) {
    if (d->p >= d->end) MALFORMED("unexpected end");
    return *d->p++;
}

static uint64_t leb(Dec *d, unsigned bits, int sign) {
    uint64_t r = 0;
    unsigned shift = 0, maxb = (bits + 6) / 7;
    uint8_t b = 0;
    for (unsigned i = 0;; i++) {
        if (i >= maxb) MALFORMED("integer representation too long");
        b = u8(d);
        if (i == maxb - 1) {
            if (b & 0x80) MALFORMED("integer representation too long");
            unsigned used = bits - 7 * i;
            if (!sign) {
                if (used < 7 && (b >> used)) MALFORMED("integer too large");
            } else {
                uint8_t mask = (uint8_t)((0x7f >> (used - 1)) << (used - 1)) & 0x7f;
                uint8_t v = b & mask;
                if (v != 0 && v != mask) MALFORMED("integer too large");
            }
        }
        r |= (uint64_t)(b & 0x7f) << shift;
        shift += 7;
        if (!(b & 0x80)) break;
    }
    if (sign && shift < 64 && (b & 0x40)) r |= ~0ull << shift;
    return r;
}

static uint32_t u32(Dec *d) { return (uint32_t)leb(d, 32, 0); }

static const uint8_t *bytes(Dec *d, size_t n) {
    if ((size_t)(d->end - d->p) < n) MALFORMED("unexpected end");
    const uint8_t *p = d->p;
    d->p += n;
    return p;
}

static int utf8_ok(const uint8_t *s, size_t n) {
    for (size_t i = 0; i < n;) {
        uint8_t c = s[i];
        size_t len;
        uint32_t cp, min;
        if (c < 0x80) { i++; continue; }
        else if ((c & 0xe0) == 0xc0) { len = 2; cp = c & 0x1f; min = 0x80; }
        else if ((c & 0xf0) == 0xe0) { len = 3; cp = c & 0x0f; min = 0x800; }
        else if ((c & 0xf8) == 0xf0) { len = 4; cp = c & 0x07; min = 0x10000; }
        else return 0;
        if (i + len > n) return 0;
        for (size_t k = 1; k < len; k++) {
            if ((s[i + k] & 0xc0) != 0x80) return 0;
            cp = cp << 6 | (s[i + k] & 0x3f);
        }
        if (cp < min || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) return 0;
        i += len;
    }
    return 1;
}

static char *name(Dec *d, uint32_t *len) {
    uint32_t n = u32(d);
    if (len) *len = n;
    const uint8_t *s = bytes(d, n);
    if (!utf8_ok(s, n)) MALFORMED("malformed UTF-8 encoding");
    char *r = xalloc(d, (size_t)n + 1);
    memcpy(r, s, n);
    return r;
}

static int is_num(uint8_t t) { return t == WASM_I32 || t == WASM_I64 || t == WASM_F32 || t == WASM_F64; }
static int is_ref(uint8_t t) { return t == WASM_FUNCREF || t == WASM_EXTERNREF; }

static uint8_t valtype(Dec *d) {
    uint8_t t = u8(d);
    if (!is_num(t) && !is_ref(t)) MALFORMED("malformed value type 0x%02x", t);
    return t;
}

static uint8_t reftype(Dec *d) {
    uint8_t t = u8(d);
    if (!is_ref(t)) MALFORMED("malformed reference type");
    return t;
}

static void limits(Dec *d, uint32_t *min, uint32_t *max, uint32_t cap, const char *what) {
    uint8_t f = u8(d);
    if (f > 1) MALFORMED("integer too large");     /* shared or 64-bit: not supported */
    *min = u32(d);
    *max = f ? u32(d) : WASM_NO_MAX;
    if (cap && *min > cap) INVALID("%s size must be at most %u pages (4GiB)", what, cap);
    if (cap && f && *max > cap) INVALID("%s size must be at most %u pages (4GiB)", what, cap);
    if (f && *max < *min) INVALID("size minimum must not be greater than maximum");
}

/* Constant expression: validates and stores its bytes (evaluated at instantiation). */
static ConstExpr const_expr(Dec *d, uint8_t want) {
    WasmModule *m = d->m;
    ConstExpr e = { d->p, 0 };
    uint8_t stack[8];
    int n = 0;
    for (;;) {
        uint8_t op = u8(d), t;
        switch (op) {
        case 0x0b:
            if (n != 1 || (stack[0] != want && stack[0] != UNKNOWN)) INVALID("type mismatch");
            e.len = (size_t)(d->p - e.p);
            return e;
        case 0x41: leb(d, 32, 1); t = WASM_I32; break;
        case 0x42: leb(d, 64, 1); t = WASM_I64; break;
        case 0x43: bytes(d, 4); t = WASM_F32; break;
        case 0x44: bytes(d, 8); t = WASM_F64; break;
        case 0xd0: t = reftype(d); break;
        case 0xd2: {
            uint32_t f = u32(d);
            if (f >= m->nfuncs) INVALID("unknown function %u", f);
            m->declared[f] = 1;
            t = WASM_FUNCREF;
            break;
        }
        case 0x23: {
            uint32_t g = u32(d);
            if (g >= m->nglobal_imports || g >= m->nglobals) INVALID("unknown global %u", g);
            if (m->globals[g].mut) INVALID("constant expression required");
            t = m->globals[g].type;
            break;
        }
        default:
            if (op <= 0x11 || (op >= 0x1a && op <= 0x1c) || (op >= 0x20 && op <= 0x26) || (op >= 0x28 && op <= 0xc4) ||
                op == 0xd1 || op == 0xfc)
                INVALID("constant expression required");
            MALFORMED("illegal opcode 0x%02x", op);
        }
        if (n >= 8) INVALID("type mismatch");
        stack[n++] = t;
    }
}

/* ---- function compilation ---- */

static int emitting(Dec *d) { Ctrl *c = &d->ctrls[d->nctrls - 1]; return c->emit && !c->unreachable; }

static size_t emit_raw(Dec *d, uint64_t v) {
    if (d->ncode + 1 > d->capcode) {
        size_t c = d->capcode ? d->capcode * 2 : 256;
        Cell *n = realloc(d->code, c * sizeof *n);
        if (!n) fail(d, WASM_ERR_NOMEM, "out of memory");
        d->code = n;
        d->capcode = c;
    }
    d->code[d->ncode].u = v;
    return d->ncode++;
}

static size_t emit_op(Dec *d, int op) {
    size_t at = d->ncode;
    if (d->capcode <= d->ncode) emit_raw(d, 0), d->ncode--;
    d->code[d->ncode++].op = wasm_op_labels[op];
    return at;
}

static void push(Dec *d, uint8_t t) {
    grow(d, (void **)&d->vals, &d->capvals, d->nvals + 1, 1);
    d->vals[d->nvals++] = t;
    if (d->nvals > d->maxvals) d->maxvals = d->nvals;
}

static uint8_t pop(Dec *d) {
    Ctrl *c = &d->ctrls[d->nctrls - 1];
    if (d->nvals == c->height) {
        if (c->unreachable) return UNKNOWN;
        INVALID("type mismatch");
    }
    return d->vals[--d->nvals];
}

static uint8_t pop_t(Dec *d, uint8_t want) {
    uint8_t t = pop(d);
    if (t != want && t != UNKNOWN && want != UNKNOWN) INVALID("type mismatch");
    return t;
}

static void pop_list(Dec *d, const uint8_t *ts, uint32_t n) {
    for (uint32_t i = n; i-- > 0;) pop_t(d, ts[i]);
}

static void push_list(Dec *d, const uint8_t *ts, uint32_t n) {
    for (uint32_t i = 0; i < n; i++) push(d, ts[i]);
}

static void push_ctrl(Dec *d, uint8_t kind, uint32_t np, const uint8_t *params, uint32_t nr, const uint8_t *results) {
    int emit = d->nctrls ? emitting(d) : 1;
    grow(d, (void **)&d->ctrls, &d->capctrls, d->nctrls + 1, sizeof(Ctrl));
    Ctrl *c = &d->ctrls[d->nctrls++];
    *c = (Ctrl){ .kind = kind, .np = np, .nr = nr, .params = params, .results = results,
                 .height = d->nvals, .emit = (uint8_t)emit, .loop_pc = d->ncode, .else_op = SIZE_MAX, .patches = -1 };
    push_list(d, params, np);
}

static void label_types(Ctrl *c, const uint8_t **ts, uint32_t *n) {
    if (c->kind == 2) { *ts = c->params; *n = c->np; }
    else { *ts = c->results; *n = c->nr; }
}

static void unreachable(Dec *d) {
    Ctrl *c = &d->ctrls[d->nctrls - 1];
    d->nvals = c->height;
    c->unreachable = 1;
}

static void add_patch(Dec *d, Ctrl *c, size_t op, size_t cell) {
    grow(d, (void **)&d->patches, &d->cappatches, d->npatches + 1, sizeof(Patch));
    d->patches[d->npatches] = (Patch){ op, cell, c->patches };
    c->patches = (int32_t)d->npatches++;
}

static void resolve_patches(Dec *d, Ctrl *c, size_t target) {
    for (int32_t i = c->patches; i >= 0; i = d->patches[i].next)
        d->code[d->patches[i].cell].i = (int64_t)target - (int64_t)d->patches[i].op;
    c->patches = -1;
}

/* Jump to label `depth`: the type stack already has the label's values on top. */
static void emit_branch(Dec *d, uint32_t depth, int cond) {
    Ctrl *c = &d->ctrls[d->nctrls - 1 - depth];
    const uint8_t *ts;
    uint32_t a;
    label_types(c, &ts, &a);
    uint32_t h = d->nlocals + c->height, cur = d->nlocals + d->nvals;
    int move = cur - a != h;
    size_t op = emit_op(d, cond ? (move ? OP_BR_IF_N : OP_BR_IF) : (move ? OP_BR_N : OP_BR));
    size_t cell = emit_raw(d, 0);
    if (move) { emit_raw(d, h); emit_raw(d, a); }
    if (c->kind == 2) d->code[cell].i = (int64_t)c->loop_pc - (int64_t)op;
    else add_patch(d, c, op, cell);
}

static void emit_return(Dec *d) {
    uint32_t n = d->ftype->nresults;
    if (n == 0) emit_op(d, OP_RETURN0);
    else if (n == 1) emit_op(d, OP_RETURN1);
    else { emit_op(d, OP_RETURN); emit_raw(d, n); }
}

static const uint8_t vt_single[] = { WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_FUNCREF, WASM_EXTERNREF };
static const uint8_t *single(uint8_t t) {
    for (size_t i = 0; i < sizeof vt_single; i++) if (vt_single[i] == t) return &vt_single[i];
    return NULL;
}

static void blocktype(Dec *d, uint32_t *np, const uint8_t **params, uint32_t *nr, const uint8_t **results) {
    uint8_t b = d->p < d->end ? *d->p : 0;
    *np = *nr = 0;
    *params = *results = NULL;
    if (b == 0x40) { d->p++; return; }
    if (is_num(b) || is_ref(b)) { d->p++; *nr = 1; *results = single(b); return; }
    int64_t idx = (int64_t)leb(d, 33, 1);
    if (idx < 0) MALFORMED("malformed block type");
    if ((uint64_t)idx >= d->m->ntypes) INVALID("unknown type %lld", (long long)idx);
    const WasmFuncType *t = d->m->types[idx];
    *np = t->nparams; *params = t->params;
    *nr = t->nresults; *results = t->results;
}

/* Types of the numeric instructions without immediates (0x45..0xC4): input(s) and output. */
static void numsig(uint8_t op, uint8_t *a, uint8_t *b, uint8_t *r) {
    static const struct { uint8_t lo, hi, a, b, r; } ranges[] = {
        { 0x45, 0x45, WASM_I32, 0, WASM_I32 }, { 0x46, 0x4f, WASM_I32, WASM_I32, WASM_I32 },
        { 0x50, 0x50, WASM_I64, 0, WASM_I32 }, { 0x51, 0x5a, WASM_I64, WASM_I64, WASM_I32 },
        { 0x5b, 0x60, WASM_F32, WASM_F32, WASM_I32 }, { 0x61, 0x66, WASM_F64, WASM_F64, WASM_I32 },
        { 0x67, 0x69, WASM_I32, 0, WASM_I32 }, { 0x6a, 0x78, WASM_I32, WASM_I32, WASM_I32 },
        { 0x79, 0x7b, WASM_I64, 0, WASM_I64 }, { 0x7c, 0x8a, WASM_I64, WASM_I64, WASM_I64 },
        { 0x8b, 0x91, WASM_F32, 0, WASM_F32 }, { 0x92, 0x98, WASM_F32, WASM_F32, WASM_F32 },
        { 0x99, 0x9f, WASM_F64, 0, WASM_F64 }, { 0xa0, 0xa6, WASM_F64, WASM_F64, WASM_F64 },
        { 0xa7, 0xa7, WASM_I64, 0, WASM_I32 }, { 0xa8, 0xa9, WASM_F32, 0, WASM_I32 },
        { 0xaa, 0xab, WASM_F64, 0, WASM_I32 }, { 0xac, 0xad, WASM_I32, 0, WASM_I64 },
        { 0xae, 0xaf, WASM_F32, 0, WASM_I64 }, { 0xb0, 0xb1, WASM_F64, 0, WASM_I64 },
        { 0xb2, 0xb3, WASM_I32, 0, WASM_F32 }, { 0xb4, 0xb5, WASM_I64, 0, WASM_F32 },
        { 0xb6, 0xb6, WASM_F64, 0, WASM_F32 }, { 0xb7, 0xb8, WASM_I32, 0, WASM_F64 },
        { 0xb9, 0xba, WASM_I64, 0, WASM_F64 }, { 0xbb, 0xbb, WASM_F32, 0, WASM_F64 },
        { 0xbc, 0xbc, WASM_F32, 0, WASM_I32 }, { 0xbd, 0xbd, WASM_F64, 0, WASM_I64 },
        { 0xbe, 0xbe, WASM_I32, 0, WASM_F32 }, { 0xbf, 0xbf, WASM_I64, 0, WASM_F64 },
        { 0xc0, 0xc1, WASM_I32, 0, WASM_I32 }, { 0xc2, 0xc4, WASM_I64, 0, WASM_I64 },
    };
    for (size_t i = 0; i < sizeof ranges / sizeof *ranges; i++)
        if (op >= ranges[i].lo && op <= ranges[i].hi) { *a = ranges[i].a; *b = ranges[i].b; *r = ranges[i].r; return; }
    *a = *b = *r = 0;
}

static int byte2op[0x112];

static void init_byte2op(void) {
    static int done;
    if (done) return;
    done = 1;
    for (int i = 0; i < 0x112; i++) byte2op[i] = -1;
#define X(name, byte) if ((byte) >= 0) byte2op[(byte)] = OP_##name;
    WASM_OPS(X)
#undef X
}

static void memarg(Dec *d, uint32_t natural, uint32_t *offset) {
    if (!d->m->nmems) INVALID("unknown memory 0");
    uint32_t align = u32(d);
    if (align >= 32) MALFORMED("malformed memop flags");
    *offset = u32(d);
    if ((1u << align) > natural) INVALID("alignment must not be larger than natural");
}

static uint32_t table_idx(Dec *d) {
    uint32_t t = u32(d);
    if (t >= d->m->ntables) INVALID("unknown table %u", t);
    return t;
}

static void compile_body(Dec *d, uint32_t fidx) {
    WasmModule *m = d->m;
    const WasmFuncType *ft = m->functypes[fidx];
    d->ftype = ft;
    /* locals */
    uint32_t ngroups = u32(d);
    uint64_t total = ft->nparams;
    const uint8_t *save = d->p;
    for (uint32_t i = 0; i < ngroups; i++) {
        total += u32(d);
        valtype(d);
        if (total > 50000) MALFORMED("too many locals");
    }
    d->p = save;
    free(d->locals);
    d->locals = xalloc(d, (size_t)total + 1);
    memcpy(d->locals, ft->params, ft->nparams);
    uint32_t nl = ft->nparams;
    for (uint32_t i = 0; i < ngroups; i++) {
        uint32_t n = u32(d);
        uint8_t t = valtype(d);
        memset(d->locals + nl, t, n);
        nl += n;
    }
    d->nlocals = nl;
    d->nvals = d->nctrls = d->maxvals = 0;
    d->ncode = d->npatches = 0;
    push_ctrl(d, 0, 0, NULL, ft->nresults, ft->results);

    while (d->nctrls) {
        uint8_t op = u8(d);
        int em = emitting(d);
        switch (op) {
        case 0x00:                                       /* unreachable */
            if (em) emit_op(d, OP_UNREACHABLE);
            unreachable(d);
            break;
        case 0x01: break;                                /* nop */
        case 0x02: case 0x03: {                          /* block, loop */
            uint32_t np, nr;
            const uint8_t *ps, *rs;
            blocktype(d, &np, &ps, &nr, &rs);
            pop_list(d, ps, np);
            push_ctrl(d, op == 0x02 ? 1 : 2, np, ps, nr, rs);
            break;
        }
        case 0x04: {                                     /* if */
            uint32_t np, nr;
            const uint8_t *ps, *rs;
            blocktype(d, &np, &ps, &nr, &rs);
            pop_t(d, WASM_I32);
            pop_list(d, ps, np);
            size_t at = SIZE_MAX;
            if (em) { at = emit_op(d, OP_BR_UNLESS); emit_raw(d, 0); }
            push_ctrl(d, 3, np, ps, nr, rs);
            d->ctrls[d->nctrls - 1].else_op = at;
            break;
        }
        case 0x05: {                                     /* else */
            Ctrl *c = &d->ctrls[d->nctrls - 1];
            if (c->kind != 3) MALFORMED("else without if");
            pop_list(d, c->results, c->nr);
            if (d->nvals != c->height) INVALID("type mismatch");
            if (em) { size_t at = emit_op(d, OP_BR); add_patch(d, c, at, emit_raw(d, 0)); }
            if (c->else_op != SIZE_MAX) d->code[c->else_op + 1].i = (int64_t)d->ncode - (int64_t)c->else_op;
            c->else_op = SIZE_MAX;
            c->kind = 4;
            c->unreachable = 0;
            push_list(d, c->params, c->np);
            break;
        }
        case 0x0b: {                                     /* end */
            Ctrl *c = &d->ctrls[d->nctrls - 1];
            pop_list(d, c->results, c->nr);
            if (d->nvals != c->height) INVALID("type mismatch");
            if (c->kind == 3) {                          /* if without else: the parameters pass through as-is */
                if (c->np != c->nr || memcmp(c->params, c->results, c->np)) INVALID("type mismatch");
                if (c->else_op != SIZE_MAX) d->code[c->else_op + 1].i = (int64_t)d->ncode - (int64_t)c->else_op;
            }
            resolve_patches(d, c, d->ncode);
            if (c->kind == 0 && c->emit) emit_return(d);
            uint32_t nr = c->nr;
            const uint8_t *rs = c->results;
            d->nctrls--;
            if (d->nctrls) push_list(d, rs, nr);
            break;
        }
        case 0x0c: case 0x0d: {                          /* br, br_if */
            uint32_t depth = u32(d);
            if (depth >= d->nctrls) INVALID("unknown label");
            if (op == 0x0d) pop_t(d, WASM_I32);
            Ctrl *c = &d->ctrls[d->nctrls - 1 - depth];
            const uint8_t *ts;
            uint32_t n;
            label_types(c, &ts, &n);
            pop_list(d, ts, n);
            push_list(d, ts, n);
            if (em) emit_branch(d, depth, op == 0x0d);
            if (op == 0x0c) unreachable(d);
            break;
        }
        case 0x0e: {                                     /* br_table */
            uint32_t n = u32(d);
            if (n > 100000) MALFORMED("br_table too long");
            uint32_t *labels = xalloc(d, ((size_t)n + 1) * sizeof *labels);
            for (uint32_t i = 0; i <= n; i++) labels[i] = u32(d);
            pop_t(d, WASM_I32);
            for (uint32_t i = 0; i <= n; i++) if (labels[i] >= d->nctrls) { free(labels); INVALID("unknown label"); }
            Ctrl *dc = &d->ctrls[d->nctrls - 1 - labels[n]];
            const uint8_t *dts;
            uint32_t arity;
            label_types(dc, &dts, &arity);
            for (uint32_t i = 0; i <= n; i++) {
                Ctrl *c = &d->ctrls[d->nctrls - 1 - labels[i]];
                const uint8_t *ts;
                uint32_t k;
                label_types(c, &ts, &k);
                if (k != arity) { free(labels); INVALID("type mismatch"); }
                /* each target is validated against the same values */
                uint8_t tmp[64];
                uint32_t got = 0;
                for (uint32_t j = k; j-- > 0;) { uint8_t t = pop_t(d, ts[j]); if (got < 64) tmp[got++] = t; }
                for (uint32_t j = got; j-- > 0;) push(d, tmp[j]);
                if (got < k) { free(labels); INVALID("br_table with too many values"); }
            }
            if (em) {
                size_t at = emit_op(d, OP_BR_TABLE);
                emit_raw(d, n);
                emit_raw(d, arity);
                uint32_t cur = d->nlocals + d->nvals;
                for (uint32_t i = 0; i <= n; i++) {
                    Ctrl *c = &d->ctrls[d->nctrls - 1 - labels[i]];
                    size_t cell = emit_raw(d, 0);
                    emit_raw(d, d->nlocals + c->height);
                    if (c->kind == 2) d->code[cell].i = (int64_t)c->loop_pc - (int64_t)at;
                    else add_patch(d, c, at, cell);
                }
                (void)cur;
            }
            free(labels);
            pop_list(d, dts, arity);
            unreachable(d);
            break;
        }
        case 0x0f:                                       /* return */
            pop_list(d, ft->results, ft->nresults);
            if (em) {
                push_list(d, ft->results, ft->nresults);
                emit_return(d);
            }
            unreachable(d);
            break;
        case 0x10: {                                     /* call */
            uint32_t f = u32(d);
            if (f >= m->nfuncs) INVALID("unknown function %u", f);
            const WasmFuncType *t = m->functypes[f];
            pop_list(d, t->params, t->nparams);
            push_list(d, t->results, t->nresults);
            if (em) { emit_op(d, OP_CALL); emit_raw(d, f); }
            break;
        }
        case 0x11: {                                     /* call_indirect */
            uint32_t ti = u32(d);
            uint32_t tab = u32(d);
            if (tab >= m->ntables) INVALID("unknown table %u", tab);
            if (ti >= m->ntypes) INVALID("unknown type %u", ti);
            if (m->tables[tab].type != WASM_FUNCREF) INVALID("type mismatch");
            const WasmFuncType *t = m->types[ti];
            pop_t(d, WASM_I32);
            pop_list(d, t->params, t->nparams);
            push_list(d, t->results, t->nresults);
            if (em) { emit_op(d, OP_CALL_INDIRECT); emit_raw(d, tab); emit_raw(d, (uint64_t)(uintptr_t)t); }
            break;
        }
        case 0x1a:                                       /* drop */
            pop(d);
            if (em) emit_op(d, OP_DROP);
            break;
        case 0x1b: case 0x1c: {                          /* select */
            uint8_t want = UNKNOWN;
            if (op == 0x1c) {
                if (u32(d) != 1) INVALID("invalid result arity");
                want = valtype(d);
            }
            pop_t(d, WASM_I32);
            uint8_t b = pop_t(d, want), a = pop_t(d, want);
            if (op == 0x1b) {
                if ((a != UNKNOWN && !is_num(a)) || (b != UNKNOWN && !is_num(b))) INVALID("type mismatch");
                if (a != b && a != UNKNOWN && b != UNKNOWN) INVALID("type mismatch");
                want = a != UNKNOWN ? a : b;
            }
            push(d, want);
            if (em) emit_op(d, OP_SELECT);
            break;
        }
        case 0x20: case 0x21: case 0x22: {               /* local.get/set/tee */
            uint32_t i = u32(d);
            if (i >= d->nlocals) INVALID("unknown local %u", i);
            uint8_t t = d->locals[i];
            if (op == 0x20) push(d, t);
            else { pop_t(d, t); if (op == 0x22) push(d, t); }
            if (em) { emit_op(d, byte2op[op]); emit_raw(d, i); }
            break;
        }
        case 0x23: case 0x24: {                          /* global.get/set */
            uint32_t g = u32(d);
            if (g >= m->nglobals) INVALID("unknown global %u", g);
            uint8_t t = m->globals[g].type;
            if (op == 0x23) push(d, t);
            else {
                if (!m->globals[g].mut) INVALID("global is immutable");
                pop_t(d, t);
            }
            if (em) { emit_op(d, byte2op[op]); emit_raw(d, g); }
            break;
        }
        case 0x25: case 0x26: {                          /* table.get/set */
            uint32_t t = table_idx(d);
            uint8_t et = m->tables[t].type;
            if (op == 0x25) { pop_t(d, WASM_I32); push(d, et); }
            else { pop_t(d, et); pop_t(d, WASM_I32); }
            if (em) { emit_op(d, byte2op[op]); emit_raw(d, t); }
            break;
        }
        case 0x28: case 0x29: case 0x2a: case 0x2b: case 0x2c: case 0x2d: case 0x2e: case 0x2f:
        case 0x30: case 0x31: case 0x32: case 0x33: case 0x34: case 0x35: {    /* loads */
            static const uint8_t nat[] = { 4, 8, 4, 8, 1, 1, 2, 2, 1, 1, 2, 2, 4, 4 };
            static const uint8_t res[] = { WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_I32, WASM_I32, WASM_I32,
                                           WASM_I32, WASM_I64, WASM_I64, WASM_I64, WASM_I64, WASM_I64, WASM_I64 };
            uint32_t off;
            memarg(d, nat[op - 0x28], &off);
            pop_t(d, WASM_I32);
            push(d, res[op - 0x28]);
            if (em) { emit_op(d, byte2op[op]); emit_raw(d, off); }
            break;
        }
        case 0x36: case 0x37: case 0x38: case 0x39: case 0x3a: case 0x3b: case 0x3c: case 0x3d: case 0x3e: {
            static const uint8_t nat[] = { 4, 8, 4, 8, 1, 2, 1, 2, 4 };
            static const uint8_t ty[] = { WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_I32, WASM_I32, WASM_I64, WASM_I64, WASM_I64 };
            uint32_t off;
            memarg(d, nat[op - 0x36], &off);
            pop_t(d, ty[op - 0x36]);
            pop_t(d, WASM_I32);
            if (em) { emit_op(d, byte2op[op]); emit_raw(d, off); }
            break;
        }
        case 0x3f: case 0x40:                            /* memory.size/grow */
            if (u8(d) != 0) MALFORMED("zero byte expected");
            if (!m->nmems) INVALID("unknown memory 0");
            if (op == 0x40) pop_t(d, WASM_I32);
            push(d, WASM_I32);
            if (em) emit_op(d, byte2op[op]);
            break;
        case 0x41: case 0x42: case 0x43: case 0x44: {    /* constants */
            uint64_t v = 0;
            uint8_t t;
            if (op == 0x41) { v = (uint64_t)(uint32_t)(int32_t)leb(d, 32, 1); t = WASM_I32; }
            else if (op == 0x42) { v = leb(d, 64, 1); t = WASM_I64; }
            else if (op == 0x43) { uint32_t b; memcpy(&b, bytes(d, 4), 4); v = b; t = WASM_F32; }
            else { memcpy(&v, bytes(d, 8), 8); t = WASM_F64; }
            push(d, t);
            if (em) { emit_op(d, OP_CONST); emit_raw(d, v); }
            break;
        }
        case 0xd0: {                                     /* ref.null */
            uint8_t t = reftype(d);
            push(d, t);
            if (em) { emit_op(d, OP_CONST); emit_raw(d, 0); }
            break;
        }
        case 0xd1: {                                     /* ref.is_null */
            uint8_t t = pop(d);
            if (t != UNKNOWN && !is_ref(t)) INVALID("type mismatch");
            push(d, WASM_I32);
            if (em) emit_op(d, OP_REF_IS_NULL);
            break;
        }
        case 0xd2: {                                     /* ref.func */
            uint32_t f = u32(d);
            if (f >= m->nfuncs) INVALID("unknown function %u", f);
            if (!m->declared[f]) INVALID("undeclared function reference");
            push(d, WASM_FUNCREF);
            if (em) { emit_op(d, OP_REF_FUNC); emit_raw(d, f); }
            break;
        }
        case 0xfc: {
            uint32_t sub = u32(d);
            if (sub <= 7) {
                static const uint8_t in[] = { WASM_F32, WASM_F32, WASM_F64, WASM_F64, WASM_F32, WASM_F32, WASM_F64, WASM_F64 };
                pop_t(d, in[sub]);
                push(d, sub < 4 ? WASM_I32 : WASM_I64);
                if (em) emit_op(d, byte2op[0x100 + sub]);
                break;
            }
            switch (sub) {
            case 8: {                                    /* memory.init */
                uint32_t seg = u32(d);
                if (u8(d) != 0) MALFORMED("zero byte expected");
                if (!m->nmems) INVALID("unknown memory 0");
                if (!m->has_datacount) MALFORMED("data count section required");
                if (seg >= m->ndata) INVALID("unknown data segment %u", seg);
                pop_t(d, WASM_I32); pop_t(d, WASM_I32); pop_t(d, WASM_I32);
                if (em) { emit_op(d, OP_MEMORY_INIT); emit_raw(d, seg); }
                break;
            }
            case 9: {                                    /* data.drop */
                uint32_t seg = u32(d);
                if (!m->has_datacount) MALFORMED("data count section required");
                if (seg >= m->ndata) INVALID("unknown data segment %u", seg);
                if (em) { emit_op(d, OP_DATA_DROP); emit_raw(d, seg); }
                break;
            }
            case 10:                                     /* memory.copy */
                if (u8(d) != 0 || u8(d) != 0) MALFORMED("zero byte expected");
                if (!m->nmems) INVALID("unknown memory 0");
                pop_t(d, WASM_I32); pop_t(d, WASM_I32); pop_t(d, WASM_I32);
                if (em) emit_op(d, OP_MEMORY_COPY);
                break;
            case 11:                                     /* memory.fill */
                if (u8(d) != 0) MALFORMED("zero byte expected");
                if (!m->nmems) INVALID("unknown memory 0");
                pop_t(d, WASM_I32); pop_t(d, WASM_I32); pop_t(d, WASM_I32);
                if (em) emit_op(d, OP_MEMORY_FILL);
                break;
            case 12: {                                   /* table.init */
                uint32_t seg = u32(d);
                uint32_t t = table_idx(d);
                if (seg >= m->nelems) INVALID("unknown elem segment %u", seg);
                if (m->elems[seg].type != m->tables[t].type) INVALID("type mismatch");
                pop_t(d, WASM_I32); pop_t(d, WASM_I32); pop_t(d, WASM_I32);
                if (em) { emit_op(d, OP_TABLE_INIT); emit_raw(d, seg); emit_raw(d, t); }
                break;
            }
            case 13: {                                   /* elem.drop */
                uint32_t seg = u32(d);
                if (seg >= m->nelems) INVALID("unknown elem segment %u", seg);
                if (em) { emit_op(d, OP_ELEM_DROP); emit_raw(d, seg); }
                break;
            }
            case 14: {                                   /* table.copy */
                uint32_t dst = table_idx(d), src = table_idx(d);
                if (m->tables[dst].type != m->tables[src].type) INVALID("type mismatch");
                pop_t(d, WASM_I32); pop_t(d, WASM_I32); pop_t(d, WASM_I32);
                if (em) { emit_op(d, OP_TABLE_COPY); emit_raw(d, dst); emit_raw(d, src); }
                break;
            }
            case 15: case 16: case 17: {                 /* table.grow/size/fill */
                uint32_t t = table_idx(d);
                uint8_t et = m->tables[t].type;
                if (sub == 15) { pop_t(d, WASM_I32); pop_t(d, et); push(d, WASM_I32); }
                else if (sub == 16) push(d, WASM_I32);
                else { pop_t(d, WASM_I32); pop_t(d, et); pop_t(d, WASM_I32); }
                if (em) { emit_op(d, byte2op[0x100 + sub]); emit_raw(d, t); }
                break;
            }
            default: MALFORMED("illegal opcode 0xfc %u", sub);
            }
            break;
        }
        default:
            if (op >= 0x45 && op <= 0xc4) {
                uint8_t a, b, r;
                numsig(op, &a, &b, &r);
                if (!r) MALFORMED("illegal opcode 0x%02x", op);
                if (b) pop_t(d, b);
                pop_t(d, a);
                push(d, r);
                /* i32.wrap_i64 and the reinterprets don't change the stored bits */
                int noop = op == 0xa7 || (op >= 0xbc && op <= 0xbf);
                if (em && !noop) emit_op(d, byte2op[op]);
                break;
            }
            MALFORMED("illegal opcode 0x%02x", op);
        }
    }
    if (d->p != d->end) MALFORMED("section size mismatch");
}

/* ---- module ---- */

static void module_free(WasmModule *m) {
    if (!m) return;
    for (uint32_t i = 0; i < m->nimports; i++) { free(m->imports[i].module); free(m->imports[i].name); }
    free(m->imports);
    for (uint32_t i = 0; i < m->nexports; i++) free(m->exports[i].name);
    free(m->exports);
    if (m->code) for (uint32_t i = 0; i < m->nfuncs - m->nfunc_imports; i++) free(m->code[i].code);
    free(m->code);
    for (uint32_t i = 0; i < m->nelems; i++) free(m->elems[i].items);
    free(m->elems);
    for (uint32_t i = 0; i < m->ncustom; i++) free((char *)m->custom[i].name);
    free(m->custom);
    free(m->types);
    free(m->functypes);
    free(m->functypeidx);
    free(m->tables);
    free(m->globals);
    free(m->data);
    free(m->declared);
    free(m->bytes);
    free(m);
}

void wasm_module_ref(WasmModule *m) { m->rc++; }
void wasm_module_unref(WasmModule *m) { if (m && --m->rc == 0) module_free(m); }

static void sec_types(Dec *d) {
    WasmModule *m = d->m;
    m->ntypes = u32(d);
    if (m->ntypes > 1000000) MALFORMED("too many types");
    m->types = xalloc(d, (size_t)m->ntypes * sizeof *m->types);
    for (uint32_t i = 0; i < m->ntypes; i++) {
        if (u8(d) != 0x60) MALFORMED("integer representation too long");
        uint8_t tmp[1024];
        uint32_t np = u32(d);
        if (np > 1000) MALFORMED("too many parameters");
        for (uint32_t k = 0; k < np; k++) tmp[k] = valtype(d);
        uint32_t nr = u32(d);
        if (nr > 1000 || np + nr > sizeof tmp) MALFORMED("too many results");
        for (uint32_t k = 0; k < nr; k++) tmp[np + k] = valtype(d);
        m->types[i] = wasm_functype(tmp, np, tmp + np, nr);
        if (!m->types[i]) fail(d, WASM_ERR_NOMEM, "out of memory");
    }
}

static void add_func(Dec *d, uint32_t typeidx) {
    WasmModule *m = d->m;
    if (typeidx >= m->ntypes) INVALID("unknown type %u", typeidx);
    uint32_t cap = m->nfuncs;      /* allocated in blocks */
    if ((cap & (cap - 1)) == 0 || cap == 0) {
        uint32_t nc = cap ? cap * 2 : 16;
        void *a = realloc(m->functypes, nc * sizeof *m->functypes);
        if (!a) fail(d, WASM_ERR_NOMEM, "out of memory");
        m->functypes = a;
        a = realloc(m->functypeidx, nc * sizeof *m->functypeidx);
        if (!a) fail(d, WASM_ERR_NOMEM, "out of memory");
        m->functypeidx = a;
    }
    m->functypes[m->nfuncs] = m->types[typeidx];
    m->functypeidx[m->nfuncs++] = typeidx;
}

static void add_table(Dec *d, uint8_t type, uint32_t min, uint32_t max) {
    WasmModule *m = d->m;
    void *a = realloc(m->tables, (m->ntables + 1) * sizeof *m->tables);
    if (!a) fail(d, WASM_ERR_NOMEM, "out of memory");
    m->tables = a;
    m->tables[m->ntables++] = (TableDef){ type, min, max };
}

static void add_global(Dec *d, uint8_t type, uint8_t mut, ConstExpr init) {
    WasmModule *m = d->m;
    void *a = realloc(m->globals, (m->nglobals + 1) * sizeof *m->globals);
    if (!a) fail(d, WASM_ERR_NOMEM, "out of memory");
    m->globals = a;
    m->globals[m->nglobals++] = (GlobalDef){ type, mut, init };
}

static void add_memory(Dec *d, uint32_t min, uint32_t max) {
    WasmModule *m = d->m;
    if (m->nmems) INVALID("multiple memories");
    m->nmems = 1;
    m->mem_min = min;
    m->mem_max = max;
}

static void sec_imports(Dec *d) {
    WasmModule *m = d->m;
    uint32_t n = u32(d);
    if (n > 100000) MALFORMED("too many imports");
    m->imports = xalloc(d, (size_t)n * sizeof *m->imports);
    for (uint32_t i = 0; i < n; i++) {
        Import *im = &m->imports[i];
        im->module = name(d, &im->module_len);
        m->nimports = i + 1;
        im->name = name(d, &im->name_len);
        uint8_t kind = u8(d);
        im->type.kind = kind;
        switch (kind) {
        case 0: {
            uint32_t t = u32(d);
            add_func(d, t);
            im->typeidx = t;
            im->type.func = m->types[t];
            m->nfunc_imports++;
            break;
        }
        case 1: {
            uint8_t t = reftype(d);
            uint32_t min, max;
            limits(d, &min, &max, 0, "table");
            add_table(d, t, min, max);
            im->type.valtype = t; im->type.min = min; im->type.max = max;
            m->ntable_imports++;
            break;
        }
        case 2: {
            uint32_t min, max;
            limits(d, &min, &max, 65536, "memory");
            add_memory(d, min, max);
            im->type.min = min; im->type.max = max;
            m->nmem_imports++;
            break;
        }
        case 3: {
            uint8_t t = valtype(d), mut = u8(d);
            if (mut > 1) MALFORMED("malformed mutability");
            add_global(d, t, mut, (ConstExpr){ 0 });
            im->type.valtype = t; im->type.mut = mut;
            m->nglobal_imports++;
            break;
        }
        default: MALFORMED("malformed import kind");
        }
    }
}

static void sec_functions(Dec *d) {
    uint32_t n = u32(d);
    if (n > 1000000) MALFORMED("too many functions");
    for (uint32_t i = 0; i < n; i++) add_func(d, u32(d));
}

static void sec_tables(Dec *d) {
    uint32_t n = u32(d);
    for (uint32_t i = 0; i < n; i++) {
        uint8_t t = reftype(d);
        uint32_t min, max;
        limits(d, &min, &max, 0, "table");
        add_table(d, t, min, max);
    }
}

static void sec_memories(Dec *d) {
    uint32_t n = u32(d);
    for (uint32_t i = 0; i < n; i++) {
        uint32_t min, max;
        limits(d, &min, &max, 65536, "memory");
        add_memory(d, min, max);
    }
}

static void sec_globals(Dec *d) {
    uint32_t n = u32(d);
    for (uint32_t i = 0; i < n; i++) {
        uint8_t t = valtype(d), mut = u8(d);
        if (mut > 1) MALFORMED("malformed mutability");
        ConstExpr e = const_expr(d, t);
        add_global(d, t, mut, e);
    }
}

static void sec_exports(Dec *d) {
    WasmModule *m = d->m;
    uint32_t n = u32(d);
    if (n > 100000) MALFORMED("too many exports");
    m->exports = xalloc(d, (size_t)n * sizeof *m->exports);
    for (uint32_t i = 0; i < n; i++) {
        Export *e = &m->exports[i];
        e->name = name(d, &e->len);
        m->nexports = i + 1;
        e->kind = u8(d);
        e->idx = u32(d);
        switch (e->kind) {
        case 0: if (e->idx >= m->nfuncs) INVALID("unknown function %u", e->idx); m->declared[e->idx] = 1; break;
        case 1: if (e->idx >= m->ntables) INVALID("unknown table %u", e->idx); break;
        case 2: if (e->idx >= m->nmems) INVALID("unknown memory %u", e->idx); break;
        case 3: if (e->idx >= m->nglobals) INVALID("unknown global %u", e->idx); break;
        default: MALFORMED("malformed export kind");
        }
        for (uint32_t k = 0; k < i; k++)
            if (m->exports[k].len == e->len && !memcmp(m->exports[k].name, e->name, e->len)) INVALID("duplicate export name");
    }
}

static void sec_start(Dec *d) {
    WasmModule *m = d->m;
    uint32_t f = u32(d);
    if (f >= m->nfuncs) INVALID("unknown function %u", f);
    if (m->functypes[f]->nparams || m->functypes[f]->nresults) INVALID("start function");
    m->start = f;
}

static void sec_elems(Dec *d) {
    WasmModule *m = d->m;
    uint32_t n = u32(d);
    if (n > 100000) MALFORMED("too many segments");
    m->elems = xalloc(d, (size_t)n * sizeof *m->elems);
    for (uint32_t i = 0; i < n; i++) {
        ElemSeg *s = &m->elems[i];
        m->nelems = i + 1;
        uint32_t flags = u32(d);
        if (flags > 7) MALFORMED("malformed elements segment kind");
        s->mode = (flags & 1) ? ((flags & 2) ? 2 : 1) : 0;
        s->type = WASM_FUNCREF;
        s->table = 0;
        if ((flags & 3) == 2) s->table = u32(d);
        if (s->mode == 0) {
            if (s->table >= m->ntables) INVALID("unknown table %u", s->table);
            s->offset = const_expr(d, WASM_I32);
        }
        int exprs = (flags & 4) != 0;
        if (flags & 3) {
            if (exprs) s->type = reftype(d);
            else if (u8(d) != 0x00) MALFORMED("malformed element kind");
        }
        s->n = u32(d);
        if (s->n > 10000000) MALFORMED("too many elements");
        s->items = xalloc(d, (size_t)s->n * sizeof *s->items);
        for (uint32_t k = 0; k < s->n; k++) {
            if (exprs) { s->items[k].is_expr = 1; s->items[k].expr = const_expr(d, s->type); }
            else {
                uint32_t f = u32(d);
                if (f >= m->nfuncs) INVALID("unknown function %u", f);
                m->declared[f] = 1;
                s->items[k].func = f;
            }
        }
        if (s->mode == 0 && m->tables[s->table].type != s->type) INVALID("type mismatch");
    }
}

static void sec_datacount(Dec *d) {
    d->m->ndata = u32(d);
    d->m->has_datacount = 1;
}

static void sec_code(Dec *d) {
    WasmModule *m = d->m;
    uint32_t n = u32(d);
    uint32_t ndef = m->nfuncs - m->nfunc_imports;
    if (n != ndef) MALFORMED("function and code section have inconsistent lengths");
    m->code = xalloc(d, (size_t)ndef * sizeof *m->code);
    const uint8_t *outer_end = d->end;
    for (uint32_t i = 0; i < n; i++) {
        uint32_t size = u32(d);
        if ((size_t)(outer_end - d->p) < size) MALFORMED("unexpected end");
        d->end = d->p + size;
        compile_body(d, m->nfunc_imports + i);
        d->end = outer_end;
        WasmCode *c = &m->code[i];
        c->nlocals = d->nlocals;
        c->max_stack = d->nlocals + d->maxvals + 4;
        c->code = malloc(d->ncode * sizeof *c->code);
        if (!c->code) fail(d, WASM_ERR_NOMEM, "out of memory");
        memcpy(c->code, d->code, d->ncode * sizeof *c->code);
        c->ncode = d->ncode;
    }
}

static void sec_data(Dec *d) {
    WasmModule *m = d->m;
    uint32_t n = u32(d);
    if (m->has_datacount && n != m->ndata) MALFORMED("data count and data section have inconsistent lengths");
    if (n > 1000000) MALFORMED("too many segments");
    m->ndata = n;
    m->data = xalloc(d, (size_t)n * sizeof *m->data);
    for (uint32_t i = 0; i < n; i++) {
        DataSeg *s = &m->data[i];
        uint32_t flags = u32(d);
        if (flags > 2) MALFORMED("malformed data segment kind");
        if (flags != 1) {
            if (flags == 2 && u32(d) != 0) INVALID("unknown memory");
            if (!m->nmems) INVALID("unknown memory 0");
            s->active = 1;
            s->offset = const_expr(d, WASM_I32);
        }
        s->len = u32(d);
        s->bytes = bytes(d, s->len);
    }
}

static void sec_custom(Dec *d) {
    WasmModule *m = d->m;
    char *nm = name(d, NULL);
    void *a = realloc(m->custom, (m->ncustom + 1) * sizeof *m->custom);
    if (!a) { free(nm); fail(d, WASM_ERR_NOMEM, "out of memory"); }
    m->custom = a;
    m->custom[m->ncustom++] = (Custom){ nm, d->p, (size_t)(d->end - d->p) };
    d->p = d->end;
}

static WasmModule *decode(const uint8_t *in, size_t len, WasmError *err) {
    wasm_init_labels();
    init_byte2op();
    Dec *d = calloc(1, sizeof *d);
    WasmModule *m = calloc(1, sizeof *m);
    if (!d || !m) { free(d); free(m); wasm_err(err, WASM_ERR_NOMEM, "out of memory"); return NULL; }
    if (err) err->kind = WASM_OK;
    m->rc = 1;
    m->start = -1;
    d->err = err;
    d->m = m;
    if (setjmp(d->jb)) {
        free(d->vals); free(d->ctrls); free(d->code); free(d->patches); free(d->locals);
        free(d);
        module_free(m);
        return NULL;
    }
    m->bytes = xalloc(d, len);
    memcpy(m->bytes, in, len);
    m->len = len;
    d->p = m->bytes;
    d->end = m->bytes + len;
    if (len < 4 || memcmp(d->p, "\0asm", 4)) MALFORMED("magic header not detected");
    d->p += 4;
    if (len < 8 || memcmp(d->p, "\1\0\0\0", 4)) MALFORMED("unknown binary version");
    d->p += 4;
    int last = 0;
    static const int order[] = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 10 };   /* datacount (12) goes before code */
    const uint8_t *file_end = d->end;
    int has_code = 0;
    while (d->p < file_end) {
        uint8_t id = u8(d);
        uint32_t size = u32(d);
        if ((size_t)(file_end - d->p) < size) MALFORMED("length out of bounds");
        d->end = d->p + size;
        if (id > 12) MALFORMED("malformed section id");
        if (id) {
            if (order[id] <= last) MALFORMED("unexpected content after last section");
            last = order[id];
        }
        /* functions ref.func may name: the function list is already complete */
        if (id >= 4 && !m->declared) m->declared = xalloc(d, (size_t)m->nfuncs + 1);
        switch (id) {
        case 0: sec_custom(d); break;
        case 1: sec_types(d); break;
        case 2: sec_imports(d); break;
        case 3: sec_functions(d); break;
        case 4: sec_tables(d); break;
        case 5: sec_memories(d); break;
        default:
            if (id == 6) sec_globals(d);
            else if (id == 7) sec_exports(d);
            else if (id == 8) sec_start(d);
            else if (id == 9) sec_elems(d);
            else if (id == 10) { sec_code(d); has_code = 1; }
            else if (id == 11) sec_data(d);
            else sec_datacount(d);
            break;
        }
        if (d->p != d->end) MALFORMED("section size mismatch");
        d->end = file_end;
    }
    if (!has_code && m->nfuncs != m->nfunc_imports) MALFORMED("function and code section have inconsistent lengths");
    if (m->has_datacount && !m->data && m->ndata) MALFORMED("data count and data section have inconsistent lengths");
    free(d->vals); free(d->ctrls); free(d->code); free(d->patches); free(d->locals);
    free(d);
    return m;
}

WasmModule *wasm_module_new(const uint8_t *bytes, size_t len, WasmError *err) {
    return decode(bytes, len, err);
}

int wasm_validate(const uint8_t *bytes, size_t len, WasmError *err) {
    WasmModule *m = decode(bytes, len, err);
    if (!m) return -1;
    module_free(m);
    return 0;
}

uint32_t wasm_module_num_imports(const WasmModule *m) { return m->nimports; }
const char *wasm_import_module(const WasmModule *m, uint32_t i, size_t *len) {
    if (i >= m->nimports) return NULL;
    if (len) *len = m->imports[i].module_len;
    return m->imports[i].module;
}
const char *wasm_import_name(const WasmModule *m, uint32_t i, size_t *len) {
    if (i >= m->nimports) return NULL;
    if (len) *len = m->imports[i].name_len;
    return m->imports[i].name;
}
WasmExternType wasm_import_type(const WasmModule *m, uint32_t i) {
    return i < m->nimports ? m->imports[i].type : (WasmExternType){ .kind = -1 };
}
uint32_t wasm_module_num_exports(const WasmModule *m) { return m->nexports; }
const char *wasm_export_name(const WasmModule *m, uint32_t i, size_t *len) {
    if (i >= m->nexports) return NULL;
    if (len) *len = m->exports[i].len;
    return m->exports[i].name;
}

WasmExternType wasm_export_type(const WasmModule *m, uint32_t i) {
    WasmExternType t = { .kind = -1 };
    if (i >= m->nexports) return t;
    const Export *e = &m->exports[i];
    t.kind = e->kind;
    switch (e->kind) {
    case 0: t.func = m->functypes[e->idx]; break;
    case 1: t.valtype = m->tables[e->idx].type; t.min = m->tables[e->idx].min; t.max = m->tables[e->idx].max; break;
    case 2: t.min = m->mem_min; t.max = m->mem_max; break;
    case 3: t.valtype = m->globals[e->idx].type; t.mut = m->globals[e->idx].mut; break;
    }
    return t;
}

const uint8_t *wasm_custom_section(const WasmModule *m, const char *nm, uint32_t n, size_t *len) {
    for (uint32_t i = 0; i < m->ncustom; i++)
        if (!strcmp(m->custom[i].name, nm) && n-- == 0) { *len = m->custom[i].len; return m->custom[i].p; }
    return NULL;
}
