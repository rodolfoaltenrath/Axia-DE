//! The sysfs driver against fake trees: a Raspberry Pi 5 fan header with kernel trips, a PC Super
//! I/O chip with a GPU next to it, a machine with nothing to drive; take, set, release and the
//! restore file after a crash.
mod common;

use axia_fan::hw::{restore_file, FanHw, Kind, BACKSTOP_C, TEMP_NONE};
use common::*;
use std::fs;

#[test]
fn a_raspberry_pi_5_fan_header() {
    let d = temp_dir("pi5");
    let root = d.join("pi5");
    make_pi5(&root, 63000);
    let state = d.join("fand.restore");
    let state = state.to_str().unwrap();
    let mut h = FanHw::scan(root.to_str().unwrap());
    assert_eq!(h.t.len(), 1);
    assert!(h.board && h.t[0].kind == Kind::Board && h.t[0].has_rpm && h.temp_src == "cpu-thermal");
    assert_eq!(h.temp(), 63000);
    assert_eq!((h.trip.len(), h.trip[0], h.trip_orig[0], h.trip_orig[3]), (4, 1, 50000, 75000));
    assert!(h.ro.is_empty());
    assert_eq!(h.take(Some(state)), 1);
    assert!(fs::metadata(state).is_ok(), "the notes are written before anything changes");
    let trip = |n: i32| get(&root, &format!("sys/class/thermal/thermal_zone0/trip_point_{n}_temp"));
    assert_eq!((trip(1), trip(4)), (BACKSTOP_C * 1000, 84000));
    assert_eq!(trip(0), 110000, "the critical trip is untouched");
    let pwm = || get(&root, "sys/class/hwmon/hwmon2/pwm1");
    // starting a stopped fan at 30 %: full speed first, then 30 %
    assert!(h.set(0, 30) && pwm() == 255);
    assert!(h.set(0, 30) && pwm() == 77);
    assert_eq!(h.pct(0), 30);
    assert!(h.set(0, 100) && pwm() == 255);
    assert!(h.set(0, 0) && pwm() == 0);
    assert!(h.set(0, 60) && pwm() == 153, "60 % is not kicked");
    put(&root, "sys/class/hwmon/hwmon2/pwm1", "125"); // the kernel stepped in
    assert!(h.set(0, 60) && pwm() == 153);
    put(&root, "sys/class/hwmon/hwmon2/fan1_input", "4120");
    assert_eq!(h.rpm(0), 4120);
    assert!(!h.set(5, 50));
    h.release();
    assert_eq!((trip(1), trip(3)), (50000, 67500));
    assert_eq!(get(&root, "sys/class/thermal/cooling_device0/cur_state"), 2, "63 °C: trips 50 and 60");
    assert!(!h.set(0, 50), "not ours any more");
    // a crash: take, then restore from the notes with a fresh process
    let mut h2 = FanHw::scan(root.to_str().unwrap());
    h2.take(Some(state));
    assert_eq!(trip(2), 80000);
    assert!(restore_file(root.to_str().unwrap(), state).is_ok());
    assert_eq!(trip(2), 60000);
    assert!(fs::metadata(state).is_err(), "the notes are gone");
    // trips that look raised with no notes: the original values are unknown
    let mut h3 = FanHw::scan(root.to_str().unwrap());
    h3.take(None);
    assert!(restore_file(root.to_str().unwrap(), state).is_err());
    assert_eq!(trip(2), 80000, "nothing is guessed");
}

#[test]
fn a_pc_with_a_super_io_chip_and_a_gpu() {
    let d = temp_dir("pc");
    let root = d.join("pc");
    make_pc(&root);
    let state = d.join("fand.restore");
    let state = state.to_str().unwrap();
    let mut h = FanHw::scan(root.to_str().unwrap());
    assert_eq!(h.t.len(), 2);
    assert!(!h.board && h.temp_src == "k10temp" && h.temp() == 48250);
    assert!(h.t[0].kind == Kind::Hwmon && h.t[0].ch == 1 && h.t[0].has_rpm && h.t[1].ch == 2 && !h.t[1].has_rpm);
    assert!(h.ro.len() == 1 && h.ro[0].chip == "amdgpu" && h.ro_rpm(0) == 1100);
    assert_eq!(h.take(Some(state)), 2);
    let en = |hw: i32, ch: i32| get(&root, &format!("sys/class/hwmon/hwmon{hw}/pwm{ch}_enable"));
    assert!(en(1, 1) == 1 && en(1, 2) == 1);
    assert_eq!(en(3, 1), 2, "the GPU is untouched");
    h.set(0, 70);
    assert_eq!(get(&root, "sys/class/hwmon/hwmon1/pwm1"), 179);
    h.release();
    assert!(en(1, 1) == 5 && en(1, 2) == 2, "each one goes back to what it was");
    // crash restore with notes; a tampered line is ignored
    let mut h = FanHw::scan(root.to_str().unwrap());
    h.take(Some(state));
    assert!(en(1, 1) == 1);
    let mut notes = fs::read_to_string(state).unwrap();
    assert!(notes.starts_with("axia-fand 1\nenable 1 5 ") && notes.contains("\nenable 2 2 "));
    notes.push_str("enable 1 9 /etc\nexec /bin/sh\nenable 1 4 /not/the/device\ntrip 0 99999999\n");
    fs::write(state, notes).unwrap();
    assert!(restore_file(root.to_str().unwrap(), state).is_ok());
    assert!(en(1, 1) == 5 && en(1, 2) == 2);
    // a file that is not ours is refused and still removed
    fs::write(state, "something else\nenable 1 0 x\n").unwrap();
    assert!(restore_file(root.to_str().unwrap(), state).is_err());
    assert!(fs::metadata(state).is_err());
}

#[test]
fn nothing_to_drive() {
    let d = temp_dir("none");
    let root = d.join("none");
    make_none(&root);
    let h = FanHw::scan(root.to_str().unwrap());
    assert!(h.t.is_empty() && h.ro.is_empty() && h.temp() == 41000 && h.temp_src == "thermal_zone0");
    let empty = d.join("empty");
    fs::create_dir(&empty).unwrap();
    let h = FanHw::scan(empty.to_str().unwrap());
    assert!(h.t.is_empty() && h.temp() == TEMP_NONE && h.temp_src.is_empty());
}

#[test]
fn what_is_not_a_plain_number_or_is_out_of_range_is_not_a_temperature() {
    let d = temp_dir("temps");
    let root = d.join("t");
    make_none(&root);
    let h = FanHw::scan(root.to_str().unwrap());
    for (v, want) in [("41000", 41000), ("-50000", -50000), ("200000", 200000), ("200001", TEMP_NONE), ("-50001", TEMP_NONE), ("abc", TEMP_NONE), ("", TEMP_NONE), ("41000x", TEMP_NONE), ("99999999999", TEMP_NONE), ("+42000", 42000), (" 41000 ", 41000)] {
        put(&root, "sys/class/thermal/thermal_zone0/temp", v);
        assert_eq!(h.temp(), want, "{v:?}");
    }
    // a temperature file that is a symlink is not followed
    fs::remove_file(root.join("sys/class/thermal/thermal_zone0/temp")).unwrap();
    put(&root, "elsewhere", "50000");
    link(&root, "sys/class/thermal/thermal_zone0/temp", root.join("elsewhere").to_str().unwrap());
    assert_eq!(h.temp(), TEMP_NONE);
}
