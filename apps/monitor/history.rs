//! Bounded, private traffic history, compatible with the existing LNH1 journal.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
const LIMIT: usize = 8 << 20;
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Count {
    pub rx: u64,
    pub tx: u64,
}
impl Count {
    fn add(&mut self, rx: u64, tx: u64) {
        self.rx = self.rx.saturating_add(rx);
        self.tx = self.tx.saturating_add(tx)
    }
}
#[derive(Default)]
pub struct History {
    pub path: PathBuf,
    pub buckets: BTreeMap<(u64, u64, String), Count>,
    loaded: bool,
    pub enabled: bool,
    last_save: u64,
}
pub fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb88320
            } else {
                c >> 1
            }
        }
    }
    !c
}
fn var(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let b = (n & 127) as u8;
        n >>= 7;
        out.push(b | if n != 0 { 128 } else { 0 });
        if n == 0 {
            break;
        }
    }
}
fn take(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0;
    for i in 0..10 {
        let b = *bytes.get(*at)?;
        *at += 1;
        if i == 9 && b > 1 {
            return None;
        }
        v |= ((b & 127) as u64) << (i * 7);
        if b & 128 == 0 {
            return Some(v);
        }
    }
    None
}
fn record(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    if body.len() > 65535 {
        return;
    }
    let start = out.len();
    out.push(kind);
    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.extend_from_slice(body);
    let checksum = crc(&out[start..]);
    out.extend_from_slice(&checksum.to_le_bytes());
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:@+~/-".contains(&b))
}
impl History {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }
    pub fn decode(&mut self, bytes: &[u8]) {
        if bytes.len() > LIMIT || bytes.get(..8) != Some(b"LNH1\x01\0\0\0") {
            return;
        }
        let mut at = 8;
        let mut apps = BTreeMap::from([(0, "other".to_owned())]);
        while at + 7 <= bytes.len() {
            let kind = bytes[at];
            let len = u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]) as usize;
            let end = at + 3 + len;
            if end + 4 > bytes.len() {
                break;
            }
            let want = u32::from_le_bytes(bytes[end..end + 4].try_into().unwrap());
            if crc(&bytes[at..end]) != want {
                break;
            }
            let b = &bytes[at + 3..end];
            let mut p = 0;
            let parsed = (|| -> Option<()> {
                if kind == 1 {
                    let i = take(b, &mut p)?;
                    let id = std::str::from_utf8(&b[p..]).ok()?;
                    if i == 0 || i > 1024 || !valid_id(id) {
                        return None;
                    }
                    apps.insert(i, id.to_owned());
                } else if kind == 2 || kind == 3 {
                    let time = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as u64;
                    p = 4;
                    let rx = take(b, &mut p)?;
                    let tx = take(b, &mut p)?;
                    let n = take(b, &mut p)?;
                    if n > 1024 {
                        return None;
                    }
                    let step = if kind == 2 { 60 } else { 3600 };
                    let time = time.checked_mul(step)?;
                    let mut rows = vec![((time, step, String::new()), Count { rx, tx })];
                    for _ in 0..n {
                        let i = take(b, &mut p)?;
                        let rx = take(b, &mut p)?;
                        let tx = take(b, &mut p)?;
                        let app = apps.get(&i)?.clone();
                        rows.push(((time, step, app), Count { rx, tx }));
                    }
                    if p != b.len() {
                        return None;
                    }
                    for (k, v) in rows {
                        self.buckets.entry(k).or_default().add(v.rx, v.tx);
                    }
                } else {
                    return None;
                }
                Some(())
            })();
            if parsed.is_none() {
                break;
            }
            at = end + 4;
        }
    }
    pub fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Ok(f) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            if f.metadata()
                .map(|m| {
                    m.is_file() && m.uid() == unsafe { libc::getuid() } && m.len() <= LIMIT as u64
                })
                .unwrap_or(false)
            {
                let mut bytes = Vec::new();
                let _ = f.take(LIMIT as u64 + 1).read_to_end(&mut bytes);
                self.decode(&bytes);
            }
        }
    }
    pub fn add(&mut self, now: u64, app: &str, rx: u64, tx: u64) {
        if !self.enabled || (!app.is_empty() && !valid_id(app)) {
            return;
        }
        self.load();
        let app = if self
            .buckets
            .keys()
            .filter(|k| !k.2.is_empty())
            .map(|k| &k.2)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 1024
            && !self.buckets.keys().any(|k| k.2 == app)
        {
            "other"
        } else {
            app
        };
        self.buckets
            .entry((now / 60 * 60, 60, app.into()))
            .or_default()
            .add(rx, tx);
    }
    pub fn compact(&mut self, now: u64) {
        let old = std::mem::take(&mut self.buckets);
        for ((time, step, app), count) in old {
            if time.saturating_add(30 * 86400) < now {
                continue;
            }
            let step = if time.saturating_add(7 * 86400) < now {
                3600
            } else {
                step
            };
            self.buckets
                .entry((time / step * step, step, app))
                .or_default()
                .add(count.rx, count.tx);
        }
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut out = b"LNH1\x01\0\0\0".to_vec();
        let apps: Vec<_> = self
            .buckets
            .keys()
            .filter(|k| !k.2.is_empty())
            .map(|k| k.2.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for (i, app) in apps.iter().enumerate() {
            let mut body = Vec::new();
            var(&mut body, i as u64 + 1);
            body.extend_from_slice(app.as_bytes());
            record(&mut out, 1, &body);
        }
        let mut slots: BTreeMap<(u64, u64), Vec<(&str, &Count)>> = BTreeMap::new();
        for ((time, step, app), count) in &self.buckets {
            slots.entry((*time, *step)).or_default().push((app, count));
        }
        for ((time, step), rows) in slots {
            let mut body = ((time / step) as u32).to_le_bytes().to_vec();
            let total = rows
                .iter()
                .find(|r| r.0.is_empty())
                .map(|r| r.1)
                .cloned()
                .unwrap_or_default();
            var(&mut body, total.rx);
            var(&mut body, total.tx);
            var(
                &mut body,
                rows.iter().filter(|r| !r.0.is_empty()).count() as u64,
            );
            for (app, count) in rows {
                if app.is_empty() {
                    continue;
                }
                if let Ok(i) = apps.binary_search(&app.to_owned()) {
                    var(&mut body, i as u64 + 1);
                    var(&mut body, count.rx);
                    var(&mut body, count.tx);
                }
            }
            record(&mut out, if step == 60 { 2 } else { 3 }, &body);
        }
        out
    }
    pub fn save(&mut self, now: u64) -> std::io::Result<()> {
        if self.path.as_os_str().is_empty() || now / 60 == self.last_save / 60 {
            return Ok(());
        }
        self.compact(now);
        let bytes = self.encode();
        if bytes.len() > LIMIT {
            return Err(std::io::Error::other("history limit"));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("history path"))?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let temp = self.path.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            super::worker::wall()
        ));
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        let result = (|| {
            f.write_all(&bytes)?;
            f.sync_all()?;
            fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result?;
        self.last_save = now;
        Ok(())
    }
    pub fn clear(&mut self) -> bool {
        self.buckets.clear();
        self.loaded = true;
        self.last_save = 0;
        match fs::remove_file(&self.path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
    pub fn query(&mut self, from: u64, step: u64, n: usize, app: &str, now: u64) -> Value {
        if n == 0
            || n > 1500
            || step == 0
            || step
                .checked_mul(n as u64)
                .and_then(|span| from.checked_add(span))
                .is_none()
        {
            return Value::Null;
        }
        self.load();
        let mut rx = vec![0.; n];
        let mut tx = vec![0.; n];
        let mut top: BTreeMap<String, Count> = BTreeMap::new();
        let end = from + step * n as u64;
        let mut res = 0;
        for ((time, width, id), count) in &self.buckets {
            let a = (*time).max(from);
            let b = time.saturating_add(*width).min(end);
            if a >= b {
                continue;
            }
            if !id.is_empty() {
                let portion = (b - a) as f64 / *width as f64;
                top.entry(id.clone()).or_default().add(
                    (count.rx as f64 * portion) as u64,
                    (count.tx as f64 * portion) as u64,
                )
            }
            if id != app {
                continue;
            }
            res = res.max(*width);
            let first = ((a - from) / step) as usize;
            let last = ((b - 1 - from) / step) as usize;
            for i in first..=last.min(n - 1) {
                let overlap = (b.min(from + (i + 1) as u64 * step))
                    .saturating_sub(a.max(from + i as u64 * step))
                    as f64
                    / *width as f64;
                rx[i] += count.rx as f64 * overlap;
                tx[i] += count.tx as f64 * overlap;
            }
        }
        let mut top: Vec<_> = top.into_iter().collect();
        top.sort_by_key(|(_, v)| std::cmp::Reverse(v.rx.saturating_add(v.tx)));
        top.truncate(32);
        json!({"res":res,"oldest":self.buckets.keys().next().map(|k|k.0).unwrap_or(0),"now":now,"rx":rx,"tx":tx,"top":top.into_iter().map(|(id,c)|json!([id,c.rx,c.tx])).collect::<Vec<_>>()})
    }
}
