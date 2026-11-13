//! Thai word breaking, like libthai (brk-maximal.c) but without linking it: reads
//! the libthai-data dictionary (/usr/share/libthai/thbrk.tri, a libdatrie
//! double-array trie) and looks for the segmentation by "maximal matching": among
//! the possible paths of dictionary words, the one that reaches farthest with the
//! fewest characters outside words and the fewest breaks. Without the dictionary it
//! breaks where a syllable can start (the same hints libthai uses).
//!
//! The file comes from the system, but everything is still validated: sizes,
//! signatures and indices. C ABI of server/src/ui/thbrk.h.
use std::sync::OnceLock;

const DICT_PATH: &str = "/usr/share/libthai/thbrk.tri";
const MAX_FILE: usize = 32 << 20;

struct Dict {
    alpha_begin: u32,
    alpha_end: u32,
    alpha_to_trie: Vec<i32>, // -1 = not in the alphabet
    cells: Vec<(i32, i32)>,  // (base, check)
    tails: Vec<Vec<u8>>,     // each suffix ends in 0
}

fn rd32(f: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([f[p], f[p + 1], f[p + 2], f[p + 3]])
}

fn parse(f: &[u8]) -> Option<Dict> {
    let n = f.len();
    let mut p = 0usize;
    let need = |p: usize, k: usize| n - p >= k;
    // alphabet: character ranges → trie characters (1, 2, …; 0 is end of word)
    if !need(p, 8) || rd32(f, 0) != 0xD9FC_D9FC {
        return None;
    }
    let nranges = rd32(f, 4) as usize;
    p = 8;
    if nranges == 0 || nranges > 1024 || !need(p, nranges * 8) {
        return None;
    }
    let (mut lo, mut hi) = (u32::MAX, 0u32);
    for i in 0..nranges {
        let (b, e) = (rd32(f, p + 8 * i), rd32(f, p + 8 * i + 4));
        if b > e || e > 0x10FFFF {
            return None;
        }
        lo = lo.min(b);
        hi = hi.max(e);
    }
    if hi - lo > 0xFFFF {
        return None;
    }
    let mut alpha_to_trie = vec![-1i32; (hi - lo + 1) as usize];
    let mut tc = 0i32;
    for i in 0..nranges {
        // in file order, like libdatrie
        let (b, e) = (rd32(f, p + 8 * i), rd32(f, p + 8 * i + 4));
        for a in b..=e {
            if tc == 0 {
                tc += 1;
            }
            let slot = &mut alpha_to_trie[(a - lo) as usize];
            if *slot < 0 {
                *slot = tc;
                tc += 1;
            }
        }
    }
    p += nranges * 8;
    // double array: cell 0 = signature and number of cells; the root is cell 2
    if !need(p, 8) || rd32(f, p) != 0xDAFC_DAFC {
        return None;
    }
    let ncells = rd32(f, p + 4) as i32;
    p += 8;
    if ncells < 3 || ncells > (MAX_FILE / 8) as i32 || !need(p, (ncells as usize - 1) * 8) {
        return None;
    }
    let mut cells = Vec::with_capacity(ncells as usize);
    cells.push((0, 0));
    for _ in 1..ncells {
        cells.push((rd32(f, p) as i32, rd32(f, p + 4) as i32));
        p += 8;
    }
    // tails: the rest of each word once it no longer branches
    if !need(p, 12) || rd32(f, p) != 0xDFFC_DFFC {
        return None;
    }
    let ntails = rd32(f, p + 8) as i32;
    p += 12;
    if ntails < 0 || ntails > (MAX_FILE / 10) as i32 {
        return None;
    }
    let mut tails = Vec::with_capacity(ntails as usize);
    for _ in 0..ntails {
        if !need(p, 10) {
            return None;
        }
        let len = i16::from_be_bytes([f[p + 8], f[p + 9]]);
        p += 10;
        if len < 0 || !need(p, len as usize) {
            return None;
        }
        let mut s = f[p..p + len as usize].to_vec();
        s.push(0);
        tails.push(s);
        p += len as usize;
    }
    Some(Dict { alpha_begin: lo, alpha_end: hi, alpha_to_trie, cells, tails })
}

fn load() -> Option<Dict> {
    let meta = std::fs::metadata(DICT_PATH).ok()?;
    if meta.len() == 0 || meta.len() > MAX_FILE as u64 {
        return None;
    }
    let f = std::fs::read(DICT_PATH).ok()?;
    if f.is_empty() || f.len() > MAX_FILE {
        return None;
    }
    parse(&f)
}

fn dict() -> Option<&'static Dict> {
    static D: OnceLock<Option<Dict>> = OnceLock::new();
    D.get_or_init(load).as_ref()
}

// ---- trie traversal (libdatrie's trie_state_*) ----

#[derive(Clone, Copy)]
struct TState {
    index: i32,
    suffix_idx: usize,
    is_suffix: bool,
}

const ROOT: TState = TState { index: 2, suffix_idx: 0, is_suffix: false };

impl Dict {
    fn trie_char(&self, c: u32) -> i32 {
        if c == 0 {
            return 0;
        }
        if c >= self.alpha_begin && c <= self.alpha_end {
            self.alpha_to_trie[(c - self.alpha_begin) as usize]
        } else {
            -1
        }
    }
    fn base(&self, s: i32) -> i32 {
        if s >= 0 && (s as usize) < self.cells.len() { self.cells[s as usize].0 } else { 0 }
    }
    fn check(&self, s: i32) -> i32 {
        if s >= 0 && (s as usize) < self.cells.len() { self.cells[s as usize].1 } else { -1 }
    }
    fn suffix(&self, t: i32) -> Option<&[u8]> {
        let t = t.wrapping_sub(1);
        if t >= 0 && (t as usize) < self.tails.len() { Some(&self.tails[t as usize]) } else { None }
    }
    fn walk(&self, s: &mut TState, c: u32) -> bool {
        let tc = self.trie_char(c);
        if tc < 0 {
            return false;
        }
        if !s.is_suffix {
            let next = self.base(s.index).wrapping_add(tc);
            if self.check(next) != s.index {
                return false;
            }
            s.index = next;
            let b = self.base(next);
            if b < 0 {
                *s = TState { index: b.wrapping_neg(), suffix_idx: 0, is_suffix: true };
            }
            return true;
        }
        match self.suffix(s.index) {
            Some(suf) if suf[s.suffix_idx] as i32 == tc => {
                if tc != 0 {
                    s.suffix_idx += 1;
                }
                true
            }
            _ => false,
        }
    }
    fn terminal(&self, s: &TState) -> bool {
        if !s.is_suffix {
            return self.check(self.base(s.index)) == s.index;
        }
        self.suffix(s.index).is_some_and(|suf| suf[s.suffix_idx] == 0)
    }
}

// ---- hints: where a syllable can start (libthai's brk_brkpos_hints) ----

const KO_KAI: u32 = 0x0E01;
const RU: u32 = 0x0E24;
const LU: u32 = 0x0E26;
const WO_WAEN: u32 = 0x0E27;
const O_ANG: u32 = 0x0E2D;
const SARA_A: u32 = 0x0E30;
const MAI_HAN_AKAT: u32 = 0x0E31;
const SARA_AA: u32 = 0x0E32;
const SARA_UEE: u32 = 0x0E37;
const SARA_E: u32 = 0x0E40;
const SARA_AE: u32 = 0x0E41;
const MAITAIKHU: u32 = 0x0E47;
const THANTHAKHAT: u32 = 0x0E4C;

fn is_cons(c: u32) -> bool {
    (0x0E01..=0x0E2E).contains(&c) && c != RU && c != LU
}
fn is_tone(c: u32) -> bool {
    (0x0E48..=0x0E4B).contains(&c)
}
fn is_upvowel(c: u32) -> bool {
    c == 0x0E31 || (0x0E34..=0x0E37).contains(&c)
}
fn is_ldvowel(c: u32) -> bool {
    (0x0E40..=0x0E44).contains(&c)
}

fn hints(s: &[u32]) -> Vec<bool> {
    let len = s.len();
    let mut h = vec![false; len + 1];
    let mut i = 0usize;
    while i < len {
        if is_cons(s[i]) {
            if i + 1 < len && s[i + 1] == THANTHAKHAT {
                i += 2;
            } else if i + 2 < len && s[i + 2] == THANTHAKHAT {
                i += 3;
            } else if i + 2 < len && s[i] != KO_KAI && s[i + 1] == MAITAIKHU && (s[i + 2] == O_ANG || s[i + 2] == WO_WAEN) {
                h[i] = true;
                i += 4;
            } else if (i > 0 && (s[i - 1] == MAI_HAN_AKAT || s[i - 1] == SARA_UEE))
                || (i > 1 && is_tone(s[i - 1]) && (s[i - 2] == MAI_HAN_AKAT || s[i - 2] == SARA_UEE))
            {
                i += 1;
            } else {
                h[i] = true;
                i += 1;
            }
        } else if s[i] == SARA_E || s[i] == SARA_AE {
            h[i] = true;
            i += 2;
            if i >= len {
                break;
            }
            if s[i] == MAITAIKHU {
                i += 2;
            } else if is_upvowel(s[i]) {
                i += 1;
                if i < len && is_tone(s[i]) {
                    i += 1;
                }
                i += 1;
            } else if i + 2 < len
                && ((s[i + 1] == SARA_AA && s[i + 2] == SARA_A)
                    || (s[i] != KO_KAI && s[i + 1] == MAITAIKHU && s[i + 2] != O_ANG && s[i + 2] != WO_WAEN))
            {
                i += 3;
            }
        } else if is_ldvowel(s[i]) {
            h[i] = true;
            i += 2;
        } else if s[i] == RU || s[i] == LU {
            h[i] = true;
            i += 1;
        } else {
            i += 1;
        }
    }
    h
}

// ---- maximal matching (libthai's brk_maximal_do_impl) ----
//
// The pool is a singly linked list kept in an arena, in the same order as the C
// list: which path is taken next and which ones get merged depend on that order.

const NIL: usize = usize::MAX;

struct Shot {
    next: usize,
    st: TState,
    str_pos: usize,
    penalty: i64,
    brk: Vec<usize>,
}

struct Pool {
    shots: Vec<Shot>,
    free: Vec<usize>,
    head: usize,
}

impl Pool {
    fn new() -> Self {
        let mut p = Pool { shots: Vec::new(), free: Vec::new(), head: NIL };
        p.head = p.alloc(ROOT, 0, 0, Vec::new());
        p
    }
    fn alloc(&mut self, st: TState, str_pos: usize, penalty: i64, brk: Vec<usize>) -> usize {
        let s = Shot { next: NIL, st, str_pos, penalty, brk };
        if let Some(i) = self.free.pop() {
            self.shots[i] = s;
            i
        } else {
            self.shots.push(s);
            self.shots.len() - 1
        }
    }
    /// A copy of `src` pushed at the head of the pool.
    fn fork(&mut self, src: usize) -> usize {
        let s = &self.shots[src];
        let (st, pos, pen, brk) = (s.st, s.str_pos, s.penalty, s.brk.clone());
        let n = self.alloc(st, pos, pen, brk);
        self.shots[n].next = self.head;
        self.head = n;
        n
    }
    fn delete(&mut self, node: usize) {
        if self.head == node {
            self.head = self.shots[node].next;
        } else {
            let mut p = self.head;
            while p != NIL && self.shots[p].next != node {
                p = self.shots[p].next;
            }
            if p != NIL {
                self.shots[p].next = self.shots[node].next;
            }
        }
        self.shots[node].brk = Vec::new();
        self.free.push(node);
    }
    /// The one that advanced least.
    fn get(&self) -> usize {
        let (mut chosen, mut min) = (NIL, usize::MAX);
        let mut p = self.head;
        while p != NIL {
            let s = &self.shots[p];
            match s.brk.last() {
                None => return p,
                Some(&b) if b < min => {
                    min = b;
                    chosen = p;
                }
                _ => {}
            }
            p = s.next;
        }
        chosen
    }
    /// Another one, from `from` on, that broke at the same place as `node`.
    fn matching(&self, mut from: usize, node: usize) -> usize {
        let last = self.shots[node].brk.last();
        while from != NIL {
            if from != node && self.shots[from].brk.last() == last {
                return from;
            }
            from = self.shots[from].next;
        }
        NIL
    }
}

const RECOVERED_WORDS: usize = 3;

fn recover_try(d: &Dict, ws: &[u32], h: &[bool], words: usize, last: &mut usize) -> usize {
    let len = ws.len();
    let mut pool = Pool::new();
    let mut ret = 0;
    loop {
        let mut node = pool.get();
        if node == NIL {
            break;
        }
        loop {
            let mut keep = true;
            loop {
                let s = &mut pool.shots[node];
                let c = ws[s.str_pos];
                s.str_pos += 1;
                if !d.walk(&mut s.st, c) {
                    keep = false;
                    break;
                }
                let term = d.terminal(&s.st);
                if s.str_pos >= len {
                    if !term {
                        keep = false;
                    }
                    break;
                }
                if term && h[s.str_pos] {
                    break;
                }
            }
            if !keep {
                pool.delete(node);
                break;
            }
            if pool.shots[node].str_pos < len && !pool.shots[node].st.is_suffix {
                node = pool.fork(node);
            }
            let s = &mut pool.shots[node];
            s.st = ROOT;
            s.brk.push(s.str_pos);
            if s.str_pos == len || s.brk.len() == words {
                if s.brk.len() > ret {
                    ret = s.brk.len();
                    *last = s.brk[ret - 1];
                }
                pool.delete(node);
                if ret == words {
                    return ret;
                }
                break;
            }
            let mut tail = pool.head;
            loop {
                let m = pool.matching(tail, node);
                if m == NIL {
                    break;
                }
                let nx = pool.shots[m].next;
                pool.delete(m);
                tail = nx;
            }
        }
    }
    ret
}

struct Hist {
    pos: Option<usize>,
    recov: Option<usize>,
}

/// From where words can be recognized again (RECOVERED_WORDS in a row) after a run
/// that is not in the dictionary.
fn recover(d: &Dict, ws: &[u32], mut pos: usize, h: &[bool], rh: &mut Hist) -> Option<usize> {
    let len = ws.len();
    while pos < len && !h[pos] {
        pos += 1;
    }
    if rh.pos == Some(pos) {
        return rh.recov;
    }
    for p in pos..len {
        if h[p] {
            let mut last = 0;
            let n = recover_try(d, &ws[p..], &h[p..], RECOVERED_WORDS, &mut last);
            // libthai compares ws[last] (last is relative to p) with the end of the text: we keep that
            if n == RECOVERED_WORDS || (n > 0 && last >= len) {
                rh.pos = Some(pos);
                rh.recov = Some(p);
                return Some(p);
            }
        }
    }
    None
}

/// Breaks inside a Thai run: positions 1..len-1.
pub fn chunk(ws: &[u32]) -> Vec<usize> {
    let len = ws.len();
    if len <= 1 {
        return Vec::new();
    }
    let h = hints(ws);
    let Some(d) = dict() else {
        return (1..len).filter(|&i| h[i]).collect();
    };
    let mut pool = Pool::new();
    let (mut best, mut best_pos, mut best_pen): (Vec<usize>, usize, i64) = (Vec::new(), 0, 0);
    let mut rh = Hist { pos: None, recov: None };
    loop {
        let mut node = pool.get();
        if node == NIL {
            break;
        }
        let (mut keep, mut term, mut recovered) = (true, false, false);
        let mut sp = pool.shots[node].str_pos;
        loop {
            let s = &mut pool.shots[node];
            let c = ws[sp];
            sp += 1;
            if !d.walk(&mut s.st, c) {
                term = false;
                let base = s.brk.last().map_or(0, |&b| b as i64);
                match recover(d, ws, s.str_pos + 1, &h, &mut rh) {
                    Some(r) => {
                        s.penalty += r as i64 - base;
                        sp = r;
                        recovered = true;
                    }
                    None => {
                        s.penalty += len as i64 - base;
                        sp = len;
                        s.brk.push(len);
                        keep = false;
                    }
                }
                break;
            }
            term = d.terminal(&s.st);
            if sp >= len {
                if !term {
                    let base = s.brk.last().map_or(0, |&b| b as i64);
                    s.penalty += len as i64 - base;
                    s.brk.push(len);
                    keep = false;
                }
                break;
            }
            if term && h[sp] {
                break;
            }
        }
        pool.shots[node].str_pos = sp;
        if keep && (term || recovered) {
            if sp < len && term && !pool.shots[node].st.is_suffix {
                node = pool.fork(node); // also the path that follows the longest word
            }
            let s = &mut pool.shots[node];
            s.st = ROOT;
            s.brk.push(s.str_pos);
        }
        let s = &pool.shots[node];
        if !keep || s.str_pos == len || s.brk.len() >= len + 1 {
            if s.str_pos > best_pos
                || (s.str_pos == best_pos
                    && (s.penalty < best_pen || (s.penalty == best_pen && s.brk.len() <= best.len())))
            {
                best = s.brk.clone();
                best_pos = s.str_pos;
                best_pen = s.penalty;
            }
            pool.delete(node);
        } else {
            let mut tail = pool.head;
            loop {
                let m = pool.matching(tail, node);
                if m == NIL {
                    break;
                }
                let mut nx = pool.shots[m].next;
                let (ms, ns) = (&pool.shots[m], &pool.shots[node]);
                let del = if ms.penalty < ns.penalty || (ms.penalty == ns.penalty && ms.brk.len() < ns.brk.len()) {
                    if nx == node {
                        nx = ns.next;
                    }
                    let old = node;
                    node = m;
                    old
                } else {
                    m
                };
                pool.delete(del);
                tail = nx;
            }
        }
    }
    best.into_iter().filter(|&b| b > 0 && b < len).collect()
}

// libthai's break classes (brk-ctype.c) for the Thai block: only letters and marks
// (THAI) go to the dictionary; ๆ and ฯ are not separated from what precedes them;
// digits and ฿ follow the number rules.
const C_THAI: usize = 0;
const C_ALPHA: usize = 1;
const C_NUM: usize = 2;
const C_NBB: usize = 3;
const C_NUM_CUR: usize = 4;
const C_TERM: usize = 5;
const C_OTHER: usize = 6;

fn th_class(c: u32) -> usize {
    match c {
        0x0E2F | 0x0E46 => C_NBB,
        0x0E3F => C_NUM_CUR,
        0x0E4F => C_ALPHA,
        0x0E50..=0x0E59 => C_NUM,
        0x0E5A | 0x0E5B => C_TERM,
        0x0E01..=0x0E3A | 0x0E40..=0x0E4E => C_THAI,
        _ => C_OTHER,
    }
}

pub fn is_thai(c: u32) -> bool {
    th_class(c) == C_THAI
}

/// 1 can break, 0 cannot, -1 one of them is not in the block.
pub fn pair(a: u32, b: u32) -> i32 {
    let (x, y) = (th_class(a), th_class(b));
    if x == C_OTHER || y == C_OTHER {
        return -1;
    }
    // libthai's _break_table, only these rows and columns (indirect: without a space, no)
    const T: [[u8; 6]; 6] = [
        // THAI ALPHA NUM NBB NUM_CUR TERM
        [0, 1, 0, 0, 1, 0], // THAI
        [1, 0, 0, 0, 0, 0], // ALPHA
        [1, 0, 0, 0, 0, 0], // NUM
        [1, 1, 1, 0, 1, 0], // NBB
        [1, 0, 0, 0, 0, 0], // NUM_CUR
        [1, 1, 1, 0, 1, 0], // TERM
    ];
    T[x][y] as i32
}

const MAX_ACRONYM_FRAG: usize = 3;

/// For each place Thai decides, bk[i] = 1 (may break before s[i]) or 2 (may not).
pub fn text(s: &[u32], bk: &mut [u8]) {
    let n = s.len();
    // between letters and ๆ, digits or ฿: libthai's table
    for i in 1..n {
        let pr = pair(s[i - 1], s[i]);
        if pr >= 0 && !(is_thai(s[i - 1]) && is_thai(s[i])) {
            bk[i] = if pr != 0 { 1 } else { 2 };
        }
    }
    let mut i = 0;
    while i < n {
        if !is_thai(s[i]) {
            i += 1;
            continue;
        }
        // abbreviations like libthai: up to 3 letters followed by a period ("พ.ศ.", "กอ.รมน.") are not broken
        let (chunk_start, mut ae, mut p) = (i, i, i);
        while p < n {
            if is_thai(s[p]) {
                if ae > chunk_start && p - ae > MAX_ACRONYM_FRAG {
                    break;
                }
                p += 1;
            } else if s[p] == '.' as u32 && p - ae <= MAX_ACRONYM_FRAG {
                p += 1;
                ae = p;
            } else {
                break;
            }
        }
        if ae > chunk_start {
            for b in &mut bk[chunk_start + 1..ae] {
                *b = 2;
            }
            if ae < n && is_thai(s[ae]) {
                bk[ae] = 1; // after the period it can break
            }
            i = ae;
            continue;
        }
        for b in &mut bk[chunk_start + 1..p] {
            *b = 2;
        }
        for k in chunk(&s[chunk_start..p]) {
            bk[chunk_start + k] = 1;
        }
        i = p;
    }
}

#[no_mangle]
pub extern "C" fn thbrk_available() -> i32 {
    dict().is_some() as i32
}

#[no_mangle]
pub extern "C" fn thbrk_is_thai(c: u32) -> i32 {
    is_thai(c) as i32
}

#[no_mangle]
pub extern "C" fn thbrk_pair(a: u32, b: u32) -> i32 {
    pair(a, b)
}

/// # Safety
/// `ws` covers `len` code points and `out` has room for `max` ints.
#[no_mangle]
pub unsafe extern "C" fn thbrk_chunk(ws: *const u32, len: i32, out: *mut i32, max: i32) -> i32 {
    if len <= 1 || max <= 0 || ws.is_null() || out.is_null() {
        return 0;
    }
    let ws = std::slice::from_raw_parts(ws, len as usize);
    let out = std::slice::from_raw_parts_mut(out, max as usize);
    let mut k = 0;
    for b in chunk(ws) {
        if k == out.len() {
            break;
        }
        out[k] = b as i32;
        k += 1;
    }
    k as i32
}

/// # Safety
/// `s` covers `n` code points and `bk` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn thbrk_text(s: *const u32, n: i32, bk: *mut u8) {
    if n <= 0 || s.is_null() || bk.is_null() {
        return;
    }
    text(std::slice::from_raw_parts(s, n as usize), std::slice::from_raw_parts_mut(bk, n as usize));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cps(s: &str) -> Vec<u32> {
        s.chars().map(|c| c as u32).collect()
    }

    #[test]
    fn classes_and_pairs() {
        assert!(is_thai(0x0E01));
        assert!(!is_thai(0x0E46));
        assert_eq!(pair('a' as u32, 0x0E01), -1);
        assert_eq!(pair(0x0E01, 0x0E46), 0); // ๆ stays with what precedes it
        assert_eq!(pair(0x0E46, 0x0E01), 1);
    }

    #[test]
    fn abbreviation_is_not_broken() {
        let s = cps("พ.ศ.");
        let mut bk = vec![0u8; s.len()];
        text(&s, &mut bk);
        assert_eq!(bk, [0, 2, 2, 2]);
    }

    #[test]
    fn breaks_are_inside_the_run() {
        let s = cps("สวัสดีครับวันนี้อากาศดีมาก");
        let b = chunk(&s);
        assert!(!b.is_empty());
        assert!(b.windows(2).all(|w| w[0] < w[1]));
        assert!(b.iter().all(|&x| x > 0 && x < s.len()));
    }

    #[test]
    fn corrupt_dictionaries_are_rejected() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0xD9, 0xFC, 0xD9, 0xFC, 0, 0, 0, 1]).is_none());
        let mut f = vec![0xD9, 0xFC, 0xD9, 0xFC, 0, 0, 0, 1, 0, 0, 0x0E, 1, 0, 0, 0x0E, 2];
        f.extend_from_slice(&[0xDA, 0xFC, 0xDA, 0xFC, 0, 0, 0, 3]);
        f.extend_from_slice(&[0; 16]);
        f.extend_from_slice(&[0xDF, 0xFC, 0xDF, 0xFC, 0, 0, 0, 0, 0, 0, 0, 1]);
        f.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2]);
        assert!(parse(&f).is_some());
        for cut in 0..f.len() {
            assert!(parse(&f[..cut]).is_none());
        }
    }
}
