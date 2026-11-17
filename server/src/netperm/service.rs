//! Root service: fixed-path policies, authenticated Unix clients, cgroup lifecycle and accounting.
use crate::{
    bpf::{self, Maps, Ring, Spec},
    compile::{self, NpNode},
    diag::{Diag, Socket},
    dns::Monitor,
    policy::{self, Lists, Names, Store, NAMES},
    shared::*,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const ROOT: &str = "/sys/fs/cgroup";
const SERVICE: &str = "/sys/fs/cgroup/system.slice/lordos.service";
const SOCK: &str = "/run/lordos/netperm.sock";
const CLIENT_OUT: usize = 256 << 10;
static QUIT: AtomicBool = AtomicBool::new(false);
extern "C" fn terminate(_: i32) {
    QUIT.store(true, Ordering::Relaxed)
}
pub fn mono() -> i64 {
    unsafe {
        let mut t = std::mem::zeroed::<libc::timespec>();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
        t.tv_sec
    }
}
fn nanos() -> u64 {
    unsafe {
        let mut t = std::mem::zeroed::<libc::timespec>();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
        t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
    }
}
fn dir(p: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(p)
}
fn cg(parent: &str, leaf: &str) -> u64 {
    dir(parent)
        .map(|f| bpf::cgroup_id(f.as_raw_fd(), leaf))
        .unwrap_or(0)
}
fn address(f: i32, a: &[u8; 16]) -> String {
    match f {
        4 => Ipv4Addr::new(a[0], a[1], a[2], a[3]).to_string(),
        6 => Ipv6Addr::from(*a).to_string(),
        _ => String::new(),
    }
}
fn protocol(p: u8) -> &'static str {
    match p {
        1 => "tcp",
        2 => "udp",
        _ => "other",
    }
}
fn reason(p: u8) -> &'static str {
    match p {
        0 => "restricted",
        1 => "rule",
        2 => "trusted",
        4 => "list",
        _ => "mode",
    }
}
fn flag(f: u32) -> &'static str {
    if f.count_ones() == 1 {
        policy::FLAGS
            .get(f.trailing_zeros() as usize)
            .copied()
            .unwrap_or("")
    } else {
        ""
    }
}
fn rate(new: u64, old: u64, dt: f64) -> f64 {
    if dt > 0.0 && new >= old {
        (new - old) as f64 / dt
    } else {
        0.0
    }
}
fn rounded(v: f64) -> Value {
    json!(v.round() as u64)
}
#[derive(Default)]
struct Scope {
    name: String,
    app: String,
    id: u64,
    idx: u32,
    generation: u32,
    domains: bool,
    policy: NpApp,
    count: NpCount,
    cuts: u64,
    rx_rate: f64,
    tx_rate: f64,
}
struct Session {
    uid: u32,
    store: Store,
    lists: Option<Lists>,
    scopes: Vec<Scope>,
    user_cg: u64,
    user_links: Vec<OwnedFd>,
    blkset: u32,
    blocks: HashMap<(i32, i32, [u8; 16]), bool>,
    learned: usize,
    dirty: bool,
    watch: i32,
}
impl Session {
    fn new(uid: u32) -> Self {
        Self {
            uid,
            store: Store::default(),
            lists: None,
            scopes: vec![],
            user_cg: 0,
            user_links: vec![],
            blkset: 0,
            blocks: HashMap::new(),
            learned: 0,
            dirty: true,
            watch: -1,
        }
    }
    fn load(&mut self) {
        match policy::read_user(self.uid, "firewall.json", 4 << 20) {
            Ok(Some(b)) => match Store::parse(&b) {
                Ok(s) => self.store = s,
                Err(e) => eprintln!(
                    "axia-netperm: uid {} invalid firewall.json ({e}); keeping previous rules",
                    self.uid
                ),
            },
            Ok(None) => self.store = Store::default(),
            Err(e) => eprintln!(
                "axia-netperm: uid {} firewall.json: {e}; keeping previous rules",
                self.uid
            ),
        }
        match policy::read_user(self.uid, "fwlists.txt", 32 << 20) {
            Ok(Some(b)) => match Lists::parse(&b) {
                Ok(l) => self.lists = l,
                Err(e) => eprintln!(
                    "axia-netperm: uid {} invalid fwlists.txt ({e}); keeping previous lists",
                    self.uid
                ),
            },
            Ok(None) => self.lists = None,
            Err(e) => eprintln!(
                "axia-netperm: uid {} fwlists.txt: {e}; keeping previous lists",
                self.uid
            ),
        }
    }
}
fn errno_name(errno: i32) -> String {
    // Stable symbols used by sdk/errors.js, including on libc implementations
    // without strerrorname_np. Unknown values retain their numeric identity.
    let name = match errno {
        libc::EPERM => "EPERM",
        libc::EACCES => "EACCES",
        libc::ENOENT => "ENOENT",
        libc::EINVAL => "EINVAL",
        libc::ENOMEM => "ENOMEM",
        libc::ENOSYS => "ENOSYS",
        libc::EOPNOTSUPP => "EOPNOTSUPP",
        libc::E2BIG => "E2BIG",
        libc::ENOSPC => "ENOSPC",
        libc::EMFILE => "EMFILE",
        libc::ENFILE => "ENFILE",
        libc::EBADF => "EBADF",
        libc::EFAULT => "EFAULT",
        libc::EBUSY => "EBUSY",
        _ => return errno.to_string(),
    };
    name.into()
}
fn io_code(code: &str, e: io::Error) -> String {
    format!(
        "{code} errno={}",
        errno_name(e.raw_os_error().unwrap_or(libc::EIO))
    )
}

struct Engine {
    maps: Maps,
    ring: Ring,
    programs: Vec<(Spec, OwnedFd)>,
    devices: bool,
    bluetooth: bool,
    accounting: bool,
}
impl Engine {
    fn new() -> Result<Self, String> {
        if bpf::PROGRAMS.is_empty() {
            return Err("fw.bpfNotBuilt".into());
        }
        let maps = Maps::create().map_err(|e| io_code("fw.bpfMaps", e))?;
        let ring = Ring::open(maps.fd("np_events"), NP_RINGBUF as usize)
            .map_err(|e| io_code("fw.bpfRing", e))?;
        let mut progs = vec![];
        for spec in bpf::SPECS {
            let p = bpf::PROGRAMS
                .iter()
                .find(|p| p.section == spec.section)
                .ok_or_else(|| format!("fw.bpfMissing program={}", spec.section))?;
            match bpf::load(spec, p, &maps) {
                Ok(fd) => progs.push((*spec, fd)),
                Err(e) if spec.optional => {
                    eprintln!("axia-netperm: optional {} unavailable: {e}", spec.section)
                }
                Err(e) => {
                    eprintln!("axia-netperm: {}: {e}", spec.section);
                    return Err(format!(
                        "fw.bpfRejected program={} errno={}",
                        spec.section,
                        errno_name(e.errno)
                    ));
                }
            }
        }
        let mut hid = 0;
        let mut chr = false;
        for l in fs::read_to_string("/proc/devices")
            .unwrap_or_default()
            .lines()
        {
            if l == "Character devices:" {
                chr = true
            } else if l == "Block devices:" {
                chr = false
            } else if chr {
                let mut w = l.split_whitespace();
                if let (Some(n), Some("hidraw")) = (w.next(), w.next()) {
                    hid = n.parse().unwrap_or(0)
                }
            }
        }
        maps.put(
            "np_cfg",
            &0u32,
            &NpCfg {
                level: 4,
                on: 1,
                user_level: 3,
                hidraw_major: hid,
            },
            0,
        )
        .map_err(|e| io_code("fw.bpfConfig", e))?;
        Ok(Self {
            devices: progs.iter().any(|(s, _)| s.attach == 6),
            bluetooth: progs.iter().any(|(s, _)| s.attach == 43),
            accounting: progs.iter().filter(|(s, _)| s.attach <= 1).count() == 2,
            maps,
            ring,
            programs: progs,
        })
    }
}
struct Client {
    stream: UnixStream,
    uid: u32,
    input: Vec<u8>,
    output: Vec<u8>,
}
impl Client {
    fn enqueue(&mut self, ty: u8, value: &Value) {
        let Ok(b) = serde_json::to_vec(value) else {
            return;
        };
        if b.len() + self.output.len() + 5 > CLIENT_OUT {
            return;
        }
        self.output
            .extend_from_slice(&(b.len() as u32 + 1).to_le_bytes());
        self.output.push(ty);
        self.output.extend_from_slice(&b);
    }
    fn flush(&mut self) -> io::Result<()> {
        while !self.output.is_empty() {
            match self.stream.write(&self.output) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.output.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn read(&mut self) -> io::Result<bool> {
        let mut reload = false;
        let mut buf = [0; 68];
        for _ in 0..64 {
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    self.input.extend_from_slice(&buf[..n]);
                    while self.input.len() >= 5 {
                        if self.input[..5] != [1, 0, 0, 0, 1] {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                        self.input.drain(..5);
                        reload = true
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(reload)
    }
}
#[derive(Default)]
struct System {
    id: u64,
    links: Vec<OwnedFd>,
    count: NpCount,
    rx_rate: f64,
    tx_rate: f64,
}
const SYSTEMS: [(&str, &str, &str); 3] = [
    (
        "system:dns",
        "/sys/fs/cgroup/system.slice",
        "systemd-resolved.service",
    ),
    (
        "system:time",
        "/sys/fs/cgroup/system.slice",
        "systemd-timesyncd.service",
    ),
    ("system:network", SERVICE, "netd"),
];
#[derive(Default)]
struct Flow {
    count: NpFlow,
    rx_rate: f64,
    tx_rate: f64,
    active: u64,
    tick: u64,
}
struct Service {
    engine: Option<Engine>,
    error: String,
    names: Names,
    dns: Option<Monitor>,
    sessions: BTreeMap<u32, Session>,
    clients: Vec<Client>,
    indices: HashMap<(u32, String), u32>,
    next_idx: u32,
    next_set: u32,
    installed: HashMap<(u32, i32, i32, [u8; 16]), (u32, NpList)>,
    links: Vec<OwnedFd>,
    service_cg: u64,
    system: [System; 3],
    flows: HashMap<NpFlowKey, Flow>,
    tick: u64,
    counts_ns: u64,
    diag: Option<Diag>,
    diag_warned: bool,
    notify: OwnedFd,
    root_watch: i32,
    dns_source: &'static str,
    sound: HashSet<u32>,
}
impl Service {
    fn new() -> io::Result<Self> {
        let notify = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if notify < 0 {
            return Err(io::Error::last_os_error());
        }
        let notify = unsafe { OwnedFd::from_raw_fd(notify) };
        let (engine, error) = match Engine::new() {
            Ok(e) => (Some(e), String::new()),
            Err(e) => {
                eprintln!("axia-netperm: BPF unavailable ({e}); no rule is enforced");
                (None, e)
            }
        };
        let dns = engine.as_ref().map(|_| Monitor::default());
        let s = Self {
            engine,
            error,
            names: Names::default(),
            dns,
            sessions: BTreeMap::new(),
            clients: vec![],
            indices: HashMap::new(),
            next_idx: 0,
            next_set: 0,
            installed: HashMap::new(),
            links: vec![],
            service_cg: 0,
            system: Default::default(),
            flows: HashMap::new(),
            tick: 0,
            counts_ns: 0,
            diag: None,
            diag_warned: false,
            notify,
            root_watch: -1,
            dns_source: "none",
            sound: HashSet::new(),
        };
        s.watch("/sys/fs/cgroup/system.slice");
        Ok(s)
    }
    fn watch(&self, path: &str) -> i32 {
        let p = policy::cs(path);
        unsafe {
            libc::inotify_add_watch(
                self.notify.as_raw_fd(),
                p.as_ptr(),
                libc::IN_CREATE | libc::IN_DELETE | libc::IN_ONLYDIR,
            )
        }
    }
    fn status(&self) -> Value {
        json!({"bpf":self.engine.is_some(),"devices":self.engine.as_ref().is_some_and(|e|e.devices),"bluetooth":self.engine.as_ref().is_some_and(|e|e.bluetooth),"bytes":self.engine.as_ref().is_some_and(|e|e.accounting),"dns":self.dns.as_ref().map_or("none",Monitor::source),"error":self.error})
    }
    fn send(&mut self, uid: u32, ty: u8, v: &Value) {
        for c in &mut self.clients {
            if c.uid == uid {
                c.enqueue(ty, v)
            }
        }
    }
    fn index(&mut self, uid: u32, app: &str) -> u32 {
        let k = (uid, app.to_owned());
        if let Some(&i) = self.indices.get(&k) {
            return i;
        }
        self.next_idx = self
            .next_idx
            .checked_add(1)
            .expect("policy index exhausted");
        self.indices.insert(k, self.next_idx);
        self.next_idx
    }
    fn attach(&mut self) {
        let Some(e) = &self.engine else { return };
        let id = cg("/sys/fs/cgroup/system.slice", "lordos.service");
        if id == self.service_cg {
            return;
        }
        self.links.clear();
        self.service_cg = 0;
        if id == 0 {
            return;
        }
        let Ok(d) = dir(SERVICE) else { return };
        let mut links = vec![];
        for (spec, fd) in &e.programs {
            match bpf::link(fd.as_raw_fd(), d.as_raw_fd(), spec.attach) {
                Ok(l) => links.push(l),
                Err(err) => {
                    eprintln!("axia-netperm: attaching {}: {err}", spec.section);
                    return;
                }
            }
        }
        self.links = links;
        self.service_cg = id;
        if self.root_watch >= 0 {
            unsafe {
                libc::inotify_rm_watch(self.notify.as_raw_fd(), self.root_watch);
            }
        }
        self.root_watch = self.watch(SERVICE);
    }
    fn put_node(&mut self, idx: u32, generation: u32, n: &NpNode) -> bool {
        let Some(e) = &self.engine else { return false };
        let key = (idx, n.fam, n.prefix, n.addr);
        if let Some((g, l)) = self.installed.get_mut(&key) {
            if l == &n.list {
                *g = generation;
                return true;
            }
        }
        let r = if n.fam == 4 {
            e.maps.put("np_lpm4", &compile::key4(idx, n), &n.list, 0)
        } else {
            e.maps.put("np_lpm6", &compile::key6(idx, n), &n.list, 0)
        };
        if let Err(err) = r {
            eprintln!("axia-netperm: trie entry: {err}");
            return false;
        }
        self.installed.insert(key, (generation, n.list));
        true
    }
    fn prune(&mut self, idx: u32, generation: u32) {
        let Some(e) = &self.engine else { return };
        self.installed.retain(|&(i, f, p, a), (g, _)| {
            if i != idx || generation != 0 && *g == generation {
                return true;
            }
            let n = NpNode {
                fam: f,
                prefix: p,
                addr: a,
                ..Default::default()
            };
            if f == 4 {
                e.maps.delete("np_lpm4", &compile::key4(idx, &n))
            } else {
                e.maps.delete("np_lpm6", &compile::key6(idx, &n))
            }
            false
        });
    }
    fn sync_scope(&mut self, store: &Store, sc: &mut Scope, blkset: u32) {
        if self.engine.is_none() {
            return;
        }
        let app = policy::cs(&sc.app);
        let mut c = unsafe {
            compile::compile(
                store.0,
                app.as_ptr(),
                sc.idx,
                self.names.0,
                NAMES.as_ptr(),
                mono(),
            )
        };
        if c.truncated > 0 {
            eprintln!(
                "axia-netperm: {}: {} rules did not fit the map lists",
                sc.app, c.truncated
            )
        }
        sc.generation = sc.generation.wrapping_add(1).max(1);
        for n in &c.nodes {
            self.put_node(sc.idx, sc.generation, n);
        }
        self.prune(sc.idx, sc.generation);
        sc.domains = c.has_domains;
        c.app.blk = blkset;
        sc.policy = c.app;
        let e = self.engine.as_ref().unwrap();
        if let Err(err) = e.maps.put("np_apps", &sc.id, &c.app, 0) {
            eprintln!("axia-netperm: np_apps: {err}")
        }
        let _ = e.maps.put("np_counts", &sc.id, &NpCount::default(), 1);
    }
    fn drop_scope(&mut self, sc: &Scope) {
        if let Some(e) = &self.engine {
            e.maps.delete("np_apps", &sc.id);
            e.maps.delete("np_counts", &sc.id);
            self.flows.retain(|k, _| {
                if k.cgid == sc.id {
                    e.maps.delete("np_flows", k);
                    false
                } else {
                    true
                }
            });
        }
        self.prune(sc.idx, 0);
    }
    fn block(&self, set: u32, key: (i32, i32, [u8; 16]), del: bool) -> bool {
        let Some(e) = &self.engine else { return false };
        let (f, p, a) = key;
        let n = NpNode {
            fam: f,
            prefix: p,
            addr: a,
            ..Default::default()
        };
        if f == 4 {
            let k = compile::key4(set, &n);
            if del {
                e.maps.delete("np_blk4", &k);
                true
            } else {
                e.maps.put("np_blk4", &k, &1u8, 0).is_ok()
            }
        } else {
            let k = compile::key6(set, &n);
            if del {
                e.maps.delete("np_blk6", &k);
                true
            } else {
                e.maps.put("np_blk6", &k, &1u8, 0).is_ok()
            }
        }
    }
    fn sync_blocks(&mut self, s: &mut Session) -> bool {
        if self.engine.is_none() {
            return false;
        }
        let mut want = HashMap::new();
        if let Some(l) = &s.lists {
            if s.blkset == 0 {
                self.next_set = self.next_set.checked_add(1).expect("list index exhausted");
                s.blkset = self.next_set
            }
            for (f, a, p) in l.networks() {
                want.insert((f, p, a), false);
            }
            let mut learned = 0;
            for (f, a, n) in self.names.each(mono()) {
                if learned >= 16384 {
                    break;
                }
                let key = (f, if f == 4 { 32 } else { 128 }, a);
                if l.blocks(&n) && !want.contains_key(&key) {
                    want.insert(key, true);
                    learned += 1
                }
            }
        }
        let mut added = false;
        s.blocks.retain(|&k, _| {
            if want.contains_key(&k) {
                true
            } else {
                self.block(s.blkset, k, true);
                false
            }
        });
        for (k, v) in want {
            if s.blocks.contains_key(&k) {
                s.blocks.insert(k, v);
            } else if self.block(s.blkset, k, false) {
                s.blocks.insert(k, v);
                added = true
            }
        }
        s.learned = s.blocks.values().filter(|&&learned| learned).count();
        added
    }
    fn user_scope(&mut self, s: &mut Session) {
        let Some(e) = &self.engine else { return };
        let parent = format!("{ROOT}/user.slice/user-{}.slice", s.uid);
        let leaf = format!("user@{}.service", s.uid);
        let id = cg(&parent, &leaf);
        if id != s.user_cg {
            s.user_links.clear();
            if s.user_cg != 0 {
                e.maps.delete("np_union", &s.user_cg);
                e.maps.delete("np_counts", &s.user_cg)
            }
            s.user_cg = 0;
            if id != 0 {
                if let Ok(d) = dir(&format!("{parent}/{leaf}")) {
                    let mut links = vec![];
                    let mut ok = true;
                    for (spec, fd) in &e.programs {
                        if spec.attach <= 1 {
                            continue;
                        }
                        match bpf::link(fd.as_raw_fd(), d.as_raw_fd(), spec.attach) {
                            Ok(l) => links.push(l),
                            Err(err) => {
                                eprintln!("axia-netperm: uid {} user@ attach: {err}", s.uid);
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        s.user_cg = id;
                        s.user_links = links
                    }
                }
            }
        }
        if s.user_cg != 0 {
            let flags = s
                .scopes
                .iter()
                .fold(0, |v, sc| v | (sc.policy.flags & !NP_R_NO_MIC));
            if flags == 0 {
                e.maps.delete("np_union", &s.user_cg)
            } else {
                let _ = e.maps.put(
                    "np_union",
                    &s.user_cg,
                    &NpApp {
                        flags,
                        trusted: 1,
                        ..Default::default()
                    },
                    0,
                );
            }
        }
    }
    fn scan_sound(&mut self) {
        let Some(e) = &self.engine else { return };
        let mut now = HashSet::new();
        for de in fs::read_dir("/sys/class/sound")
            .into_iter()
            .flatten()
            .flatten()
            .take(1024)
        {
            let name = de.file_name().to_string_lossy().into_owned();
            if name.len() > 64 {
                continue;
            }
            let p = de.path().join("dev");
            let Ok(mut f) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(p)
            else {
                continue;
            };
            let mut b = String::new();
            if Read::by_ref(&mut f)
                .take(32)
                .read_to_string(&mut b)
                .is_err()
            {
                continue;
            }
            let Some((ma, mi)) = b.trim().split_once(':') else {
                continue;
            };
            if ma != "116" {
                continue;
            }
            let Ok(mi) = mi.parse::<u32>() else { continue };
            let kind = if name.starts_with("pcmC") && name.len() >= 6 && name.ends_with('c') {
                2u32
            } else {
                1u32
            };
            let _ = e.maps.put("np_snd", &mi, &kind, 0);
            now.insert(mi);
        }
        for mi in self.sound.difference(&now) {
            e.maps.delete("np_snd", mi)
        }
        self.sound = now;
    }
    fn scan(&mut self) {
        self.scan_sound();
        let mut old = std::mem::take(&mut self.sessions);
        let mut sweeps = vec![];
        for de in fs::read_dir(SERVICE).into_iter().flatten().flatten() {
            if !de.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = de.file_name().to_string_lossy().into_owned();
            let Some(uid) = name.strip_prefix("lordos-session-").and_then(|s| {
                if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) {
                    s.parse::<u32>().ok()
                } else {
                    None
                }
            }) else {
                continue;
            };
            if self.sessions.len() >= 64 {
                continue;
            }
            let is_new = !old.contains_key(&uid);
            let mut s = old.remove(&uid).unwrap_or_else(|| {
                let mut s = Session::new(uid);
                s.load();
                s
            });
            if is_new && self.sync_blocks(&mut s) {
                sweeps.push(uid)
            }
            let path = format!("{SERVICE}/{name}");
            let wd = self.watch(&path);
            if wd >= 0 {
                s.watch = wd
            }
            let mut scopes = std::mem::take(&mut s.scopes);
            if let Ok(entries) = fs::read_dir(&path) {
                for de in entries.flatten() {
                    if !de.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let name = de.file_name().to_string_lossy().into_owned();
                    let Some(app) = scope_app(&name) else {
                        continue;
                    };
                    let id = cg(&path, &name);
                    if id == 0 {
                        continue;
                    }
                    let mut sc = if let Some(i) =
                        scopes.iter().position(|sc| sc.id == id && sc.name == name)
                    {
                        scopes.swap_remove(i)
                    } else {
                        let idx = self.index(uid, &app);
                        let mut sc = Scope {
                            name,
                            app,
                            id,
                            idx,
                            ..Default::default()
                        };
                        self.sync_scope(
                            &s.store,
                            &mut sc,
                            if s.lists.is_some() { s.blkset } else { 0 },
                        );
                        sc
                    };
                    sc.name = de.file_name().to_string_lossy().into_owned();
                    s.scopes.push(sc);
                }
            } else {
                s.scopes = scopes;
                self.sessions.insert(uid, s);
                continue;
            }
            for sc in scopes {
                self.drop_scope(&sc)
            }
            self.user_scope(&mut s);
            self.sessions.insert(uid, s);
        }
        for (_, mut s) in old {
            for sc in &s.scopes {
                self.drop_scope(sc)
            }
            s.lists = None;
            self.sync_blocks(&mut s);
            if let Some(e) = &self.engine {
                if s.user_cg != 0 {
                    e.maps.delete("np_union", &s.user_cg);
                    e.maps.delete("np_counts", &s.user_cg)
                }
            }
            if s.watch >= 0 {
                unsafe {
                    libc::inotify_rm_watch(self.notify.as_raw_fd(), s.watch);
                }
            }
        }
        for uid in sweeps {
            self.sweep(Some(uid))
        }
    }
    fn reload(&mut self, uid: u32) {
        if let Some(mut s) = self.sessions.remove(&uid) {
            s.load();
            self.sync_blocks(&mut s);
            for sc in &mut s.scopes {
                self.sync_scope(&s.store, sc, if s.lists.is_some() { s.blkset } else { 0 })
            }
            self.user_scope(&mut s);
            s.dirty = true;
            self.sessions.insert(uid, s);
            self.sweep(Some(uid));
        }
    }
    fn system(&mut self) {
        let Some(e) = &self.engine else { return };
        if !e.accounting {
            return;
        }
        for (i, (_, parent, leaf)) in SYSTEMS.iter().enumerate() {
            let id = cg(parent, leaf);
            if id == self.system[i].id {
                continue;
            }
            let sys = &mut self.system[i];
            sys.links.clear();
            if sys.id != 0 {
                e.maps.delete("np_sysc", &sys.id)
            }
            *sys = System::default();
            if id == 0 {
                continue;
            }
            let _ = e.maps.put("np_sysc", &id, &NpCount::default(), 0);
            if *leaf != "netd" {
                if let Ok(d) = dir(&format!("{parent}/{leaf}")) {
                    for (spec, fd) in &e.programs {
                        if spec.attach > 1 {
                            continue;
                        }
                        match bpf::link(fd.as_raw_fd(), d.as_raw_fd(), spec.attach) {
                            Ok(l) => sys.links.push(l),
                            Err(err) => {
                                eprintln!("axia-netperm: accounting {leaf}: {err}");
                                break;
                            }
                        }
                    }
                }
            }
            sys.id = id;
        }
    }
    fn dns(&mut self, packets: Vec<Vec<u8>>) {
        let now = mono();
        let mut changed = HashSet::new();
        for p in packets {
            if unsafe {
                crate::dnsmap::dnsmap_learn(self.names.0, NAMES.as_ptr(), p.as_ptr(), p.len(), now)
            } > 0
            {
                if let Some(addresses) = crate::dnsmap::response_addresses(&p) {
                    changed.extend(addresses);
                }
            }
        }
        if changed.is_empty() {
            return;
        }
        // Only addresses in this reply are recompiled. Large subscribed lists and
        // unrelated domain rules are not walked for each resolver notification.
        let mut sessions = std::mem::take(&mut self.sessions);
        for (fam, addr) in changed {
            let names = self.names.lookup(fam, &addr, now);
            if names.is_empty() {
                continue;
            }
            let key = (fam, if fam == 4 { 32 } else { 128 }, addr);
            for session in sessions.values_mut() {
                if let Some(lists) = &session.lists {
                    if lists.blocks(&names) {
                        if !session.blocks.contains_key(&key)
                            && session.learned < 16384
                            && self.block(session.blkset, key, false)
                        {
                            session.blocks.insert(key, true);
                            session.learned += 1;
                        }
                    } else if session.blocks.get(&key) == Some(&true) {
                        self.block(session.blkset, key, true);
                        session.blocks.remove(&key);
                        session.learned -= 1;
                    }
                }
                for scope in &mut session.scopes {
                    if !scope.domains {
                        continue;
                    }
                    let app = policy::cs(&scope.app);
                    let mut node = NpNode::default();
                    let mut truncated = 0;
                    let rc = unsafe {
                        compile::np_compile_host(
                            session.store.0,
                            app.as_ptr(),
                            fam,
                            addr.as_ptr(),
                            names.as_ptr(),
                            names.len() as i32,
                            &mut node,
                            &mut truncated,
                        )
                    };
                    if rc != 1 || !self.put_node(scope.idx, scope.generation, &node) {
                        continue;
                    }
                    let has = if fam == 4 {
                        &mut scope.policy.has4
                    } else {
                        &mut scope.policy.has6
                    };
                    if *has == 0 {
                        *has = 1;
                        if let Some(engine) = &self.engine {
                            let _ = engine.maps.put("np_apps", &scope.id, &scope.policy, 0);
                        }
                    }
                }
            }
        }
        self.sessions = sessions;
    }
    fn find(&self, id: u64) -> Option<(u32, usize)> {
        for (&uid, s) in &self.sessions {
            if let Some(i) = s.scopes.iter().position(|sc| sc.id == id) {
                return Some((uid, i));
            }
        }
        None
    }
    fn diag(&mut self) -> bool {
        if self.diag.is_none() {
            match Diag::open() {
                Ok(d) => self.diag = Some(d),
                Err(e) => {
                    if !self.diag_warned {
                        eprintln!("axia-netperm: sock_diag: {e}; packet drops still apply");
                        self.diag_warned = true
                    }
                    return false;
                }
            }
        }
        true
    }
    fn cut(&mut self, s: &Socket) -> bool {
        if !self.diag() {
            return false;
        }
        match self.diag.as_ref().unwrap().destroy(s) {
            Ok(()) => true,
            Err(e) => {
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EAGAIN | libc::EINTR | libc::EIO)
                ) {
                    self.diag = None
                }
                if !self.diag_warned
                    && matches!(e.raw_os_error(), Some(libc::EPERM | libc::EOPNOTSUPP))
                {
                    eprintln!("axia-netperm: socket destruction unavailable ({e}); packet drops still apply");
                    self.diag_warned = true
                }
                false
            }
        }
    }
    fn denied(&self, sc: &Scope, s: &Socket) -> Option<NpEvent> {
        let e = self.engine.as_ref()?;
        let a = &sc.policy;
        if a.can_deny == 0 && a.blk == 0 {
            return None;
        }
        let (f, addr) = remote(s);
        let node = NpNode {
            fam: f,
            prefix: if f == 4 { 32 } else { 128 },
            addr,
            ..Default::default()
        };
        let l = if f == 4 && a.has4 != 0 {
            e.maps.get("np_lpm4", &compile::key4(a.idx, &node))
        } else if f == 6 && a.has6 != 0 {
            e.maps.get("np_lpm6", &compile::key6(a.idx, &node))
        } else {
            None
        };
        let pr = if s.proto == 6 { 1 } else { 2 };
        let bk = NpBoundKey {
            cgid: sc.id,
            port: s.sport,
            proto: pr,
            ..Default::default()
        };
        let incoming = e.maps.get::<_, u8>("np_bound", &bk).is_some();
        let port = if incoming { s.sport } else { s.dport };
        let listed = a.blk != 0
            && if f == 4 {
                e.maps
                    .get::<_, u8>("np_blk4", &compile::key4(a.blk, &node))
                    .is_some()
            } else {
                e.maps
                    .get::<_, u8>("np_blk6", &compile::key6(a.blk, &node))
                    .is_some()
            };
        let lan = if f == 4 {
            np_is_lan4(&addr)
        } else {
            np_is_lan6(&addr)
        } as i32;
        let r = np_decide(
            a,
            l.as_ref(),
            incoming as i32,
            pr as i32,
            port as i32,
            lan,
            listed,
            np_is_loop(f, &addr),
        );
        if r.verdict != 1 {
            return None;
        }
        Some(NpEvent {
            cgid: sc.id,
            addr,
            port,
            fam: f as u8,
            proto: pr,
            dir: incoming as u8,
            verdict: 1,
            reason: r.reason as u8,
            record: r.record as u8,
            rule_id: r.rule_id,
            flag: r.flag,
            ..Default::default()
        })
    }
    fn sweep(&mut self, uid: Option<u32>) {
        if self.engine.is_none() || !self.diag() {
            return;
        }
        let mut cuts = vec![];
        for fam in [2, 10] {
            for (pr, states) in [
                (6, !((1 << 3) | (1 << 6) | (1 << 7) | (1 << 12))),
                (17, 1 << 1),
            ] {
                let Some(d) = &self.diag else { break };
                match d.each(fam, pr, states) {
                    Ok(v) => {
                        for s in v {
                            let Some((u, i)) = self.find(s.cgid) else {
                                continue;
                            };
                            if uid.is_some_and(|want| want != u) {
                                continue;
                            }
                            let sc = &self.sessions[&u].scopes[i];
                            if pr == 6 && s.state == 10 {
                                let k = NpBoundKey {
                                    cgid: sc.id,
                                    port: s.sport,
                                    proto: 1,
                                    ..Default::default()
                                };
                                let _ = self
                                    .engine
                                    .as_ref()
                                    .unwrap()
                                    .maps
                                    .put("np_bound", &k, &1u8, 0);
                                continue;
                            }
                            if s.dport == 0 {
                                continue;
                            }
                            if let Some(ev) = self.denied(sc, &s) {
                                cuts.push((s, ev))
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("axia-netperm: sock_diag dump: {err}");
                        self.diag = None;
                        break;
                    }
                }
            }
        }
        for (s, ev) in cuts {
            if !self.cut(&s) {
                continue;
            }
            let cookie = s.cookie[0] as u64 | (s.cookie[1] as u64) << 32;
            let mut k = NpSeenKey {
                cgid: ev.cgid,
                dir: 4,
                verdict: 1,
                ..Default::default()
            };
            k.addr[..8].copy_from_slice(&cookie.to_le_bytes());
            let e = self.engine.as_ref().unwrap();
            if e.maps.get::<_, u64>("np_seen", &k).is_some() {
                continue;
            }
            let _ = e.maps.put("np_seen", &k, &nanos(), 0);
            if let Some((u, i)) = self.find(ev.cgid) {
                let s = self.sessions.get_mut(&u).unwrap();
                s.scopes[i].cuts += 1;
                s.dirty = true
            }
            self.net_event(&ev, true);
        }
    }
    fn net_event(&mut self, ev: &NpEvent, cut: bool) {
        let Some((uid, i)) = self.find(ev.cgid) else {
            if let Some(s) = self
                .sessions
                .values()
                .find(|s| s.user_cg != 0 && s.user_cg == ev.cgid)
            {
                eprintln!(
                    "axia-netperm: uid {} user@: denied {}:{} ({})",
                    s.uid,
                    address(ev.fam as i32, &ev.addr),
                    ev.port,
                    flag(ev.flag)
                );
            }
            return;
        };
        let sc = &self.sessions[&uid].scopes[i];
        let app = sc.app.clone();
        let addr = address(ev.fam as i32, &ev.addr);
        let hosts = self.names.hosts(ev.fam as i32, &ev.addr, mono());
        let v = json!({"app":app,"v":if ev.verdict==1{"deny"}else{"allow"},"dir":if ev.dir==1{"in"}else{"out"},"proto":protocol(ev.proto),"addr":addr,"port":ev.port,"by":reason(ev.reason),"rule":ev.rule_id,"flag":flag(ev.flag),"record":ev.record,"hosts":hosts});
        self.send(uid, 11, &v);
        if ev.verdict == 1 {
            eprintln!(
                "axia-netperm: uid {uid} {app}: {} {} {}:{}, {} {}",
                if cut {
                    "cut open"
                } else if ev.cut != 0 {
                    "dropping open"
                } else {
                    "denied"
                },
                protocol(ev.proto),
                addr,
                ev.port,
                reason(ev.reason),
                flag(ev.flag)
            );
        }
    }
    fn event(&mut self, ev: NpEvent) {
        if ev.dir == 2 || ev.dir == 3 {
            let who = self
                .find(ev.cgid)
                .map(|(u, i)| format!("uid {u} {}", self.sessions[&u].scopes[i].app))
                .or_else(|| {
                    self.sessions
                        .values()
                        .find(|s| s.user_cg != 0 && s.user_cg == ev.cgid)
                        .map(|s| format!("uid {} user@", s.uid))
                });
            if let Some(who) = who {
                let minor = u32::from_le_bytes(ev.addr[..4].try_into().unwrap());
                eprintln!(
                    "axia-netperm: {who}: denied {} {}:{minor} ({})",
                    if ev.dir == 2 {
                        "device"
                    } else {
                        "Bluetooth socket"
                    },
                    ev.port,
                    flag(ev.flag)
                );
            }
            return;
        }
        let mut cut = false;
        if ev.cut != 0
            && ev.destroy != 0
            && self.find(ev.cgid).is_some()
            && matches!(ev.fam, 4 | 6)
            && matches!(ev.proto, 1 | 2)
        {
            let s = Socket {
                fam: if ev.fam == 4 { 2 } else { 10 },
                proto: if ev.proto == 1 { 6 } else { 17 },
                src: ev.laddr,
                dst: ev.addr,
                sport: ev.lport,
                dport: ev.rport,
                cookie: [ev.cookie as u32, (ev.cookie >> 32) as u32],
                ..Default::default()
            };
            cut = self.cut(&s)
        }
        if ev.cut > 1 {
            return;
        }
        self.net_event(&ev, cut)
    }
    fn flow_json(&self, id: u64) -> Value {
        let mut groups: BTreeMap<(u8, [u8; 16]), Vec<(&NpFlowKey, &Flow)>> = BTreeMap::new();
        for (k, f) in &self.flows {
            if k.cgid == id && self.tick.saturating_sub(f.active) <= 120 {
                groups.entry((k.fam, k.addr)).or_default().push((k, f));
            }
        }
        let mut agg = vec![];
        for ((fam, addr), mut ports) in groups {
            ports.sort_by(|a, b| {
                (b.1.rx_rate + b.1.tx_rate)
                    .total_cmp(&(a.1.rx_rate + a.1.tx_rate))
                    .then_with(|| (b.1.count.rx + b.1.count.tx).cmp(&(a.1.count.rx + a.1.count.tx)))
            });
            let (rx, tx, blocked, rrx, rtx) =
                ports
                    .iter()
                    .fold((0u64, 0u64, 0u64, 0.0, 0.0), |(a, b, c, d, e), (_, f)| {
                        (
                            a.saturating_add(f.count.rx),
                            b.saturating_add(f.count.tx),
                            c.saturating_add(f.count.blocked),
                            d + f.rx_rate,
                            e + f.tx_rate,
                        )
                    });
            let ps: Vec<_> = ports
                .iter()
                .take(8)
                .map(|(k, f)| {
                    json!([
                        k.port,
                        protocol(k.proto),
                        if k.dir == 1 { "in" } else { "out" },
                        f.count.rx,
                        f.count.tx,
                        rounded(f.rx_rate),
                        rounded(f.tx_rate)
                    ])
                })
                .collect();
            agg.push((
                rrx + rtx,
                rx.saturating_add(tx),
                json!([
                    address(fam as i32, &addr),
                    rx,
                    tx,
                    rounded(rrx),
                    rounded(rtx),
                    blocked,
                    ps
                ]),
            ));
        }
        agg.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        Value::Array(agg.into_iter().take(64).map(|x| x.2).collect())
    }
    fn counts(&mut self) {
        let now = nanos();
        let dt = if self.counts_ns == 0 {
            0.0
        } else {
            (now - self.counts_ns) as f64 / 1e9
        };
        self.counts_ns = now;
        let mut moved = false;
        for s in self.sessions.values_mut() {
            for sc in &mut s.scopes {
                let mut c: NpCount = self
                    .engine
                    .as_ref()
                    .and_then(|e| e.maps.get("np_counts", &sc.id))
                    .unwrap_or_default();
                c.denied += sc.cuts;
                let rr = rate(c.rx, sc.count.rx, dt);
                let rt = rate(c.tx, sc.count.tx, dt);
                if c != sc.count
                    || (rr == 0.0) != (sc.rx_rate == 0.0)
                    || (rt == 0.0) != (sc.tx_rate == 0.0)
                {
                    s.dirty = true;
                    moved = true
                }
                sc.count = c;
                sc.rx_rate = rr;
                sc.tx_rate = rt;
            }
        }
        let mut sys_moved = false;
        for s in &mut self.system {
            if s.id == 0 {
                continue;
            }
            let c: NpCount = self
                .engine
                .as_ref()
                .and_then(|e| e.maps.get("np_sysc", &s.id))
                .unwrap_or_default();
            let rr = rate(c.rx, s.count.rx, dt);
            let rt = rate(c.tx, s.count.tx, dt);
            if c != s.count
                || (rr == 0.0) != (s.rx_rate == 0.0)
                || (rt == 0.0) != (s.tx_rate == 0.0)
            {
                sys_moved = true
            }
            s.count = c;
            s.rx_rate = rr;
            s.tx_rate = rt;
        }
        if sys_moved {
            for s in self.sessions.values_mut() {
                s.dirty = true
            }
        }
        if let Some(e) = &self.engine {
            if e.accounting && moved {
                if let Ok(v) = e.maps.flows() {
                    for (k, c) in v {
                        let f = self.flows.entry(k).or_default();
                        if f.tick == self.tick {
                            continue;
                        }
                        let dr = if c.rx >= f.count.rx {
                            c.rx - f.count.rx
                        } else {
                            c.rx
                        };
                        let dtc = if c.tx >= f.count.tx {
                            c.tx - f.count.tx
                        } else {
                            c.tx
                        };
                        f.rx_rate = if dt > 0.0 { dr as f64 / dt } else { 0.0 };
                        f.tx_rate = if dt > 0.0 { dtc as f64 / dt } else { 0.0 };
                        if dr != 0 || dtc != 0 || c.blocked != f.count.blocked || f.active == 0 {
                            f.active = self.tick
                        }
                        f.count = c;
                        f.tick = self.tick;
                    }
                    self.flows.retain(|_, f| f.tick == self.tick);
                }
            } else {
                for f in self.flows.values_mut() {
                    f.rx_rate = 0.0;
                    f.tx_rate = 0.0;
                }
            }
        }
        let uids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(uid, s)| s.dirty && self.clients.iter().any(|c| c.uid == **uid))
            .map(|(&u, _)| u)
            .collect();
        for uid in uids {
            let mut out = serde_json::Map::new();
            for sc in &self.sessions[&uid].scopes {
                out.insert(
                    sc.app.clone(),
                    json!([
                        sc.count.allowed,
                        sc.count.denied,
                        sc.count.rx,
                        sc.count.tx,
                        rounded(sc.rx_rate),
                        rounded(sc.tx_rate),
                        self.flow_json(sc.id),
                        sc.count.blocked
                    ]),
                );
            }
            for (i, (name, _, _)) in SYSTEMS.iter().enumerate() {
                let s = &self.system[i];
                if s.id != 0 {
                    out.insert(
                        name.to_string(),
                        json!([
                            0,
                            0,
                            s.count.rx,
                            s.count.tx,
                            rounded(s.rx_rate),
                            rounded(s.tx_rate),
                            [],
                            0
                        ]),
                    );
                }
            }
            self.send(uid, 12, &Value::Object(out));
            self.sessions.get_mut(&uid).unwrap().dirty = false;
        }
    }
    fn accept(&mut self, l: &UnixListener) {
        for _ in 0..64 {
            let Ok((stream, _)) = l.accept() else { break };
            if self.clients.len() >= 64 {
                continue;
            }
            if stream.set_nonblocking(true).is_err() {
                continue;
            }
            let mut cred = unsafe { std::mem::zeroed::<libc::ucred>() };
            let mut n = size_of_val(&cred) as u32;
            if unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut cred as *mut libc::ucred).cast(),
                    &mut n,
                )
            } != 0
                || n as usize != size_of_val(&cred)
                || self.clients.iter().filter(|c| c.uid == cred.uid).count() >= 4
            {
                continue;
            }
            let mut c = Client {
                stream,
                uid: cred.uid,
                input: vec![],
                output: vec![],
            };
            c.enqueue(10, &self.status());
            self.clients.push(c);
            if let Some(s) = self.sessions.get_mut(&cred.uid) {
                s.dirty = true
            }
        }
    }
    fn tick(&mut self) {
        self.tick += 1;
        self.attach();
        self.system();
        if let Some(dns) = &mut self.dns {
            dns.tick(mono());
            let src = dns.source();
            if src != self.dns_source {
                self.dns_source = src;
                let status = self.status();
                for c in &mut self.clients {
                    c.enqueue(10, &status)
                }
            }
        }
        if self.tick % 5 == 0 {
            self.scan()
        }
        if self.tick % 30 == 0 {
            unsafe { crate::dnsmap::dnsmap_expire(self.names.0, mono()) }
            let mut sessions = std::mem::take(&mut self.sessions);
            let mut sweeps = vec![];
            for s in sessions.values_mut() {
                if self.sync_blocks(s) {
                    sweeps.push(s.uid)
                }
                for sc in &mut s.scopes {
                    if sc.domains {
                        self.sync_scope(&s.store, sc, if s.lists.is_some() { s.blkset } else { 0 })
                    }
                }
            }
            self.sessions = sessions;
            for uid in sweeps {
                self.sweep(Some(uid))
            }
        }
        self.counts();
    }
    fn serve(&mut self, l: &UnixListener) -> io::Result<()> {
        self.attach();
        self.scan();
        self.sweep(None);
        let mut deadline = Instant::now();
        while !QUIT.load(Ordering::Relaxed) {
            let mut poll = vec![
                libc::pollfd {
                    fd: l.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.notify.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            if let Some(e) = &self.engine {
                poll.push(libc::pollfd {
                    fd: e.maps.fd("np_events"),
                    events: libc::POLLIN,
                    revents: 0,
                })
            }
            if let Some(d) = &self.dns {
                if d.fd() >= 0 {
                    poll.push(libc::pollfd {
                        fd: d.fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    })
                }
            }
            let client_start = poll.len();
            for c in &self.clients {
                poll.push(libc::pollfd {
                    fd: c.stream.as_raw_fd(),
                    events: libc::POLLIN
                        | if c.output.is_empty() {
                            0
                        } else {
                            libc::POLLOUT
                        },
                    revents: 0,
                })
            }
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(1000) as i32;
            let n = unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as _, timeout) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if poll[1].revents != 0 {
                let mut b = [0u8; 4096];
                while unsafe { libc::read(self.notify.as_raw_fd(), b.as_mut_ptr().cast(), b.len()) }
                    > 0
                {}
                self.attach();
                self.scan()
            }
            if let Some(e) = &mut self.engine {
                let fd = e.maps.fd("np_events");
                if poll.iter().any(|p| p.fd == fd && p.revents != 0) {
                    let events = e.ring.drain();
                    for ev in events {
                        self.event(ev)
                    }
                }
            }
            if let Some(d) = &mut self.dns {
                let fd = d.fd();
                if fd >= 0 && poll.iter().any(|p| p.fd == fd && p.revents != 0) {
                    let v = d.ready();
                    self.dns(v)
                }
            }
            let mut reloads = HashSet::new();
            let mut i = 0;
            while i < self.clients.len() {
                let fd = self.clients[i].stream.as_raw_fd();
                let re = poll[client_start..]
                    .iter()
                    .find(|p| p.fd == fd)
                    .map_or(0, |p| p.revents);
                let c = &mut self.clients[i];
                let mut drop = re & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0;
                if !drop && re & libc::POLLIN != 0 {
                    match c.read() {
                        Ok(true) => {
                            reloads.insert(c.uid);
                        }
                        Ok(false) => {}
                        Err(_) => drop = true,
                    }
                }
                if !drop && c.flush().is_err() {
                    drop = true
                }
                if drop {
                    self.clients.swap_remove(i);
                } else {
                    i += 1
                }
            }
            for uid in reloads {
                self.reload(uid)
            }
            if poll[0].revents != 0 {
                self.accept(l)
            }
            if Instant::now() >= deadline {
                self.tick();
                deadline = Instant::now() + Duration::from_secs(1)
            }
            for c in &mut self.clients {
                let _ = c.flush();
            }
        }
        Ok(())
    }
}
fn scope_app(name: &str) -> Option<String> {
    if name == "server" {
        return Some("lordos".into());
    }
    if name.starts_with("lordos-net-") {
        let scope = CString::new(name).ok()?;
        let mut out = [0i8; 128];
        if unsafe {
            crate::fwrules::fw_component_scope(scope.as_ptr(), out.as_mut_ptr(), out.len())
        } == 0
        {
            return Some(
                unsafe { std::ffi::CStr::from_ptr(out.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        return None;
    }
    if !name.starts_with("app-lordos-") || !name.ends_with(".scope") {
        return None;
    }
    let app = crate::fwrules::app_canon(name.as_bytes())?;
    String::from_utf8(app).ok()
}
fn remote(s: &Socket) -> (i32, [u8; 16]) {
    if s.fam == 2 {
        return (4, s.dst);
    }
    if s.dst[..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255] {
        let mut a = [0; 16];
        a[..4].copy_from_slice(&s.dst[12..]);
        (4, a)
    } else {
        (6, s.dst)
    }
}
struct Listener {
    socket: UnixListener,
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(SOCK);
    }
}
fn listen() -> io::Result<Listener> {
    match fs::create_dir("/run/lordos") {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata("/run/lordos")?;
    if !m.is_dir() || m.uid() != 0 || m.permissions().mode() & 0o022 != 0 {
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }
    let _ = fs::remove_file(SOCK);
    let l = Listener {
        socket: UnixListener::bind(SOCK)?,
    };
    fs::set_permissions(SOCK, fs::Permissions::from_mode(0o666))?;
    l.socket.set_nonblocking(true)?;
    Ok(l)
}
pub fn run() -> i32 {
    if std::env::args_os().len() != 1 {
        eprintln!("axia-netperm takes no arguments (see docs/FIREWALL.md)");
        return 2;
    }
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("axia-netperm: must run as root (axia-netperm.service)");
        return 1;
    }
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = terminate as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    }
    match listen().and_then(|l| {
        let mut s = Service::new()?;
        s.serve(&l.socket)
    }) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("axia-netperm: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unix_protocol_handles_fragmentation_and_rejects_every_other_request() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        stream.set_nonblocking(true).unwrap();
        let mut c = Client {
            stream,
            uid: 1000,
            input: vec![],
            output: vec![],
        };
        peer.write_all(&[1, 0]).unwrap();
        assert!(!c.read().unwrap());
        peer.write_all(&[0, 0, 1, 1, 0, 0, 0, 1]).unwrap();
        assert!(c.read().unwrap());
        assert!(c.input.is_empty());
        c.enqueue(10, &json!({"bpf":true}));
        c.flush().unwrap();
        let mut header = [0; 5];
        peer.read_exact(&mut header).unwrap();
        assert_eq!(header[4], 10);
        let n = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let mut body = vec![0; n - 1];
        peer.read_exact(&mut body).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"bpf":true})
        );
        peer.write_all(&[2, 0, 0, 0, 1]).unwrap();
        assert!(c.read().is_err());
    }
    #[test]
    fn scopes_and_mapped_addresses_keep_the_existing_ids() {
        assert_eq!(scope_app("server").as_deref(), Some("lordos"));
        assert_eq!(
            scope_app("app-lordos-editor.scope").as_deref(),
            Some("desktop:editor")
        );
        assert_eq!(
            scope_app("app-lordos-lpk-editor.scope").as_deref(),
            Some("lpk:editor")
        );
        assert_eq!(
            scope_app("lordos-net-mail.scope").as_deref(),
            Some("lordos:mail")
        );
        assert!(scope_app("unrelated.scope").is_none());
        assert!(scope_app("lordos-net-evil.scope").is_none());
        let mut s = Socket {
            fam: 10,
            ..Default::default()
        };
        s.dst = "::ffff:203.0.113.5".parse::<Ipv6Addr>().unwrap().octets();
        let (f, a) = remote(&s);
        assert_eq!(f, 4);
        assert_eq!(&a[..4], &[203, 0, 113, 5]);
    }
    #[test]
    fn counts_preserve_the_nested_desktop_protocol_and_port_limits() {
        let notify = unsafe {
            OwnedFd::from_raw_fd(libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK))
        };
        // No Engine::new: this fixture does not issue a single bpf syscall.
        let mut s = Service {
            engine: None,
            error: "fw.bpfNotBuilt".into(),
            names: Names::default(),
            dns: None,
            sessions: BTreeMap::new(),
            clients: vec![],
            indices: HashMap::new(),
            next_idx: 0,
            next_set: 0,
            installed: HashMap::new(),
            links: vec![],
            service_cg: 0,
            system: Default::default(),
            flows: HashMap::new(),
            tick: 10,
            counts_ns: 0,
            diag: None,
            diag_warned: false,
            notify,
            root_watch: -1,
            dns_source: "none",
            sound: HashSet::new(),
        };
        for port in 1..=12 {
            let k = NpFlowKey {
                cgid: 42,
                fam: 4,
                addr: [8; 16],
                proto: 1,
                port,
                ..Default::default()
            };
            s.flows.insert(
                k,
                Flow {
                    count: NpFlow {
                        rx: port as u64,
                        tx: 2,
                        ..Default::default()
                    },
                    active: 10,
                    rx_rate: port as f64,
                    ..Default::default()
                },
            );
        }
        let v = s.flow_json(42);
        let groups = v.as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0][1], 78);
        assert_eq!(groups[0][2], 24);
        assert_eq!(groups[0][6].as_array().unwrap().len(), 8);
        assert_eq!(groups[0][6][0][0], 12);
        assert_eq!(groups[0][6][0][1], "tcp");
        assert_eq!(groups[0][6][0][2], "out");
    }
}
