//! The daemon's logic on a fake tree (ticks, requests, status) and the real program over its
//! socket.
mod common;

use axia_fan::daemon::Daemon;
use common::*;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn daemon(name: &str, make: impl FnOnce(&std::path::Path)) -> (Daemon, std::path::PathBuf) {
    let d = temp_dir(name);
    let root = d.join("root");
    make(&root);
    let mut dm = Daemon::new(root.to_str().unwrap(), d.join("fan.json").to_str().unwrap(), d.join("fand.restore").to_str().unwrap());
    dm.load_config();
    dm.apply_profile();
    (dm, root)
}

fn ask(d: &mut Daemon, line: &str, admin: bool, now: i64) -> String {
    String::from_utf8(d.handle(format!("{line}\n").as_bytes(), || admin, now)).unwrap()
}

#[test]
fn a_board_fan_follows_its_curve_and_goes_back_when_set_to_auto() {
    let (mut d, root) = daemon("board", |r| make_pi5(r, 40000));
    assert_eq!(d.cfg.profile.name(), "balanced", "a board fan is ours from the first boot");
    let pwm = || get(&root, "sys/class/hwmon/hwmon2/pwm1");
    let set_temp = |mc: i32| put(&root, "sys/class/thermal/thermal_zone0/temp", &mc.to_string());
    d.tick(1000);
    assert!(d.controlling && d.out_pct == 0 && d.step == -1 && pwm() == 0, "40 °C: below the first step");
    set_temp(56000);
    d.tick(1002);
    assert_eq!((d.step, d.out_pct), (0, 30));
    assert_eq!(pwm(), 255, "a stopped fan starts with a kick");
    d.tick(1004);
    assert_eq!(pwm(), 77);
    set_temp(66000);
    d.tick(1006);
    assert_eq!((d.step, d.out_pct, pwm()), (2, 100, 255));
    set_temp(63000); // inside the hysteresis
    d.tick(1008);
    assert_eq!(d.step, 2);
    set_temp(-1_000_000); // no reading: be safe
    put(&root, "sys/class/thermal/thermal_zone0/temp", "garbage");
    d.tick(1010);
    assert_eq!((d.out_pct, pwm()), (100, 255));
    // settings: needs an administrator and is saved
    assert_eq!(ask(&mut d, "PROFILE quiet", false, 1012), "ERR fan.notAdmin\n");
    assert_eq!(d.cfg.profile.name(), "balanced");
    assert_eq!(ask(&mut d, "PROFILE quiet", true, 1012), "OK\n");
    let saved = std::fs::read_to_string(d.config_path.clone()).unwrap();
    assert!(saved.contains("\"profile\":\"quiet\""), "{saved}");
    assert_eq!(ask(&mut d, "CURVE 40:20,50:50,70:100", true, 1014), "OK\n");
    assert_eq!((d.cfg.profile.name(), d.cfg.custom.points.len()), ("custom", 3));
    // auto gives the fan back
    assert_eq!(ask(&mut d, "PROFILE auto", true, 1016), "OK\n");
    assert!(!d.controlling && d.out_pct == -1);
    assert_eq!(get(&root, "sys/class/thermal/thermal_zone0/trip_point_1_temp"), 50000, "the kernel's trips are back");
    assert!(std::fs::metadata(&d.state_path).is_err());
    // forced: full speed for a while, then it ends by itself and is saved as off
    assert_eq!(ask(&mut d, "FORCE 5", true, 2000), "OK\n");
    assert!(d.controlling && d.out_pct == 100);
    let s: Value = serde_json::from_str(&d.status_json(true, 2060)).unwrap();
    assert_eq!((s["force"].as_str(), s["forceLeft"].as_i64()), (Some("timed"), Some(240)));
    d.tick(2301);
    assert!(!d.controlling && d.cfg.force == axia_fan::curve::Force::Off);
    assert!(std::fs::read_to_string(d.config_path.clone()).unwrap().contains("\"force\":\"off\""));
    assert_eq!(ask(&mut d, "FORCE on", true, 2400), "OK\n");
    assert!(d.controlling);
    // bad requests
    for bad in ["", "NOPE", "PROFILE", "FORCE 0", "FORCE 0\n"] {
        assert_eq!(ask(&mut d, bad, true, 1), "ERR fan.badRequest\n", "{bad:?}");
    }
    assert_eq!(d.handle(b"STATUS", || true, 1), b"ERR fan.badRequest\n", "a line with no end");
    assert_eq!(ask(&mut d, "STATUS\nPROFILE quiet", false, 1).chars().next(), Some('{'), "only the first line counts");
}

#[test]
fn a_pc_stays_with_its_firmware_until_a_profile_is_chosen() {
    let (mut d, root) = daemon("pc", make_pc);
    assert_eq!(d.cfg.profile.name(), "auto");
    d.tick(1000);
    assert!(!d.controlling);
    assert_eq!(get(&root, "sys/class/hwmon/hwmon1/pwm1_enable"), 5, "untouched");
    assert_eq!(ask(&mut d, "PROFILE performance", true, 1001), "OK\n");
    assert!(d.controlling);
    assert_eq!(get(&root, "sys/class/hwmon/hwmon1/pwm1_enable"), 1);
    // 48.25 °C is above the first performance step (45 °C → 40 %): 40 % of 255
    assert_eq!((d.step, d.out_pct, get(&root, "sys/class/hwmon/hwmon1/pwm1")), (0, 40, 102));
    assert_eq!(get(&root, "sys/class/hwmon/hwmon3/pwm1"), 60, "the GPU fan is left alone");
}

#[test]
fn the_status_line_says_what_the_daemon_knows() {
    let (mut d, _) = daemon("status", make_pc);
    d.tick(1000);
    let s: Value = serde_json::from_str(&d.status_json(true, 1000)).unwrap();
    assert_eq!(s["ok"], true);
    assert_eq!((s["canChange"].as_bool(), s["board"].as_str(), s["tempSource"].as_str(), s["temp"].as_f64()), (Some(true), Some(""), Some("k10temp"), Some(48.2)));
    assert_eq!((s["profile"].as_str(), s["hyst"].as_i64(), s["controlling"].as_bool(), s["step"].as_i64(), s["target"].as_i64()), (Some("auto"), Some(3), Some(false), Some(-1), Some(-1)));
    assert_eq!((s["floor"].as_i64(), s["curve"].as_array().map(Vec::len), s["custom"].as_array().map(Vec::len)), (Some(0), Some(0), Some(3)));
    assert_eq!(s["fans"][0], serde_json::json!({ "id": 0, "kind": "hwmon", "chip": "nct6798", "ch": 1, "pct": 39, "rpm": 820 }));
    assert_eq!(s["fans"][1]["rpm"], -1);
    assert_eq!(s["firmwareFans"], serde_json::json!([{ "chip": "amdgpu", "rpm": 1100 }]));
    let json = d.status_json(false, 1000);
    assert!(json.ends_with("}\n") && json.contains("\"canChange\":false"));
    // no temperature at all
    let (mut d, _) = daemon("status-none", |r| std::fs::create_dir_all(r).unwrap());
    d.tick(1);
    let s: Value = serde_json::from_str(&d.status_json(false, 1)).unwrap();
    assert!(s["temp"].is_null() && s["fans"].as_array().unwrap().is_empty());
}

#[test]
fn a_config_that_is_not_root_owned_is_ignored_with_the_defaults() {
    let (mut d, root) = daemon("cfgowner", make_pc);
    std::fs::write(&d.config_path, r#"{"profile":"quiet"}"#).unwrap();
    d.load_config();
    // the test runs as a normal user: the file is not root's
    assert_eq!(d.cfg.profile.name(), if unsafe { libc::geteuid() } == 0 { "quiet" } else { "auto" });
    let _ = root;
}

struct Run(Child, std::path::PathBuf);
impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn talk(sock: &std::path::Path, line: &str) -> String {
    let mut s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn the_program_serves_its_socket_and_gives_the_fans_back_when_stopped() {
    let d = temp_dir("prog");
    let root = d.join("root");
    make_pi5(&root, 58000);
    let run = d.join("run");
    let child = Command::new(env!("CARGO_BIN_EXE_axia-fand"))
        .args(["--root", root.to_str().unwrap(), "--config", d.join("fan.json").to_str().unwrap(), "--run", run.to_str().unwrap()])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut p = Run(child, run.clone());
    let sock = run.join("fand.sock");
    let end = Instant::now() + Duration::from_secs(10);
    while UnixStream::connect(&sock).is_err() {
        assert!(Instant::now() < end, "the socket never came up");
        std::thread::sleep(Duration::from_millis(20));
    }
    let s: Value = serde_json::from_str(&talk(&sock, "STATUS\n")).unwrap();
    assert_eq!((s["ok"].as_bool(), s["board"].as_str(), s["profile"].as_str(), s["fans"].as_array().map(Vec::len)), (Some(true), Some("pi5"), Some("balanced"), Some(1)));
    let can = s["canChange"].as_bool().unwrap();
    assert_eq!(talk(&sock, "PROFILE bogus\n"), "ERR fan.badRequest\n");
    assert_eq!(talk(&sock, "no newline"), "", "a client that never finishes its line is dropped");
    let r = talk(&sock, "PROFILE performance\n");
    assert_eq!(r, if can { "OK\n" } else { "ERR fan.notAdmin\n" });
    if can {
        let s: Value = serde_json::from_str(&talk(&sock, "STATUS\n")).unwrap();
        assert_eq!(s["profile"], "performance");
    }
    // the first tick has taken the fan: the trips are at the backstop
    let end = Instant::now() + Duration::from_secs(10);
    while get(&root, "sys/class/thermal/thermal_zone0/trip_point_1_temp") != 78000 {
        assert!(Instant::now() < end, "the fan was never taken");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(std::fs::metadata(run.join("fand.restore")).is_ok());
    // SIGTERM: everything goes back and the socket is removed
    unsafe { libc::kill(p.0.id() as i32, libc::SIGTERM) };
    let status = p.0.wait().unwrap();
    assert!(status.success());
    assert_eq!(get(&root, "sys/class/thermal/thermal_zone0/trip_point_1_temp"), 50000);
    assert!(std::fs::metadata(&sock).is_err() && std::fs::metadata(run.join("fand.restore")).is_err());
    let _ = &p.1;
}

#[test]
fn restore_and_detect_modes() {
    let d = temp_dir("modes");
    let root = d.join("root");
    make_pc(&root);
    let run = d.join("run");
    std::fs::create_dir(&run).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axia-fand")).args(["--detect", "--root", root.to_str().unwrap()]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(text.starts_with("board: other\ntemperature: k10temp ("), "{text}");
    assert!(text.contains("fan 0: hwmon nct6798 pwm1 at 39 %, 820 rpm (") && text.contains("firmware fan: amdgpu 1100 rpm\n"), "{text}");
    // --restore with nothing taken does nothing; with notes it puts the fans back
    let r = |extra: &[&str]| Command::new(env!("CARGO_BIN_EXE_axia-fand")).args(["--restore", "--root", root.to_str().unwrap(), "--run", run.to_str().unwrap()]).args(extra).output().unwrap();
    assert!(r(&[]).status.success());
    let mut h = axia_fan::hw::FanHw::scan(root.to_str().unwrap());
    h.take(Some(run.join("fand.restore").to_str().unwrap()));
    assert_eq!(get(&root, "sys/class/hwmon/hwmon1/pwm1_enable"), 1);
    assert!(r(&[]).status.success());
    assert_eq!(get(&root, "sys/class/hwmon/hwmon1/pwm1_enable"), 5);
    // usage
    let bad = Command::new(env!("CARGO_BIN_EXE_axia-fand")).arg("--nope").output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
}
