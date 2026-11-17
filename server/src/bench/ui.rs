//! Fixed desktop and five-script shaping workloads. The engine is borrowed through
//! its transitional ABI; scene construction, worker ownership and verification are Rust.
#[path = "shaping.rs"]
mod shaping;
#[path = "../../../rust/axia-shell/src/runtime/types.rs"]
#[allow(dead_code)]
mod types;
use super::bench_now_ms;
use libc::{c_char, c_int, c_void};
pub use shaping::{lb_shape, lb_shape_check};
use std::{
    cell::{Cell, RefCell},
    ffi::CString,
    sync::{Arc, Condvar, Mutex},
    thread,
};
use types::{Node, Rect, Style, Ui};
const W: i32 = 1920;
const H: i32 = 1080;
const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
#[repr(C)]
pub struct Canvas {
    px: *mut u32,
    w: i32,
    h: i32,
    stride: i32,
    clip: Clip,
}
#[repr(C)]
struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
type Band = unsafe extern "C" fn(*mut c_void, i32, i32);
extern "C" {
    static mut ui: Ui;
    fn node_new(kind: i32) -> *mut Node;
    fn node_append(parent: *mut Node, child: *mut Node);
    fn node_set_text(node: *mut Node, text: *const c_char);
    fn node_invalidate(node: *mut Node);
    fn node_count() -> i32;
    fn ui_reset();
    fn style_set_number(s: *mut Style, k: *const c_char, v: f64) -> i32;
    fn style_set_string(s: *mut Style, k: *const c_char, v: *const c_char) -> i32;
    fn text_init_mem(bytes: *const u8, n: usize) -> i32;
    fn layout_root(root: *mut Node, w: f32, h: f32) -> i32;
    fn paint_damage(root: *mut Node) -> i32;
    fn paint_root(c: *mut Canvas, root: *mut Node);
    fn canvas_init(c: *mut Canvas, px: *mut u32, w: i32, h: i32, stride: i32);
    fn canvas_clear(c: *mut Canvas, color: u32);
    fn canvas_set_clip(c: *mut Canvas, r: Clip);
    fn canvas_parallel(f: Option<unsafe extern "C" fn(i32, i32, Option<Band>, *mut c_void)>);
}
thread_local! {
    static ROOT: Cell<*mut Node> = const { Cell::new(std::ptr::null_mut()) };
    static UNKNOWN: Cell<i32> = const { Cell::new(0) };
    static POOL: RefCell<Option<Pool>> = const { RefCell::new(None) };
    static IN_POOL: Cell<bool> = const { Cell::new(false) };
}
unsafe fn view(p: *mut Node) -> *mut Node {
    let n = node_new(0);
    assert!(!n.is_null(), "benchmark scene allocation");
    if !p.is_null() {
        node_append(p, n)
    }
    n
}
unsafe fn sn(n: *mut Node, k: &str, v: impl Into<f64>) {
    if style_set_number(&mut (*n).s, CString::new(k).unwrap().as_ptr(), v.into()) != 0 {
        UNKNOWN.set(UNKNOWN.get() + 1)
    }
}
unsafe fn ss(n: *mut Node, k: &str, v: &str) {
    if style_set_string(
        &mut (*n).s,
        CString::new(k).unwrap().as_ptr(),
        CString::new(v).unwrap().as_ptr(),
    ) != 0
    {
        UNKNOWN.set(UNKNOWN.get() + 1)
    }
}
unsafe fn pad(n: *mut Node, v: f64) {
    for k in ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"] {
        sn(n, k, v)
    }
}
unsafe fn text(p: *mut Node, s: &str, size: f64, color: u32) -> *mut Node {
    let n = node_new(1);
    let r = node_new(3);
    assert!(!n.is_null() && !r.is_null());
    node_set_text(r, CString::new(s).unwrap().as_ptr());
    node_append(n, r);
    node_append(p, n);
    sn(n, "fontSize", size);
    sn(n, "color", color);
    n
}
unsafe fn bx(
    p: *mut Node,
    w: impl Into<f64>,
    h: impl Into<f64>,
    r: impl Into<f64>,
    c: u32,
) -> *mut Node {
    let n = view(p);
    sn(n, "width", w);
    sn(n, "height", h);
    sn(n, "borderRadius", r);
    sn(n, "backgroundColor", c);
    n
}
unsafe fn window(p: *mut Node, x: i32, y: i32, w: i32, h: i32, bg: u32) -> *mut Node {
    let n = view(p);
    ss(n, "position", "absolute");
    sn(n, "left", x);
    sn(n, "top", y);
    sn(n, "width", w);
    sn(n, "height", h);
    sn(n, "backgroundColor", bg);
    sn(n, "borderRadius", 12);
    sn(n, "shadowColor", 0x59000000u32);
    sn(n, "shadowBlur", 28);
    sn(n, "shadowOffsetY", 10);
    ss(n, "overflow", "hidden");
    n
}
unsafe fn row(n: *mut Node) {
    ss(n, "flexDirection", "row");
    ss(n, "alignItems", "center")
}
unsafe fn build_scene() {
    let words = [
        "Quarterly",
        "report",
        "holiday",
        "photos",
        "final",
        "version",
        "backup",
        "draft",
        "invoice",
        "scan",
        "notes",
        "presentation",
        "budget",
        "summary",
    ];
    let places = [
        "Home",
        "Desktop",
        "Documents",
        "Downloads",
        "Music",
        "Pictures",
        "Videos",
        "Trash",
        "This computer",
        "Network",
        "USB disk",
        "Recent",
        "Starred",
        "Shared",
    ];
    let menu = ["LordOS", "Files", "Edit", "View", "Go", "Window", "Help"];
    let labels = [
        "Wallpaper",
        "Accent colour",
        "Appearance",
        "Text size",
        "Night light",
        "Animations",
        "Notifications",
        "Sounds",
        "Hot corners",
        "Dock size",
    ];
    let colors: [u32; 12] = [
        0xff0a84ff, 0xff30d158, 0xffff9f0a, 0xffff453a, 0xffbf5af2, 0xff64d2ff, 0xffffd60a,
        0xffac8e68, 0xff5e5ce6, 0xffff375f, 0xff66d4cf, 0xff8e8e93,
    ];
    ui_reset();
    UNKNOWN.set(0);
    let root = view(std::ptr::null_mut());
    ROOT.set(root);
    ui.root = root;
    sn(root, "width", W);
    sn(root, "height", H);
    sn(root, "backgroundColor", 0xff2b3a4au32);
    for i in 0..6 {
        let b = bx(
            root,
            520 - i * 40,
            520 - i * 40,
            260 - i * 20,
            colors[i as usize] & 0x80ffffff,
        );
        ss(b, "position", "absolute");
        sn(b, "left", 80 + i * 300);
        sn(b, "top", 120 + (i % 3) * 260)
    }
    let bar = view(root);
    ss(bar, "position", "absolute");
    sn(bar, "left", 0);
    sn(bar, "top", 0);
    sn(bar, "width", W);
    sn(bar, "height", 30);
    sn(bar, "backgroundColor", 0x99202020u32);
    sn(bar, "backdropBlur", 20);
    row(bar);
    sn(bar, "paddingLeft", 14);
    sn(bar, "paddingRight", 14);
    sn(bar, "gap", 18);
    for (i, m) in menu.iter().enumerate() {
        let t = text(bar, m, 13.0, 0xffffffff);
        if i == 0 {
            sn(t, "fontWeight", 600)
        }
    }
    let spring = view(bar);
    sn(spring, "flexGrow", 1);
    text(bar, "Wed 28 Oct  16:20", 13.0, 0xffffffff);
    let files = window(root, 140, 80, 1180, 780, 0xfff5f5f7);
    let title = view(files);
    sn(title, "height", 44);
    row(title);
    sn(title, "paddingLeft", 16);
    sn(title, "gap", 8);
    sn(title, "backgroundColor", 0xffebebefu32);
    sn(title, "borderBottomWidth", 1);
    sn(title, "borderBottomColor", 0xffd4d4d8u32);
    for c in [0xffff5f57, 0xfffebc2e, 0xff28c840] {
        bx(title, 12, 12, 6, c);
    }
    let tt = text(title, "Documents", 14.0, 0xff202020);
    sn(tt, "marginLeft", 20);
    sn(tt, "fontWeight", 600);
    let body = view(files);
    sn(body, "flexGrow", 1);
    ss(body, "flexDirection", "row");
    let side = view(body);
    sn(side, "width", 220);
    sn(side, "backgroundColor", 0xffe6e6ebu32);
    sn(side, "paddingTop", 12);
    sn(side, "gap", 2);
    for i in 0..14 {
        let r = view(side);
        row(r);
        sn(r, "height", 28);
        sn(r, "paddingLeft", 14);
        sn(r, "gap", 10);
        if i == 2 {
            sn(r, "backgroundColor", 0x330a84ffu32);
            sn(r, "borderRadius", 6);
            sn(r, "marginLeft", 6);
            sn(r, "marginRight", 6)
        }
        bx(r, 16, 16, 4, colors[i % 12]);
        text(r, places[i], 13.0, 0xff303030);
    }
    let main = view(body);
    sn(main, "flexGrow", 1);
    let tools = view(main);
    sn(tools, "height", 40);
    row(tools);
    sn(tools, "paddingLeft", 12);
    sn(tools, "gap", 8);
    sn(tools, "borderBottomWidth", 1);
    sn(tools, "borderBottomColor", 0xffd8d8dcu32);
    for _ in 0..5 {
        let b = bx(tools, 28, 24, 6, 0xffffffff);
        sn(b, "borderWidth", 1);
        sn(b, "borderColor", 0xffc8c8ccu32)
    }
    let search = bx(tools, 240, 26, 13, 0xffffffff);
    sn(search, "marginLeft", 300);
    sn(search, "borderWidth", 1);
    sn(search, "borderColor", 0xffc0c0c4u32);
    let list = view(main);
    sn(list, "flexGrow", 1);
    ss(list, "overflow", "hidden");
    let mut seed = 7u64;
    for i in 0..40 {
        let r = view(list);
        row(r);
        sn(r, "height", 26);
        sn(r, "paddingLeft", 12);
        sn(r, "paddingRight", 12);
        sn(r, "gap", 8);
        if i & 1 != 0 {
            sn(r, "backgroundColor", 0xffededf0u32)
        }
        bx(r, 16, 16, 3, colors[(i * 5) % 12]);
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let nw = 2 + (seed >> 60) % 9;
        let mut name = String::new();
        for k in 0..nw {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            if k != 0 {
                name.push(' ')
            }
            name.push_str(words[((seed >> 33) % 14) as usize])
        }
        name.push_str(&format!(" {i}.pdf"));
        let t = text(r, &name, 13.0, 0xff202020);
        sn(t, "flexGrow", 1);
        sn(t, "flexShrink", 1);
        sn(t, "flexBasis", 0);
        ss(t, "whiteSpace", "nowrap");
        ss(t, "textOverflow", "ellipsis");
        let size = text(r, "1.2 MB", 12.0, 0xff707070);
        sn(size, "width", 70);
        ss(size, "textAlign", "end");
        let date = text(r, "18 Oct 2026, 04:20", 12.0, 0xff707070);
        sn(date, "width", 140);
    }
    let status = view(main);
    sn(status, "height", 26);
    sn(status, "paddingLeft", 12);
    ss(status, "justifyContent", "center");
    sn(status, "borderTopWidth", 1);
    sn(status, "borderTopColor", 0xffd8d8dcu32);
    text(status, "40 items, 1.4 GB free", 12.0, 0xff707070);
    let set = window(root, 880, 260, 640, 600, 0xffffffff);
    pad(set, 24.0);
    sn(set, "gap", 10);
    let st = text(set, "Appearance", 22.0, 0xff1c1c1e);
    sn(st, "fontWeight", 700);
    let para = text(set, shaping::PARAGRAPHS[0], 13.0, 0xff48484a);
    sn(para, "lineHeight", 1.45);
    for i in 0..10 {
        let r = view(set);
        row(r);
        sn(r, "height", 30);
        sn(r, "gap", 12);
        let l = text(r, labels[i], 13.0, 0xff1c1c1e);
        sn(l, "width", 150);
        let field = bx(r, 0, 28, 6, 0xfff2f2f7);
        sn(field, "flexGrow", 1);
        sn(field, "borderWidth", 1);
        sn(field, "borderColor", 0xffc7c7ccu32);
        let tog = bx(
            r,
            44,
            24,
            12,
            if i % 3 != 0 { 0xff34c759 } else { 0xffd1d1d6 },
        );
        ss(tog, "flexDirection", "row");
        ss(
            tog,
            "justifyContent",
            if i % 3 != 0 { "flex-end" } else { "flex-start" },
        );
        pad(tog, 2.0);
        let knob = bx(tog, 20, 20, 10, 0xffffffff);
        sn(knob, "shadowColor", 0x40000000u32);
        sn(knob, "shadowBlur", 3);
        sn(knob, "shadowOffsetY", 1)
    }
    let note = window(root, 1500, 44, 390, 110, 0xd0f8f8fa);
    sn(note, "backdropBlur", 24);
    sn(note, "borderRadius", 16);
    pad(note, 14.0);
    sn(note, "gap", 4);
    let nt = text(note, "LordBench", 14.0, 0xff1c1c1e);
    sn(nt, "fontWeight", 600);
    text(
        note,
        "Measuring the interface: layout and paint of a fixed scene, then shaping five scripts.",
        13.0,
        0xff3a3a3c,
    );
    let card = window(root, 1420, 620, 420, 240, 0xff1c1c1e);
    sn(card, "opacity", 0.9);
    pad(card, 18.0);
    sn(card, "gap", 8);
    for i in 0..5 {
        let r = view(card);
        row(r);
        sn(r, "gap", 10);
        bx(r, 140 - i as i32 * 18, 10, 5, colors[i]);
        text(r, menu[i], 12.0, 0xffd0d0d0);
    }
    let badge = bx(root, 120, 36, 18, 0xffff375f);
    ss(badge, "position", "absolute");
    sn(badge, "left", 1330);
    sn(badge, "top", 210);
    sn(badge, "rotate", 8);
    ss(badge, "alignItems", "center");
    ss(badge, "justifyContent", "center");
    text(badge, "New", 14.0, 0xffffffff);
    let dock = view(root);
    ss(dock, "position", "absolute");
    sn(dock, "left", 560);
    sn(dock, "top", 990);
    sn(dock, "width", 800);
    sn(dock, "height", 76);
    sn(dock, "backgroundColor", 0x80ffffffu32);
    sn(dock, "borderRadius", 22);
    sn(dock, "backdropBlur", 24);
    row(dock);
    ss(dock, "justifyContent", "center");
    sn(dock, "gap", 12);
    for c in colors {
        let ic = bx(dock, 52, 52, 12, c);
        sn(ic, "shadowColor", 0x40000000u32);
        sn(ic, "shadowBlur", 6);
        sn(ic, "shadowOffsetY", 2)
    }
}

#[derive(Clone, Copy)]
struct Job {
    j0: i32,
    n: i64,
    pieces: i32,
    next: i32,
    f: Option<Band>,
    ctx: usize,
    generation: u64,
    left: usize,
    stop: bool,
}
struct Shared {
    job: Mutex<Job>,
    go: Condvar,
    done: Condvar,
}
struct Pool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
}
fn pieces(shared: &Shared) {
    IN_POOL.with(|flag| {
        let old = flag.replace(true);
        loop {
            let job = {
                let mut j = shared.job.lock().unwrap();
                if j.next >= j.pieces {
                    break;
                }
                let p = j.next;
                j.next += 1;
                (*j, p)
            };
            let (j, p) = job;
            let a = (j.j0 as i64 + j.n * p as i64 / j.pieces as i64) as i32;
            let b = (j.j0 as i64 + j.n * (p + 1) as i64 / j.pieces as i64) as i32;
            if b > a {
                if let Some(f) = j.f {
                    unsafe { f(j.ctx as *mut c_void, a, b) }
                }
            }
        }
        flag.set(old)
    })
}
impl Pool {
    fn new(n: usize) -> Option<Self> {
        let shared = Arc::new(Shared {
            job: Mutex::new(Job {
                j0: 0,
                n: 0,
                pieces: 0,
                next: 0,
                f: None,
                ctx: 0,
                generation: 0,
                left: 0,
                stop: false,
            }),
            go: Condvar::new(),
            done: Condvar::new(),
        });
        let mut pool = Self {
            shared,
            workers: Vec::new(),
        };
        for _ in 1..n {
            let shared = pool.shared.clone();
            let w = thread::Builder::new()
                .spawn(move || {
                    let mut seen = 0;
                    loop {
                        let mut j = shared.job.lock().unwrap();
                        while j.generation == seen && !j.stop {
                            j = shared.go.wait(j).unwrap()
                        }
                        if j.stop {
                            break;
                        }
                        seen = j.generation;
                        drop(j);
                        pieces(&shared);
                        let mut j = shared.job.lock().unwrap();
                        j.left -= 1;
                        if j.left == 0 {
                            shared.done.notify_one()
                        }
                    }
                })
                .ok()?;
            pool.workers.push(w)
        }
        Some(pool)
    }
    fn rows(&self, j0: i32, j1: i32, f: Band, ctx: *mut c_void) {
        let mut j = self.shared.job.lock().unwrap();
        j.j0 = j0;
        j.n = j1 as i64 - j0 as i64;
        j.pieces = ((self.workers.len() + 1) * 3).min(j.n as usize) as i32;
        j.next = 0;
        j.f = Some(f);
        j.ctx = ctx as usize;
        j.left = self.workers.len();
        j.generation += 1;
        self.shared.go.notify_all();
        drop(j);
        pieces(&self.shared);
        let mut j = self.shared.job.lock().unwrap();
        while j.left > 0 {
            j = self.shared.done.wait(j).unwrap()
        }
        j.f = None;
        j.ctx = 0;
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        {
            let mut j = self.shared.job.lock().unwrap();
            j.stop = true;
            self.shared.go.notify_all()
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
unsafe extern "C" fn rows(a: i32, b: i32, f: Option<Band>, ctx: *mut c_void) {
    if b <= a {
        return;
    }
    if let Some(f) = f {
        if IN_POOL.get() {
            f(ctx, a, b);
            return;
        }
        POOL.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                p.rows(a, b, f, ctx)
            } else {
                f(ctx, a, b)
            }
        })
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_ui_init() -> c_int {
    if !ROOT.get().is_null() {
        return 0;
    }
    if text_init_mem(INTER.as_ptr(), INTER.len()) != 0 {
        return -1;
    }
    build_scene();
    0
}
unsafe fn frame(c: &mut Canvas) {
    for i in 0..ui.cap {
        let n = *ui.nodes.add(i as usize);
        if !n.is_null() {
            node_invalidate(n)
        }
    }
    let root = ROOT.get();
    if layout_root(root, W as f32, H as f32) == 0 {
        return;
    }
    paint_damage(root);
    ui.damage = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: W as f32,
        y1: H as f32,
    };
    canvas_set_clip(
        c,
        Clip {
            x0: 0,
            y0: 0,
            x1: W,
            y1: H,
        },
    );
    canvas_clear(c, 0);
    paint_root(c, root);
    ui.damage = Rect::default();
}
#[no_mangle]
pub unsafe extern "C" fn lb_ui(threads: c_int, secs: f64) -> f64 {
    if !secs.is_finite() || !(0.0..=1800.0).contains(&secs) || lb_ui_init() != 0 {
        return -1.0;
    }
    let mut px = Vec::new();
    if px.try_reserve_exact((W * H) as usize).is_err() {
        return -1.0;
    }
    px.resize((W * H) as usize, 0u32);
    let mut c = std::mem::zeroed();
    canvas_init(&mut c, px.as_mut_ptr(), W, H, W);
    let Some(pool) = Pool::new(threads.clamp(1, 8) as usize) else {
        return -1.0;
    };
    POOL.with(|p| *p.borrow_mut() = Some(pool));
    canvas_parallel(Some(rows));
    frame(&mut c);
    let start = bench_now_ms();
    let mut n = 0;
    let now = loop {
        frame(&mut c);
        n += 1;
        let now = bench_now_ms();
        if now >= start + secs * 1000.0 {
            break now;
        }
    };
    canvas_parallel(None);
    POOL.with(|p| *p.borrow_mut() = None);
    n as f64 * 1000.0 / (now - start)
}
#[no_mangle]
pub unsafe extern "C" fn lb_ui_check(nodes: *mut c_int, unknown: *mut c_int) -> c_int {
    if nodes.is_null() || unknown.is_null() || lb_ui_init() != 0 {
        return -1;
    }
    *nodes = node_count();
    *unknown = UNKNOWN.get();
    0
}

/// Integration-only pixel oracle for comparing serial and parallel painting.
#[cfg(feature = "native-host")]
pub fn lb_ui_checksum(threads: i32) -> Option<u64> {
    unsafe {
        if lb_ui_init() != 0 {
            return None;
        }
        let mut pixels = vec![0u32; (W * H) as usize];
        let mut c = std::mem::zeroed();
        canvas_init(&mut c, pixels.as_mut_ptr(), W, H, W);
        let pool = Pool::new(threads.clamp(1, 8) as usize)?;
        POOL.with(|p| *p.borrow_mut() = Some(pool));
        canvas_parallel(Some(rows));
        frame(&mut c);
        canvas_parallel(None);
        POOL.with(|p| *p.borrow_mut() = None);
        Some(pixels.iter().fold(0xcbf29ce484222325u64, |hash, pixel| {
            pixel
                .to_ne_bytes()
                .iter()
                .fold(hash, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pool_joins_every_generation_before_context_expires() {
        unsafe extern "C" fn mark(p: *mut c_void, a: i32, b: i32) {
            let v = &*(p as *const Vec<std::sync::atomic::AtomicUsize>);
            for i in a..b {
                v[i as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let p = Pool::new(4).unwrap();
        let v: Vec<_> = (0..31)
            .map(|_| std::sync::atomic::AtomicUsize::new(0))
            .collect();
        for _ in 0..500 {
            p.rows(0, 31, mark, &v as *const _ as *mut c_void)
        }
        assert!(v
            .iter()
            .all(|v| v.load(std::sync::atomic::Ordering::Relaxed) == 500));
    }
}
