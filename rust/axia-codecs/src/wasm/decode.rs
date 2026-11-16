//! WebAssembly decoder, validator and compiler (the former wasm.c).
//!
//! A single pass per function: it validates the types with the algorithm from the specification's
//! appendix and at the same time generates the internal code. Since the validator knows the exact stack
//! height at every point, jumps come out with their target already resolved and with how many values
//! to move, without looking up labels at run time. Unreachable code is validated but not generated.
use super::*;
use core::ffi::c_int;

// Internal instructions (the others use their WebAssembly byte, 0x100 + n for the 0xFC prefix).
pub(crate) const OP_BR: u64 = 0x200;
pub(crate) const OP_BR_N: u64 = 0x201;
pub(crate) const OP_BR_IF: u64 = 0x202;
pub(crate) const OP_BR_IF_N: u64 = 0x203;
pub(crate) const OP_BR_UNLESS: u64 = 0x204;
pub(crate) const OP_BR_TABLE: u64 = 0x205;
pub(crate) const OP_RETURN: u64 = 0x206;
pub(crate) const OP_RETURN0: u64 = 0x207;
pub(crate) const OP_RETURN1: u64 = 0x208;
pub(crate) const OP_CALL: u64 = 0x209;
pub(crate) const OP_CALL_INDIRECT: u64 = 0x20a;
pub(crate) const OP_CONST: u64 = 0x20b;

const UNKNOWN: u8 = 0;

type R<T> = Result<T, ()>;

struct Ctrl {
    kind: u8, // 0 function, 1 block, 2 loop, 3 if, 4 else
    params: &'static [u8],
    results: &'static [u8],
    height: u32, // type stack height below the parameters
    unreachable: bool,
    emit: bool,
    loop_pc: usize,
    else_op: usize, // the if's BR_UNLESS (usize::MAX if not generated)
    patches: i32,   // jumps to the end, still to be resolved
}

struct Patch {
    op: usize,
    cell: usize,
    next: i32,
}

struct Dec {
    err: *mut WasmError,
    m: WasmModule,
    base: *const u8,
    p: usize,
    end: usize,
    // function being compiled
    vals: Vec<u8>,
    maxvals: u32,
    ctrls: Vec<Ctrl>,
    code: Vec<u64>,
    patches: Vec<Patch>,
    locals: Vec<u8>,
    ftype: *const WasmFuncType,
}

fn is_num(t: u8) -> bool {
    t == WASM_I32 || t == WASM_I64 || t == WASM_F32 || t == WASM_F64
}
fn is_ref(t: u8) -> bool {
    t == WASM_FUNCREF || t == WASM_EXTERNREF
}

fn utf8_ok(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let (len, mut cp, min);
        if c < 0x80 {
            i += 1;
            continue;
        } else if c & 0xe0 == 0xc0 {
            (len, cp, min) = (2, (c & 0x1f) as u32, 0x80);
        } else if c & 0xf0 == 0xe0 {
            (len, cp, min) = (3, (c & 0x0f) as u32, 0x800);
        } else if c & 0xf8 == 0xf0 {
            (len, cp, min) = (4, (c & 0x07) as u32, 0x10000);
        } else {
            return false;
        }
        if i + len > s.len() {
            return false;
        }
        for k in 1..len {
            if s[i + k] & 0xc0 != 0x80 {
                return false;
            }
            cp = cp << 6 | (s[i + k] & 0x3f) as u32;
        }
        if cp < min || cp > 0x10ffff || (0xd800..=0xdfff).contains(&cp) {
            return false;
        }
        i += len;
    }
    true
}

/// Types of the numeric instructions without immediates (0x45..0xC4): input(s) and output.
fn numsig(op: u8) -> (u8, u8, u8) {
    const RANGES: [(u8, u8, u8, u8, u8); 31] = [
        (0x45, 0x45, WASM_I32, 0, WASM_I32), (0x46, 0x4f, WASM_I32, WASM_I32, WASM_I32),
        (0x50, 0x50, WASM_I64, 0, WASM_I32), (0x51, 0x5a, WASM_I64, WASM_I64, WASM_I32),
        (0x5b, 0x60, WASM_F32, WASM_F32, WASM_I32), (0x61, 0x66, WASM_F64, WASM_F64, WASM_I32),
        (0x67, 0x69, WASM_I32, 0, WASM_I32), (0x6a, 0x78, WASM_I32, WASM_I32, WASM_I32),
        (0x79, 0x7b, WASM_I64, 0, WASM_I64), (0x7c, 0x8a, WASM_I64, WASM_I64, WASM_I64),
        (0x8b, 0x91, WASM_F32, 0, WASM_F32), (0x92, 0x98, WASM_F32, WASM_F32, WASM_F32),
        (0x99, 0x9f, WASM_F64, 0, WASM_F64), (0xa0, 0xa6, WASM_F64, WASM_F64, WASM_F64),
        (0xa7, 0xa7, WASM_I64, 0, WASM_I32), (0xa8, 0xa9, WASM_F32, 0, WASM_I32),
        (0xaa, 0xab, WASM_F64, 0, WASM_I32), (0xac, 0xad, WASM_I32, 0, WASM_I64),
        (0xae, 0xaf, WASM_F32, 0, WASM_I64), (0xb0, 0xb1, WASM_F64, 0, WASM_I64),
        (0xb2, 0xb3, WASM_I32, 0, WASM_F32), (0xb4, 0xb5, WASM_I64, 0, WASM_F32),
        (0xb6, 0xb6, WASM_F64, 0, WASM_F32), (0xb7, 0xb8, WASM_I32, 0, WASM_F64),
        (0xb9, 0xba, WASM_I64, 0, WASM_F64), (0xbb, 0xbb, WASM_F32, 0, WASM_F64),
        (0xbc, 0xbc, WASM_F32, 0, WASM_I32), (0xbd, 0xbd, WASM_F64, 0, WASM_I64),
        (0xbe, 0xbe, WASM_I32, 0, WASM_F32), (0xbf, 0xbf, WASM_I64, 0, WASM_F64),
        (0xc0, 0xc1, WASM_I32, 0, WASM_I32),
    ];
    const RANGES2: (u8, u8, u8, u8, u8) = (0xc2, 0xc4, WASM_I64, 0, WASM_I64);
    for r in RANGES.iter().chain(std::iter::once(&RANGES2)) {
        if op >= r.0 && op <= r.1 {
            return (r.2, r.3, r.4);
        }
    }
    (0, 0, 0)
}

const VT_SINGLE: [u8; 6] = [WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_FUNCREF, WASM_EXTERNREF];

fn single(t: u8) -> &'static [u8] {
    match VT_SINGLE.iter().position(|&x| x == t) {
        Some(i) => &VT_SINGLE[i..i + 1],
        None => &[],
    }
}

impl Dec {
    fn bytes(&self) -> &[u8] {
        // SAFETY: base points into the module's byte buffer, which is never resized while decoding.
        unsafe { std::slice::from_raw_parts(self.base, self.m.bytes.len()) }
    }

    fn fail<T>(&mut self, kind: c_int, msg: String) -> R<T> {
        unsafe { set_err(self.err, kind, msg.as_bytes()) };
        Err(())
    }
    fn mal<T>(&mut self, msg: impl Into<String>) -> R<T> {
        self.fail(WASM_ERR_MALFORMED, msg.into())
    }
    fn inv<T>(&mut self, msg: impl Into<String>) -> R<T> {
        self.fail(WASM_ERR_INVALID, msg.into())
    }

    // ---- reading ----

    fn u8(&mut self) -> R<u8> {
        if self.p >= self.end {
            return self.mal("unexpected end");
        }
        let b = self.bytes()[self.p];
        self.p += 1;
        Ok(b)
    }

    fn leb(&mut self, bits: u32, sign: bool) -> R<u64> {
        let mut r: u64 = 0;
        let mut shift = 0u32;
        let maxb = (bits + 6) / 7;
        let mut b;
        let mut i = 0;
        loop {
            if i >= maxb {
                return self.mal("integer representation too long");
            }
            b = self.u8()?;
            if i == maxb - 1 {
                if b & 0x80 != 0 {
                    return self.mal("integer representation too long");
                }
                let used = bits - 7 * i;
                if !sign {
                    if used < 7 && (b >> used) != 0 {
                        return self.mal("integer too large");
                    }
                } else {
                    let mask = (((0x7f >> (used - 1)) << (used - 1)) as u8) & 0x7f;
                    let v = b & mask;
                    if v != 0 && v != mask {
                        return self.mal("integer too large");
                    }
                }
            }
            r |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
            i += 1;
        }
        if sign && shift < 64 && (b & 0x40) != 0 {
            r |= !0u64 << shift;
        }
        Ok(r)
    }

    fn u32(&mut self) -> R<u32> {
        Ok(self.leb(32, false)? as u32)
    }

    /// Takes n bytes: returns their offset.
    fn take(&mut self, n: usize) -> R<usize> {
        if self.end - self.p < n {
            return self.mal("unexpected end");
        }
        let p = self.p;
        self.p += n;
        Ok(p)
    }

    /// A name, NUL-terminated (it may contain NULs: the real length is one less).
    fn name(&mut self) -> R<Vec<u8>> {
        let n = self.u32()? as usize;
        let off = self.take(n)?;
        if !utf8_ok(&self.bytes()[off..off + n]) {
            return self.mal("malformed UTF-8 encoding");
        }
        let mut v = self.bytes()[off..off + n].to_vec();
        v.push(0);
        Ok(v)
    }

    fn valtype(&mut self) -> R<u8> {
        let t = self.u8()?;
        if !is_num(t) && !is_ref(t) {
            return self.mal(format!("malformed value type 0x{t:02x}"));
        }
        Ok(t)
    }

    fn reftype(&mut self) -> R<u8> {
        let t = self.u8()?;
        if !is_ref(t) {
            return self.mal("malformed reference type");
        }
        Ok(t)
    }

    fn limits(&mut self, cap: u32, what: &str) -> R<(u32, u32)> {
        let f = self.u8()?;
        if f > 1 {
            return self.mal("integer too large"); // shared or 64-bit: not supported
        }
        let min = self.u32()?;
        let max = if f != 0 { self.u32()? } else { WASM_NO_MAX };
        if cap != 0 && min > cap {
            return self.inv(format!("{what} size must be at most {cap} pages (4GiB)"));
        }
        if cap != 0 && f != 0 && max > cap {
            return self.inv(format!("{what} size must be at most {cap} pages (4GiB)"));
        }
        if f != 0 && max < min {
            return self.inv("size minimum must not be greater than maximum");
        }
        Ok((min, max))
    }

    /// Constant expression: validates and stores its position (evaluated at instantiation).
    fn const_expr(&mut self, want: u8) -> R<ConstExpr> {
        let start = self.p;
        let mut stack = [0u8; 8];
        let mut n = 0;
        loop {
            let op = self.u8()?;
            let t;
            match op {
                0x0b => {
                    if n != 1 || (stack[0] != want && stack[0] != UNKNOWN) {
                        return self.inv("type mismatch");
                    }
                    return Ok(ConstExpr { off: start });
                }
                0x41 => {
                    self.leb(32, true)?;
                    t = WASM_I32;
                }
                0x42 => {
                    self.leb(64, true)?;
                    t = WASM_I64;
                }
                0x43 => {
                    self.take(4)?;
                    t = WASM_F32;
                }
                0x44 => {
                    self.take(8)?;
                    t = WASM_F64;
                }
                0xd0 => t = self.reftype()?,
                0xd2 => {
                    let f = self.u32()?;
                    if f as usize >= self.m.functypes.len() {
                        return self.inv(format!("unknown function {f}"));
                    }
                    self.m.declared[f as usize] = 1;
                    t = WASM_FUNCREF;
                }
                0x23 => {
                    let g = self.u32()?;
                    if g >= self.m.nglobal_imports || g as usize >= self.m.globals.len() {
                        return self.inv(format!("unknown global {g}"));
                    }
                    if self.m.globals[g as usize].mutable != 0 {
                        return self.inv("constant expression required");
                    }
                    t = self.m.globals[g as usize].ty;
                }
                _ => {
                    if op <= 0x11 || (0x1a..=0x1c).contains(&op) || (0x20..=0x26).contains(&op) || (0x28..=0xc4).contains(&op) || op == 0xd1 || op == 0xfc {
                        return self.inv("constant expression required");
                    }
                    return self.mal(format!("illegal opcode 0x{op:02x}"));
                }
            }
            if n >= 8 {
                return self.inv("type mismatch");
            }
            stack[n] = t;
            n += 1;
        }
    }

    // ---- function compilation ----

    fn cur(&mut self) -> &mut Ctrl {
        self.ctrls.last_mut().expect("control stack")
    }
    fn emitting(&self) -> bool {
        let c = self.ctrls.last().expect("control stack");
        c.emit && !c.unreachable
    }
    fn emit_raw(&mut self, v: u64) -> usize {
        self.code.push(v);
        self.code.len() - 1
    }
    fn emit_op(&mut self, op: u64) -> usize {
        self.emit_raw(op)
    }
    fn nvals(&self) -> u32 {
        self.vals.len() as u32
    }
    fn push(&mut self, t: u8) {
        self.vals.push(t);
        if self.vals.len() as u32 > self.maxvals {
            self.maxvals = self.vals.len() as u32;
        }
    }
    fn pop(&mut self) -> R<u8> {
        let (h, unreachable) = {
            let c = self.ctrls.last().expect("control stack");
            (c.height, c.unreachable)
        };
        if self.nvals() == h {
            if unreachable {
                return Ok(UNKNOWN);
            }
            return self.inv("type mismatch");
        }
        Ok(self.vals.pop().expect("value stack"))
    }
    fn pop_t(&mut self, want: u8) -> R<u8> {
        let t = self.pop()?;
        if t != want && t != UNKNOWN && want != UNKNOWN {
            return self.inv("type mismatch");
        }
        Ok(t)
    }
    fn pop_list(&mut self, ts: &[u8]) -> R<()> {
        for &t in ts.iter().rev() {
            self.pop_t(t)?;
        }
        Ok(())
    }
    fn push_list(&mut self, ts: &[u8]) {
        for &t in ts {
            self.push(t);
        }
    }
    fn push_ctrl(&mut self, kind: u8, params: &'static [u8], results: &'static [u8]) {
        let emit = if self.ctrls.is_empty() { true } else { self.emitting() };
        let height = self.nvals();
        let loop_pc = self.code.len();
        self.ctrls.push(Ctrl { kind, params, results, height, unreachable: false, emit, loop_pc, else_op: usize::MAX, patches: -1 });
        self.push_list(params);
    }
    fn label_types(c: &Ctrl) -> &'static [u8] {
        if c.kind == 2 { c.params } else { c.results }
    }
    fn set_unreachable(&mut self) {
        let h = self.ctrls.last().expect("control stack").height;
        self.vals.truncate(h as usize);
        self.cur().unreachable = true;
    }
    fn add_patch(&mut self, ci: usize, op: usize, cell: usize) {
        let head = self.ctrls[ci].patches;
        self.patches.push(Patch { op, cell, next: head });
        self.ctrls[ci].patches = self.patches.len() as i32 - 1;
    }
    fn resolve_patches(&mut self, ci: usize, target: usize) {
        let mut i = self.ctrls[ci].patches;
        while i >= 0 {
            let p = &self.patches[i as usize];
            self.code[p.cell] = (target as i64 - p.op as i64) as u64;
            i = p.next;
        }
        self.ctrls[ci].patches = -1;
    }

    /// Jump to label `depth`: the type stack already has the label's values on top.
    fn emit_branch(&mut self, depth: u32, cond: bool) {
        let ci = self.ctrls.len() - 1 - depth as usize;
        let c = &self.ctrls[ci];
        let a = Self::label_types(c).len() as u32;
        let nlocals = self.locals.len() as u32;
        let h = nlocals.wrapping_add(c.height);
        let cur = nlocals.wrapping_add(self.nvals());
        let mv = cur.wrapping_sub(a) != h;
        let kind = c.kind;
        let loop_pc = c.loop_pc;
        let op = self.emit_op(match (cond, mv) {
            (true, true) => OP_BR_IF_N,
            (true, false) => OP_BR_IF,
            (false, true) => OP_BR_N,
            (false, false) => OP_BR,
        });
        let cell = self.emit_raw(0);
        if mv {
            self.emit_raw(h as u64);
            self.emit_raw(a as u64);
        }
        if kind == 2 {
            self.code[cell] = (loop_pc as i64 - op as i64) as u64;
        } else {
            self.add_patch(ci, op, cell);
        }
    }

    fn emit_return(&mut self) {
        let n = unsafe { (*self.ftype).nresults };
        if n == 0 {
            self.emit_op(OP_RETURN0);
        } else if n == 1 {
            self.emit_op(OP_RETURN1);
        } else {
            self.emit_op(OP_RETURN);
            self.emit_raw(n as u64);
        }
    }

    fn blocktype(&mut self) -> R<(&'static [u8], &'static [u8])> {
        let b = if self.p < self.end { self.bytes()[self.p] } else { 0 };
        if b == 0x40 {
            self.p += 1;
            return Ok((&[], &[]));
        }
        if is_num(b) || is_ref(b) {
            self.p += 1;
            return Ok((&[], single(b)));
        }
        let idx = self.leb(33, true)? as i64;
        if idx < 0 {
            return self.mal("malformed block type");
        }
        if idx as u64 >= self.m.types.len() as u64 {
            return self.inv(format!("unknown type {idx}"));
        }
        let t = self.m.types[idx as usize];
        Ok(unsafe { (type_params(t), type_results(t)) })
    }

    fn memarg(&mut self, natural: u32) -> R<u32> {
        if self.m.nmems == 0 {
            return self.inv("unknown memory 0");
        }
        let align = self.u32()?;
        if align >= 32 {
            return self.mal("malformed memop flags");
        }
        let offset = self.u32()?;
        if (1u32 << align) > natural {
            return self.inv("alignment must not be larger than natural");
        }
        Ok(offset)
    }

    fn table_idx(&mut self) -> R<u32> {
        let t = self.u32()?;
        if t as usize >= self.m.tables.len() {
            return self.inv(format!("unknown table {t}"));
        }
        Ok(t)
    }

    fn compile_body(&mut self, fidx: u32) -> R<()> {
        let ft = self.m.functypes[fidx as usize];
        self.ftype = ft;
        let (fparams, fresults) = unsafe { (type_params(ft), type_results(ft)) };
        // locals
        let ngroups = self.u32()?;
        let mut total = fparams.len() as u64;
        let save = self.p;
        for _ in 0..ngroups {
            total += self.u32()? as u64;
            self.valtype()?;
            if total > 50000 {
                return self.mal("too many locals");
            }
        }
        self.p = save;
        self.locals.clear();
        self.locals.extend_from_slice(fparams);
        for _ in 0..ngroups {
            let n = self.u32()?;
            let t = self.valtype()?;
            self.locals.extend(std::iter::repeat(t).take(n as usize));
        }
        let nlocals = self.locals.len() as u32;
        self.vals.clear();
        self.ctrls.clear();
        self.maxvals = 0;
        self.code.clear();
        self.patches.clear();
        self.push_ctrl(0, &[], fresults);

        while !self.ctrls.is_empty() {
            let op = self.u8()?;
            let em = self.emitting();
            match op {
                0x00 => {
                    if em {
                        self.emit_op(0);
                    }
                    self.set_unreachable();
                }
                0x01 => {}
                0x02 | 0x03 => {
                    let (ps, rs) = self.blocktype()?;
                    self.pop_list(ps)?;
                    self.push_ctrl(if op == 0x02 { 1 } else { 2 }, ps, rs);
                }
                0x04 => {
                    let (ps, rs) = self.blocktype()?;
                    self.pop_t(WASM_I32)?;
                    self.pop_list(ps)?;
                    let mut at = usize::MAX;
                    if em {
                        at = self.emit_op(OP_BR_UNLESS);
                        self.emit_raw(0);
                    }
                    self.push_ctrl(3, ps, rs);
                    self.cur().else_op = at;
                }
                0x05 => {
                    let ci = self.ctrls.len() - 1;
                    if self.ctrls[ci].kind != 3 {
                        return self.mal("else without if");
                    }
                    let (results, params, height) = (self.ctrls[ci].results, self.ctrls[ci].params, self.ctrls[ci].height);
                    self.pop_list(results)?;
                    if self.nvals() != height {
                        return self.inv("type mismatch");
                    }
                    if em {
                        let at = self.emit_op(OP_BR);
                        let cell = self.emit_raw(0);
                        self.add_patch(ci, at, cell);
                    }
                    let else_op = self.ctrls[ci].else_op;
                    if else_op != usize::MAX {
                        self.code[else_op + 1] = (self.code.len() as i64 - else_op as i64) as u64;
                    }
                    let c = &mut self.ctrls[ci];
                    c.else_op = usize::MAX;
                    c.kind = 4;
                    c.unreachable = false;
                    self.push_list(params);
                }
                0x0b => {
                    let ci = self.ctrls.len() - 1;
                    let (results, params, height, kind, emit, else_op) = {
                        let c = &self.ctrls[ci];
                        (c.results, c.params, c.height, c.kind, c.emit, c.else_op)
                    };
                    self.pop_list(results)?;
                    if self.nvals() != height {
                        return self.inv("type mismatch");
                    }
                    if kind == 3 {
                        // an if without else: the parameters pass through as-is
                        if params != results {
                            return self.inv("type mismatch");
                        }
                        if else_op != usize::MAX {
                            self.code[else_op + 1] = (self.code.len() as i64 - else_op as i64) as u64;
                        }
                    }
                    let target = self.code.len();
                    self.resolve_patches(ci, target);
                    if kind == 0 && emit {
                        self.emit_return();
                    }
                    self.ctrls.pop();
                    if !self.ctrls.is_empty() {
                        self.push_list(results);
                    }
                }
                0x0c | 0x0d => {
                    let depth = self.u32()?;
                    if depth as usize >= self.ctrls.len() {
                        return self.inv("unknown label");
                    }
                    if op == 0x0d {
                        self.pop_t(WASM_I32)?;
                    }
                    let ts = Self::label_types(&self.ctrls[self.ctrls.len() - 1 - depth as usize]);
                    self.pop_list(ts)?;
                    self.push_list(ts);
                    if em {
                        self.emit_branch(depth, op == 0x0d);
                    }
                    if op == 0x0c {
                        self.set_unreachable();
                    }
                }
                0x0e => {
                    let n = self.u32()?;
                    if n > 100000 {
                        return self.mal("br_table too long");
                    }
                    let mut labels = Vec::with_capacity(n as usize + 1);
                    for _ in 0..=n {
                        labels.push(self.u32()?);
                    }
                    self.pop_t(WASM_I32)?;
                    for &l in &labels {
                        if l as usize >= self.ctrls.len() {
                            return self.inv("unknown label");
                        }
                    }
                    let dts = Self::label_types(&self.ctrls[self.ctrls.len() - 1 - labels[n as usize] as usize]);
                    let arity = dts.len() as u32;
                    for &l in &labels {
                        let ts = Self::label_types(&self.ctrls[self.ctrls.len() - 1 - l as usize]);
                        if ts.len() as u32 != arity {
                            return self.inv("type mismatch");
                        }
                        // each target is validated against the same values
                        let mut tmp = [0u8; 64];
                        let mut got = 0usize;
                        for j in (0..ts.len()).rev() {
                            let t = self.pop_t(ts[j])?;
                            if got < 64 {
                                tmp[got] = t;
                                got += 1;
                            }
                        }
                        for j in (0..got).rev() {
                            self.push(tmp[j]);
                        }
                        if got < ts.len() {
                            return self.inv("br_table with too many values");
                        }
                    }
                    if em {
                        let at = self.emit_op(OP_BR_TABLE);
                        self.emit_raw(n as u64);
                        self.emit_raw(arity as u64);
                        let nlocals = self.locals.len() as u32;
                        for &l in &labels {
                            let ci = self.ctrls.len() - 1 - l as usize;
                            let cell = self.emit_raw(0);
                            let (height, kind, loop_pc) = (self.ctrls[ci].height, self.ctrls[ci].kind, self.ctrls[ci].loop_pc);
                            self.emit_raw((nlocals + height) as u64);
                            if kind == 2 {
                                self.code[cell] = (loop_pc as i64 - at as i64) as u64;
                            } else {
                                self.add_patch(ci, at, cell);
                            }
                        }
                    }
                    self.pop_list(dts)?;
                    self.set_unreachable();
                }
                0x0f => {
                    self.pop_list(fresults)?;
                    if em {
                        self.push_list(fresults);
                        self.emit_return();
                    }
                    self.set_unreachable();
                }
                0x10 => {
                    let f = self.u32()?;
                    if f as usize >= self.m.functypes.len() {
                        return self.inv(format!("unknown function {f}"));
                    }
                    let t = self.m.functypes[f as usize];
                    let (ps, rs) = unsafe { (type_params(t), type_results(t)) };
                    self.pop_list(ps)?;
                    self.push_list(rs);
                    if em {
                        self.emit_op(OP_CALL);
                        self.emit_raw(f as u64);
                    }
                }
                0x11 => {
                    let ti = self.u32()?;
                    let tab = self.u32()?;
                    if tab as usize >= self.m.tables.len() {
                        return self.inv(format!("unknown table {tab}"));
                    }
                    if ti as usize >= self.m.types.len() {
                        return self.inv(format!("unknown type {ti}"));
                    }
                    if self.m.tables[tab as usize].ty != WASM_FUNCREF {
                        return self.inv("type mismatch");
                    }
                    let t = self.m.types[ti as usize];
                    let (ps, rs) = unsafe { (type_params(t), type_results(t)) };
                    self.pop_t(WASM_I32)?;
                    self.pop_list(ps)?;
                    self.push_list(rs);
                    if em {
                        self.emit_op(OP_CALL_INDIRECT);
                        self.emit_raw(tab as u64);
                        self.emit_raw(t as usize as u64);
                    }
                }
                0x1a => {
                    self.pop()?;
                    if em {
                        self.emit_op(0x1a);
                    }
                }
                0x1b | 0x1c => {
                    let mut want = UNKNOWN;
                    if op == 0x1c {
                        if self.u32()? != 1 {
                            return self.inv("invalid result arity");
                        }
                        want = self.valtype()?;
                    }
                    self.pop_t(WASM_I32)?;
                    let b = self.pop_t(want)?;
                    let a = self.pop_t(want)?;
                    if op == 0x1b {
                        if (a != UNKNOWN && !is_num(a)) || (b != UNKNOWN && !is_num(b)) {
                            return self.inv("type mismatch");
                        }
                        if a != b && a != UNKNOWN && b != UNKNOWN {
                            return self.inv("type mismatch");
                        }
                        want = if a != UNKNOWN { a } else { b };
                    }
                    self.push(want);
                    if em {
                        self.emit_op(0x1b);
                    }
                }
                0x20..=0x22 => {
                    let i = self.u32()?;
                    if i >= nlocals {
                        return self.inv(format!("unknown local {i}"));
                    }
                    let t = self.locals[i as usize];
                    if op == 0x20 {
                        self.push(t);
                    } else {
                        self.pop_t(t)?;
                        if op == 0x22 {
                            self.push(t);
                        }
                    }
                    if em {
                        self.emit_op(op as u64);
                        self.emit_raw(i as u64);
                    }
                }
                0x23 | 0x24 => {
                    let g = self.u32()?;
                    if g as usize >= self.m.globals.len() {
                        return self.inv(format!("unknown global {g}"));
                    }
                    let t = self.m.globals[g as usize].ty;
                    if op == 0x23 {
                        self.push(t);
                    } else {
                        if self.m.globals[g as usize].mutable == 0 {
                            return self.inv("global is immutable");
                        }
                        self.pop_t(t)?;
                    }
                    if em {
                        self.emit_op(op as u64);
                        self.emit_raw(g as u64);
                    }
                }
                0x25 | 0x26 => {
                    let t = self.table_idx()?;
                    let et = self.m.tables[t as usize].ty;
                    if op == 0x25 {
                        self.pop_t(WASM_I32)?;
                        self.push(et);
                    } else {
                        self.pop_t(et)?;
                        self.pop_t(WASM_I32)?;
                    }
                    if em {
                        self.emit_op(op as u64);
                        self.emit_raw(t as u64);
                    }
                }
                0x28..=0x35 => {
                    const NAT: [u32; 14] = [4, 8, 4, 8, 1, 1, 2, 2, 1, 1, 2, 2, 4, 4];
                    const RES: [u8; 14] = [WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_I32, WASM_I32, WASM_I32, WASM_I32, WASM_I64, WASM_I64, WASM_I64, WASM_I64, WASM_I64, WASM_I64];
                    let off = self.memarg(NAT[(op - 0x28) as usize])?;
                    self.pop_t(WASM_I32)?;
                    self.push(RES[(op - 0x28) as usize]);
                    if em {
                        self.emit_op(op as u64);
                        self.emit_raw(off as u64);
                    }
                }
                0x36..=0x3e => {
                    const NAT: [u32; 9] = [4, 8, 4, 8, 1, 2, 1, 2, 4];
                    const TY: [u8; 9] = [WASM_I32, WASM_I64, WASM_F32, WASM_F64, WASM_I32, WASM_I32, WASM_I64, WASM_I64, WASM_I64];
                    let off = self.memarg(NAT[(op - 0x36) as usize])?;
                    self.pop_t(TY[(op - 0x36) as usize])?;
                    self.pop_t(WASM_I32)?;
                    if em {
                        self.emit_op(op as u64);
                        self.emit_raw(off as u64);
                    }
                }
                0x3f | 0x40 => {
                    if self.u8()? != 0 {
                        return self.mal("zero byte expected");
                    }
                    if self.m.nmems == 0 {
                        return self.inv("unknown memory 0");
                    }
                    if op == 0x40 {
                        self.pop_t(WASM_I32)?;
                    }
                    self.push(WASM_I32);
                    if em {
                        self.emit_op(op as u64);
                    }
                }
                0x41..=0x44 => {
                    let (v, t);
                    if op == 0x41 {
                        v = self.leb(32, true)? as u32 as u64;
                        t = WASM_I32;
                    } else if op == 0x42 {
                        v = self.leb(64, true)?;
                        t = WASM_I64;
                    } else if op == 0x43 {
                        let o = self.take(4)?;
                        v = u32::from_le_bytes(self.bytes()[o..o + 4].try_into().expect("4 bytes")) as u64;
                        t = WASM_F32;
                    } else {
                        let o = self.take(8)?;
                        v = u64::from_le_bytes(self.bytes()[o..o + 8].try_into().expect("8 bytes"));
                        t = WASM_F64;
                    }
                    self.push(t);
                    if em {
                        self.emit_op(OP_CONST);
                        self.emit_raw(v);
                    }
                }
                0xd0 => {
                    let t = self.reftype()?;
                    self.push(t);
                    if em {
                        self.emit_op(OP_CONST);
                        self.emit_raw(0);
                    }
                }
                0xd1 => {
                    let t = self.pop()?;
                    if t != UNKNOWN && !is_ref(t) {
                        return self.inv("type mismatch");
                    }
                    self.push(WASM_I32);
                    if em {
                        self.emit_op(0xd1);
                    }
                }
                0xd2 => {
                    let f = self.u32()?;
                    if f as usize >= self.m.functypes.len() {
                        return self.inv(format!("unknown function {f}"));
                    }
                    if self.m.declared[f as usize] == 0 {
                        return self.inv("undeclared function reference");
                    }
                    self.push(WASM_FUNCREF);
                    if em {
                        self.emit_op(0xd2);
                        self.emit_raw(f as u64);
                    }
                }
                0xfc => {
                    let sub = self.u32()?;
                    if sub <= 7 {
                        const IN: [u8; 8] = [WASM_F32, WASM_F32, WASM_F64, WASM_F64, WASM_F32, WASM_F32, WASM_F64, WASM_F64];
                        self.pop_t(IN[sub as usize])?;
                        self.push(if sub < 4 { WASM_I32 } else { WASM_I64 });
                        if em {
                            self.emit_op(0x100 + sub as u64);
                        }
                        continue;
                    }
                    match sub {
                        8 => {
                            let seg = self.u32()?;
                            if self.u8()? != 0 {
                                return self.mal("zero byte expected");
                            }
                            if self.m.nmems == 0 {
                                return self.inv("unknown memory 0");
                            }
                            if !self.m.has_datacount {
                                return self.mal("data count section required");
                            }
                            if seg >= self.m.ndata {
                                return self.inv(format!("unknown data segment {seg}"));
                            }
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            if em {
                                self.emit_op(0x108);
                                self.emit_raw(seg as u64);
                            }
                        }
                        9 => {
                            let seg = self.u32()?;
                            if !self.m.has_datacount {
                                return self.mal("data count section required");
                            }
                            if seg >= self.m.ndata {
                                return self.inv(format!("unknown data segment {seg}"));
                            }
                            if em {
                                self.emit_op(0x109);
                                self.emit_raw(seg as u64);
                            }
                        }
                        10 => {
                            if self.u8()? != 0 || self.u8()? != 0 {
                                return self.mal("zero byte expected");
                            }
                            if self.m.nmems == 0 {
                                return self.inv("unknown memory 0");
                            }
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            if em {
                                self.emit_op(0x10a);
                            }
                        }
                        11 => {
                            if self.u8()? != 0 {
                                return self.mal("zero byte expected");
                            }
                            if self.m.nmems == 0 {
                                return self.inv("unknown memory 0");
                            }
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            if em {
                                self.emit_op(0x10b);
                            }
                        }
                        12 => {
                            let seg = self.u32()?;
                            let t = self.table_idx()?;
                            if seg as usize >= self.m.elems.len() {
                                return self.inv(format!("unknown elem segment {seg}"));
                            }
                            if self.m.elems[seg as usize].ty != self.m.tables[t as usize].ty {
                                return self.inv("type mismatch");
                            }
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            if em {
                                self.emit_op(0x10c);
                                self.emit_raw(seg as u64);
                                self.emit_raw(t as u64);
                            }
                        }
                        13 => {
                            let seg = self.u32()?;
                            if seg as usize >= self.m.elems.len() {
                                return self.inv(format!("unknown elem segment {seg}"));
                            }
                            if em {
                                self.emit_op(0x10d);
                                self.emit_raw(seg as u64);
                            }
                        }
                        14 => {
                            let dst = self.table_idx()?;
                            let src = self.table_idx()?;
                            if self.m.tables[dst as usize].ty != self.m.tables[src as usize].ty {
                                return self.inv("type mismatch");
                            }
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            self.pop_t(WASM_I32)?;
                            if em {
                                self.emit_op(0x10e);
                                self.emit_raw(dst as u64);
                                self.emit_raw(src as u64);
                            }
                        }
                        15..=17 => {
                            let t = self.table_idx()?;
                            let et = self.m.tables[t as usize].ty;
                            if sub == 15 {
                                self.pop_t(WASM_I32)?;
                                self.pop_t(et)?;
                                self.push(WASM_I32);
                            } else if sub == 16 {
                                self.push(WASM_I32);
                            } else {
                                self.pop_t(WASM_I32)?;
                                self.pop_t(et)?;
                                self.pop_t(WASM_I32)?;
                            }
                            if em {
                                self.emit_op(0x100 + sub as u64);
                                self.emit_raw(t as u64);
                            }
                        }
                        _ => return self.mal(format!("illegal opcode 0xfc {sub}")),
                    }
                }
                _ => {
                    if (0x45..=0xc4).contains(&op) {
                        let (a, b, r) = numsig(op);
                        if r == 0 {
                            return self.mal(format!("illegal opcode 0x{op:02x}"));
                        }
                        if b != 0 {
                            self.pop_t(b)?;
                        }
                        self.pop_t(a)?;
                        self.push(r);
                        // i32.wrap_i64 and the reinterprets don't change the stored bits
                        let noop = op == 0xa7 || (0xbc..=0xbf).contains(&op);
                        if em && !noop {
                            self.emit_op(op as u64);
                        }
                        continue;
                    }
                    return self.mal(format!("illegal opcode 0x{op:02x}"));
                }
            }
        }
        if self.p != self.end {
            return self.mal("section size mismatch");
        }
        Ok(())
    }

    // ---- module ----

    fn sec_types(&mut self) -> R<()> {
        let n = self.u32()?;
        if n > 1_000_000 {
            return self.mal("too many types");
        }
        for _ in 0..n {
            if self.u8()? != 0x60 {
                return self.mal("integer representation too long");
            }
            let mut tmp = [0u8; 1024];
            let np = self.u32()?;
            if np > 1000 {
                return self.mal("too many parameters");
            }
            for k in 0..np as usize {
                tmp[k] = self.valtype()?;
            }
            let nr = self.u32()?;
            if nr > 1000 || np as usize + nr as usize > tmp.len() {
                return self.mal("too many results");
            }
            for k in 0..nr as usize {
                tmp[np as usize + k] = self.valtype()?;
            }
            let t = functype(&tmp[..np as usize], &tmp[np as usize..np as usize + nr as usize]);
            self.m.types.push(t);
        }
        Ok(())
    }

    fn add_func(&mut self, typeidx: u32) -> R<()> {
        if typeidx as usize >= self.m.types.len() {
            return self.inv(format!("unknown type {typeidx}"));
        }
        self.m.functypes.push(self.m.types[typeidx as usize]);
        Ok(())
    }

    fn add_memory(&mut self, min: u32, max: u32) -> R<()> {
        if self.m.nmems != 0 {
            return self.inv("multiple memories");
        }
        self.m.nmems = 1;
        self.m.mem_min = min;
        self.m.mem_max = max;
        Ok(())
    }

    fn sec_imports(&mut self) -> R<()> {
        let n = self.u32()?;
        if n > 100000 {
            return self.mal("too many imports");
        }
        for _ in 0..n {
            let module = self.name()?;
            let name = self.name()?;
            let kind = self.u8()?;
            let mut ty = WasmExternType::new(kind as c_int);
            match kind {
                0 => {
                    let t = self.u32()?;
                    self.add_func(t)?;
                    ty.func = self.m.types[t as usize];
                    self.m.nfunc_imports += 1;
                }
                1 => {
                    let t = self.reftype()?;
                    let (min, max) = self.limits(0, "table")?;
                    self.m.tables.push(TableDef { ty: t, min, max });
                    (ty.valtype, ty.min, ty.max) = (t, min, max);
                    self.m.ntable_imports += 1;
                }
                2 => {
                    let (min, max) = self.limits(65536, "memory")?;
                    self.add_memory(min, max)?;
                    (ty.min, ty.max) = (min, max);
                    self.m.nmem_imports += 1;
                }
                3 => {
                    let t = self.valtype()?;
                    let mutable = self.u8()?;
                    if mutable > 1 {
                        return self.mal("malformed mutability");
                    }
                    self.m.globals.push(GlobalDef { ty: t, mutable, init: ConstExpr::default() });
                    (ty.valtype, ty.r#mut) = (t, mutable as c_int);
                    self.m.nglobal_imports += 1;
                }
                _ => return self.mal("malformed import kind"),
            }
            self.m.imports.push(Import { module, name, ty });
        }
        Ok(())
    }

    fn sec_functions(&mut self) -> R<()> {
        let n = self.u32()?;
        if n > 1_000_000 {
            return self.mal("too many functions");
        }
        for _ in 0..n {
            let t = self.u32()?;
            self.add_func(t)?;
        }
        Ok(())
    }

    fn sec_tables(&mut self) -> R<()> {
        let n = self.u32()?;
        for _ in 0..n {
            let t = self.reftype()?;
            let (min, max) = self.limits(0, "table")?;
            self.m.tables.push(TableDef { ty: t, min, max });
        }
        Ok(())
    }

    fn sec_memories(&mut self) -> R<()> {
        let n = self.u32()?;
        for _ in 0..n {
            let (min, max) = self.limits(65536, "memory")?;
            self.add_memory(min, max)?;
        }
        Ok(())
    }

    fn sec_globals(&mut self) -> R<()> {
        let n = self.u32()?;
        for _ in 0..n {
            let t = self.valtype()?;
            let mutable = self.u8()?;
            if mutable > 1 {
                return self.mal("malformed mutability");
            }
            let init = self.const_expr(t)?;
            self.m.globals.push(GlobalDef { ty: t, mutable, init });
        }
        Ok(())
    }

    fn sec_exports(&mut self) -> R<()> {
        let n = self.u32()?;
        if n > 100000 {
            return self.mal("too many exports");
        }
        for i in 0..n as usize {
            let name = self.name()?;
            let kind = self.u8()? as c_int;
            let idx = self.u32()?;
            match kind {
                0 => {
                    if idx as usize >= self.m.functypes.len() {
                        return self.inv(format!("unknown function {idx}"));
                    }
                    self.m.declared[idx as usize] = 1;
                }
                1 => {
                    if idx as usize >= self.m.tables.len() {
                        return self.inv(format!("unknown table {idx}"));
                    }
                }
                2 => {
                    if idx >= self.m.nmems {
                        return self.inv(format!("unknown memory {idx}"));
                    }
                }
                3 => {
                    if idx as usize >= self.m.globals.len() {
                        return self.inv(format!("unknown global {idx}"));
                    }
                }
                _ => return self.mal("malformed export kind"),
            }
            self.m.exports.push(Export { name, kind, idx });
            for k in 0..i {
                if self.m.exports[k].name == self.m.exports[i].name {
                    return self.inv("duplicate export name");
                }
            }
        }
        Ok(())
    }

    fn sec_start(&mut self) -> R<()> {
        let f = self.u32()?;
        if f as usize >= self.m.functypes.len() {
            return self.inv(format!("unknown function {f}"));
        }
        let t = self.m.functypes[f as usize];
        if unsafe { (*t).nparams != 0 || (*t).nresults != 0 } {
            return self.inv("start function");
        }
        self.m.start = f as i64;
        Ok(())
    }

    fn sec_elems(&mut self) -> R<()> {
        let n = self.u32()?;
        if n > 100000 {
            return self.mal("too many segments");
        }
        for _ in 0..n {
            let flags = self.u32()?;
            if flags > 7 {
                return self.mal("malformed elements segment kind");
            }
            let mode = if flags & 1 != 0 { if flags & 2 != 0 { 2 } else { 1 } } else { 0 };
            let mut s = ElemSeg { mode, ty: WASM_FUNCREF, table: 0, offset: ConstExpr::default(), items: Vec::new() };
            if flags & 3 == 2 {
                s.table = self.u32()?;
            }
            if mode == 0 {
                if s.table as usize >= self.m.tables.len() {
                    return self.inv(format!("unknown table {}", s.table));
                }
                s.offset = self.const_expr(WASM_I32)?;
            }
            let exprs = flags & 4 != 0;
            if flags & 3 != 0 {
                if exprs {
                    s.ty = self.reftype()?;
                } else if self.u8()? != 0x00 {
                    return self.mal("malformed element kind");
                }
            }
            let cnt = self.u32()?;
            if cnt > 10_000_000 {
                return self.mal("too many elements");
            }
            for _ in 0..cnt {
                if exprs {
                    let expr = self.const_expr(s.ty)?;
                    s.items.push(ElemItem { is_expr: true, func: 0, expr });
                } else {
                    let f = self.u32()?;
                    if f as usize >= self.m.functypes.len() {
                        return self.inv(format!("unknown function {f}"));
                    }
                    self.m.declared[f as usize] = 1;
                    s.items.push(ElemItem { is_expr: false, func: f, expr: ConstExpr::default() });
                }
            }
            if mode == 0 && self.m.tables[s.table as usize].ty != s.ty {
                return self.inv("type mismatch");
            }
            self.m.elems.push(s);
        }
        Ok(())
    }

    fn sec_datacount(&mut self) -> R<()> {
        self.m.ndata = self.u32()?;
        self.m.has_datacount = true;
        Ok(())
    }

    fn sec_code(&mut self) -> R<()> {
        let n = self.u32()?;
        let ndef = (self.m.functypes.len() as u32).wrapping_sub(self.m.nfunc_imports);
        if n != ndef {
            return self.mal("function and code section have inconsistent lengths");
        }
        let outer_end = self.end;
        for i in 0..n {
            let size = self.u32()?;
            if outer_end - self.p < size as usize {
                return self.mal("unexpected end");
            }
            self.end = self.p + size as usize;
            self.compile_body(self.m.nfunc_imports + i)?;
            self.end = outer_end;
            let nlocals = self.locals.len() as u32;
            self.m.code.push(WasmCode { nlocals, max_stack: nlocals + self.maxvals + 4, code: self.code.clone() });
        }
        Ok(())
    }

    fn sec_data(&mut self) -> R<()> {
        let n = self.u32()?;
        if self.m.has_datacount && n != self.m.ndata {
            return self.mal("data count and data section have inconsistent lengths");
        }
        if n > 1_000_000 {
            return self.mal("too many segments");
        }
        self.m.ndata = n;
        for _ in 0..n {
            let flags = self.u32()?;
            if flags > 2 {
                return self.mal("malformed data segment kind");
            }
            let mut s = DataSeg { active: false, offset: ConstExpr::default(), off: 0, len: 0 };
            if flags != 1 {
                if flags == 2 && self.u32()? != 0 {
                    return self.inv("unknown memory");
                }
                if self.m.nmems == 0 {
                    return self.inv("unknown memory 0");
                }
                s.active = true;
                s.offset = self.const_expr(WASM_I32)?;
            }
            s.len = self.u32()?;
            s.off = self.take(s.len as usize)?;
            self.m.data.push(s);
        }
        Ok(())
    }

    fn sec_custom(&mut self) -> R<()> {
        let name = self.name()?;
        self.m.custom.push(Custom { name, off: self.p, len: self.end - self.p });
        self.p = self.end;
        Ok(())
    }

    fn decode(&mut self) -> R<()> {
        let len = self.m.bytes.len();
        if len < 4 || &self.bytes()[..4] != b"\0asm" {
            return self.mal("magic header not detected");
        }
        self.p += 4;
        if len < 8 || &self.bytes()[4..8] != b"\x01\0\0\0" {
            return self.mal("unknown binary version");
        }
        self.p += 4;
        let mut last = 0;
        const ORDER: [i32; 13] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 10]; // datacount (12) goes before code
        let file_end = self.end;
        let mut has_code = false;
        let mut have_declared = false;
        while self.p < file_end {
            let id = self.u8()?;
            let size = self.u32()?;
            if file_end - self.p < size as usize {
                return self.mal("length out of bounds");
            }
            self.end = self.p + size as usize;
            if id > 12 {
                return self.mal("malformed section id");
            }
            if id != 0 {
                if ORDER[id as usize] <= last {
                    return self.mal("unexpected content after last section");
                }
                last = ORDER[id as usize];
            }
            // functions ref.func may name: the function list is already complete
            if id >= 4 && !have_declared {
                self.m.declared = vec![0; self.m.functypes.len() + 1];
                have_declared = true;
            }
            match id {
                0 => self.sec_custom()?,
                1 => self.sec_types()?,
                2 => self.sec_imports()?,
                3 => self.sec_functions()?,
                4 => self.sec_tables()?,
                5 => self.sec_memories()?,
                6 => self.sec_globals()?,
                7 => self.sec_exports()?,
                8 => self.sec_start()?,
                9 => self.sec_elems()?,
                10 => {
                    self.sec_code()?;
                    has_code = true;
                }
                11 => self.sec_data()?,
                _ => self.sec_datacount()?,
            }
            if self.p != self.end {
                return self.mal("section size mismatch");
            }
            self.end = file_end;
        }
        if !has_code && self.m.functypes.len() as u32 != self.m.nfunc_imports {
            return self.mal("function and code section have inconsistent lengths");
        }
        if self.m.has_datacount && self.m.data.is_empty() && self.m.ndata != 0 {
            return self.mal("data count and data section have inconsistent lengths");
        }
        Ok(())
    }
}

/// Decodes, validates and compiles. NULL (with the error set) on failure.
pub(crate) unsafe fn decode(input: &[u8], err: *mut WasmError) -> *mut WasmModule {
    if !err.is_null() {
        (*err).kind = WASM_OK;
    }
    let m = WasmModule {
        rc: 1,
        bytes: input.to_vec(),
        types: Vec::new(),
        imports: Vec::new(),
        nfunc_imports: 0,
        ntable_imports: 0,
        nmem_imports: 0,
        nglobal_imports: 0,
        functypes: Vec::new(),
        code: Vec::new(),
        tables: Vec::new(),
        nmems: 0,
        mem_min: 0,
        mem_max: 0,
        globals: Vec::new(),
        exports: Vec::new(),
        start: -1,
        elems: Vec::new(),
        ndata: 0,
        has_datacount: false,
        data: Vec::new(),
        declared: Vec::new(),
        custom: Vec::new(),
    };
    let len = m.bytes.len();
    let base = m.bytes.as_ptr();
    let mut d = Dec {
        err,
        m,
        base,
        p: 0,
        end: len,
        vals: Vec::new(),
        maxvals: 0,
        ctrls: Vec::new(),
        code: Vec::new(),
        patches: Vec::new(),
        locals: Vec::new(),
        ftype: std::ptr::null(),
    };
    exec_init();
    match d.decode() {
        Ok(()) => Box::into_raw(Box::new(d.m)),
        Err(()) => null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(bytes: *const u8, len: usize, err: *mut WasmError) -> *mut WasmModule {
    decode(if len == 0 { &[] } else { std::slice::from_raw_parts(bytes, len) }, err)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_validate(bytes: *const u8, len: usize, err: *mut WasmError) -> c_int {
    let m = decode(if len == 0 { &[] } else { std::slice::from_raw_parts(bytes, len) }, err);
    if m.is_null() {
        return -1;
    }
    drop(Box::from_raw(m));
    0
}
