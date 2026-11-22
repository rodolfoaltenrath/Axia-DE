//! axia-fand: the root service that drives the fans by temperature (docs/FANS.md). Closed and
//! minimal, like axia-netperm:
//!
//!   - executes nothing and opens no network sockets (the unit forbids every family but AF_UNIX);
//!   - reads the CPU temperature every `TICK_MS` and writes each fan's duty (`hw`);
//!   - its configuration is /etc/lordos/fan.json, written only here, after validating;
//!   - talks to sessions over /run/lordos/fand.sock (0666) with one line per connection:
//!       STATUS                     anyone: a JSON line with temperature, fans, profile, step and
//!                                  whether this peer may change them ("canChange")
//!       PROFILE <name> / CURVE <t:p,...> / FORCE off|on|<minutes>
//!                                  administrators only (SO_PEERCRED: root, or a member of sudo,
//!                                  admin or wheel): "OK" or "ERR <code>"
//!     Clients never name paths or devices: the daemon drives every fan it found.
//!   - on SIGTERM/SIGINT, and in `axia-fand --restore` (the unit's ExecStopPost, which also runs
//!     after a crash), the fans go back to the kernel or the firmware.
//!
//! Other modes, for diagnosis: `axia-fand --detect` prints what it found and exits. For tests
//! only (never in the unit): --root DIR (sysfs root), --config FILE, --run DIR.
use crate::curve::{self, Config, Curve, Force, Profile, Request};
use crate::hw::{self, FanHw, Kind, TEMP_NONE};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const TICK_MS: i64 = 2000;
const CLIENTS: usize = 8;
const CLIENT_MS: i64 = 1000;
const STATUS_MAX: usize = 4096;

static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, Ordering::SeqCst);
}

fn wall_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The daemon's state, apart from the socket: everything the requests and the ticks change.
pub struct Daemon {
    pub config_path: String,
    pub state_path: String,
    pub hw: FanHw,
    pub cfg: Config,
    pub curve: Curve,
    pub step: i32,
    pub temp: i32,
    /// What the fans were asked for, -1 none.
    pub out_pct: i32,
    /// True while we drive the fans (profile != auto or forced).
    pub controlling: bool,
}

impl Daemon {
    pub fn new(root: &str, config_path: &str, state_path: &str) -> Daemon {
        let hw = FanHw::scan(root);
        Daemon {
            config_path: config_path.to_string(),
            state_path: state_path.to_string(),
            cfg: Config::default_for(false),
            curve: Curve::default(),
            hw,
            step: -1,
            temp: 0,
            out_pct: 0,
            controlling: false,
        }
    }

    // ---- configuration ----

    pub fn load_config(&mut self) {
        self.cfg = Config::default_for(self.hw.t.first().is_some_and(|t| t.kind == Kind::Board));
        let Ok(mut f) = OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(&self.config_path) else { return };
        // only a root-owned regular file that nobody else can write
        let ok = f.metadata().is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.len() < 4096);
        let mut buf = Vec::new();
        let read = ok && f.read_to_end(&mut buf).is_ok();
        let r = if read { self.cfg.apply_json(&buf) } else { Err("not a root-owned file".to_string()) };
        if let Err(e) = r {
            eprintln!("axia-fand: {} ignored ({e}), using defaults", self.config_path);
        }
    }

    fn save_config(&self) -> bool {
        let text = self.cfg.to_json();
        let tmp = format!("{}.tmp", self.config_path);
        let ok = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&tmp)
            .ok()
            .is_some_and(|mut f| f.write_all(text.as_bytes()).is_ok() && f.sync_all().is_ok());
        if !ok || fs::rename(&tmp, &self.config_path).is_err() {
            let _ = fs::remove_file(&tmp);
            return false;
        }
        true
    }

    // ---- control ----

    fn forced(&mut self, now: i64) -> bool {
        match self.cfg.force {
            Force::On => true,
            Force::Timed => {
                if now < self.cfg.force_until {
                    return true;
                }
                self.cfg.force = Force::Off;
                self.cfg.force_until = 0;
                self.save_config();
                false
            }
            Force::Off => false,
        }
    }

    pub fn apply_profile(&mut self) {
        self.curve = self.cfg.curve();
        self.step = -1;
    }

    /// Reads the temperature and writes the fans' duty (or gives them back).
    pub fn tick(&mut self, now: i64) {
        self.temp = self.hw.temp();
        let forced = self.forced(now);
        let want_control = !self.hw.t.is_empty() && (self.cfg.profile != Profile::Auto || forced);
        if !want_control {
            if self.controlling {
                self.hw.release();
                let _ = fs::remove_file(&self.state_path);
                self.controlling = false;
            }
            self.out_pct = -1;
            self.step = -1;
            return;
        }
        if !self.controlling {
            let state = self.state_path.clone();
            self.hw.take(Some(&state));
            self.controlling = true;
        }
        let pct = if forced || self.temp <= TEMP_NONE {
            100 // forced, or no temperature: be safe
        } else {
            self.step = self.curve.step(self.cfg.hyst, self.step, self.temp);
            self.curve.pct(self.step)
        };
        self.out_pct = curve::output_pct(pct);
        for i in 0..self.hw.t.len() {
            self.hw.set(i, self.out_pct);
        }
    }

    // ---- status ----

    /// The JSON line of STATUS, or nothing if it would not fit in `STATUS_MAX`.
    pub fn status_json(&self, can_change: bool, now: i64) -> String {
        let h = &self.hw;
        let mut o = format!(
            "{{\"ok\":true,\"canChange\":{},\"board\":\"{}\",\"tempSource\":\"{}\",",
            can_change,
            if h.board { "pi5" } else { "" },
            h.temp_src
        );
        if self.temp > -1_000_000 {
            o.push_str(&format!("\"temp\":{}.{},", self.temp / 1000, (self.temp.unsigned_abs() % 1000) / 100));
        } else {
            o.push_str("\"temp\":null,");
        }
        o.push_str(&format!(
            "\"profile\":\"{}\",\"hyst\":{},\"controlling\":{},\"step\":{},\"target\":{},",
            self.cfg.profile.name(),
            self.cfg.hyst,
            self.controlling,
            self.step,
            self.out_pct
        ));
        let cur = self.cfg.curve();
        let pts = |c: &Curve| c.points.iter().map(|(t, p)| format!("[{t},{p}]")).collect::<Vec<_>>().join(",");
        o.push_str(&format!("\"floor\":{},\"curve\":[{}],\"custom\":[{}],", cur.floor_pct, pts(&cur), pts(&self.cfg.custom)));
        let left = if self.cfg.force == Force::Timed { self.cfg.force_until - now } else { 0 };
        o.push_str(&format!("\"force\":\"{}\",\"forceLeft\":{},\"fans\":[", self.cfg.force.name(), left.max(0)));
        let fans: Vec<String> = (0..h.t.len())
            .map(|i| {
                format!(
                    "{{\"id\":{i},\"kind\":\"{}\",\"chip\":\"{}\",\"ch\":{},\"pct\":{},\"rpm\":{}}}",
                    h.t[i].kind.name(),
                    h.t[i].chip,
                    h.t[i].ch,
                    h.pct(i),
                    h.rpm(i)
                )
            })
            .collect();
        o.push_str(&fans.join(","));
        o.push_str("],\"firmwareFans\":[");
        let ro: Vec<String> = (0..h.ro.len()).map(|i| format!("{{\"chip\":\"{}\",\"rpm\":{}}}", h.ro[i].chip, h.ro_rpm(i))).collect();
        o.push_str(&ro.join(","));
        o.push_str("]}\n");
        if o.len() < STATUS_MAX { o } else { String::new() }
    }

    // ---- requests ----

    /// The answer to the bytes a client sent (up to its newline). `admin` is asked only when the
    /// request needs it.
    pub fn handle(&mut self, buf: &[u8], admin: impl FnOnce() -> bool, now: i64) -> Vec<u8> {
        const BAD: &[u8] = b"ERR fan.badRequest\n";
        let Some(nl) = buf.iter().position(|&c| c == b'\n') else { return BAD.to_vec() };
        let line = &buf[..nl];
        let Some(r) = curve::parse_request(line) else { return BAD.to_vec() };
        if r == Request::Status {
            return self.status_json(admin(), now).into_bytes();
        }
        if !admin() {
            return b"ERR fan.notAdmin\n".to_vec();
        }
        let old = self.cfg.clone();
        match r {
            Request::Profile(p) => self.cfg.profile = p,
            Request::Curve(c) => {
                self.cfg.custom = c;
                self.cfg.profile = Profile::Custom;
            }
            Request::Force(f, minutes) => {
                self.cfg.force = f;
                self.cfg.force_until = if f == Force::Timed { now + minutes as i64 * 60 } else { 0 };
            }
            Request::Status => {}
        }
        if !self.save_config() {
            self.cfg = old;
            return b"ERR fan.saveFailed\n".to_vec();
        }
        eprintln!("axia-fand: {}", String::from_utf8_lossy(line));
        self.apply_profile();
        self.tick(now);
        b"OK\n".to_vec()
    }

    /// `axia-fand --detect`: what it found.
    pub fn detect(&self) -> String {
        let h = &self.hw;
        let mut o = format!(
            "board: {}\ntemperature: {} ({})",
            if h.board { "Raspberry Pi 5" } else { "other" },
            if h.temp_src.is_empty() { "none" } else { &h.temp_src },
            h.temp_path
        );
        let t = h.temp();
        if t > -1_000_000 {
            o.push_str(&format!(" {}.{} C", t / 1000, (t.unsigned_abs() % 1000) / 100));
        }
        o.push('\n');
        for i in 0..h.t.len() {
            o.push_str(&format!("fan {i}: {} {} pwm{} at {} %, {} rpm ({})\n", h.t[i].kind.name(), h.t[i].chip, h.t[i].ch, h.pct(i), h.rpm(i), h.t[i].dir));
        }
        for i in 0..h.trip.len() {
            o.push_str(&format!("kernel trip {}: {} mC ({})\n", h.trip[i], h.trip_orig[i], h.zone));
        }
        for i in 0..h.ro.len() {
            o.push_str(&format!("firmware fan: {} {} rpm\n", h.ro[i].chip, h.ro_rpm(i)));
        }
        if h.t.is_empty() {
            o.push_str("no controllable fans\n");
        }
        o
    }
}

/// Is the peer on this socket root, or a member of sudo, admin or wheel?
pub fn peer_is_admin(fd: i32) -> bool {
    let mut uc: libc::ucred = unsafe { std::mem::zeroed() };
    let mut l = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: a valid ucred and its length.
    if unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut uc as *mut libc::ucred).cast(), &mut l) } != 0 {
        return false;
    }
    if uc.uid == 0 {
        return true;
    }
    let mut pwbuf = vec![0 as libc::c_char; 2048];
    // SAFETY: the passwd and its buffer outlive their use below.
    let (name, gid) = unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut pp: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uc.uid, &mut pw, pwbuf.as_mut_ptr(), pwbuf.len(), &mut pp) != 0 || pp.is_null() {
            return false;
        }
        (std::ffi::CStr::from_ptr(pw.pw_name).to_owned(), pw.pw_gid)
    };
    let mut groups = [0 as libc::gid_t; 64];
    let mut n: libc::c_int = 64;
    // SAFETY: `groups` has room for `n` entries.
    if unsafe { libc::getgrouplist(name.as_ptr(), gid, groups.as_mut_ptr(), &mut n) } < 0 {
        return false;
    }
    for &g in &groups[..n.max(0) as usize] {
        let mut gbuf = vec![0 as libc::c_char; 4096];
        // SAFETY: as above.
        let gname = unsafe {
            let mut gr: libc::group = std::mem::zeroed();
            let mut gp: *mut libc::group = std::ptr::null_mut();
            if libc::getgrgid_r(g, &mut gr, gbuf.as_mut_ptr(), gbuf.len(), &mut gp) != 0 || gp.is_null() {
                continue;
            }
            std::ffi::CStr::from_ptr(gr.gr_name).to_owned()
        };
        if matches!(gname.to_bytes(), b"sudo" | b"admin" | b"wheel") {
            return true;
        }
    }
    false
}

struct Client {
    fd: OwnedFd,
    deadline: i64,
    buf: Vec<u8>,
}

fn reply(fd: i32, mut s: &[u8]) {
    while !s.is_empty() {
        // SAFETY: `s` is a valid buffer.
        let w = unsafe { libc::send(fd, s.as_ptr().cast(), s.len(), libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT) };
        if w <= 0 {
            return;
        }
        s = &s[w as usize..];
    }
}

fn listen_sock(run: &str, sock: &str) -> std::io::Result<UnixListener> {
    let _ = fs::DirBuilder::new().mode(0o755).create(run);
    let _ = fs::remove_file(sock);
    let l = UnixListener::bind(sock)?;
    fs::set_permissions(sock, fs::Permissions::from_mode(0o666))?;
    l.set_nonblocking(true)?;
    Ok(l)
}

/// What the command line asked for.
#[derive(Default)]
pub struct Args {
    pub detect: bool,
    pub restore: bool,
    pub root: String,
    pub config: String,
    pub run: String,
}

pub fn parse_args(argv: &[String]) -> Option<Args> {
    let mut a = Args { root: String::new(), config: "/etc/lordos/fan.json".into(), run: "/run/lordos".into(), ..Args::default() };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--detect" => a.detect = true,
            "--restore" => a.restore = true,
            "--root" if i + 1 < argv.len() => {
                i += 1;
                a.root = argv[i].clone();
            }
            "--config" if i + 1 < argv.len() => {
                i += 1;
                a.config = argv[i].clone();
            }
            "--run" if i + 1 < argv.len() => {
                i += 1;
                a.run = argv[i].clone();
            }
            _ => return None,
        }
        i += 1;
    }
    Some(a)
}

/// The program: the exit code.
pub fn main(argv: &[String]) -> i32 {
    let Some(a) = parse_args(argv) else {
        eprintln!("usage: axia-fand [--detect | --restore]");
        return 2;
    };
    if a.run.len() > 100 || a.config.len() > 200 {
        return 2;
    }
    let sock = format!("{}/fand.sock", a.run);
    let state = format!("{}/fand.restore", a.run);
    // SAFETY: umask has no failure mode.
    unsafe { libc::umask(0o022) };

    if a.restore {
        if fs::symlink_metadata(&state).is_err() {
            return 0; // nothing taken
        }
        let r = hw::restore_file(&a.root, &state);
        eprintln!("axia-fand: fans given back to the kernel/firmware{}", if r.is_err() { " (partially)" } else { "" });
        return 0;
    }
    // a previous run that died without cleaning up: undo it before reading the "original" values
    if !a.detect && fs::symlink_metadata(&state).is_ok() {
        let _ = hw::restore_file(&a.root, &state);
    }
    let mut d = Daemon::new(&a.root, &a.config, &state);
    if a.detect {
        print!("{}", d.detect());
        return 0;
    }
    d.load_config();
    d.apply_profile();
    eprintln!(
        "axia-fand: {} fan(s), {} firmware fan(s), temperature from {}, profile {}",
        d.hw.t.len(),
        d.hw.ro.len(),
        if d.hw.temp_src.is_empty() { "nothing" } else { &d.hw.temp_src },
        d.cfg.profile.name()
    );
    // SAFETY: installing handlers that only set an atomic flag.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let listener = match listen_sock(&a.run, &sock) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("axia-fand: {sock}: {e} (fans still controlled, no settings)");
            None
        }
    };
    serve(&mut d, listener.as_ref());
    if d.controlling {
        d.hw.release();
        let _ = fs::remove_file(&d.state_path);
    }
    if listener.is_some() {
        drop(listener);
        let _ = fs::remove_file(&sock);
    }
    eprintln!("axia-fand: stopped, fans given back to the kernel/firmware");
    0
}

fn serve(d: &mut Daemon, listener: Option<&UnixListener>) {
    let start = Instant::now();
    let now_ms = || start.elapsed().as_millis() as i64;
    let mut clients: Vec<Option<Client>> = (0..CLIENTS).map(|_| None).collect();
    let mut next = now_ms();
    while !QUIT.load(Ordering::SeqCst) {
        let mut now = now_ms();
        if now >= next {
            d.tick(wall_s());
            next = now + TICK_MS;
        }
        let mut fds: Vec<libc::pollfd> = Vec::new();
        if let Some(l) = listener {
            fds.push(libc::pollfd { fd: l.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        let mut wake = next;
        for c in clients.iter().flatten() {
            fds.push(libc::pollfd { fd: c.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            wake = wake.min(c.deadline);
        }
        let to = (wake - now_ms()).max(0) as i32;
        // SAFETY: a valid array of pollfds.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, to) };
        if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break;
        }
        now = now_ms();
        for p in fds.iter().filter(|_| r > 0).filter(|p| p.revents != 0) {
            if listener.is_some_and(|l| l.as_raw_fd() == p.fd) {
                // SAFETY: accept4 on our listening socket.
                let fd = unsafe { libc::accept4(p.fd, std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) };
                if fd < 0 {
                    continue;
                }
                // SAFETY: a new descriptor that nobody else owns.
                let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                if let Some(slot) = clients.iter_mut().find(|c| c.is_none()) {
                    *slot = Some(Client { fd, deadline: now + CLIENT_MS, buf: Vec::new() });
                }
                continue;
            }
            let Some(slot) = clients.iter_mut().find(|c| c.as_ref().is_some_and(|c| c.fd.as_raw_fd() == p.fd)) else { continue };
            let c = slot.as_mut().unwrap();
            let mut chunk = [0u8; curve::REQ_MAX + 2];
            let room = chunk.len() - 1 - c.buf.len();
            // SAFETY: `chunk` has room for `room` bytes.
            let n = unsafe { libc::recv(p.fd, chunk.as_mut_ptr().cast(), room, 0) };
            if n <= 0 {
                *slot = None;
                continue;
            }
            c.buf.extend_from_slice(&chunk[..n as usize]);
            if c.buf.contains(&b'\n') || c.buf.len() >= chunk.len() - 1 {
                let fd = p.fd;
                let out = d.handle(&c.buf, || peer_is_admin(fd), wall_s());
                reply(fd, &out);
                *slot = None;
            }
        }
        for slot in clients.iter_mut() {
            if slot.as_ref().is_some_and(|c| now >= c.deadline) {
                *slot = None;
            }
        }
    }
}
