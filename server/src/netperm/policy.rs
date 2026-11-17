//! Strict on-disk policy reader. All parsing completes before a live policy is replaced.
use crate::{dnsmap::*, fwlists::*, fwrules::*};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::MetadataExt,
};
pub const FLAGS: [&str; 12] = [
    "no-internet",
    "no-lan",
    "no-network",
    "no-incoming",
    "no-camera",
    "no-mic",
    "no-screen",
    "no-usb",
    "no-bluetooth",
    "no-files",
    "no-notifications",
    "no-clipboard",
];
pub fn cs(s: &str) -> CString {
    CString::new(s).expect("validated text has no NUL")
}
pub struct Store(pub *mut FwStore);
impl Default for Store {
    fn default() -> Self {
        let p = fw_store_new();
        assert!(!p.is_null());
        Self(p)
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        unsafe { fw_store_free(self.0) }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    v: f64,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    trusted: Vec<String>,
    #[serde(default)]
    restricted: HashMap<String, Vec<String>>,
    #[serde(default)]
    rules: Vec<Rule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    app: String,
    #[serde(default)]
    dir: Option<String>,
    dst: String,
    #[serde(default)]
    port: Option<serde_json::Value>,
    #[serde(default)]
    proto: Option<String>,
    action: String,
    #[serde(default)]
    when: Option<f64>,
}
// serde_json::Value normally accepts repeated object keys. Reject them throughout
// the document before schema validation, matching the existing strict C reader.
pub fn strict_json(bytes: &[u8]) -> Result<serde_json::Value, String> {
    struct Strict(serde_json::Value);
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Strict;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("JSON without repeated keys")
                }
                fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Strict, E> {
                    Ok(Strict(
                        serde_json::Number::from_f64(v)
                            .ok_or_else(|| E::custom("nonfinite number"))?
                            .into(),
                    ))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Strict, E> {
                    Ok(Strict(serde_json::Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut a: A,
                ) -> Result<Strict, A::Error> {
                    let mut v = vec![];
                    while let Some(Strict(x)) = a.next_element()? {
                        v.push(x)
                    }
                    Ok(Strict(v.into()))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut a: A,
                ) -> Result<Strict, A::Error> {
                    let mut v = serde_json::Map::new();
                    while let Some(k) = a.next_key::<String>()? {
                        if v.contains_key(&k) {
                            return Err(serde::de::Error::custom("repeated key"));
                        }
                        let Strict(x) = a.next_value()?;
                        v.insert(k, x);
                    }
                    Ok(Strict(v.into()))
                }
            }
            d.deserialize_any(V)
        }
    }
    serde_json::from_slice::<Strict>(bytes)
        .map(|s| s.0)
        .map_err(|e| e.to_string())
}
fn canonical(s: &str) -> Result<CString, String> {
    if app_canon(s.as_bytes()).as_deref() != Some(s.as_bytes()) || s.as_bytes().contains(&0) {
        return Err("invalid app id".into());
    }
    CString::new(s).map_err(|_| "invalid app id".into())
}
fn number(s: &str) -> Result<u16, String> {
    if s.is_empty()
        || s.len() > 5
        || s.len() > 1 && s.starts_with('0')
        || !s.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("invalid port".into());
    }
    s.parse().map_err(|_| "invalid port".into())
}
impl Store {
    pub fn parse(text: &[u8]) -> Result<Self, String> {
        if text.len() > 4 << 20 || text.contains(&0) {
            return Err("invalid file length or NUL".into());
        }
        let value = strict_json(text)?;
        if value.get("mode").is_some_and(serde_json::Value::is_null) {
            return Err("invalid mode".into());
        }
        if let Some(rules) = value.get("rules").and_then(serde_json::Value::as_array) {
            for rule in rules {
                if ["dir", "port", "proto", "when"]
                    .iter()
                    .any(|k| rule.get(k).is_some_and(serde_json::Value::is_null))
                {
                    return Err("null rule field".into());
                }
            }
        }
        let doc: Document = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if doc.v != 1.0
            || doc.trusted.len() > 4096
            || doc.restricted.len() > 4096
            || doc.rules.len() > 65536
        {
            return Err("unsupported version or too many entries".into());
        }
        let s = Self::default();
        unsafe {
            let mode = match doc.mode.as_deref().unwrap_or("silent") {
                "silent" => 0,
                "ask" => 1,
                "off" => 2,
                "deny" => 3,
                _ => return Err("invalid mode".into()),
            };
            fw_set_mode(s.0, mode);
            let mut trusted = HashSet::new();
            for app in doc.trusted {
                let c = canonical(&app)?;
                if !trusted.insert(app) || fw_set_trusted(s.0, c.as_ptr(), 1) != 0 {
                    return Err("repeated or invalid trusted app".into());
                }
            }
            for (app, flags) in doc.restricted {
                let c = canonical(&app)?;
                if trusted.contains(&app) || flags.is_empty() || flags.len() > 12 {
                    return Err("invalid restrictions".into());
                }
                let mut bits = 0u32;
                for f in flags {
                    let Some(i) = FLAGS.iter().position(|&x| x == f) else {
                        return Err("unknown restriction".into());
                    };
                    if bits & (1 << i) != 0 {
                        return Err("repeated restriction".into());
                    }
                    bits |= 1 << i;
                }
                if fw_set_restricted(s.0, c.as_ptr(), bits) != 0 {
                    return Err("restrictions don't fit".into());
                }
            }
            for r in doc.rules {
                let app = canonical(&r.app)?;
                let mut rule = zeroed_rule();
                let bytes = app.as_bytes();
                rule.app[..bytes.len()].copy_from_slice(bytes);
                rule.port_hi = 65535;
                rule.dir = match r.dir.as_deref().unwrap_or("out") {
                    "out" => 0,
                    "in" => 1,
                    _ => return Err("invalid direction".into()),
                };
                if r.dst.len() >= 258 || !parse_dest(r.dst.as_bytes(), &mut rule) {
                    return Err("invalid destination".into());
                }
                if let Some(port) = r.port {
                    let (lo, hi) = if let Some(v) = port.as_str() {
                        if v.len() >= 16 {
                            return Err("invalid port".into());
                        }
                        if let Some((a, b)) = v.split_once('-') {
                            (number(a)?, number(b)?)
                        } else {
                            let n = number(v)?;
                            (n, n)
                        }
                    } else {
                        let n = port
                            .as_f64()
                            .filter(|v| *v >= 1.0 && *v <= 65535.0 && v.fract() == 0.0)
                            .ok_or("invalid port")? as u16;
                        (n, n)
                    };
                    if lo == 0 || lo > hi {
                        return Err("invalid port range".into());
                    }
                    rule.port_lo = lo;
                    rule.port_hi = hi;
                }
                rule.proto = match r.proto.as_deref().unwrap_or("any") {
                    "any" => 0,
                    "tcp" => 1,
                    "udp" => 2,
                    _ => return Err("invalid protocol".into()),
                };
                rule.action = match r.action.as_str() {
                    "allow" => 0,
                    "deny" => 1,
                    _ => return Err("invalid action".into()),
                };
                if let Some(w) = r.when {
                    if w < 0.0 || w > 9007199254740992.0 || w.fract() != 0.0 {
                        return Err("invalid time".into());
                    }
                    rule.when = w as i64
                }
                if fw_rule_add(s.0, &rule, std::ptr::null_mut()) != 0 {
                    return Err("rule doesn't fit".into());
                }
            }
        }
        Ok(s)
    }
}
pub struct Lists(pub *mut FwLists);
impl Lists {
    pub fn parse(b: &[u8]) -> Result<Option<Self>, String> {
        unsafe {
            let mut err = [0i8; 120];
            let p = fwl_load_text(b.as_ptr().cast(), b.len(), err.as_mut_ptr(), err.len());
            if p.is_null() {
                return Err(std::ffi::CStr::from_ptr(err.as_ptr())
                    .to_string_lossy()
                    .into_owned());
            }
            let l = Self(p);
            if fwl_domains(p) == 0 && fwl_nets(p) == 0 {
                Ok(None)
            } else {
                Ok(Some(l))
            }
        }
    }
    pub fn blocks(&self, names: &[[u8; 254]]) -> bool {
        let p: Vec<_> = names.iter().map(|s| s.as_ptr().cast()).collect();
        unsafe { fwl_block_names(self.0, p.as_ptr(), p.len() as i32) != 0 }
    }
    pub fn networks(&self) -> Vec<(i32, [u8; 16], i32)> {
        unsafe extern "C" fn cb(u: *mut libc::c_void, f: i32, a: *const u8, p: i32) -> i32 {
            let v = &mut *u.cast::<Vec<(i32, [u8; 16], i32)>>();
            let mut b = [0; 16];
            std::ptr::copy_nonoverlapping(a, b.as_mut_ptr(), if f == 4 { 4 } else { 16 });
            v.push((f, b, p));
            0
        }
        let mut v = vec![];
        unsafe {
            fwl_net_each(self.0, Some(cb), (&mut v as *mut Vec<_>).cast());
        }
        v
    }
}
impl Drop for Lists {
    fn drop(&mut self) {
        unsafe { fwl_free(self.0) }
    }
}
pub struct Names(pub *mut DnsMap);
impl Default for Names {
    fn default() -> Self {
        Self(dnsmap_new(16384))
    }
}
impl Drop for Names {
    fn drop(&mut self) {
        unsafe { dnsmap_free(self.0) }
    }
}
pub const NAMES: &std::ffi::CStr = c"lordos";
impl Names {
    pub fn each(&self, now: i64) -> Vec<(i32, [u8; 16], Vec<[u8; 254]>)> {
        unsafe extern "C" fn cb(
            u: *mut libc::c_void,
            f: i32,
            a: *const u8,
            n: *const [u8; 254],
            len: i32,
        ) -> i32 {
            let v = &mut *u.cast::<Vec<(i32, [u8; 16], Vec<[u8; 254]>)>>();
            let mut b = [0; 16];
            std::ptr::copy_nonoverlapping(a, b.as_mut_ptr(), if f == 4 { 4 } else { 16 });
            v.push((
                f,
                b,
                std::slice::from_raw_parts(n, len.max(0) as usize).to_vec(),
            ));
            0
        }
        let mut v = vec![];
        unsafe {
            dnsmap_each(
                self.0,
                NAMES.as_ptr(),
                now,
                Some(cb),
                (&mut v as *mut Vec<_>).cast(),
            );
        }
        v
    }
    pub fn lookup(&self, f: i32, a: &[u8; 16], now: i64) -> Vec<[u8; 254]> {
        let mut names = [[0; 254]; 8];
        let n = unsafe {
            dnsmap_lookup(
                self.0,
                NAMES.as_ptr(),
                f,
                a.as_ptr(),
                now,
                names.as_mut_ptr(),
                8,
            )
        };
        names[..n.max(0) as usize].to_vec()
    }
    pub fn hosts(&self, f: i32, a: &[u8; 16], now: i64) -> Vec<String> {
        let mut n = [[0; 254]; 4];
        let got = unsafe {
            dnsmap_lookup(
                self.0,
                NAMES.as_ptr(),
                f,
                a.as_ptr(),
                now,
                n.as_mut_ptr(),
                4,
            )
        };
        n[..got.max(0) as usize]
            .iter()
            .map(|n| String::from_utf8_lossy(crate::fwrules::cs(n)).into_owned())
            .collect()
    }
}
pub fn read_user(uid: u32, file: &str, max: usize) -> io::Result<Option<Vec<u8>>> {
    unsafe {
        let pw = libc::getpwuid(uid);
        if pw.is_null() || (*pw).pw_dir.is_null() || *(*pw).pw_dir as u8 != b'/' {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let home = std::ffi::CStr::from_ptr((*pw).pw_dir);
        read_at(uid, home, file, max)
    }
}
fn read_at(uid: u32, home: &std::ffi::CStr, file: &str, max: usize) -> io::Result<Option<Vec<u8>>> {
    let fd = unsafe {
        libc::open(
            home.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    if f.metadata()?.uid() != uid {
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }
    for (i, part) in [".config", "axia", file].iter().enumerate() {
        let part = CString::new(*part).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
        let n = unsafe {
            libc::openat(
                f.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | if i == 2 {
                        libc::O_NONBLOCK
                    } else {
                        libc::O_DIRECTORY
                    },
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(e)
            };
        }
        f = unsafe { std::fs::File::from_raw_fd(n) };
        let m = f.metadata()?;
        if m.uid() != uid || i == 2 && (!m.is_file() || m.len() > max as u64) {
            return Err(io::Error::from_raw_os_error(libc::EPERM));
        }
    }
    let mut b = vec![];
    f.take(max as u64 + 1).read_to_end(&mut b)?;
    if b.len() > max {
        return Err(io::Error::from_raw_os_error(libc::EFBIG));
    }
    Ok(Some(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_path_reader_refuses_links_nonregular_files_and_oversize() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        let mut template = b"/tmp/axia-netperm-reader-XXXXXX\0".to_vec();
        let p = unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) };
        assert!(!p.is_null());
        let home = unsafe { std::ffi::CStr::from_ptr(p) }.to_owned();
        let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(home.to_bytes()));
        let conf = path.join(".config/axia");
        std::fs::create_dir_all(&conf).unwrap();
        let uid = unsafe { libc::getuid() };
        assert!(read_at(uid, &home, "firewall.json", 20).unwrap().is_none());
        std::fs::write(conf.join("firewall.json"), b"{\"v\":1}").unwrap();
        assert_eq!(
            read_at(uid, &home, "firewall.json", 20).unwrap().unwrap(),
            b"{\"v\":1}"
        );
        assert!(read_at(uid, &home, "firewall.json", 2).is_err());
        std::fs::remove_file(conf.join("firewall.json")).unwrap();
        std::fs::write(path.join("outside"), b"{\"v\":1}").unwrap();
        symlink(path.join("outside"), conf.join("firewall.json")).unwrap();
        assert!(read_at(uid, &home, "firewall.json", 20).is_err());
        std::fs::remove_file(conf.join("firewall.json")).unwrap();
        std::fs::create_dir(conf.join("firewall.json")).unwrap();
        assert!(read_at(uid, &home, "firewall.json", 20).is_err());
        std::fs::remove_dir_all(&path).unwrap();
    }
}
