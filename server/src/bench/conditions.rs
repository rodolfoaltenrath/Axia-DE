//! Read-only machine conditions and thermal/power guards. Shared firmware
//! discovery remains supplied by the hardware subsystem through its existing ABI.
use libc::{c_char, c_int, c_uint, c_ulong};
use std::{
    ffi::{CStr, CString, OsStr},
    io::Read,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
#[repr(C)]
pub struct LbCond {
    pub temp_c: f64,
    pub passive_c: f64,
    pub critical_c: f64,
    pub zone: [c_char; 24],
    pub throttle_events: c_ulong,
    pub throttle_known: c_int,
    pub rpi: c_int,
    pub rpi_throttled: c_int,
    pub rpi_uv_alarm: c_int,
    pub freq_mhz: f64,
    pub freq_max_mhz: f64,
    pub governor: [c_char; 24],
    pub platform_profile: [c_char; 24],
    pub on_battery: c_int,
    pub battery_pct: c_int,
}
impl Default for LbCond {
    fn default() -> Self {
        Self {
            temp_c: -273.,
            passive_c: 0.,
            critical_c: 0.,
            zone: [0; 24],
            throttle_events: 0,
            throttle_known: 0,
            rpi: 0,
            rpi_throttled: -1,
            rpi_uv_alarm: -1,
            freq_mhz: 0.,
            freq_max_mhz: 0.,
            governor: [0; 24],
            platform_profile: [0; 24],
            on_battery: -1,
            battery_pct: -1,
        }
    }
}
pub const LB_RPI_UV: u32 = 1;
pub const LB_RPI_CAPPED: u32 = 2;
pub const LB_RPI_THROTTLED: u32 = 4;
pub const LB_RPI_SOFTTEMP: u32 = 8;
pub const LB_RPI_HOT: u32 = 16;
pub const LB_RPI_NOW_MASK: u32 = 0xf;
pub const LB_RPI_STICKY_MASK: u32 = 0xf0000;
pub const LB_RPI_SLOWED: u32 = LB_RPI_CAPPED | LB_RPI_THROTTLED | LB_RPI_SOFTTEMP | LB_RPI_HOT;
pub const LB_RPI_FW_TEMP_C: f64 = 80.;
#[derive(Default, Debug)]
#[repr(C)]
pub struct LbRpiWatch {
    pub base: c_uint,
    pub seen: c_uint,
    pub rpi: c_int,
    pub bits: c_int,
    pub alarm: c_int,
}
#[derive(Default)]
#[repr(C)]
struct RpiFw {
    hwmon: c_int,
    uv_alarm: c_int,
    throttled: c_int,
}
extern "C" {
    fn rpifw_read(root: *const c_char, fw: *mut RpiFw);
}

struct Root<'a>(&'a Path);
impl Root<'_> {
    fn path(&self, path: &str) -> PathBuf {
        self.0.join(path.trim_start_matches('/'))
    }
    fn read(&self, path: &str, n: usize) -> Option<Vec<u8>> {
        read_line(&self.path(path), n)
    }
    fn number(&self, path: &str, default: i64) -> i64 {
        self.read(path, 64)
            .and_then(|b| decimal(&b))
            .unwrap_or(default)
    }
    fn entries(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(self.path(path))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect()
    }
}
fn read_line(path: &Path, n: usize) -> Option<Vec<u8>> {
    let f = std::fs::File::open(path).ok()?;
    let mut b = Vec::new();
    f.take(n.saturating_sub(1) as u64)
        .read_to_end(&mut b)
        .ok()?;
    b.truncate(
        b.iter()
            .position(|v| *v == b'\n' || *v == 0)
            .unwrap_or(b.len()),
    );
    Some(b)
}
fn decimal(b: &[u8]) -> Option<i64> {
    let s = CString::new(b).ok()?;
    let mut end = std::ptr::null_mut();
    let n = unsafe { libc::strtol(s.as_ptr(), &mut end, 10) };
    if end == s.as_ptr().cast_mut() {
        None
    } else {
        Some(n as i64)
    }
}
fn copy<const N: usize>(out: &mut [c_char; N], bytes: &[u8]) {
    out.fill(0);
    for (out, b) in out.iter_mut().take(N.saturating_sub(1)).zip(bytes) {
        *out = *b as c_char;
    }
}
fn sanitize(out: &mut [c_char], zone: bool) {
    for b in out.iter_mut().take_while(|b| **b != 0) {
        let v = *b as u8;
        if !(v.is_ascii_lowercase()
            || (zone && v.is_ascii_alphanumeric())
            || v == b'-'
            || v == b'_')
        {
            *b = b'_' as c_char;
        }
    }
}
fn lower_trip(current: &mut f64, v: i64) {
    if (30000..=130000).contains(&v) && (*current == 0. || (v as f64 / 1000.) < *current) {
        *current = v as f64 / 1000.;
    }
}
fn thermal(root: &Root<'_>, c: &mut LbCond) {
    for name in root.entries("/sys/class/thermal") {
        if !name.starts_with("thermal_zone") || name.len() > 20 {
            continue;
        }
        let base = format!("/sys/class/thermal/{name}");
        let t = root.number(&format!("{base}/temp"), i64::MIN);
        if !(-50000..=150000).contains(&t) {
            continue;
        }
        if t as f64 / 1000. > c.temp_c {
            c.temp_c = t as f64 / 1000.;
            copy(
                &mut c.zone,
                &root.read(&format!("{base}/type"), 24).unwrap_or_default(),
            );
        }
        for i in 0..16 {
            let Some(kind) = root.read(&format!("{base}/trip_point_{i}_type"), 24) else {
                break;
            };
            let v = root.number(&format!("{base}/trip_point_{i}_temp"), 0);
            if kind == b"passive" {
                lower_trip(&mut c.passive_c, v);
            }
            if kind == b"critical" {
                lower_trip(&mut c.critical_c, v);
            }
        }
    }
}
fn hwmon(root: &Root<'_>, c: &mut LbCond) {
    for dir in root.entries("/sys/class/hwmon") {
        if !dir.starts_with("hwmon") || dir.len() > 10 {
            continue;
        }
        let base = format!("/sys/class/hwmon/{dir}");
        let Some(name) = root.read(&format!("{base}/name"), 24) else {
            continue;
        };
        if ![b"coretemp".as_slice(), b"k10temp", b"zenpower"].contains(&name.as_slice()) {
            continue;
        }
        for i in 1..64 {
            let t = root.number(&format!("{base}/temp{i}_input"), i64::MIN);
            if t == i64::MIN {
                if i > 2 {
                    break;
                }
                continue;
            }
            if !(-50000..=150000).contains(&t) {
                continue;
            }
            if t as f64 / 1000. > c.temp_c {
                c.temp_c = t as f64 / 1000.;
                copy(&mut c.zone, &name);
            }
            lower_trip(
                &mut c.critical_c,
                root.number(&format!("{base}/temp{i}_crit"), 0),
            );
            lower_trip(
                &mut c.passive_c,
                root.number(&format!("{base}/temp{i}_max"), 0),
            );
        }
    }
}
fn cpus(root: &Root<'_>, c: &mut LbCond) {
    let mut sum = 0.;
    let mut n = 0;
    for name in root.entries("/sys/devices/system/cpu") {
        let Some(id) = name.strip_prefix("cpu") else {
            continue;
        };
        if id.is_empty() || name.len() > 12 || !id.bytes().all(|v| v.is_ascii_digit()) {
            continue;
        }
        let base = format!("/sys/devices/system/cpu/{name}");
        let f = root.number(&format!("{base}/cpufreq/scaling_cur_freq"), 0);
        if f > 0 {
            sum += f as f64 / 1000.;
            n += 1;
        }
        c.freq_max_mhz = c
            .freq_max_mhz
            .max(root.number(&format!("{base}/cpufreq/cpuinfo_max_freq"), 0) as f64 / 1000.);
        for counter in ["core_throttle_count", "package_throttle_count"] {
            let count = root.number(&format!("{base}/thermal_throttle/{counter}"), -1);
            if count >= 0 {
                c.throttle_events = c.throttle_events.wrapping_add(count as c_ulong);
                c.throttle_known = 1;
            }
        }
    }
    if n > 0 {
        c.freq_mhz = sum / n as f64;
    }
    copy(
        &mut c.governor,
        &root
            .read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", 24)
            .unwrap_or_default(),
    );
}
fn power(root: &Root<'_>, c: &mut LbCond) {
    let mut mains = -1;
    for name in root.entries("/sys/class/power_supply") {
        if name.starts_with('.') || name.len() > 48 {
            continue;
        }
        let base = format!("/sys/class/power_supply/{name}");
        let Some(kind) = root.read(&format!("{base}/type"), 24) else {
            continue;
        };
        if kind == b"Battery" {
            if root.read(&format!("{base}/scope"), 16).as_deref() == Some(b"Device") {
                continue;
            }
            c.battery_pct = root.number(&format!("{base}/capacity"), -1) as c_int;
            if root.read(&format!("{base}/status"), 24).as_deref() == Some(b"Discharging") {
                c.on_battery = 1;
            } else if c.on_battery < 0 {
                c.on_battery = 0;
            }
        } else if kind == b"Mains" || kind == b"USB" {
            if root.number(&format!("{base}/online"), 0) == 1 {
                mains = 1;
            } else if mains < 0 {
                mains = 0;
            }
        }
    }
    if mains == 1 && c.on_battery == 1 && c.battery_pct >= 0 {
        c.on_battery = 0;
    }
    if c.on_battery < 0 && mains >= 0 {
        c.on_battery = 1 - mains;
    }
}
fn conditions(root: &Root<'_>, fw: RpiFw) -> LbCond {
    let mut c = LbCond::default();
    thermal(root, &mut c);
    hwmon(root, &mut c);
    cpus(root, &mut c);
    c.rpi_uv_alarm = fw.uv_alarm;
    c.rpi_throttled = fw.throttled;
    c.rpi = (fw.hwmon != 0 || fw.throttled >= 0) as c_int;
    if c.rpi == 0 {
        if let Ok(f) = std::fs::File::open(root.path("/sys/firmware/devicetree/base/compatible")) {
            let mut b = Vec::new();
            let _ = f.take(255).read_to_end(&mut b);
            c.rpi = b.split(|v| *v == 0).any(|s| s.starts_with(b"raspberrypi,")) as c_int;
        }
    }
    power(root, &mut c);
    copy(
        &mut c.platform_profile,
        &root
            .read("/sys/firmware/acpi/platform_profile", 24)
            .unwrap_or_default(),
    );
    sanitize(&mut c.zone, true);
    sanitize(&mut c.governor, false);
    sanitize(&mut c.platform_profile, false);
    c
}
#[no_mangle]
pub unsafe extern "C" fn lb_cond_read(root: *const c_char, c: *mut LbCond) {
    let Some(c) = c.as_mut() else {
        return;
    };
    let raw = super::bytes(root);
    let root_c = CString::new(raw).unwrap();
    let mut fw = RpiFw::default();
    rpifw_read(root_c.as_ptr(), &mut fw);
    // Empty root means the real filesystem, as in the C hardware readers.
    let path = if raw.is_empty() {
        Path::new("/")
    } else {
        Path::new(OsStr::from_bytes(raw))
    };
    *c = conditions(&Root(path), fw);
}
fn cpu_ticks(root: &Root<'_>) -> Option<(u64, u64)> {
    let b = root.read("/proc/stat", 512)?;
    let line = b.strip_prefix(b"cpu ")?;
    // sscanf accepts decimal prefixes and stops at the first conversion failure.
    let text = CString::new(line).ok()?;
    let mut cursor = text.as_ptr();
    let mut v = Vec::new();
    for _ in 0..10 {
        let mut end = std::ptr::null_mut();
        let n = unsafe { libc::strtoull(cursor, &mut end, 10) };
        if end == cursor.cast_mut() {
            break;
        }
        v.push(n);
        cursor = end;
    }
    if v.len() < 4 {
        return None;
    }
    let total = v.iter().take(8).fold(0u64, |a, b| a.wrapping_add(*b));
    Some((
        total
            .wrapping_sub(v[3])
            .wrapping_sub(v.get(4).copied().unwrap_or(0)),
        total,
    ))
}
#[no_mangle]
pub unsafe extern "C" fn lb_cpu_ticks(
    root: *const c_char,
    busy: *mut u64,
    total: *mut u64,
) -> c_int {
    let raw = super::bytes(root);
    let path = if raw.is_empty() {
        Path::new("/")
    } else {
        Path::new(OsStr::from_bytes(raw))
    };
    let Some((b, t)) = cpu_ticks(&Root(path)) else {
        return -1;
    };
    if let Some(out) = busy.as_mut() {
        *out = b;
    }
    if let Some(out) = total.as_mut() {
        *out = t;
    }
    0
}
fn device_kind(mut real: PathBuf) -> &'static [u8] {
    if let Ok(slaves) = std::fs::read_dir(real.join("slaves")) {
        if let Some(first) = slaves
            .flatten()
            .find(|e| !e.file_name().as_bytes().starts_with(b"."))
        {
            if let Ok(path) = first.path().canonicalize() {
                real = path;
            }
        }
    }
    if real.join("partition").exists() {
        real.pop();
    }
    let path = real.as_os_str().as_bytes();
    let name = real.file_name().unwrap_or_default().as_bytes();
    if path.windows(4).any(|s| s == b"/usb") {
        return b"usb\0";
    }
    if name.starts_with(b"nvme") {
        return b"nvme\0";
    }
    if name.starts_with(b"mmcblk") {
        return b"sd\0";
    }
    if name.starts_with(b"vd") || name.starts_with(b"xvd") {
        return b"virtual\0";
    }
    if name.starts_with(b"sd") {
        let vendor = read_line(&real.join("device/vendor"), 64).unwrap_or_default();
        if [b"QEMU".as_slice(), b"VMware", b"VBOX", b"Msft"]
            .iter()
            .any(|s| vendor.windows(s.len()).any(|v| v == *s))
        {
            return b"virtual\0";
        }
        return match read_line(&real.join("queue/rotational"), 64).and_then(|s| decimal(&s)) {
            Some(1) => b"hdd\0",
            Some(0) => b"ssd\0",
            _ => b"unknown\0",
        };
    }
    b"unknown\0"
}
#[no_mangle]
pub unsafe extern "C" fn lb_storage_kind(
    root: *const c_char,
    path: *const c_char,
) -> *const c_char {
    let path = super::bytes(path);
    let cpath = CString::new(path).unwrap();
    let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if libc::statfs(cpath.as_ptr(), fs.as_mut_ptr()) == 0 {
        let kind = fs.assume_init().f_type;
        if kind == 0x01021994 || kind == 0x794c7630 {
            return b"ram\0".as_ptr().cast();
        }
    }
    let Ok(meta) = std::fs::metadata(OsStr::from_bytes(path)) else {
        return b"unknown\0".as_ptr().cast();
    };
    let major = libc::major(meta.dev());
    let minor = libc::minor(meta.dev());
    if major == 0 {
        return b"unknown\0".as_ptr().cast();
    }
    let raw = super::bytes(root);
    let path = if raw.is_empty() {
        Path::new("/")
    } else {
        Path::new(OsStr::from_bytes(raw))
    };
    match Root(path)
        .path(&format!("/sys/dev/block/{major}:{minor}"))
        .canonicalize()
    {
        Ok(real) => device_kind(real).as_ptr().cast(),
        Err(_) => b"unknown\0".as_ptr().cast(),
    }
}
impl LbRpiWatch {
    fn start(c: &LbCond) -> Self {
        let mut w = Self {
            rpi: c.rpi,
            bits: (c.rpi_throttled >= 0) as c_int,
            alarm: (c.rpi_uv_alarm >= 0) as c_int,
            ..Default::default()
        };
        if c.rpi_throttled >= 0 {
            w.base = (c.rpi_throttled as u32 & LB_RPI_STICKY_MASK) >> 16;
            w.seen = c.rpi_throttled as u32 & LB_RPI_NOW_MASK;
        }
        if c.rpi_uv_alarm == 1 {
            w.base |= LB_RPI_UV << 4;
        }
        if c.rpi != 0 && c.temp_c >= LB_RPI_FW_TEMP_C {
            w.seen |= LB_RPI_HOT;
        }
        w
    }
    fn sample(&mut self, c: &LbCond) -> u32 {
        let mut ev = 0;
        if c.rpi != 0 {
            self.rpi = 1;
        }
        if c.rpi_throttled >= 0 {
            self.bits = 1;
            let v = c.rpi_throttled as u32;
            let sticky = (v & LB_RPI_STICKY_MASK) >> 16;
            ev |= v & LB_RPI_NOW_MASK;
            ev |= sticky & !self.base & 0xf;
            self.base &= !(0xf & !sticky);
        }
        if c.rpi_uv_alarm >= 0 {
            self.alarm = 1;
            if c.rpi_uv_alarm == 1 {
                if self.base & (LB_RPI_UV << 4) == 0 {
                    ev |= LB_RPI_UV;
                }
            } else {
                self.base &= !(LB_RPI_UV << 4);
            }
        }
        if c.rpi != 0 && c.temp_c >= LB_RPI_FW_TEMP_C {
            ev |= LB_RPI_HOT;
        }
        self.seen |= ev;
        ev
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_rpi_watch_start(w: *mut LbRpiWatch, c: *const LbCond) {
    if let (Some(w), Some(c)) = (w.as_mut(), c.as_ref()) {
        *w = LbRpiWatch::start(c);
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_rpi_watch(w: *mut LbRpiWatch, c: *const LbCond) -> c_uint {
    match (w.as_mut(), c.as_ref()) {
        (Some(w), Some(c)) => w.sample(c),
        _ => 0,
    }
}
fn stop(start: &LbCond, now: &LbCond, events: u32, peak: bool) -> Option<&'static CStr> {
    if now.temp_c > -270.
        && ((now.passive_c > 0. && now.temp_c >= now.passive_c - 2.)
            || (now.passive_c <= 0. && now.critical_c > 0. && now.temp_c >= now.critical_c - 10.))
    {
        return Some(c"thermal");
    }
    if peak
        && ((start.throttle_known != 0 && now.throttle_events > start.throttle_events)
            || events & LB_RPI_SLOWED != 0)
    {
        return Some(c"throttled");
    }
    None
}
#[no_mangle]
pub unsafe extern "C" fn lb_cond_stop(
    start: *const LbCond,
    now: *const LbCond,
    events: c_uint,
    peak: c_int,
) -> *const c_char {
    match (start.as_ref(), now.as_ref()) {
        (Some(start), Some(now)) => {
            stop(start, now, events, peak != 0).map_or(std::ptr::null(), |s| s.as_ptr())
        }
        _ => std::ptr::null(),
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_cond_battery_low(c: *const LbCond) -> c_int {
    c.as_ref()
        .is_some_and(|c| c.on_battery == 1 && (0..30).contains(&c.battery_pct)) as c_int
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let mut name = b"/tmp/axia-bench-cond-XXXXXX\0".to_vec();
            let raw = unsafe { libc::mkdtemp(name.as_mut_ptr().cast()) };
            assert!(!raw.is_null());
            Self(PathBuf::from(OsStr::from_bytes(
                unsafe { CStr::from_ptr(raw) }.to_bytes(),
            )))
        }
        fn put(&self, path: &str, text: &str) {
            let p = Root(&self.0).path(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        fn read(&self) -> LbCond {
            conditions(
                &Root(&self.0),
                RpiFw {
                    hwmon: 0,
                    uv_alarm: -1,
                    throttled: -1,
                },
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn sensors_battery_and_cpu_ticks_use_only_the_fixture() {
        let f = Fixture::new();
        for (path, value) in [
            ("/sys/class/thermal/thermal_zone0/temp", "61000\n"),
            ("/sys/class/thermal/thermal_zone0/type", "x86_pkg_temp\n"),
            (
                "/sys/class/thermal/thermal_zone0/trip_point_0_type",
                "passive\n",
            ),
            (
                "/sys/class/thermal/thermal_zone0/trip_point_0_temp",
                "95000\n",
            ),
            (
                "/sys/class/thermal/thermal_zone0/trip_point_1_type",
                "critical\n",
            ),
            (
                "/sys/class/thermal/thermal_zone0/trip_point_1_temp",
                "105000\n",
            ),
            ("/sys/class/power_supply/BAT0/type", "Battery\n"),
            ("/sys/class/power_supply/BAT0/status", "Discharging\n"),
            ("/sys/class/power_supply/BAT0/capacity", "25\n"),
            ("/sys/class/power_supply/mouse/type", "Battery\n"),
            ("/sys/class/power_supply/mouse/scope", "Device\n"),
            ("/sys/class/power_supply/mouse/capacity", "90\n"),
            (
                "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
                "2400000\n",
            ),
            (
                "/sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq",
                "2000000\n",
            ),
            (
                "/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq",
                "4200000\n",
            ),
            (
                "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
                "performance\n",
            ),
            (
                "/sys/devices/system/cpu/cpu0/thermal_throttle/core_throttle_count",
                "3\n",
            ),
            (
                "/sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count",
                "1\n",
            ),
            (
                "/proc/stat",
                "cpu 100 0 50 800 50 0 0 0 100 200\ncpu0 1 2 3\n",
            ),
        ] {
            f.put(path, value);
        }
        let c = f.read();
        assert_eq!((c.temp_c, c.passive_c, c.critical_c), (61., 95., 105.));
        assert_eq!(
            (
                c.freq_mhz,
                c.freq_max_mhz,
                c.throttle_events,
                c.throttle_known
            ),
            (2200., 4200., 4, 1)
        );
        assert_eq!((c.on_battery, c.battery_pct), (1, 25));
        assert_eq!(unsafe { lb_cond_battery_low(&c) }, 1);
        assert_eq!(cpu_ticks(&Root(&f.0)), Some((150, 1000)));
        f.put("/sys/class/power_supply/AC/type", "Mains\n");
        f.put("/sys/class/power_supply/AC/online", "1\n");
        assert_eq!(f.read().on_battery, 0);
        f.put("/proc/stat", "cpu 1 2 3\n");
        assert_eq!(cpu_ticks(&Root(&f.0)), None);
    }
    #[test]
    fn thermal_guards_and_firmware_sticky_bits_keep_their_boundaries() {
        let mut c = LbCond {
            rpi: 1,
            rpi_throttled: 0x50000,
            rpi_uv_alarm: 1,
            ..Default::default()
        };
        let mut w = LbRpiWatch::start(&c);
        assert_eq!(w.seen, 0);
        assert_eq!(w.sample(&c), 0);
        c.rpi_throttled = 0;
        c.rpi_uv_alarm = 0;
        assert_eq!(w.sample(&c), 0);
        c.rpi_throttled = 0x50000;
        c.rpi_uv_alarm = 1;
        assert_eq!(w.sample(&c), LB_RPI_UV | LB_RPI_THROTTLED);
        let start = LbCond::default();
        assert!(stop(&start, &c, LB_RPI_UV, true).is_none());
        assert_eq!(stop(&start, &c, LB_RPI_THROTTLED, true), Some(c"throttled"));
        assert!(stop(&start, &c, LB_RPI_THROTTLED, false).is_none());
        c.passive_c = 95.;
        c.critical_c = 105.;
        c.temp_c = 92.999;
        assert!(stop(&start, &c, 0, true).is_none());
        c.temp_c = 93.;
        assert_eq!(stop(&start, &c, 0, false), Some(c"thermal"));
        c.passive_c = 0.;
        c.temp_c = 94.999;
        assert!(stop(&start, &c, 0, true).is_none());
        c.temp_c = 95.;
        assert_eq!(stop(&start, &c, 0, true), Some(c"thermal"));
        c.temp_c = 80.;
        assert_eq!(w.sample(&c) & LB_RPI_HOT, LB_RPI_HOT);
    }
    #[test]
    fn hwmon_thresholds_and_strings_are_bounded() {
        let f = Fixture::new();
        f.put("/sys/class/hwmon/hwmon0/name", "k10temp\n");
        f.put("/sys/class/hwmon/hwmon0/temp1_input", "85000\n");
        f.put("/sys/class/hwmon/hwmon0/temp1_max", "90000\n");
        f.put("/sys/class/hwmon/hwmon0/temp1_crit", "0\n");
        f.put("/sys/firmware/acpi/platform_profile", "Perf 123!\n");
        f.put(
            "/sys/firmware/devicetree/base/compatible",
            "raspberrypi,5-model-b\0brcm,bcm2712\0",
        );
        let c = f.read();
        assert_eq!(
            (c.temp_c, c.passive_c, c.critical_c, c.rpi),
            (85., 90., 0., 1)
        );
        assert_eq!(unsafe { CStr::from_ptr(c.zone.as_ptr()) }, c"k10temp");
        assert_eq!(
            unsafe { CStr::from_ptr(c.platform_profile.as_ptr()) },
            c"_erf_____"
        );
        assert_eq!(decimal(b"  -123junk"), Some(-123));
        assert_eq!(decimal(b"junk"), None);
    }
    #[test]
    fn storage_classification_follows_partitions_and_mapper_slaves() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let disk = f.0.join("nvme0n1");
        std::fs::create_dir_all(disk.join("nvme0n1p1")).unwrap();
        std::fs::write(disk.join("nvme0n1p1/partition"), "1").unwrap();
        assert_eq!(device_kind(disk.join("nvme0n1p1")), b"nvme\0");
        let mapper = f.0.join("dm-0/slaves");
        std::fs::create_dir_all(&mapper).unwrap();
        symlink(&disk, mapper.join("nvme0n1")).unwrap();
        assert_eq!(device_kind(f.0.join("dm-0")), b"nvme\0");
        f.put("/sda/queue/rotational", "0\n");
        assert_eq!(device_kind(f.0.join("sda")), b"ssd\0");
        f.put("/sda/device/vendor", "QEMU\n");
        assert_eq!(device_kind(f.0.join("sda")), b"virtual\0");
        assert_eq!(device_kind(f.0.join("usb1/sda")), b"usb\0");
    }
}
