//! A fake sysfs tree (what axia-fand reads and writes) and helpers to read it back.
#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};

pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axia-fan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

pub fn put(root: &Path, rel: &str, val: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, format!("{val}\n")).unwrap();
}

pub fn link(root: &Path, rel: &str, target: &str) {
    let p = root.join(rel);
    let _ = fs::remove_file(&p);
    std::os::unix::fs::symlink(target, p).unwrap();
}

/// A number from a file, -99999 if it is not there.
pub fn get(root: &Path, rel: &str) -> i32 {
    fs::read_to_string(root.join(rel)).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(-99999)
}

/// A Raspberry Pi 5 with the fan header: a cpu-thermal zone, a pwm-fan cooling device bound to
/// four active trips, the hwmon pwmfan with its tachometer, and an unrelated hwmon.
pub fn make_pi5(r: &Path, temp_mc: i32) {
    put(r, "proc/device-tree/model", "Raspberry Pi 5 Model B Rev 1.1");
    let z = "sys/class/thermal/thermal_zone0";
    put(r, &format!("{z}/type"), "cpu-thermal");
    put(r, &format!("{z}/temp"), &temp_mc.to_string());
    put(r, &format!("{z}/trip_point_0_type"), "critical");
    put(r, &format!("{z}/trip_point_0_temp"), "110000");
    for (i, t) in [50000, 60000, 67500, 75000].iter().enumerate() {
        let n = i + 1;
        put(r, &format!("{z}/trip_point_{n}_type"), "active");
        put(r, &format!("{z}/trip_point_{n}_temp"), &t.to_string());
        put(r, &format!("{z}/cdev{n}_trip_point"), &n.to_string());
        link(r, &format!("{z}/cdev{n}"), "../cooling_device0");
    }
    put(r, "sys/class/thermal/cooling_device0/type", "pwm-fan");
    put(r, "sys/class/thermal/cooling_device0/max_state", "4");
    put(r, "sys/class/thermal/cooling_device0/cur_state", "0");
    put(r, "sys/class/hwmon/hwmon0/name", "cpu_thermal");
    put(r, "sys/class/hwmon/hwmon0/temp1_input", &temp_mc.to_string());
    put(r, "sys/class/hwmon/hwmon2/name", "pwmfan");
    put(r, "sys/class/hwmon/hwmon2/pwm1", "0");
    put(r, "sys/class/hwmon/hwmon2/pwm1_enable", "1");
    put(r, "sys/class/hwmon/hwmon2/fan1_input", "0");
}

/// A desktop PC: k10temp, a Super I/O with two PWM headers (one with a tachometer), and a GPU
/// whose fan must never be touched.
pub fn make_pc(r: &Path) {
    put(r, "sys/class/hwmon/hwmon0/name", "k10temp");
    put(r, "sys/class/hwmon/hwmon0/temp1_input", "48250");
    put(r, "sys/class/hwmon/hwmon1/name", "nct6798");
    put(r, "sys/class/hwmon/hwmon1/pwm1", "100");
    put(r, "sys/class/hwmon/hwmon1/pwm1_enable", "5");
    put(r, "sys/class/hwmon/hwmon1/fan1_input", "820");
    put(r, "sys/class/hwmon/hwmon1/pwm2", "80");
    put(r, "sys/class/hwmon/hwmon1/pwm2_enable", "2");
    put(r, "sys/class/hwmon/hwmon3/name", "amdgpu");
    put(r, "sys/class/hwmon/hwmon3/pwm1", "60");
    put(r, "sys/class/hwmon/hwmon3/pwm1_enable", "2");
    put(r, "sys/class/hwmon/hwmon3/fan1_input", "1100");
}

pub fn make_none(r: &Path) {
    put(r, "sys/class/thermal/thermal_zone0/type", "acpitz");
    put(r, "sys/class/thermal/thermal_zone0/temp", "41000");
}
