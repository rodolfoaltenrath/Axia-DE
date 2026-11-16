//! Text extraction, selection, search, marks, links, copy mode and serialization.
use super::*;

/// A growing NUL-terminated byte string from the C allocator (callers `free()` it).
pub(crate) struct Str {
    pub s: *mut c_char,
    pub n: usize,
    pub cap: usize,
}

impl Default for Str {
    fn default() -> Self {
        Str { s: null_mut(), n: 0, cap: 0 }
    }
}

pub(crate) unsafe fn str_put(s: &mut Str, d: *const u8, n: usize) {
    if s.n + n + 1 > s.cap {
        let mut cap = if s.cap != 0 { s.cap } else { 256 };
        while cap < s.n + n + 1 {
            cap *= 2;
        }
        let ns = libc::realloc(s.s.cast(), cap) as *mut c_char;
        if ns.is_null() {
            return;
        }
        s.s = ns;
        s.cap = cap;
    }
    std::ptr::copy_nonoverlapping(d, s.s.add(s.n).cast(), n);
    s.n += n;
    *s.s.add(s.n) = 0;
}

fn str_put_b(s: &mut Str, d: &[u8]) {
    unsafe { str_put(s, d.as_ptr(), d.len()) }
}

unsafe fn str_cp1(s: &mut Str, cp: u32) {
    let mut b = [0u8; 4];
    let n;
    if cp < 0x80 {
        b[0] = cp as u8;
        n = 1;
    } else if cp < 0x800 {
        b[0] = (0xc0 | cp >> 6) as u8;
        b[1] = (0x80 | (cp & 0x3f)) as u8;
        n = 2;
    } else if cp < 0x10000 {
        b[0] = (0xe0 | cp >> 12) as u8;
        b[1] = (0x80 | (cp >> 6 & 0x3f)) as u8;
        b[2] = (0x80 | (cp & 0x3f)) as u8;
        n = 3;
    } else {
        b[0] = (0xf0 | cp >> 18) as u8;
        b[1] = (0x80 | (cp >> 12 & 0x3f)) as u8;
        b[2] = (0x80 | (cp >> 6 & 0x3f)) as u8;
        b[3] = (0x80 | (cp & 0x3f)) as u8;
        n = 4;
    }
    str_put(s, b.as_ptr(), n);
}

pub(crate) unsafe fn str_cp(s: &mut Str, cp: u32) {
    let (seq, n) = cps_of(cp);
    for &c in &seq[..n] {
        str_cp1(s, c);
    }
}

pub(crate) unsafe fn line_text(s: &mut Str, l: *mut Line, from: c_int, to: c_int, keep_trailing: bool) {
    if l.is_null() {
        return;
    }
    let len = (*l).len as c_int;
    let mut end = if to < len - 1 { to } else { len - 1 };
    let c = (*l).c;
    if !keep_trailing {
        while end >= from && ((*c.add(end as usize)).cp == 0 || (*c.add(end as usize)).cp == b' ' as u32) && (*c.add(end as usize)).attr & A_WIDE_TAIL == 0 {
            end -= 1;
        }
    }
    let mut x = from;
    while x <= end {
        if (*c.add(x as usize)).attr & A_WIDE_TAIL == 0 {
            let cp = (*c.add(x as usize)).cp;
            str_cp(s, if cp != 0 { cp } else { b' ' as u32 });
        }
        x += 1;
    }
}

fn into_c(mut s: Str) -> *mut c_char {
    if s.s.is_null() {
        str_put_b(&mut s, b"");
    }
    s.s
}

// ---- selection ----

unsafe fn is_word(t: *const Term, cp: u32) -> bool {
    if cp == 0 {
        return false;
    }
    if cp < 128 && !libc::strchr((*(*t).prof).word_chars.as_ptr(), cp as c_int).is_null() {
        return true;
    }
    iswalnum(cp) != 0
}

unsafe fn cell_cp(t: *mut Term, line: i64, col: c_int) -> u32 {
    let l = vt_abs_line(t, line);
    if l.is_null() || col < 0 || col >= (*l).len as c_int {
        return 0;
    }
    if (*(*l).c.add(col as usize)).attr & A_WIDE_TAIL != 0 && col > 0 {
        return vt_base_cp((*(*l).c.add(col as usize - 1)).cp);
    }
    vt_base_cp((*(*l).c.add(col as usize)).cp)
}

unsafe fn url_char(cp: u32) -> bool {
    cp > b' ' as u32 && libc::strchr(c"<>\"'`|{}".as_ptr(), if cp < 128 { cp as c_int } else { b'a' as c_int }).is_null()
}

unsafe fn sel_update(t: *mut Term) {
    let (mut sl, mut el) = ((*t).sel_al, (*t).sel_bl);
    let (mut sc, mut ec) = ((*t).sel_ac, (*t).sel_bc);
    if (*t).sel_rect == 0 && (sl > el || (sl == el && sc > ec)) {
        std::mem::swap(&mut sl, &mut el);
        std::mem::swap(&mut sc, &mut ec);
    }
    if (*t).sel_rect != 0 {
        if sl > el {
            std::mem::swap(&mut sl, &mut el);
        }
        if sc > ec {
            std::mem::swap(&mut sc, &mut ec);
        }
    }
    if (*t).sel_mode == 1 {
        if is_word(t, cell_cp(t, sl, sc)) {
            while sc > 0 && is_word(t, cell_cp(t, sl, sc - 1)) {
                sc -= 1;
            }
        }
        if is_word(t, cell_cp(t, el, ec)) {
            while ec < (*t).cols - 1 && is_word(t, cell_cp(t, el, ec + 1)) {
                ec += 1;
            }
        }
    } else if (*t).sel_mode == 3 {
        if url_char(cell_cp(t, sl, sc)) {
            while sc > 0 && url_char(cell_cp(t, sl, sc - 1)) {
                sc -= 1;
            }
        }
        if url_char(cell_cp(t, el, ec)) {
            while ec < (*t).cols - 1 && url_char(cell_cp(t, el, ec + 1)) {
                ec += 1;
            }
        }
    } else if (*t).sel_mode == 2 {
        while sl > vt_first_abs(t) {
            let p = vt_abs_line(t, sl - 1);
            if p.is_null() || (*p).flags & LINE_WRAPPED == 0 {
                break;
            }
            sl -= 1;
        }
        loop {
            let l = vt_abs_line(t, el);
            if l.is_null() || (*l).flags & LINE_WRAPPED == 0 {
                break;
            }
            el += 1;
        }
        sc = 0;
        ec = (*t).cols - 1;
    }
    (*t).sel_sl = sl;
    (*t).sel_sc = sc;
    (*t).sel_el = el;
    (*t).sel_ec = ec;
    dirty_all(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_select_start(t: *mut Term, line: i64, col: c_int, mode: c_int, rect: c_int) {
    (*t).sel_active = 1;
    (*t).sel_mode = mode;
    (*t).sel_rect = rect;
    (*t).sel_al = line;
    (*t).sel_bl = line;
    (*t).sel_ac = col;
    (*t).sel_bc = col;
    sel_update(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_select_extend(t: *mut Term, line: i64, col: c_int) {
    if (*t).sel_active == 0 {
        return;
    }
    (*t).sel_bl = line;
    (*t).sel_bc = col;
    sel_update(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_select_all(t: *mut Term) {
    (*t).sel_active = 1;
    (*t).sel_mode = 0;
    (*t).sel_rect = 0;
    (*t).sel_al = vt_first_abs(t);
    (*t).sel_ac = 0;
    (*t).sel_bl = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    (*t).sel_bc = (*t).cols - 1;
    sel_update(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_select_clear(t: *mut Term) {
    if (*t).sel_active != 0 {
        dirty_all(t);
    }
    (*t).sel_active = 0;
}

#[no_mangle]
pub unsafe extern "C" fn vt_selected(t: *const Term, line: i64, col: c_int) -> c_int {
    if (*t).sel_active == 0 || line < (*t).sel_sl || line > (*t).sel_el {
        return 0;
    }
    if (*t).sel_rect != 0 {
        return (col >= (*t).sel_sc && col <= (*t).sel_ec) as c_int;
    }
    if line == (*t).sel_sl && col < (*t).sel_sc {
        return 0;
    }
    if line == (*t).sel_el && col > (*t).sel_ec {
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn vt_selection_text(t: *mut Term) -> *mut c_char {
    if (*t).sel_active == 0 {
        return null_mut();
    }
    let mut s = Str::default();
    let mut a = (*t).sel_sl;
    while a <= (*t).sel_el {
        let l = vt_abs_line(t, a);
        let from = if (*t).sel_rect != 0 || a == (*t).sel_sl { (*t).sel_sc } else { 0 };
        let to = if (*t).sel_rect != 0 || a == (*t).sel_el { (*t).sel_ec } else { (*t).cols - 1 };
        let wrapped = !l.is_null() && (*l).flags & LINE_WRAPPED != 0 && (*t).sel_rect == 0;
        line_text(&mut s, l, from, to, wrapped && a != (*t).sel_el);
        if a != (*t).sel_el && !wrapped {
            str_put_b(&mut s, b"\n");
        }
        a += 1;
    }
    into_c(s)
}

#[no_mangle]
pub unsafe extern "C" fn vt_text_range(t: *mut Term, from: i64, to: i64) -> *mut c_char {
    let mut s = Str::default();
    let mut a = from;
    while a <= to {
        let l = vt_abs_line(t, a);
        let wrapped = !l.is_null() && (*l).flags & LINE_WRAPPED != 0;
        line_text(&mut s, l, 0, (*t).cols - 1, wrapped);
        if a != to && !wrapped {
            str_put_b(&mut s, b"\n");
        }
        a += 1;
    }
    into_c(s)
}

#[no_mangle]
pub unsafe extern "C" fn vt_select_last_output(t: *mut Term) -> c_int {
    let first = vt_first_abs(t);
    let mut a = vt_cursor_abs(t);
    while a >= first {
        let l = vt_abs_line(t, a);
        if !l.is_null() && (*l).flags & LINE_PROMPT != 0 {
            break;
        }
        a -= 1;
    }
    let prompt = a;
    a = prompt - 1;
    while a >= first {
        let l = vt_abs_line(t, a);
        if !l.is_null() && (*l).flags & LINE_OUTPUT != 0 {
            break;
        }
        a -= 1;
    }
    if a < first || prompt <= a {
        return 0;
    }
    (*t).sel_active = 1;
    (*t).sel_mode = 0;
    (*t).sel_rect = 0;
    (*t).sel_al = a;
    (*t).sel_ac = 0;
    (*t).sel_bl = prompt - 1;
    (*t).sel_bc = (*t).cols - 1;
    sel_update(t);
    1
}

// ---- search ----

#[no_mangle]
pub unsafe extern "C" fn vt_search_clear(t: *mut Term) {
    if (*t).nmatches != 0 {
        dirty_all(t);
    }
    libc::free((*t).matches.cast());
    (*t).matches = null_mut();
    (*t).nmatches = 0;
    (*t).match_cur = -1;
}

unsafe fn show_line(t: *mut Term, abs: i64) {
    let top = vt_first_abs(t) + (*(*t).buf).count as i64 - (*t).rows as i64 - (*t).view as i64;
    if abs >= top && abs < top + (*t).rows as i64 {
        return;
    }
    let idx = (abs - vt_first_abs(t)) as c_int;
    let v = (*(*t).buf).count - (*t).rows - (idx - (*t).rows / 2);
    let max = (*(*t).buf).count - (*t).rows;
    (*t).view = if v < 0 {
        0
    } else if v > max {
        max
    } else {
        v
    };
    dirty_all(t);
}

unsafe fn fold(cp: u32, cs: bool) -> u32 {
    if cs {
        cp
    } else {
        towlower(cp)
    }
}

/// A logical line (the physical ones joined by line wrapping) as character codes, with the grid
/// position of each one.
struct Gather {
    text: *mut u32,
    pl: *mut i64,
    pc: *mut c_int,
    n: usize,
    cap: usize,
}

impl Gather {
    fn new() -> Self {
        Gather { text: null_mut(), pl: null_mut(), pc: null_mut(), n: 0, cap: 0 }
    }
    unsafe fn free(&mut self) {
        libc::free(self.text.cast());
        libc::free(self.pl.cast());
        libc::free(self.pc.cast());
        *self = Gather::new();
    }
}

unsafe fn gather_push(g: &mut Gather, cp: u32, line: i64, col: c_int) -> bool {
    if g.n + 1 > g.cap {
        let cap = if g.cap != 0 { g.cap * 2 } else { 4096 };
        let nt = libc::realloc(g.text.cast(), cap * 4) as *mut u32;
        if !nt.is_null() {
            g.text = nt;
        }
        let npl = libc::realloc(g.pl.cast(), cap * 8) as *mut i64;
        if !npl.is_null() {
            g.pl = npl;
        }
        let npc = libc::realloc(g.pc.cast(), cap * 4) as *mut c_int;
        if !npc.is_null() {
            g.pc = npc;
        }
        if nt.is_null() || npl.is_null() || npc.is_null() {
            return true;
        }
        g.cap = cap;
    }
    *g.text.add(g.n) = cp;
    *g.pl.add(g.n) = line;
    *g.pc.add(g.n) = col;
    g.n += 1;
    false
}

/// Gathers the logical line that starts at `a`; returns the first line of the next one.
unsafe fn gather_line(t: *mut Term, a: i64, g: &mut Gather) -> i64 {
    let last = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    let mut b = a;
    g.n = 0;
    loop {
        let l = vt_abs_line(t, b);
        let w = if !l.is_null() {
            if (*l).flags & LINE_WRAPPED != 0 {
                (*t).cols
            } else {
                (*l).len as c_int
            }
        } else {
            0
        };
        for x in 0..w {
            if x < (*l).len as c_int && (*(*l).c.add(x as usize)).attr & A_WIDE_TAIL != 0 {
                continue;
            }
            let cp = if x < (*l).len as c_int { (*(*l).c.add(x as usize)).cp } else { 0 };
            let (seq, n) = cps_of(if cp != 0 { cp } else { b' ' as u32 });
            for k in 0..n {
                if gather_push(g, seq[k], b, x) {
                    return last + 1;
                }
            }
        }
        if l.is_null() || (*l).flags & LINE_WRAPPED == 0 || b >= last {
            break;
        }
        b += 1;
    }
    b + 1
}

unsafe fn add_match(t: *mut Term, g: &Gather, i: usize, e: usize) {
    let el = vt_abs_line(t, *g.pl.add(e));
    let pce = *g.pc.add(e);
    let ec = pce + if !el.is_null() && pce < (*el).len as c_int && (*(*el).c.add(pce as usize)).attr & A_WIDE != 0 { 1 } else { 0 };
    *(*t).matches.add((*t).nmatches as usize) = Match { l0: *g.pl.add(i), l1: *g.pl.add(e), c0: *g.pc.add(i), c1: ec };
    (*t).nmatches += 1;
}

unsafe fn search_done(t: *mut Term) {
    if (*t).nmatches != 0 {
        (*t).match_cur = (*t).nmatches - 1;
        show_line(t, (*(*t).matches.add((*t).match_cur as usize)).l0);
    }
    dirty_all(t);
}

/// The query as character codes (a lenient UTF-8 reader, as the C code had it).
unsafe fn decode_query(query: *const c_char, max: usize, f: impl Fn(u32) -> u32) -> ([u32; 512], usize) {
    let mut q = [0u32; 512];
    let mut qn = 0usize;
    let mut p = query as *const u8;
    while *p != 0 && qn < max {
        let mut c = *p as u32;
        let extra = if c < 0x80 {
            0
        } else if c & 0xe0 == 0xc0 {
            1
        } else if c & 0xf0 == 0xe0 {
            2
        } else {
            3
        };
        c &= match extra {
            0 => 0x7f,
            1 => 0x1f,
            2 => 0x0f,
            _ => 0x07,
        };
        p = p.add(1);
        let mut k = 0;
        while k < extra && *p & 0xc0 == 0x80 {
            c = c << 6 | (*p & 0x3f) as u32;
            p = p.add(1);
            k += 1;
        }
        q[qn] = f(c);
        qn += 1;
    }
    (q, qn)
}

#[no_mangle]
pub unsafe extern "C" fn vt_search(t: *mut Term, query: *const c_char, cs: c_int) -> c_int {
    vt_search_clear(t);
    let cs = cs != 0;
    let (q, qn) = decode_query(query, 256, |c| fold(c, cs));
    if qn == 0 {
        return 0;
    }
    (*t).matches = libc::malloc(MAX_MATCHES * std::mem::size_of::<Match>()) as *mut Match;
    if (*t).matches.is_null() {
        return 0;
    }
    let mut g = Gather::new();
    let last = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    let mut a = vt_first_abs(t);
    while a <= last && ((*t).nmatches as usize) < MAX_MATCHES {
        a = gather_line(t, a, &mut g);
        let mut i = 0usize;
        while i + qn <= g.n && ((*t).nmatches as usize) < MAX_MATCHES {
            let mut k = 0;
            while k < qn && fold(*g.text.add(i + k), cs) == q[k] {
                k += 1;
            }
            if k < qn {
                i += 1;
                continue;
            }
            add_match(t, &g, i, i + qn - 1);
            i += qn - 1;
            i += 1;
        }
    }
    g.free();
    search_done(t);
    (*t).nmatches
}

// ---- saving and restoring the screen ----

unsafe fn sgr_color(s: &mut Str, c: u32, fg: bool) {
    let b = match c >> 24 {
        1 => format!(";{};5;{}", if fg { 38 } else { 48 }, c & 0xff),
        2 => format!(";{};2;{};{};{}", if fg { 38 } else { 48 }, c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff),
        _ => return,
    };
    str_put_b(s, b.as_bytes());
}

unsafe fn sgr_cell(s: &mut Str, c: Cell) {
    str_put_b(s, b"\x1b[0");
    const MAP: [(u16, &[u8]); 11] = [
        (A_BOLD, b";1"),
        (A_DIM, b";2"),
        (A_ITALIC, b";3"),
        (A_UNDER, b";4"),
        (A_UNDER2, b";21"),
        (A_CURLY, b";4:3"),
        (A_BLINK, b";5"),
        (A_INVERSE, b";7"),
        (A_HIDDEN, b";8"),
        (A_STRIKE, b";9"),
        (A_OVERLINE, b";53"),
    ];
    for (a, code) in MAP {
        if c.attr & a != 0 {
            str_put_b(s, code);
        }
    }
    sgr_color(s, c.fg, true);
    sgr_color(s, c.bg, false);
    str_put_b(s, b"m");
}

unsafe fn serialize_line(s: &mut Str, l: *mut Line, pen: &mut Cell) {
    for x in 0..(*l).len as usize {
        let c = *(*l).c.add(x);
        if c.attr & A_WIDE_TAIL != 0 {
            continue;
        }
        let look = c.attr & !(A_WIDE | A_WIDE_TAIL);
        if look != pen.attr || c.fg != pen.fg || c.bg != pen.bg {
            let mut p = c;
            p.attr = look;
            sgr_cell(s, p);
            *pen = p;
        }
        if c.cp != 0 {
            str_cp(s, c.cp);
        } else {
            str_put_b(s, b" ");
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn vt_serialize(t: *mut Term, max_lines: c_int, len: *mut usize) -> *mut c_char {
    let mut s = Str::default();
    let b = &mut (*t).main as *mut Buffer;
    let from = if (*b).count > max_lines { (*b).count - max_lines } else { 0 };
    let mut pen = Cell::default();
    for i in from..(*b).count {
        let l = bl(b, i);
        if (*l).flags & LINE_PROMPT != 0 {
            str_put_b(&mut s, b"\x1b]133;A\x07");
        }
        serialize_line(&mut s, l, &mut pen);
        if i + 1 < (*b).count && !((*l).flags & LINE_WRAPPED != 0 && (*l).len as c_int >= (*t).cols) {
            str_put_b(&mut s, b"\x1b[0m\r\n");
            pen = Cell::default();
        }
    }
    str_put_b(&mut s, b"\x1b[0m");
    if (*t).buf == &mut (*t).alt as *mut Buffer {
        str_put_b(&mut s, b"\x1b[?1049h");
        for r in 0..(*t).rows {
            str_put_b(&mut s, format!("\x1b[{}H", r + 1).as_bytes());
            pen = Cell::default();
            serialize_line(&mut s, bl(&mut (*t).alt, (*t).alt.count - (*t).rows + r), &mut pen);
        }
        str_put_b(&mut s, b"\x1b[0m");
    }
    str_put_b(&mut s, format!("\x1b[{};{}H", (*t).cy + 1, (*t).cx + 1).as_bytes());
    if (*t).modes & M_HIDECURSOR != 0 {
        str_put_b(&mut s, b"\x1b[?25l");
    }
    *len = s.n;
    s.s
}

#[no_mangle]
pub unsafe extern "C" fn vt_highlight(t: *mut Term, text: *const c_char, fg: i64, bg: i64) -> c_int {
    let (q, qn) = decode_query(text, 512, |c| c);
    if qn == 0 {
        return 0;
    }
    let mut g = Gather::new();
    let last = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    let first = last - (*t).rows as i64 + 1;
    let mut hit_line: i64 = -1;
    let mut hit = 0usize;
    let mut best = Gather::new();
    let mut a = first;
    while a <= last {
        a = gather_line(t, a, &mut g);
        let mut i = 0usize;
        while i + qn <= g.n {
            let mut k = 0;
            while k < qn && *g.text.add(i + k) == q[k] {
                k += 1;
            }
            if k >= qn {
                hit_line = a;
                hit = i;
            }
            i += 1;
        }
        if hit_line == a {
            best.free();
            best = std::mem::replace(&mut g, Gather::new());
        }
    }
    g.free();
    if hit_line < 0 {
        best.free();
        return 0;
    }
    let mut i = hit;
    while i < hit + qn && i < best.n {
        let l = vt_abs_line(t, *best.pl.add(i));
        let col = *best.pc.add(i);
        if !l.is_null() && col < (*l).len as c_int {
            let c = (*l).c.add(col as usize);
            if fg >= 0 {
                (*c).fg = color_rgb(fg as u32);
            }
            if bg >= 0 {
                (*c).bg = color_rgb(bg as u32);
            }
            if (*c).attr & A_WIDE != 0 && col + 1 < (*l).len as c_int {
                let c1 = (*l).c.add(col as usize + 1);
                if fg >= 0 {
                    (*c1).fg = (*c).fg;
                }
                if bg >= 0 {
                    (*c1).bg = (*c).bg;
                }
            }
        }
        i += 1;
    }
    best.free();
    dirty_all(t);
    1
}

#[no_mangle]
pub unsafe extern "C" fn vt_logical_text(t: *mut Term) -> *mut c_char {
    let mut g = Gather::new();
    let mut s = Str::default();
    let last = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    let mut a = vt_first_abs(t);
    while a <= last {
        a = gather_line(t, a, &mut g);
        for i in 0..g.n {
            str_cp(&mut s, *g.text.add(i));
        }
        str_put_b(&mut s, b"\n");
    }
    g.free();
    into_c(s)
}

#[no_mangle]
pub unsafe extern "C" fn vt_set_matches(t: *mut Term, trip: *const c_int, n: c_int) -> c_int {
    vt_search_clear(t);
    if n <= 0 {
        dirty_all(t);
        return 0;
    }
    (*t).matches = libc::malloc(MAX_MATCHES * std::mem::size_of::<Match>()) as *mut Match;
    if (*t).matches.is_null() {
        return 0;
    }
    let mut g = Gather::new();
    let last = vt_first_abs(t) + (*(*t).buf).count as i64 - 1;
    let (mut idx, mut k) = (0, 0);
    let mut a = vt_first_abs(t);
    while a <= last && k < n && ((*t).nmatches as usize) < MAX_MATCHES {
        a = gather_line(t, a, &mut g);
        while k < n && *trip.add(k as usize * 3) == idx {
            let st = *trip.add(k as usize * 3 + 1);
            let len = *trip.add(k as usize * 3 + 2);
            if st >= 0 && len > 0 && (st + len) as usize <= g.n && ((*t).nmatches as usize) < MAX_MATCHES {
                add_match(t, &g, st as usize, (st + len - 1) as usize);
            }
            k += 1;
        }
        while k < n && *trip.add(k as usize * 3) < idx {
            k += 1;
        }
        idx += 1;
    }
    g.free();
    search_done(t);
    (*t).nmatches
}

#[no_mangle]
pub unsafe extern "C" fn vt_search_move(t: *mut Term, dir: c_int) -> c_int {
    if (*t).nmatches == 0 {
        return -1;
    }
    (*t).match_cur = ((*t).match_cur + dir + (*t).nmatches) % (*t).nmatches;
    show_line(t, (*(*t).matches.add((*t).match_cur as usize)).l0);
    dirty_all(t);
    (*t).match_cur
}

#[no_mangle]
pub unsafe extern "C" fn vt_match_at(t: *const Term, line: i64, col: c_int) -> c_int {
    let (mut lo, mut hi, mut best): (c_int, c_int, c_int) = (0, (*t).nmatches - 1, -1);
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let m = (*t).matches.add(mid as usize);
        if (*m).l0 < line || ((*m).l0 == line && (*m).c0 <= col) {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    if best < 0 {
        return 0;
    }
    let m = (*t).matches.add(best as usize);
    if (*m).l1 > line || ((*m).l1 == line && (*m).c1 >= col) {
        return if best == (*t).match_cur { 2 } else { 1 };
    }
    0
}

// ---- marks ----

#[no_mangle]
pub unsafe extern "C" fn vt_set_mark(t: *mut Term) {
    (*lrow(t, (*t).cy)).flags |= LINE_MARK;
    vt_mark_dirty(t, (*t).cy);
}

#[no_mangle]
pub unsafe extern "C" fn vt_jump_mark(t: *mut Term, dir: c_int) -> c_int {
    let b = (*t).buf;
    let top = (*b).count - (*t).rows - (*t).view;
    let mut i = top + dir;
    while i >= 0 && i < (*b).count {
        if (*bl(b, i)).flags & (LINE_PROMPT | LINE_MARK) != 0 {
            let v = (*b).count - (*t).rows - i;
            let max = (*b).count - (*t).rows;
            (*t).view = if v < 0 {
                0
            } else if v > max {
                max
            } else {
                v
            };
            dirty_all(t);
            return 1;
        }
        i += dir;
    }
    if dir > 0 && (*t).view != 0 {
        (*t).view = 0;
        dirty_all(t);
        return 1;
    }
    0
}

// ---- links, words ----

#[no_mangle]
pub unsafe extern "C" fn vt_url_at(t: *mut Term, line: i64, col: c_int) -> *mut c_char {
    let l = vt_abs_line(t, line);
    if l.is_null() || col < 0 || col >= (*l).len as c_int {
        return null_mut();
    }
    let c = (*l).c;
    let cc = *c.add(col as usize);
    if cc.link != 0 && cc.link as c_int <= (*t).nlinks {
        return libc::strdup(*(*t).links.add(cc.link as usize - 1));
    }
    if !url_char(vt_base_cp(cc.cp)) {
        return null_mut();
    }
    let (mut a, mut b) = (col, col);
    while a > 0 && url_char(vt_base_cp((*c.add(a as usize - 1)).cp)) {
        a -= 1;
    }
    while b + 1 < (*l).len as c_int && url_char(vt_base_cp((*c.add(b as usize + 1)).cp)) {
        b += 1;
    }
    let mut s = Str::default();
    for x in a..=b {
        if (*c.add(x as usize)).attr & A_WIDE_TAIL == 0 {
            str_cp(&mut s, (*c.add(x as usize)).cp);
        }
    }
    if s.s.is_null() {
        return null_mut();
    }
    while s.n != 0 && !libc::strchr(c".,;:!?)]".as_ptr(), *s.s.add(s.n - 1) as c_int).is_null() && !(*s.s.add(s.n - 1) as u8 == b')' && !libc::strchr(s.s, b'(' as c_int).is_null()) {
        s.n -= 1;
        *s.s.add(s.n) = 0;
    }
    let p = s.s;
    let mut scheme = 0usize;
    while (*p.add(scheme) as u8).is_ascii_alphabetic() {
        scheme += 1;
    }
    let ok = (scheme >= 2 && c_starts(p.add(scheme), b"://")) || c_starts(p, b"mailto:") || *p as u8 == b'/' || c_starts(p, b"~/") || c_starts(p, b"./") || c_starts(p, b"www.");
    if !ok {
        libc::free(s.s.cast());
        return null_mut();
    }
    s.s
}

unsafe fn token_char(cp: u32) -> bool {
    cp != 0 && (iswalnum(cp) != 0 || (cp < 128 && !libc::strchr(c"-_./~:@".as_ptr(), cp as c_int).is_null()))
}

#[no_mangle]
pub unsafe extern "C" fn vt_cursor_word(t: *mut Term) -> *mut c_char {
    let l = lrow(t, (*t).cy);
    let b = (*t).cx - 1;
    let mut a = b;
    while a >= 0 && a < (*l).len as c_int && token_char(vt_base_cp((*(*l).c.add(a as usize)).cp)) {
        a -= 1;
    }
    let mut s = Str::default();
    let mut x = a + 1;
    while x <= b && x < (*l).len as c_int {
        str_cp(&mut s, (*(*l).c.add(x as usize)).cp);
        x += 1;
    }
    into_c(s)
}

#[no_mangle]
pub unsafe extern "C" fn vt_words(t: *mut Term, prefix: *const c_char, max: c_int) -> *mut c_char {
    let pn = libc::strlen(prefix);
    let mut out = Str::default();
    let mut found = 0;
    let first = vt_first_abs(t);
    let last = first + (*(*t).buf).count as i64 - 1;
    let stop = if last - 2000 > first { last - 2000 } else { first };
    let mut a = last;
    while a >= stop && found < max {
        let l = vt_abs_line(t, a);
        a -= 1;
        if l.is_null() {
            continue;
        }
        let mut x = (*l).len as c_int - 1;
        while x >= 0 && found < max {
            if !token_char(vt_base_cp((*(*l).c.add(x as usize)).cp)) {
                x -= 1;
                continue;
            }
            let e = x;
            while x >= 0 && token_char(vt_base_cp((*(*l).c.add(x as usize)).cp)) {
                x -= 1;
            }
            let mut w = Str::default();
            for k in x + 1..=e {
                str_cp(&mut w, (*(*l).c.add(k as usize)).cp);
            }
            if !w.s.is_null() && w.n > pn && libc::strncmp(w.s, prefix, pn) == 0 {
                let mut dup = false;
                if !out.s.is_null() {
                    let mut p = out.s;
                    while !p.is_null() && *p != 0 {
                        let nl = libc::strchr(p, b'\n' as c_int);
                        let len = if nl.is_null() { libc::strlen(p) } else { nl as usize - p as usize };
                        if len == w.n && libc::strncmp(p, w.s, len) == 0 {
                            dup = true;
                            break;
                        }
                        p = if nl.is_null() { null_mut() } else { nl.add(1) };
                    }
                }
                if !dup {
                    if out.n != 0 {
                        str_put_b(&mut out, b"\n");
                    }
                    str_put(&mut out, w.s.cast(), w.n);
                    found += 1;
                }
            }
            libc::free(w.s.cast());
        }
    }
    into_c(out)
}

// ---- copy mode ----

#[no_mangle]
pub unsafe extern "C" fn vt_copy_mode(t: *mut Term, on: c_int) -> c_int {
    (*t).copy_active = on;
    (*t).copy_sel = 0;
    (*t).sel_active = 0;
    if on != 0 {
        let top = vt_first_abs(t) + (*(*t).buf).count as i64 - (*t).rows as i64 - (*t).view as i64;
        (*t).copy_line = if (*t).view != 0 { top } else { vt_cursor_abs(t) };
        (*t).copy_col = if (*t).view != 0 { 0 } else { (*t).cx };
    } else {
        (*t).view = 0;
    }
    dirty_all(t);
    on
}

unsafe fn copy_move(t: *mut Term, line: i64, col: c_int) {
    let first = vt_first_abs(t);
    let last = first + (*(*t).buf).count as i64 - 1;
    (*t).copy_line = if line < first {
        first
    } else if line > last {
        last
    } else {
        line
    };
    (*t).copy_col = if col < 0 {
        0
    } else if col >= (*t).cols {
        (*t).cols - 1
    } else {
        col
    };
    if (*t).copy_sel != 0 {
        vt_select_extend(t, (*t).copy_line, (*t).copy_col);
    }
    show_line(t, (*t).copy_line);
    dirty_all(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_copy_key(t: *mut Term, k: *const c_char, ctrl: c_int, yank: *mut *mut c_char) -> c_int {
    let l = (*t).copy_line;
    let c = (*t).copy_col;
    let half = (*t).rows / 2;
    let key = |s: &[u8]| c_eq(k, s);
    let ctrl_b = ctrl != 0;
    if key(b"Escape") || key(b"q") || (ctrl_b && key(b"c")) {
        vt_copy_mode(t, 0);
        return 1;
    }
    if key(b"h") || key(b"ArrowLeft") {
        copy_move(t, l, c - 1);
    } else if key(b"l") || key(b"ArrowRight") {
        copy_move(t, l, c + 1);
    } else if key(b"k") || key(b"ArrowUp") {
        copy_move(t, l - 1, c);
    } else if key(b"j") || key(b"ArrowDown") {
        copy_move(t, l + 1, c);
    } else if key(b"0") || key(b"Home") {
        copy_move(t, l, 0);
    } else if key(b"$") || key(b"End") {
        let ln = vt_abs_line(t, l);
        copy_move(t, l, if !ln.is_null() && (*ln).len != 0 { (*ln).len as c_int - 1 } else { 0 });
    } else if key(b"^") {
        let ln = vt_abs_line(t, l);
        let mut x = 0;
        while !ln.is_null() && x < (*ln).len as c_int && ((*(*ln).c.add(x as usize)).cp == 0 || (*(*ln).c.add(x as usize)).cp == b' ' as u32) {
            x += 1;
        }
        copy_move(t, l, x);
    } else if key(b"g") {
        copy_move(t, vt_first_abs(t), 0);
    } else if key(b"G") {
        copy_move(t, vt_cursor_abs(t), (*t).cx);
    } else if (ctrl_b && key(b"u")) || key(b"PageUp") {
        copy_move(t, l - if ctrl_b { half } else { (*t).rows } as i64, c);
    } else if (ctrl_b && key(b"d")) || key(b"PageDown") {
        copy_move(t, l + if ctrl_b { half } else { (*t).rows } as i64, c);
    } else if key(b"w") || key(b"b") {
        let dir = if *k as u8 == b'w' { 1 } else { -1 };
        let first = vt_first_abs(t);
        let last = first + (*(*t).buf).count as i64 - 1;
        let mut ln = l;
        let mut x = c;
        if dir > 0 {
            while x < (*t).cols && is_word(t, cell_cp(t, ln, x)) {
                x += 1;
            }
            loop {
                while x < (*t).cols && !is_word(t, cell_cp(t, ln, x)) {
                    x += 1;
                }
                if x < (*t).cols || ln >= last {
                    break;
                }
                ln += 1;
                x = 0;
            }
            if x >= (*t).cols {
                x = (*t).cols - 1;
            }
        } else {
            x -= 1;
            loop {
                while x >= 0 && !is_word(t, cell_cp(t, ln, x)) {
                    x -= 1;
                }
                if x >= 0 || ln <= first {
                    break;
                }
                ln -= 1;
                x = (*t).cols - 1;
            }
            if x < 0 {
                x = 0;
            }
            while x > 0 && is_word(t, cell_cp(t, ln, x - 1)) {
                x -= 1;
            }
        }
        copy_move(t, ln, x);
    } else if key(b"v") || key(b"V") || (ctrl_b && key(b"v")) {
        let mode = if ctrl_b {
            3
        } else if *k as u8 == b'V' {
            2
        } else {
            1
        };
        if (*t).copy_sel == mode {
            (*t).copy_sel = 0;
            vt_select_clear(t);
        } else {
            (*t).copy_sel = mode;
            vt_select_start(t, l, c, if mode == 2 { 2 } else { 0 }, (mode == 3) as c_int);
        }
    } else if key(b"y") || key(b"Enter") {
        if (*t).sel_active == 0 {
            vt_select_start(t, l, 0, 2, 0);
        }
        *yank = vt_selection_text(t);
        vt_copy_mode(t, 0);
        return 2;
    }
    0
}
