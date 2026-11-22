//! Batteries and chargers from /sys/class/power_supply (Settings → Battery, the menu bar percentage
//! and the low-battery notice). Read-only and unprivileged. The root directory is a parameter so the
//! tests can use a fake sysfs tree. The structures have the C layout of `power.h`.
use crate::{cstr, put_c, quote};
use std::ffi::c_int;
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

pub const POWER_MAX: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PowerSupply {
    /// "BAT0", "AC", "hidpp_battery_0".
    pub name: [u8; 32],
    /// type Battery (else a charger: Mains, USB …).
    pub battery: c_int,
    /// scope Device: a mouse or keyboard battery, not the system's.
    pub device_scope: c_int,
    /// Chargers: plugged in.
    pub online: c_int,
    pub present: c_int,
    /// 0..100, -1 unknown.
    pub percent: c_int,
    /// Charging, Discharging, Full, Not charging, Unknown.
    pub status: [u8; 16],
    /// To empty (discharging) or to full (charging); -1 unknown.
    pub minutes_left: c_int,
    /// Full / design capacity, percent; -1 unknown.
    pub health: c_int,
    /// -1 unknown.
    pub cycles: c_int,
    /// Current draw, -1 unknown.
    pub watts: f64,
    pub model: [u8; 48],
    pub vendor: [u8; 48],
    pub technology: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PowerState {
    pub s: [PowerSupply; POWER_MAX],
    pub n: c_int,
    /// Any charger online (or no system battery).
    pub on_ac: c_int,
    /// A system battery (scope != Device).
    pub has_battery: c_int,
    /// System batteries combined, -1 without one.
    pub percent: c_int,
    /// Combined, -1 unknown.
    pub minutes_left: c_int,
    pub charging: c_int,
}

impl PowerSupply {
    pub fn name(&self) -> &str {
        std::str::from_utf8(cstr(&self.name)).unwrap_or("")
    }
    pub fn status(&self) -> &str {
        std::str::from_utf8(cstr(&self.status)).unwrap_or("")
    }
    pub fn model(&self) -> &str {
        std::str::from_utf8(cstr(&self.model)).unwrap_or("")
    }
    pub fn technology(&self) -> &str {
        std::str::from_utf8(cstr(&self.technology)).unwrap_or("")
    }
    fn zeroed() -> PowerSupply {
        // SAFETY: only integers, floats and byte arrays: zero is valid.
        unsafe { std::mem::zeroed() }
    }
}

impl PowerState {
    pub fn zeroed() -> PowerState {
        // SAFETY: as above.
        unsafe { std::mem::zeroed() }
    }
    /// The supplies read.
    pub fn supplies(&self) -> &[PowerSupply] {
        &self.s[..(self.n.max(0) as usize).min(POWER_MAX)]
    }
}

/// One attribute file, trimmed of trailing newlines and blanks (at most `n - 1` bytes). `None` if it cannot
/// be read.
fn attr(root: &str, dev: &str, name: &str, n: usize) -> Option<Vec<u8>> {
    let f = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(format!("{root}/{dev}/{name}")).ok()?;
    let mut b = Vec::new();
    f.take(n as u64 - 1).read_to_end(&mut b).ok()?;
    while matches!(b.last(), Some(b'\n' | b' ')) {
        b.pop();
    }
    Some(b)
}

/// A number in an attribute: the whole text must be one (strtoll); -1 when it is missing or not.
fn num(root: &str, dev: &str, name: &str) -> i64 {
    let Some(b) = attr(root, dev, name, 32).filter(|b| !b.is_empty()) else { return -1 };
    let t = String::from_utf8_lossy(cstr(&b)).into_owned();
    let t = t.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
    t.parse::<i64>().unwrap_or(-1)
}

fn read_one(root: &str, dev: &str) -> PowerSupply {
    let mut s = PowerSupply::zeroed();
    put_c(&mut s.name, dev.as_bytes());
    let ty = attr(root, dev, "type", 32).unwrap_or_default();
    let scope = attr(root, dev, "scope", 32).unwrap_or_default();
    s.battery = (cstr(&ty) == b"Battery") as c_int;
    s.device_scope = (cstr(&scope) == b"Device") as c_int;
    let (online, present) = (num(root, dev, "online"), num(root, dev, "present"));
    s.online = (online > 0) as c_int;
    s.present = (present != 0) as c_int; // a missing file = present
    s.percent = -1;
    s.minutes_left = -1;
    s.health = -1;
    s.cycles = -1;
    s.watts = -1.0;
    if s.battery == 0 {
        return s;
    }
    match attr(root, dev, "status", 16) {
        Some(b) => put_c(&mut s.status, &b),
        None => put_c(&mut s.status, b"Unknown"),
    }
    put_c(&mut s.model, &attr(root, dev, "model_name", 48).unwrap_or_default());
    put_c(&mut s.vendor, &attr(root, dev, "manufacturer", 48).unwrap_or_default());
    put_c(&mut s.technology, &attr(root, dev, "technology", 16).unwrap_or_default());
    let cap = num(root, dev, "capacity");
    // energy_* in µWh with power_now in µW, or charge_* in µAh with current_now in µA
    let (mut now, mut full, mut design, mut rate) = (num(root, dev, "energy_now"), num(root, dev, "energy_full"), num(root, dev, "energy_full_design"), num(root, dev, "power_now"));
    if now < 0 {
        now = num(root, dev, "charge_now");
        full = num(root, dev, "charge_full");
        design = num(root, dev, "charge_full_design");
        rate = num(root, dev, "current_now");
        let uv = num(root, dev, "voltage_now");
        if rate >= 0 && uv > 0 {
            s.watts = rate as f64 * uv as f64 / 1e12;
        }
    } else if rate >= 0 {
        s.watts = rate as f64 / 1e6;
    }
    if (0..=100).contains(&cap) {
        s.percent = cap as c_int;
    } else if now >= 0 && full > 0 {
        s.percent = (now * 100 / full) as c_int;
    }
    s.percent = s.percent.min(100);
    if full > 0 && design > 0 {
        s.health = (full * 100 / design) as c_int;
    }
    s.health = s.health.min(100);
    s.cycles = num(root, dev, "cycle_count") as c_int;
    if s.cycles == 0 {
        s.cycles = -1; // many firmwares report 0 for "unknown"
    }
    let (tte, ttf) = (num(root, dev, "time_to_empty_now"), num(root, dev, "time_to_full_now"));
    match s.status() {
        "Discharging" => {
            if tte > 0 {
                s.minutes_left = (tte / 60) as c_int;
            } else if rate > 0 && now >= 0 {
                s.minutes_left = (now * 60 / rate) as c_int;
            }
        }
        "Charging" => {
            if ttf > 0 {
                s.minutes_left = (ttf / 60) as c_int;
            } else if rate > 0 && full > now && now >= 0 {
                s.minutes_left = ((full - now) * 60 / rate) as c_int;
            }
        }
        _ => {}
    }
    s
}

/// Reads every supply under `root` ("/sys/class/power_supply"), at most `POWER_MAX`, in name order.
pub fn read(root: &str) -> PowerState {
    let mut ps = PowerState::zeroed();
    ps.percent = -1;
    ps.minutes_left = -1;
    let mut names: Vec<String> = fs::read_dir(root).map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    names.retain(|n| !n.starts_with('.') && n.len() < 32);
    names.sort();
    names.truncate(POWER_MAX);
    for n in &names {
        ps.s[ps.n as usize] = read_one(root, n);
        ps.n += 1;
    }
    let (mut sum, mut cnt, mut mins, mut have_mins, mut any_charger) = (0i64, 0i64, 0i64, false, false);
    let supplies = ps.s;
    for s in &supplies[..ps.n as usize] {
        if s.battery == 0 {
            any_charger = true;
            if s.online != 0 {
                ps.on_ac = 1;
            }
            continue;
        }
        if s.device_scope != 0 || s.present == 0 {
            continue;
        }
        ps.has_battery = 1;
        if s.percent >= 0 {
            sum += s.percent as i64;
            cnt += 1;
        }
        if s.minutes_left >= 0 {
            mins += s.minutes_left as i64;
            have_mins = true;
        }
        if s.status() == "Charging" {
            ps.charging = 1;
        }
    }
    if ps.has_battery == 0 {
        ps.on_ac = 1;
    } else if !any_charger {
        ps.on_ac = ps.charging;
    }
    if cnt != 0 {
        ps.percent = (sum / cnt) as c_int;
    }
    if have_mins {
        ps.minutes_left = mins as c_int;
    }
    ps
}

/// The JSON of `lord.power.state()`.
pub fn json(ps: &PowerState) -> Vec<u8> {
    let b = |v: c_int| if v != 0 { "true" } else { "false" };
    let mut o: Vec<u8> = format!(
        "{{\"onAC\":{},\"hasBattery\":{},\"percent\":{},\"minutesLeft\":{},\"charging\":{},\"supplies\":[",
        b(ps.on_ac),
        b(ps.has_battery),
        ps.percent,
        ps.minutes_left,
        b(ps.charging)
    )
    .into_bytes();
    for (i, s) in ps.supplies().iter().enumerate() {
        o.extend_from_slice(if i > 0 { b",{\"name\":" } else { b"{\"name\":" });
        quote(cstr(&s.name), &mut o);
        o.extend_from_slice(format!(",\"battery\":{},\"device\":{},\"online\":{},\"present\":{},\"percent\":{},\"status\":", b(s.battery), b(s.device_scope), b(s.online), b(s.present), s.percent).as_bytes());
        quote(cstr(&s.status), &mut o);
        o.extend_from_slice(format!(",\"minutesLeft\":{},\"health\":{},\"cycles\":{},\"watts\":{:.2},\"model\":", s.minutes_left, s.health, s.cycles, s.watts).as_bytes());
        quote(cstr(&s.model), &mut o);
        o.extend_from_slice(b",\"vendor\":");
        quote(cstr(&s.vendor), &mut o);
        o.extend_from_slice(b",\"technology\":");
        quote(cstr(&s.technology), &mut o);
        o.push(b'}');
    }
    o.extend_from_slice(b"]}");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c_layout_is_the_one_power_h_has() {
        assert_eq!(size_of::<PowerSupply>(), 200);
        assert_eq!(size_of::<PowerState>(), 8 * 200 + 24);
    }
}
