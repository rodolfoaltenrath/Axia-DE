//! Owns shaper/font lifetimes while exercising the engine's existing shaping ABI.
use libc::{c_char, c_int, c_long};
#[path = "paragraphs.rs"]
mod paragraphs;
pub(super) use paragraphs::PARAGRAPHS;
const FONTS: [&[u8]; 5] = [
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("fonts/NotoSansArabic-LB.ttf"),
    include_bytes!("fonts/NotoSansDevanagari-LB.ttf"),
    include_bytes!("fonts/NotoSansThai-LB.ttf"),
    include_bytes!("fonts/NotoSansCJKsc-LB.otf"),
];
#[repr(C)]
struct Lookup {
    lookup: u16,
    stage: u8,
    per_syllable: u8,
    mask: u32,
}
#[repr(C)]
struct Plan {
    script: [c_char; 4],
    joining: i32,
    indic: i32,
    kind: i32,
    zero_marks: i32,
    gsub: *mut Lookup,
    gpos: *mut Lookup,
    ngsub: i32,
    ngpos: i32,
    first: *mut u8,
    gpos_kern: i32,
}
#[repr(C)]
struct Otf {
    d: *const u8,
    n: usize,
    gsub: u32,
    gsub_len: u32,
    gpos: u32,
    gpos_len: u32,
    cblc: u32,
    cblc_len: u32,
    cbdt: u32,
    cbdt_len: u32,
    cmap: u32,
    hmtx: u32,
    nhmetrics: u32,
    glyph_class: u32,
    mark_class: u32,
    mark_sets: u32,
    lookups: *mut u16,
    nlookups: [i32; 2],
    first: [*mut u8; 2],
    nglyphs: i32,
    plans: [Plan; 6],
    nplans: i32,
}
#[repr(C)]
struct Run {
    g: *mut u16,
    cl: *mut i32,
    len: i32,
    cap: i32,
    mask: *mut u32,
    lig: *mut u8,
    aux: *mut u32,
}
#[repr(C)]
struct Pos {
    adv: i32,
    dx: i32,
    dy: i32,
    att: i32,
    cur: i32,
}
#[repr(C)]
struct Shaper {
    run: Run,
    pos: *mut Pos,
    pos_cap: i32,
    gpos_kern: i32,
    cp: *mut u32,
    order: *mut i32,
    cp_cap: i32,
    plan: *const Plan,
}
extern "C" {
    fn otf_init(o: *mut Otf, data: *const u8, len: usize, offset: i32) -> i32;
    fn otf_free(o: *mut Otf);
    fn otf_cmap(o: *const Otf, cp: u32) -> u16;
    fn shape_run(
        s: *mut Shaper,
        o: *mut Otf,
        cp: *const u32,
        n: i32,
        script: *const c_char,
        rtl: i32,
    ) -> i32;
    fn shape_free(s: *mut Shaper);
    fn bidi_levels(cp: *const u32, n: i32, para: i32, lv: *mut u8, removed: *mut u8) -> i32;
    fn bidi_line(cp: *const u32, lv: *mut u8, start: i32, end: i32, para: i32, order: *mut i32);
    fn uc_ccc(cp: u32) -> i32;
    fn uc_ignorable(cp: u32) -> i32;
    fn uc_bidi(cp: u32) -> i32;
    fn uc_join(cp: u32) -> i32;
    fn uc_script(cp: u32) -> i32;
    fn uc_script_tag(sc: i32) -> *const c_char;
}
struct State {
    fonts: [Otf; 5],
    shaper: Shaper,
}
impl State {
    fn new() -> Option<Self> {
        unsafe {
            let mut state: Self = std::mem::zeroed();
            for (font, data) in state.fonts.iter_mut().zip(FONTS) {
                if otf_init(font, data.as_ptr(), data.len(), 0) != 0 {
                    return None;
                }
            }
            Some(state)
        }
    }
    fn paragraph(&mut self, p: usize) -> Option<(i32, i32, i64)> {
        unsafe {
            let cp: Vec<u32> = PARAGRAPHS.get(p)?.chars().map(u32::from).collect();
            let n = cp.len();
            if n == 0 || n >= 1024 {
                return None;
            }
            let mut lv = vec![0; n];
            let para = bidi_levels(
                cp.as_ptr(),
                n as i32,
                -1,
                lv.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            let mut scr = vec![0; n];
            let mut font = vec![0usize; n];
            let mut last = 0;
            for i in 0..n {
                let sc = uc_script(cp[i]);
                if sc > 1 {
                    last = sc
                }
                scr[i] = last
            }
            let mut next = 0;
            for i in (0..n).rev() {
                if scr[i] <= 1 {
                    scr[i] = next
                } else {
                    next = scr[i]
                }
            }
            let deva = uc_script(0x0915);
            for i in 0..n {
                let c = cp[i];
                let prev = font[i.saturating_sub(1)];
                let sticky = uc_ccc(c) != 0
                    || uc_ignorable(c) != 0
                    || uc_bidi(c) == 8
                    || uc_join(c) == 5
                    || (0x1f3fb..=0x1f3ff).contains(&c);
                font[i] = if i > 0
                    && (sticky || (uc_script(c) <= 1 && otf_cmap(&self.fonts[prev], c) != 0))
                {
                    prev
                } else if otf_cmap(&self.fonts[0], c) != 0 {
                    0
                } else {
                    let f = match scr[i] {
                        7 => 1,
                        20 => 3,
                        36 | 33 | 34 => 4,
                        s if s == deva => 2,
                        _ => 0,
                    };
                    if otf_cmap(&self.fonts[f], c) != 0 {
                        f
                    } else {
                        0
                    }
                };
            }
            let (mut g, mut bad, mut adv) = (0, 0, 0i64);
            let mut rs = 0;
            while rs < n {
                let mut re = rs + 1;
                while re < n && font[re] == font[rs] && scr[re] == scr[rs] && lv[re] == lv[rs] {
                    re += 1
                }
                let k = shape_run(
                    &mut self.shaper,
                    &mut self.fonts[font[rs]],
                    cp[rs..].as_ptr(),
                    (re - rs) as i32,
                    uc_script_tag(scr[rs]),
                    (lv[rs] & 1) as i32,
                );
                if k <= 0 {
                    return None;
                }
                for j in 0..k as usize {
                    if *self.shaper.run.g.add(j) == 0 {
                        bad += 1
                    }
                    let pos = &*self.shaper.pos.add(j);
                    if pos.att < 0 {
                        adv += pos.adv as i64
                    }
                }
                g += k;
                rs = re;
            }
            let mut order = vec![0; n];
            bidi_line(
                cp.as_ptr(),
                lv.as_mut_ptr(),
                0,
                n as i32,
                para,
                order.as_mut_ptr(),
            );
            std::hint::black_box(order);
            Some((g, bad, adv))
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            for f in &mut self.fonts {
                otf_free(f)
            }
            shape_free(&mut self.shaper)
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_shape_check(
    p: c_int,
    g: *mut c_int,
    bad: *mut c_int,
    advance: *mut c_long,
) -> c_int {
    let Some((ng, nb, adv)) = State::new().and_then(|mut s| s.paragraph(p as usize)) else {
        return -1;
    };
    if !g.is_null() {
        *g = ng
    }
    if !bad.is_null() {
        *bad = nb
    }
    if !advance.is_null() {
        *advance = adv as c_long
    }
    0
}
#[no_mangle]
pub extern "C" fn lb_shape(threads: c_int, secs: f64) -> f64 {
    super::super::work::timed(threads, secs, State::new, |s| {
        for p in 0..5 {
            if s.paragraph(p).is_none() {
                return false;
            }
        }
        true
    })
}
