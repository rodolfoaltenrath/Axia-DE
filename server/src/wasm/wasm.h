/* wasm.h — LordOS's own WebAssembly interpreter (no dependencies; see docs/WASM.md).
 *
 * Supports WebAssembly 2.0 without SIMD: the MVP plus sign-extension, non-trapping float-to-int,
 * multi-value, bulk memory, reference types and mutable globals. A single 32-bit memory.
 *
 * Usage:
 *   WasmModule *m = wasm_module_new(bytes, len, &err);          // decodes, validates and compiles
 *   WasmExtern imports[...] = ...;                               // in wasm_import_* order
 *   WasmInstance *inst = wasm_instantiate(m, imports, n, &err);  // runs the start function
 *   WasmExtern e = wasm_instance_export_find(inst, "add");
 *   WasmVal args[2] = { { .i32 = 2 }, { .i32 = 3 } }, res[1];
 *   if (wasm_call(e.func, args, res, &err)) ...                  // trap or host error
 *
 * Reference-counted objects: module, instance, function (an instance's functions count
 * on their instance), memory, table and global. Everything returned by *_new or wasm_instantiate
 * starts with one reference; wasm_extern_ref/unref for those from exports. Single-threaded. */
#ifndef LORD_WASM_H
#define LORD_WASM_H

#include <stddef.h>
#include <stdint.h>

enum {
    WASM_I32 = 0x7f, WASM_I64 = 0x7e, WASM_F32 = 0x7d, WASM_F64 = 0x7c,
    WASM_FUNCREF = 0x70, WASM_EXTERNREF = 0x6f,
};
enum { WASM_EXT_FUNC = 0, WASM_EXT_TABLE = 1, WASM_EXT_MEMORY = 2, WASM_EXT_GLOBAL = 3 };

#define WASM_PAGE 65536u
#define WASM_NO_MAX UINT32_MAX

/* A value. f32 and f64 keep their bits as-is (NaNs included). funcref = WasmFunc * (NULL
 * is ref.null); externref = opaque host value (0 is ref.null). */
typedef union {
    int32_t i32; uint32_t u32;
    int64_t i64; uint64_t u64;
    float f32; double f64;
    void *ref;
} WasmVal;

typedef enum {
    WASM_OK = 0,
    WASM_ERR_MALFORMED,                 /* the binary can't be decoded */
    WASM_ERR_INVALID,                   /* doesn't validate (types, indices...) */
    WASM_ERR_LINK,                      /* imports that don't match */
    WASM_ERR_TRAP,                      /* trap while running */
    WASM_ERR_EXHAUSTED,                 /* call stack exhausted */
    WASM_ERR_HOST,                      /* a host function failed (e.g. a JS exception) */
    WASM_ERR_NOMEM,
} WasmErrKind;

typedef struct {
    WasmErrKind kind;
    char msg[160];
} WasmError;

/* Fills err (for host functions). */
void wasm_err(WasmError *err, WasmErrKind kind, const char *fmt, ...) __attribute__((format(printf, 3, 4)));

/* Function type (interned: two equal types are the same pointer). */
typedef struct WasmFuncType {
    uint32_t nparams, nresults;
    const uint8_t *params, *results;
} WasmFuncType;

typedef struct WasmModule WasmModule;
typedef struct WasmInstance WasmInstance;
typedef struct WasmFunc WasmFunc;
typedef struct WasmMemory WasmMemory;
typedef struct WasmTable WasmTable;
typedef struct WasmGlobal WasmGlobal;

typedef struct {
    int kind;                           /* WASM_EXT_* */
    union { WasmFunc *func; WasmTable *table; WasmMemory *memory; WasmGlobal *global; void *ptr; };
} WasmExtern;

/* Description of an import or export. */
typedef struct {
    int kind;
    const WasmFuncType *func;           /* function */
    uint8_t valtype;                    /* global: type; table: element type */
    int mut;                            /* global */
    uint32_t min, max;                  /* table and memory (pages); max = WASM_NO_MAX if none */
} WasmExternType;

const WasmFuncType *wasm_functype(const uint8_t *params, uint32_t np, const uint8_t *results, uint32_t nr);

/* ---- modules ---- */
WasmModule *wasm_module_new(const uint8_t *bytes, size_t len, WasmError *err);
int  wasm_validate(const uint8_t *bytes, size_t len, WasmError *err);   /* 0 if valid */
void wasm_module_ref(WasmModule *m);
void wasm_module_unref(WasmModule *m);
uint32_t wasm_module_num_imports(const WasmModule *m);
/* Names are 0-terminated but may contain 0 bytes: len (if not NULL) gives the length. */
const char *wasm_import_module(const WasmModule *m, uint32_t i, size_t *len);
const char *wasm_import_name(const WasmModule *m, uint32_t i, size_t *len);
WasmExternType wasm_import_type(const WasmModule *m, uint32_t i);
uint32_t wasm_module_num_exports(const WasmModule *m);
const char *wasm_export_name(const WasmModule *m, uint32_t i, size_t *len);
WasmExternType wasm_export_type(const WasmModule *m, uint32_t i);
/* Custom sections with that name (WebAssembly.Module.customSections): the n-th one, or NULL. */
const uint8_t *wasm_custom_section(const WasmModule *m, const char *name, uint32_t n, size_t *len);

/* ---- instances ---- */
/* imports in wasm_import_* order; each type is validated (LINK if it doesn't match). If the
 * start function (or an active segment) traps, returns NULL with kind TRAP. */
WasmInstance *wasm_instantiate(WasmModule *m, const WasmExtern *imports, uint32_t nimports, WasmError *err);
void wasm_instance_ref(WasmInstance *i);
void wasm_instance_unref(WasmInstance *i);
WasmModule *wasm_instance_module(const WasmInstance *i);
WasmExtern wasm_instance_export(const WasmInstance *i, uint32_t idx);      /* without adding a reference */
WasmExtern wasm_instance_export_find(const WasmInstance *i, const char *name);  /* kind -1 if not found */
WasmExtern wasm_instance_export_find_len(const WasmInstance *i, const char *name, size_t len);

/* ---- functions ---- */
/* Host function: reads args (nparams) and writes results (nresults). Returns 0, or an error
 * (kind TRAP or HOST) that aborts the wasm call in progress. */
typedef int (*WasmHostFn)(void *ud, const WasmVal *args, WasmVal *results, WasmError *err);
WasmFunc *wasm_func_new_host(const WasmFuncType *type, WasmHostFn fn, void *ud, void (*free_ud)(void *));
const WasmFuncType *wasm_func_type(const WasmFunc *f);
int  wasm_func_is_host(const WasmFunc *f);
void *wasm_func_host_data(const WasmFunc *f);
void wasm_func_ref(WasmFunc *f);
void wasm_func_unref(WasmFunc *f);
/* A free pointer for the host (the JS binding stores the function object there). */
void **wasm_func_slot(WasmFunc *f);
int  wasm_call(WasmFunc *f, const WasmVal *args, WasmVal *results, WasmError *err);

/* ---- memories (64 KiB pages; data doesn't move when growing) ---- */
WasmMemory *wasm_memory_new(uint32_t min, uint32_t max, WasmError *err);
void     wasm_memory_ref(WasmMemory *m);
void     wasm_memory_unref(WasmMemory *m);
uint8_t *wasm_memory_data(const WasmMemory *m);
size_t   wasm_memory_size(const WasmMemory *m);                 /* in bytes */
uint32_t wasm_memory_max(const WasmMemory *m);                  /* pages */
int64_t  wasm_memory_grow(WasmMemory *m, uint32_t delta);       /* previous pages, or -1 */
/* Notice that it grew (from wasm or the host), e.g. to detach the JS ArrayBuffer. */
void     wasm_memory_on_grow(WasmMemory *m, void (*fn)(void *ud, WasmMemory *m), void *ud);
/* Free pointers for the host (the JS binding stores the Memory object and its ArrayBuffer). */
void   **wasm_memory_slots(WasmMemory *m);                      /* 2 */

/* ---- tables ---- */
WasmTable *wasm_table_new(uint8_t elemtype, uint32_t min, uint32_t max, WasmVal init, WasmError *err);
void     wasm_table_ref(WasmTable *t);
void     wasm_table_unref(WasmTable *t);
uint8_t  wasm_table_elemtype(const WasmTable *t);
uint32_t wasm_table_size(const WasmTable *t);
uint32_t wasm_table_max(const WasmTable *t);
int      wasm_table_get(const WasmTable *t, uint32_t i, WasmVal *v);   /* 0 on success, -1 out of range */
int      wasm_table_set(WasmTable *t, uint32_t i, WasmVal v);
int64_t  wasm_table_grow(WasmTable *t, uint32_t delta, WasmVal init);
void   **wasm_table_slot(WasmTable *t);

/* ---- globals ---- */
WasmGlobal *wasm_global_new(uint8_t type, int mut, WasmVal v);
void    wasm_global_ref(WasmGlobal *g);
void    wasm_global_unref(WasmGlobal *g);
uint8_t wasm_global_type(const WasmGlobal *g);
int     wasm_global_mut(const WasmGlobal *g);
WasmVal wasm_global_get(const WasmGlobal *g);
void    wasm_global_set(WasmGlobal *g, WasmVal v);
void  **wasm_global_slot(WasmGlobal *g);

void wasm_extern_ref(WasmExtern e);
void wasm_extern_unref(WasmExtern e);

/* Limits (can be adjusted before the first call). */
extern uint32_t wasm_max_call_depth;    /* nested wasm call frames (default 20000) */
extern uint32_t wasm_max_memory_pages;  /* cap per memory (default 65536 = 4 GiB) */

#endif
