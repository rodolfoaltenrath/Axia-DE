//! xterm-compatible terminal emulator: the port of server/src/term/vt.c.
//!
//! `Term` and the types it holds are the C structs of server/src/term/term.h, field for field
//! (`#[repr(C)]`): term.c, term_paint.c and ui/js_term.c read and write them directly, so the layout
//! is the contract. Everything the C code allocates (lines, rings, strings returned to the caller)
//! comes from the C allocator, because the callers `free()` it.
#![allow(
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::too_many_arguments,
    dangerous_implicit_autorefs,
    static_mut_refs
)]

use libc::{c_char, c_int, c_void};
use std::ptr::{null, null_mut};

mod osc;
mod text;

pub use osc::*;
pub use text::*;

// ---- term.h ----

pub const TERM_MAX_COLS: c_int = 1000;
pub const TERM_MAX_ROWS: c_int = 500;

pub const A_BOLD: u16 = 1 << 0;
pub const A_DIM: u16 = 1 << 1;
pub const A_ITALIC: u16 = 1 << 2;
pub const A_UNDER: u16 = 1 << 3;
pub const A_UNDER2: u16 = 1 << 4;
pub const A_CURLY: u16 = 1 << 5;
pub const A_BLINK: u16 = 1 << 6;
pub const A_INVERSE: u16 = 1 << 7;
pub const A_HIDDEN: u16 = 1 << 8;
pub const A_STRIKE: u16 = 1 << 9;
pub const A_WIDE: u16 = 1 << 10;
pub const A_WIDE_TAIL: u16 = 1 << 11;
pub const A_OVERLINE: u16 = 1 << 12;

pub const fn color_idx(i: u32) -> u32 {
    0x0100_0000 | i
}
pub const fn color_rgb(c: u32) -> u32 {
    0x0200_0000 | (c & 0xff_ffff)
}

pub const CP_CLUSTER: u32 = 0x8000_0000;
pub const CLUSTER_LEN: usize = 12;

pub const LINE_WRAPPED: u8 = 1;
pub const LINE_PROMPT: u8 = 2;
pub const LINE_OUTPUT: u8 = 4;
pub const LINE_MARK: u8 = 8;
pub const LINE_FAIL: u8 = 16;
pub const LINE_OK: u8 = 32;

pub const M_ORIGIN: u32 = 1 << 0;
pub const M_AUTOWRAP: u32 = 1 << 1;
pub const M_INSERT: u32 = 1 << 2;
pub const M_APPCURSOR: u32 = 1 << 3;
pub const M_APPKEYPAD: u32 = 1 << 4;
pub const M_HIDECURSOR: u32 = 1 << 5;
pub const M_BRACKETED: u32 = 1 << 6;
pub const M_FOCUS: u32 = 1 << 7;
pub const M_MOUSE_X10: u32 = 1 << 8;
pub const M_MOUSE_BTN: u32 = 1 << 9;
pub const M_MOUSE_DRAG: u32 = 1 << 10;
pub const M_MOUSE_ANY: u32 = 1 << 11;
pub const M_MOUSE_SGR: u32 = 1 << 12;
pub const M_SYNC: u32 = 1 << 13;
pub const M_LNM: u32 = 1 << 14;
pub const M_REVERSE: u32 = 1 << 15;
pub const M_ALTSCROLL: u32 = 1 << 16;
pub const M_CURSOR_BLINK: u32 = 1 << 17;
pub const M_GRAPHEME: u32 = 1 << 18;
pub const M_MOUSE: u32 = M_MOUSE_X10 | M_MOUSE_BTN | M_MOUSE_DRAG | M_MOUSE_ANY;

pub const EV_TITLE: u32 = 1 << 0;
pub const EV_CWD: u32 = 1 << 1;
pub const EV_BELL: u32 = 1 << 2;
pub const EV_BADGE: u32 = 1 << 4;
pub const EV_NOTIFY: u32 = 1 << 5;
pub const EV_PROFILE: u32 = 1 << 7;
pub const EV_ATTENTION: u32 = 1 << 8;
pub const EV_COPY: u32 = 1 << 9;
pub const EV_COMMAND: u32 = 1 << 10;
pub const EV_RUN: u32 = 1 << 11;
pub const EV_HOST: u32 = 1 << 12;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Cell {
    pub cp: u32,
    pub fg: u32,
    pub bg: u32,
    pub attr: u16,
    pub link: u16,
}

#[repr(C)]
pub struct Line {
    pub c: *mut Cell,
    pub len: u16,
    pub cap: u16,
    pub flags: u8,
    pub time: u32,
}

#[repr(C)]
pub struct Buffer {
    pub ring: *mut Line,
    pub cap: c_int,
    pub count: c_int,
    pub head: c_int,
    pub dropped: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TermImage {
    pub line: i64,
    pub col: c_int,
    pub wc: c_int,
    pub hc: c_int,
    pub pw: c_int,
    pub ph: c_int,
    pub px: *mut u32,
    pub scaled: *mut u32,
    pub sw: c_int,
    pub sh: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Match {
    pub l0: i64,
    pub l1: i64,
    pub c0: c_int,
    pub c1: c_int,
}

#[repr(C)]
pub struct TermProfile {
    pub font_size: f32,
    pub line_spacing: f32,
    pub padding: f32,
    pub palette: [u32; 16],
    pub fg: u32,
    pub bg: u32,
    pub bold: u32,
    pub cursor: u32,
    pub cursor_text: u32,
    pub sel_bg: u32,
    pub sel_fg: u32,
    pub link: u32,
    pub guide: u32,
    pub bg_opacity: f32,
    pub min_contrast: f32,
    pub bold_bright: c_int,
    pub use_bold: c_int,
    pub use_italic: c_int,
    pub cursor_style: c_int,
    pub cursor_blink: c_int,
    pub cursor_guide: c_int,
    pub blink_text: c_int,
    pub alt_esc: c_int,
    pub scrollback: c_int,
    pub mouse_reporting: c_int,
    pub timestamps: c_int,
    pub copy_on_select: c_int,
    pub alt_scroll: c_int,
    pub has_sel_fg: c_int,
    pub has_cursor_text: c_int,
    pub word_chars: [c_char; 64],
    pub font: [c_int; 4],
    pub font_na: [c_int; 4],
    pub ligatures: c_int,
    pub bidi: c_int,
    pub font_family: [c_char; 64],
    pub na_family: [c_char; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SavedCursor {
    pub cx: c_int,
    pub cy: c_int,
    pub wrap: c_int,
    pub origin: c_int,
    pub gl: c_int,
    pub g: [c_int; 2],
    pub pen: Cell,
}

pub type RespondFn = unsafe extern "C" fn(*mut Term, *const c_char, usize);
pub type TmuxFn = unsafe extern "C" fn(*mut Term, c_int, *const c_char, usize);

#[repr(C)]
pub struct Term {
    pub cols: c_int,
    pub rows: c_int,
    pub main: Buffer,
    pub alt: Buffer,
    pub buf: *mut Buffer,
    pub cx: c_int,
    pub cy: c_int,
    pub wrap_pending: c_int,
    pub pen: Cell,
    pub top: c_int,
    pub bot: c_int,
    pub modes: u32,
    pub tabs: *mut u8,
    pub g: [c_int; 2],
    pub gl: c_int,
    pub cursor_shape: c_int,
    pub saved: SavedCursor,
    pub saved_alt: SavedCursor,
    pub last_cp: u32,

    pub state: c_int,
    pub params: [c_int; 32],
    pub nparams: c_int,
    pub colon: [c_int; 32],
    pub priv_: c_char,
    pub inter: [c_char; 4],
    pub ninter: c_int,
    pub str_: *mut c_char,
    pub str_len: usize,
    pub str_cap: usize,
    pub utf8_cp: u32,
    pub utf8_left: c_int,

    pub title: [c_char; 256],
    pub icon_title: [c_char; 256],
    pub cwd: [c_char; 1024],
    pub badge: [c_char; 256],
    pub notify: [c_char; 512],
    pub profile_req: [c_char; 128],
    pub command: [c_char; 512],
    pub host: [c_char; 256],
    pub palette: [u32; 256],
    pub fg: u32,
    pub bg: u32,
    pub cursor_color: u32,
    pub fg_set: c_int,
    pub bg_set: c_int,
    pub cursor_set: c_int,
    pub links: *mut *mut c_char,
    pub nlinks: c_int,
    pub images: *mut TermImage,
    pub nimages: c_int,
    pub cmd_prompt: i64,
    pub cmd_line: i64,
    pub cmd_col: c_int,
    pub run_cmd: [c_char; 1024],

    pub copy_active: c_int,
    pub copy_col: c_int,
    pub copy_sel: c_int,
    pub copy_line: i64,

    pub sel_active: c_int,
    pub sel_mode: c_int,
    pub sel_rect: c_int,
    pub sel_al: i64,
    pub sel_bl: i64,
    pub sel_ac: c_int,
    pub sel_bc: c_int,
    pub sel_sl: i64,
    pub sel_el: i64,
    pub sel_sc: c_int,
    pub sel_ec: c_int,

    pub matches: *mut Match,
    pub nmatches: c_int,
    pub match_cur: c_int,

    pub view: c_int,
    pub dirty: *mut u8,
    pub all_dirty: c_int,
    pub preview_gen: u64,
    pub events: u32,
    pub sync_until: f64,
    pub focused: c_int,

    pub prof: *mut TermProfile,
    pub cell_w: f32,
    pub cell_h: f32,
    pub respond: Option<RespondFn>,
    pub tmux: Option<TmuxFn>,
    pub user: *mut c_void,
}

pub const S_GROUND: c_int = 0;
pub const S_ESC: c_int = 1;
pub const S_ESC_INT: c_int = 2;
pub const S_CSI: c_int = 3;
pub const S_OSC: c_int = 4;
pub const S_OSC_ESC: c_int = 5;
pub const S_DCS: c_int = 6;
pub const S_DCS_ESC: c_int = 7;
pub const S_IGNORE: c_int = 8;
pub const S_IGNORE_ESC: c_int = 9;
pub const S_TMUX: c_int = 10;
pub const S_TMUX_ESC: c_int = 11;

pub(crate) const STR_MAX: usize = 64 * 1024;
pub(crate) const STR_MAX_FILE: usize = 48 * 1024 * 1024;
pub(crate) const MAX_LINKS: usize = 4096;
pub(crate) const MAX_IMAGES: usize = 64;
pub(crate) const MAX_MATCHES: usize = 20000;

extern "C" {
    pub(crate) fn wcwidth(c: libc::wchar_t) -> c_int;
    pub(crate) fn iswalnum(c: u32) -> c_int;
    pub(crate) fn towlower(c: u32) -> u32;
}

pub(crate) static mut NOW_S: u32 = 0;

/// term.c copies text to the clipboard (`term_set_clipboard`); the C side registers it at start-up so this
/// crate does not depend on a symbol only the compositor defines.
pub type ClipboardFn = unsafe extern "C" fn(*const c_char, usize);
pub(crate) static mut CLIPBOARD: Option<ClipboardFn> = None;

#[no_mangle]
pub unsafe extern "C" fn vt_set_clipboard_hook(f: Option<ClipboardFn>) {
    CLIPBOARD = f;
}

// ---- small libc-flavoured helpers ----

pub(crate) unsafe fn c_eq(a: *const c_char, b: &[u8]) -> bool {
    // b has no NUL; true when the C string a equals it
    let mut i = 0;
    while i < b.len() {
        if *a.add(i) as u8 != b[i] {
            return false;
        }
        i += 1;
    }
    *a.add(i) == 0
}

pub(crate) unsafe fn c_starts(a: *const c_char, b: &[u8]) -> bool {
    for (i, &x) in b.iter().enumerate() {
        if *a.add(i) as u8 != x {
            return false;
        }
    }
    true
}

// ---- lines and buffers ----

pub(crate) unsafe fn line_reserve(l: *mut Line, n: c_int) {
    if n <= (*l).cap as c_int {
        return;
    }
    let c = libc::realloc((*l).c.cast(), n as usize * std::mem::size_of::<Cell>()) as *mut Cell;
    if c.is_null() {
        return;
    }
    std::ptr::write_bytes(c.add((*l).cap as usize), 0, (n - (*l).cap as c_int) as usize);
    (*l).c = c;
    (*l).cap = n as u16;
}

pub(crate) unsafe fn line_reset(l: *mut Line) {
    if !(*l).c.is_null() {
        std::ptr::write_bytes((*l).c, 0, (*l).cap as usize);
    }
    (*l).len = 0;
    (*l).flags = 0;
    (*l).time = 0;
}

pub(crate) fn cell_blank(c: Cell) -> bool {
    c.cp == 0 && c.bg == 0 && c.attr == 0 && c.link == 0
}

pub(crate) unsafe fn line_fill(l: *mut Line, from: c_int, mut to: c_int, b: Cell) {
    let from = if from < 0 { 0 } else { from };
    if to <= from {
        return;
    }
    if cell_blank(b) && to >= (*l).len as c_int {
        if from < (*l).len as c_int {
            std::ptr::write_bytes((*l).c.add(from as usize), 0, ((*l).len as c_int - from) as usize);
            (*l).len = from as u16;
        }
        return;
    }
    line_reserve(l, to);
    if to > (*l).cap as c_int {
        to = (*l).cap as c_int;
    }
    for i in from..to {
        *(*l).c.add(i as usize) = b;
    }
    if to > (*l).len as c_int {
        (*l).len = to as u16;
    }
}

pub(crate) unsafe fn bl(b: *mut Buffer, i: c_int) -> *mut Line {
    (*b).ring.add((((*b).head + i) % (*b).cap) as usize)
}

pub(crate) unsafe fn lrow(t: *mut Term, row: c_int) -> *mut Line {
    bl((*t).buf, (*(*t).buf).count - (*t).rows + row)
}

unsafe fn buf_init(b: *mut Buffer, cap: c_int, rows: c_int) -> c_int {
    (*b).ring = libc::calloc(cap as usize, std::mem::size_of::<Line>()) as *mut Line;
    (*b).cap = cap;
    (*b).count = rows;
    (*b).head = 0;
    (*b).dropped = 0;
    if (*b).ring.is_null() {
        -1
    } else {
        0
    }
}

pub(crate) unsafe fn buf_free(b: *mut Buffer) {
    if (*b).ring.is_null() {
        return;
    }
    for i in 0..(*b).cap {
        libc::free((*(*b).ring.add(i as usize)).c.cast());
    }
    libc::free((*b).ring.cast());
    (*b).ring = null_mut();
}

unsafe fn buf_push(b: *mut Buffer) {
    if (*b).count < (*b).cap {
        line_reset(bl(b, (*b).count));
        (*b).count += 1;
        return;
    }
    line_reset((*b).ring.add((*b).head as usize));
    (*b).head = ((*b).head + 1) % (*b).cap;
    (*b).dropped += 1;
}

pub(crate) unsafe fn blank(t: *const Term) -> Cell {
    Cell { cp: 0, fg: 0, bg: (*t).pen.bg, attr: 0, link: 0 }
}

#[no_mangle]
pub unsafe extern "C" fn vt_mark_dirty(t: *mut Term, row: c_int) {
    (*t).preview_gen += 1;
    if row >= 0 && row < (*t).rows {
        *(*t).dirty.add(row as usize) = 1;
    }
}

pub(crate) unsafe fn dirty_all(t: *mut Term) {
    (*t).preview_gen += 1;
    (*t).all_dirty = 1;
}

/// The C `reply()` formatted with vsnprintf into 512 bytes; callers here pass the already formatted text.
pub(crate) unsafe fn reply_bytes(t: *mut Term, s: &[u8]) {
    let n = s.len();
    // vsnprintf into buf[512]: a longer reply is cut to 511 bytes
    let n = if n < 512 { n } else { 511 };
    if n > 0 {
        if let Some(f) = (*t).respond {
            f(t, s.as_ptr().cast(), n);
        }
    }
}

macro_rules! reply {
    ($t:expr, $($arg:tt)*) => {{
        let s = format!($($arg)*);
        $crate::vt::reply_bytes($t, s.as_bytes());
    }};
}
pub(crate) use reply;

#[no_mangle]
pub unsafe extern "C" fn vt_first_abs(t: *const Term) -> i64 {
    (*(*t).buf).dropped
}
#[no_mangle]
pub unsafe extern "C" fn vt_total_lines(t: *const Term) -> i64 {
    (*(*t).buf).count as i64
}
#[no_mangle]
pub unsafe extern "C" fn vt_cursor_abs(t: *const Term) -> i64 {
    (*(*t).buf).dropped + (*(*t).buf).count as i64 - (*t).rows as i64 + (*t).cy as i64
}

#[no_mangle]
pub unsafe extern "C" fn vt_abs_line(t: *mut Term, abs: i64) -> *mut Line {
    let i = abs - (*(*t).buf).dropped;
    if i >= 0 && i < (*(*t).buf).count as i64 {
        bl((*t).buf, i as c_int)
    } else {
        null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn vt_view_line(t: *mut Term, row: c_int, abs: *mut i64) -> *mut Line {
    let i = (*(*t).buf).count - (*t).rows - (*t).view + row;
    if !abs.is_null() {
        *abs = (*(*t).buf).dropped + i as i64;
    }
    if i >= 0 && i < (*(*t).buf).count {
        bl((*t).buf, i)
    } else {
        null_mut()
    }
}

// ---- images ----

pub(crate) unsafe fn image_free(im: *mut TermImage) {
    libc::free((*im).px.cast());
    libc::free((*im).scaled.cast());
}

pub(crate) unsafe fn prune_images(t: *mut Term) {
    let mut k = 0;
    for i in 0..(*t).nimages {
        let im = (*t).images.add(i as usize);
        if (*im).line + (*im).hc as i64 <= (*t).main.dropped {
            image_free(im);
        } else {
            *(*t).images.add(k) = *im;
            k += 1;
        }
    }
    (*t).nimages = k as c_int;
}

// ---- scroll ----

pub(crate) unsafe fn scroll_up(t: *mut Term, top: c_int, bot: c_int, mut n: c_int, to_history: bool) {
    let h = bot - top + 1;
    if n <= 0 || h <= 0 {
        return;
    }
    if n > h {
        n = h;
    }
    let b = blank(t);
    if to_history && top == 0 && bot == (*t).rows - 1 && (*t).buf == &mut (*t).main as *mut Buffer {
        let before = (*t).main.dropped;
        for _ in 0..n {
            buf_push(&mut (*t).main);
            let l = lrow(t, (*t).rows - 1);
            line_fill(l, 0, (*t).cols, b);
        }
        if (*t).view > 0 {
            let max = (*t).main.count - (*t).rows;
            (*t).view += n;
            if (*t).view > max {
                (*t).view = max;
            }
        }
        if (*t).main.dropped != before {
            prune_images(t);
        }
    } else {
        for _ in 0..n {
            let tmp = std::ptr::read(lrow(t, top));
            for r in top..bot {
                std::ptr::copy_nonoverlapping(lrow(t, r + 1), lrow(t, r), 1);
            }
            std::ptr::write(lrow(t, bot), tmp);
            line_reset(lrow(t, bot));
            line_fill(lrow(t, bot), 0, (*t).cols, b);
        }
    }
    dirty_all(t);
}

pub(crate) unsafe fn scroll_down(t: *mut Term, top: c_int, bot: c_int, mut n: c_int) {
    let h = bot - top + 1;
    if n <= 0 || h <= 0 {
        return;
    }
    if n > h {
        n = h;
    }
    let b = blank(t);
    for _ in 0..n {
        let tmp = std::ptr::read(lrow(t, bot));
        let mut r = bot;
        while r > top {
            std::ptr::copy_nonoverlapping(lrow(t, r - 1), lrow(t, r), 1);
            r -= 1;
        }
        std::ptr::write(lrow(t, top), tmp);
        line_reset(lrow(t, top));
        line_fill(lrow(t, top), 0, (*t).cols, b);
    }
    dirty_all(t);
}

pub(crate) unsafe fn index_down(t: *mut Term) {
    if (*t).cy == (*t).bot {
        scroll_up(t, (*t).top, (*t).bot, 1, true);
    } else if (*t).cy < (*t).rows - 1 {
        vt_mark_dirty(t, (*t).cy);
        (*t).cy += 1;
        vt_mark_dirty(t, (*t).cy);
    }
}

pub(crate) unsafe fn reverse_index(t: *mut Term) {
    if (*t).cy == (*t).top {
        scroll_down(t, (*t).top, (*t).bot, 1);
    } else if (*t).cy > 0 {
        vt_mark_dirty(t, (*t).cy);
        (*t).cy -= 1;
        vt_mark_dirty(t, (*t).cy);
    }
}

// ---- cursor ----

pub(crate) unsafe fn move_to(t: *mut Term, x: c_int, y: c_int) {
    vt_mark_dirty(t, (*t).cy);
    let (mut miny, mut maxy) = (0, (*t).rows - 1);
    if (*t).modes & M_ORIGIN != 0 {
        miny = (*t).top;
        maxy = (*t).bot;
    }
    (*t).cx = if x < 0 {
        0
    } else if x >= (*t).cols {
        (*t).cols - 1
    } else {
        x
    };
    (*t).cy = if y < miny {
        miny
    } else if y > maxy {
        maxy
    } else {
        y
    };
    (*t).wrap_pending = 0;
    vt_mark_dirty(t, (*t).cy);
}

pub(crate) unsafe fn save_cursor(t: *mut Term, s: *mut SavedCursor) {
    *s = SavedCursor {
        cx: (*t).cx,
        cy: (*t).cy,
        wrap: (*t).wrap_pending,
        origin: ((*t).modes & M_ORIGIN != 0) as c_int,
        gl: (*t).gl,
        g: [(*t).g[0], (*t).g[1]],
        pen: (*t).pen,
    };
}

pub(crate) unsafe fn restore_cursor(t: *mut Term, s: *const SavedCursor) {
    (*t).pen = (*s).pen;
    (*t).gl = (*s).gl;
    (*t).g[0] = (*s).g[0];
    (*t).g[1] = (*s).g[1];
    if (*s).origin != 0 {
        (*t).modes |= M_ORIGIN;
    } else {
        (*t).modes &= !M_ORIGIN;
    }
    vt_mark_dirty(t, (*t).cy);
    (*t).cx = if (*s).cx < (*t).cols { (*s).cx } else { (*t).cols - 1 };
    (*t).cy = if (*s).cy < (*t).rows { (*s).cy } else { (*t).rows - 1 };
    (*t).wrap_pending = ((*s).wrap != 0 && (*t).cx == (*t).cols - 1) as c_int;
    vt_mark_dirty(t, (*t).cy);
}

pub(crate) unsafe fn reset_tabs(t: *mut Term, from: c_int) {
    for i in from..(*t).cols {
        *(*t).tabs.add(i as usize) = (i % 8 == 0 && i != 0) as u8;
    }
}

// ---- graphemes (an internal global table) ----

const CLUSTER_MAX: usize = 32768;
const CLUSTER_HASH: usize = 65536;

#[repr(C)]
struct Cluster {
    cp: [u32; CLUSTER_LEN],
    n: u8,
}

static mut CLUSTERS: *mut Cluster = null_mut();
static mut NCLUSTERS: c_int = 0;
static mut CLUSTER_HASH_TAB: *mut i32 = null_mut();

fn cluster_key(cp: &[u32]) -> u32 {
    let mut h: u32 = 2166136261;
    for &c in cp {
        h = (h ^ c).wrapping_mul(16777619);
    }
    h & (CLUSTER_HASH as u32 - 1)
}

unsafe fn cluster_intern(cp: &[u32]) -> u32 {
    let n = cp.len();
    if n == 1 {
        return cp[0];
    }
    if CLUSTERS.is_null() {
        CLUSTERS = libc::calloc(CLUSTER_MAX, std::mem::size_of::<Cluster>()) as *mut Cluster;
        CLUSTER_HASH_TAB = libc::malloc(CLUSTER_HASH * 4) as *mut i32;
        if CLUSTERS.is_null() || CLUSTER_HASH_TAB.is_null() {
            libc::free(CLUSTERS.cast());
            libc::free(CLUSTER_HASH_TAB.cast());
            CLUSTERS = null_mut();
            CLUSTER_HASH_TAB = null_mut();
            return 0;
        }
        for i in 0..CLUSTER_HASH {
            *CLUSTER_HASH_TAB.add(i) = -1;
        }
    }
    let mut h = cluster_key(cp);
    loop {
        let i = *CLUSTER_HASH_TAB.add(h as usize);
        if i < 0 {
            if NCLUSTERS as usize >= CLUSTER_MAX {
                return 0;
            }
            let c = CLUSTERS.add(NCLUSTERS as usize);
            (*c).cp[..n].copy_from_slice(cp);
            (*c).n = n as u8;
            *CLUSTER_HASH_TAB.add(h as usize) = NCLUSTERS;
            let id = CP_CLUSTER | NCLUSTERS as u32;
            NCLUSTERS += 1;
            return id;
        }
        let c = CLUSTERS.add(i as usize);
        if (*c).n as usize == n && (*c).cp[..n] == *cp {
            return CP_CLUSTER | i as u32;
        }
        h = (h + 1) & (CLUSTER_HASH as u32 - 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn vt_cluster_cps(cp: u32, out: *mut u32, max: c_int) -> c_int {
    if cp & CP_CLUSTER == 0 {
        if max > 0 {
            *out = cp;
        }
        return 1;
    }
    let i = cp & !CP_CLUSTER;
    if CLUSTERS.is_null() || i >= NCLUSTERS as u32 {
        if max > 0 {
            *out = 0xfffd;
        }
        return 1;
    }
    let c = CLUSTERS.add(i as usize);
    let n = if ((*c).n as c_int) < max { (*c).n as c_int } else { max };
    std::ptr::copy_nonoverlapping((*c).cp.as_ptr(), out, n as usize);
    n
}

#[no_mangle]
pub unsafe extern "C" fn vt_base_cp(cp: u32) -> u32 {
    if cp & CP_CLUSTER == 0 {
        return cp;
    }
    let i = cp & !CP_CLUSTER;
    if !CLUSTERS.is_null() && i < NCLUSTERS as u32 {
        (*CLUSTERS.add(i as usize)).cp[0]
    } else {
        0xfffd
    }
}

/// All characters of a grapheme (cluster or single).
pub(crate) unsafe fn cps_of(cp: u32) -> ([u32; CLUSTER_LEN], usize) {
    let mut seq = [0u32; CLUSTER_LEN];
    let n = vt_cluster_cps(cp, seq.as_mut_ptr(), CLUSTER_LEN as c_int);
    (seq, n as usize)
}

// ---- writing characters ----

static DEC_GRAPHICS: [u16; 31] = [
    0x25c6, 0x2592, 0x2409, 0x240c, 0x240d, 0x240a, 0x00b0, 0x00b1, 0x2424, 0x240b, 0x2518, 0x2510, 0x250c, 0x2514, 0x253c, 0x23ba, 0x23bb, 0x2500, 0x23bc, 0x23bd,
    0x251c, 0x2524, 0x2534, 0x252c, 0x2502, 0x2264, 0x2265, 0x03c0, 0x2260, 0x00a3, 0x00b7,
];

pub(crate) unsafe fn char_width(cp: u32) -> c_int {
    if cp < 0x7f {
        return (cp >= 0x20) as c_int;
    }
    let w = wcwidth(cp as libc::wchar_t);
    if w < 0 {
        1
    } else {
        w
    }
}

pub(crate) unsafe fn clear_wide(l: *mut Line, x: c_int) {
    if x < 0 || x >= (*l).len as c_int {
        return;
    }
    let c = (*l).c;
    if (*c.add(x as usize)).attr & A_WIDE_TAIL != 0 && x > 0 {
        (*c.add(x as usize - 1)).cp = 0;
        (*c.add(x as usize - 1)).attr &= !A_WIDE;
    }
    if (*c.add(x as usize)).attr & A_WIDE != 0 && x + 1 < (*l).len as c_int {
        (*c.add(x as usize + 1)).cp = 0;
        (*c.add(x as usize + 1)).attr &= !A_WIDE_TAIL;
    }
}

unsafe fn prev_cell(t: *mut Term, col: *mut c_int) -> *mut Cell {
    let mut x = if (*t).wrap_pending != 0 { (*t).cx } else { (*t).cx - 1 };
    if x < 0 {
        return null_mut();
    }
    let l = lrow(t, (*t).cy);
    if x >= (*l).len as c_int {
        return null_mut();
    }
    if (*(*l).c.add(x as usize)).attr & A_WIDE_TAIL != 0 && x > 0 {
        x -= 1;
    }
    if (*(*l).c.add(x as usize)).cp == 0 {
        return null_mut();
    }
    *col = x;
    (*l).c.add(x as usize)
}

fn is_ri(cp: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&cp)
}

unsafe fn joins(t: *mut Term, cp: u32, w: c_int, prev: *mut Cell) -> bool {
    if w == 0 {
        return true;
    }
    if (*t).modes & M_GRAPHEME == 0 || prev.is_null() {
        return false;
    }
    let (seq, n) = cps_of((*prev).cp);
    if seq[n - 1] == 0x200d {
        return true;
    }
    if (0x1f3fb..=0x1f3ff).contains(&cp) && (*prev).attr & A_WIDE != 0 {
        return true;
    }
    is_ri(cp) && n == 1 && is_ri(seq[0])
}

pub(crate) unsafe fn put_char(t: *mut Term, mut cp: u32) {
    if (*t).g[(*t).gl as usize] == 1 && (0x60..=0x7e).contains(&cp) {
        cp = DEC_GRAPHICS[(cp - 0x60) as usize] as u32;
    }
    let w = char_width(cp);
    let mut pcol: c_int = 0;
    let mut prev = prev_cell(t, &mut pcol);
    if joins(t, cp, w, prev) {
        if prev.is_null() {
            return;
        }
        let (mut seq, mut n) = cps_of((*prev).cp);
        if n >= CLUSTER_LEN {
            return;
        }
        seq[n] = cp;
        n += 1;
        let id = cluster_intern(&seq[..n]);
        if id != 0 {
            (*prev).cp = id;
        }
        if (*t).modes & M_GRAPHEME != 0
            && (*prev).attr & A_WIDE == 0
            && (cp == 0xfe0f || is_ri(cp))
            && (*t).wrap_pending == 0
            && pcol + 1 < (*t).cols
            && (*t).cx == pcol + 1
        {
            let l = lrow(t, (*t).cy);
            line_reserve(l, pcol + 2);
            clear_wide(l, pcol + 1);
            prev = (*l).c.add(pcol as usize);
            (*prev).attr |= A_WIDE;
            let mut tail = *prev;
            tail.cp = 0;
            tail.attr = ((*prev).attr & !A_WIDE) | A_WIDE_TAIL;
            *(*l).c.add(pcol as usize + 1) = tail;
            if ((*l).len as c_int) < pcol + 2 {
                (*l).len = (pcol + 2) as u16;
            }
            if pcol + 2 >= (*t).cols {
                (*t).cx = (*t).cols - 1;
                (*t).wrap_pending = ((*t).modes & M_AUTOWRAP != 0) as c_int;
            } else {
                (*t).cx = pcol + 2;
            }
        }
        vt_mark_dirty(t, (*t).cy);
        return;
    }
    (*t).last_cp = cp;
    if (*t).wrap_pending != 0 && (*t).modes & M_AUTOWRAP != 0 {
        (*lrow(t, (*t).cy)).flags |= LINE_WRAPPED;
        (*t).cx = 0;
        index_down(t);
    }
    (*t).wrap_pending = 0;
    if w == 2 && (*t).cx == (*t).cols - 1 {
        if (*t).modes & M_AUTOWRAP != 0 {
            let l = lrow(t, (*t).cy);
            line_fill(l, (*t).cx, (*t).cx + 1, blank(t));
            (*l).flags |= LINE_WRAPPED;
            (*t).cx = 0;
            index_down(t);
        } else {
            return;
        }
    }
    let l = lrow(t, (*t).cy);
    line_reserve(l, (*t).cols);
    if (*t).modes & M_INSERT != 0 && (*t).cx + w < (*t).cols {
        std::ptr::copy((*l).c.add((*t).cx as usize), (*l).c.add(((*t).cx + w) as usize), ((*t).cols - (*t).cx - w) as usize);
        if ((*l).len as c_int) < (*t).cols {
            (*l).len = if (*l).len as c_int + w > (*t).cols { (*t).cols } else { (*l).len as c_int + w } as u16;
        }
    }
    clear_wide(l, (*t).cx);
    if w == 2 {
        clear_wide(l, (*t).cx + 1);
    }
    let mut c = (*t).pen;
    c.cp = cp;
    c.attr &= !(A_WIDE | A_WIDE_TAIL);
    if w == 2 {
        c.attr |= A_WIDE;
    }
    *(*l).c.add((*t).cx as usize) = c;
    if w == 2 {
        let mut tail = c;
        tail.cp = 0;
        tail.attr = (c.attr & !A_WIDE) | A_WIDE_TAIL;
        *(*l).c.add((*t).cx as usize + 1) = tail;
    }
    if (*t).cx + w > (*l).len as c_int {
        (*l).len = ((*t).cx + w) as u16;
    }
    if (*l).time == 0 {
        (*l).time = NOW_S;
    }
    vt_mark_dirty(t, (*t).cy);
    if (*t).cx + w >= (*t).cols {
        (*t).cx = (*t).cols - 1;
        (*t).wrap_pending = ((*t).modes & M_AUTOWRAP != 0) as c_int;
    } else {
        (*t).cx += w;
    }
}

// ---- erasing and editing ----

pub(crate) unsafe fn erase_line(t: *mut Term, row: c_int, from: c_int, to: c_int) {
    let l = lrow(t, row);
    clear_wide(l, from);
    clear_wide(l, to - 1);
    line_fill(l, from, to, blank(t));
    if from == 0 && to >= (*t).cols {
        (*l).flags &= !LINE_WRAPPED;
    }
    vt_mark_dirty(t, row);
}

pub(crate) unsafe fn erase_display(t: *mut Term, mode: c_int) {
    if mode == 0 {
        erase_line(t, (*t).cy, (*t).cx, (*t).cols);
        for r in (*t).cy + 1..(*t).rows {
            line_reset(lrow(t, r));
            erase_line(t, r, 0, (*t).cols);
        }
    } else if mode == 1 {
        for r in 0..(*t).cy {
            line_reset(lrow(t, r));
            erase_line(t, r, 0, (*t).cols);
        }
        erase_line(t, (*t).cy, 0, (*t).cx + 1);
    } else if mode == 2 {
        for r in 0..(*t).rows {
            line_reset(lrow(t, r));
            erase_line(t, r, 0, (*t).cols);
        }
    } else if mode == 3 {
        vt_clear_scrollback(t);
    }
    (*t).wrap_pending = 0;
}

pub(crate) unsafe fn insert_chars(t: *mut Term, mut n: c_int) {
    let l = lrow(t, (*t).cy);
    if n > (*t).cols - (*t).cx {
        n = (*t).cols - (*t).cx;
    }
    line_reserve(l, (*t).cols);
    clear_wide(l, (*t).cx);
    std::ptr::copy((*l).c.add((*t).cx as usize), (*l).c.add(((*t).cx + n) as usize), ((*t).cols - (*t).cx - n) as usize);
    for i in 0..n {
        *(*l).c.add(((*t).cx + i) as usize) = blank(t);
    }
    if (*l).len as c_int > (*t).cx {
        (*l).len = if (*l).len as c_int + n > (*t).cols { (*t).cols } else { (*l).len as c_int + n } as u16;
    } else if !cell_blank(blank(t)) {
        (*l).len = ((*t).cx + n) as u16;
    }
    vt_mark_dirty(t, (*t).cy);
}

pub(crate) unsafe fn delete_chars(t: *mut Term, mut n: c_int) {
    let l = lrow(t, (*t).cy);
    if n > (*t).cols - (*t).cx {
        n = (*t).cols - (*t).cx;
    }
    line_reserve(l, (*t).cols);
    clear_wide(l, (*t).cx);
    clear_wide(l, (*t).cx + n - 1);
    std::ptr::copy((*l).c.add(((*t).cx + n) as usize), (*l).c.add((*t).cx as usize), ((*t).cols - (*t).cx - n) as usize);
    for i in (*t).cols - n..(*t).cols {
        *(*l).c.add(i as usize) = blank(t);
    }
    if cell_blank(blank(t)) {
        let mut len = if (*l).len as c_int > n { (*l).len as c_int - n } else { 0 };
        if len < (*t).cx {
            len = if ((*l).len as c_int) < (*t).cx { (*l).len as c_int } else { (*t).cx };
        }
        (*l).len = len as u16;
    } else {
        (*l).len = (*t).cols as u16;
    }
    vt_mark_dirty(t, (*t).cy);
}

// ---- alternate screen ----

pub(crate) unsafe fn use_alt(t: *mut Term, on: bool, clear: bool) {
    if on == ((*t).buf == &mut (*t).alt as *mut Buffer) {
        return;
    }
    if on {
        save_cursor(t, &mut (*t).saved_alt);
        (*t).buf = &mut (*t).alt;
        if clear {
            for r in 0..(*t).rows {
                line_reset(lrow(t, r));
            }
        }
    } else {
        (*t).buf = &mut (*t).main;
        let s = (*t).saved_alt;
        restore_cursor(t, &s);
    }
    (*t).view = 0;
    (*t).sel_active = 0;
    vt_search_clear(t);
    dirty_all(t);
}

// ---- palette ----

#[no_mangle]
pub unsafe extern "C" fn vt_apply_palette(t: *mut Term) {
    const LV: [u32; 6] = [0, 95, 135, 175, 215, 255];
    for i in 0..16 {
        (*t).palette[i] = (*(*t).prof).palette[i] | 0xff00_0000;
    }
    for i in 0..216 {
        (*t).palette[16 + i] = 0xff00_0000 | LV[i / 36] << 16 | LV[i / 6 % 6] << 8 | LV[i % 6];
    }
    for i in 0..24u32 {
        let g = 8 + 10 * i;
        (*t).palette[232 + i as usize] = 0xff00_0000 | g << 16 | g << 8 | g;
    }
    dirty_all(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_resolve(t: *const Term, color: u32, is_fg: c_int) -> u32 {
    match color >> 24 {
        1 => (*t).palette[(color & 0xff) as usize],
        2 => 0xff00_0000 | (color & 0xff_ffff),
        _ => {
            if is_fg != 0 {
                if (*t).fg_set != 0 {
                    (*t).fg
                } else {
                    (*(*t).prof).fg | 0xff00_0000
                }
            } else if (*t).bg_set != 0 {
                (*t).bg
            } else {
                (*(*t).prof).bg | 0xff00_0000
            }
        }
    }
}

// ---- SGR ----

unsafe fn prm(t: *const Term, i: usize, def: c_int) -> c_int {
    if (i as c_int) < (*t).nparams && (*t).params[i] > 0 {
        (*t).params[i]
    } else {
        def
    }
}
unsafe fn prm0(t: *const Term, i: usize, def: c_int) -> c_int {
    if (i as c_int) < (*t).nparams && (*t).params[i] >= 0 {
        (*t).params[i]
    } else {
        def
    }
}

unsafe fn sgr(t: *mut Term) {
    if (*t).nparams == 0 {
        (*t).pen.attr = 0;
        (*t).pen.fg = 0;
        (*t).pen.bg = 0;
        return;
    }
    let np = (*t).nparams;
    let mut i: c_int = 0;
    while i < np {
        let p = if (*t).params[i as usize] < 0 { 0 } else { (*t).params[i as usize] };
        let mut ns: c_int = 0;
        while i + 1 + ns < np && (*t).colon[(i + 1 + ns) as usize] != 0 {
            ns += 1;
        }
        let a = &mut (*t).pen.attr as *mut u16;
        match p {
            0 => {
                *a = 0;
                (*t).pen.fg = 0;
                (*t).pen.bg = 0;
            }
            1 => *a |= A_BOLD,
            2 => *a |= A_DIM,
            3 => *a |= A_ITALIC,
            4 => {
                *a &= !(A_UNDER | A_UNDER2 | A_CURLY);
                if ns > 0 {
                    let s = (*t).params[i as usize + 1];
                    if s == 2 {
                        *a |= A_UNDER2;
                    } else if s == 3 {
                        *a |= A_CURLY;
                    } else if s > 0 {
                        *a |= A_UNDER;
                    }
                } else {
                    *a |= A_UNDER;
                }
            }
            5 | 6 => *a |= A_BLINK,
            7 => *a |= A_INVERSE,
            8 => *a |= A_HIDDEN,
            9 => *a |= A_STRIKE,
            21 => *a = (*a & !(A_UNDER | A_CURLY)) | A_UNDER2,
            22 => *a &= !(A_BOLD | A_DIM),
            23 => *a &= !A_ITALIC,
            24 => *a &= !(A_UNDER | A_UNDER2 | A_CURLY),
            25 => *a &= !A_BLINK,
            27 => *a &= !A_INVERSE,
            28 => *a &= !A_HIDDEN,
            29 => *a &= !A_STRIKE,
            53 => *a |= A_OVERLINE,
            55 => *a &= !A_OVERLINE,
            39 => (*t).pen.fg = 0,
            49 => (*t).pen.bg = 0,
            38 | 48 | 58 => {
                let mut col: u32 = 0;
                let mut ok = false;
                let mut used: c_int = 0;
                let q = (i + 1) as usize;
                let m = if (q as c_int) < np { (*t).params[q] } else { -1 };
                let pr = |k: usize| (*t).params[k];
                if ns > 0 {
                    used = ns;
                    if m == 5 && ns >= 2 {
                        col = color_idx((pr(q + 1) & 0xff) as u32);
                        ok = true;
                    } else if m == 2 && ns >= 4 {
                        let o = if ns >= 5 { q + 2 } else { q + 1 };
                        col = color_rgb(((pr(o) & 0xff) as u32) << 16 | ((pr(o + 1) & 0xff) as u32) << 8 | (pr(o + 2) & 0xff) as u32);
                        ok = true;
                    }
                } else if m == 5 && (q as c_int) + 1 < np {
                    col = color_idx((pr(q + 1) & 0xff) as u32);
                    ok = true;
                    used = 2;
                } else if m == 2 && (q as c_int) + 3 < np {
                    col = color_rgb(((pr(q + 1) & 0xff) as u32) << 16 | ((pr(q + 2) & 0xff) as u32) << 8 | (pr(q + 3) & 0xff) as u32);
                    ok = true;
                    used = 4;
                }
                if ok && p == 38 {
                    (*t).pen.fg = col;
                }
                if ok && p == 48 {
                    (*t).pen.bg = col;
                }
                i += used;
                i += 1;
                continue;
            }
            _ => {
                if (30..=37).contains(&p) {
                    (*t).pen.fg = color_idx((p - 30) as u32);
                } else if (40..=47).contains(&p) {
                    (*t).pen.bg = color_idx((p - 40) as u32);
                } else if (90..=97).contains(&p) {
                    (*t).pen.fg = color_idx((p - 90 + 8) as u32);
                } else if (100..=107).contains(&p) {
                    (*t).pen.bg = color_idx((p - 100 + 8) as u32);
                }
            }
        }
        i += ns;
        i += 1;
    }
}

// ---- modes ----

unsafe fn set_flag(t: *mut Term, f: u32, on: bool) {
    if on {
        (*t).modes |= f;
    } else {
        (*t).modes &= !f;
    }
}

unsafe fn set_mouse(t: *mut Term, f: u32, on: bool) {
    if on {
        (*t).modes &= !M_MOUSE;
    }
    set_flag(t, f, on);
}

fn mono_ms() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 * 1e3 + ts.tv_nsec as f64 / 1e6
}

unsafe fn dec_mode(t: *mut Term, mode: c_int, on: bool) {
    match mode {
        1 => set_flag(t, M_APPCURSOR, on),
        5 => {
            set_flag(t, M_REVERSE, on);
            dirty_all(t);
        }
        6 => {
            set_flag(t, M_ORIGIN, on);
            move_to(t, 0, if on { (*t).top } else { 0 });
        }
        7 => set_flag(t, M_AUTOWRAP, on),
        9 => set_mouse(t, M_MOUSE_X10, on),
        12 => set_flag(t, M_CURSOR_BLINK, on),
        25 => {
            set_flag(t, M_HIDECURSOR, !on);
            vt_mark_dirty(t, (*t).cy);
        }
        47 | 1047 => use_alt(t, on, mode == 1047),
        66 => set_flag(t, M_APPKEYPAD, on),
        1000 => set_mouse(t, M_MOUSE_BTN, on),
        1002 => set_mouse(t, M_MOUSE_DRAG, on),
        1003 => set_mouse(t, M_MOUSE_ANY, on),
        1004 => set_flag(t, M_FOCUS, on),
        1006 => set_flag(t, M_MOUSE_SGR, on),
        1007 => set_flag(t, M_ALTSCROLL, on),
        1048 => {
            if on {
                save_cursor(t, &mut (*t).saved);
            } else {
                let s = (*t).saved;
                restore_cursor(t, &s);
            }
        }
        1049 => use_alt(t, on, true),
        2004 => set_flag(t, M_BRACKETED, on),
        2027 => set_flag(t, M_GRAPHEME, on),
        2026 => {
            set_flag(t, M_SYNC, on);
            (*t).sync_until = if on { mono_ms() + 150.0 } else { 0.0 };
            if !on {
                dirty_all(t);
            }
        }
        _ => {}
    }
}

unsafe fn dec_mode_state(t: *const Term, mode: c_int) -> c_int {
    let f = match mode {
        1 => M_APPCURSOR,
        5 => M_REVERSE,
        6 => M_ORIGIN,
        7 => M_AUTOWRAP,
        9 => M_MOUSE_X10,
        12 => M_CURSOR_BLINK,
        25 => return if (*t).modes & M_HIDECURSOR != 0 { 2 } else { 1 },
        47 | 1047 | 1049 => return if (*t).buf == &(*t).alt as *const Buffer as *mut Buffer { 1 } else { 2 },
        66 => M_APPKEYPAD,
        1000 => M_MOUSE_BTN,
        1002 => M_MOUSE_DRAG,
        1003 => M_MOUSE_ANY,
        1004 => M_FOCUS,
        1006 => M_MOUSE_SGR,
        1007 => M_ALTSCROLL,
        2004 => M_BRACKETED,
        2026 => M_SYNC,
        2027 => M_GRAPHEME,
        _ => return 0,
    };
    if (*t).modes & f != 0 {
        1
    } else {
        2
    }
}

// ---- CSI ----

unsafe fn soft_reset(t: *mut Term) {
    (*t).modes = ((*t).modes & (M_MOUSE | M_MOUSE_SGR | M_FOCUS | M_BRACKETED)) | M_AUTOWRAP;
    (*t).pen = Cell::default();
    (*t).top = 0;
    (*t).bot = (*t).rows - 1;
    (*t).g = [0, 0];
    (*t).gl = 0;
    (*t).cursor_shape = 0;
    save_cursor(t, &mut (*t).saved);
}

unsafe fn csi(t: *mut Term, f: u8) {
    let n = prm(t, 0, 1);
    let origin_top = if (*t).modes & M_ORIGIN != 0 { (*t).top } else { 0 };
    if (*t).priv_ as u8 == b'?' {
        if f == b'h' || f == b'l' {
            for i in 0..(*t).nparams as usize {
                dec_mode(t, (*t).params[i], f == b'h');
            }
        } else if f == b'p' && (*t).ninter == 1 && (*t).inter[0] as u8 == b'$' {
            let m = prm0(t, 0, 0);
            reply!(t, "\x1b[?{};{}$y", m, dec_mode_state(t, m));
        } else if f == b'n' && prm0(t, 0, 0) == 6 {
            reply!(t, "\x1b[?{};{}R", (*t).cy + 1 - origin_top, (*t).cx + 1);
        } else if f == b'J' {
            erase_display(t, prm0(t, 0, 0));
        } else if f == b'K' {
            let m = prm0(t, 0, 0);
            erase_line(t, (*t).cy, if m == 0 { (*t).cx } else { 0 }, if m == 1 { (*t).cx + 1 } else { (*t).cols });
        } else if f == b's' {
            save_cursor(t, &mut (*t).saved);
        } else if f == b'S' {
            let item = prm0(t, 0, 0);
            if item == 1 {
                reply!(t, "\x1b[?1;0;256S");
            } else if item == 2 {
                reply!(t, "\x1b[?2;0;{};{}S", ((*t).cols as f32 * (*t).cell_w) as c_int, ((*t).rows as f32 * (*t).cell_h) as c_int);
            } else {
                reply!(t, "\x1b[?{};1;0S", item);
            }
        }
        return;
    }
    if (*t).priv_ as u8 == b'>' {
        if f == b'c' {
            reply!(t, "\x1b[>0;276;0c");
        } else if f == b'q' {
            reply!(t, "\x1bP>|LordOS Terminal 0.4\x1b\\");
        }
        return;
    }
    if (*t).priv_ != 0 {
        return;
    }
    if (*t).ninter == 1 {
        let inn = (*t).inter[0] as u8;
        if inn == b' ' && f == b'q' {
            (*t).cursor_shape = prm0(t, 0, 0);
            vt_mark_dirty(t, (*t).cy);
        } else if inn == b'!' && f == b'p' {
            soft_reset(t);
        } else if inn == b'$' && f == b'p' {
            let m = prm0(t, 0, 0);
            let st = if m == 4 {
                if (*t).modes & M_INSERT != 0 {
                    1
                } else {
                    2
                }
            } else if m == 20 {
                if (*t).modes & M_LNM != 0 {
                    1
                } else {
                    2
                }
            } else {
                0
            };
            reply!(t, "\x1b[{};{}$y", m, st);
        }
        return;
    }
    let (cx, cy, cols, rows, top, bot) = ((*t).cx, (*t).cy, (*t).cols, (*t).rows, (*t).top, (*t).bot);
    match f {
        b'@' => insert_chars(t, n),
        b'A' => {
            let lim = if cy >= top { top } else { 0 };
            move_to(t, cx, if cy - n < lim { lim } else { cy - n });
        }
        b'B' | b'e' => {
            let lim = if cy <= bot { bot } else { rows - 1 };
            move_to(t, cx, if cy + n > lim { lim } else { cy + n });
        }
        b'C' | b'a' => move_to(t, cx + n, cy),
        b'D' => move_to(t, cx - n, cy),
        b'E' => move_to(t, 0, if cy + n > bot { bot } else { cy + n }),
        b'F' => move_to(t, 0, if cy - n < top { top } else { cy - n }),
        b'G' | b'`' => move_to(t, n - 1, cy),
        b'H' | b'f' => {
            let y = prm(t, 0, 1) - 1;
            let x = prm(t, 1, 1) - 1;
            move_to(t, x, if (*t).modes & M_ORIGIN != 0 { y + top } else { y });
        }
        b'I' => {
            for _ in 0..n {
                let mut x = (*t).cx + 1;
                while x < cols - 1 && *(*t).tabs.add(x as usize) == 0 {
                    x += 1;
                }
                move_to(t, x, (*t).cy);
            }
        }
        b'Z' => {
            for _ in 0..n {
                let mut x = (*t).cx - 1;
                while x > 0 && *(*t).tabs.add(x as usize) == 0 {
                    x -= 1;
                }
                move_to(t, x, (*t).cy);
            }
        }
        b'J' => erase_display(t, prm0(t, 0, 0)),
        b'K' => {
            let m = prm0(t, 0, 0);
            erase_line(t, cy, if m == 0 { cx } else { 0 }, if m == 1 { cx + 1 } else { cols });
            (*t).wrap_pending = 0;
        }
        b'L' => {
            if cy >= top && cy <= bot {
                scroll_down(t, cy, bot, n);
                (*t).cx = 0;
            }
        }
        b'M' => {
            if cy >= top && cy <= bot {
                scroll_up(t, cy, bot, n, false);
                (*t).cx = 0;
            }
        }
        b'P' => delete_chars(t, n),
        b'S' => scroll_up(t, top, bot, n, false),
        b'T' => {
            if (*t).nparams <= 1 {
                scroll_down(t, top, bot, n);
            }
        }
        b'X' => {
            let l = lrow(t, cy);
            let to = if cx + n > cols { cols } else { cx + n };
            clear_wide(l, cx);
            clear_wide(l, to - 1);
            line_fill(l, cx, to, blank(t));
            vt_mark_dirty(t, cy);
        }
        b'b' => {
            if (*t).last_cp == 0 {
                return;
            }
            let max = cols * rows;
            let mut i = 0;
            while i < n && i < max {
                put_char(t, (*t).last_cp);
                i += 1;
            }
        }
        b'c' => {
            if prm0(t, 0, 0) == 0 {
                reply!(t, "\x1b[?62;4;22c");
            }
        }
        b'd' => move_to(t, cx, if (*t).modes & M_ORIGIN != 0 { n - 1 + top } else { n - 1 }),
        b'g' => {
            let m = prm0(t, 0, 0);
            if m == 0 && cx < cols {
                *(*t).tabs.add(cx as usize) = 0;
            } else if m == 3 {
                std::ptr::write_bytes((*t).tabs, 0, cols as usize);
            }
        }
        b'h' | b'l' => {
            for i in 0..(*t).nparams as usize {
                if (*t).params[i] == 4 {
                    set_flag(t, M_INSERT, f == b'h');
                }
                if (*t).params[i] == 20 {
                    set_flag(t, M_LNM, f == b'h');
                }
            }
        }
        b'm' => sgr(t),
        b'n' => {
            if prm0(t, 0, 0) == 5 {
                reply!(t, "\x1b[0n");
            } else if prm0(t, 0, 0) == 6 {
                reply!(t, "\x1b[{};{}R", cy + 1 - origin_top, cx + 1);
            }
        }
        b'r' => {
            let tp = prm(t, 0, 1) - 1;
            let mut bt = prm(t, 1, rows) - 1;
            if bt >= rows {
                bt = rows - 1;
            }
            if tp < bt {
                (*t).top = tp;
                (*t).bot = bt;
                move_to(t, 0, if (*t).modes & M_ORIGIN != 0 { tp } else { 0 });
            }
        }
        b's' => save_cursor(t, &mut (*t).saved),
        b'u' => {
            let s = (*t).saved;
            restore_cursor(t, &s);
        }
        b't' => {
            let op = prm0(t, 0, 0);
            if op == 14 {
                reply!(t, "\x1b[4;{};{}t", (rows as f32 * (*t).cell_h) as c_int, (cols as f32 * (*t).cell_w) as c_int);
            } else if op == 16 {
                reply!(t, "\x1b[6;{};{}t", (*t).cell_h as c_int, (*t).cell_w as c_int);
            } else if op == 18 {
                reply!(t, "\x1b[8;{};{}t", rows, cols);
            }
        }
        _ => {}
    }
}

// ---- the byte parser ----

unsafe fn str_add(t: *mut Term, b: u8) {
    let max = if ((*t).str_len >= 10 && libc::strncmp((*t).str_, b"1337;File=".as_ptr().cast(), 10) == 0) || (*t).state == S_DCS || (*t).state == S_TMUX {
        STR_MAX_FILE
    } else {
        STR_MAX
    };
    if (*t).str_len + 1 >= max {
        return;
    }
    if (*t).str_len + 1 >= (*t).str_cap {
        let cap = if (*t).str_cap != 0 { (*t).str_cap * 2 } else { 256 };
        let s = libc::realloc((*t).str_.cast(), cap) as *mut c_char;
        if s.is_null() {
            return;
        }
        (*t).str_ = s;
        (*t).str_cap = cap;
    }
    *(*t).str_.add((*t).str_len) = b as c_char;
    (*t).str_len += 1;
    *(*t).str_.add((*t).str_len) = 0;
}

pub(crate) unsafe fn str_begin(t: *mut Term) {
    (*t).str_len = 0;
    if !(*t).str_.is_null() {
        *(*t).str_ = 0;
    } else {
        str_add(t, 0);
        (*t).str_len = 0;
    }
}

pub(crate) unsafe fn str_end(t: *mut Term) {
    if (*t).str_cap > STR_MAX {
        libc::free((*t).str_.cast());
        (*t).str_ = null_mut();
        (*t).str_cap = 0;
    }
    (*t).str_len = 0;
}

unsafe fn control(t: *mut Term, b: u8) {
    match b {
        0x07 => (*t).events |= EV_BELL,
        0x08 => {
            if (*t).cx > 0 {
                (*t).cx -= 1;
                vt_mark_dirty(t, (*t).cy);
            }
            (*t).wrap_pending = 0;
        }
        0x09 => {
            let mut x = (*t).cx + 1;
            while x < (*t).cols - 1 && *(*t).tabs.add(x as usize) == 0 {
                x += 1;
            }
            (*t).cx = if x < (*t).cols { x } else { (*t).cols - 1 };
            (*t).wrap_pending = 0;
            vt_mark_dirty(t, (*t).cy);
        }
        0x0a | 0x0b | 0x0c => {
            index_down(t);
            if (*t).modes & M_LNM != 0 {
                (*t).cx = 0;
            }
            (*t).wrap_pending = 0;
        }
        0x0d => {
            (*t).cx = 0;
            (*t).wrap_pending = 0;
            vt_mark_dirty(t, (*t).cy);
        }
        0x0e => (*t).gl = 1,
        0x0f => (*t).gl = 0,
        0x18 | 0x1a => (*t).state = S_GROUND,
        0x1b => {
            (*t).state = S_ESC;
            (*t).ninter = 0;
        }
        _ => {}
    }
}

unsafe fn esc(t: *mut Term, b: u8) {
    (*t).state = S_GROUND;
    match b {
        b'[' => {
            (*t).state = S_CSI;
            (*t).nparams = 0;
            (*t).priv_ = 0;
            (*t).ninter = 0;
            (*t).colon = [0; 32];
        }
        b']' => {
            (*t).state = S_OSC;
            str_begin(t);
        }
        b'P' => {
            (*t).state = S_DCS;
            str_begin(t);
        }
        b'X' | b'^' | b'_' => (*t).state = S_IGNORE,
        b'7' => save_cursor(t, &mut (*t).saved),
        b'8' => {
            let s = (*t).saved;
            restore_cursor(t, &s);
        }
        b'D' => index_down(t),
        b'E' => {
            (*t).cx = 0;
            index_down(t);
        }
        b'H' => {
            if (*t).cx < (*t).cols {
                *(*t).tabs.add((*t).cx as usize) = 1;
            }
        }
        b'M' => reverse_index(t),
        b'c' => vt_reset(t),
        b'=' => (*t).modes |= M_APPKEYPAD,
        b'>' => (*t).modes &= !M_APPKEYPAD,
        _ => {
            if (0x20..=0x2f).contains(&b) {
                (*t).inter[0] = b as c_char;
                (*t).ninter = 1;
                (*t).state = S_ESC_INT;
            }
        }
    }
}

unsafe fn esc_int(t: *mut Term, b: u8) {
    if (0x20..=0x2f).contains(&b) {
        if (*t).ninter < 3 {
            (*t).inter[(*t).ninter as usize] = b as c_char;
            (*t).ninter += 1;
        }
        return;
    }
    (*t).state = S_GROUND;
    let i0 = (*t).inter[0] as u8;
    if i0 == b'(' || i0 == b')' {
        (*t).g[(i0 == b')') as usize] = (b == b'0') as c_int;
    } else if i0 == b'#' && b == b'8' {
        for r in 0..(*t).rows {
            let l = lrow(t, r);
            line_fill(l, 0, (*t).cols, Cell { cp: b'E' as u32, fg: 0, bg: 0, attr: 0, link: 0 });
            vt_mark_dirty(t, r);
        }
    }
}

unsafe fn csi_byte(t: *mut Term, b: u8) {
    if b.is_ascii_digit() {
        if (*t).nparams == 0 {
            (*t).nparams = 1;
            (*t).params[0] = -1;
        }
        let p = &mut (*t).params[((*t).nparams - 1) as usize];
        if *p < 0 {
            *p = 0;
        }
        if *p < 100000 {
            *p = *p * 10 + (b - b'0') as c_int;
        }
    } else if b == b';' || b == b':' {
        if (*t).nparams == 0 {
            (*t).nparams = 1;
            (*t).params[0] = -1;
        }
        if (*t).nparams < 32 {
            (*t).colon[(*t).nparams as usize] = (b == b':') as c_int;
            (*t).params[(*t).nparams as usize] = -1;
            (*t).nparams += 1;
        }
    } else if (0x3c..=0x3f).contains(&b) {
        if (*t).nparams == 0 {
            (*t).priv_ = b as c_char;
        }
    } else if (0x20..=0x2f).contains(&b) {
        if (*t).ninter < 3 {
            (*t).inter[(*t).ninter as usize] = b as c_char;
            (*t).ninter += 1;
        }
    } else if (0x40..=0x7e).contains(&b) {
        (*t).state = S_GROUND;
        csi(t, b);
    } else if b < 0x20 {
        if b == 0x1b {
            (*t).state = S_ESC;
            (*t).ninter = 0;
        } else {
            control(t, b);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn vt_write(t: *mut Term, d: *const u8, n: usize) {
    NOW_S = libc::time(null_mut()) as u32;
    let d = if n == 0 { &[][..] } else { std::slice::from_raw_parts(d, n) };
    let mut i = 0usize;
    while i < n {
        let b = d[i];
        match (*t).state {
            S_GROUND => {
                if (*t).utf8_left != 0 {
                    if b & 0xc0 == 0x80 {
                        (*t).utf8_cp = (*t).utf8_cp << 6 | (b & 0x3f) as u32;
                        (*t).utf8_left -= 1;
                        if (*t).utf8_left == 0 {
                            put_char(t, if (*t).utf8_cp > 0x10ffff { 0xfffd } else { (*t).utf8_cp });
                        }
                        i += 1;
                        continue;
                    }
                    (*t).utf8_left = 0;
                    put_char(t, 0xfffd);
                }
                if (0x20..0x7f).contains(&b) {
                    put_char(t, b as u32);
                    while i + 1 < n && (0x20..0x7f).contains(&d[i + 1]) {
                        i += 1;
                        put_char(t, d[i] as u32);
                    }
                } else if b < 0x20 {
                    control(t, b);
                } else if b == 0x7f {
                } else if b & 0xe0 == 0xc0 {
                    (*t).utf8_cp = (b & 0x1f) as u32;
                    (*t).utf8_left = 1;
                } else if b & 0xf0 == 0xe0 {
                    (*t).utf8_cp = (b & 0x0f) as u32;
                    (*t).utf8_left = 2;
                } else if b & 0xf8 == 0xf0 {
                    (*t).utf8_cp = (b & 0x07) as u32;
                    (*t).utf8_left = 3;
                } else {
                    put_char(t, 0xfffd);
                }
            }
            S_ESC => {
                if b == 0x1b {
                } else if b < 0x20 {
                    control(t, b);
                } else {
                    esc(t, b);
                }
            }
            S_ESC_INT => {
                if b < 0x20 {
                    control(t, b);
                } else {
                    esc_int(t, b);
                }
            }
            S_CSI => csi_byte(t, b),
            S_OSC => {
                if b == 0x07 {
                    osc(t);
                    str_end(t);
                    (*t).state = S_GROUND;
                } else if b == 0x1b {
                    (*t).state = S_OSC_ESC;
                } else if b == 0x18 || b == 0x1a {
                    str_end(t);
                    (*t).state = S_GROUND;
                } else {
                    str_add(t, b);
                }
            }
            S_OSC_ESC => {
                osc(t);
                str_end(t);
                (*t).state = S_GROUND;
                if b != b'\\' {
                    (*t).state = S_ESC;
                    esc(t, b);
                }
            }
            S_DCS => {
                if b == 0x1b {
                    (*t).state = S_DCS_ESC;
                } else if b == 0x18 || b == 0x1a {
                    str_end(t);
                    (*t).state = S_GROUND;
                } else if b == b'p' && (*t).tmux.is_some() && (*t).str_len == 4 && libc::memcmp((*t).str_.cast(), b"1000".as_ptr().cast(), 4) == 0 {
                    str_end(t);
                    str_begin(t);
                    (*t).state = S_TMUX;
                    ((*t).tmux.unwrap())(t, 1, null(), 0);
                } else {
                    str_add(t, b);
                }
            }
            S_TMUX => {
                if b == b'\n' {
                    let mut len = (*t).str_len;
                    if len != 0 && *(*t).str_.add(len - 1) as u8 == b'\r' {
                        len -= 1;
                    }
                    if !(*t).str_.is_null() {
                        *(*t).str_.add(len) = 0;
                    }
                    ((*t).tmux.unwrap())(t, 0, if (*t).str_.is_null() { c"".as_ptr() } else { (*t).str_ }, len);
                    (*t).str_len = 0;
                } else if b == 0x1b {
                    (*t).state = S_TMUX_ESC;
                } else {
                    str_add(t, b);
                }
            }
            S_TMUX_ESC => {
                if b == b'\\' {
                    str_end(t);
                    (*t).state = S_GROUND;
                    ((*t).tmux.unwrap())(t, 2, null(), 0);
                } else {
                    (*t).state = S_TMUX;
                    str_add(t, 0x1b);
                    str_add(t, b);
                }
            }
            S_DCS_ESC => {
                if b == b'\\' {
                    dcs(t);
                    str_end(t);
                    (*t).state = S_GROUND;
                } else {
                    str_add(t, 0x1b);
                    str_add(t, b);
                    (*t).state = S_DCS;
                }
            }
            S_IGNORE => {
                if b == 0x1b {
                    (*t).state = S_IGNORE_ESC;
                } else if b == 0x18 || b == 0x1a {
                    (*t).state = S_GROUND;
                }
            }
            S_IGNORE_ESC => (*t).state = if b == b'\\' { S_GROUND } else { S_IGNORE },
            _ => {}
        }
        i += 1;
    }
}

// ---- life cycle ----

#[no_mangle]
pub unsafe extern "C" fn vt_init(t: *mut Term, mut cols: c_int, mut rows: c_int, scrollback: c_int, p: *mut TermProfile) -> c_int {
    std::ptr::write_bytes(t, 0, 1);
    if cols < 2 {
        cols = 2;
    }
    if rows < 1 {
        rows = 1;
    }
    (*t).cols = cols;
    (*t).rows = rows;
    (*t).prof = p;
    if buf_init(&mut (*t).main, scrollback + rows, rows) != 0 || buf_init(&mut (*t).alt, rows, rows) != 0 {
        return -1;
    }
    (*t).buf = &mut (*t).main;
    (*t).tabs = libc::calloc(cols as usize, 1) as *mut u8;
    (*t).dirty = libc::calloc(rows as usize, 1) as *mut u8;
    if (*t).tabs.is_null() || (*t).dirty.is_null() {
        return -1;
    }
    (*t).cmd_prompt = -1;
    (*t).cmd_line = -1;
    (*t).focused = 1;
    vt_reset(t);
    0
}

#[no_mangle]
pub unsafe extern "C" fn vt_reset(t: *mut Term) {
    if (*t).buf == &mut (*t).alt as *mut Buffer {
        (*t).buf = &mut (*t).main;
    }
    (*t).modes = M_AUTOWRAP | M_ALTSCROLL;
    (*t).pen = Cell::default();
    (*t).top = 0;
    (*t).bot = (*t).rows - 1;
    (*t).cx = 0;
    (*t).cy = 0;
    (*t).wrap_pending = 0;
    (*t).g = [0, 0];
    (*t).gl = 0;
    (*t).cursor_shape = 0;
    (*t).fg_set = 0;
    (*t).bg_set = 0;
    (*t).cursor_set = 0;
    (*t).state = S_GROUND;
    (*t).utf8_left = 0;
    reset_tabs(t, 0);
    for r in 0..(*t).rows {
        line_reset(lrow(t, r));
    }
    save_cursor(t, &mut (*t).saved);
    save_cursor(t, &mut (*t).saved_alt);
    vt_apply_palette(t);
    dirty_all(t);
}

#[no_mangle]
pub unsafe extern "C" fn vt_free(t: *mut Term) {
    buf_free(&mut (*t).main);
    buf_free(&mut (*t).alt);
    libc::free((*t).tabs.cast());
    libc::free((*t).dirty.cast());
    libc::free((*t).str_.cast());
    for i in 0..(*t).nlinks {
        libc::free((*(*t).links.add(i as usize)).cast());
    }
    libc::free((*t).links.cast());
    for i in 0..(*t).nimages {
        image_free((*t).images.add(i as usize));
    }
    libc::free((*t).images.cast());
    libc::free((*t).matches.cast());
    std::ptr::write_bytes(t, 0, 1);
}

#[no_mangle]
pub unsafe extern "C" fn vt_enter_tmux(t: *mut Term) {
    str_end(t);
    str_begin(t);
    (*t).state = S_TMUX;
}

#[no_mangle]
pub unsafe extern "C" fn vt_in_tmux(t: *const Term) -> c_int {
    ((*t).state == S_TMUX || (*t).state == S_TMUX_ESC) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn vt_scroll_view(t: *mut Term, delta: c_int) {
    let max = (*(*t).buf).count - (*t).rows;
    let mut v = (*t).view + delta;
    if v < 0 {
        v = 0;
    }
    if v > max {
        v = max;
    }
    if v != (*t).view {
        (*t).view = v;
        dirty_all(t);
    }
}

#[no_mangle]
pub unsafe extern "C" fn vt_clear_scrollback(t: *mut Term) {
    if (*t).buf != &mut (*t).main as *mut Buffer {
        return;
    }
    let b = &mut (*t).main as *mut Buffer;
    let extra = (*b).count - (*t).rows;
    for i in 0..extra {
        line_reset(bl(b, i));
    }
    (*b).head = ((*b).head + extra) % (*b).cap;
    (*b).count = (*t).rows;
    (*b).dropped += extra as i64;
    (*t).view = 0;
    (*t).sel_active = 0;
    vt_search_clear(t);
    prune_images(t);
    dirty_all(t);
}

mod resize;


#[cfg(test)]
mod tests {
    use super::resize::vt_resize;
    use super::*;

    unsafe fn new_term(cols: c_int, rows: c_int, prof: *mut TermProfile) -> Box<Term> {
        let mut t: Box<Term> = Box::new(std::mem::zeroed());
        assert_eq!(vt_init(&mut *t, cols, rows, 10, prof), 0);
        t
    }

    unsafe fn profile() -> Box<TermProfile> {
        Box::new(std::mem::zeroed())
    }

    unsafe fn row_text(t: &mut Term, row: c_int) -> String {
        let l = lrow(t, row);
        (0..(*l).len as usize).map(|x| char::from_u32((*(*l).c.add(x)).cp.max(32)).unwrap_or('?')).collect()
    }

    #[test]
    fn text_colors_and_cursor() {
        unsafe {
            let mut p = profile();
            let mut t = new_term(10, 3, &mut *p);
            let s = b"ab\x1b[31mc\r\nx";
            vt_write(&mut *t, s.as_ptr(), s.len());
            assert_eq!(row_text(&mut t, 0), "abc");
            assert_eq!((*(*lrow(&mut *t, 0)).c.add(2)).fg, color_idx(1));
            assert_eq!(((*t).cx, (*t).cy), (1, 1));
            vt_free(&mut *t);
        }
    }

    #[test]
    fn wide_characters_take_two_cells_and_wrap_whole() {
        unsafe {
            // wcwidth() needs the UTF-8 locale that term_init() sets in the server
            libc::setlocale(libc::LC_CTYPE, c"C.UTF-8".as_ptr());
            let mut p = profile();
            let mut t = new_term(4, 3, &mut *p);
            let s = "ab\u{65e5}\u{672c}".as_bytes();
            vt_write(&mut *t, s.as_ptr(), s.len());
            let l = lrow(&mut *t, 0);
            assert_eq!((*(*l).c.add(2)).attr & A_WIDE, A_WIDE);
            assert_eq!((*(*l).c.add(3)).attr & A_WIDE_TAIL, A_WIDE_TAIL);
            // the next wide character does not fit on the row: the whole of it goes to the next
            assert_eq!(((*t).cx, (*t).cy), (2, 1));
            vt_free(&mut *t);
        }
    }

    #[test]
    fn device_reports_go_through_respond() {
        use std::sync::Mutex;
        static GOT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
        unsafe extern "C" fn respond(_t: *mut Term, s: *const c_char, n: usize) {
            GOT.lock().unwrap().extend_from_slice(std::slice::from_raw_parts(s.cast::<u8>(), n));
        }
        unsafe {
            let mut p = profile();
            let mut t = new_term(10, 3, &mut *p);
            t.respond = Some(respond);
            let s = b"\x1b[2;3H\x1b[6n";
            vt_write(&mut *t, s.as_ptr(), s.len());
            assert_eq!(&*GOT.lock().unwrap(), b"\x1b[2;3R");
            vt_free(&mut *t);
        }
    }

    #[test]
    fn resize_reflows_wrapped_lines() {
        unsafe {
            let mut p = profile();
            let mut t = new_term(5, 3, &mut *p);
            let s = b"0123456789";
            vt_write(&mut *t, s.as_ptr(), s.len());
            vt_resize(&mut *t, 10, 3);
            assert_eq!(row_text(&mut t, 0), "0123456789");
            vt_free(&mut *t);
        }
    }

    #[test]
    fn selection_text_and_search() {
        unsafe {
            let mut p = profile();
            let mut t = new_term(20, 3, &mut *p);
            let s = b"hello world\r\nfoo hello";
            vt_write(&mut *t, s.as_ptr(), s.len());
            assert_eq!(vt_search(&mut *t, c"hello".as_ptr(), 0), 2);
            vt_select_start(&mut *t, vt_first_abs(&*t), 6, 0, 0);
            vt_select_extend(&mut *t, vt_first_abs(&*t), 10);
            let txt = vt_selection_text(&mut *t);
            assert_eq!(std::ffi::CStr::from_ptr(txt).to_str().unwrap(), "world");
            libc::free(txt.cast());
            vt_free(&mut *t);
        }
    }
}
