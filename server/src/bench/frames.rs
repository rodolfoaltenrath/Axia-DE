//! Bounded compositor measurements. Timestamps and percentile definitions match
//! the existing reports; recording-off hooks perform only relaxed atomic reads.
use libc::{c_char, c_int, c_long};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, MutexGuard, OnceLock,
};

const MAXF: usize = 8192;
const MAXI: usize = 16384;
const BATCH: usize = 64;
pub const LBF_GDRAG_R: usize = 8;
pub const LBF_GDRAG_L: usize = 17;

#[derive(Default, Debug)]
#[repr(C)]
pub struct LbFrameStats {
    pub frames: c_int,
    pub shown: c_int,
    pub span_s: f64,
    pub fps: f64,
    pub work_p50: f64,
    pub work_p99: f64,
    pub work_max: f64,
    pub budget_ms: f64,
    pub missed_pct: f64,
    pub gap_max: f64,
    pub inputs: c_int,
    pub in_p50: f64,
    pub in_p99: f64,
}
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct LbPhase {
    pub prev_flip: u64,
    pub wake: u64,
    pub from_flip: c_int,
    pub render0: u64,
    pub built: u64,
    pub paint_ms: f64,
    pub up_ms: f64,
    pub gdrag_ms: f64,
    pub swap_ms: f64,
    pub submit_ms: f64,
    pub up_px: c_long,
    pub gdrag_px: c_long,
    pub gdrag_full: c_int,
    pub feed_ms: f64,
    pub feed_px: c_long,
    pub submit: u64,
    pub period: u32,
    pub par: c_int,
    pub gdrag: c_int,
    pub gdrag_why: c_int,
    pub gdrag_sub: c_int,
    pub unshown: c_int,
    pub piped: c_int,
    pub sfrom_flip: c_int,
    pub pre_end: u64,
    pub swake: u64,
    pub sub0: u64,
    pub sub1: u64,
}
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct LbPct {
    pub p50: f64,
    pub p99: f64,
}
#[derive(Clone, Default, Debug)]
#[repr(C)]
pub struct LbPhaseStats {
    pub n: c_int,
    pub wake: LbPct,
    pub pre: LbPct,
    pub paint: LbPct,
    pub up: LbPct,
    pub gdragup: LbPct,
    pub swap: LbPct,
    pub submit_call: LbPct,
    pub build: LbPct,
    pub gpu: LbPct,
    pub wait: LbPct,
    pub chain: LbPct,
    pub feed: LbPct,
    pub held: LbPct,
    pub up_mpx: f64,
    pub gdrag_mpx: f64,
    pub feed_mpx: f64,
    pub on_time: c_int,
    pub cpu_late: c_int,
    pub gpu_late: c_int,
    pub other_late: c_int,
    pub gpu_known: c_int,
    pub par: c_int,
    pub gdrag_tried: c_int,
    pub gdrag_taken: c_int,
    pub gdrag_refused: [c_int; LBF_GDRAG_R],
    pub gdrag_layer: [c_int; LBF_GDRAG_L],
    pub gdrag_full: c_int,
    pub gdrag_full_px: c_long,
    pub unshown: c_int,
    pub piped: c_int,
}

fn sort(v: &mut [f64]) {
    v.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.;
    }
    let k = (p / 100. * v.len() as f64 + 0.999999) as usize;
    v[k.saturating_sub(1).min(v.len() - 1)]
}
fn append_events(to: &mut Vec<u64>, from: impl IntoIterator<Item = u64>) {
    for event in from {
        if to.len() == BATCH {
            to.remove(0);
        }
        to.push(event);
    }
}
#[derive(Default)]
struct Frames {
    work: Vec<f64>,
    shown: Vec<u64>,
    lat: Vec<f64>,
    period: u32,
    applied: Vec<u64>,
    painted: Vec<u64>,
    inframe: Vec<u64>,
}
impl Frames {
    fn clear(&mut self) {
        self.work.clear();
        self.shown.clear();
        self.lat.clear();
        self.period = 0;
        self.applied.clear();
        self.painted.clear();
        self.inframe.clear();
        self.work.reserve(MAXF);
        self.shown.reserve(MAXF);
        self.lat.reserve(MAXI);
        self.applied.reserve(BATCH);
        self.painted.reserve(BATCH);
        self.inframe.reserve(BATCH);
    }
    fn frame(&mut self, work: f64) {
        if self.work.len() < MAXF && work >= 0. {
            self.work.push(work);
        }
    }
    fn presented(&mut self, ns: u64, period: u32) {
        if self.shown.len() < MAXF && self.shown.last().map_or(true, |v| ns > *v) {
            self.shown.push(ns);
        }
        if period != 0 {
            self.period = period;
        }
        for ev in self.inframe.drain(..) {
            if self.lat.len() < MAXI && ns >= ev {
                self.lat.push((ns - ev) as f64 / 1e6);
            }
        }
    }
    fn painted(&mut self) {
        append_events(&mut self.painted, self.applied.drain(..));
    }
    fn submitted(&mut self) {
        append_events(&mut self.inframe, self.painted.drain(..));
    }
    fn built(&mut self) {
        self.submitted();
        append_events(&mut self.inframe, self.applied.drain(..));
    }
    fn stats(&self) -> LbFrameStats {
        let mut st = LbFrameStats {
            frames: self.work.len() as c_int,
            shown: self.shown.len() as c_int,
            budget_ms: if self.period == 0 {
                16666667
            } else {
                self.period
            } as f64
                / 1e6,
            inputs: self.lat.len() as c_int,
            ..Default::default()
        };
        if self.shown.len() >= 2 {
            let mut missed = 0;
            for pair in self.shown.windows(2) {
                let gap = (pair[1] - pair[0]) as f64 / 1e6;
                if gap > st.budget_ms * 1.5 {
                    missed += 1;
                }
                st.gap_max = st.gap_max.max(gap);
            }
            st.span_s = (self.shown[self.shown.len() - 1] - self.shown[0]) as f64 / 1e9;
            st.fps = (self.shown.len() - 1) as f64 / st.span_s;
            st.missed_pct = 100. * missed as f64 / (self.shown.len() - 1) as f64;
        }
        let mut work = self.work.clone();
        sort(&mut work);
        st.work_p50 = pct(&work, 50.);
        st.work_p99 = pct(&work, 99.);
        st.work_max = work.last().copied().unwrap_or(0.);
        let mut lat = self.lat.clone();
        sort(&mut lat);
        st.in_p50 = pct(&lat, 50.);
        st.in_p99 = pct(&lat, 99.);
        st
    }
}

const WAKE: usize = 0;
const PRE: usize = 1;
const PAINT: usize = 2;
const UP: usize = 3;
const GDRAG_UP: usize = 4;
const SWAP: usize = 5;
const SUBC: usize = 6;
const BUILD: usize = 7;
const GPU: usize = 8;
const WAIT: usize = 9;
const CHAIN: usize = 10;
const FEED: usize = 11;
const HELD: usize = 12;
const PH_N: usize = 13;
#[derive(Default)]
struct Rec {
    v: [Vec<f32>; PH_N],
    st: LbPhaseStats,
    built: c_int,
    up_px: f64,
    gdrag_px: f64,
    feed_px: f64,
    full_px: f64,
}
fn between(a: u64, b: u64) -> f64 {
    if a != 0 && b != 0 && b >= a {
        (b - a) as f64 / 1e6
    } else {
        -1.
    }
}
impl Rec {
    fn clear(&mut self) {
        for v in &mut self.v {
            v.clear();
            v.reserve(MAXF);
        }
        self.st = LbPhaseStats::default();
        self.built = 0;
        self.up_px = 0.;
        self.gdrag_px = 0.;
        self.feed_px = 0.;
        self.full_px = 0.;
    }
    fn put(&mut self, ph: usize, ms: f64) {
        if ms >= 0. && self.v[ph].len() < MAXF {
            self.v[ph].push(ms as f32);
        }
    }
    fn built(&mut self, p: &LbPhase) {
        self.built += 1;
        self.st.par += (p.par != 0) as c_int;
        self.st.unshown += (p.unshown != 0) as c_int;
        self.st.piped += (p.piped != 0) as c_int;
        if p.gdrag >= 0 {
            self.st.gdrag_tried += 1;
            if p.gdrag > 0 {
                self.st.gdrag_taken += 1;
            } else if let Some(count) = self.st.gdrag_refused.get_mut(p.gdrag_why as usize) {
                *count += 1;
                if let Some(count) = self.st.gdrag_layer.get_mut(p.gdrag_sub as usize) {
                    *count += 1;
                }
            }
        }
        if p.gdrag_full != 0 {
            self.st.gdrag_full += 1;
            self.full_px += p.gdrag_px as f64;
        }
        self.up_px += p.up_px as f64;
        self.gdrag_px += p.gdrag_px as f64;
        self.feed_px += p.feed_px as f64;
        self.put(FEED, p.feed_ms);
    }
    fn shown(&mut self, p: &LbPhase, flip: u64, gpu: u64) {
        self.st.n += 1;
        let piped = p.piped != 0;
        let start = if piped { p.swake } else { p.wake };
        if (if piped { p.sfrom_flip } else { p.from_flip }) != 0 {
            self.put(WAKE, between(p.prev_flip, start));
        }
        self.put(
            PRE,
            between(p.wake, if piped { p.pre_end } else { p.render0 }),
        );
        self.put(PAINT, p.paint_ms);
        self.put(UP, p.up_ms);
        if p.gdrag > 0 {
            self.put(GDRAG_UP, p.gdrag_ms);
        }
        self.put(SWAP, p.swap_ms);
        self.put(SUBC, p.submit_ms);
        let mut build = between(p.render0, p.built);
        if piped {
            let sub = between(p.sub0, p.sub1);
            build = if build >= 0. && sub >= 0. {
                build + sub
            } else {
                -1.
            };
            self.put(HELD, between(p.built, p.sub0));
        }
        self.put(BUILD, build);
        let mut ready = p.submit;
        if gpu != 0 {
            self.st.gpu_known += 1;
            self.put(GPU, gpu.saturating_sub(p.submit) as f64 / 1e6);
            ready = ready.max(gpu);
        }
        self.put(WAIT, between(ready, flip));
        self.put(CHAIN, between(p.wake, flip));
        if p.prev_flip == 0 || p.period == 0 || start == 0 || start < p.prev_flip {
            self.st.other_late += 1;
            return;
        }
        let k = (start - p.prev_flip) / p.period as u64 + 1;
        let target = p.prev_flip.wrapping_add(k.wrapping_mul(p.period as u64));
        if flip <= target.wrapping_add(p.period as u64 / 2) {
            self.st.on_time += 1;
        } else if p.submit > target {
            self.st.cpu_late += 1;
        } else if gpu > target {
            self.st.gpu_late += 1;
        } else {
            self.st.other_late += 1;
        }
    }
    fn percentile(&self, ph: usize) -> LbPct {
        let mut v: Vec<f64> = self.v[ph].iter().map(|v| *v as f64).collect();
        sort(&mut v);
        LbPct {
            p50: pct(&v, 50.),
            p99: pct(&v, 99.),
        }
    }
    fn stats(&self) -> LbPhaseStats {
        // Counters have the same C representation as the exported snapshot.
        let mut st = LbPhaseStats {
            wake: self.percentile(WAKE),
            pre: self.percentile(PRE),
            paint: self.percentile(PAINT),
            up: self.percentile(UP),
            gdragup: self.percentile(GDRAG_UP),
            swap: self.percentile(SWAP),
            submit_call: self.percentile(SUBC),
            build: self.percentile(BUILD),
            gpu: self.percentile(GPU),
            wait: self.percentile(WAIT),
            chain: self.percentile(CHAIN),
            feed: self.percentile(FEED),
            held: self.percentile(HELD),
            ..self.st.clone()
        };
        let divisor = self.built.max(1) as f64 * 1e6;
        st.up_mpx = self.up_px / divisor;
        st.gdrag_mpx = self.gdrag_px / divisor;
        st.feed_mpx = self.feed_px / divisor;
        st.gdrag_full_px = self.full_px as c_long;
        st
    }
}

#[derive(Default)]
struct State {
    frames: Frames,
    lb: Rec,
    drag: Rec,
    pending: Option<LbPhase>,
}
static ON: AtomicBool = AtomicBool::new(false);
static LB_ON: AtomicBool = AtomicBool::new(false);
static DRAG_ON: AtomicBool = AtomicBool::new(false);
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn lock() -> MutexGuard<'static, State> {
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
fn recording() -> bool {
    LB_ON.load(Ordering::Relaxed) || DRAG_ON.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn lbf_recording() -> c_int {
    recording() as c_int
}
#[no_mangle]
pub extern "C" fn lbf_begin() {
    let mut s = lock();
    s.frames.clear();
    s.lb.clear();
    ON.store(true, Ordering::SeqCst);
    LB_ON.store(true, Ordering::SeqCst);
}
#[no_mangle]
pub unsafe extern "C" fn lbf_end(st: *mut LbFrameStats) -> c_int {
    let Some(st) = st.as_mut() else {
        return -1;
    };
    let s = lock();
    ON.store(false, Ordering::SeqCst);
    LB_ON.store(false, Ordering::SeqCst);
    *st = s.frames.stats();
    if st.shown >= 2 {
        0
    } else {
        -1
    }
}
#[no_mangle]
pub extern "C" fn lbf_frame(ms: f64) {
    if ON.load(Ordering::Relaxed) {
        lock().frames.frame(ms);
    }
}
#[no_mangle]
pub extern "C" fn lbf_presented(ns: u64, period: u32) {
    if ON.load(Ordering::Relaxed) {
        lock().frames.presented(ns, period);
    }
}
#[no_mangle]
pub extern "C" fn lbf_input(ns: u64) {
    if ON.load(Ordering::Relaxed) {
        append_events(&mut lock().frames.applied, [ns]);
    }
}
#[no_mangle]
pub extern "C" fn lbf_built(main_drawn: c_int) {
    if main_drawn != 0 && ON.load(Ordering::Relaxed) {
        lock().frames.built();
    }
}
#[no_mangle]
pub extern "C" fn lbf_painted() {
    if ON.load(Ordering::Relaxed) {
        lock().frames.painted();
    }
}
#[no_mangle]
pub extern "C" fn lbf_submitted() {
    if ON.load(Ordering::Relaxed) {
        lock().frames.submitted();
    }
}
#[no_mangle]
pub unsafe extern "C" fn lbf_phase_built(p: *const LbPhase) {
    if !recording() {
        return;
    }
    let Some(p) = p.as_ref() else {
        return;
    };
    let mut s = lock();
    if LB_ON.load(Ordering::Relaxed) {
        s.lb.built(p);
    }
    if DRAG_ON.load(Ordering::Relaxed) {
        s.drag.built(p);
    }
    s.pending = if p.submit != 0 { Some(*p) } else { None };
}
#[no_mangle]
pub extern "C" fn lbf_phase_shown(flip: u64, gpu: u64) {
    if !recording() {
        return;
    }
    let mut s = lock();
    if let Some(p) = s.pending.take().filter(|p| flip >= p.submit) {
        if LB_ON.load(Ordering::Relaxed) {
            s.lb.shown(&p, flip, gpu);
        }
        if DRAG_ON.load(Ordering::Relaxed) {
            s.drag.shown(&p, flip, gpu);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn lbf_phases(st: *mut LbPhaseStats) {
    let Some(st) = st.as_mut() else {
        return;
    };
    let s = lock();
    LB_ON.store(false, Ordering::SeqCst);
    *st = s.lb.stats();
}
#[no_mangle]
pub extern "C" fn lbf_drag_begin() {
    let mut s = lock();
    s.drag.clear();
    DRAG_ON.store(true, Ordering::SeqCst);
}
#[no_mangle]
pub unsafe extern "C" fn lbf_drag_end(st: *mut LbPhaseStats) {
    let Some(st) = st.as_mut() else {
        return;
    };
    let s = lock();
    DRAG_ON.store(false, Ordering::SeqCst);
    *st = s.drag.stats();
}
const REASONS: [&[u8]; 7] = [
    b"layer\0",
    b"clip\0",
    b"ancestor\0",
    b"app\0",
    b"after\0",
    b"post\0",
    b"feed\0",
];
const LAYERS: [&[u8]; 16] = [
    b"gone\0",
    b"invalid\0",
    b"resize\0",
    b"rendering\0",
    b"capture\0",
    b"view\0",
    b"transform\0",
    b"raw\0",
    b"holes\0",
    b"surfaces\0",
    b"premul\0",
    b"nobuf\0",
    b"tree\0",
    b"opacity\0",
    b"part\0",
    b"size\0",
];
#[no_mangle]
pub extern "C" fn lbf_gdrag_r_name(i: c_int) -> *const c_char {
    REASONS
        .get(i as usize)
        .map_or(std::ptr::null(), |s| s.as_ptr().cast())
}
#[no_mangle]
pub extern "C" fn lbf_gdrag_l_name(i: c_int) -> *const c_char {
    LAYERS
        .get(i as usize)
        .map_or(std::ptr::null(), |s| s.as_ptr().cast())
}
#[no_mangle]
pub unsafe extern "C" fn lbf_phases_text(st: *const LbPhaseStats, buf: *mut c_char, n: usize) {
    if n == 0 || buf.is_null() {
        return;
    }
    let Some(st) = st.as_ref() else {
        *buf = 0;
        return;
    };
    let text = st.text();
    let len = text.len().min(n - 1);
    std::ptr::copy_nonoverlapping(text.as_ptr(), buf.cast(), len);
    *buf.add(len) = 0;
}
impl LbPhaseStats {
    fn text(&self) -> String {
        use std::fmt::Write;
        let st = self;
        let mut s = format!(concat!(
            "phases, {} frames (ms p50/p99): wake {:.2}/{:.2}, before render {:.2}/{:.2}, render {:.2}/{:.2} (paint {:.2}/{:.2}, ",
            "upload {:.2}/{:.2}, GPU drag upload {:.2}/{:.2}, swap {:.2}/{:.2}, flip ioctl {:.2}/{:.2}), GPU after the flip was queued ",
            "{:.2}/{:.2}, to the vblank {:.2}/{:.2}, wake to flip {:.2}/{:.2}, GPU drag catch-up {:.2}/{:.2}; per frame upload {:.3} Mpx, ",
            "GPU drag {:.3}, catch-up {:.3}; on time {}, CPU-late {}, GPU-late {}, other {} (GPU time known for {}); paint threads ",
            "on {}; GPU drag {} of {} ("),
            st.n, st.wake.p50, st.wake.p99, st.pre.p50, st.pre.p99, st.build.p50, st.build.p99,
            st.paint.p50, st.paint.p99, st.up.p50, st.up.p99, st.gdragup.p50, st.gdragup.p99,
            st.swap.p50, st.swap.p99, st.submit_call.p50, st.submit_call.p99, st.gpu.p50, st.gpu.p99,
            st.wait.p50, st.wait.p99, st.chain.p50, st.chain.p99, st.feed.p50, st.feed.p99,
            st.up_mpx, st.gdrag_mpx, st.feed_mpx, st.on_time, st.cpu_late, st.gpu_late, st.other_late,
            st.gpu_known, st.par, st.gdrag_taken, st.gdrag_tried);
        let mut first = true;
        for (i, name) in REASONS.iter().enumerate() {
            if st.gdrag_refused[i] == 0 {
                continue;
            }
            let name = std::str::from_utf8(&name[..name.len() - 1]).unwrap();
            write!(
                s,
                "{}not {} {}",
                if first { "" } else { ", " },
                name,
                st.gdrag_refused[i]
            )
            .unwrap();
            first = false;
            if i == 0 {
                let mut first_layer = true;
                for (j, name) in LAYERS.iter().enumerate() {
                    if st.gdrag_layer[j] == 0 {
                        continue;
                    }
                    let name = std::str::from_utf8(&name[..name.len() - 1]).unwrap();
                    write!(
                        s,
                        "{}{} {}",
                        if first_layer { " [" } else { ", " },
                        name,
                        st.gdrag_layer[j]
                    )
                    .unwrap();
                    first_layer = false;
                }
                if !first_layer {
                    s.push(']');
                }
            }
        }
        write!(s,"), whole GPU drag uploads {} ({:.2} Mpx), unshown {}; prepared ahead {} (held for the flip {:.2}/{:.2})",
            st.gdrag_full,st.gdrag_full_px as f64 / 1e6,st.unshown,st.piped,st.held.p50,st.held.p99).unwrap();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_statistics_keep_refresh_budget_and_bounded_samples() {
        let mut f = Frames::default();
        f.clear();
        let mut t = 1_000_000_000;
        for i in 0..200 {
            f.frame(if i == 99 { 40. } else { 2. + (i % 3) as f64 });
            t += if i == 99 { 50_000_001 } else { 16_666_667 };
            f.presented(t, 16_666_667);
        }
        let st = f.stats();
        assert_eq!(
            (st.frames, st.shown, st.work_p50, st.work_p99, st.work_max),
            (200, 200, 3., 4., 40.)
        );
        assert!((st.missed_pct - 100. / 199.).abs() < 1e-8);
        f.presented(t, 0);
        f.presented(t - 1, 0);
        assert_eq!(f.shown.len(), 200);
        for i in 0..MAXF + 100 {
            f.frame(1.);
            f.presented(t + 1 + i as u64, 0);
        }
        assert_eq!((f.work.len(), f.shown.len()), (MAXF, MAXF));
        f.clear();
        assert_eq!(f.stats().budget_ms, 16.666667);
        assert_eq!(f.stats().shown, 0);
    }
    #[test]
    fn prepared_frames_keep_events_until_their_own_presentation() {
        let mut f = Frames::default();
        f.presented(10_000_000, 16_666_667);
        append_events(&mut f.applied, [11_000_000]);
        f.painted();
        append_events(&mut f.applied, [12_000_000]);
        f.presented(26_666_667, 16_666_667);
        assert!(f.lat.is_empty());
        f.submitted();
        append_events(&mut f.applied, [27_000_000]);
        f.painted();
        f.presented(43_333_334, 16_666_667);
        f.submitted();
        f.presented(60_000_000, 16_666_667);
        let st = f.stats();
        assert_eq!((st.inputs, st.in_p50, st.in_p99), (3, 33., 48.));
        f.clear();
        append_events(&mut f.applied, 0..200);
        f.painted();
        append_events(&mut f.applied, [500]);
        f.built();
        f.presented(1000, 0);
        assert_eq!(f.lat.len(), 64);
        assert!((f.stats().in_p99 - (1000. - 137.) / 1e6).abs() < 1e-12);
        f.clear();
        append_events(&mut f.applied, [9000]);
        f.built();
        f.presented(5000, 0);
        assert!(f.lat.is_empty());
    }
    fn phase() -> LbPhase {
        LbPhase {
            prev_flip: 1_000_000_000,
            wake: 1_001_000_000,
            from_flip: 1,
            render0: 1_002_000_000,
            built: 1_006_000_000,
            submit: 1_010_000_000,
            period: 16_666_667,
            paint_ms: 3.,
            up_ms: 1.,
            swap_ms: 0.2,
            submit_ms: 0.1,
            gdrag: -1,
            ..Default::default()
        }
    }
    #[test]
    fn phase_attribution_distinguishes_cpu_gpu_and_unknown_delays() {
        let p = phase();
        let mut r = Rec::default();
        r.clear();
        r.built(&p);
        r.shown(&p, 1_016_666_667, 1_012_000_000);
        let st = r.stats();
        assert_eq!((st.n, st.on_time, st.gpu_known), (1, 1, 1));
        assert_eq!(
            (st.wake.p50, st.pre.p50, st.build.p50, st.gpu.p50),
            (1., 1., 4., 2.)
        );
        let mut late = p;
        late.submit = 1_020_000_000;
        r.built(&late);
        r.shown(&late, 1_033_333_334, 1_022_000_000);
        r.built(&p);
        r.shown(&p, 1_033_333_334, 1_020_000_000);
        r.built(&p);
        r.shown(&p, 1_033_333_334, 0);
        let st = r.stats();
        assert_eq!(
            (st.n, st.on_time, st.cpu_late, st.gpu_late, st.other_late),
            (4, 1, 1, 1, 1)
        );
    }
    #[test]
    fn piped_phase_separates_preparation_hold_and_submission() {
        let mut p = phase();
        p.piped = 1;
        p.swake = 1_017_000_000;
        p.sfrom_flip = 1;
        p.pre_end = p.render0;
        p.sub0 = 1_018_000_000;
        p.sub1 = 1_021_000_000;
        p.submit = p.sub1;
        p.prev_flip = 1_016_666_667;
        p.gdrag = 0;
        p.gdrag_why = 0;
        p.gdrag_sub = 14;
        p.gdrag_px = 2_000_000;
        p.gdrag_full = 1;
        let mut r = Rec::default();
        r.built(&p);
        r.shown(&p, 1_033_333_334, 1_024_000_000);
        let st = r.stats();
        assert_eq!(
            (st.piped, st.on_time, st.build.p50, st.held.p50),
            (1, 1, 7., 12.)
        );
        assert_eq!(
            (
                st.gdrag_refused[0],
                st.gdrag_layer[14],
                st.gdrag_full,
                st.gdrag_full_px
            ),
            (1, 1, 1, 2_000_000)
        );
        let text = st.text();
        assert!(text.contains("not layer 1 [part 1]"));
        assert!(text.ends_with("prepared ahead 1 (held for the flip 12.00/12.00)"));
        unsafe {
            let mut small = [1i8; 7];
            lbf_phases_text(&st, small.as_mut_ptr(), small.len());
            assert_eq!(small, [112, 104, 97, 115, 101, 115, 0]);
        }
    }
}
