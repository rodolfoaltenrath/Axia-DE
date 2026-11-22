//! The power_supply reader over a fake sysfs tree: a laptop battery, a charger and a mouse's battery.
mod support;

use axia_hw::power;
use serde_json::Value;
use std::fs;
use support::*;

#[test]
fn a_laptop_discharging_with_a_mouse_beside_it() {
    let d = temp_dir("power");
    let root = d.to_str().unwrap();
    let ps = power::read(root);
    assert!(ps.n == 0 && ps.has_battery == 0 && ps.on_ac == 1 && ps.percent == -1, "an empty tree is a machine on the mains");
    supply(&d, "AC", "type", "Mains");
    supply(&d, "AC", "online", "0");
    supply(&d, "BAT0", "type", "Battery");
    supply(&d, "BAT0", "status", "Discharging");
    supply(&d, "BAT0", "capacity", "42");
    supply(&d, "BAT0", "energy_now", "21000000"); // 21 Wh
    supply(&d, "BAT0", "energy_full", "50000000");
    supply(&d, "BAT0", "energy_full_design", "57000000");
    supply(&d, "BAT0", "power_now", "7000000"); // 7 W → 3 h
    supply(&d, "BAT0", "cycle_count", "312");
    supply(&d, "BAT0", "model_name", "5B10W13930");
    supply(&d, "BAT0", "technology", "Li-poly");
    supply(&d, "hidpp_battery_0", "type", "Battery"); // a mouse: not the system battery
    supply(&d, "hidpp_battery_0", "scope", "Device");
    supply(&d, "hidpp_battery_0", "capacity", "90");
    supply(&d, "hidpp_battery_0", "status", "Discharging");
    let ps = power::read(root);
    assert_eq!(ps.n, 3);
    assert!(ps.has_battery == 1 && ps.on_ac == 0 && ps.percent == 42 && ps.charging == 0);
    assert_eq!(ps.minutes_left, 180);
    let b = &ps.supplies()[1];
    assert!(b.name() == "BAT0" && b.health == 87 && b.cycles == 312 && (b.watts - 7.0).abs() < 1e-9);
    assert!(b.technology() == "Li-poly" && b.model() == "5B10W13930");
    assert!(ps.supplies()[2].device_scope == 1 && ps.supplies()[2].percent == 90);
}

#[test]
fn charging_with_charge_and_current_and_the_time_to_full() {
    let d = temp_dir("power-charge");
    let root = d.to_str().unwrap();
    supply(&d, "AC", "type", "Mains");
    supply(&d, "AC", "online", "1");
    supply(&d, "BAT0", "type", "Battery");
    supply(&d, "BAT0", "status", "Charging");
    supply(&d, "BAT0", "charge_now", "2000000"); // 2 Ah
    supply(&d, "BAT0", "charge_full", "4000000");
    supply(&d, "BAT0", "charge_full_design", "4000000");
    supply(&d, "BAT0", "current_now", "1000000"); // 1 A → 2 h to full
    supply(&d, "BAT0", "voltage_now", "12000000");
    let ps = power::read(root);
    assert!(ps.on_ac == 1 && ps.charging == 1 && ps.percent == 50 && ps.minutes_left == 120);
    assert!(ps.supplies()[1].health == 100 && (ps.supplies()[1].watts - 12.0).abs() < 1e-9);
    let j = power::json(&ps);
    let v: Value = serde_json::from_slice(&j).unwrap();
    assert_eq!((v["percent"].as_i64(), v["charging"].as_bool(), v["onAC"].as_bool(), v["hasBattery"].as_bool()), (Some(50), Some(true), Some(true), Some(true)));
    assert_eq!(v["supplies"][1]["name"], "BAT0");
    assert_eq!(v["supplies"][1]["watts"].as_f64(), Some(12.0));
    assert!(String::from_utf8_lossy(&j).contains("\"percent\":50") && String::from_utf8_lossy(&j).contains("\"name\":\"BAT0\""));
}

#[test]
fn what_firmware_gets_wrong_is_left_unknown_or_clamped() {
    let d = temp_dir("power-odd");
    supply(&d, "BAT0", "type", "Battery");
    supply(&d, "BAT0", "capacity", "250"); // out of range: from the energy instead
    supply(&d, "BAT0", "energy_now", "60000000");
    supply(&d, "BAT0", "energy_full", "50000000");
    supply(&d, "BAT0", "energy_full_design", "40000000");
    supply(&d, "BAT0", "cycle_count", "0"); // 0 means unknown
    supply(&d, "BAT0", "status", "Full");
    supply(&d, "BAT1", "type", "Battery");
    supply(&d, "BAT1", "present", "0"); // a missing battery bay
    supply(&d, "BAT1", "capacity", "10");
    let ps = power::read(d.to_str().unwrap());
    let b = &ps.supplies()[0];
    assert_eq!((b.percent, b.health, b.cycles, b.minutes_left), (100, 100, -1, -1));
    assert_eq!(ps.percent, 100, "an absent battery is not counted");
    assert_eq!(ps.supplies()[1].present, 0);
    // more supplies than the table has: the first ones by name
    let many = temp_dir("power-many");
    for i in 0..12 {
        supply(&many, &format!("S{i:02}"), "type", "Mains");
    }
    let ps = power::read(many.to_str().unwrap());
    assert_eq!(ps.n, 8);
    assert_eq!(ps.supplies()[7].name(), "S07");
    // a link at the leaf of an attribute is not followed
    let link = temp_dir("power-link");
    supply(&link, "BAT0", "type", "Battery");
    fs::write(link.join("target"), "99\n").unwrap();
    lnk(&link, "BAT0/capacity", link.join("target").to_str().unwrap());
    assert_eq!(power::read(link.to_str().unwrap()).supplies()[0].percent, -1);
}

#[test]
fn the_json_fits_the_buffer_it_is_given() {
    let d = temp_dir("power-json");
    supply(&d, "BAT0", "type", "Battery");
    supply(&d, "BAT0", "capacity", "50");
    let ps = power::read(d.to_str().unwrap());
    let mut small = [0 as std::ffi::c_char; 40];
    let n = unsafe { power_json_ffi(&ps, small.as_mut_ptr(), small.len()) };
    assert!(n < small.len(), "{n}");
    let mut big = vec![0 as std::ffi::c_char; 4096];
    let n = unsafe { power_json_ffi(&ps, big.as_mut_ptr(), big.len()) };
    assert!(n > 0 && n < 4096 && big[n - 1] as u8 == b'}');
}

extern "C" {
    #[link_name = "power_json"]
    fn power_json_ffi(ps: *const power::PowerState, out: *mut std::ffi::c_char, n: usize) -> usize;
}
