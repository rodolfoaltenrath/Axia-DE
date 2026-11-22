//! Fan profiles, curve evaluation with hysteresis, the configuration file and the socket
//! requests of axia-fand. Pure code: no I/O.
//!
//! A curve is a list of steps "from T °C, P %", sorted by temperature. Below the first step the
//! fan is off (or at the profile's floor). A step is entered when the temperature reaches it and
//! left only when it falls `hyst` °C below it, so the fan does not flap around a threshold.
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::fmt;

pub const MAX_POINTS: usize = 8;
/// Custom curve limits, °C.
pub const TEMP_MIN: i32 = 30;
pub const TEMP_MAX: i32 = 90;
pub const HYST_DEFAULT: i32 = 3;
pub const HYST_MIN: i32 = 1;
pub const HYST_MAX: i32 = 10;
/// Below this most small fans stall: a non-zero duty is raised to it.
pub const MIN_PCT: i32 = 25;
/// Starting from 0 % to less than this: one tick at 100 % first.
pub const KICK_BELOW: i32 = 50;
/// The longest request line accepted on the socket.
pub const REQ_MAX: usize = 256;
pub const FORCE_MAX_MIN: i32 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Auto,
    Quiet,
    Balanced,
    Performance,
    Always,
    Custom,
}

const PROFILES: [(Profile, &str); 6] = [
    (Profile::Auto, "auto"),
    (Profile::Quiet, "quiet"),
    (Profile::Balanced, "balanced"),
    (Profile::Performance, "performance"),
    (Profile::Always, "always"),
    (Profile::Custom, "custom"),
];

impl Profile {
    pub fn name(self) -> &'static str {
        PROFILES.iter().find(|p| p.0 == self).map_or("auto", |p| p.1)
    }
    pub fn parse(s: &str) -> Option<Profile> {
        PROFILES.iter().find(|p| p.1 == s).map(|p| p.0)
    }
}

/// Steps of (°C, %) and the duty below the first one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Curve {
    pub points: Vec<(i32, i32)>,
    pub floor_pct: i32,
}

impl Curve {
    pub fn new(points: &[(i32, i32)], floor_pct: i32) -> Curve {
        Curve { points: points.to_vec(), floor_pct }
    }

    /// 1..8 points, temperatures 30..90 strictly increasing, percentages 0..100 never decreasing.
    pub fn valid(&self) -> bool {
        let n = self.points.len();
        if n < 1 || n > MAX_POINTS || !(0..=100).contains(&self.floor_pct) {
            return false;
        }
        for (i, &(t, p)) in self.points.iter().enumerate() {
            if !(TEMP_MIN..=TEMP_MAX).contains(&t) || !(0..=100).contains(&p) {
                return false;
            }
            if i > 0 && (t <= self.points[i - 1].0 || p < self.points[i - 1].1) {
                return false;
            }
        }
        true
    }

    /// The next step (-1 = below the first) for `temp_mc` (milli-°C), given the previous step.
    pub fn step(&self, hyst: i32, prev: i32, temp_mc: i32) -> i32 {
        let n = self.points.len() as i32;
        if n <= 0 {
            return -1;
        }
        let mut s = if prev < -1 { -1 } else if prev >= n { n - 1 } else { prev };
        // leave a step only `hyst` °C below it ...
        while s >= 0 && (temp_mc as i64) < (self.points[s as usize].0 - hyst) as i64 * 1000 {
            s -= 1;
        }
        // ... and enter every step the temperature has reached
        while s + 1 < n && (temp_mc as i64) >= self.points[(s + 1) as usize].0 as i64 * 1000 {
            s += 1;
        }
        s
    }

    /// The duty (%) of a step: the step's %, never below the floor.
    pub fn pct(&self, step: i32) -> i32 {
        let f = self.floor_pct.max(0);
        if step < 0 || step as usize >= self.points.len() {
            return f;
        }
        self.points[step as usize].1.max(f)
    }
}

/// What reaches the hardware: 0, or at least `MIN_PCT`, at most 100.
pub fn output_pct(pct: i32) -> i32 {
    if pct <= 0 {
        0
    } else if pct >= 100 {
        100
    } else {
        pct.max(MIN_PCT)
    }
}

fn preset(p: Profile) -> Option<Curve> {
    Some(match p {
        Profile::Quiet => Curve::new(&[(60, 30), (67, 60), (72, 100)], 0),
        // the curve most cases ship with: 55 °C → 30 %, 60 °C → 55 %, 65 °C → 100 %
        Profile::Balanced => Curve::new(&[(55, 30), (60, 55), (65, 100)], 0),
        Profile::Performance => Curve::new(&[(45, 40), (55, 70), (60, 100)], 0),
        Profile::Always => Curve::new(&[(55, 60), (65, 100)], 40),
        Profile::Auto | Profile::Custom => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    Off,
    Timed,
    On,
}

impl Force {
    pub fn name(self) -> &'static str {
        match self {
            Force::On => "on",
            Force::Timed => "timed",
            Force::Off => "off",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub profile: Profile,
    /// The user's curve (kept even while another profile is active).
    pub custom: Curve,
    pub hyst: i32,
    pub force: Force,
    /// `Force::Timed`: CLOCK_REALTIME seconds.
    pub force_until: i64,
}

impl Config {
    /// A board fan (the Raspberry Pi fan header) gets our curve from the first boot; a PC's
    /// motherboard fans stay with the firmware until the user picks a profile.
    pub fn default_for(has_board_fan: bool) -> Config {
        Config {
            profile: if has_board_fan { Profile::Balanced } else { Profile::Auto },
            custom: preset(Profile::Balanced).unwrap(),
            hyst: HYST_DEFAULT,
            force: Force::Off,
            force_until: 0,
        }
    }

    /// The curve a profile runs. `Auto` has none: the fans are left to the kernel or the firmware.
    pub fn curve(&self) -> Curve {
        match self.profile {
            Profile::Custom => self.custom.clone(),
            p => preset(p).unwrap_or_default(),
        }
    }

    /// The text of `/etc/lordos/fan.json`.
    pub fn to_json(&self) -> String {
        let pts: Vec<String> = self.custom.points.iter().map(|(t, p)| format!("[{t},{p}]")).collect();
        format!(
            "{{\"version\":1,\"profile\":\"{}\",\"hyst\":{},\"custom\":[{}],\"force\":\"{}\",\"forceUntil\":{}}}\n",
            self.profile.name(),
            self.hyst,
            pts.join(","),
            self.force.name(),
            if self.force == Force::Timed { self.force_until } else { 0 }
        )
    }

    /// Reads `fan.json` over `self` (the defaults: a missing key keeps its value). Strict: unknown
    /// or repeated keys, wrong types, an invalid curve and trailing text are refused, and then
    /// `self` is left as it was. The error is the name of what was wrong.
    pub fn apply_json(&mut self, text: &[u8]) -> Result<(), String> {
        let pairs: Pairs = serde_json::from_slice(text).map_err(|e| e.to_string())?;
        let mut c = self.clone();
        let int = |v: &Value, lo: i32, hi: i32| -> Option<i32> {
            let x = v.as_f64()?;
            (x >= lo as f64 && x <= hi as f64 && x == (x as i32) as f64).then_some(x as i32)
        };
        let mut seen: Vec<&str> = Vec::new();
        for (k, v) in &pairs.0 {
            if seen.contains(&k.as_str()) {
                return Err("duplicate key".into());
            }
            seen.push(k);
            match k.as_str() {
                "version" => {
                    if v.as_f64() != Some(1.0) {
                        return Err("version".into());
                    }
                }
                "profile" => c.profile = v.as_str().and_then(Profile::parse).ok_or("profile")?,
                "hyst" => c.hyst = int(v, HYST_MIN, HYST_MAX).ok_or("hyst")?,
                "custom" => {
                    let a = v.as_array().filter(|a| (1..=MAX_POINTS).contains(&a.len())).ok_or("custom")?;
                    let mut pts = Vec::new();
                    for pt in a {
                        let pt = pt.as_array().filter(|p| p.len() == 2).ok_or("custom")?;
                        let (t, p) = (int(&pt[0], TEMP_MIN, TEMP_MAX), int(&pt[1], 0, 100));
                        pts.push((t.ok_or("custom")?, p.ok_or("custom")?));
                    }
                    let cu = Curve { points: pts, floor_pct: 0 };
                    if !cu.valid() {
                        return Err("custom".into());
                    }
                    c.custom = cu;
                }
                "force" => {
                    c.force = match v.as_str() {
                        Some("off") => Force::Off,
                        Some("on") => Force::On,
                        Some("timed") => Force::Timed,
                        _ => return Err("force".into()),
                    }
                }
                "forceUntil" => {
                    let x = v.as_f64().filter(|x| *x >= 0.0 && *x <= 4102444800.0 && *x == (*x as i64) as f64).ok_or("forceUntil")?;
                    c.force_until = x as i64;
                }
                _ => return Err("unknown key".into()),
            }
        }
        if c.force == Force::Timed && c.force_until <= 0 {
            return Err("forceUntil".into());
        }
        if c.force != Force::Timed {
            c.force_until = 0;
        }
        *self = c;
        Ok(())
    }
}

/// The members of a JSON object in order, repeated keys included (a `Value` would drop them).
struct Pairs(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Pairs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Pairs, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Pairs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Pairs, A::Error> {
                let mut v = Vec::new();
                while let Some(kv) = m.next_entry::<String, Value>()? {
                    v.push(kv);
                }
                Ok(Pairs(v))
            }
        }
        d.deserialize_map(V)
    }
}

/// A decimal integer of 1..4 digits at the start of `s`; the rest follows it.
fn take_int(s: &[u8]) -> Option<(i32, &[u8])> {
    let d = s.iter().take_while(|c| c.is_ascii_digit()).count();
    if d == 0 || d > 4 {
        return None;
    }
    let v = s[..d].iter().fold(0i32, |a, c| a * 10 + (c - b'0') as i32);
    Some((v, &s[d..]))
}

/// "55:30,60:55,65:100" → a curve (valid).
pub fn parse_curve_text(s: &[u8]) -> Option<Curve> {
    let mut points = Vec::new();
    let mut s = s;
    loop {
        if points.len() >= MAX_POINTS {
            return None;
        }
        let (t, rest) = take_int(s)?;
        let rest = rest.strip_prefix(b":")?;
        let (p, rest) = take_int(rest)?;
        points.push((t, p));
        match rest {
            [] => break,
            [b',', more @ ..] => s = more,
            _ => return None,
        }
    }
    let c = Curve { points, floor_pct: 0 };
    c.valid().then_some(c)
}

/// A request over the socket (one line, no trailing newline):
///   STATUS, PROFILE auto|quiet|balanced|performance|always|custom, CURVE <t:p,...>,
///   FORCE off|on|<minutes> (1..1440).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Status,
    Profile(Profile),
    /// Sets the custom curve and selects it.
    Curve(Curve),
    Force(Force, i32),
}

pub fn parse_request(line: &[u8]) -> Option<Request> {
    if line.is_empty() || line.len() >= REQ_MAX || line.iter().any(|&c| !(0x20..=0x7e).contains(&c)) {
        return None;
    }
    if line == b"STATUS" {
        return Some(Request::Status);
    }
    if let Some(p) = line.strip_prefix(b"PROFILE ") {
        return Profile::parse(std::str::from_utf8(p).ok()?).map(Request::Profile);
    }
    if let Some(c) = line.strip_prefix(b"CURVE ") {
        return parse_curve_text(c).map(Request::Curve);
    }
    if let Some(a) = line.strip_prefix(b"FORCE ") {
        return match a {
            b"off" => Some(Request::Force(Force::Off, 0)),
            b"on" => Some(Request::Force(Force::On, 0)),
            _ => {
                let (m, rest) = take_int(a)?;
                (rest.is_empty() && (1..=FORCE_MAX_MIN).contains(&m)).then_some(Request::Force(Force::Timed, m))
            }
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balanced() -> Curve {
        Config::default_for(true).curve()
    }

    #[test]
    fn a_curve_rises_with_the_temperature_and_falls_with_hysteresis() {
        let cfg = Config::default_for(true);
        assert_eq!(cfg.profile, Profile::Balanced);
        let c = balanced();
        assert_eq!((c.points.len(), c.points[0], c.points[2].1), (3, (55, 30), 100));
        assert!(c.valid());
        // going up
        let mut s = -1;
        for (t, want, pct) in [(40000, -1, 0), (54999, -1, 0), (55000, 0, 30), (61000, 1, 55), (70000, 2, 100)] {
            s = c.step(3, s, t);
            assert_eq!((s, c.pct(s)), (want, pct), "at {t}");
        }
        assert_eq!(c.step(3, -1, 80000), 2, "straight from off to the top");
        // hysteresis on the way down: 65 stays until below 62
        let mut s = 2;
        for (t, want) in [(64000, 2), (62000, 2), (61999, 1), (57500, 1), (56999, 0), (52000, 0), (51999, -1)] {
            s = c.step(3, s, t);
            assert_eq!(s, want, "at {t}");
        }
        assert_eq!(c.step(3, 2, 30000), -1, "a big drop leaves several steps at once");
        // oscillating around a threshold does not flap
        let mut s = 0;
        for i in 0..20 {
            s = c.step(3, s, if i & 1 == 1 { 54500 } else { 55500 });
            assert_eq!(s, 0);
        }
        // bad previous steps are clamped
        assert_eq!((c.step(3, 99, 70000), c.step(3, -7, 20000)), (2, -1));
    }

    #[test]
    fn auto_has_no_curve_always_has_a_floor_and_the_output_is_clamped() {
        let mut cfg = Config::default_for(true);
        cfg.profile = Profile::Auto;
        let c = cfg.curve();
        assert!(c.points.is_empty() && c.step(3, 1, 90000) == -1);
        cfg.profile = Profile::Always;
        let c = cfg.curve();
        assert_eq!(c.pct(-1), 40);
        assert_eq!(c.pct(c.step(3, -1, 66000)), 100);
        assert_eq!((output_pct(0), output_pct(-5), output_pct(1), output_pct(24), output_pct(25)), (0, 0, MIN_PCT, MIN_PCT, 25));
        assert_eq!((output_pct(60), output_pct(99), output_pct(100), output_pct(150)), (60, 99, 100, 100));
        for p in [Profile::Quiet, Profile::Balanced, Profile::Performance, Profile::Always] {
            cfg.profile = p;
            assert!(cfg.curve().valid(), "{p:?}");
        }
        assert_eq!(Config::default_for(false).profile, Profile::Auto, "a PC keeps its firmware's fans until asked");
    }

    #[test]
    fn what_makes_a_curve_valid() {
        let v = |pts: &[(i32, i32)]| Curve::new(pts, 0).valid();
        assert!(!v(&[(50, 40), (60, 30)]), "% going down");
        assert!(!v(&[(60, 40), (60, 50)]), "the same temperature");
        assert!(!v(&[(29, 40)]) && !v(&[(91, 40)]) && !v(&[(90, 101)]) && !v(&[(50, -1)]));
        assert!(!v(&[]) && !v(&[(30, 1); 9]));
        assert!(v(&[(30, 0), (90, 100)]));
        assert!(!Curve::new(&[(50, 50)], 101).valid() && !Curve::new(&[(50, 50)], -1).valid());
        let eight: Vec<(i32, i32)> = (0..8).map(|i| (30 + i, i)).collect();
        assert!(v(&eight));
    }

    fn parse(s: &str) -> Result<Config, String> {
        let mut c = Config::default_for(true);
        c.apply_json(s.as_bytes()).map(|_| c)
    }

    #[test]
    fn fan_json_good_files() {
        let c = parse("{}").unwrap();
        assert_eq!((c.profile, c.hyst), (Profile::Balanced, 3));
        let c = parse(r#"{"version":1,"profile":"quiet","hyst":5}"#).unwrap();
        assert_eq!((c.profile, c.hyst), (Profile::Quiet, 5));
        let c = parse(r#"{"profile":"custom","custom":[[40,20],[50,50],[70,100]]}"#).unwrap();
        assert_eq!((c.profile, c.custom.points.len(), c.custom.points[1].1), (Profile::Custom, 3, 50));
        let c = parse(r#"{"force":"timed","forceUntil":1900000000}"#).unwrap();
        assert_eq!((c.force, c.force_until), (Force::Timed, 1900000000));
        let c = parse(r#"{"force":"on","forceUntil":5}"#).unwrap();
        assert_eq!((c.force, c.force_until), (Force::On, 0));
        // round trip
        let mut a = Config::default_for(false);
        a.profile = Profile::Custom;
        a.hyst = 4;
        a.force = Force::Timed;
        a.force_until = 1800000000;
        a.custom = Curve::new(&[(35, 0), (45, 25), (60, 60), (85, 100)], 0);
        let mut b = Config::default_for(false);
        b.apply_json(a.to_json().as_bytes()).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a.to_json(),
            "{\"version\":1,\"profile\":\"custom\",\"hyst\":4,\"custom\":[[35,0],[45,25],[60,60],[85,100]],\"force\":\"timed\",\"forceUntil\":1800000000}\n"
        );
        let mut off = a.clone();
        off.force = Force::Off;
        assert!(off.to_json().ends_with("\"force\":\"off\",\"forceUntil\":0}\n"));
    }

    #[test]
    fn fan_json_broken_files_are_refused_and_change_nothing() {
        let bad = [
            "", " ", "null", "[]", "1", "\"x\"", "{", "{}}", r#"{"profile":"turbo"}"#, r#"{"profile":1}"#,
            r#"{"profile":"quiet","profile":"quiet"}"#, r#"{"hyst":0}"#, r#"{"hyst":11}"#, r#"{"hyst":2.5}"#, r#"{"hyst":"3"}"#,
            r#"{"version":2}"#, r#"{"extra":1}"#, r#"{"custom":[]}"#, r#"{"custom":[[50]]}"#, r#"{"custom":[[50,30,1]]}"#,
            r#"{"custom":[[50,30],[45,40]]}"#, r#"{"custom":[[50,30],[60,20]]}"#, r#"{"custom":[[20,30]]}"#, r#"{"custom":[[95,30]]}"#,
            r#"{"custom":[[50,-1]]}"#, r#"{"custom":[[50,101]]}"#, r#"{"custom":[[50.5,30]]}"#, r#"{"custom":[["50",30]]}"#,
            r#"{"custom":[[30,1],[31,2],[32,3],[33,4],[34,5],[35,6],[36,7],[37,8],[38,9]]}"#, r#"{"custom":{}}"#,
            r#"{"force":"maybe"}"#, r#"{"force":"timed"}"#, r#"{"force":"timed","forceUntil":-1}"#, r#"{"forceUntil":1e30}"#,
            r#"{"forceUntil":1.5}"#, r#"{"profile":"quiet"} x"#, r#"{"profile":"qu\u0000iet"}"#,
        ];
        for b in bad {
            let mut c = Config::default_for(true);
            assert!(c.apply_json(b.as_bytes()).is_err(), "accepted: {b}");
            assert_eq!(c, Config::default_for(true), "half applied: {b}");
        }
        // half a file never half-applies: the first keys are good, the last one is not
        let mut c = Config::default_for(true);
        assert!(c.apply_json(br#"{"profile":"quiet","hyst":5,"force":"x"}"#).is_err());
        assert_eq!(c, Config::default_for(true));
    }

    #[test]
    fn truncated_and_damaged_files_never_crash_or_half_apply() {
        let json = Config::default_for(true).to_json();
        let mut seed = 12345u32;
        let mut next = || {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            seed >> 8
        };
        for _ in 0..4000 {
            let mut s = json.clone().into_bytes();
            s.truncate(next() as usize % (s.len() + 1));
            if !s.is_empty() && next() % 2 == 0 {
                let at = next() as usize % s.len();
                s[at] = (next() >> 12) as u8;
            }
            let mut c = Config::default_for(true);
            match c.apply_json(&s) {
                Ok(()) => assert!((HYST_MIN..=HYST_MAX).contains(&c.hyst) && c.custom.valid()),
                Err(_) => assert_eq!(c, Config::default_for(true)),
            }
        }
    }

    #[test]
    fn requests() {
        let r = |s: &str| parse_request(s.as_bytes());
        assert_eq!(r("STATUS"), Some(Request::Status));
        assert_eq!(r("PROFILE quiet"), Some(Request::Profile(Profile::Quiet)));
        assert_eq!(r("PROFILE auto"), Some(Request::Profile(Profile::Auto)));
        assert_eq!(r("CURVE 55:30,60:55,65:100"), Some(Request::Curve(Curve::new(&[(55, 30), (60, 55), (65, 100)], 0))));
        assert_eq!(r("FORCE on"), Some(Request::Force(Force::On, 0)));
        assert_eq!(r("FORCE off"), Some(Request::Force(Force::Off, 0)));
        assert_eq!(r("FORCE 30"), Some(Request::Force(Force::Timed, 30)));
        assert!(r("FORCE 1440").is_some());
        for bad in [
            "", "status", "STATUS ", " STATUS", "PROFILE", "PROFILE ", "PROFILE turbo", "PROFILE quiet x", "CURVE", "CURVE ", "CURVE 55:30,",
            "CURVE 55:30,,60:50", "CURVE 55-30", "CURVE 55:30,50:40", "CURVE 55:30,60:20", "CURVE 10:30", "CURVE 55:300", "CURVE 00055:30",
            "CURVE 55:30\n", "FORCE", "FORCE 0", "FORCE 1441", "FORCE -1", "FORCE 30m", "FORCE on ", "SET /sys/class/hwmon/hwmon0/pwm1 255",
            "PROFILE quiet\tx", "CURVE 30:1,31:2,32:3,33:4,34:5,35:6,36:7,37:8,38:9", "PROFILE quiet\u{e9}",
        ] {
            assert_eq!(r(bad), None, "accepted request: {bad:?}");
        }
        assert_eq!(parse_request("A".repeat(REQ_MAX + 7).as_bytes()), None);
        assert_eq!(parse_request("A".repeat(REQ_MAX).as_bytes()), None);
        assert_eq!(parse_request(b"STATUS\0x"), None);
        assert!(parse_request(b"CURVE 30:1,31:2,32:3,33:4,34:5,35:6,36:7,37:8").is_some(), "eight points fit");
    }

    #[test]
    fn profile_names_round_trip() {
        for (p, n) in PROFILES {
            assert_eq!((p.name(), Profile::parse(n)), (n, Some(p)));
        }
        assert_eq!(Profile::parse("Quiet"), None);
    }
}
