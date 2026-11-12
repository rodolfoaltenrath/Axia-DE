//! Contact classification, pointer/tap state machines, gestures and inertia.
use super::device::{Device, Gesture, Tap};
use super::*;
impl Device {
    fn scroll_gain(&self, velocity: f64) -> f64 {
        if self.direct() {
            return screen().1 as f64 / if self.hmm > 1.0 { self.hmm } else { 1.0 };
        }
        let gain = 1.0 + velocity / 180.0;
        5.0 * if gain > 3.0 { 3.0 } else { gain }
    }
    fn pointer_move(&mut self, dx: f64, dy: f64, dt: f64) {
        let velocity = dx.hypot(dy) / dt;
        self.vel = if self.vel <= 0.0 {
            velocity
        } else {
            self.vel * 0.6 + velocity * 0.4
        };
        self.sdx = 0.7 * dx + 0.3 * self.sdx;
        self.sdy = 0.7 * dy + 0.3 * self.sdy;
        let gain = tp_accel(self.vel, config().speed);
        if self.sdx == 0.0 && self.sdy == 0.0 {
            return;
        }
        out_motion(self.sdx * gain, self.sdy * gain, dx * 4.0, dy * 4.0);
    }
    fn mean_delta(&self, indices: &[usize]) -> (f64, f64) {
        let (mut dx, mut dy) = (0.0, 0.0);
        for &index in indices {
            let s = self.s[index];
            dx += s.mx - s.pmx;
            dy += s.my - s.pmy;
        }
        if !indices.is_empty() {
            dx /= indices.len() as f64;
            dy /= indices.len() as f64;
        }
        (dx, dy)
    }
    fn centroid(&self, indices: &[usize]) -> (f64, f64) {
        let (mut x, mut y) = (0.0, 0.0);
        for &index in indices {
            x += self.s[index].mx;
            y += self.s[index].my;
        }
        if !indices.is_empty() {
            x /= indices.len() as f64;
            y /= indices.len() as f64;
        }
        (x, y)
    }
    fn scroll_out(&mut self, dx: f64, dy: f64, dt: f64, phase: i32) {
        let gain = self.scroll_gain(dx.hypot(dy) / dt);
        let mut px = dx * gain;
        let mut py = dy * gain;
        if self.direct() {
            px = dx * screen().0 as f64 / if self.wmm > 1.0 { self.wmm } else { 1.0 };
            py = dy * gain;
        }
        if self.direct() || config().natural != 0 {
            px = -px;
            py = -py;
        }
        if px.abs() < 0.15 * py.abs() {
            px = 0.0;
        } else if py.abs() < 0.15 * px.abs() {
            py = 0.0;
        }
        if phase == UPDATE && px == 0.0 && py == 0.0 {
            return;
        }
        self.ht[self.hi] = self.now;
        self.hx[self.hi] = px;
        self.hy[self.hi] = py;
        self.hi = (self.hi + 1) % 8;
        self.hn = (self.hn + 1).min(8);
        out_scroll(phase, px, py);
    }
    fn kinetic_start(&mut self) {
        if config().kinetic == 0 {
            return;
        }
        let (mut sx, mut sy, mut first, mut last) = (0.0, 0.0, self.now, -1.0);
        for i in 0..self.hn {
            if self.now - self.ht[i] > 80.0 {
                continue;
            }
            sx += self.hx[i];
            sy += self.hy[i];
            if self.ht[i] < first {
                first = self.ht[i];
            }
            if self.ht[i] > last {
                last = self.ht[i];
            }
        }
        if last < 0.0 || self.now - last > 60.0 {
            return;
        }
        let span = last - first + 12.0;
        let mut vx = sx / span * 1000.0;
        let mut vy = sy / span * 1000.0;
        if vx.hypot(vy) < 200.0 {
            return;
        }
        if vx.hypot(vy) > 9000.0 {
            let gain = 9000.0 / vx.hypot(vy);
            vx *= gain;
            vy *= gain;
        }
        self.kin = true;
        self.kvx = vx;
        self.kvy = vy;
        self.kt = self.now;
    }
    fn kinetic_stop(&mut self) {
        if !self.kin {
            return;
        }
        self.kin = false;
        out_scroll(MOMENTUM_END, 0.0, 0.0);
    }
    fn hold_end(&mut self, cancel: bool) {
        if !self.hold {
            return;
        }
        self.hold = false;
        out_hold(if cancel { CANCEL } else { END }, self.gn as i32);
    }
    fn gesture_end(&mut self, next: usize) {
        match self.g {
            Gesture::Scroll => {
                out_scroll(END, 0.0, 0.0);
                if next < 2 {
                    self.kinetic_start();
                }
            }
            Gesture::Pinch => out_pinch(END, 2, 0.0, 0.0, self.pscale, self.rot),
            Gesture::Swipe => out_swipe(END, self.gn as i32, 0.0, 0.0),
            Gesture::DragThree if self.d3_down => {
                if next == 0 {
                    self.d3_release = self.now + 500.0;
                } else {
                    out_button(0, 0);
                    self.d3_down = false;
                }
            }
            _ => {}
        }
        if self.hold {
            self.hold_end(next != 0);
        }
    }
    fn gesture_begin(&mut self, n: usize, indices: &[usize]) {
        self.gn = n;
        self.g_t0 = self.now;
        self.vel = 0.0;
        self.sdx = 0.0;
        self.sdy = 0.0;
        self.hold_move = 0.0;
        self.hn = 0;
        self.hi = 0;
        if n == 0 {
            self.g = Gesture::None;
            return;
        }
        if n == 1 {
            self.g = Gesture::Pointer;
            if !self.direct() {
                self.hold = true;
                out_hold(BEGIN, 1);
            }
            return;
        }
        (self.ccx, self.ccy) = self.centroid(indices);
        if n == 2 && indices.len() < 2 {
            self.g = Gesture::None;
            return;
        }
        if n == 2 {
            self.g = Gesture::UndecidedTwo;
            self.bi = indices[0];
            self.bj = indices[1];
            self.b1x = self.s[self.bi].mx;
            self.b1y = self.s[self.bi].my;
            self.b2x = self.s[self.bj].mx;
            self.b2y = self.s[self.bj].my;
            let cfg = config();
            if !self.direct() && (cfg.scroll != 0 || cfg.pinch != 0) {
                self.hold = true;
                out_hold(BEGIN, 2);
            }
            return;
        }
        if self.direct() {
            self.g = Gesture::None;
            return;
        }
        if n == 3 && config().three_drag != 0 {
            self.g = Gesture::DragThree;
            if self.d3_down {
                self.d3_release = 0.0;
            }
            return;
        }
        self.g = Gesture::UndecidedMany;
        self.hold = true;
        out_hold(BEGIN, n as i32);
    }
    fn pinch_update(&mut self) {
        let a = self.s[self.bi];
        let b = self.s[self.bj];
        let dist = (b.mx - a.mx).hypot(b.my - a.my);
        let angle = angle_of(a.mx, a.my, b.mx, b.my);
        let mut delta = angle - self.last_ang;
        while delta > 180.0 {
            delta -= 360.0;
        }
        while delta < -180.0 {
            delta += 360.0;
        }
        self.last_ang = angle;
        self.rot += delta;
        self.pscale = if self.dist0 > 0.5 {
            dist / self.dist0
        } else {
            1.0
        };
        let dx = ((a.mx - a.pmx) + (b.mx - b.pmx)) / 2.0;
        let dy = ((a.my - a.pmy) + (b.my - b.pmy)) / 2.0;
        let gain = if self.direct() {
            screen().0 as f64 / if self.wmm > 1.0 { self.wmm } else { 1.0 }
        } else {
            tp_accel(0.0, config().speed)
        };
        out_pinch(UPDATE, 2, dx * gain, dy * gain, self.pscale, self.rot);
        if self.direct() {
            let (x, y) = self.ts_pos((a.mx + b.mx) / 2.0, (a.my + b.my) / 2.0);
            out_warp(x, y);
        }
    }
    fn decide_two(&mut self, dt: f64) {
        let a = self.s[self.bi];
        let b = self.s[self.bj];
        let (v1x, v1y, v2x, v2y) = (
            a.mx - self.b1x,
            a.my - self.b1y,
            b.mx - self.b2x,
            b.my - self.b2y,
        );
        let l1 = v1x.hypot(v1y);
        let l2 = v2x.hypot(v2y);
        let dist0 = (self.b2x - self.b1x).hypot(self.b2y - self.b1y);
        let dist = (b.mx - a.mx).hypot(b.my - a.my);
        let dd = (dist - dist0).abs();
        let cm = ((v1x + v2x) / 2.0).hypot((v1y + v2y) / 2.0);
        let (mut pinch, mut scroll) = (false, false);
        if l1 > 1.0 && l2 > 1.0 {
            if (v1x * v2x + v1y * v2y) / (l1 * l2) > 0.3 {
                scroll = true;
            } else {
                pinch = true;
            }
        } else if l1 > 3.0 || l2 > 3.0 {
            if dd > 0.7 * if l1 > l2 { l1 } else { l2 } {
                pinch = true;
            } else {
                scroll = true;
            }
        } else if self.now - self.g_t0 > 400.0 && cm > 1.5 {
            scroll = true;
        }
        let cfg = config();
        if pinch && cfg.pinch == 0 {
            pinch = false;
            scroll = cfg.scroll != 0;
        }
        if scroll && cfg.scroll == 0 {
            scroll = false;
        }
        if !pinch && !scroll {
            return;
        }
        self.hold_end(true);
        if self.direct() {
            let (x, y) = self.ts_pos((a.mx + b.mx) / 2.0, (a.my + b.my) / 2.0);
            out_warp(x, y);
        }
        if scroll {
            self.g = Gesture::Scroll;
            self.kinetic_stop();
            self.scroll_out(0.0, 0.0, dt, BEGIN);
            self.scroll_out(
                (v1x + v2x) / 2.0,
                (v1y + v2y) / 2.0,
                if self.now - self.g_t0 > 1.0 {
                    (self.now - self.g_t0) / 1000.0
                } else {
                    dt
                },
                UPDATE,
            );
        } else {
            self.g = Gesture::Pinch;
            self.dist0 = dist0;
            self.last_ang = angle_of(self.b1x, self.b1y, self.b2x, self.b2y);
            self.rot = 0.0;
            self.pscale = 1.0;
            out_pinch(BEGIN, 2, 0.0, 0.0, 1.0, 0.0);
            self.pinch_update();
        }
    }
    fn gesture_frame(&mut self, indices: &[usize], dt: f64) {
        match self.g {
            Gesture::Pointer => {
                let mut k = indices[0];
                if self.phys && indices.len() > 1 {
                    let mut best = -1.0;
                    for &index in indices {
                        let s = self.s[index];
                        let distance = (s.mx - s.pmx).hypot(s.my - s.pmy);
                        if distance > best {
                            best = distance;
                            k = index;
                        }
                    }
                }
                let s = self.s[k];
                let (dx, dy) = (s.mx - s.pmx, s.my - s.pmy);
                if self.direct() {
                    return;
                }
                self.hold_move += dx.hypot(dy);
                if self.hold && self.hold_move > 0.6 {
                    self.hold_end(true);
                }
                if dx != 0.0 || dy != 0.0 {
                    self.pointer_move(dx, dy, dt);
                }
            }
            Gesture::UndecidedTwo => self.decide_two(dt),
            Gesture::Scroll => {
                let (dx, dy) = self.mean_delta(indices);
                self.scroll_out(dx, dy, dt, UPDATE);
            }
            Gesture::Pinch => self.pinch_update(),
            Gesture::UndecidedMany => {
                let (cx, cy) = self.centroid(indices);
                if (cx - self.ccx).hypot(cy - self.ccy) < 3.0 {
                    return;
                }
                self.hold_end(true);
                self.g = Gesture::Swipe;
                let gain = tp_accel(60.0, 0.5);
                out_swipe(BEGIN, indices.len() as i32, 0.0, 0.0);
                out_swipe(
                    UPDATE,
                    indices.len() as i32,
                    (cx - self.ccx) * gain,
                    (cy - self.ccy) * gain,
                );
            }
            Gesture::Swipe => {
                let (dx, dy) = self.mean_delta(indices);
                let gain = tp_accel(60.0, 0.5);
                if dx != 0.0 || dy != 0.0 {
                    out_swipe(UPDATE, indices.len() as i32, dx * gain, dy * gain);
                }
            }
            Gesture::DragThree => {
                let (dx, dy) = self.mean_delta(indices);
                let (cx, cy) = self.centroid(indices);
                if !self.d3_down && (cx - self.ccx).hypot(cy - self.ccy) > 1.5 {
                    self.d3_down = true;
                    self.d3_release = 0.0;
                    out_button(0, 1);
                }
                if self.d3_down && (dx != 0.0 || dy != 0.0) {
                    self.pointer_move(dx, dy, dt);
                }
            }
            _ => {}
        }
    }
    fn tap_moved(&self) -> bool {
        self.s[..self.slots()].iter().any(|s| {
            s.down && !s.palm && !s.edge && !s.dwt && (s.mx - s.sx).hypot(s.my - s.sy) > 1.5
        })
    }
    pub(super) fn tap_release(&mut self) {
        if self.tap_down {
            out_button(0, 0);
            self.tap_down = false;
        }
    }
    fn do_tap(&mut self) {
        let button = match self.tap_max {
            1 => 0,
            2 => 2,
            3 => 1,
            _ => -1,
        };
        self.tap = Tap::Idle;
        if button < 0 {
            return;
        }
        if button == 0 && config().tap_drag != 0 {
            out_button(0, 1);
            self.tap_down = true;
            self.tap = Tap::Tapped;
            self.tap_deadline = self.now + TAP_DRAG_TIME;
            return;
        }
        out_button(button, 1);
        out_button(button, 0);
    }
    fn tap_frame(&mut self, n: usize) {
        let moved = self.tap_moved();
        match self.tap {
            Tap::Idle => {
                if config().tap != 0 && n > 0 && !self.phys && !self.d3_down {
                    self.tap = Tap::Touch;
                    self.tap_t = self.now;
                    self.tap_max = n;
                }
            }
            Tap::Touch => {
                if n > self.tap_max {
                    self.tap_max = n;
                }
                if n == 0 {
                    if self.now - self.tap_t <= TAP_TIME {
                        self.do_tap();
                    } else {
                        self.tap = Tap::Idle;
                    }
                } else if self.phys
                    || moved
                    || self.now - self.tap_t > TAP_TIME
                    || matches!(self.g, Gesture::Scroll | Gesture::Pinch | Gesture::Swipe)
                {
                    self.tap = Tap::Dead;
                }
            }
            Tap::Tapped => {
                if n > 0 {
                    self.tap = Tap::DragOrDouble;
                    self.tap_t = self.now;
                }
            }
            Tap::DragOrDouble => {
                if n > 1 {
                    self.tap_release();
                    self.tap = Tap::Dead;
                } else if n == 0 {
                    self.tap_release();
                    out_button(0, 1);
                    self.tap_down = true;
                    self.tap = Tap::Tapped;
                    self.tap_deadline = self.now + TAP_DRAG_TIME;
                } else if moved || self.now - self.tap_t > TAP_TIME {
                    self.tap = Tap::Dragging;
                }
            }
            Tap::Dragging => {
                if n == 0 {
                    self.tap_release();
                    self.tap = Tap::Idle;
                }
            }
            Tap::Dead => {
                if n == 0 {
                    self.tap = Tap::Idle;
                }
            }
        }
    }
    fn click_button(&self) -> i32 {
        if self.c.clickpad == 0 {
            return 0;
        }
        let (mut all, mut fingers, mut left, mut right) = (0, 0, false, false);
        for s in &self.s[..self.slots()] {
            if !s.down || s.palm {
                continue;
            }
            all += 1;
            if !s.thumb {
                fingers += 1;
            }
            if s.my > self.hmm - self.bottom_zone() {
                if s.mx > self.wmm / 2.0 {
                    right = true;
                } else {
                    left = true;
                }
            }
        }
        let cfg = config();
        if cfg.secondary == 1 {
            return if right { 2 } else { 0 };
        }
        if cfg.secondary == 2 {
            return if left { 2 } else { 0 };
        }
        let mut n = if fingers != 0 { fingers } else { all };
        if self.tool > n && self.tool > self.slots() {
            n = self.tool;
        }
        if n >= 3 {
            1
        } else if n == 2 {
            2
        } else {
            0
        }
    }
    pub(super) fn touchpad_frame(&mut self, dt: f64) {
        let indices = self.active();
        let mut n = indices.len();
        let down = self.count_down();
        if self.tool > self.slots() && down >= self.slots() && n == down {
            n = self.tool;
        }
        let npos = n.min(down);
        let indices = &indices[..npos.min(indices.len())];
        if n > 0 && self.kin {
            self.kinetic_stop();
        }
        if n > 0 && self.d3_down && self.d3_release > 0.0 && n != 3 {
            out_button(0, 0);
            self.d3_down = false;
            self.d3_release = 0.0;
        }
        if self.phys != self.phys_prev {
            if self.phys {
                if self.tap_down {
                    self.tap_release();
                }
                if self.tap != Tap::Idle {
                    self.tap = Tap::Dead;
                }
                self.phys_btn = self.click_button();
                out_button(self.phys_btn, 1);
            } else {
                out_button(self.phys_btn, 0);
            }
            self.phys_prev = self.phys;
        }
        for button in 0..3 {
            if self.btn_pending[button] != self.btn_state[button] {
                self.btn_state[button] = self.btn_pending[button];
                out_button(button as i32, self.btn_state[button]);
            }
        }
        self.tap_frame(n);
        let want = if self.phys {
            if n > 0 {
                1
            } else {
                0
            }
        } else {
            n
        };
        if want != self.gn {
            self.gesture_end(want);
            self.gesture_begin(want, indices);
        } else if want > 0 && !indices.is_empty() {
            self.gesture_frame(indices, dt);
        }
    }
    pub(super) fn touchscreen_frame(&mut self, dt: f64) {
        let indices: Vec<usize> = (0..self.slots()).filter(|&i| self.s[i].down).collect();
        let n = indices.len();
        if self.ts_prev_n == 0 && n > 0 {
            let s = self.s[indices[0]];
            let (x, y) = self.ts_pos(s.mx, s.my);
            self.kinetic_stop();
            self.ts_app = out_touch(BEGIN, indices[0] as i32, x, y) != 0;
            self.ts_multi = false;
            if self.ts_app {
                for &index in &indices[1..] {
                    let s = self.s[index];
                    let (x, y) = self.ts_pos(s.mx, s.my);
                    out_touch(BEGIN, index as i32, x, y);
                }
                out_touch_frame();
                self.ts_prev_n = n;
                return;
            }
        } else if self.ts_app {
            for index in 0..self.slots() {
                let s = self.s[index];
                let (x, y) = self.ts_pos(s.mx, s.my);
                if s.down && !s.was {
                    out_touch(BEGIN, index as i32, x, y);
                } else if s.down && (s.mx != s.pmx || s.my != s.pmy) {
                    out_touch(UPDATE, index as i32, x, y);
                } else if !s.down && s.was {
                    out_touch(END, index as i32, x, y);
                }
            }
            out_touch_frame();
            if n == 0 {
                self.ts_app = false;
            }
            self.ts_prev_n = n;
            return;
        }
        if n >= 2 && !self.ts_multi {
            self.ts_multi = true;
            if self.ts_btn {
                out_button(0, 0);
                self.ts_btn = false;
            }
            self.ts_pending = false;
        }
        if n == 1 && !self.ts_multi {
            let s = self.s[indices[0]];
            let (x, y) = self.ts_pos(s.mx, s.my);
            if self.ts_prev_n == 0 {
                out_warp(x, y);
                self.ts_pending = true;
                self.ts_t0 = self.now;
            } else if s.mx != s.pmx || s.my != s.pmy {
                out_warp(x, y);
            }
            if self.ts_pending
                && ((s.mx - s.sx).hypot(s.my - s.sy) > 2.0 || self.now - self.ts_t0 > TS_PRESS_TIME)
            {
                self.ts_pending = false;
                self.ts_btn = true;
                out_button(0, 1);
            }
        }
        if n == 0 {
            if self.ts_pending {
                out_button(0, 1);
                out_button(0, 0);
            } else if self.ts_btn {
                out_button(0, 0);
            }
            self.ts_pending = false;
            self.ts_btn = false;
            self.ts_multi = false;
        }
        let want = if n >= 2 { n } else { 0 };
        if want != self.gn {
            self.gesture_end(want);
            self.gesture_begin(want, &indices);
        } else if want != 0 {
            self.gesture_frame(&indices, dt);
        }
        self.ts_prev_n = n;
    }
}
fn angle_of(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (y2 - y1).atan2(x2 - x1) * 180.0 / std::f64::consts::PI
}
