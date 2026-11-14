//! Fixed desktop shortcut profiles for focused Wayland applications.
use std::ffi::{c_char, c_int, c_uint, CStr};
use std::sync::atomic::{AtomicU8, Ordering};

const SHIFT: u32 = 1;
const CTRL: u32 = 2;
const ALT: u32 = 4;
const META: u32 = 8;
const PASS: c_int = 0;
const TAKE: c_int = 1;
const HOLD: c_int = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Profile {
    Disabled,
    Flow,
    FlowDesktops,
    Linux,
}

impl Profile {
    fn parse(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"" => Self::Disabled,
            b"flow" | b"macos" => Self::Flow,
            b"flow-desktops" | b"macos-desktops" => Self::FlowDesktops,
            b"linux" => Self::Linux,
            _ => return None,
        })
    }

    fn remaps(self) -> bool {
        matches!(self, Self::Flow | Self::FlowDesktops)
    }

    fn match_key(self, code: c_int, letter: u8, mods: u32) -> c_int {
        if self == Self::Disabled {
            return PASS;
        }
        // Linux evdev codes are ABI constants, independent of the active layout.
        if letter == 0 {
            if code == 57 && mods == META {
                return TAKE;
            }
            if code == 15 && (mods == META || mods == META | SHIFT) {
                return HOLD;
            }
        }
        if self == Self::Linux {
            if code == 41 && letter == 0 && (mods == ALT || mods == ALT | SHIFT) {
                return TAKE;
            }
            if letter == b'h' && mods == META {
                return TAKE;
            }
        }
        if self.remaps() {
            if letter != 0 {
                if (mods == META && matches!(letter, b'm' | b'h'))
                    || (mods == META | ALT && matches!(letter, b'h' | b'd'))
                {
                    return TAKE;
                }
            } else if (code == 51 && mods == META)
                || (matches!(code, 105 | 106) && mods == META | CTRL)
                || (self == Self::FlowDesktops
                    && matches!(code, 103 | 105 | 106 | 108)
                    && mods == CTRL)
            {
                return TAKE;
            }
        } else if letter != 0 {
            if (mods == META && b"andeir".contains(&letter))
                || (mods == META | SHIFT && letter == b's')
                || (mods == META | CTRL && letter == b'd')
            {
                return TAKE;
            }
        } else {
            if code == 15 && (mods == ALT || mods == ALT | SHIFT) {
                return HOLD;
            }
            if (code == 62 && (mods == ALT || mods == META | CTRL))
                || (code == 1 && mods == CTRL | SHIFT)
                || (mods == META
                    && (matches!(code, 103 | 105 | 106 | 108) || (2..=10).contains(&code)))
                || (mods == META | CTRL && matches!(code, 105 | 106))
            {
                return TAKE;
            }
        }
        PASS
    }
}

static PROFILE: AtomicU8 = AtomicU8::new(Profile::Disabled as u8);

fn current() -> Profile {
    match PROFILE.load(Ordering::Relaxed) {
        1 => Profile::Flow,
        2 => Profile::FlowDesktops,
        3 => Profile::Linux,
        _ => Profile::Disabled,
    }
}

/// # Safety
/// `profile` must be null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn keygrab_set(profile: *const c_char) -> c_int {
    if profile.is_null() {
        return 0;
    }
    let Some(profile) = Profile::parse(CStr::from_ptr(profile).to_bytes()) else {
        return 0;
    };
    PROFILE.store(profile as u8, Ordering::Relaxed);
    1
}

/// # Safety
/// `base` must be null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn keygrab_letter(code: c_int, base: *const c_char) -> c_char {
    if !base.is_null() {
        let base = CStr::from_ptr(base).to_bytes();
        if base.len() == 1 && base[0].is_ascii() {
            return if base[0].is_ascii_alphabetic() {
                base[0].to_ascii_lowercase() as c_char
            } else {
                0
            };
        }
    }
    crate::keymap::letter(code).map_or(0, |letter| letter as c_char)
}

#[no_mangle]
pub extern "C" fn keygrab_match(code: c_int, letter: c_char, mods: c_uint) -> c_int {
    current().match_key(code, letter as u8, mods)
}

#[no_mangle]
pub extern "C" fn keygrab_is_remap(letter: c_char, mods: c_uint) -> c_int {
    (current().remaps() && mods == META && b"cvxazftwnlrps".contains(&(letter as u8))) as c_int
}
