//! Battery refresh scheduling and host power requests. C retains the existing
//! sysfs reader and asynchronous D-Bus transport; all state lives on the UI thread.
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, CStr};

#[derive(Default)]
struct BatteryRefresh {
    next: f64,
    snapshot: Vec<u8>,
}

impl BatteryRefresh {
    fn due(&mut self, now: f64, force: bool, battery: bool) -> bool {
        if !now.is_finite() || (!force && now < self.next) {
            return false;
        }
        self.next = now + if battery { 10_000.0 } else { 60_000.0 };
        true
    }

    fn changed(&mut self, snapshot: &[u8]) -> bool {
        if self.snapshot == snapshot { return false; }
        self.snapshot.clear();
        self.snapshot.extend_from_slice(snapshot);
        true
    }
}

thread_local! {
    static BATTERY: RefCell<BatteryRefresh> = RefCell::new(BatteryRefresh::default());
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

#[no_mangle]
pub extern "C" fn axia_battery_due(now: f64, force: bool, battery: bool) -> bool {
    BATTERY.with(|b| b.borrow_mut().due(now, force, battery))
}

#[no_mangle]
pub extern "C" fn axia_battery_timeout(now: f64) -> f64 {
    if !now.is_finite() { return 0.0; }
    BATTERY.with(|b| (b.borrow().next - now).max(0.0))
}

// SAFETY: C supplies a live NUL-terminated snapshot for this call only.
#[no_mangle]
pub unsafe extern "C" fn axia_battery_changed(json: *const c_char) -> bool {
    if json.is_null() { return false; }
    let bytes = CStr::from_ptr(json).to_bytes();
    if bytes.len() > 4096 { return false; }
    BATTERY.with(|b| b.borrow_mut().changed(bytes))
}

fn method(action: &[u8]) -> Option<&'static [u8]> {
    match action {
        b"reboot" => Some(b"Reboot\0"),
        b"poweroff" => Some(b"PowerOff\0"),
        _ => None,
    }
}

// SAFETY: action is null or a live C string. The returned method is static.
// Only one outstanding request is allowed, including while polkit asks a password.
#[no_mangle]
pub unsafe extern "C" fn axia_host_power_begin(action: *const c_char) -> *const c_char {
    if action.is_null() { return std::ptr::null(); }
    let Some(member) = method(CStr::from_ptr(action).to_bytes()) else { return std::ptr::null(); };
    PENDING.with(|pending| {
        if pending.replace(true) { std::ptr::null() } else { member.as_ptr().cast() }
    })
}

fn result(error: &[u8]) -> i32 {
    if error.is_empty() { return 1; }
    match error.rsplit(|c| *c == b'.').next().unwrap_or(error) {
        b"AccessDenied" | b"AuthFailed" | b"NotAuthorized" |
        b"InteractiveAuthorizationRequired" => 2,
        b"OperationInProgress" | b"ShutdownInProgress" | b"OperationInhibited" => 3,
        _ => 4,
    }
}

// 1 success, 2 authorization refused/cancelled, 3 busy/inhibited, 4 unavailable.
#[no_mangle]
pub unsafe extern "C" fn axia_host_power_finish(error: *const c_char) -> i32 {
    PENDING.with(|p| p.set(false));
    result(if error.is_null() { b"" } else { CStr::from_ptr(error).to_bytes() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_refreshes_without_uevents_and_avoids_frame_rate_reads() {
        let mut b = BatteryRefresh::default();
        assert!(b.due(100.0, false, true));
        for n in 1..10_000 { assert!(!b.due(100.0 + n as f64, false, true)); }
        assert!(b.due(10_100.0, false, true));
        assert!(b.due(10_101.0, true, true)); // charger event: immediate
        assert!(!b.due(10_102.0, false, true));
        assert!(b.due(100_000.0, false, false)); // desktops check less often
        assert_eq!(b.next, 160_000.0);
        assert!(b.due(500_000.0, false, false)); // late wake: one read, no catch-up loop
        assert!(!b.due(f64::NAN, true, true));
    }

    #[test]
    fn only_changed_battery_snapshots_are_published() {
        let mut b = BatteryRefresh::default();
        assert!(b.changed(br#"{"percent":70}"#));
        assert!(!b.changed(br#"{"percent":70}"#));
        assert!(b.changed(br#"{"percent":69}"#));
    }

    #[test]
    fn power_requests_are_fixed_and_errors_are_distinguished() {
        assert_eq!(method(b"reboot"), Some(b"Reboot\0".as_slice()));
        assert_eq!(method(b"poweroff"), Some(b"PowerOff\0".as_slice()));
        for invalid in [b"suspend".as_slice(), b"reboot --force", b"", b"Reboot"] {
            assert!(method(invalid).is_none());
        }
        assert_eq!(result(b""), 1);
        assert_eq!(result(b"org.freedesktop.DBus.Error.AccessDenied"), 2);
        assert_eq!(result(b"org.freedesktop.login1.OperationInhibited"), 3);
        assert_eq!(result(b"org.freedesktop.DBus.Error.Disconnected"), 4);
    }
}
