//! The C ABI of server/src/fan/fancurve.h: what `js_fan.c` validates its arguments with (the
//! daemon validates everything again).
use crate::curve::{Curve, Profile};
use std::ffi::{c_char, c_int, CStr};

/// The canonical name of a profile ("quiet", …) copied to `out` (`n` bytes with the NUL). 0, or
/// -1 if `s` is not a profile or does not fit.
///
/// # Safety
/// `s` is a C string and `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn fan_profile_canonical(s: *const c_char, out: *mut c_char, n: usize) -> c_int {
    if s.is_null() || out.is_null() {
        return -1;
    }
    let Some(p) = CStr::from_ptr(s).to_str().ok().and_then(Profile::parse) else { return -1 };
    let name = p.name().as_bytes();
    if name.len() + 1 > n {
        return -1;
    }
    std::ptr::copy_nonoverlapping(name.as_ptr(), out.cast::<u8>(), name.len());
    *out.add(name.len()) = 0;
    0
}

/// 0 if a user curve of `n` points (°C and %) is acceptable: 1..8 points, 30..90 °C strictly
/// increasing, 0..100 % never decreasing. -1 otherwise.
///
/// # Safety
/// `temps` and `pcts` have `n` entries.
#[no_mangle]
pub unsafe extern "C" fn fan_curve_points_valid(temps: *const c_int, pcts: *const c_int, n: c_int) -> c_int {
    if temps.is_null() || pcts.is_null() || n < 0 {
        return -1;
    }
    let points: Vec<(i32, i32)> = (0..n as usize).map(|i| (*temps.add(i), *pcts.add(i))).collect();
    if (Curve { points, floor_pct: 0 }).valid() { 0 } else { -1 }
}
