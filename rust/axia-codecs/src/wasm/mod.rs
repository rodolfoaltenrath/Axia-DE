//! LordOS's own WebAssembly interpreter (no dependencies; docs/WASM.md, docs/RUST-WASM.md). C ABI
//! of server/src/wasm/wasm.h: WebAssembly 2.0 without SIMD, a single 32-bit memory.
//!
//! This is a faithful port of the former wasm.c (decoder, validator and compiler) and wasm_run.c
//! (instances and the interpreter). The objects the C side holds (modules, instances, functions,
//! memories, tables, globals) are heap allocations behind raw pointers with the same reference
//! counting, because their identity matters: function types are interned and compared by pointer,
//! a funcref is the address of its function, and an instance and its own tables or globals keep
//! each other alive through a rule that avoids the cycle (`ref_hold`). Everything that reads
//! untrusted bytes (the decoder) is bounds-checked Rust; the interpreter runs validated code.
// The objects behind the raw pointers are valid for as long as C holds a reference, and the interpreter
// is single-threaded by contract (wasm.h), so the implicit reborrows of `(*ptr).field[..]` are sound.
#![allow(dangerous_implicit_autorefs)]

use core::ffi::{c_char, c_int, c_void};
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

mod decode;
mod exec;

pub(crate) use exec::exec_init;

pub const WASM_I32: u8 = 0x7f;
pub const WASM_I64: u8 = 0x7e;
pub const WASM_F32: u8 = 0x7d;
pub const WASM_F64: u8 = 0x7c;
pub const WASM_FUNCREF: u8 = 0x70;
pub const WASM_EXTERNREF: u8 = 0x6f;

pub const WASM_EXT_FUNC: c_int = 0;
pub const WASM_EXT_TABLE: c_int = 1;
pub const WASM_EXT_MEMORY: c_int = 2;
pub const WASM_EXT_GLOBAL: c_int = 3;

pub const WASM_PAGE: u32 = 65536;
pub const WASM_NO_MAX: u32 = u32::MAX;

pub const WASM_OK: c_int = 0;
pub const WASM_ERR_MALFORMED: c_int = 1;
pub const WASM_ERR_INVALID: c_int = 2;
pub const WASM_ERR_LINK: c_int = 3;
pub const WASM_ERR_TRAP: c_int = 4;
pub const WASM_ERR_EXHAUSTED: c_int = 5;
pub const WASM_ERR_HOST: c_int = 6;
pub const WASM_ERR_NOMEM: c_int = 7;

/// A value. f32 and f64 keep their bits as-is (NaNs included).
#[repr(C)]
#[derive(Clone, Copy)]
pub union WasmVal {
    pub i32: i32,
    pub u32: u32,
    pub i64: i64,
    pub u64: u64,
    pub f32: f32,
    pub f64: f64,
    pub r#ref: *mut c_void,
}

impl WasmVal {
    pub const ZERO: WasmVal = WasmVal { u64: 0 };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WasmError {
    pub kind: c_int,
    pub msg: [c_char; 160],
}

/// `snprintf(err->msg, sizeof err->msg, ...)` and the kind.
pub(crate) unsafe fn set_err(err: *mut WasmError, kind: c_int, msg: &[u8]) {
    if err.is_null() {
        return;
    }
    (*err).kind = kind;
    let n = msg.len().min(159);
    std::ptr::copy_nonoverlapping(msg.as_ptr(), (*err).msg.as_mut_ptr().cast::<u8>(), n);
    (*err).msg[n] = 0;
}

/// Function type (interned: two equal types are the same pointer).
#[repr(C)]
pub struct WasmFuncType {
    pub nparams: u32,
    pub nresults: u32,
    pub params: *const u8,
    pub results: *const u8,
}

unsafe impl Send for WasmFuncType {}

struct Interned(*mut WasmFuncType);
unsafe impl Send for Interned {}

static TYPES: Mutex<Option<HashMap<(Vec<u8>, Vec<u8>), Interned>>> = Mutex::new(None);

/// Interns a type. The storage is never freed, so the pointers stay valid forever.
pub(crate) fn functype(params: &[u8], results: &[u8]) -> *const WasmFuncType {
    let mut g = TYPES.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(t) = map.get(&(params.to_vec(), results.to_vec())) {
        return t.0;
    }
    // one leaked allocation holds the type and its bytes, so the pointers inside stay put
    let mut v: Vec<u8> = Vec::with_capacity(params.len() + results.len() + 1);
    v.extend_from_slice(params);
    v.extend_from_slice(results);
    v.push(0);
    let v: &'static mut [u8] = Box::leak(v.into_boxed_slice());
    let p = v.as_ptr();
    let t = Box::into_raw(Box::new(WasmFuncType {
        nparams: params.len() as u32,
        nresults: results.len() as u32,
        params: p,
        results: unsafe { p.add(params.len()) },
    }));
    map.insert((params.to_vec(), results.to_vec()), Interned(t));
    t
}

/// A type's parameter and result lists as slices. Interned types live forever.
pub(crate) unsafe fn type_params(t: *const WasmFuncType) -> &'static [u8] {
    if (*t).nparams == 0 { &[] } else { std::slice::from_raw_parts((*t).params, (*t).nparams as usize) }
}
pub(crate) unsafe fn type_results(t: *const WasmFuncType) -> &'static [u8] {
    if (*t).nresults == 0 { &[] } else { std::slice::from_raw_parts((*t).results, (*t).nresults as usize) }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype(params: *const u8, np: u32, results: *const u8, nr: u32) -> *const WasmFuncType {
    let p = if np == 0 { &[][..] } else { std::slice::from_raw_parts(params, np as usize) };
    let r = if nr == 0 { &[][..] } else { std::slice::from_raw_parts(results, nr as usize) };
    functype(p, r)
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WasmExtern {
    pub kind: c_int,
    pub ptr: *mut c_void,
}

/// Description of an import or export.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WasmExternType {
    pub kind: c_int,
    pub func: *const WasmFuncType,
    pub valtype: u8,
    pub r#mut: c_int,
    pub min: u32,
    pub max: u32,
}

impl WasmExternType {
    pub(crate) fn new(kind: c_int) -> Self {
        WasmExternType { kind, func: null(), valtype: 0, r#mut: 0, min: 0, max: 0 }
    }
}

pub type WasmHostFn = unsafe extern "C" fn(ud: *mut c_void, args: *const WasmVal, results: *mut WasmVal, err: *mut WasmError) -> c_int;

// ---- internal structures ----

/// Compiled code of a function: each instruction is its opcode (a WebAssembly byte, 0x100 + n for the
/// 0xFC prefix, or one of the internal ones in exec.rs) followed by its immediates, as 64-bit cells.
/// Jumps are relative to the start of the instruction.
pub(crate) struct WasmCode {
    pub nlocals: u32,
    pub max_stack: u32,
    pub code: Vec<u64>,
}

/// A constant expression: its bytes in the module (up to its `end`).
#[derive(Clone, Copy, Default)]
pub(crate) struct ConstExpr {
    pub off: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct ElemItem {
    pub is_expr: bool,
    pub func: u32,
    pub expr: ConstExpr,
}

pub(crate) struct ElemSeg {
    pub mode: u8, // 0 active, 1 passive, 2 declarative
    pub ty: u8,
    pub table: u32,
    pub offset: ConstExpr,
    pub items: Vec<ElemItem>,
}

pub(crate) struct DataSeg {
    pub active: bool,
    pub offset: ConstExpr,
    pub off: usize,
    pub len: u32,
}

pub(crate) struct Import {
    /// NUL-terminated; the name may contain NULs, `len` is the real length.
    pub module: Vec<u8>,
    pub name: Vec<u8>,
    pub ty: WasmExternType,
}

pub(crate) struct Export {
    pub name: Vec<u8>,
    pub kind: c_int,
    pub idx: u32,
}

pub(crate) struct GlobalDef {
    pub ty: u8,
    pub mutable: u8,
    pub init: ConstExpr,
}

pub(crate) struct TableDef {
    pub ty: u8,
    pub min: u32,
    pub max: u32,
}

pub(crate) struct Custom {
    pub name: Vec<u8>,
    pub off: usize,
    pub len: usize,
}

pub struct WasmModule {
    pub(crate) rc: i32,
    pub(crate) bytes: Vec<u8>,
    pub(crate) types: Vec<*const WasmFuncType>,
    pub(crate) imports: Vec<Import>,
    pub(crate) nfunc_imports: u32,
    pub(crate) ntable_imports: u32,
    pub(crate) nmem_imports: u32,
    pub(crate) nglobal_imports: u32,
    pub(crate) functypes: Vec<*const WasmFuncType>,
    pub(crate) code: Vec<WasmCode>,
    pub(crate) tables: Vec<TableDef>,
    pub(crate) nmems: u32,
    pub(crate) mem_min: u32,
    pub(crate) mem_max: u32,
    pub(crate) globals: Vec<GlobalDef>,
    pub(crate) exports: Vec<Export>,
    pub(crate) start: i64,
    pub(crate) elems: Vec<ElemSeg>,
    pub(crate) ndata: u32,
    pub(crate) has_datacount: bool,
    pub(crate) data: Vec<DataSeg>,
    pub(crate) declared: Vec<u8>,
    pub(crate) custom: Vec<Custom>,
}

pub struct WasmFunc {
    pub(crate) ty: *const WasmFuncType,
    pub(crate) inst: *mut WasmInstance,
    pub(crate) code: *const WasmCode,
    pub(crate) host: Option<WasmHostFn>,
    pub(crate) ud: *mut c_void,
    pub(crate) free_ud: Option<unsafe extern "C" fn(*mut c_void)>,
    pub(crate) rc: i32,
    pub(crate) slot: *mut c_void,
}

impl WasmFunc {
    pub(crate) fn blank() -> WasmFunc {
        WasmFunc { ty: null(), inst: null_mut(), code: null(), host: None, ud: null_mut(), free_ud: None, rc: 0, slot: null_mut() }
    }
}

pub struct WasmMemory {
    pub(crate) rc: i32,
    pub(crate) data: *mut u8, // reserved up to max (PROT_NONE beyond size)
    pub(crate) size: usize,   // accessible bytes
    pub(crate) reserved: usize,
    pub(crate) max: u32,
    pub(crate) on_grow: Option<unsafe extern "C" fn(*mut c_void, *mut WasmMemory)>,
    pub(crate) on_grow_ud: *mut c_void,
    pub(crate) slot: [*mut c_void; 2],
}

pub struct WasmTable {
    pub(crate) rc: i32, // outside references (JS, other instances)
    pub(crate) owner: *mut WasmInstance,
    pub(crate) ty: u8,
    pub(crate) size: u32,
    pub(crate) max: u32,
    pub(crate) elems: Vec<WasmVal>,
    pub(crate) slot: *mut c_void,
}

pub struct WasmGlobal {
    pub(crate) rc: i32,
    pub(crate) owner: *mut WasmInstance,
    pub(crate) ty: u8,
    pub(crate) mutable: u8,
    pub(crate) val: WasmVal,
    pub(crate) slot: *mut c_void,
}

pub struct WasmInstance {
    pub(crate) rc: i32,
    pub(crate) m: *mut WasmModule,
    pub(crate) funcs: Vec<*mut WasmFunc>, // by index (imports point to foreign functions)
    pub(crate) own: Box<[WasmFunc]>,      // the defined ones
    pub(crate) tables: Vec<*mut WasmTable>,
    pub(crate) mem: *mut WasmMemory,
    pub(crate) globals: Vec<*mut WasmGlobal>,
    pub(crate) elem_dropped: Vec<u8>,
    pub(crate) data_dropped: Vec<u8>,
    pub(crate) exports: Vec<WasmExtern>,
}

#[no_mangle]
pub static mut wasm_max_call_depth: u32 = 20000;
#[no_mangle]
pub static mut wasm_max_memory_pages: u32 = 65536;

// ---- references ----

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_ref(i: *mut WasmInstance) {
    (*i).rc += 1;
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_unref(i: *mut WasmInstance) {
    if i.is_null() {
        return;
    }
    (*i).rc -= 1;
    if (*i).rc == 0 {
        exec::instance_free(i);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_module(i: *const WasmInstance) -> *mut WasmModule {
    (*i).m
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_ref(f: *mut WasmFunc) {
    if !(*f).inst.is_null() {
        wasm_instance_ref((*f).inst);
    } else {
        (*f).rc += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_unref(f: *mut WasmFunc) {
    if f.is_null() {
        return;
    }
    if !(*f).inst.is_null() {
        wasm_instance_unref((*f).inst);
        return;
    }
    (*f).rc -= 1;
    if (*f).rc == 0 {
        if let Some(fr) = (*f).free_ud {
            fr((*f).ud);
        }
        drop(Box::from_raw(f));
    }
}

/// Retain a funcref stored in an object (table or global) of `owner`: functions of the same
/// owning instance don't add a reference (avoids the instance <-> its table cycle).
pub(crate) unsafe fn ref_hold(owner: *mut WasmInstance, ty: u8, v: WasmVal) {
    if ty != WASM_FUNCREF || v.r#ref.is_null() {
        return;
    }
    let f = v.r#ref.cast::<WasmFunc>();
    if (*f).inst != owner || owner.is_null() {
        wasm_func_ref(f);
    }
}

pub(crate) unsafe fn ref_release(owner: *mut WasmInstance, ty: u8, v: WasmVal) {
    if ty != WASM_FUNCREF || v.r#ref.is_null() {
        return;
    }
    let f = v.r#ref.cast::<WasmFunc>();
    if (*f).inst != owner || owner.is_null() {
        wasm_func_unref(f);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_host(
    ty: *const WasmFuncType,
    f: Option<WasmHostFn>,
    ud: *mut c_void,
    free_ud: Option<unsafe extern "C" fn(*mut c_void)>,
) -> *mut WasmFunc {
    let mut w = WasmFunc::blank();
    w.ty = ty;
    w.host = f;
    w.ud = ud;
    w.free_ud = free_ud;
    w.rc = 1;
    Box::into_raw(Box::new(w))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_type(f: *const WasmFunc) -> *const WasmFuncType {
    (*f).ty
}
#[no_mangle]
pub unsafe extern "C" fn wasm_func_is_host(f: *const WasmFunc) -> c_int {
    (*f).host.is_some() as c_int
}
#[no_mangle]
pub unsafe extern "C" fn wasm_func_host_data(f: *const WasmFunc) -> *mut c_void {
    (*f).ud
}
#[no_mangle]
pub unsafe extern "C" fn wasm_func_slot(f: *mut WasmFunc) -> *mut *mut c_void {
    &mut (*f).slot
}

// ---- memories ----

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(min: u32, max: u32, err: *mut WasmError) -> *mut WasmMemory {
    let cap = wasm_max_memory_pages;
    if min > cap {
        set_err(err, WASM_ERR_NOMEM, format!("memory too large ({min} pages)").as_bytes());
        return null_mut();
    }
    let top = if max == WASM_NO_MAX || max > cap { cap } else { max };
    let reserved = (if top != 0 { top } else { 1 }) as usize * WASM_PAGE as usize;
    let p = libc::mmap(null_mut(), reserved, libc::PROT_NONE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0);
    if p == libc::MAP_FAILED {
        set_err(err, WASM_ERR_NOMEM, b"could not reserve the memory");
        return null_mut();
    }
    let size = min as usize * WASM_PAGE as usize;
    if size != 0 && libc::mprotect(p, size, libc::PROT_READ | libc::PROT_WRITE) != 0 {
        libc::munmap(p, reserved);
        set_err(err, WASM_ERR_NOMEM, b"out of memory");
        return null_mut();
    }
    Box::into_raw(Box::new(WasmMemory {
        rc: 1,
        data: p.cast(),
        size,
        reserved,
        max,
        on_grow: None,
        on_grow_ud: null_mut(),
        slot: [null_mut(); 2],
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_ref(m: *mut WasmMemory) {
    (*m).rc += 1;
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_unref(m: *mut WasmMemory) {
    if m.is_null() {
        return;
    }
    (*m).rc -= 1;
    if (*m).rc == 0 {
        libc::munmap((*m).data.cast(), (*m).reserved);
        drop(Box::from_raw(m));
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(m: *const WasmMemory) -> *mut u8 {
    (*m).data
}
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(m: *const WasmMemory) -> usize {
    (*m).size
}
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_max(m: *const WasmMemory) -> u32 {
    (*m).max
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_on_grow(m: *mut WasmMemory, f: Option<unsafe extern "C" fn(*mut c_void, *mut WasmMemory)>, ud: *mut c_void) {
    (*m).on_grow = f;
    (*m).on_grow_ud = ud;
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_slots(m: *mut WasmMemory) -> *mut *mut c_void {
    (*m).slot.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn wasm_table_slot(t: *mut WasmTable) -> *mut *mut c_void {
    &mut (*t).slot
}
#[no_mangle]
pub unsafe extern "C" fn wasm_global_slot(g: *mut WasmGlobal) -> *mut *mut c_void {
    &mut (*g).slot
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(m: *mut WasmMemory, delta: u32) -> i64 {
    let mem = &mut *m;
    let old = (mem.size / WASM_PAGE as usize) as u64;
    let n = old + delta as u64;
    let limit = if mem.max == WASM_NO_MAX { 65536 } else { mem.max as u64 };
    if n > limit || n * WASM_PAGE as u64 > mem.reserved as u64 {
        return -1;
    }
    if delta != 0 {
        if libc::mprotect(mem.data.add(mem.size).cast(), delta as usize * WASM_PAGE as usize, libc::PROT_READ | libc::PROT_WRITE) != 0 {
            return -1;
        }
        mem.size = n as usize * WASM_PAGE as usize;
    }
    if let Some(f) = mem.on_grow {
        f(mem.on_grow_ud, m);
    }
    old as i64
}

// ---- tables ----

pub(crate) unsafe fn table_alloc(owner: *mut WasmInstance, ty: u8, min: u32, max: u32, init: WasmVal, err: *mut WasmError) -> *mut WasmTable {
    if min > 10_000_000 {
        set_err(err, WASM_ERR_NOMEM, b"table too large");
        return null_mut();
    }
    let t = Box::into_raw(Box::new(WasmTable { rc: 0, owner, ty, size: min, max, elems: vec![init; min as usize], slot: null_mut() }));
    for _ in 0..min {
        ref_hold(owner, ty, init);
    }
    t
}

pub(crate) unsafe fn table_destroy(t: *mut WasmTable) {
    for i in 0..(*t).size as usize {
        ref_release((*t).owner, (*t).ty, (*t).elems[i]);
    }
    drop(Box::from_raw(t));
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_new(ty: u8, min: u32, max: u32, init: WasmVal, err: *mut WasmError) -> *mut WasmTable {
    let t = table_alloc(null_mut(), ty, min, max, init, err);
    if !t.is_null() {
        (*t).rc = 1;
    }
    t
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_ref(t: *mut WasmTable) {
    let first = (*t).rc == 0;
    (*t).rc += 1;
    if first && !(*t).owner.is_null() {
        wasm_instance_ref((*t).owner);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_unref(t: *mut WasmTable) {
    if t.is_null() {
        return;
    }
    (*t).rc -= 1;
    if (*t).rc != 0 {
        return;
    }
    if !(*t).owner.is_null() {
        wasm_instance_unref((*t).owner);
    } else {
        table_destroy(t);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_elemtype(t: *const WasmTable) -> u8 {
    (*t).ty
}
#[no_mangle]
pub unsafe extern "C" fn wasm_table_size(t: *const WasmTable) -> u32 {
    (*t).size
}
#[no_mangle]
pub unsafe extern "C" fn wasm_table_max(t: *const WasmTable) -> u32 {
    (*t).max
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_get(t: *const WasmTable, i: u32, v: *mut WasmVal) -> c_int {
    if i >= (*t).size {
        return -1;
    }
    *v = (*t).elems[i as usize];
    0
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_set(t: *mut WasmTable, i: u32, v: WasmVal) -> c_int {
    if i >= (*t).size {
        return -1;
    }
    ref_hold((*t).owner, (*t).ty, v);
    let old = (*t).elems[i as usize];
    (*t).elems[i as usize] = v;
    ref_release((*t).owner, (*t).ty, old);
    0
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_grow(t: *mut WasmTable, delta: u32, init: WasmVal) -> i64 {
    let tb = &mut *t;
    let n = tb.size as u64 + delta as u64;
    let limit = if tb.max == WASM_NO_MAX { 10_000_000 } else { tb.max as u64 };
    if n > limit || n > 10_000_000 {
        return -1;
    }
    if delta != 0 {
        tb.elems.resize(n as usize, init);
        for _ in tb.size as u64..n {
            ref_hold(tb.owner, tb.ty, init);
        }
    }
    let old = tb.size;
    tb.size = n as u32;
    old as i64
}

// ---- globals ----

pub(crate) unsafe fn global_alloc(owner: *mut WasmInstance, ty: u8, mutable: c_int, v: WasmVal) -> *mut WasmGlobal {
    ref_hold(owner, ty, v);
    Box::into_raw(Box::new(WasmGlobal { rc: 0, owner, ty, mutable: mutable as u8, val: v, slot: null_mut() }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_new(ty: u8, mutable: c_int, v: WasmVal) -> *mut WasmGlobal {
    let g = global_alloc(null_mut(), ty, mutable, v);
    (*g).rc = 1;
    g
}

pub(crate) unsafe fn global_destroy(g: *mut WasmGlobal) {
    ref_release((*g).owner, (*g).ty, (*g).val);
    drop(Box::from_raw(g));
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_ref(g: *mut WasmGlobal) {
    let first = (*g).rc == 0;
    (*g).rc += 1;
    if first && !(*g).owner.is_null() {
        wasm_instance_ref((*g).owner);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_unref(g: *mut WasmGlobal) {
    if g.is_null() {
        return;
    }
    (*g).rc -= 1;
    if (*g).rc != 0 {
        return;
    }
    if !(*g).owner.is_null() {
        wasm_instance_unref((*g).owner);
    } else {
        global_destroy(g);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_type(g: *const WasmGlobal) -> u8 {
    (*g).ty
}
#[no_mangle]
pub unsafe extern "C" fn wasm_global_mut(g: *const WasmGlobal) -> c_int {
    (*g).mutable as c_int
}
#[no_mangle]
pub unsafe extern "C" fn wasm_global_get(g: *const WasmGlobal) -> WasmVal {
    (*g).val
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_set(g: *mut WasmGlobal, v: WasmVal) {
    ref_hold((*g).owner, (*g).ty, v);
    let old = (*g).val;
    (*g).val = v;
    ref_release((*g).owner, (*g).ty, old);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_ref(e: WasmExtern) {
    match e.kind {
        WASM_EXT_FUNC => wasm_func_ref(e.ptr.cast()),
        WASM_EXT_TABLE => wasm_table_ref(e.ptr.cast()),
        WASM_EXT_MEMORY => wasm_memory_ref(e.ptr.cast()),
        WASM_EXT_GLOBAL => wasm_global_ref(e.ptr.cast()),
        _ => {}
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_unref(e: WasmExtern) {
    match e.kind {
        WASM_EXT_FUNC => wasm_func_unref(e.ptr.cast()),
        WASM_EXT_TABLE => wasm_table_unref(e.ptr.cast()),
        WASM_EXT_MEMORY => wasm_memory_unref(e.ptr.cast()),
        WASM_EXT_GLOBAL => wasm_global_unref(e.ptr.cast()),
        _ => {}
    }
}

// ---- module accessors ----

#[no_mangle]
pub unsafe extern "C" fn wasm_module_ref(m: *mut WasmModule) {
    (*m).rc += 1;
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_unref(m: *mut WasmModule) {
    if m.is_null() {
        return;
    }
    (*m).rc -= 1;
    if (*m).rc == 0 {
        drop(Box::from_raw(m));
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_num_imports(m: *const WasmModule) -> u32 {
    (*m).imports.len() as u32
}

#[no_mangle]
pub unsafe extern "C" fn wasm_import_module(m: *const WasmModule, i: u32, len: *mut usize) -> *const c_char {
    let Some(im) = (*m).imports.get(i as usize) else { return null() };
    if !len.is_null() {
        *len = im.module.len() - 1;
    }
    im.module.as_ptr().cast()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_import_name(m: *const WasmModule, i: u32, len: *mut usize) -> *const c_char {
    let Some(im) = (*m).imports.get(i as usize) else { return null() };
    if !len.is_null() {
        *len = im.name.len() - 1;
    }
    im.name.as_ptr().cast()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_import_type(m: *const WasmModule, i: u32) -> WasmExternType {
    match (*m).imports.get(i as usize) {
        Some(im) => im.ty,
        None => WasmExternType::new(-1),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_num_exports(m: *const WasmModule) -> u32 {
    (*m).exports.len() as u32
}

#[no_mangle]
pub unsafe extern "C" fn wasm_export_name(m: *const WasmModule, i: u32, len: *mut usize) -> *const c_char {
    let Some(e) = (*m).exports.get(i as usize) else { return null() };
    if !len.is_null() {
        *len = e.name.len() - 1;
    }
    e.name.as_ptr().cast()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_export_type(m: *const WasmModule, i: u32) -> WasmExternType {
    let m = &*m;
    let mut t = WasmExternType::new(-1);
    let Some(e) = m.exports.get(i as usize) else { return t };
    t.kind = e.kind;
    match e.kind {
        WASM_EXT_FUNC => t.func = m.functypes[e.idx as usize],
        WASM_EXT_TABLE => {
            let d = &m.tables[e.idx as usize];
            (t.valtype, t.min, t.max) = (d.ty, d.min, d.max);
        }
        WASM_EXT_MEMORY => (t.min, t.max) = (m.mem_min, m.mem_max),
        WASM_EXT_GLOBAL => {
            let g = &m.globals[e.idx as usize];
            (t.valtype, t.r#mut) = (g.ty, g.mutable as c_int);
        }
        _ => {}
    }
    t
}

#[no_mangle]
pub unsafe extern "C" fn wasm_custom_section(m: *const WasmModule, name: *const c_char, n: u32, len: *mut usize) -> *const u8 {
    let name = std::ffi::CStr::from_ptr(name).to_bytes();
    let mut n = n;
    for c in &(*m).custom {
        // a custom section name is a C string in the original: it ends at a NUL
        let cn = &c.name[..c.name.iter().position(|&b| b == 0).unwrap_or(c.name.len())];
        if cn == name {
            if n == 0 {
                *len = c.len;
                return (*m).bytes.as_ptr().add(c.off);
            }
            n -= 1;
        }
    }
    null()
}

#[cfg(test)]
mod tests {
    use super::decode::{wasm_module_new, wasm_validate};
    use super::exec::{wasm_call, wasm_instance_export_find_len, wasm_instantiate};
    use super::*;

    // the interpreter has one global machine (like the C one: a single JS thread): tests take turns
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// (i32, i32) -> i32 exported as "f"; `op` is the instruction applied to both arguments.
    fn binop(op: u8) -> Vec<u8> {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        m.extend_from_slice(&[1, 7, 1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f]);
        m.extend_from_slice(&[3, 2, 1, 0]);
        m.extend_from_slice(&[7, 5, 1, 1, b'f', 0, 0]);
        m.extend_from_slice(&[10, 9, 1, 7, 0, 0x20, 0, 0x20, 1, op, 0x0b]);
        m
    }

    unsafe fn call(bytes: &[u8], a: u32, b: u32) -> Result<u32, String> {
        let mut err = WasmError { kind: 0, msg: [0; 160] };
        let m = wasm_module_new(bytes.as_ptr(), bytes.len(), &mut err);
        assert!(!m.is_null(), "module: {}", msg(&err));
        let inst = wasm_instantiate(m, null(), 0, &mut err);
        assert!(!inst.is_null());
        let e = wasm_instance_export_find_len(inst, b"f".as_ptr().cast(), 1);
        let args = [WasmVal { u64: a as u64 }, WasmVal { u64: b as u64 }];
        let mut res = [WasmVal { u64: 0 }];
        let r = wasm_call(e.ptr.cast(), args.as_ptr(), res.as_mut_ptr(), &mut err);
        wasm_instance_unref(inst);
        wasm_module_unref(m);
        if r == 0 { Ok(res[0].u32) } else { Err(msg(&err)) }
    }

    fn msg(e: &WasmError) -> String {
        let n = e.msg.iter().position(|&c| c == 0).unwrap_or(160);
        String::from_utf8_lossy(&e.msg[..n].iter().map(|&c| c as u8).collect::<Vec<_>>()).into_owned()
    }

    #[test]
    fn adds_and_wraps() {
        let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            assert_eq!(call(&binop(0x6a), 2, 3), Ok(5));
            assert_eq!(call(&binop(0x6a), u32::MAX, 2), Ok(1));
        }
    }

    #[test]
    fn traps_with_the_specification_messages() {
        let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            assert_eq!(call(&binop(0x6d), 1, 0), Err("integer divide by zero".into()));
            assert_eq!(call(&binop(0x6d), 0x8000_0000, u32::MAX), Err("integer overflow".into()));
            assert_eq!(call(&binop(0x6d), 7, 2), Ok(3));
        }
    }

    #[test]
    fn rejects_bad_binaries() {
        let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let mut err = WasmError { kind: 0, msg: [0; 160] };
            let good = binop(0x6a);
            assert!(wasm_module_new(good.as_ptr(), 3, &mut err).is_null());
            assert_eq!(err.kind, WASM_ERR_MALFORMED);
            // i32.add, then the wrong result type: validation, not decoding
            let mut bad = good.clone();
            bad[16] = 0x7e;
            assert_eq!(wasm_validate(bad.as_ptr(), bad.len(), &mut err), -1);
            assert_eq!(err.kind, WASM_ERR_INVALID);
            assert_eq!(wasm_validate(good.as_ptr(), good.len(), &mut err), 0);
        }
    }
}
