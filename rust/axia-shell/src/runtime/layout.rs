//! Flex layout with explicit work stacks. No recursion or fixed sibling limit.
//! Scratch allocations are reused, and reserved before changing the scene.
use super::{
    edges::{self, Edges},
    memory,
    scene::Scene,
    types::*,
};
use std::{
    ffi::{c_void, CStr},
    mem::MaybeUninit,
};

pub trait Metrics {
    fn generation(&mut self) -> u32;
    fn text(&mut self, text: &CStr, style: &Style, max_width: f32) -> (f32, f32);
    fn image(&mut self, image: *mut c_void) -> (i32, i32);
}
#[derive(Clone, Copy)]
struct Job {
    id: usize,
    w: f32,
    h: f32,
}
struct Line {
    parent: usize,
    range: std::ops::Range<usize>,
    main: f32,
    cross: f32,
    offset: f32,
    total_cross: f32,
    ix: f32,
    iy: f32,
    ih: f32,
}
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    leaves: usize,
    kids: usize,
    frames: usize,
    growing: usize,
}
#[derive(Clone, Copy)]
struct Kid {
    id: usize,
    main: f32,
    cross: f32,
    outer_cross: f32,
    size_cross: f32,
}
#[derive(Clone, Copy)]
struct Grow {
    id: usize,
    base: f32,
    margin_v: f32,
    width: f32,
}
#[derive(Clone, Copy)]
enum Waiting {
    Child { id: usize, mh: f32, mv: f32 },
    Regrow { mv: f32 },
}
struct Measure {
    id: usize,
    aw: f32,
    ah: f32,
    w: f32,
    h: f32,
    ph: f32,
    pv: f32,
    iaw: f32,
    iah: f32,
    cw: f32,
    ch: f32,
    next: usize,
    count: usize,
    wrap: bool,
    lim: f32,
    line_m: f32,
    line_c: f32,
    tot_m: f32,
    tot_c: f32,
    in_line: usize,
    // Each active frame owns a segment of the shared growth arena. Child
    // frames append after it and truncate their segment when they complete.
    grow_start: usize,
    grow_count: usize,
    fixed: f32,
    grow_sum: f32,
    // First scan children, then remeasure growing row children at their final
    // widths. Waiting records which continuation consumes the child result.
    regrow: bool,
    cursor: usize,
    free: f32,
    waiting: Option<Waiting>,
}
enum Measured {
    Ready((f32, f32)),
    Pending(Measure),
}
pub struct Layout {
    jobs: Vec<Job>,
    kids: Vec<Kid>,
    measures: Vec<Measure>,
    grows: Vec<Grow>,
    text: [MaybeUninit<u8>; 8192],
    generation: Option<(u32, u8)>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            kids: Vec::new(),
            measures: Vec::new(),
            grows: Vec::new(),
            text: [MaybeUninit::uninit(); 8192],
            generation: None,
        }
    }
}
fn res(d: Dim, reference: f32) -> f32 {
    match d.unit {
        1 => d.v,
        2 if !reference.is_nan() => reference * d.v / 100.,
        _ => f32::NAN,
    }
}
fn clamp(mut v: f32, min: Dim, max: Dim, reference: f32) -> f32 {
    let a = res(min, reference);
    let b = res(max, reference);
    if !b.is_nan() && v > b {
        v = b;
    }
    if !a.is_nan() && v < a {
        v = a;
    }
    if v < 0. {
        0.
    } else {
        v
    }
}
fn flow(n: &Node) -> bool {
    n.kind != 3 && n.s.display_none == 0 && n.s.absolute == 0
}
fn intrinsic_container(n: &Node) -> bool {
    n.kind != 1
        && n.kind != 3
        && !(n.kind == 2 && !n.img.is_null())
        && (res(n.s.width, f32::NAN).is_nan() || res(n.s.height, f32::NAN).is_nan())
}
fn prepare(scene: &mut Scene, id: usize, rtl: u8) {
    let parent = scene.parent(id);
    let inherited = if parent == 0 {
        rtl
    } else {
        scene.node(parent).unwrap().rtl
    };
    let n = scene.node_mut(id).unwrap();
    n.rtl = match n.s.dir {
        2 => 1,
        1 => 0,
        _ => inherited,
    };
    if unsafe { n.s.ext.as_ref() }.is_some_and(|x| x.r.ok == 0 || x.r.rtl != n.rtl) {
        edges::resolve(n);
    }
}
fn reserve<T>(v: &mut Vec<T>, required: usize) -> bool {
    v.clear();
    memory::try_reserve(v, required)
}
impl Layout {
    #[cfg(test)]
    pub fn scratch_bytes(&self) -> usize {
        self.jobs.capacity() * std::mem::size_of::<Job>()
            + self.kids.capacity() * std::mem::size_of::<Kid>()
            + self.measures.capacity() * std::mem::size_of::<Measure>()
            + self.grows.capacity() * std::mem::size_of::<Grow>()
    }
    fn bounds(scene: &Scene, root: usize) -> Bounds {
        let (mut leaves, mut growing, mut active, mut max_depth, mut max_kids) = (0, 0, 0, 0, 0);
        let mut id = root;
        'visit: loop {
            let n = scene.node(id).unwrap();
            active += usize::from(intrinsic_container(n));
            max_depth = max_depth.max(active);
            growing += usize::from(n.s.grow > 0.);
            let (mut child, mut children) = (scene.first(id), 0);
            while child != 0 {
                children += 1;
                child = scene.next(child);
            }
            max_kids = max_kids.max(children);
            if scene.first(id) != 0 {
                id = scene.first(id);
                continue;
            }
            leaves += 1;
            loop {
                active -= usize::from(intrinsic_container(scene.node(id).unwrap()));
                if id == root {
                    break 'visit;
                }
                if scene.next(id) != 0 {
                    id = scene.next(id);
                    break;
                }
                id = scene.parent(id);
            }
        }
        Bounds {
            leaves,
            kids: max_kids,
            frames: max_depth,
            growing,
        }
    }
    fn reserve_scene(&mut self, scene: &mut Scene, root: usize) -> bool {
        let bounds = match scene.layout_bounds {
            Some((cached_root, bounds)) if cached_root == root => bounds,
            _ => {
                let bounds = Self::bounds(scene, root);
                scene.layout_bounds = Some((root, bounds));
                bounds
            }
        };
        // Pending DFS jobs cannot exceed distinct leaves. Only intrinsic
        // containers need measurement frames. Scene invalidation clears this
        // shape cache; position/opacity/transform-only animations can reuse it.
        reserve(&mut self.jobs, bounds.leaves)
            && reserve(&mut self.kids, bounds.kids)
            && reserve(&mut self.measures, bounds.frames)
            && reserve(&mut self.grows, bounds.growing)
    }
    pub fn run(
        &mut self,
        scene: &mut Scene,
        ui: &mut Ui,
        root: usize,
        w: f32,
        h: f32,
        metrics: &mut impl Metrics,
    ) -> bool {
        if !self.reserve_scene(scene, root) {
            ui.dirty = 1;
            return false;
        }
        #[cfg(debug_assertions)]
        let capacities = [
            self.jobs.capacity(),
            self.kids.capacity(),
            self.measures.capacity(),
            self.grows.capacity(),
        ];
        let generation = (metrics.generation(), ui.rtl);
        if self.generation != Some(generation) {
            scene.forget_layout();
            self.generation = Some(generation);
        }
        let n = scene.node_mut(root).unwrap();
        n.x = 0.;
        n.y = 0.;
        prepare(scene, root, ui.rtl);
        self.jobs.push(Job { id: root, w, h });
        while let Some(job) = self.jobs.pop() {
            self.layout_node(scene, ui.rtl, job, metrics);
        }
        self.place(scene, root);
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            capacities,
            [
                self.jobs.capacity(),
                self.kids.capacity(),
                self.measures.capacity(),
                self.grows.capacity()
            ],
            "layout exceeded its preflight reservation"
        );
        true
    }
    fn begin_measure(
        &mut self,
        scene: &mut Scene,
        id: usize,
        aw: f32,
        ah: f32,
        metrics: &mut impl Metrics,
    ) -> Measured {
        let n = scene.node(id).unwrap();
        let s = n.s;
        let edges = Edges::of(n);
        let ph = edges.horizontal();
        let pv = edges.vertical();
        let mut w = res(s.width, aw);
        let mut h = res(s.height, ah);
        if !w.is_nan() && !h.is_nan() {
            return Measured::Ready((
                clamp(w, s.min_w, s.max_w, aw),
                clamp(h, s.min_h, s.max_h, ah),
            ));
        }
        let (cw, ch);
        if n.kind == 1 {
            let mut maxw = if !w.is_nan() {
                w - ph
            } else if !aw.is_nan() {
                aw - ph
            } else {
                0.
            };
            let mx = res(s.max_w, aw);
            if !mx.is_nan() && (maxw <= 0. || mx - ph < maxw) {
                maxw = mx - ph;
            }
            if maxw < 0. {
                maxw = 0.;
            }
            if n.measure_valid == 0 || (n.measure_maxw - maxw).abs() > 0.5 {
                let used = scene.gather_text(id, &mut self.text);
                // gather_text initialized exactly this NUL-terminated prefix.
                let text = unsafe {
                    CStr::from_bytes_with_nul_unchecked(std::slice::from_raw_parts(
                        self.text.as_ptr().cast(),
                        used + 1,
                    ))
                };
                let (mw, mh) = metrics.text(text, &s, maxw);
                let n = scene.node_mut(id).unwrap();
                n.measure_w = mw;
                n.measure_h = mh;
                n.measure_maxw = maxw;
                n.measure_valid = 1;
            }
            let n = scene.node(id).unwrap();
            cw = if (s.text_overflow != 0 || s.line_clamp != 0) && maxw > 0. && n.measure_w > maxw {
                maxw
            } else {
                n.measure_w
            };
            ch = n.measure_h;
        } else if n.kind == 2 && !n.img.is_null() {
            let (iw, ih) = metrics.image(n.img);
            let (mut iwf, mut ihf) = (iw as f32, ih as f32);
            if !w.is_nan() && iw != 0 {
                ihf = (w - ph) * ih as f32 / iw as f32;
            } else if !h.is_nan() && ih != 0 {
                iwf = (h - pv) * iw as f32 / ih as f32;
            }
            cw = iwf;
            ch = ihf;
        } else {
            let iaw = if !w.is_nan() {
                w - ph
            } else if !aw.is_nan() {
                aw - ph
            } else {
                f32::NAN
            };
            let iah = if !h.is_nan() {
                h - pv
            } else if !ah.is_nan() {
                ah - pv
            } else {
                f32::NAN
            };
            let lim = if s.row != 0 { iaw } else { iah };
            return Measured::Pending(Measure {
                id,
                aw,
                ah,
                w,
                h,
                ph,
                pv,
                iaw,
                iah,
                cw: 0.,
                ch: 0.,
                next: scene.first(id),
                count: 0,
                wrap: s.wrap != 0 && !lim.is_nan(),
                lim,
                line_m: 0.,
                line_c: 0.,
                tot_m: 0.,
                tot_c: 0.,
                in_line: 0,
                grow_start: self.grows.len(),
                grow_count: 0,
                fixed: 0.,
                grow_sum: 0.,
                regrow: false,
                cursor: 0,
                free: 0.,
                waiting: None,
            });
        }
        if w.is_nan() {
            w = cw + ph;
        }
        if h.is_nan() {
            h = ch + pv;
        }
        Measured::Ready((
            clamp(w, s.min_w, s.max_w, aw),
            clamp(h, s.min_h, s.max_h, ah),
        ))
    }
    fn measure(
        &mut self,
        scene: &mut Scene,
        rtl: u8,
        id: usize,
        aw: f32,
        ah: f32,
        metrics: &mut impl Metrics,
    ) -> (f32, f32) {
        debug_assert!(self.measures.is_empty());
        let mut result = match self.begin_measure(scene, id, aw, ah, metrics) {
            Measured::Ready(value) => return value,
            Measured::Pending(frame) => {
                self.measures.push(frame);
                None
            }
        };
        loop {
            let f = self.measures.last_mut().unwrap();
            let s = scene.node(f.id).unwrap().s;
            if let Some((mut cw, mut ch)) = result.take() {
                match f.waiting.take().unwrap() {
                    Waiting::Regrow { mv } => {
                        if ch + mv > f.ch {
                            f.ch = ch + mv;
                        }
                    }
                    Waiting::Child { id, mh, mv } => {
                        cw += mh;
                        ch += mv;
                        if f.wrap {
                            let (cm, cc) = if s.row != 0 { (cw, ch) } else { (ch, cw) };
                            if f.in_line != 0 && f.line_m + s.gap + cm > f.lim + 0.5 {
                                if f.line_m > f.tot_m {
                                    f.tot_m = f.line_m;
                                }
                                f.tot_c += f.line_c + s.gap;
                                f.line_m = 0.;
                                f.line_c = 0.;
                                f.in_line = 0;
                            }
                            f.line_m += if f.in_line != 0 { s.gap + cm } else { cm };
                            if cc > f.line_c {
                                f.line_c = cc;
                            }
                            f.in_line += 1;
                        } else {
                            if s.row != 0 {
                                f.cw += cw;
                                if ch > f.ch {
                                    f.ch = ch;
                                }
                                let cs = scene.node(id).unwrap().s;
                                if cs.grow > 0. && !f.iaw.is_nan() {
                                    let basis = res(cs.basis, f.iaw);
                                    self.grows.push(Grow {
                                        id,
                                        base: (if basis.is_nan() { cw - mh } else { basis }) + mh,
                                        margin_v: mv,
                                        width: cw - mh,
                                    });
                                    f.grow_count += 1;
                                    f.grow_sum += cs.grow;
                                } else {
                                    f.fixed += cw;
                                }
                            } else {
                                f.ch += ch;
                                if cw > f.cw {
                                    f.cw = cw;
                                }
                            }
                            f.count += 1;
                        }
                    }
                }
            }
            if !f.regrow {
                if f.next != 0 {
                    let child = f.next;
                    f.next = scene.next(child);
                    if !flow(scene.node(child).unwrap()) {
                        continue;
                    }
                    prepare(scene, child, rtl);
                    let mg = Edges::of(scene.node(child).unwrap()).margin;
                    let (mh, mv) = (mg[0] + mg[2], mg[1] + mg[3]);
                    let aw = if f.iaw.is_nan() { f32::NAN } else { f.iaw - mh };
                    let ah = if f.iah.is_nan() { f32::NAN } else { f.iah - mv };
                    f.waiting = Some(Waiting::Child { id: child, mh, mv });
                    match self.begin_measure(scene, child, aw, ah, metrics) {
                        Measured::Ready(value) => result = Some(value),
                        Measured::Pending(frame) => self.measures.push(frame),
                    }
                    continue;
                }
                f.regrow = true;
                f.free = f.iaw
                    - f.fixed
                    - if f.count > 1 {
                        s.gap * (f.count - 1) as f32
                    } else {
                        0.
                    };
                for g in &self.grows[f.grow_start..f.grow_start + f.grow_count] {
                    f.free -= g.base;
                }
            }
            if !f.wrap && s.row != 0 && f.cursor < f.grow_count {
                let g = self.grows[f.grow_start + f.cursor];
                f.cursor += 1;
                let cs = scene.node(g.id).unwrap().s;
                let mg = Edges::of(scene.node(g.id).unwrap()).margin;
                let size = g.base
                    + if f.free > 0. {
                        f.free * cs.grow / f.grow_sum
                    } else {
                        0.
                    };
                let size = clamp(size - (mg[0] + mg[2]), cs.min_w, cs.max_w, f.iaw);
                if g.width <= size + 0.5 {
                    continue;
                }
                let ah = if f.iah.is_nan() {
                    f32::NAN
                } else {
                    f.iah - g.margin_v
                };
                f.waiting = Some(Waiting::Regrow { mv: g.margin_v });
                match self.begin_measure(scene, g.id, size, ah, metrics) {
                    Measured::Ready(value) => result = Some(value),
                    Measured::Pending(frame) => self.measures.push(frame),
                }
                continue;
            }
            let mut f = self.measures.pop().unwrap();
            self.grows.truncate(f.grow_start);
            if f.wrap {
                if f.line_m > f.tot_m {
                    f.tot_m = f.line_m;
                }
                f.tot_c += f.line_c;
                if s.row != 0 {
                    f.cw = f.tot_m;
                    f.ch = f.tot_c;
                } else {
                    f.ch = f.tot_m;
                    f.cw = f.tot_c;
                }
            } else if f.count > 1 {
                if s.row != 0 {
                    f.cw += s.gap * (f.count - 1) as f32;
                } else {
                    f.ch += s.gap * (f.count - 1) as f32;
                }
            }
            if f.w.is_nan() {
                f.w = f.cw + f.ph;
            }
            if f.h.is_nan() {
                f.h = f.ch + f.pv;
            }
            let value = (
                clamp(f.w, s.min_w, s.max_w, f.aw),
                clamp(f.h, s.min_h, s.max_h, f.ah),
            );
            if self.measures.is_empty() {
                return value;
            }
            result = Some(value);
        }
    }
    fn absolute(
        &mut self,
        scene: &mut Scene,
        rtl: u8,
        parent: Job,
        child: usize,
        metrics: &mut impl Metrics,
    ) {
        let Job { id: parent, w, h } = parent;
        prepare(scene, child, rtl);
        let n = scene.node(parent).unwrap();
        let p = Edges::of(n);
        let parent_rtl = n.rtl;
        let c = scene.node(child).unwrap();
        let s = c.s;
        let mg = Edges::of(c).margin;
        let cbw = w - p.border[0] - p.border[2];
        let cbh = h - p.border[1] - p.border[3];
        let mut inset = s.pos;
        if let Some(x) = unsafe { s.ext.as_ref() } {
            if x.inset_inline[0].unit != 0 {
                inset[if c.rtl != 0 { 2 } else { 0 }] = x.inset_inline[0];
            }
            if x.inset_inline[1].unit != 0 {
                inset[if c.rtl != 0 { 0 } else { 2 }] = x.inset_inline[1];
            }
        }
        let (l, t, r, b) = (
            res(inset[0], cbw),
            res(inset[1], cbh),
            res(inset[2], cbw),
            res(inset[3], cbh),
        );
        let mut cw = res(s.width, cbw);
        let mut ch = res(s.height, cbh);
        if cw.is_nan() && !l.is_nan() && !r.is_nan() {
            cw = cbw - l - r;
        }
        if ch.is_nan() && !t.is_nan() && !b.is_nan() {
            ch = cbh - t - b;
        }
        if cw.is_nan() || ch.is_nan() {
            let (mw, mh) = self.measure(
                scene,
                rtl,
                child,
                if cw.is_nan() { cbw } else { cw },
                if ch.is_nan() { cbh } else { ch },
                metrics,
            );
            if cw.is_nan() {
                cw = mw;
            }
            if ch.is_nan() {
                ch = mh;
            }
        }
        cw = clamp(cw, s.min_w, s.max_w, cbw);
        ch = clamp(ch, s.min_h, s.max_h, cbh);
        let sx = if parent_rtl != 0 {
            cbw - p.padding[2] - cw - mg[2] - mg[0]
        } else {
            p.padding[0]
        };
        let c = scene.node_mut(child).unwrap();
        c.x = p.border[0]
            + if !l.is_nan() {
                l
            } else if !r.is_nan() {
                cbw - r - cw
            } else {
                sx
            }
            + mg[0];
        c.y = p.border[1]
            + if !t.is_nan() {
                t
            } else if !b.is_nan() {
                cbh - b - ch
            } else {
                p.padding[1]
            }
            + mg[1];
        self.jobs.push(Job {
            id: child,
            w: cw,
            h: ch,
        });
    }
    fn line(&mut self, scene: &mut Scene, rtl: u8, line: Line, metrics: &mut impl Metrics) {
        let Line {
            parent,
            range,
            main,
            cross: line_cross,
            offset: cross_off,
            total_cross: cross_total,
            ix,
            iy,
            ih,
        } = line;
        let n = scene.node(parent).unwrap();
        let s = n.s;
        let row = s.row != 0;
        let mirror = n.rtl != 0;
        let k = range.len();
        let mut used = if k > 1 { s.gap * (k - 1) as f32 } else { 0. };
        let (mut grow, mut shrink) = (0., 0.);
        for kid in &self.kids[range.clone()] {
            let c = scene.node(kid.id).unwrap();
            let mg = Edges::of(c).margin;
            used += kid.main + if row { mg[0] + mg[2] } else { mg[1] + mg[3] };
            grow += c.s.grow;
            shrink += c.s.shrink * kid.main;
        }
        let mut free = main - used;
        if free > 0. && grow > 0. {
            for kid in &mut self.kids[range.clone()] {
                kid.main += free * scene.node(kid.id).unwrap().s.grow / grow;
            }
            free = 0.;
        } else if free < 0. && shrink > 0. {
            for kid in &mut self.kids[range.clone()] {
                kid.main += free * scene.node(kid.id).unwrap().s.shrink * kid.main / shrink;
                if kid.main < 0. {
                    kid.main = 0.;
                }
            }
            free = 0.;
        }
        if free != 0. || grow > 0. {
            let mut total = if k > 1 { s.gap * (k - 1) as f32 } else { 0. };
            for kid in &mut self.kids[range.clone()] {
                let c = scene.node(kid.id).unwrap();
                let cs = c.s;
                let mg = Edges::of(c).margin;
                kid.main = clamp(
                    kid.main,
                    if row { cs.min_w } else { cs.min_h },
                    if row { cs.max_w } else { cs.max_h },
                    main,
                );
                total += kid.main + if row { mg[0] + mg[2] } else { mg[1] + mg[3] };
            }
            free = main - total;
        }
        for i in range.clone() {
            let kid = self.kids[i];
            let c = scene.node(kid.id).unwrap();
            let cs = c.s;
            let mg = Edges::of(c).margin;
            let mc = if row { mg[1] + mg[3] } else { mg[0] + mg[2] };
            let align = if cs.align_self != 0 {
                cs.align_self
            } else {
                s.align_items
            };
            let ec = res(if row { cs.height } else { cs.width }, line_cross);
            let sz = if !ec.is_nan() {
                ec
            } else if align == 1 {
                line_cross - mc
            } else if row {
                self.measure(scene, rtl, kid.id, kid.main, ih - mc, metrics)
                    .1
            } else {
                kid.cross
            };
            self.kids[i].size_cross = clamp(
                sz,
                if row { cs.min_h } else { cs.min_w },
                if row { cs.max_h } else { cs.max_w },
                line_cross,
            );
        }
        let (mut pos, mut between) = (0., s.gap);
        if (free > 0. || s.justify == 1 || s.justify == 2) && k > 0 {
            match s.justify {
                1 => pos = free / 2.,
                2 => pos = free,
                3 if k > 1 => between += free / (k - 1) as f32,
                4 => {
                    pos = free / k as f32 / 2.;
                    between += free / k as f32;
                }
                5 => {
                    pos = free / (k + 1) as f32;
                    between += free / (k + 1) as f32;
                }
                _ => {}
            }
        }
        for kid in &self.kids[range] {
            let c = scene.node(kid.id).unwrap();
            let mg = Edges::of(c).margin;
            let (ml, mr) = if mirror {
                (mg[2], mg[0])
            } else {
                (mg[0], mg[2])
            };
            let (ms, me, cs, ce) = if row {
                (ml, mr, mg[1], mg[3])
            } else {
                (mg[1], mg[3], ml, mr)
            };
            let align = if c.s.align_self != 0 {
                c.s.align_self
            } else {
                s.align_items
            };
            let mut cp = match align {
                3 => (line_cross - kid.size_cross - cs - ce) / 2. + cs,
                4 => line_cross - kid.size_cross - ce,
                _ => cs,
            };
            cp += cross_off;
            pos += ms;
            let c = scene.node_mut(kid.id).unwrap();
            let (w, h) = if row {
                c.x = if mirror {
                    ix + main - pos - kid.main
                } else {
                    ix + pos
                };
                c.y = iy + cp;
                (kid.main, kid.size_cross)
            } else {
                c.x = if mirror {
                    ix + cross_total - cp - kid.size_cross
                } else {
                    ix + cp
                };
                c.y = iy + pos;
                (kid.size_cross, kid.main)
            };
            self.jobs.push(Job { id: kid.id, w, h });
            pos += kid.main + me + between;
        }
    }
    fn layout_node(&mut self, scene: &mut Scene, rtl: u8, job: Job, metrics: &mut impl Metrics) {
        let Job { id, w, h } = job;
        let n = scene.node_mut(id).unwrap();
        if n.layout_ok != 0 && n.w == w && n.h == h && n.layout_rtl == n.rtl {
            return;
        }
        n.w = w;
        n.h = h;
        n.layout_ok = 1;
        n.layout_rtl = n.rtl;
        if n.kind == 3 || n.kind == 1 {
            return;
        }
        let s = n.s;
        let row = s.row != 0;
        let p = Edges::of(n);
        let ix = p.padding[0] + p.border[0];
        let iy = p.padding[1] + p.border[1];
        let mut iw = w - p.horizontal();
        let mut ih = h - p.vertical();
        if iw < 0. {
            iw = 0.;
        }
        if ih < 0. {
            ih = 0.;
        }
        let (main, cross) = if row { (iw, ih) } else { (ih, iw) };
        self.kids.clear();
        let mut child = scene.first(id);
        while child != 0 {
            let c = child;
            child = scene.next(child);
            if !flow(scene.node(c).unwrap()) {
                continue;
            }
            prepare(scene, c, rtl);
            let n = scene.node(c).unwrap();
            let cs = n.s;
            let mg = Edges::of(n).margin;
            let mc = if row { mg[1] + mg[3] } else { mg[0] + mg[2] };
            let basis = res(cs.basis, main);
            let em = res(if row { cs.width } else { cs.height }, main);
            let (mw, mh) = if (basis.is_nan() && em.is_nan())
                || res(if row { cs.height } else { cs.width }, cross).is_nan()
                || s.wrap != 0
            {
                self.measure(
                    scene,
                    rtl,
                    c,
                    if row { iw } else { iw - mc },
                    if row { ih - mc } else { ih },
                    metrics,
                )
            } else {
                (0., 0.)
            };
            let base = if !basis.is_nan() {
                basis
            } else if !em.is_nan() {
                em
            } else if row {
                mw
            } else {
                mh
            };
            let measured = if row { mh } else { mw };
            let ec = res(if row { cs.height } else { cs.width }, cross);
            self.kids.push(Kid {
                id: c,
                main: base,
                cross: measured,
                outer_cross: if !ec.is_nan() { ec + mc } else { measured + mc },
                size_cross: 0.,
            });
        }
        let job_start = self.jobs.len();
        let k = self.kids.len();
        if s.wrap == 0 {
            self.line(
                scene,
                rtl,
                Line {
                    parent: id,
                    range: 0..k,
                    main,
                    cross,
                    offset: 0.,
                    total_cross: cross,
                    ix,
                    iy,
                    ih,
                },
                metrics,
            );
        } else {
            let (mut start, mut off) = (0, 0.);
            while start < k {
                let (mut end, mut len, mut lc) = (start, 0., 0.);
                while end < k {
                    let kid = self.kids[end];
                    let mg = Edges::of(scene.node(kid.id).unwrap()).margin;
                    let mm = if row { mg[0] + mg[2] } else { mg[1] + mg[3] };
                    let add = if end > start { s.gap } else { 0. } + kid.main + mm;
                    if end > start && len + add > main + 0.5 {
                        break;
                    }
                    len += add;
                    if kid.outer_cross > lc {
                        lc = kid.outer_cross;
                    }
                    end += 1;
                }
                self.line(
                    scene,
                    rtl,
                    Line {
                        parent: id,
                        range: start..end,
                        main,
                        cross: lc,
                        offset: off,
                        total_cross: cross,
                        ix,
                        iy,
                        ih,
                    },
                    metrics,
                );
                off += lc + s.gap;
                start = end;
            }
        }
        let mut child = scene.first(id);
        while child != 0 {
            let c = child;
            child = scene.next(child);
            let n = scene.node(c).unwrap();
            if n.kind != 3 && n.s.display_none == 0 && n.s.absolute != 0 {
                self.absolute(scene, rtl, job, c, metrics);
            }
        }
        // LIFO jobs still visit siblings in the original paint-independent tree order.
        self.jobs[job_start..].reverse();
    }
    fn place(&mut self, scene: &mut Scene, root: usize) {
        let mut id = root;
        while id != 0 {
            let parent = if id == root { 0 } else { scene.parent(id) };
            let (px, py, world) = if parent == 0 {
                (0., 0., None)
            } else {
                let p = scene.node(parent).unwrap();
                (
                    p.ax,
                    p.ay,
                    if p.has_tf != 0 {
                        Some(unsafe { (*p.tf).world })
                    } else {
                        None
                    },
                )
            };
            let n = scene.node_mut(id).unwrap();
            n.ax = px + n.x + n.s.translate_x;
            n.ay = py + n.y + n.s.translate_y;
            n.own_tf = u8::from(n.kind != 3 && n.s.has_transform());
            let mut transformed = n.own_tf != 0 || world.is_some();
            if transformed {
                if n.tf.is_null() {
                    if let Some(tf) = memory::try_box(NodeTf::default()) {
                        n.tf = Box::into_raw(tf);
                    } else {
                        n.own_tf = 0;
                        transformed = false;
                    }
                }
                if transformed {
                    let tf = unsafe { &mut *n.tf };
                    if n.own_tf != 0 {
                        tf.own = n.s.transform(n.ax, n.ay, n.w, n.h);
                        tf.world = world.map_or(tf.own, |m| m.compose(tf.own));
                    } else {
                        tf.world = world.unwrap();
                    }
                    tf.inv = tf.world.inverse().unwrap_or(Affine {
                        e: -1e9,
                        f: -1e9,
                        ..Affine::default()
                    });
                }
            }
            n.has_tf = u8::from(transformed);
            id = scene.next_in_subtree(root, id, true);
        }
    }
}

#[cfg(not(test))]
pub struct NativeMetrics;
#[cfg(not(test))]
impl Metrics for NativeMetrics {
    fn generation(&mut self) -> u32 {
        unsafe {
            extern "C" {
                fn text_generation() -> u32;
            }
            text_generation()
        }
    }
    fn text(&mut self, text: &CStr, s: &Style, max_width: f32) -> (f32, f32) {
        unsafe {
            extern "C" {
                fn text_measure_ex(
                    text: *const std::ffi::c_char,
                    weight: f32,
                    size: f32,
                    line_h: f32,
                    max_w: f32,
                    nowrap: i32,
                    max_lines: i32,
                    w: *mut f32,
                    h: *mut f32,
                );
            }
            let (mut w, mut h) = (0., 0.);
            let weight = s.font_weight
                + match s.mono {
                    2 => 2000.,
                    0 => 0.,
                    _ => 1000.,
                };
            text_measure_ex(
                text.as_ptr(),
                weight,
                s.font_size,
                s.line_height,
                max_width,
                s.nowrap as i32,
                s.line_clamp as i32,
                &mut w,
                &mut h,
            );
            (w, h)
        }
    }
    fn image(&mut self, image: *mut c_void) -> (i32, i32) {
        unsafe {
            extern "C" {
                fn image_size(image: *mut c_void, w: *mut i32, h: *mut i32);
            }
            let (mut w, mut h) = (0, 0);
            image_size(image, &mut w, &mut h);
            (w, h)
        }
    }
}
