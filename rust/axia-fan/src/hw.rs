//! axia-fand's hardware side: finds the CPU temperature and the fans it may drive, and writes
//! their duty. Everything is found under a sysfs root (normally "", so the real /sys; the tests
//! build a fake tree). Paths never come from a client: a client only picks a profile, and the
//! daemon drives every fan it found.
//!
//! Kinds of fan:
//!   Board  the Raspberry Pi 5 fan header (hwmon "pwmfan", cooling device "pwm-fan"): the official
//!          Active Cooler and case fans plugged there (the Argon ONE V5 case's blower too). pwm1 is
//!          written directly; the kernel's active trip points for this fan are raised to a
//!          backstop (`BACKSTOP_C`) while we drive it, and put back when we stop.
//!   Hwmon  a motherboard Super I/O or fan controller with pwmN + pwmN_enable (allow-listed
//!          drivers). pwmN_enable is set to 1 (manual) while we drive it and put back after.
//! Fans that only report fanN_input (laptops, GPUs, unknown drivers) are listed as "managed by
//! firmware" and never written.
use crate::curve::KICK_BELOW;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

pub const MAX_FANS: usize = 8;
pub const MAX_RO: usize = 8;
pub const MAX_TRIPS: usize = 8;
/// Longest path kept (a longer one is never used).
const PATH_MAX: usize = 320;
/// Kernel trips while we drive the board fan: 78, 80, 82, 84 °C.
pub const BACKSTOP_C: i32 = 78;
/// What `temp()` gives when the temperature cannot be read.
pub const TEMP_NONE: i32 = -1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Board,
    Hwmon,
}

impl Kind {
    pub fn name(self) -> &'static str {
        if self == Kind::Board { "board" } else { "hwmon" }
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub kind: Kind,
    /// The hwmon name.
    pub chip: String,
    /// .../hwmonN
    pub dir: String,
    /// The realpath of hwmonN/device (the identity across restarts).
    pub dev: String,
    /// pwmN
    pub ch: i32,
    pub has_rpm: bool,
    /// Hwmon: pwmN_enable before we took it (-1 unknown).
    pub orig_enable: i32,
    pub taken: bool,
    /// The last duty written, -1 none.
    pub last_pct: i32,
    /// One tick at 100 % before a low duty.
    pub kicking: bool,
}

#[derive(Clone, Debug)]
pub struct Ro {
    pub chip: String,
    /// fanN_input
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct FanHw {
    pub root: String,
    pub t: Vec<Target>,
    pub ro: Vec<Ro>,
    pub temp_path: String,
    pub temp_src: String,
    /// Board: .../thermal_zoneN of the kernel trips bound to its cooling device.
    pub zone: String,
    /// .../cooling_deviceM (type pwm-fan)
    pub cdev: String,
    /// The trip indexes and their original temperatures (milli-°C), ordered by temperature.
    pub trip: Vec<i32>,
    pub trip_orig: Vec<i32>,
    pub trips_raised: bool,
    /// Raspberry Pi 5 (device-tree model).
    pub board: bool,
}

// ---- small sysfs helpers ----

/// Up to `n - 1` bytes of the file, without trailing newlines, blanks or NULs.
fn read_str(path: &str, n: usize) -> Option<Vec<u8>> {
    let f = OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path).ok()?;
    let mut b = Vec::new();
    f.take(n as u64 - 1).read_to_end(&mut b).ok()?;
    while matches!(b.last(), Some(b'\n' | b' ' | 0)) {
        b.pop();
    }
    Some(b)
}

fn read_int(path: &str) -> Option<i32> {
    let b = read_str(path, 32)?;
    let s = std::str::from_utf8(&b).ok()?.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
    if s.is_empty() {
        return None;
    }
    i32::try_from(s.parse::<i64>().ok()?).ok()
}

fn write_int(path: &str, v: i32) -> bool {
    // O_TRUNC: a no-op on sysfs, needed for the test tree
    match OpenOptions::new().write(true).truncate(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path) {
        Ok(mut f) => f.write_all(v.to_string().as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// sysfs names end up in JSON: keep only [A-Za-z0-9_.-].
fn clean(b: &[u8]) -> String {
    b.iter().map(|&c| if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.') { c as char } else { '_' }).collect()
}

fn exists(p: &str) -> bool {
    !p.is_empty() && fs::symlink_metadata(p).is_ok() && fs::metadata(p).is_ok()
}

/// A path that does not fit is empty: a truncated path is never used.
fn path(s: String) -> String {
    if s.len() >= PATH_MAX { String::new() } else { s }
}

/// hwmonN, thermal_zoneN, cooling_deviceN: the prefix followed only by digits.
fn numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_digit()))
}

/// The names in a directory that are `prefix` + digits, in numeric order (readdir order is not
/// stable).
fn list(dir: &str, prefix: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| numbered(n, prefix)).collect())
        .unwrap_or_default();
    v.sort_by_key(|n| (n[prefix.len()..].parse::<u64>().unwrap_or(u64::MAX), n.clone()));
    v
}

/// Super I/O and fan controller drivers whose pwmN_enable=1 means "manual" and whose fans are case
/// or CPU fans. GPU drivers and laptop embedded controllers are not here: their fans stay with
/// the firmware.
fn chip_allowed(chip: &str) -> bool {
    ["nct6", "nct7", "it8", "f71", "f8000", "w83", "sch5", "emc2", "adt7", "amc6821", "max6650", "max31790"].iter().any(|p| chip.starts_with(p))
}

fn to_pwm(pct: i32) -> i32 {
    if pct <= 0 { 0 } else if pct >= 100 { 255 } else { (pct * 255 + 50) / 100 }
}

fn to_pct(pwm: i32) -> i32 {
    if pwm <= 0 { 0 } else if pwm >= 255 { 100 } else { (pwm * 100 + 127) / 255 }
}

impl FanHw {
    /// Finds the temperature source and the fans. `root`: "" for the real system. The number of
    /// drivable fans is `t.len()`.
    pub fn scan(root: &str) -> FanHw {
        let mut h = FanHw { root: root.to_string(), ..FanHw::default() };
        if let Some(m) = read_str(&path(format!("{root}/proc/device-tree/model")), 128) {
            h.board = m.windows(14).any(|w| w == b"Raspberry Pi 5");
        }
        h.find_temp();
        let base = path(format!("{root}/sys/class/hwmon"));
        let mut names = list(&base, "hwmon");
        names.retain(|n| n.len() < 24);
        names.truncate(32);
        for name in names {
            let dir = path(format!("{base}/{name}"));
            let Some(chip) = read_str(&path(format!("{dir}/name")), 32) else { continue };
            let chip = clean(&chip);
            let board = chip == "pwmfan";
            let allowed = board || chip_allowed(&chip);
            for ch in 1..=8 {
                let pwm = path(format!("{dir}/pwm{ch}"));
                let en = path(format!("{dir}/pwm{ch}_enable"));
                let rpm = path(format!("{dir}/fan{ch}_input"));
                let has_pwm = exists(&pwm) && exists(&en);
                if allowed && has_pwm && h.t.len() < MAX_FANS {
                    let dev = fs::canonicalize(path(format!("{dir}/device"))).ok().and_then(|p| p.into_os_string().into_string().ok()).map(path).filter(|p| !p.is_empty()).unwrap_or_else(|| dir.clone());
                    h.t.push(Target {
                        kind: if board { Kind::Board } else { Kind::Hwmon },
                        chip: chip.clone(),
                        dir: dir.clone(),
                        dev,
                        ch,
                        has_rpm: exists(&rpm),
                        orig_enable: -1,
                        taken: false,
                        last_pct: -1,
                        kicking: false,
                    });
                    if board {
                        h.find_board_trips();
                    }
                } else if exists(&rpm) && h.ro.len() < MAX_RO {
                    h.ro.push(Ro { chip: chip.clone(), path: rpm });
                }
            }
        }
        h
    }

    fn find_temp(&mut self) {
        let base = path(format!("{}/sys/class/thermal", self.root));
        let mut zone0 = String::new();
        // 1. thermal zones of the CPU package
        for name in list(&base, "thermal_zone") {
            let Some(ty) = read_str(&path(format!("{base}/{name}/type")), 32) else { continue };
            let ty = clean(&ty);
            if name == "thermal_zone0" {
                zone0 = path(format!("{base}/{name}/temp"));
            }
            if matches!(ty.as_str(), "cpu-thermal" | "x86_pkg_temp" | "cpu_thermal") {
                self.temp_path = path(format!("{base}/{name}/temp"));
                self.temp_src = ty;
                break;
            }
        }
        if !self.temp_path.is_empty() {
            return;
        }
        // 2. CPU sensors in hwmon
        let base = path(format!("{}/sys/class/hwmon", self.root));
        for name in list(&base, "hwmon") {
            let Some(ty) = read_str(&path(format!("{base}/{name}/name")), 32) else { continue };
            let ty = String::from_utf8_lossy(&ty).into_owned();
            if matches!(ty.as_str(), "k10temp" | "coretemp" | "zenpower" | "cpu_thermal") {
                self.temp_path = path(format!("{base}/{name}/temp1_input"));
                self.temp_src = ty;
                break;
            }
        }
        if !self.temp_path.is_empty() {
            return;
        }
        // 3. whatever thermal_zone0 is
        if !zone0.is_empty() {
            self.temp_path = zone0;
            self.temp_src = "thermal_zone0".into();
        }
    }

    /// The cooling device of type "pwm-fan" and the active trips of the zone bound to it.
    fn find_board_trips(&mut self) {
        let base = path(format!("{}/sys/class/thermal", self.root));
        let mut cdev_name = String::new();
        for name in list(&base, "cooling_device") {
            if read_str(&path(format!("{base}/{name}/type")), 64).as_deref() == Some(b"pwm-fan") {
                self.cdev = path(format!("{base}/{name}"));
                cdev_name = name;
                break;
            }
        }
        if cdev_name.is_empty() {
            return;
        }
        for zone in list(&base, "thermal_zone") {
            if !self.trip.is_empty() {
                break;
            }
            let zdir = path(format!("{base}/{zone}"));
            for f in list(&zdir, "cdev") {
                let Ok(link) = fs::read_link(path(format!("{zdir}/{f}"))) else { continue };
                if link.file_name().and_then(|n| n.to_str()) != Some(cdev_name.as_str()) {
                    continue;
                }
                let Some(trip) = read_int(&path(format!("{zdir}/{f}_trip_point"))).filter(|t| (0..=63).contains(t)) else { continue };
                if read_str(&path(format!("{zdir}/trip_point_{trip}_type")), 64).as_deref() != Some(b"active") {
                    continue;
                }
                let Some(v) = read_int(&path(format!("{zdir}/trip_point_{trip}_temp"))) else { continue };
                if self.trip.contains(&trip) || self.trip.len() >= MAX_TRIPS {
                    continue;
                }
                self.trip.push(trip);
                self.trip_orig.push(v);
            }
            if !self.trip.is_empty() {
                self.zone = zdir;
            }
        }
        // sorted by original temperature (the backstop keeps their order); stable
        let mut both: Vec<(i32, i32)> = self.trip_orig.iter().copied().zip(self.trip.iter().copied()).collect();
        both.sort_by_key(|b| b.0);
        self.trip_orig = both.iter().map(|b| b.0).collect();
        self.trip = both.iter().map(|b| b.1).collect();
    }

    /// The CPU temperature in milli-°C, or `TEMP_NONE` if it cannot be read.
    pub fn temp(&self) -> i32 {
        if self.temp_path.is_empty() {
            return TEMP_NONE;
        }
        read_int(&self.temp_path).filter(|v| (-50000..=200000).contains(v)).unwrap_or(TEMP_NONE)
    }

    fn tpath(&self, i: usize, what: &str) -> String {
        let t = &self.t[i];
        path(format!("{}/pwm{}{what}", t.dir, t.ch))
    }

    fn trip_path(&self, i: usize) -> String {
        path(format!("{}/trip_point_{}_temp", self.zone, self.trip[i]))
    }

    /// Takes manual control of every fan (hwmon: pwmN_enable=1; board: raise the trips). Writes
    /// what it changed to `state_path` first (if any) so `axia-fand --restore` can undo it after
    /// a crash. The number of fans taken.
    pub fn take(&mut self, state_path: Option<&str>) -> usize {
        // 1. the restore notes first, so a crash right after leaves enough to undo it
        if let Some(sp) = state_path {
            let tmp = path(format!("{sp}.tmp"));
            let f = if tmp.is_empty() { None } else { OpenOptions::new().write(true).create(true).truncate(true).mode(0o666).custom_flags(libc::O_CLOEXEC).open(&tmp).ok() };
            if let Some(mut f) = f {
                let mut text = String::from("axia-fand 1\n");
                for i in 0..self.t.len() {
                    if self.t[i].kind == Kind::Hwmon {
                        if let Some(v) = read_int(&self.tpath(i, "_enable")) {
                            self.t[i].orig_enable = v;
                        }
                    }
                    let t = &self.t[i];
                    if t.kind == Kind::Hwmon && t.orig_enable >= 0 && !t.dev.contains([' ', '\n']) {
                        text.push_str(&format!("enable {} {} {}\n", t.ch, t.orig_enable, t.dev));
                    }
                }
                for i in 0..self.trip.len() {
                    text.push_str(&format!("trip {} {}\n", self.trip[i], self.trip_orig[i]));
                }
                if f.write_all(text.as_bytes()).is_ok() && f.flush().is_ok() {
                    drop(f);
                    let _ = fs::rename(&tmp, sp);
                } else {
                    drop(f);
                    let _ = fs::remove_file(&tmp);
                }
            }
        }
        let mut ok = 0;
        for i in 0..self.t.len() {
            if self.t[i].kind == Kind::Hwmon {
                let p = self.tpath(i, "_enable");
                if self.t[i].orig_enable < 0 {
                    if let Some(v) = read_int(&p) {
                        self.t[i].orig_enable = v;
                    }
                }
                if !write_int(&p, 1) {
                    eprintln!("axia-fand: {p}: manual mode refused");
                    continue;
                }
            }
            self.t[i].taken = true;
            ok += 1;
        }
        // the board fan: move the kernel's trips up to the backstop, so it does not fight our
        // writes but still spins the fan up by itself if we ever stop answering above it
        for i in 0..self.trip.len() {
            let want = (BACKSTOP_C + 2 * i as i32) * 1000;
            if self.trip_orig[i] < want && write_int(&self.trip_path(i), want) {
                self.trips_raised = true;
            }
        }
        ok
    }

    /// The duty read back, -1 unknown.
    pub fn pct(&self, i: usize) -> i32 {
        if i >= self.t.len() {
            return -1;
        }
        read_int(&self.tpath(i, "")).map_or(-1, to_pct)
    }

    /// Duty 0..100 to fan `i` (already through `output_pct`). Rewrites only when the value read
    /// back differs, so a kernel step in between is corrected on the next tick. `false` if the
    /// fan is not ours or could not be written.
    pub fn set(&mut self, i: usize, pct: i32) -> bool {
        if i >= self.t.len() || !self.t[i].taken {
            return false;
        }
        let p = self.tpath(i, "");
        let Some(cur) = read_int(&p) else { return false };
        let mut want = to_pwm(pct);
        // a stopped fan given a low duty may not start: one tick at full speed first
        if cur == 0 && pct > 0 && pct < KICK_BELOW {
            want = 255;
            self.t[i].kicking = true;
        } else {
            self.t[i].kicking = false;
        }
        self.t[i].last_pct = pct;
        cur == want || write_int(&p, want)
    }

    /// -1 if the fan has no tachometer.
    pub fn rpm(&self, i: usize) -> i32 {
        if i >= self.t.len() || !self.t[i].has_rpm {
            return -1;
        }
        read_int(&path(format!("{}/fan{}_input", self.t[i].dir, self.t[i].ch))).filter(|v| *v >= 0).unwrap_or(-1)
    }

    pub fn ro_rpm(&self, i: usize) -> i32 {
        if i >= self.ro.len() {
            return -1;
        }
        read_int(&self.ro[i].path).filter(|v| *v >= 0).unwrap_or(-1)
    }

    /// The board fan's cooling state for the temperature under the original trips, written to the
    /// cooling device (which sets the duty from the device tree's cooling levels).
    fn board_state_now(&self) {
        if self.cdev.is_empty() {
            return;
        }
        let temp = self.temp();
        let Some(max) = read_int(&path(format!("{}/max_state", self.cdev))).filter(|m| *m >= 0) else { return };
        let st = self.trip_orig.iter().filter(|&&o| temp != TEMP_NONE && temp >= o).count() as i32;
        write_int(&path(format!("{}/cur_state", self.cdev)), st.min(max));
    }

    /// Gives every fan back to the kernel or firmware: the original pwmN_enable and trip points,
    /// and the board fan's cooling state for the current temperature.
    pub fn release(&mut self) {
        for i in 0..self.trip.len() {
            write_int(&self.trip_path(i), self.trip_orig[i]);
        }
        self.trips_raised = false;
        self.board_state_now();
        for i in 0..self.t.len() {
            if self.t[i].kind == Kind::Hwmon && self.t[i].taken && self.t[i].orig_enable >= 0 {
                write_int(&self.tpath(i, "_enable"), self.t[i].orig_enable);
            }
            self.t[i].taken = false;
        }
    }
}

/// `axia-fand --restore` after a crash: rescans and restores what `state_path` says (validated:
/// only values, matched to the fans found by the scan). Then removes the file. `Err` if the notes
/// were bad or missing while the trips look raised.
pub fn restore_file(root: &str, state_path: &str) -> Result<(), ()> {
    let mut h = FanHw::scan(root);
    let text = fs::File::open(state_path).ok().and_then(|mut f| {
        let mut b = Vec::new();
        f.read_to_end(&mut b).ok().map(|_| b)
    });
    let have_file = text.is_some();
    let mut rc = Ok(());
    if let Some(text) = text {
        let mut lines = text.split_inclusive(|&c| c == b'\n');
        if lines.next() != Some(b"axia-fand 1\n") {
            rc = Err(());
        }
        let notes: Vec<&[u8]> = if rc.is_ok() { lines.collect() } else { Vec::new() };
        for line in notes {
            let (a, b, dev) = match parse_note(line) {
                Some(n) => n,
                None => continue,
            };
            match dev {
                None => {
                    if !(0..=200000).contains(&b) {
                        continue;
                    }
                    for i in 0..h.trip.len() {
                        if h.trip[i] == a {
                            h.trip_orig[i] = b;
                        }
                    }
                }
                Some(dev) => {
                    if !(0..=5).contains(&b) {
                        continue;
                    }
                    for t in h.t.iter_mut() {
                        if t.kind == Kind::Hwmon && t.ch == a && t.dev == dev {
                            t.orig_enable = b;
                            t.taken = true;
                        }
                    }
                }
            }
        }
    }
    // the trips go back even without notes: never leave them at the backstop
    if !h.trip.is_empty() {
        let raised = h.trip_orig.iter().all(|&o| o >= BACKSTOP_C * 1000);
        if raised && !have_file {
            rc = Err(()); // no notes and the trips look raised: the original values are unknown
        }
    }
    if have_file || h.trip.is_empty() {
        h.release();
    }
    let _ = fs::remove_file(state_path);
    rc
}

/// "trip <index> <milli-°C>\n" or "enable <pwm> <value> <device>\n" exactly (nothing after the
/// newline's place, a device of at most 319 characters): (a, b, device).
fn parse_note(line: &[u8]) -> Option<(i32, i32, Option<String>)> {
    if line.len() > 383 {
        return None;
    }
    let body = line.strip_suffix(b"\n")?;
    if body.last().is_some_and(|c| c.is_ascii_whitespace()) {
        return None;
    }
    let s = std::str::from_utf8(body).ok()?;
    let mut w = s.split_ascii_whitespace();
    match w.next()? {
        "trip" => {
            let (a, b) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?);
            w.next().is_none().then_some((a, b, None))
        }
        "enable" => {
            let (a, b) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?);
            let dev = w.next()?;
            (w.next().is_none() && dev.len() <= 319).then(|| (a, b, Some(dev.to_string())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_read_exactly() {
        assert_eq!(parse_note(b"trip 1 50000\n"), Some((1, 50000, None)));
        assert_eq!(parse_note(b"enable 2 5 /sys/devices/x\n"), Some((2, 5, Some("/sys/devices/x".into()))));
        for bad in ["trip 1\n", "trip 1 2 3\n", "trip a b\n", "exec /bin/sh\n", "trip 1 2", "trip 1 2 \n", "enable 1 2\n", "enable 1 2 a b\n", ""] {
            assert_eq!(parse_note(bad.as_bytes()), None, "{bad:?}");
        }
        assert_eq!(parse_note(format!("enable 1 2 {}\n", "d".repeat(320)).as_bytes()), None);
    }

    #[test]
    fn duty_conversions() {
        assert_eq!((to_pwm(0), to_pwm(30), to_pwm(60), to_pwm(100), to_pwm(150)), (0, 77, 153, 255, 255));
        assert_eq!((to_pct(0), to_pct(77), to_pct(255), to_pct(300), to_pct(-3)), (0, 30, 100, 100, 0));
        assert!(numbered("hwmon12", "hwmon") && !numbered("hwmon", "hwmon") && !numbered("hwmon1a", "hwmon") && !numbered("cdev1_trip_point", "cdev"));
        assert_eq!(clean(b"a b/c\xc3\xa9-d.e_"), "a_b_c__-d.e_");
        assert!(chip_allowed("nct6798") && chip_allowed("it8728") && !chip_allowed("amdgpu") && !chip_allowed("thinkpad"));
    }
}
