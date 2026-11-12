//! On-screen keyboard tap queue and physical keyboard detection.
use std::{
    ffi::{c_char, c_int, c_ulong, CStr},
    sync::Mutex,
};

const QUEUE_MAX: usize = 16;

// Linux evdev codes in the fixed visual order used by lord.input.keyLabels.
#[export_name = "osk_label_codes"]
pub static LABEL_CODES: [u16; 49] = [
    41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 43, 86, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 57,
];
#[export_name = "osk_label_count"]
pub static LABEL_COUNT: usize = LABEL_CODES.len();

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tap {
    code: c_int,
    level: c_int,
}

struct Queue {
    taps: [Tap; QUEUE_MAX],
    head: usize,
    len: usize,
}
impl Queue {
    const fn new() -> Self {
        Self {
            taps: [Tap { code: 0, level: 0 }; QUEUE_MAX],
            head: 0,
            len: 0,
        }
    }
    fn push(&mut self, code: c_int, level: c_int) -> bool {
        if osk_key_allowed(code) == 0 || osk_level_valid(level) == 0 || self.len == QUEUE_MAX {
            return false;
        }
        self.taps[(self.head + self.len) % QUEUE_MAX] = Tap { code, level };
        self.len += 1;
        true
    }
    fn pop(&mut self) -> Option<Tap> {
        if self.len == 0 {
            return None;
        }
        let tap = std::mem::take(&mut self.taps[self.head]);
        self.head = (self.head + 1) % QUEUE_MAX;
        self.len -= 1;
        Some(tap)
    }
}
static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());

#[no_mangle]
pub extern "C" fn osk_key_allowed(code: c_int) -> c_int {
    (u16::try_from(code).is_ok_and(|code| LABEL_CODES.contains(&code))
        || matches!(code, 14 | 28 | 15 | 1 | 105 | 106 | 103 | 108)) as c_int
}
#[no_mangle]
pub extern "C" fn osk_level_valid(level: c_int) -> c_int {
    (0..=3).contains(&level) as c_int
}
#[no_mangle]
pub extern "C" fn osk_push(code: c_int, level: c_int) -> c_int {
    QUEUE.lock().unwrap().push(code, level) as c_int
}
/// # Safety
/// When the queue is nonempty, `out` must point to writable, aligned OskTap storage.
#[no_mangle]
pub unsafe extern "C" fn osk_pop(out: *mut Tap) -> c_int {
    if let Some(tap) = QUEUE.lock().unwrap().pop() {
        out.write(tap);
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn osk_clear() {
    *QUEUE.lock().unwrap() = Queue::new();
}

/// # Safety
/// `keys` must hold `nlongs` readable capability words (may be null for zero words).
/// `name` must be null or a valid NUL-terminated kernel device name.
#[no_mangle]
pub unsafe extern "C" fn osk_full_keyboard(
    keys: *const c_ulong,
    nlongs: usize,
    name: *const c_char,
) -> c_int {
    const REQUIRED: [usize; 30] = [
        57, 28, 14, 42, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38,
        44, 45, 46, 47, 48, 49, 50,
    ];
    for code in REQUIRED {
        let index = code / c_ulong::BITS as usize;
        if index >= nlongs || (*keys.add(index) >> (code % c_ulong::BITS as usize)) & 1 == 0 {
            return 0;
        }
    }
    let name = if name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(name).to_bytes()
    };
    let tokens: &[&[u8]] = &[
        b"YubiKey",
        b"Yubico",
        b"Nitrokey",
        b"SoloKey",
        b"OnlyKey",
        b"Token",
    ];
    (!tokens
        .iter()
        .any(|token| name.windows(token.len()).any(|part| part == *token))) as c_int
}
#[no_mangle]
pub extern "C" fn osk_keyboard_counts(bus: c_int, portable: c_int, typed: c_int) -> c_int {
    (bus != 0x11 || portable != 0 || typed != 0) as c_int
}
#[no_mangle]
pub extern "C" fn osk_chassis_portable(chassis: c_int) -> c_int {
    matches!(chassis, 8 | 9 | 10 | 14 | 31 | 32) as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_are_bounded_ordered_and_erased_after_consumption() {
        let mut queue = Queue::new();
        for level in 0..4 {
            assert!(queue.push(30, level));
        }
        for level in 0..4 {
            assert_eq!(queue.pop(), Some(Tap { code: 30, level }));
        }
        assert!(queue.taps.iter().all(|tap| *tap == Tap::default()));
        for i in 0..QUEUE_MAX {
            assert!(queue.push(30, (i % 4) as i32));
        }
        assert!(!queue.push(30, 0));
        for i in 0..QUEUE_MAX * 4 {
            assert_eq!(queue.pop().unwrap().level, (i % 4) as i32);
            assert!(queue.push(48, (i % 4) as i32));
        }
        for _ in 0..QUEUE_MAX {
            assert!(queue.pop().is_some());
        }
        assert_eq!(queue.pop(), None);
        for code in [-1, 65566, 29, 42, 56, 125, 59, 116, 272] {
            assert!(!queue.push(code, 0));
        }
        for level in [-1, 4, i32::MAX] {
            assert!(!queue.push(30, level));
        }
    }

    #[test]
    fn token_names_and_incomplete_capabilities_do_not_hide_virtual_keyboard() {
        let full = [c_ulong::MAX; 12];
        unsafe {
            assert_eq!(
                osk_full_keyboard(full.as_ptr(), full.len(), c"USB Keyboard".as_ptr()),
                1
            );
            assert_eq!(
                osk_full_keyboard(full.as_ptr(), 0, c"USB Keyboard".as_ptr()),
                0
            );
            for name in [
                c"YubiKey",
                c"Yubico token",
                c"Nitrokey",
                c"SoloKey",
                c"OnlyKey",
                c"USB Token",
            ] {
                assert_eq!(
                    osk_full_keyboard(full.as_ptr(), full.len(), name.as_ptr()),
                    0
                );
            }
            let mut missing_q = full;
            missing_q[0] &= !(1 << 16);
            assert_eq!(
                osk_full_keyboard(missing_q.as_ptr(), missing_q.len(), std::ptr::null()),
                0
            );
        }
        assert_eq!(osk_keyboard_counts(0x11, 0, 0), 0);
        assert_eq!(osk_keyboard_counts(0x11, 0, -1), 1);
        assert_eq!(osk_keyboard_counts(3, 0, 0), 1);
        assert_eq!(osk_chassis_portable(31), 1);
        assert_eq!(osk_chassis_portable(30), 0);
    }
}
