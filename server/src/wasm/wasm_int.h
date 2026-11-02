/* wasm_int.h — internal structures of the interpreter (wasm.c decodes/validates/compiles,
 * wasm_run.c instantiates and runs). */
#ifndef LORD_WASM_INT_H
#define LORD_WASM_INT_H

#include "wasm.h"

#include <stdint.h>

/* Compiled code: each instruction is the address of its handler (computed goto) followed
 * by its immediates. Jumps are relative to the start of the instruction. */
typedef union Cell {
    const void *op;
    uint64_t u;
    int64_t i;
} Cell;

/* Internal instructions. Those with byte >= 0 map 1 to 1 to a WebAssembly
 * instruction with the same meaning (0x100 + n: 0xFC prefix). */
#define WASM_OPS(X) \
    X(UNREACHABLE, 0x00) X(BR, -1) X(BR_N, -1) X(BR_IF, -1) X(BR_IF_N, -1) X(BR_UNLESS, -1) \
    X(BR_TABLE, -1) X(RETURN, -1) X(RETURN0, -1) X(RETURN1, -1) X(CALL, -1) X(CALL_INDIRECT, -1) \
    X(DROP, 0x1a) X(SELECT, 0x1b) \
    X(LOCAL_GET, 0x20) X(LOCAL_SET, 0x21) X(LOCAL_TEE, 0x22) X(GLOBAL_GET, 0x23) X(GLOBAL_SET, 0x24) \
    X(TABLE_GET, 0x25) X(TABLE_SET, 0x26) \
    X(I32_LOAD, 0x28) X(I64_LOAD, 0x29) X(F32_LOAD, 0x2a) X(F64_LOAD, 0x2b) \
    X(I32_LOAD8_S, 0x2c) X(I32_LOAD8_U, 0x2d) X(I32_LOAD16_S, 0x2e) X(I32_LOAD16_U, 0x2f) \
    X(I64_LOAD8_S, 0x30) X(I64_LOAD8_U, 0x31) X(I64_LOAD16_S, 0x32) X(I64_LOAD16_U, 0x33) \
    X(I64_LOAD32_S, 0x34) X(I64_LOAD32_U, 0x35) \
    X(I32_STORE, 0x36) X(I64_STORE, 0x37) X(F32_STORE, 0x38) X(F64_STORE, 0x39) \
    X(I32_STORE8, 0x3a) X(I32_STORE16, 0x3b) X(I64_STORE8, 0x3c) X(I64_STORE16, 0x3d) X(I64_STORE32, 0x3e) \
    X(MEMORY_SIZE, 0x3f) X(MEMORY_GROW, 0x40) X(CONST, -1) \
    X(I32_EQZ, 0x45) X(I32_EQ, 0x46) X(I32_NE, 0x47) X(I32_LT_S, 0x48) X(I32_LT_U, 0x49) \
    X(I32_GT_S, 0x4a) X(I32_GT_U, 0x4b) X(I32_LE_S, 0x4c) X(I32_LE_U, 0x4d) X(I32_GE_S, 0x4e) X(I32_GE_U, 0x4f) \
    X(I64_EQZ, 0x50) X(I64_EQ, 0x51) X(I64_NE, 0x52) X(I64_LT_S, 0x53) X(I64_LT_U, 0x54) \
    X(I64_GT_S, 0x55) X(I64_GT_U, 0x56) X(I64_LE_S, 0x57) X(I64_LE_U, 0x58) X(I64_GE_S, 0x59) X(I64_GE_U, 0x5a) \
    X(F32_EQ, 0x5b) X(F32_NE, 0x5c) X(F32_LT, 0x5d) X(F32_GT, 0x5e) X(F32_LE, 0x5f) X(F32_GE, 0x60) \
    X(F64_EQ, 0x61) X(F64_NE, 0x62) X(F64_LT, 0x63) X(F64_GT, 0x64) X(F64_LE, 0x65) X(F64_GE, 0x66) \
    X(I32_CLZ, 0x67) X(I32_CTZ, 0x68) X(I32_POPCNT, 0x69) X(I32_ADD, 0x6a) X(I32_SUB, 0x6b) \
    X(I32_MUL, 0x6c) X(I32_DIV_S, 0x6d) X(I32_DIV_U, 0x6e) X(I32_REM_S, 0x6f) X(I32_REM_U, 0x70) \
    X(I32_AND, 0x71) X(I32_OR, 0x72) X(I32_XOR, 0x73) X(I32_SHL, 0x74) X(I32_SHR_S, 0x75) \
    X(I32_SHR_U, 0x76) X(I32_ROTL, 0x77) X(I32_ROTR, 0x78) \
    X(I64_CLZ, 0x79) X(I64_CTZ, 0x7a) X(I64_POPCNT, 0x7b) X(I64_ADD, 0x7c) X(I64_SUB, 0x7d) \
    X(I64_MUL, 0x7e) X(I64_DIV_S, 0x7f) X(I64_DIV_U, 0x80) X(I64_REM_S, 0x81) X(I64_REM_U, 0x82) \
    X(I64_AND, 0x83) X(I64_OR, 0x84) X(I64_XOR, 0x85) X(I64_SHL, 0x86) X(I64_SHR_S, 0x87) \
    X(I64_SHR_U, 0x88) X(I64_ROTL, 0x89) X(I64_ROTR, 0x8a) \
    X(F32_ABS, 0x8b) X(F32_NEG, 0x8c) X(F32_CEIL, 0x8d) X(F32_FLOOR, 0x8e) X(F32_TRUNC, 0x8f) \
    X(F32_NEAREST, 0x90) X(F32_SQRT, 0x91) X(F32_ADD, 0x92) X(F32_SUB, 0x93) X(F32_MUL, 0x94) \
    X(F32_DIV, 0x95) X(F32_MIN, 0x96) X(F32_MAX, 0x97) X(F32_COPYSIGN, 0x98) \
    X(F64_ABS, 0x99) X(F64_NEG, 0x9a) X(F64_CEIL, 0x9b) X(F64_FLOOR, 0x9c) X(F64_TRUNC, 0x9d) \
    X(F64_NEAREST, 0x9e) X(F64_SQRT, 0x9f) X(F64_ADD, 0xa0) X(F64_SUB, 0xa1) X(F64_MUL, 0xa2) \
    X(F64_DIV, 0xa3) X(F64_MIN, 0xa4) X(F64_MAX, 0xa5) X(F64_COPYSIGN, 0xa6) \
    X(I32_WRAP_I64, 0xa7) X(I32_TRUNC_F32_S, 0xa8) X(I32_TRUNC_F32_U, 0xa9) X(I32_TRUNC_F64_S, 0xaa) \
    X(I32_TRUNC_F64_U, 0xab) X(I64_EXTEND_I32_S, 0xac) X(I64_EXTEND_I32_U, 0xad) \
    X(I64_TRUNC_F32_S, 0xae) X(I64_TRUNC_F32_U, 0xaf) X(I64_TRUNC_F64_S, 0xb0) X(I64_TRUNC_F64_U, 0xb1) \
    X(F32_CONVERT_I32_S, 0xb2) X(F32_CONVERT_I32_U, 0xb3) X(F32_CONVERT_I64_S, 0xb4) \
    X(F32_CONVERT_I64_U, 0xb5) X(F32_DEMOTE_F64, 0xb6) X(F64_CONVERT_I32_S, 0xb7) \
    X(F64_CONVERT_I32_U, 0xb8) X(F64_CONVERT_I64_S, 0xb9) X(F64_CONVERT_I64_U, 0xba) \
    X(F64_PROMOTE_F32, 0xbb) \
    X(I32_EXTEND8_S, 0xc0) X(I32_EXTEND16_S, 0xc1) X(I64_EXTEND8_S, 0xc2) X(I64_EXTEND16_S, 0xc3) \
    X(I64_EXTEND32_S, 0xc4) \
    X(REF_IS_NULL, 0xd1) X(REF_FUNC, 0xd2) \
    X(I32_TRUNC_SAT_F32_S, 0x100) X(I32_TRUNC_SAT_F32_U, 0x101) X(I32_TRUNC_SAT_F64_S, 0x102) \
    X(I32_TRUNC_SAT_F64_U, 0x103) X(I64_TRUNC_SAT_F32_S, 0x104) X(I64_TRUNC_SAT_F32_U, 0x105) \
    X(I64_TRUNC_SAT_F64_S, 0x106) X(I64_TRUNC_SAT_F64_U, 0x107) \
    X(MEMORY_INIT, 0x108) X(DATA_DROP, 0x109) X(MEMORY_COPY, 0x10a) X(MEMORY_FILL, 0x10b) \
    X(TABLE_INIT, 0x10c) X(ELEM_DROP, 0x10d) X(TABLE_COPY, 0x10e) X(TABLE_GROW, 0x10f) \
    X(TABLE_SIZE, 0x110) X(TABLE_FILL, 0x111)

enum {
#define X(name, byte) OP_##name,
    WASM_OPS(X)
#undef X
    OP__COUNT
};

/* Handler addresses (filled in by wasm_run.c). */
extern const void *const *wasm_op_labels;
void wasm_init_labels(void);

typedef struct {
    uint32_t nlocals;                   /* parameters included */
    uint32_t max_stack;                 /* nlocals + maximum operand stack height */
    Cell *code;
    size_t ncode;
} WasmCode;

typedef struct {
    const uint8_t *p;                   /* constant expression (up to its `end`) */
    size_t len;
} ConstExpr;

typedef struct {
    uint8_t is_expr;
    uint32_t func;                      /* if not an expression */
    ConstExpr expr;
} ElemItem;

typedef struct {
    uint8_t mode;                       /* 0 active, 1 passive, 2 declarative */
    uint8_t type;                       /* funcref / externref */
    uint32_t table;
    ConstExpr offset;
    uint32_t n;
    ElemItem *items;
} ElemSeg;

typedef struct {
    uint8_t active;
    ConstExpr offset;
    const uint8_t *bytes;
    uint32_t len;
} DataSeg;

typedef struct {
    char *module, *name;
    uint32_t module_len, name_len;      /* names may contain 0 bytes */
    WasmExternType type;
    uint32_t typeidx;
} Import;

typedef struct {
    char *name;
    uint32_t len;
    int kind;
    uint32_t idx;
} Export;

typedef struct {
    uint8_t type;
    uint8_t mut;
    ConstExpr init;
} GlobalDef;

typedef struct {
    uint8_t type;
    uint32_t min, max;
} TableDef;

typedef struct {
    const char *name;
    const uint8_t *p;
    size_t len;
} Custom;

struct WasmModule {
    int rc;
    uint8_t *bytes;                     /* copy of the binary (segments point here) */
    size_t len;
    uint32_t ntypes;
    const WasmFuncType **types;
    uint32_t nimports, nfunc_imports, ntable_imports, nmem_imports, nglobal_imports;
    Import *imports;
    uint32_t nfuncs;                    /* imports included */
    const WasmFuncType **functypes;     /* by function index */
    uint32_t *functypeidx;
    WasmCode *code;                     /* of the defined ones (nfuncs - nfunc_imports) */
    uint32_t ntables;
    TableDef *tables;                   /* imports included */
    uint32_t nmems;
    uint32_t mem_min, mem_max;
    uint32_t nglobals;
    GlobalDef *globals;                 /* imports included (empty init) */
    uint32_t nexports;
    Export *exports;
    int64_t start;                      /* -1 if none */
    uint32_t nelems;
    ElemSeg *elems;
    uint32_t ndata;
    int has_datacount;
    DataSeg *data;
    uint8_t *declared;                  /* functions ref.func may name */
    uint32_t ncustom;
    Custom *custom;
};

struct WasmFunc {
    const WasmFuncType *type;
    WasmInstance *inst;                 /* NULL if it's the host's */
    const WasmCode *code;
    WasmHostFn host;
    void *ud;
    void (*free_ud)(void *);
    int rc;                             /* host ones only */
    void *slot;
};

struct WasmMemory {
    int rc;
    uint8_t *data;                      /* reserved up to max (PROT_NONE beyond size) */
    size_t size;                        /* accessible bytes */
    size_t reserved;
    uint32_t max;
    void (*on_grow)(void *, WasmMemory *);
    void *on_grow_ud;
    void *slot[2];
};

struct WasmTable {
    int rc;                             /* outside references (JS, other instances) */
    WasmInstance *owner;                /* the instance that defined it (or NULL) */
    uint8_t type;
    uint32_t size, max;
    WasmVal *elems;
    void *slot;
};

struct WasmGlobal {
    int rc;
    WasmInstance *owner;
    uint8_t type, mut;
    WasmVal val;
    void *slot;
};

struct WasmInstance {
    int rc;
    WasmModule *m;
    uint32_t nfuncs;
    WasmFunc **funcs;                   /* by index (imports point to foreign functions) */
    WasmFunc *own;                      /* the defined ones */
    WasmTable **tables;
    WasmMemory *mem;
    WasmGlobal **globals;
    uint8_t *elem_dropped, *data_dropped;
    WasmExtern *exports;
};

/* Retain / release a funcref stored in an object (table or global) of `owner`: functions
 * of the same owning instance don't add a reference (avoids the instance ↔ its table cycle). */
void wasm_ref_hold(WasmInstance *owner, uint8_t type, WasmVal v);
void wasm_ref_release(WasmInstance *owner, uint8_t type, WasmVal v);

/* Evaluates an already validated constant expression. */
WasmVal wasm_const_eval(const WasmInstance *inst, ConstExpr e);


#endif
