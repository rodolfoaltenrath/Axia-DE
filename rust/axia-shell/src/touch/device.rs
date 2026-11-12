use super::*;
use std::{
    ffi::{c_ulong, CStr},
    mem::zeroed,
};
#[derive(Default, Clone, Copy)]
pub(super) struct Slot {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub major: i32,
    pub pressure: i32,
    pub down: bool,
    pub was: bool,
    pub mx: f64,
    pub my: f64,
    pub pmx: f64,
    pub pmy: f64,
    pub sx: f64,
    pub sy: f64,
    pub t0: f64,
    pub palm: bool,
    pub edge: bool,
    pub thumb: bool,
    pub dwt: bool,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gesture {
    #[default]
    None,
    Pointer,
    UndecidedTwo,
    Scroll,
    Pinch,
    UndecidedMany,
    Swipe,
    DragThree,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tap {
    #[default]
    Idle,
    Touch,
    Tapped,
    DragOrDouble,
    Dragging,
    Dead,
}
#[derive(Default)]
pub struct Device {
    pub(super) c: Caps,
    pub(super) fd: i32,
    pub(super) rx: f64,
    pub(super) ry: f64,
    pub(super) wmm: f64,
    pub(super) hmm: f64,
    pub(super) s: [Slot; MAX_SLOTS],
    pub(super) cur: usize,
    pub(super) tool: usize,
    pub(super) dropped: bool,
    pub(super) phys: bool,
    pub(super) phys_prev: bool,
    pub(super) phys_btn: i32,
    pub(super) btn_pending: [i32; 3],
    pub(super) btn_state: [i32; 3],
    pub(super) now: f64,
    pub(super) prev_t: f64,
    pub(super) g: Gesture,
    pub(super) gn: usize,
    pub(super) g_t0: f64,
    pub(super) bi: usize,
    pub(super) bj: usize,
    pub(super) b1x: f64,
    pub(super) b1y: f64,
    pub(super) b2x: f64,
    pub(super) b2y: f64,
    pub(super) ccx: f64,
    pub(super) ccy: f64,
    pub(super) dist0: f64,
    pub(super) last_ang: f64,
    pub(super) rot: f64,
    pub(super) pscale: f64,
    pub(super) vel: f64,
    pub(super) sdx: f64,
    pub(super) sdy: f64,
    pub(super) hold: bool,
    pub(super) hold_move: f64,
    pub(super) ht: [f64; 8],
    pub(super) hx: [f64; 8],
    pub(super) hy: [f64; 8],
    pub(super) hn: usize,
    pub(super) hi: usize,
    pub(super) kin: bool,
    pub(super) kvx: f64,
    pub(super) kvy: f64,
    pub(super) kt: f64,
    pub(super) d3_down: bool,
    pub(super) d3_release: f64,
    pub(super) tap: Tap,
    pub(super) tap_max: usize,
    pub(super) tap_down: bool,
    pub(super) tap_t: f64,
    pub(super) tap_deadline: f64,
    lat_n: i32,
    lat_kind: usize,
    lat_sum: f64,
    lat_max: f64,
    pub(super) ts_app: bool,
    pub(super) ts_multi: bool,
    pub(super) ts_pending: bool,
    pub(super) ts_btn: bool,
    pub(super) ts_prev_n: usize,
    pub(super) ts_t0: f64,
}
fn has(words: &[c_ulong], code: usize) -> bool {
    words
        .get(code / c_ulong::BITS as usize)
        .is_some_and(|word| word & (1 << (code % c_ulong::BITS as usize)) != 0)
}
/// # Safety
/// fd identifies a live descriptor; name is null or NUL-terminated. Input-thread
/// lifecycle rules apply. The seat provider retains ownership of the descriptor.
#[no_mangle]
pub unsafe extern "C" fn tp_open(fd: c_int, name: *const c_char) -> *mut Device {
    const KEYS: usize = 0x300_usize.div_ceil(c_ulong::BITS as usize);
    let mut events = [0 as c_ulong; 1];
    let mut keys = [0 as c_ulong; KEYS];
    let mut abs = [0 as c_ulong; 1];
    let mut props = [0 as c_ulong; 1];
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; 1]>(b'E' as u32, 0x20),
        events.as_mut_ptr(),
    );
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; KEYS]>(b'E' as u32, 0x21),
        keys.as_mut_ptr(),
    );
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; 1]>(b'E' as u32, 0x23),
        abs.as_mut_ptr(),
    );
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; 1]>(b'E' as u32, 0x09),
        props.as_mut_ptr(),
    );
    if !has(&events, 3) || !has(&abs, 0x35) || !has(&abs, 0x36) || !has(&abs, 0x2f) {
        return ptr::null_mut();
    }
    let mut caps = Caps::default();
    caps.kind = if has(&props, 1) {
        TOUCHSCREEN
    } else if has(&keys, 0x145) && (has(&props, 0) || has(&keys, 0x110) || has(&props, 2)) {
        TOUCHPAD
    } else {
        return ptr::null_mut();
    };
    caps.clickpad = has(&props, 2) as i32;
    caps.has_right = has(&keys, 0x111) as i32;
    let mut slot: libc::input_absinfo = zeroed();
    libc::ioctl(
        fd,
        libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x2f),
        &mut slot,
    );
    caps.nslots = slot.maximum.saturating_add(1);
    libc::ioctl(
        fd,
        libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x35),
        &mut caps.x,
    );
    libc::ioctl(
        fd,
        libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x36),
        &mut caps.y,
    );
    if has(&abs, 0x3a) {
        caps.has_pressure = 1;
        libc::ioctl(
            fd,
            libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x3a),
            &mut caps.pressure,
        );
    }
    if has(&abs, 0x30) {
        caps.has_major = 1;
        libc::ioctl(
            fd,
            libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x30),
            &mut caps.major,
        );
    }
    caps.max_fingers = if has(&keys, 0x148) {
        5
    } else if has(&keys, 0x14f) {
        4
    } else if has(&keys, 0x14e) {
        3
    } else if has(&keys, 0x14d) {
        2
    } else {
        1
    };
    caps.max_fingers = caps.max_fingers.max(caps.nslots);
    if !name.is_null() {
        let bytes = CStr::from_ptr(name).to_bytes();
        let len = bytes.len().min(79);
        caps.name[..len].copy_from_slice(&bytes[..len]);
    }
    let device = tp_new(&caps);
    if device.is_null() {
        return device;
    }
    (*device).fd = fd;
    let clock = libc::CLOCK_MONOTONIC;
    libc::ioctl(fd, libc::_IOW::<c_int>(b'E' as u32, 0xa0), &clock);
    eprintln!(
        "tp: {}: {}{}, {:.0}×{:.0} mm, {} contacts",
        String::from_utf8_lossy(&caps.name[..caps.name.iter().position(|&b| b == 0).unwrap_or(80)]),
        if caps.kind == TOUCHSCREEN {
            "touchscreen"
        } else {
            "touchpad"
        },
        if caps.clickpad != 0 {
            " (clickpad)"
        } else {
            ""
        },
        (*device).wmm,
        (*device).hmm,
        caps.nslots
    );
    device
}
impl Device {
    pub(super) fn new(mut caps: Caps) -> Self {
        caps.nslots = caps.nslots.clamp(1, MAX_SLOTS as i32);
        let mut width = caps.x.maximum as f64 - caps.x.minimum as f64;
        let mut height = caps.y.maximum as f64 - caps.y.minimum as f64;
        if width <= 0.0 {
            width = 1.0;
        }
        if height <= 0.0 {
            height = 1.0;
        }
        let rx = if caps.x.resolution > 0 {
            caps.x.resolution as f64
        } else {
            width
                / if caps.kind == TOUCHSCREEN {
                    300.0
                } else {
                    100.0
                }
        };
        let ry = if caps.y.resolution > 0 {
            caps.y.resolution as f64
        } else {
            rx
        };
        Self {
            c: caps,
            fd: -1,
            rx,
            ry,
            wmm: width / rx,
            hmm: height / ry,
            s: [Slot {
                id: -1,
                ..Slot::default()
            }; MAX_SLOTS],
            ..Self::default()
        }
    }
    pub(super) fn direct(&self) -> bool {
        self.c.kind == TOUCHSCREEN
    }
    pub(super) fn slots(&self) -> usize {
        self.c.nslots as usize
    }
    fn edge_zone(&self) -> f64 {
        (self.wmm * 0.07).min(7.0)
    }
    pub(super) fn bottom_zone(&self) -> f64 {
        (self.hmm * 0.15).min(12.0)
    }
    fn in_edge(&self, s: Slot) -> bool {
        s.mx < self.edge_zone() || s.mx > self.wmm - self.edge_zone()
    }
    fn palm_size(&self, s: Slot) -> bool {
        (self.c.has_major != 0 && s.major > 0 && s.major as f64 / self.rx > 22.0)
            || (self.c.has_pressure != 0
                && self.c.pressure.maximum > self.c.pressure.minimum
                && !self.phys
                && s.pressure as f64
                    > self.c.pressure.minimum as f64
                        + 0.85 * (self.c.pressure.maximum as f64 - self.c.pressure.minimum as f64))
    }
    fn slot_mm(&self, s: &mut Slot) {
        s.mx = (s.x as f64 - self.c.x.minimum as f64) / self.rx;
        s.my = (s.y as f64 - self.c.y.minimum as f64) / self.ry;
    }
    fn begin_touch(&self, s: &mut Slot) {
        self.slot_mm(s);
        s.pmx = s.mx;
        s.sx = s.mx;
        s.pmy = s.my;
        s.sy = s.my;
        s.t0 = self.now;
        s.palm = false;
        s.edge = false;
        s.thumb = false;
        s.dwt = false;
        if self.c.kind != TOUCHPAD {
            return;
        }
        s.palm = self.palm_size(*s);
        s.edge = self.in_edge(*s);
        s.thumb = self.c.clickpad != 0 && s.my > self.hmm - self.bottom_zone();
        s.dwt = config().dwt != 0 && self.now - last_key() < 500.0;
    }
    fn update_touch(&self, s: &mut Slot) {
        self.slot_mm(s);
        if self.c.kind != TOUCHPAD {
            return;
        }
        if !s.palm && self.palm_size(*s) && self.now - s.t0 < 150.0 {
            s.palm = true;
        }
        if s.edge {
            if !self.in_edge(*s) && self.now - s.t0 < EDGE_TIME {
                s.edge = false;
            } else if self.now - s.t0 >= EDGE_TIME {
                s.edge = false;
                s.palm = true;
            }
        }
        if s.thumb && (s.mx - s.sx).hypot(s.my - s.sy) > 4.0 {
            s.thumb = false;
        }
    }
    pub(super) fn active(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        let mut thumbs = Vec::new();
        for (index, s) in self.s.iter().take(self.slots()).enumerate() {
            if !s.down || s.palm || s.edge || s.dwt {
                continue;
            }
            if s.thumb {
                thumbs.push(index);
            } else {
                indices.push(index);
            }
        }
        if indices.is_empty() {
            thumbs
        } else {
            indices
        }
    }
    pub(super) fn count_down(&self) -> usize {
        self.s[..self.slots()].iter().filter(|s| s.down).count()
    }
    pub(super) fn ts_pos(&self, mx: f64, my: f64) -> (f64, f64) {
        (
            mx / if self.wmm > 0.0 { self.wmm } else { 1.0 },
            my / if self.hmm > 0.0 { self.hmm } else { 1.0 },
        )
    }
    fn frame(&mut self) {
        let mut dt = if self.prev_t > 0.0 {
            (self.now - self.prev_t) / 1000.0
        } else {
            0.012
        };
        if dt < 0.004 {
            dt = 0.004;
        }
        if dt > 0.05 {
            dt = 0.05;
        }
        for index in 0..self.slots() {
            let mut s = self.s[index];
            if s.down && !s.was {
                self.begin_touch(&mut s);
            } else if s.down {
                self.update_touch(&mut s);
            }
            self.s[index] = s;
        }
        if self.direct() {
            self.touchscreen_frame(dt);
        } else {
            self.touchpad_frame(dt);
        }
        for index in 0..self.slots() {
            let s = &mut self.s[index];
            s.pmx = s.mx;
            s.pmy = s.my;
            s.was = s.down;
        }
        self.prev_t = self.now;
    }
    fn resync(&mut self) {
        if self.fd < 0 {
            return;
        }
        #[repr(C)]
        struct Request {
            code: u32,
            values: [i32; MAX_SLOTS],
        }
        for (code, kind) in [(0x39, 0), (0x35, 1), (0x36, 2)] {
            let mut request = Request {
                code,
                values: [0; MAX_SLOTS],
            };
            if unsafe {
                libc::ioctl(
                    self.fd,
                    libc::_IOR::<Request>(b'E' as u32, 0x0a),
                    &mut request,
                )
            } < 0
            {
                return;
            }
            for index in 0..self.slots() {
                let s = &mut self.s[index];
                match kind {
                    0 => {
                        s.id = request.values[index];
                        s.down = s.id >= 0;
                    }
                    1 => s.x = request.values[index],
                    _ => s.y = request.values[index],
                }
            }
        }
        let mut slot: libc::input_absinfo = unsafe { zeroed() };
        if unsafe {
            libc::ioctl(
                self.fd,
                libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40 + 0x2f),
                &mut slot,
            )
        } == 0
            && (0..MAX_SLOTS as i32).contains(&slot.value)
        {
            self.cur = slot.value as usize;
        }
    }
    pub(super) fn event(&mut self, event: &libc::input_event, now: f64) {
        if self.dropped {
            if event.type_ == 0 && event.code == 0 {
                self.dropped = false;
                self.resync();
                self.now = now;
                self.frame();
            }
            return;
        }
        match event.type_ {
            3 => {
                let s = &mut self.s[self.cur];
                match event.code {
                    0x2f => {
                        if (0..self.c.nslots).contains(&event.value) {
                            self.cur = event.value as usize;
                        }
                    }
                    0x39 => {
                        s.id = event.value;
                        s.down = event.value >= 0;
                    }
                    0x35 => s.x = event.value,
                    0x36 => s.y = event.value,
                    0x30 => s.major = event.value,
                    0x3a => s.pressure = event.value,
                    _ => {}
                }
            }
            1 => {
                let fingers = match event.code {
                    0x145 => 1,
                    0x14d => 2,
                    0x14e => 3,
                    0x14f => 4,
                    0x148 => 5,
                    _ => 0,
                };
                if fingers != 0 {
                    if event.value != 0 {
                        self.tool = fingers;
                    } else if self.tool == fingers {
                        self.tool = 0;
                    }
                    return;
                }
                if event.value == 2 {
                    return;
                }
                match event.code {
                    0x110 if self.c.clickpad != 0 => self.phys = event.value != 0,
                    0x110 => self.btn_pending[0] = (event.value != 0) as i32,
                    0x112 => self.btn_pending[1] = (event.value != 0) as i32,
                    0x111 => self.btn_pending[2] = (event.value != 0) as i32,
                    _ => {}
                }
            }
            0 => {
                if event.code == 3 {
                    self.dropped = true;
                } else if event.code == 0 {
                    self.now = now;
                    self.frame();
                }
            }
            _ => {}
        }
    }
    pub(super) fn latency(&mut self, syn: &libc::input_event) {
        let time = syn.time.tv_sec as f64 * 1e3 + syn.time.tv_usec as f64 / 1e3;
        let latency = mono_ms() - time;
        if time > 0.0 && latency >= 0.0 && latency < 1000.0 {
            self.lat_sum += latency;
            if latency > self.lat_max {
                self.lat_max = latency;
            }
            self.lat_n += 1;
        }
        if self.g as usize > self.lat_kind {
            self.lat_kind = self.g as usize;
        }
        if self.count_down() == 0 && self.lat_n > 0 {
            let names = [
                "tap",
                "pointer",
                "two fingers",
                "scroll",
                "pinch",
                "several fingers",
                "swipe",
                "3-finger drag",
            ];
            eprintln!(
                "tp: {}: {} frames, average latency {:.2} ms, max {:.2} ms (read → JS/Wayland)",
                names[self.lat_kind],
                self.lat_n,
                self.lat_sum / self.lat_n as f64,
                self.lat_max
            );
            self.lat_n = 0;
            self.lat_sum = 0.0;
            self.lat_max = 0.0;
            self.lat_kind = 0;
        }
    }
    pub(super) fn timeout(&self, now: f64, best: &mut f64) {
        let mut consider = |at: f64| {
            let mut wait = at - now;
            if wait < 0.0 {
                wait = 0.0;
            }
            if *best < 0.0 || wait < *best {
                *best = wait;
            }
        };
        if self.kin {
            consider(self.kt + 16.0);
        }
        if self.tap == Tap::Tapped {
            consider(self.tap_deadline);
        }
        if self.tap == Tap::DragOrDouble || self.tap == Tap::Touch {
            consider(self.tap_t + TAP_TIME + 1.0);
        }
        if self.d3_down && self.d3_release > 0.0 {
            consider(self.d3_release);
        }
        if self.ts_pending {
            consider(self.ts_t0 + TS_PRESS_TIME + 1.0);
        }
        for s in &self.s[..self.slots()] {
            if s.down && s.edge {
                consider(s.t0 + EDGE_TIME);
            }
        }
    }
    pub(super) fn tick(&mut self, now: f64) {
        let mut output = false;
        if self.kin && now - self.kt >= 15.0 {
            let mut dt = now - self.kt;
            if dt > 50.0 {
                dt = 50.0;
            }
            self.kt = now;
            let dx = self.kvx * dt / 1000.0;
            let dy = self.kvy * dt / 1000.0;
            let k = (-dt / 325.0).exp();
            self.kvx *= k;
            self.kvy *= k;
            if self.kvx.hypot(self.kvy) < 25.0 {
                self.kin = false;
                out_scroll(MOMENTUM_END, dx, dy);
            } else {
                out_scroll(MOMENTUM, dx, dy);
            }
            output = true;
        }
        if self.tap == Tap::Tapped && now >= self.tap_deadline {
            self.now = now;
            self.tap_release();
            self.tap = Tap::Idle;
            output = true;
        }
        if self.tap == Tap::DragOrDouble && now - self.tap_t > TAP_TIME {
            self.tap = Tap::Dragging;
        }
        if self.tap == Tap::Touch && now - self.tap_t > TAP_TIME {
            self.tap = Tap::Dead;
        }
        if self.d3_down && self.d3_release > 0.0 && now >= self.d3_release {
            out_button(0, 0);
            self.d3_down = false;
            self.d3_release = 0.0;
            output = true;
        }
        if self.ts_pending && now - self.ts_t0 > TS_PRESS_TIME {
            self.ts_pending = false;
            self.ts_btn = true;
            out_button(0, 1);
            output = true;
        }
        for index in 0..self.slots() {
            let s = &mut self.s[index];
            if s.down && s.edge && now - s.t0 >= EDGE_TIME {
                s.edge = false;
                s.palm = true;
            }
        }
        if output {
            out_frame();
        }
    }
}
