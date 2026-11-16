//! Instances and the interpreter (the former wasm_run.c).
//!
//! A single value stack (64 bits per value) and a frame stack, reserved with mmap: real memory is
//! used as it gets touched. Calls wasm -> wasm don't recurse; host (JS) calls can re-enter.
use super::decode::*;
use super::*;
use core::ffi::{c_char, c_int};
use std::ptr::{null, null_mut};

const VSTACK_SLOTS: usize = 1 << 20; // 8 MiB of values

pub(crate) fn exec_init() {}

struct Frame {
    ip: *const u64, // NULL: return to the host
    fp: *mut u64,
    inst: *mut WasmInstance,
    crossed: bool, // entered another instance (it has to be released)
}

struct Machine {
    vstack: *mut u64,
    vstack_end: *mut u64,
    cur_sp: *mut u64,
    frames: Vec<Frame>,
}

struct Global(std::cell::UnsafeCell<Machine>);
// Single-threaded by contract (wasm.h).
unsafe impl Sync for Global {}
static MACHINE: Global = Global(std::cell::UnsafeCell::new(Machine { vstack: null_mut(), vstack_end: null_mut(), cur_sp: null_mut(), frames: Vec::new() }));

unsafe fn machine() -> &'static mut Machine {
    &mut *MACHINE.0.get()
}

unsafe fn stacks_init() -> bool {
    let m = machine();
    if !m.vstack.is_null() {
        return true;
    }
    let vs = VSTACK_SLOTS * 8;
    let p = libc::mmap(null_mut(), vs, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0);
    if p == libc::MAP_FAILED {
        return false;
    }
    m.frames = Vec::with_capacity(wasm_max_call_depth as usize + 16);
    m.vstack = p.cast();
    m.cur_sp = m.vstack;
    m.vstack_end = m.vstack.add(VSTACK_SLOTS);
    true
}

// ---- constant expressions ----

fn rd_leb(b: &[u8], p: &mut usize, sign: bool) -> u64 {
    let mut r: u64 = 0;
    let mut shift = 0u32;
    let mut x;
    loop {
        x = b[*p];
        *p += 1;
        if shift < 64 {
            r |= ((x & 0x7f) as u64) << shift;
        }
        shift += 7;
        if x & 0x80 == 0 {
            break;
        }
    }
    if sign && shift < 64 && (x & 0x40) != 0 {
        r |= !0u64 << shift;
    }
    r
}

/// Evaluates an already validated constant expression.
pub(crate) unsafe fn const_eval(inst: *const WasmInstance, e: ConstExpr) -> WasmVal {
    let m = &*(*inst).m;
    let b = &m.bytes[..];
    let mut p = e.off;
    let mut v = WasmVal::ZERO;
    loop {
        let op = b[p];
        p += 1;
        match op {
            0x0b => return v,
            0x41 => {
                v.u64 = 0;
                v.i32 = rd_leb(b, &mut p, true) as i32;
            }
            0x42 => v.u64 = rd_leb(b, &mut p, true),
            0x43 => {
                v.u64 = 0;
                v.u32 = u32::from_le_bytes(b[p..p + 4].try_into().expect("4 bytes"));
                p += 4;
            }
            0x44 => {
                v.u64 = u64::from_le_bytes(b[p..p + 8].try_into().expect("8 bytes"));
                p += 8;
            }
            0xd0 => {
                p += 1;
                v.u64 = 0;
            }
            0xd2 => {
                v.u64 = 0;
                v.r#ref = (*inst).funcs[rd_leb(b, &mut p, false) as usize].cast();
            }
            0x23 => v = (*(*inst).globals[rd_leb(b, &mut p, false) as usize]).val,
            _ => return v,
        }
    }
}

// ---- arithmetic ----

fn fmin32(a: f32, b: f32) -> f32 {
    if a != a || b != b {
        return a + b;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}
fn fmax32(a: f32, b: f32) -> f32 {
    if a != a || b != b {
        return a + b;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}
fn fmin64(a: f64, b: f64) -> f64 {
    if a != a || b != b {
        return a + b;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}
fn fmax64(a: f64, b: f64) -> f64 {
    if a != a || b != b {
        return a + b;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}

// Saturating conversion (trunc_sat): NaN -> 0, out of range -> the extreme.
fn sat_i32(x: f64) -> i32 {
    if x != x {
        return 0;
    }
    if x <= -2147483649.0 {
        return i32::MIN;
    }
    if x >= 2147483648.0 {
        return i32::MAX;
    }
    x as i32
}
fn sat_u32(x: f64) -> u32 {
    if x != x || x <= -1.0 {
        return 0;
    }
    if x >= 4294967296.0 {
        return u32::MAX;
    }
    x as u32
}
fn sat_i64(x: f64) -> i64 {
    if x != x {
        return 0;
    }
    if x < -9223372036854775808.0 {
        return i64::MIN;
    }
    if x >= 9223372036854775808.0 {
        return i64::MAX;
    }
    x as i64
}
fn sat_u64(x: f64) -> u64 {
    if x != x || x <= -1.0 {
        return 0;
    }
    if x >= 18446744073709551616.0 {
        return u64::MAX;
    }
    x as u64
}

// ---- slots: a value is 64 bits; many instructions write only the low 32 (like the union did) ----

#[inline(always)]
unsafe fn u32_at(p: *mut u64) -> u32 {
    *p.cast::<u32>()
}
#[inline(always)]
unsafe fn set_u32(p: *mut u64, v: u32) {
    *p.cast::<u32>() = v;
}
#[inline(always)]
unsafe fn f32_at(p: *mut u64) -> f32 {
    f32::from_bits(*p.cast::<u32>())
}
#[inline(always)]
unsafe fn set_f32(p: *mut u64, v: f32) {
    *p.cast::<u32>() = v.to_bits();
}
#[inline(always)]
unsafe fn f64_at(p: *mut u64) -> f64 {
    f64::from_bits(*p)
}
#[inline(always)]
unsafe fn set_f64(p: *mut u64, v: f64) {
    *p = v.to_bits();
}

/// A quiet NaN for a rounding result (glibc returns a signaling NaN as-is; wasm requires a quiet one).
#[inline(always)]
fn q32(x: f32, e: f32) -> f32 {
    if x != x { x + x } else { e }
}
#[inline(always)]
fn q64(x: f64, e: f64) -> f64 {
    if x != x { x + x } else { e }
}

// ---- the interpreter ----

/// Runs `entry` with its arguments at `base`. 0 on success, -1 on a trap or a host error.
unsafe fn exec(entry: *mut WasmFunc, base: *mut u64, err: *mut WasmError) -> c_int {
    let entry_depth = machine().frames.len();
    let mut inst = (*entry).inst;
    let mut mem: *mut WasmMemory;
    let mut mb: *mut u8;
    let mut msz: u64;
    let mut fp: *mut u64;
    let mut sp: *mut u64;
    let mut ip: *const u64;

    macro_rules! reload {
        () => {
            mem = (*inst).mem;
            mb = if mem.is_null() { null_mut() } else { (*mem).data };
            msz = if mem.is_null() { 0 } else { (*mem).size as u64 };
        };
    }

    let code = &*(*entry).code;
    if machine().frames.len() as u32 >= wasm_max_call_depth || base.add(code.max_stack as usize) > machine().vstack_end {
        set_err(err, WASM_ERR_EXHAUSTED, b"call stack exhausted");
        return -1;
    }
    machine().frames.push(Frame { ip: null(), fp: null_mut(), inst: null_mut(), crossed: false });
    fp = base;
    let nparams = (*(*entry).ty).nparams as usize;
    std::ptr::write_bytes(fp.add(nparams), 0, code.nlocals as usize - nparams);
    sp = fp.add(code.nlocals as usize);
    ip = code.code.as_ptr();
    reload!();

    let (trapkind, trapmsg): (c_int, &[u8]) = 'run: loop {
    macro_rules! trap {
        ($msg:expr) => {{
            break 'run (WASM_ERR_TRAP, $msg)
        }};
    }
    macro_rules! next {
        ($n:expr) => {{
            ip = ip.add($n);
            continue 'run;
        }};
    }
    // effective address of a load or store; traps when it does not fit
    macro_rules! ea {
        ($at:expr, $n:expr) => {{
            let ea = u32_at($at) as u64 + *ip.add(1);
            if ea + $n > msz {
                trap!(b"out of bounds memory access");
            }
            ea as usize
        }};
    }
    // numeric helper macros over raw slots
    macro_rules! bin32 {
        ($a:ident, $b:ident, $e:expr) => {{
            let $a = u32_at(sp.sub(2));
            let $b = u32_at(sp.sub(1));
            set_u32(sp.sub(2), $e);
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! bin64 {
        ($a:ident, $b:ident, $e:expr) => {{
            let $a = *sp.sub(2);
            let $b = *sp.sub(1);
            *sp.sub(2) = $e;
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! cmp32 {
        ($ty:ty, $op:tt) => {{
            let a = u32_at(sp.sub(2)) as $ty;
            let b = u32_at(sp.sub(1)) as $ty;
            set_u32(sp.sub(2), (a $op b) as u32);
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! cmp64 {
        ($ty:ty, $op:tt) => {{
            let a = *sp.sub(2) as $ty;
            let b = *sp.sub(1) as $ty;
            set_u32(sp.sub(2), (a $op b) as u32);
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! cmpf32 {
        ($op:tt) => {{
            let r = f32_at(sp.sub(2)) $op f32_at(sp.sub(1));
            set_u32(sp.sub(2), r as u32);
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! cmpf64 {
        ($op:tt) => {{
            let r = f64_at(sp.sub(2)) $op f64_at(sp.sub(1));
            set_u32(sp.sub(2), r as u32);
            sp = sp.sub(1);
            next!(1);
        }};
    }
    macro_rules! load {
        ($n:expr, $ty:ty, $conv:expr) => {{
            let ea = ea!(sp.sub(1), $n);
            let v = std::ptr::read_unaligned(mb.add(ea).cast::<$ty>());
            *sp.sub(1) = $conv(v);
            next!(2);
        }};
    }
    macro_rules! store {
        ($n:expr, $ty:ty, $get:expr) => {{
            let ea = ea!(sp.sub(2), $n);
            let v = $get(*sp.sub(1)) as $ty;
            std::ptr::write_unaligned(mb.add(ea).cast::<$ty>(), v);
            sp = sp.sub(2);
            next!(2);
        }};
    }
    // loads that keep the upper half (the 32-bit value goes to the low half of an i32 slot): the C code
    // zeroed the slot first, then wrote the field.
    macro_rules! trunc {
        ($get:expr, $lo_ok:expr, $hi:expr, $set:expr) => {{
            let x: f64 = $get;
            if x != x {
                trap!(b"invalid conversion to integer");
            }
            if !($lo_ok(x) && x < $hi) {
                trap!(b"integer overflow");
            }
            $set(x);
            next!(1);
        }};
    }

        match *ip {
            0x00 => trap!(b"unreachable"),
            OP_BR => {
                ip = ip.offset(*ip.add(1) as i64 as isize);
                continue 'run;
            }
            OP_BR_N => {
                let dst = fp.add(*ip.add(2) as usize);
                let a = *ip.add(3) as usize;
                std::ptr::copy(sp.sub(a), dst, a);
                sp = dst.add(a);
                ip = ip.offset(*ip.add(1) as i64 as isize);
                continue 'run;
            }
            OP_BR_IF => {
                sp = sp.sub(1);
                if u32_at(sp) != 0 {
                    ip = ip.offset(*ip.add(1) as i64 as isize);
                    continue 'run;
                }
                next!(2);
            }
            OP_BR_IF_N => {
                sp = sp.sub(1);
                if u32_at(sp) != 0 {
                    let dst = fp.add(*ip.add(2) as usize);
                    let a = *ip.add(3) as usize;
                    std::ptr::copy(sp.sub(a), dst, a);
                    sp = dst.add(a);
                    ip = ip.offset(*ip.add(1) as i64 as isize);
                    continue 'run;
                }
                next!(4);
            }
            OP_BR_UNLESS => {
                sp = sp.sub(1);
                if u32_at(sp) == 0 {
                    ip = ip.offset(*ip.add(1) as i64 as isize);
                    continue 'run;
                }
                next!(2);
            }
            OP_BR_TABLE => {
                let n = *ip.add(1);
                let a = *ip.add(2) as usize;
                sp = sp.sub(1);
                let mut i = u32_at(sp) as u64;
                if i > n {
                    i = n;
                }
                let e = ip.add(3 + 2 * i as usize);
                let dst = fp.add(*e.add(1) as usize);
                if dst != sp.sub(a) {
                    std::ptr::copy(sp.sub(a), dst, a);
                    sp = dst.add(a);
                }
                ip = ip.offset(*e as i64 as isize);
                continue 'run;
            }
            OP_RETURN | OP_RETURN0 | OP_RETURN1 => {
                match *ip {
                    OP_RETURN => {
                        let n = *ip.add(1) as usize;
                        std::ptr::copy(sp.sub(n), fp, n);
                        sp = fp.add(n);
                    }
                    OP_RETURN0 => sp = fp,
                    _ => {
                        *fp = *sp.sub(1);
                        sp = fp.add(1);
                    }
                }
                let fr = machine().frames.pop().expect("frame");
                if fr.crossed {
                    wasm_instance_unref(inst);
                }
                if fr.ip.is_null() {
                    machine().cur_sp = sp;
                    return 0;
                }
                inst = fr.inst;
                fp = fr.fp;
                ip = fr.ip;
                reload!();
                continue 'run;
            }
            OP_CALL | OP_CALL_INDIRECT => {
                let callee: *mut WasmFunc;
                let ret_ip: *const u64;
                if *ip == OP_CALL {
                    callee = (*inst).funcs[*ip.add(1) as usize];
                    ret_ip = ip.add(2);
                } else {
                    let t = (*inst).tables[*ip.add(1) as usize];
                    sp = sp.sub(1);
                    let i = u32_at(sp);
                    if i >= (*t).size {
                        trap!(b"undefined element");
                    }
                    callee = (*t).elems[i as usize].r#ref.cast();
                    if callee.is_null() {
                        trap!(b"uninitialized element");
                    }
                    if (*callee).ty != *ip.add(2) as usize as *const WasmFuncType {
                        trap!(b"indirect call type mismatch");
                    }
                    ret_ip = ip.add(3);
                }
                let ft = (*callee).ty;
                let (np, nr) = ((*ft).nparams as usize, (*ft).nresults as usize);
                if let Some(host) = (*callee).host {
                    if sp.add(nr + 16) > machine().vstack_end {
                        break 'run (WASM_ERR_EXHAUSTED, b"call stack exhausted");
                    }
                    machine().cur_sp = sp.add(nr);
                    let mut herr = WasmError { kind: WASM_OK, msg: [0; 160] };
                    let r = host((*callee).ud, sp.sub(np).cast::<WasmVal>(), sp.cast::<WasmVal>(), &mut herr);
                    if r != 0 {
                        if !err.is_null() {
                            *err = herr;
                        }
                        if herr.kind == WASM_OK && !err.is_null() {
                            set_err(err, WASM_ERR_HOST, b"host error");
                        }
                        // a host error: unwind without writing a trap message
                        return unwind(entry_depth, inst, base);
                    }
                    std::ptr::copy(sp, sp.sub(np), nr);
                    sp = sp.sub(np).add(nr);
                    ip = ret_ip;
                    reload!();
                    continue 'run;
                }
                let c = &*(*callee).code;
                if machine().frames.len() as u32 >= wasm_max_call_depth || sp.add(c.max_stack as usize) > machine().vstack_end {
                    break 'run (WASM_ERR_EXHAUSTED, b"call stack exhausted");
                }
                let crossed = (*callee).inst != inst;
                machine().frames.push(Frame { ip: ret_ip, fp, inst, crossed });
                fp = sp.sub(np);
                std::ptr::write_bytes(sp, 0, c.nlocals as usize - np);
                sp = fp.add(c.nlocals as usize);
                if crossed {
                    inst = (*callee).inst;
                    wasm_instance_ref(inst);
                    reload!();
                }
                ip = c.code.as_ptr();
                continue 'run;
            }
            0x1a => {
                sp = sp.sub(1);
                next!(1);
            }
            0x1b => {
                let c = u32_at(sp.sub(1));
                sp = sp.sub(2);
                if c == 0 {
                    *sp.sub(1) = *sp;
                }
                next!(1);
            }
            0x20 => {
                *sp = *fp.add(*ip.add(1) as usize);
                sp = sp.add(1);
                next!(2);
            }
            0x21 => {
                sp = sp.sub(1);
                *fp.add(*ip.add(1) as usize) = *sp;
                next!(2);
            }
            0x22 => {
                *fp.add(*ip.add(1) as usize) = *sp.sub(1);
                next!(2);
            }
            0x23 => {
                *sp = (*(*inst).globals[*ip.add(1) as usize]).val.u64;
                sp = sp.add(1);
                next!(2);
            }
            0x24 => {
                let g = (*inst).globals[*ip.add(1) as usize];
                sp = sp.sub(1);
                if (*g).ty == WASM_FUNCREF {
                    wasm_global_set(g, WasmVal { u64: *sp });
                } else {
                    (*g).val.u64 = *sp;
                }
                next!(2);
            }
            0x25 => {
                let t = (*inst).tables[*ip.add(1) as usize];
                let i = u32_at(sp.sub(1));
                if i >= (*t).size {
                    trap!(b"out of bounds table access");
                }
                *sp.sub(1) = (*t).elems[i as usize].u64;
                next!(2);
            }
            0x26 => {
                let t = (*inst).tables[*ip.add(1) as usize];
                sp = sp.sub(2);
                if wasm_table_set(t, u32_at(sp), WasmVal { u64: *sp.add(1) }) != 0 {
                    trap!(b"out of bounds table access");
                }
                next!(2);
            }
            // loads
            0x28 => load!(4, u32, |v: u32| v as u64),
            0x29 => load!(8, u64, |v: u64| v),
            0x2a => load!(4, u32, |v: u32| v as u64),
            0x2b => load!(8, u64, |v: u64| v),
            0x2c => load!(1, i8, |v: i8| v as i32 as u32 as u64),
            0x2d => load!(1, u8, |v: u8| v as u64),
            0x2e => load!(2, i16, |v: i16| v as i32 as u32 as u64),
            0x2f => load!(2, u16, |v: u16| v as u64),
            0x30 => load!(1, i8, |v: i8| v as i64 as u64),
            0x31 => load!(1, u8, |v: u8| v as u64),
            0x32 => load!(2, i16, |v: i16| v as i64 as u64),
            0x33 => load!(2, u16, |v: u16| v as u64),
            0x34 => load!(4, i32, |v: i32| v as i64 as u64),
            0x35 => load!(4, u32, |v: u32| v as u64),
            // stores
            0x36 => store!(4, u32, |x: u64| x as u32),
            0x37 => store!(8, u64, |x: u64| x),
            0x38 => store!(4, u32, |x: u64| x as u32),
            0x39 => store!(8, u64, |x: u64| x),
            0x3a => store!(1, u8, |x: u64| x as u32),
            0x3b => store!(2, u16, |x: u64| x as u32),
            0x3c => store!(1, u8, |x: u64| x),
            0x3d => store!(2, u16, |x: u64| x),
            0x3e => store!(4, u32, |x: u64| x),
            0x3f => {
                *sp = 0;
                set_u32(sp, (msz / WASM_PAGE as u64) as u32);
                sp = sp.add(1);
                next!(1);
            }
            0x40 => {
                let r = wasm_memory_grow(mem, u32_at(sp.sub(1)));
                set_u32(sp.sub(1), r as i32 as u32);
                reload!();
                next!(1);
            }
            OP_CONST => {
                *sp = *ip.add(1);
                sp = sp.add(1);
                next!(2);
            }
            0x45 => {
                set_u32(sp.sub(1), (u32_at(sp.sub(1)) == 0) as u32);
                next!(1);
            }
            0x46 => cmp32!(u32, ==),
            0x47 => cmp32!(u32, !=),
            0x48 => cmp32!(i32, <),
            0x49 => cmp32!(u32, <),
            0x4a => cmp32!(i32, >),
            0x4b => cmp32!(u32, >),
            0x4c => cmp32!(i32, <=),
            0x4d => cmp32!(u32, <=),
            0x4e => cmp32!(i32, >=),
            0x4f => cmp32!(u32, >=),
            0x50 => {
                set_u32(sp.sub(1), (*sp.sub(1) == 0) as u32);
                next!(1);
            }
            0x51 => cmp64!(u64, ==),
            0x52 => cmp64!(u64, !=),
            0x53 => cmp64!(i64, <),
            0x54 => cmp64!(u64, <),
            0x55 => cmp64!(i64, >),
            0x56 => cmp64!(u64, >),
            0x57 => cmp64!(i64, <=),
            0x58 => cmp64!(u64, <=),
            0x59 => cmp64!(i64, >=),
            0x5a => cmp64!(u64, >=),
            0x5b => cmpf32!(==),
            0x5c => cmpf32!(!=),
            0x5d => cmpf32!(<),
            0x5e => cmpf32!(>),
            0x5f => cmpf32!(<=),
            0x60 => cmpf32!(>=),
            0x61 => cmpf64!(==),
            0x62 => cmpf64!(!=),
            0x63 => cmpf64!(<),
            0x64 => cmpf64!(>),
            0x65 => cmpf64!(<=),
            0x66 => cmpf64!(>=),
            0x67 => {
                let v = u32_at(sp.sub(1));
                set_u32(sp.sub(1), if v != 0 { v.leading_zeros() } else { 32 });
                next!(1);
            }
            0x68 => {
                let v = u32_at(sp.sub(1));
                set_u32(sp.sub(1), if v != 0 { v.trailing_zeros() } else { 32 });
                next!(1);
            }
            0x69 => {
                set_u32(sp.sub(1), u32_at(sp.sub(1)).count_ones());
                next!(1);
            }
            0x6a => bin32!(a, b, a.wrapping_add(b)),
            0x6b => bin32!(a, b, a.wrapping_sub(b)),
            0x6c => bin32!(a, b, a.wrapping_mul(b)),
            0x6d => {
                let a = u32_at(sp.sub(2)) as i32;
                let b = u32_at(sp.sub(1)) as i32;
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                if a == i32::MIN && b == -1 {
                    trap!(b"integer overflow");
                }
                set_u32(sp.sub(2), (a / b) as u32);
                sp = sp.sub(1);
                next!(1);
            }
            0x6e => {
                let a = u32_at(sp.sub(2));
                let b = u32_at(sp.sub(1));
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                set_u32(sp.sub(2), a / b);
                sp = sp.sub(1);
                next!(1);
            }
            0x6f => {
                let a = u32_at(sp.sub(2)) as i32;
                let b = u32_at(sp.sub(1)) as i32;
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                set_u32(sp.sub(2), if b == -1 { 0 } else { (a % b) as u32 });
                sp = sp.sub(1);
                next!(1);
            }
            0x70 => {
                let a = u32_at(sp.sub(2));
                let b = u32_at(sp.sub(1));
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                set_u32(sp.sub(2), a % b);
                sp = sp.sub(1);
                next!(1);
            }
            0x71 => bin32!(a, b, a & b),
            0x72 => bin32!(a, b, a | b),
            0x73 => bin32!(a, b, a ^ b),
            0x74 => bin32!(a, b, a << (b & 31)),
            0x75 => {
                let a = u32_at(sp.sub(2)) as i32;
                let b = u32_at(sp.sub(1));
                set_u32(sp.sub(2), (a >> (b & 31)) as u32);
                sp = sp.sub(1);
                next!(1);
            }
            0x76 => bin32!(a, b, a >> (b & 31)),
            0x77 => bin32!(a, b, a.rotate_left(b & 31)),
            0x78 => bin32!(a, b, a.rotate_right(b & 31)),
            0x79 => {
                let v = *sp.sub(1);
                *sp.sub(1) = if v != 0 { v.leading_zeros() as u64 } else { 64 };
                next!(1);
            }
            0x7a => {
                let v = *sp.sub(1);
                *sp.sub(1) = if v != 0 { v.trailing_zeros() as u64 } else { 64 };
                next!(1);
            }
            0x7b => {
                *sp.sub(1) = (*sp.sub(1)).count_ones() as u64;
                next!(1);
            }
            0x7c => bin64!(a, b, a.wrapping_add(b)),
            0x7d => bin64!(a, b, a.wrapping_sub(b)),
            0x7e => bin64!(a, b, a.wrapping_mul(b)),
            0x7f => {
                let a = *sp.sub(2) as i64;
                let b = *sp.sub(1) as i64;
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                if a == i64::MIN && b == -1 {
                    trap!(b"integer overflow");
                }
                *sp.sub(2) = (a / b) as u64;
                sp = sp.sub(1);
                next!(1);
            }
            0x80 => {
                let a = *sp.sub(2);
                let b = *sp.sub(1);
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                *sp.sub(2) = a / b;
                sp = sp.sub(1);
                next!(1);
            }
            0x81 => {
                let a = *sp.sub(2) as i64;
                let b = *sp.sub(1) as i64;
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                *sp.sub(2) = if b == -1 { 0 } else { (a % b) as u64 };
                sp = sp.sub(1);
                next!(1);
            }
            0x82 => {
                let a = *sp.sub(2);
                let b = *sp.sub(1);
                if b == 0 {
                    trap!(b"integer divide by zero");
                }
                *sp.sub(2) = a % b;
                sp = sp.sub(1);
                next!(1);
            }
            0x83 => bin64!(a, b, a & b),
            0x84 => bin64!(a, b, a | b),
            0x85 => bin64!(a, b, a ^ b),
            0x86 => bin64!(a, b, a << (b & 63)),
            0x87 => {
                let a = *sp.sub(2) as i64;
                let b = *sp.sub(1);
                *sp.sub(2) = (a >> (b & 63)) as u64;
                sp = sp.sub(1);
                next!(1);
            }
            0x88 => bin64!(a, b, a >> (b & 63)),
            0x89 => bin64!(a, b, a.rotate_left((b & 63) as u32)),
            0x8a => bin64!(a, b, a.rotate_right((b & 63) as u32)),
            0x8b => {
                set_u32(sp.sub(1), u32_at(sp.sub(1)) & 0x7fffffff);
                next!(1);
            }
            0x8c => {
                set_u32(sp.sub(1), u32_at(sp.sub(1)) ^ 0x80000000);
                next!(1);
            }
            0x8d => {
                let x = f32_at(sp.sub(1));
                set_f32(sp.sub(1), q32(x, x.ceil()));
                next!(1);
            }
            0x8e => {
                let x = f32_at(sp.sub(1));
                set_f32(sp.sub(1), q32(x, x.floor()));
                next!(1);
            }
            0x8f => {
                let x = f32_at(sp.sub(1));
                set_f32(sp.sub(1), q32(x, x.trunc()));
                next!(1);
            }
            0x90 => {
                let x = f32_at(sp.sub(1));
                set_f32(sp.sub(1), q32(x, x.round_ties_even()));
                next!(1);
            }
            0x91 => {
                let x = f32_at(sp.sub(1));
                set_f32(sp.sub(1), x.sqrt());
                next!(1);
            }
            0x92 => {
                let r = f32_at(sp.sub(2)) + f32_at(sp.sub(1));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x93 => {
                let r = f32_at(sp.sub(2)) - f32_at(sp.sub(1));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x94 => {
                let r = f32_at(sp.sub(2)) * f32_at(sp.sub(1));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x95 => {
                let r = f32_at(sp.sub(2)) / f32_at(sp.sub(1));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x96 => {
                let r = fmin32(f32_at(sp.sub(2)), f32_at(sp.sub(1)));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x97 => {
                let r = fmax32(f32_at(sp.sub(2)), f32_at(sp.sub(1)));
                set_f32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x98 => {
                let r = (u32_at(sp.sub(2)) & 0x7fffffff) | (u32_at(sp.sub(1)) & 0x80000000);
                set_u32(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0x99 => {
                *sp.sub(1) &= 0x7fffffffffffffff;
                next!(1);
            }
            0x9a => {
                *sp.sub(1) ^= 0x8000000000000000;
                next!(1);
            }
            0x9b => {
                let x = f64_at(sp.sub(1));
                set_f64(sp.sub(1), q64(x, x.ceil()));
                next!(1);
            }
            0x9c => {
                let x = f64_at(sp.sub(1));
                set_f64(sp.sub(1), q64(x, x.floor()));
                next!(1);
            }
            0x9d => {
                let x = f64_at(sp.sub(1));
                set_f64(sp.sub(1), q64(x, x.trunc()));
                next!(1);
            }
            0x9e => {
                let x = f64_at(sp.sub(1));
                set_f64(sp.sub(1), q64(x, x.round_ties_even()));
                next!(1);
            }
            0x9f => {
                let x = f64_at(sp.sub(1));
                set_f64(sp.sub(1), x.sqrt());
                next!(1);
            }
            0xa0 => {
                let r = f64_at(sp.sub(2)) + f64_at(sp.sub(1));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa1 => {
                let r = f64_at(sp.sub(2)) - f64_at(sp.sub(1));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa2 => {
                let r = f64_at(sp.sub(2)) * f64_at(sp.sub(1));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa3 => {
                let r = f64_at(sp.sub(2)) / f64_at(sp.sub(1));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa4 => {
                let r = fmin64(f64_at(sp.sub(2)), f64_at(sp.sub(1)));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa5 => {
                let r = fmax64(f64_at(sp.sub(2)), f64_at(sp.sub(1)));
                set_f64(sp.sub(2), r);
                sp = sp.sub(1);
                next!(1);
            }
            0xa6 => {
                let r = (*sp.sub(2) & 0x7fffffffffffffff) | (*sp.sub(1) & 0x8000000000000000);
                *sp.sub(2) = r;
                sp = sp.sub(1);
                next!(1);
            }
            0xa7 => next!(1),
            0xa8 => trunc!(f32_at(sp.sub(1)) as f64, |x: f64| x >= -2147483648.0, 2147483648.0, |x: f64| {
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), (x as i32) as u32)
            }),
            0xa9 => trunc!(f32_at(sp.sub(1)) as f64, |x: f64| x > -1.0, 4294967296.0, |x: f64| {
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), x as u32)
            }),
            0xaa => trunc!(f64_at(sp.sub(1)), |x: f64| x > -2147483649.0, 2147483648.0, |x: f64| {
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), (x as i32) as u32)
            }),
            0xab => trunc!(f64_at(sp.sub(1)), |x: f64| x > -1.0, 4294967296.0, |x: f64| {
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), x as u32)
            }),
            0xac => {
                *sp.sub(1) = u32_at(sp.sub(1)) as i32 as i64 as u64;
                next!(1);
            }
            0xad => {
                *sp.sub(1) = u32_at(sp.sub(1)) as u64;
                next!(1);
            }
            0xae => trunc!(f32_at(sp.sub(1)) as f64, |x: f64| x >= -9223372036854775808.0, 9223372036854775808.0, |x: f64| *sp.sub(1) = (x as i64) as u64),
            0xaf => trunc!(f32_at(sp.sub(1)) as f64, |x: f64| x > -1.0, 18446744073709551616.0, |x: f64| *sp.sub(1) = x as u64),
            0xb0 => trunc!(f64_at(sp.sub(1)), |x: f64| x >= -9223372036854775808.0, 9223372036854775808.0, |x: f64| *sp.sub(1) = (x as i64) as u64),
            0xb1 => trunc!(f64_at(sp.sub(1)), |x: f64| x > -1.0, 18446744073709551616.0, |x: f64| *sp.sub(1) = x as u64),
            0xb2 => {
                let f = u32_at(sp.sub(1)) as i32 as f32;
                *sp.sub(1) = 0;
                set_f32(sp.sub(1), f);
                next!(1);
            }
            0xb3 => {
                let f = u32_at(sp.sub(1)) as f32;
                *sp.sub(1) = 0;
                set_f32(sp.sub(1), f);
                next!(1);
            }
            0xb4 => {
                let f = *sp.sub(1) as i64 as f32;
                *sp.sub(1) = 0;
                set_f32(sp.sub(1), f);
                next!(1);
            }
            0xb5 => {
                let f = *sp.sub(1) as f32;
                *sp.sub(1) = 0;
                set_f32(sp.sub(1), f);
                next!(1);
            }
            0xb6 => {
                let f = f64_at(sp.sub(1)) as f32;
                *sp.sub(1) = 0;
                set_f32(sp.sub(1), f);
                next!(1);
            }
            0xb7 => {
                set_f64(sp.sub(1), u32_at(sp.sub(1)) as i32 as f64);
                next!(1);
            }
            0xb8 => {
                set_f64(sp.sub(1), u32_at(sp.sub(1)) as f64);
                next!(1);
            }
            0xb9 => {
                set_f64(sp.sub(1), *sp.sub(1) as i64 as f64);
                next!(1);
            }
            0xba => {
                set_f64(sp.sub(1), *sp.sub(1) as f64);
                next!(1);
            }
            0xbb => {
                set_f64(sp.sub(1), f32_at(sp.sub(1)) as f64);
                next!(1);
            }
            0xc0 => {
                set_u32(sp.sub(1), u32_at(sp.sub(1)) as u8 as i8 as i32 as u32);
                next!(1);
            }
            0xc1 => {
                set_u32(sp.sub(1), u32_at(sp.sub(1)) as u16 as i16 as i32 as u32);
                next!(1);
            }
            0xc2 => {
                *sp.sub(1) = *sp.sub(1) as u8 as i8 as i64 as u64;
                next!(1);
            }
            0xc3 => {
                *sp.sub(1) = *sp.sub(1) as u16 as i16 as i64 as u64;
                next!(1);
            }
            0xc4 => {
                *sp.sub(1) = *sp.sub(1) as u32 as i32 as i64 as u64;
                next!(1);
            }
            0xd1 => {
                *sp.sub(1) = (*sp.sub(1) == 0) as u64;
                next!(1);
            }
            0xd2 => {
                *sp = (*inst).funcs[*ip.add(1) as usize] as usize as u64;
                sp = sp.add(1);
                next!(2);
            }
            0x100 => {
                let v = sat_i32(f32_at(sp.sub(1)) as f64);
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), v as u32);
                next!(1);
            }
            0x101 => {
                let v = sat_u32(f32_at(sp.sub(1)) as f64);
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), v);
                next!(1);
            }
            0x102 => {
                let v = sat_i32(f64_at(sp.sub(1)));
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), v as u32);
                next!(1);
            }
            0x103 => {
                let v = sat_u32(f64_at(sp.sub(1)));
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), v);
                next!(1);
            }
            0x104 => {
                *sp.sub(1) = sat_i64(f32_at(sp.sub(1)) as f64) as u64;
                next!(1);
            }
            0x105 => {
                *sp.sub(1) = sat_u64(f32_at(sp.sub(1)) as f64);
                next!(1);
            }
            0x106 => {
                *sp.sub(1) = sat_i64(f64_at(sp.sub(1))) as u64;
                next!(1);
            }
            0x107 => {
                *sp.sub(1) = sat_u64(f64_at(sp.sub(1)));
                next!(1);
            }
            0x108 => {
                let seg = *ip.add(1) as usize;
                let m = &*(*inst).m;
                let s = &m.data[seg];
                let d = u32_at(sp.sub(3)) as u64;
                let src = u32_at(sp.sub(2)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                let len = if (*inst).data_dropped[seg] != 0 { 0 } else { s.len as u64 };
                sp = sp.sub(3);
                if src + n > len || d + n > msz {
                    trap!(b"out of bounds memory access");
                }
                std::ptr::copy_nonoverlapping(m.bytes.as_ptr().add(s.off + src as usize), mb.add(d as usize), n as usize);
                next!(2);
            }
            0x109 => {
                (*inst).data_dropped[*ip.add(1) as usize] = 1;
                next!(2);
            }
            0x10a => {
                let d = u32_at(sp.sub(3)) as u64;
                let s = u32_at(sp.sub(2)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                sp = sp.sub(3);
                if s + n > msz || d + n > msz {
                    trap!(b"out of bounds memory access");
                }
                std::ptr::copy(mb.add(s as usize), mb.add(d as usize), n as usize);
                next!(1);
            }
            0x10b => {
                let d = u32_at(sp.sub(3)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                let v = u32_at(sp.sub(2)) as u8;
                sp = sp.sub(3);
                if d + n > msz {
                    trap!(b"out of bounds memory access");
                }
                std::ptr::write_bytes(mb.add(d as usize), v, n as usize);
                next!(1);
            }
            0x10c => {
                let seg = *ip.add(1) as usize;
                let m = &*(*inst).m;
                let s = &m.elems[seg];
                let t = (*inst).tables[*ip.add(2) as usize];
                let d = u32_at(sp.sub(3)) as u64;
                let src = u32_at(sp.sub(2)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                let len = if (*inst).elem_dropped[seg] != 0 { 0 } else { s.items.len() as u64 };
                sp = sp.sub(3);
                if src + n > len || d + n > (*t).size as u64 {
                    trap!(b"out of bounds table access");
                }
                for i in 0..n {
                    let it = &s.items[(src + i) as usize];
                    let mut v = WasmVal::ZERO;
                    if it.is_expr {
                        v = const_eval(inst, it.expr);
                    } else {
                        v.r#ref = (*inst).funcs[it.func as usize].cast();
                    }
                    wasm_table_set(t, (d + i) as u32, v);
                }
                next!(3);
            }
            0x10d => {
                (*inst).elem_dropped[*ip.add(1) as usize] = 1;
                next!(2);
            }
            0x10e => {
                let td = (*inst).tables[*ip.add(1) as usize];
                let ts = (*inst).tables[*ip.add(2) as usize];
                let d = u32_at(sp.sub(3)) as u64;
                let s = u32_at(sp.sub(2)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                sp = sp.sub(3);
                if s + n > (*ts).size as u64 || d + n > (*td).size as u64 {
                    trap!(b"out of bounds table access");
                }
                if d <= s {
                    for i in 0..n {
                        wasm_table_set(td, (d + i) as u32, (*ts).elems[(s + i) as usize]);
                    }
                } else {
                    for i in (0..n).rev() {
                        wasm_table_set(td, (d + i) as u32, (*ts).elems[(s + i) as usize]);
                    }
                }
                next!(3);
            }
            0x10f => {
                let t = (*inst).tables[*ip.add(1) as usize];
                let r = wasm_table_grow(t, u32_at(sp.sub(1)), WasmVal { u64: *sp.sub(2) });
                sp = sp.sub(1);
                *sp.sub(1) = 0;
                set_u32(sp.sub(1), r as i32 as u32);
                next!(2);
            }
            0x110 => {
                *sp = (*(*inst).tables[*ip.add(1) as usize]).size as u64;
                sp = sp.add(1);
                next!(2);
            }
            0x111 => {
                let t = (*inst).tables[*ip.add(1) as usize];
                let i = u32_at(sp.sub(3)) as u64;
                let n = u32_at(sp.sub(1)) as u64;
                let v = WasmVal { u64: *sp.sub(2) };
                sp = sp.sub(3);
                if i + n > (*t).size as u64 {
                    trap!(b"out of bounds table access");
                }
                for k in 0..n {
                    wasm_table_set(t, (i + k) as u32, v);
                }
                next!(2);
            }
            _ => trap!(b"unreachable"),
        }
    };
    // trap
    set_err(err, trapkind, trapmsg);
    unwind(entry_depth, inst, base)
}

unsafe fn unwind(entry_depth: usize, mut inst: *mut WasmInstance, base: *mut u64) -> c_int {
    let mach = machine();
    while mach.frames.len() > entry_depth {
        let fr = machine().frames.pop().expect("frame");
        if fr.crossed {
            wasm_instance_unref(inst);
        }
        inst = fr.inst;
    }
    machine().cur_sp = base;
    -1
}

#[no_mangle]
pub unsafe extern "C" fn wasm_call(f: *mut WasmFunc, args: *const WasmVal, results: *mut WasmVal, err: *mut WasmError) -> c_int {
    if !err.is_null() {
        (*err).kind = WASM_OK;
    }
    if !stacks_init() {
        set_err(err, WASM_ERR_NOMEM, b"out of memory for the stack");
        return -1;
    }
    let t = (*f).ty;
    let (np, nr) = ((*t).nparams as usize, (*t).nresults as usize);
    let base = machine().cur_sp;
    if base.add(np + nr + 16) > machine().vstack_end {
        set_err(err, WASM_ERR_EXHAUSTED, b"call stack exhausted");
        return -1;
    }
    if np != 0 {
        std::ptr::copy_nonoverlapping(args.cast::<u64>(), base, np);
    }
    let r;
    if let Some(host) = (*f).host {
        machine().cur_sp = base.add(np + nr);
        let mut herr = WasmError { kind: WASM_OK, msg: [0; 160] };
        r = host((*f).ud, base.cast::<WasmVal>(), base.add(np).cast::<WasmVal>(), &mut herr);
        if r != 0 && !err.is_null() {
            *err = herr;
        }
        if r == 0 {
            std::ptr::copy(base.add(np), base, nr);
        }
    } else {
        let inst = (*f).inst;
        wasm_instance_ref(inst);
        r = exec(f, base, err);
        wasm_instance_unref(inst);
    }
    if r == 0 && nr != 0 {
        std::ptr::copy_nonoverlapping(base, results.cast::<u64>(), nr);
    }
    machine().cur_sp = base;
    r
}

// ---- instantiation ----

fn limits_match(min: u32, max: u32, want_min: u32, want_max: u32) -> bool {
    if min < want_min {
        return false;
    }
    if want_max != WASM_NO_MAX && (max == WASM_NO_MAX || max > want_max) {
        return false;
    }
    true
}

pub(crate) unsafe fn instance_free(inst: *mut WasmInstance) {
    let i = &mut *inst;
    let m = &*i.m;
    for k in 0..i.tables.len() {
        let t = i.tables[k];
        if t.is_null() {
            continue;
        }
        if k < m.ntable_imports as usize {
            wasm_table_unref(t);
        } else {
            table_destroy(t);
        }
    }
    for k in 0..i.globals.len() {
        let g = i.globals[k];
        if g.is_null() {
            continue;
        }
        if k < m.nglobal_imports as usize {
            wasm_global_unref(g);
        } else {
            global_destroy(g);
        }
    }
    wasm_memory_unref(i.mem);
    for k in 0..m.nfunc_imports as usize {
        if !i.funcs[k].is_null() {
            wasm_func_unref(i.funcs[k]);
        }
    }
    let mp = i.m;
    drop(Box::from_raw(inst));
    wasm_module_unref(mp);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instantiate(m: *mut WasmModule, imports: *const WasmExtern, nimports: u32, err: *mut WasmError) -> *mut WasmInstance {
    if !err.is_null() {
        (*err).kind = WASM_OK;
    }
    let md = &*m;
    if nimports as usize != md.imports.len() {
        set_err(err, WASM_ERR_LINK, format!("se esperaban {} imports", md.imports.len()).as_bytes());
        return null_mut();
    }
    let imps: &[WasmExtern] = if nimports == 0 { &[] } else { std::slice::from_raw_parts(imports, nimports as usize) };
    for (i, im) in md.imports.iter().enumerate() {
        let e = &imps[i];
        let mut why = false;
        if e.kind != im.ty.kind || e.ptr.is_null() {
            why = true;
        } else {
            match e.kind {
                WASM_EXT_FUNC => {
                    if (*e.ptr.cast::<WasmFunc>()).ty != im.ty.func {
                        why = true;
                    }
                }
                WASM_EXT_TABLE => {
                    let t = &*e.ptr.cast::<WasmTable>();
                    if t.ty != im.ty.valtype || !limits_match(t.size, t.max, im.ty.min, im.ty.max) {
                        why = true;
                    }
                }
                WASM_EXT_MEMORY => {
                    let mm = &*e.ptr.cast::<WasmMemory>();
                    if !limits_match((mm.size / WASM_PAGE as usize) as u32, mm.max, im.ty.min, im.ty.max) {
                        why = true;
                    }
                }
                WASM_EXT_GLOBAL => {
                    let g = &*e.ptr.cast::<WasmGlobal>();
                    if g.ty != im.ty.valtype || g.mutable as c_int != im.ty.r#mut {
                        why = true;
                    }
                }
                _ => {}
            }
        }
        if why {
            // module and name are C strings here ("%s (%s.%s)")
            let z = |v: &Vec<u8>| v[..v.iter().position(|&b| b == 0).unwrap_or(v.len())].to_vec();
            let mut msg = b"incompatible import type (".to_vec();
            msg.extend_from_slice(&z(&im.module));
            msg.push(b'.');
            msg.extend_from_slice(&z(&im.name));
            msg.push(b')');
            set_err(err, WASM_ERR_LINK, &msg);
            return null_mut();
        }
    }

    let ndef = md.functypes.len() - md.nfunc_imports as usize;
    let mut own: Vec<WasmFunc> = Vec::with_capacity(ndef + 1);
    for _ in 0..ndef + 1 {
        own.push(WasmFunc::blank());
    }
    let inst = Box::into_raw(Box::new(WasmInstance {
        rc: 1,
        m,
        funcs: vec![null_mut(); md.functypes.len() + 1],
        own: own.into_boxed_slice(),
        tables: vec![null_mut(); md.tables.len() + 1],
        mem: null_mut(),
        globals: vec![null_mut(); md.globals.len() + 1],
        elem_dropped: vec![0; md.elems.len() + 1],
        data_dropped: vec![0; md.ndata as usize + 1],
        exports: vec![WasmExtern { kind: -1, ptr: null_mut() }; md.exports.len() + 1],
    }));
    wasm_module_ref(m);
    // The vectors above carry one spare slot like the C arrays; the logical lengths are the module's.
    (*inst).funcs.truncate(md.functypes.len());
    (*inst).tables.truncate(md.tables.len());
    (*inst).globals.truncate(md.globals.len());
    (*inst).exports.truncate(md.exports.len());

    macro_rules! fail {
        () => {{
            wasm_instance_unref(inst);
            return null_mut();
        }};
    }

    // imports
    let (mut fi, mut ti, mut gi) = (0usize, 0usize, 0usize);
    for e in imps {
        match e.kind {
            WASM_EXT_FUNC => {
                (*inst).funcs[fi] = e.ptr.cast();
                fi += 1;
                wasm_func_ref(e.ptr.cast());
            }
            WASM_EXT_TABLE => {
                (*inst).tables[ti] = e.ptr.cast();
                ti += 1;
                wasm_table_ref(e.ptr.cast());
            }
            WASM_EXT_MEMORY => {
                (*inst).mem = e.ptr.cast();
                wasm_memory_ref(e.ptr.cast());
            }
            WASM_EXT_GLOBAL => {
                (*inst).globals[gi] = e.ptr.cast();
                gi += 1;
                wasm_global_ref(e.ptr.cast());
            }
            _ => {}
        }
    }
    for i in 0..ndef {
        let f = &mut (*inst).own[i] as *mut WasmFunc;
        (*f).ty = md.functypes[md.nfunc_imports as usize + i];
        (*f).inst = inst;
        (*f).code = &md.code[i];
        (*inst).funcs[md.nfunc_imports as usize + i] = f;
    }
    for i in md.ntable_imports as usize..md.tables.len() {
        let t = table_alloc(inst, md.tables[i].ty, md.tables[i].min, md.tables[i].max, WasmVal::ZERO, err);
        if t.is_null() {
            fail!();
        }
        (*inst).tables[i] = t;
    }
    if md.nmems != 0 && md.nmem_imports == 0 {
        (*inst).mem = wasm_memory_new(md.mem_min, md.mem_max, err);
        if (*inst).mem.is_null() {
            fail!();
        }
    }
    for i in md.nglobal_imports as usize..md.globals.len() {
        let v = const_eval(inst, md.globals[i].init);
        (*inst).globals[i] = global_alloc(inst, md.globals[i].ty, md.globals[i].mutable as c_int, v);
    }
    for (i, x) in md.exports.iter().enumerate() {
        let e = &mut (*inst).exports[i];
        e.kind = x.kind;
        e.ptr = match x.kind {
            WASM_EXT_FUNC => (*inst).funcs[x.idx as usize].cast(),
            WASM_EXT_TABLE => (*inst).tables[x.idx as usize].cast(),
            WASM_EXT_MEMORY => (*inst).mem.cast(),
            _ => (*inst).globals[x.idx as usize].cast(),
        };
    }
    // element segments: active ones in order; declarative ones are discarded
    for (i, s) in md.elems.iter().enumerate() {
        if s.mode == 1 {
            continue;
        }
        (*inst).elem_dropped[i] = 1;
        if s.mode == 2 {
            continue;
        }
        let off = const_eval(inst, s.offset).u32 as u64;
        let t = (*inst).tables[s.table as usize];
        if off + s.items.len() as u64 > (*t).size as u64 {
            set_err(err, WASM_ERR_TRAP, b"out of bounds table access");
            fail!();
        }
        for (k, it) in s.items.iter().enumerate() {
            let mut v = WasmVal::ZERO;
            if it.is_expr {
                v = const_eval(inst, it.expr);
            } else {
                v.r#ref = (*inst).funcs[it.func as usize].cast();
            }
            wasm_table_set(t, (off + k as u64) as u32, v);
        }
    }
    for (i, s) in md.data.iter().enumerate() {
        if !s.active {
            continue;
        }
        (*inst).data_dropped[i] = 1;
        let off = const_eval(inst, s.offset).u32 as u64;
        if off + s.len as u64 > (*(*inst).mem).size as u64 {
            set_err(err, WASM_ERR_TRAP, b"out of bounds memory access");
            fail!();
        }
        std::ptr::copy_nonoverlapping(md.bytes.as_ptr().add(s.off), (*(*inst).mem).data.add(off as usize), s.len as usize);
    }
    if md.start >= 0 && wasm_call((*inst).funcs[md.start as usize], null(), null_mut(), err) != 0 {
        fail!();
    }
    inst
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_export(i: *const WasmInstance, idx: u32) -> WasmExtern {
    match (*i).exports.get(idx as usize) {
        Some(e) => *e,
        None => WasmExtern { kind: -1, ptr: null_mut() },
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_export_find_len(i: *const WasmInstance, name: *const c_char, len: usize) -> WasmExtern {
    let name = if len == 0 { &[][..] } else { std::slice::from_raw_parts(name.cast::<u8>(), len) };
    let m = &*(*i).m;
    for (k, e) in m.exports.iter().enumerate() {
        if &e.name[..e.name.len() - 1] == name {
            return (*i).exports[k];
        }
    }
    WasmExtern { kind: -1, ptr: null_mut() }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_export_find(i: *const WasmInstance, name: *const c_char) -> WasmExtern {
    wasm_instance_export_find_len(i, name, std::ffi::CStr::from_ptr(name).to_bytes().len())
}
