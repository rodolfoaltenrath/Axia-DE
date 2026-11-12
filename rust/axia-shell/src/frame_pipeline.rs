//! Frame scheduling policy, shared with the compositor through `server/src/fpipe.h`.
//! No I/O or allocation: the caller owns one state per display.
use std::ffi::{c_char, c_int, CStr};

const OFF: c_int = 0;
const AUTO: c_int = 1;
const ALWAYS: c_int = 2;
const NONE: c_int = 0;
const READY: c_int = 1;
const EMPTY: c_int = 2;
const WAIT: c_int = 0;
const BUILD: c_int = 1;
const SUBMIT: c_int = 2;
const PREPARE: c_int = 3;
const IDLE_MS: f64 = 500.0;

/// Transitional C ABI. Keep layout in sync with FPipe in fpipe.h.
#[repr(C)]
#[derive(Default)]
pub struct FramePipeline {
    mode: c_int,
    on: c_int,
    heavy: c_int,
    light: c_int,
    last: f64,
}

impl FramePipeline {
    fn new(env: Option<&[u8]>) -> Self {
        Self {
            mode: match env {
                Some(b"1") => AUTO,
                Some(b"2") => ALWAYS,
                _ => OFF,
            },
            ..Self::default()
        }
    }

    fn note(&mut self, now_ms: f64, cpu_ms: f64, period_ms: f64) {
        if self.mode != AUTO || period_ms <= 0.0 {
            return;
        }
        let idle = self.last > 0.0 && now_ms - self.last > IDLE_MS;
        self.last = now_ms;
        if idle {
            self.on = 0;
            self.heavy = 0;
            self.light = 0;
        }
        if self.on == 0 {
            self.heavy = if cpu_ms > period_ms * 0.6 {
                self.heavy + 1
            } else {
                0
            };
            if self.heavy >= 2 {
                self.on = 1;
                self.heavy = 0;
                self.light = 0;
            }
            return;
        }
        self.light = if cpu_ms < period_ms * 0.4 {
            self.light + 1
        } else {
            0
        };
        if self.light >= 60 {
            self.on = 0;
            self.heavy = 0;
            self.light = 0;
        }
    }

    fn engaged(&self) -> bool {
        self.mode == ALWAYS || (self.mode == AUTO && self.on != 0)
    }
}

/// # Safety
/// `p` must point to writable, aligned FPipe storage; `env` must be null or a
/// valid NUL-terminated string. Initialization does not read the old state.
#[no_mangle]
pub unsafe extern "C" fn fpipe_init(p: *mut FramePipeline, env: *const c_char) {
    let env = if env.is_null() {
        None
    } else {
        Some(CStr::from_ptr(env).to_bytes())
    };
    p.write(FramePipeline::new(env));
}

/// # Safety
/// `p` must point to an initialized FPipe with exclusive access for this call.
#[no_mangle]
pub unsafe extern "C" fn fpipe_note(
    p: *mut FramePipeline,
    now_ms: f64,
    cpu_ms: f64,
    period_ms: f64,
) {
    (*p).note(now_ms, cpu_ms, period_ms);
}

/// # Safety
/// `p` must point to an initialized FPipe and must not be mutated during this call.
#[no_mangle]
pub unsafe extern "C" fn fpipe_engaged(p: *const FramePipeline) -> c_int {
    (*p).engaged() as c_int
}

#[no_mangle]
pub extern "C" fn fpipe_step(
    eligible: c_int,
    pending: c_int,
    prep: c_int,
    needs: c_int,
    paint: c_int,
) -> c_int {
    if pending == 0 {
        if prep == READY {
            SUBMIT
        } else if needs != 0 || paint != 0 {
            BUILD
        } else {
            WAIT
        }
    } else if eligible != 0 && prep == NONE && paint != 0 {
        PREPARE
    } else {
        WAIT
    }
}

#[no_mangle]
pub extern "C" fn fpipe_blocked(eligible: c_int, pending: c_int, prep: c_int) -> c_int {
    (pending != 0 && (eligible == 0 || prep != NONE)) as c_int
}

#[no_mangle]
pub extern "C" fn fpipe_hold_callbacks(prep: c_int, asked_again: c_int) -> c_int {
    (prep == EMPTY && asked_again != 0) as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_thresholds_do_not_count_as_heavy_or_light() {
        let mut p = FramePipeline::new(Some(b"1"));
        for i in 1..=100 {
            p.note(i as f64 * 10.0, 6.0, 10.0);
        }
        assert!(!p.engaged());
        p.note(1010.0, 7.0, 10.0);
        p.note(1020.0, 7.0, 10.0);
        assert!(p.engaged());
        for i in 103..=203 {
            p.note(i as f64 * 10.0, 4.0, 10.0);
        }
        assert!(p.engaged());
    }

    #[test]
    fn c_flags_accept_any_nonzero_value() {
        assert_eq!(fpipe_step(-1, 0, NONE, -2, 0), BUILD);
        assert_eq!(fpipe_step(-1, 3, NONE, 0, -2), PREPARE);
        assert_eq!(fpipe_blocked(0, -1, NONE), 1);
        assert_eq!(fpipe_hold_callbacks(EMPTY, -1), 1);
    }
}
