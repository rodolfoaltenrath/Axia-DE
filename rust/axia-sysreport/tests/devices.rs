//! The peripherals (on screen only), the session facts and the colors of the summary.
mod support;

use axia_sysreport::{collect, facts, fetch, out, periph};
use serde_json::Value;
use support::*;

#[test]
fn the_peripherals_are_what_they_are_and_never_in_a_report() {
    let root = temp_dir("periph");
    periph_tree(&root);
    let r = root.to_str().unwrap();
    let j = periph::periph_json(r, None);
    serde_json::from_str::<Value>(&j).unwrap_or_else(|e| panic!("the JSON is invalid: {e}\n{j}"));
    for want in [
        r#"{"name":"Logitech USB Receiver","kind":"receiver","connection":"usb","battery":-1}"#,
        r#"{"name":"Logitech, Inc. Unifying Receiver","kind":"receiver","connection":"usb""#, // usb.ids names a device with no strings
        r#"{"name":"C-Media Electronics Inc. USB Audio Device","kind":"audio""#,             // a sound card with buttons is audio
        r#""secretuser's keyboard","kind":"keyboard","connection":"usb""#,
        r#"{"name":"Logitech USB Optical Mouse","kind":"mouse""#, // its maker not doubled
        r#"{"name":"SanDisk Ultra Fit","kind":"storage""#,
        r#"{"name":"MX Master 3","kind":"mouse","connection":"receiver","battery":64}"#, // behind the receiver, with its battery
        r#"{"name":"Logitech Wireless Keyboard PID:408a","kind":"keyboard","connection":"receiver","battery":-1}"#,
        r#"{"name":"AT Translated Set 2 keyboard","kind":"keyboard","connection":"internal""#,
        r#"{"name":"SYNA2B33:00 Touchpad","kind":"touchpad","connection":"internal""#,
    ] {
        assert!(j.contains(want), "{want}\nin {j}");
    }
    for out in ["Root Hub", "USB HUB", "input-not-listed-twice", "MX Keys", "pwr_button", "vc4-hdmi", "Logitech Wireless Mouse"] {
        assert!(!j.contains(out), "{out:?} is listed");
    }
    for never in ["SECRET", "de:ad:be:ef", "1000480000"] {
        assert!(!j.contains(never), "carries {never:?}");
    }
    // the report of that same tree: no device's own strings
    let rep = collect(r, None, None);
    let (json, md) = (out::json(&rep), out::markdown(&rep));
    for s in SECRETS {
        assert!(!json.contains(s) && !md.contains(s), "the report carries {s:?}");
    }
    for shown in ["MX Master 3", "MX Keys", "secretuser", "USB Optical Mouse", "Wireless Keyboard"] {
        assert!(!json.contains(shown) && !md.contains(shown), "the report carries {shown:?}");
    }
    assert_eq!(periph::periph_json("/nonexistent", Some("/nonexistent/usb.ids")), "[]", "an empty tree");
}

#[test]
fn what_the_session_looks_like_from_inside() {
    let d = temp_dir("facts");
    facts_tree(&d);
    let r = d.to_str().unwrap();
    let f = facts::facts(r);
    let me_root = unsafe { libc::getuid() } == 0;
    assert_eq!((f.uid, f.euid), (1000, 1000));
    assert_eq!(f.cgroup, "/system.slice/lordos.service/lordos-session-1000/server");
    assert_eq!((f.leaf_root, f.parent_root, f.app_scopes), (me_root as i32, me_root as i32, 2));
    // a path that climbs is refused
    put(&d, "proc/self/cgroup", "0::/../../etc\n");
    let f = facts::facts(r);
    assert!(f.cgroup.is_empty() && f.leaf_root == -1);
    // through the C ABI
    let c = std::ffi::CString::new(r).unwrap();
    let mut s = std::mem::MaybeUninit::<facts::SrFacts>::zeroed();
    put(&d, "proc/self/cgroup", "0::/system.slice/lordos.service/lordos-session-1000/server\n");
    unsafe { facts::sr_facts(s.as_mut_ptr(), c.as_ptr()) };
    let s = unsafe { s.assume_init() };
    assert_eq!((s.uid, s.euid, s.app_scopes), (1000, 1000, 2));
    assert_eq!(unsafe { std::ffi::CStr::from_ptr(s.cgroup.as_ptr()) }.to_str().unwrap(), "/system.slice/lordos.service/lordos-session-1000/server");
    // a tree with nothing
    let f = facts::facts(temp_dir("facts-none").to_str().unwrap());
    assert_eq!((f.uid, f.euid, f.leaf_root, f.parent_root, f.app_scopes), (-1, -1, -1, -1, 0));
}

#[test]
fn the_rpi_firmware_reader_is_strict() {
    use axia_sysreport::rpifw;
    let d = temp_dir("rpifw");
    let r = d.to_str().unwrap();
    assert_eq!(rpifw::read(r), rpifw::RpiFw { hwmon: 0, uv_alarm: -1, throttled: -1 });
    put(&d, "sys/class/hwmon/hwmon0/name", "rpi_volt");
    put(&d, "sys/class/hwmon/hwmon0/in0_lcrit_alarm", "1");
    put(&d, "sys/devices/platform/soc/soc:firmware/get_throttled", "0x50005\n");
    assert_eq!(rpifw::read(r), rpifw::RpiFw { hwmon: 1, uv_alarm: 1, throttled: 0x50005 });
}

/// The environment is the process's: colors are checked one after the other, in one test.
#[test]
fn colors_follow_the_terminal_and_the_environment() {
    std::env::set_var("NO_COLOR", "1");
    assert_eq!(fetch::color_mode(Some("auto"), true), fetch::Color::Plain);
    assert_ne!(fetch::color_mode(Some("always"), false), fetch::Color::Plain);
    std::env::remove_var("NO_COLOR");
    std::env::set_var("TERM", "xterm-256color");
    std::env::set_var("COLORTERM", "truecolor");
    assert_eq!(fetch::color_mode(Some("auto"), true), fetch::Color::TrueColor);
    assert_eq!(fetch::color_mode(Some("auto"), false), fetch::Color::Plain);
    assert_eq!(fetch::color_mode(Some("never"), true), fetch::Color::Plain);
    assert_eq!(fetch::color_mode(None, true), fetch::Color::TrueColor);
    std::env::set_var("COLORTERM", "");
    assert_eq!(fetch::color_mode(Some("always"), false), fetch::Color::C256);
    std::env::set_var("TERM", "dumb");
    assert_eq!(fetch::color_mode(Some("auto"), true), fetch::Color::Plain);
}
