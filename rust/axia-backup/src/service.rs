//! The backup scheduler and worker thread (server/src/backup/backup.h).
//!
//!   ~/.config/axia/backup.json   the configuration: destination, schedule, exclusions, battery.
//!                                  Only `configure` writes it (lord.fs cannot: it decides where
//!                                  the data goes; it is in the protected folders).
//!
//! The thread sleeps in a timed wait until the next backup (no polling). While copying:
//!   - nice 19 and "idle" I/O priority (setpriority/ioprio_set on its own tid), and
//!     posix_fadvise DONTNEED on what was copied: it does not evict the desktop's page cache;
//!   - on battery (with "pause on battery"), it stops and checks again every 60 s;
//!   - a user pause waits on the same condition variable (no polling).
//! The time of the last good backup is the newest snapshot on the destination: there is no other
//! state file. The first scheduled backup waits 5 minutes after the session starts.
//!
//! The thread notifies the main loop through an eventfd (`event_fd`/`dispatch`), which hands the
//! status JSON to JS.
use crate::crypt::{self, Key};
use crate::engine::{self, Progress, RestoreStats, Run, DIRNAME, MAX_EXCL, MAX_SNAPS};
use crate::errcode;
use crate::monitor::{Guard, Monitor};
use axia_fs::policy;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::Relaxed};
use std::sync::OnceLock;

const START_DELAY: i64 = 300; // s after the session starts
const RETRY_FAIL: i64 = 900; // s before retrying after an error
const RETRY_NODEST: i64 = 300; // s before checking again whether the disk is back
const BATTERY_RECHECK: i64 = 60;
const DEFAULT_EXCL: [&str; 5] = [".cache", ".local/share/Trash", ".local/share/axia/apps", ".local/share/axia/trash", ".thumbnails"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Running,
    Paused,
    NoDest,
    Restoring,
    Deleting,
    Locked,
    Unlocking,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Running => "running",
            State::Paused => "paused",
            State::NoDest => "nodest",
            State::Restoring => "restoring",
            State::Deleting => "deleting",
            State::Locked => "locked",
            State::Unlocking => "unlocking",
        }
    }
}

#[derive(Clone)]
struct Config {
    /// Chosen folder (real path).
    dest: OsString,
    /// Seconds; 0 = manual.
    interval: i64,
    battery_pause: bool,
    /// A new destination is encrypted.
    encrypt: bool,
    excl: Vec<String>,
}

impl Config {
    const fn new() -> Self {
        Self { dest: OsString::new(), interval: 3600, battery_pause: true, encrypt: false, excl: Vec::new() }
    }
}

/// The result of the last backup.
struct Last {
    files: u64,
    copied: u64,
    linked: u64,
    bytes: u64,
    errors: u64,
    secs: f64,
    ok: i32,
    when: i64,
    id: String,
}

struct RJob {
    id: String,
    rel: String,
    suffix: Vec<u8>,
    to: PathBuf,
    replace: bool,
}

struct UJob {
    create: bool,
    pass: Vec<u8>,
}

struct RRes {
    done: bool,
    ok: bool,
    id: String,
    files: u64,
    kept: u64,
    replaced: u64,
    skipped: u64,
    errors: u64,
    error: Vec<u8>,
}

struct Inner {
    cfg: Config,
    state: State,
    want_run: bool,
    user_pause: bool,
    on_batt_pause: bool,
    started_at: i64,
    next_due: i64,
    last_ok: i64,
    last_try: i64,
    last_error: Vec<u8>,
    cur_file: Vec<u8>,
    last_emit: f64,
    last_batt_check: f64,
    last: Last,
    rjob: Option<RJob>,
    djob: Option<String>,
    /// The key lives only in memory (mlock'd), for as long as the session lasts.
    key: Key,
    have_key: bool,
    dest_enc: bool,
    ujob: Option<UJob>,
    ures_done: bool,
    ures_ok: bool,
    ures_error: &'static str,
    rres: RRes,
}

impl Inner {
    const fn new() -> Self {
        Self {
            cfg: Config::new(),
            state: State::Idle,
            want_run: false,
            user_pause: false,
            on_batt_pause: false,
            started_at: 0,
            next_due: 0,
            last_ok: 0,
            last_try: 0,
            last_error: Vec::new(),
            cur_file: Vec::new(),
            last_emit: 0.0,
            last_batt_check: 0.0,
            last: Last { files: 0, copied: 0, linked: 0, bytes: 0, errors: 0, secs: 0.0, ok: 0, when: 0, id: String::new() },
            rjob: None,
            djob: None,
            key: [0; crypt::KEY],
            have_key: false,
            dest_enc: false,
            ujob: None,
            ures_done: false,
            ures_ok: false,
            ures_error: "",
            rres: RRes { done: false, ok: false, id: String::new(), files: 0, kept: 0, replaced: 0, skipped: 0, errors: 0, error: Vec::new() },
        }
    }
}

struct Env {
    home: PathBuf,
    user: String,
    host: String,
    cfg_path: PathBuf,
}

static SVC: Monitor<Inner> = Monitor::new(Inner::new());
static ENV: OnceLock<Env> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static EFD: AtomicI32 = AtomicI32::new(-1);
static CANCEL: AtomicBool = AtomicBool::new(false);
/// Progress of the running (or last) backup.
static RUN: Progress = Progress::new();

type Guarded<'a> = Guard<'a, Inner>;

fn started() -> bool {
    STARTED.load(Relaxed)
}

fn env() -> &'static Env {
    ENV.get().expect("the backup service is started")
}

fn now_s() -> f64 {
    let mut t: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as f64 + t.tv_nsec as f64 / 1e9
}

fn wall() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

/// Something changed: wakes the main loop (a full counter means a notification is already pending).
fn emit() {
    let fd = EFD.load(Relaxed);
    if fd >= 0 {
        let one: u64 = 1;
        unsafe { libc::write(fd, (&one as *const u64).cast(), 8) };
    }
}

fn os(bytes: &[u8]) -> &OsStr {
    OsStr::from_bytes(bytes)
}

// ---- configuration ----

fn machine_dir(c: &Config) -> Option<PathBuf> {
    if c.dest.is_empty() {
        return None;
    }
    let e = env();
    Some(Path::new(&c.dest).join(DIRNAME).join(format!("{}-{}", e.host, e.user)))
}

fn access_w(p: &Path) -> bool {
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap_or_default();
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// The destination as a real path, or the error code line. "" clears it.
fn valid_dest(input: &[u8]) -> Result<OsString, Vec<u8>> {
    if input.is_empty() {
        return Ok(OsString::new());
    }
    let real = if input[0] == b'/' { std::fs::canonicalize(os(input)).ok().filter(|r| std::fs::metadata(r).is_ok_and(|m| m.is_dir())) } else { None };
    let Some(real) = real else { return Err(errcode::code("backup.destMissing")) };
    let policy = policy::current();
    if !policy.allowed(&real) || policy.protected(&real) {
        return Err(errcode::code("backup.destOutside"));
    }
    if !access_w(&real) {
        return Err(errcode::code("backup.destUnwritable"));
    }
    Ok(real.into_os_string())
}

fn parse_config(text: &[u8], strict: bool) -> Result<Config, Vec<u8>> {
    let bad = || errcode::code("backup.badConfig");
    let value: serde_json::Value = serde_json::from_slice(text).map_err(|_| bad())?;
    let obj = value.as_object().ok_or_else(bad)?;
    let mut c = Config::new();
    if let Some(dest) = obj.get("dest").and_then(|v| v.as_str()) {
        if strict {
            if dest.len() >= 4096 {
                return Err(bad());
            }
            c.dest = valid_dest(dest.as_bytes())?;
        } else {
            if !dest.starts_with('/') || dest.len() >= 4096 || !engine::rel_valid(&dest.as_bytes()[1..]) {
                return Err(errcode::code("backup.badDest"));
            }
            c.dest = OsString::from(dest);
        }
    }
    if let Some(iv) = obj.get("interval").and_then(|v| v.as_str()) {
        c.interval = match iv {
            "hourly" => 3600,
            "daily" => 86400,
            "weekly" => 7 * 86400,
            "manual" => 0,
            _ => return Err(errcode::code("backup.badSchedule")),
        };
    }
    c.battery_pause = obj.get("pauseOnBattery").and_then(|v| v.as_bool()).unwrap_or(true);
    if let Some(list) = obj.get("exclude").and_then(|v| v.as_array()) {
        for e in list {
            let Some(s) = e.as_str().filter(|s| !s.is_empty() && s.len() <= 255 && engine::rel_valid(s.as_bytes())) else {
                return Err(errcode::code("backup.badExclusion"));
            };
            if c.excl.len() >= MAX_EXCL - 8 {
                return Err(errcode::code("backup.tooManyExclusions"));
            }
            c.excl.push(s.to_string());
        }
    }
    c.encrypt = obj.get("encrypt").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(c)
}

/// A JSON string literal, escaped as server/src/sec/json.c does.
fn quote(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    for &c in bytes {
        match c {
            b'"' | b'\\' => out.extend_from_slice(&[b'\\', c]),
            c if c < 0x20 => out.extend_from_slice(format!("\\u{c:04x}").as_bytes()),
            c => out.push(c),
        }
    }
    out.push(b'"');
    out
}

fn config_text(c: &Config) -> Vec<u8> {
    let interval = match c.interval {
        0 => "manual",
        86400 => "daily",
        604800 => "weekly",
        _ => "hourly",
    };
    let mut out = Vec::new();
    out.extend_from_slice(b"{\n  \"dest\": ");
    out.extend_from_slice(&quote(c.dest.as_bytes()));
    let _ = write!(out, ",\n  \"interval\": \"{interval}\",\n  \"pauseOnBattery\": {},\n  \"encrypt\": {},\n  \"exclude\": [", c.battery_pause, c.encrypt);
    for (i, e) in c.excl.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b", ");
        }
        out.extend_from_slice(&quote(e.as_bytes()));
    }
    out.extend_from_slice(b"]\n}\n");
    out
}

fn load_config(g: &mut Guarded) {
    use std::io::Read;
    g.cfg = Config::new();
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&env().cfg_path) else { return };
    let mut buf = vec![0u8; 65535];
    let Some(n) = f.read(&mut buf).ok().filter(|n| *n > 0) else { return };
    // the destination need not be present (an unplugged disk): it is validated before each backup
    match parse_config(&buf[..n], false) {
        Ok(c) => g.cfg = c,
        Err(e) => eprintln!("backup: {}: {}", env().cfg_path.display(), String::from_utf8_lossy(&e)),
    }
}

fn save_config(c: &Config) -> bool {
    let path = &env().cfg_path;
    let mut tmp = path.clone().into_os_string();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    let text = config_text(c);
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp);
    let ok = file.and_then(|mut f| f.write_all(&text)).is_ok();
    if !ok || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

// ---- battery ----

fn read_small(p: &Path) -> Option<String> {
    let text = std::fs::read(p).ok()?;
    let text = String::from_utf8_lossy(&text).into_owned();
    Some(text.split('\n').next().unwrap_or("").to_string())
}

/// On battery = some battery discharging and no power source connected.
fn on_battery() -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return false };
    let (mut discharging, mut mains) = (false, false);
    for e in dir.flatten() {
        let name = e.file_name();
        if name.as_bytes().first() == Some(&b'.') {
            continue;
        }
        let base = e.path();
        match read_small(&base.join("type")).as_deref() {
            Some("Battery") => discharging |= read_small(&base.join("status")).as_deref() == Some("Discharging"),
            Some("Mains" | "USB" | "USB_C") => mains |= read_small(&base.join("online")).as_deref() == Some("1"),
            _ => {}
        }
    }
    discharging && !mains
}

// ---- the thread ----

/// Between files: progress, user pause and battery. Runs with the mutex released.
fn checkpoint() -> bool {
    let t = now_s();
    let mut g = SVC.lock();
    let mut batt = false;
    if g.cfg.battery_pause && t - g.last_batt_check > 2.0 {
        g.last_batt_check = t;
        batt = on_battery();
    }
    while !CANCEL.load(Relaxed) && (g.user_pause || batt) {
        g.on_batt_pause = batt;
        g.state = State::Paused;
        emit();
        if batt {
            g.wait_until(wall() + BATTERY_RECHECK);
            batt = g.cfg.battery_pause && on_battery();
        } else {
            g.wait();
        }
    }
    if g.state == State::Paused {
        g.state = State::Running;
        g.on_batt_pause = false;
        emit();
    }
    let cancelled = CANCEL.load(Relaxed);
    if t - g.last_emit > 0.5 {
        g.last_emit = t;
        emit();
    }
    cancelled
}

fn progress(rel: &[u8]) {
    let mut g = SVC.lock();
    g.cur_file = rel[..rel.len().min(511)].to_vec();
}

fn latest_id(machine: &Path) -> Option<String> {
    engine::list(machine, MAX_SNAPS).pop()
}

fn do_snapshot(g: &mut Guarded) {
    let c = g.cfg.clone();
    let policy = policy::current();
    let dest = Path::new(&c.dest).to_path_buf();
    let machine = machine_dir(&c);
    let dreal = std::fs::canonicalize(&dest).ok();
    let usable = machine.is_some()
        && std::fs::metadata(&dest).is_ok_and(|m| m.is_dir())
        && access_w(&dest)
        && dreal.as_ref().is_some_and(|d| policy.allowed(d) && !policy.protected(d));
    let (Some(machine), true) = (machine, usable) else {
        g.state = State::NoDest;
        emit();
        return;
    };
    let backups_dir = dest.join(DIRNAME);
    let mut excl: Vec<String> = DEFAULT_EXCL.iter().map(|s| s.to_string()).collect();
    for e in &c.excl {
        if excl.len() < MAX_EXCL {
            excl.push(e.clone());
        }
    }
    let key: Option<Key> = (g.dest_enc && g.have_key).then_some(g.key);
    RUN.reset();
    g.state = State::Running;
    CANCEL.store(false, Relaxed);
    g.last_try = wall();
    g.cur_file.clear();
    emit();

    let home = env().home.clone();
    let t0 = now_s();
    let (outcome, secs) = g.unlocked(|| {
        {
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new().mode(0o700).create(&backups_dir);
        }
        let mut checkpoint_fn = checkpoint;
        let mut progress_fn = progress;
        let mut run = Run {
            home: &home,
            machine: &machine,
            excl,
            skip_real: std::fs::canonicalize(&backups_dir).ok(),
            cancel: Some(&CANCEL),
            checkpoint: Some(&mut checkpoint_fn),
            progress: Some(&mut progress_fn),
            min_free: 0,
            key: key.as_ref(),
            stats: &RUN,
        };
        let outcome = engine::snapshot(&mut run, wall());
        (outcome, now_s() - t0)
    });
    if let Some(mut k) = key {
        crypt::wipe(&mut k);
    }

    g.state = State::Idle;
    g.cur_file.clear();
    match outcome.code {
        0 => {
            g.last.ok = 1;
            g.last_error.clear();
            g.last_ok = wall();
        }
        c if c < 0 => {
            g.last.ok = 0;
            g.last_error = if outcome.error.is_empty() { b"sys.unknown".to_vec() } else { outcome.error.clone() };
        }
        _ => {
            g.last.ok = -1; // interrupted: resumed next time
            g.last_error = b"backup.interrupted".to_vec();
        }
    }
    g.last.files = Progress::get(&RUN.files);
    g.last.copied = Progress::get(&RUN.copied);
    g.last.linked = Progress::get(&RUN.linked);
    g.last.bytes = Progress::get(&RUN.copied_bytes);
    g.last.errors = Progress::get(&RUN.errors);
    g.last.secs = secs;
    g.last.when = wall();
    g.last.id = if outcome.code == 0 { outcome.id.clone() } else { String::new() };
    eprintln!(
        "backup: {} in {:.1} s: {} files, {} copied ({} MB), {} linked, {} errors{}{}",
        if outcome.code == 0 { "done" } else if outcome.code > 0 { "interrupted" } else { "failed" },
        secs,
        g.last.files,
        g.last.copied,
        g.last.bytes >> 20,
        g.last.linked,
        g.last.errors,
        if outcome.code < 0 { ": " } else { "" },
        if outcome.code < 0 { String::from_utf8_lossy(&g.last_error).into_owned() } else { String::new() }
    );
    emit();
}

fn do_restore(g: &mut Guarded) {
    let c = g.cfg.clone();
    let Some(job) = g.rjob.take() else { return };
    let to = if job.to.as_os_str().is_empty() { env().home.clone() } else { job.to.clone() };
    g.rres = RRes { done: false, ok: false, id: job.id.clone(), files: 0, kept: 0, replaced: 0, skipped: 0, errors: 0, error: Vec::new() };
    let Some(machine) = machine_dir(&c) else {
        g.rres.done = true;
        g.rres.error = b"backup.noDestination".to_vec();
        emit();
        return;
    };
    if g.dest_enc && !g.have_key {
        g.rres.done = true;
        g.rres.error = b"backup.locked".to_vec();
        emit();
        return;
    }
    g.state = State::Restoring;
    CANCEL.store(false, Relaxed);
    emit();
    let key: Option<Key> = g.dest_enc.then_some(g.key);
    let mut stats = RestoreStats::default();
    let r = g.unlocked(|| engine::restore(&machine, &job.id, &job.rel, &to, job.replace, &job.suffix, Some(&CANCEL), key.as_ref(), &mut stats));
    if let Some(mut k) = key {
        crypt::wipe(&mut k);
    }
    g.state = State::Idle;
    g.rres.done = true;
    g.rres.ok = r == 0 && stats.errors == 0;
    g.rres.files = stats.files;
    g.rres.kept = stats.kept_both;
    g.rres.replaced = stats.replaced;
    g.rres.skipped = stats.skipped;
    g.rres.errors = stats.errors;
    g.rres.error = if r > 0 { b"backup.interrupted".to_vec() } else { stats.error.clone() };
    eprintln!("backup: restored {}/{}: {} files, {} kept alongside, {} replaced, {} errors", job.id, job.rel, stats.files, stats.kept_both, stats.replaced, stats.errors);
    emit();
}

fn do_delete(g: &mut Guarded) {
    let c = g.cfg.clone();
    let Some(id) = g.djob.take() else { return };
    let Some(machine) = machine_dir(&c) else { return };
    g.state = State::Deleting;
    emit();
    g.unlocked(|| engine::delete(&machine, &id));
    g.state = State::Idle;
    emit();
}

/// Unlock (or set up) encryption: Argon2 is slow, so it runs in this thread, never in the compositor.
fn do_unlock(g: &mut Guarded) {
    let Some(mut job) = g.ujob.take() else { return };
    g.ures_done = false;
    g.ures_ok = false;
    g.ures_error = "";
    let c = g.cfg.clone();
    let Some(machine) = machine_dir(&c) else {
        crypt::wipe(&mut job.pass);
        g.ures_done = true;
        g.ures_error = "backup.noDestination";
        emit();
        return;
    };
    let before = g.state;
    g.state = State::Unlocking;
    emit();
    let create = job.create;
    let result: Result<Key, i32> = g.unlocked(|| {
        let enc = crypt::info_read(&machine).is_some();
        let r = if enc {
            crypt::unlock(&machine, &job.pass)
        } else if !create {
            Err(-2)
        } else if !engine::list(&machine, 4).is_empty() {
            Err(-3) // unencrypted backups already exist here
        } else {
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&machine);
            crypt::setup(&machine, &job.pass)
        };
        crypt::wipe(&mut job.pass);
        r
    });
    g.ures_done = true;
    match result {
        Ok(mut k) => {
            g.ures_ok = true;
            g.key = k;
            g.have_key = true;
            g.dest_enc = true;
            crypt::wipe(&mut k);
        }
        Err(code) => {
            g.ures_error = match code {
                -1 => "backup.wrongPassphrase",
                -2 => "backup.notEncrypted",
                -3 => "backup.plainBackupsExist",
                _ => "backup.setupFailed",
            }; // code lines: the UI translates them
        }
    }
    g.state = if before == State::Unlocking || before == State::Locked { State::Idle } else { before };
    emit();
}

fn thread_main() {
    unsafe {
        let mut all: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_BLOCK, &all, std::ptr::null_mut());
    }
    if std::env::var("LORD_BACKUP_NOPRIO").as_deref() != Ok("1") {
        unsafe {
            let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, tid, 19);
            // IOPRIO_WHO_PROCESS = 1, IOPRIO_CLASS_IDLE = 3 (<< 13)
            libc::syscall(libc::SYS_ioprio_set, 1, tid as i32, 3 << 13);
        }
    }
    let mut g = SVC.lock();
    loop {
        if g.rjob.is_some() {
            do_restore(&mut g);
            continue;
        }
        if g.djob.is_some() {
            do_delete(&mut g);
            continue;
        }
        if g.ujob.is_some() {
            do_unlock(&mut g);
            continue;
        }
        let now = wall();
        if g.cfg.dest.is_empty() {
            g.next_due = 0;
            g.state = State::Idle;
            g.want_run = false;
            g.wait();
            continue;
        }
        // is the destination encrypted? (encryption.json in the machine folder)
        let machine = machine_dir(&g.cfg);
        g.dest_enc = machine.as_ref().is_some_and(|m| crypt::info_read(m).is_some());
        if g.last_ok == 0 {
            if let Some(id) = machine.as_ref().and_then(|m| latest_id(m)) {
                g.last_ok = engine::id_time(id.as_bytes()).unwrap_or(0);
            }
        }
        let mut due = 0;
        if g.want_run {
            due = now;
        } else if g.cfg.interval > 0 {
            due = if g.last_ok != 0 { g.last_ok + g.cfg.interval } else { now };
            if g.last.ok == 0 && g.last_try != 0 && due < g.last_try + RETRY_FAIL {
                due = g.last_try + RETRY_FAIL;
            }
            if g.state == State::NoDest && due < g.last_try + RETRY_NODEST {
                due = g.last_try + RETRY_NODEST;
            }
            if due < g.started_at + START_DELAY {
                due = g.started_at + START_DELAY;
            }
        }
        g.next_due = due;
        if due != 0 && due <= now && (g.dest_enc || g.cfg.encrypt) && !g.have_key {
            // encrypted and no passphrase: wait for the user to enter it (never copy unencrypted)
            g.state = State::Locked;
            emit();
            g.wait();
            continue;
        }
        if due != 0 && due <= now {
            g.want_run = false;
            g.last_try = now;
            if g.cfg.battery_pause && on_battery() && !g.user_pause {
                // on battery: do not start; check again in a minute
                g.on_batt_pause = true;
                g.state = State::Paused;
                emit();
                g.want_run = true;
                g.wait_until(now + BATTERY_RECHECK);
                continue;
            }
            g.on_batt_pause = false;
            do_snapshot(&mut g);
            continue;
        }
        if g.state != State::NoDest {
            g.state = State::Idle;
        }
        emit();
        if due != 0 {
            g.wait_until(due);
        } else {
            g.wait();
        }
    }
}

/// Starts the service (after dropping root; once). `home` is the session's home folder.
pub fn start(home: &Path, user: &str) -> bool {
    if started() || home.as_os_str().is_empty() {
        return false;
    }
    let Ok(home) = std::fs::canonicalize(home) else { return false };
    let user: String = user.chars().map(|c| if matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') { c } else { '_' }).take(63).collect();
    let mut name = [0u8; 64];
    let hostname = if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len() - 1) } == 0 {
        let n = name.iter().position(|b| *b == 0).unwrap_or(name.len());
        String::from_utf8_lossy(&name[..n]).into_owned()
    } else {
        "lordos".into()
    };
    let hostname = if hostname.is_empty() { "lordos".to_string() } else { hostname };
    let host: String = hostname.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(63).collect();
    let cfg_path = home.join(".config/axia/backup.json");
    if ENV.set(Env { home, user, host, cfg_path }).is_err() {
        return false;
    }
    let efd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    EFD.store(efd, Relaxed);
    {
        let mut g = SVC.lock();
        unsafe { libc::mlock(g.key.as_ptr().cast(), crypt::KEY) }; // the key never goes to swap
        load_config(&mut g);
        g.started_at = wall();
    }
    if std::thread::Builder::new().name("axia-backup".into()).stack_size(2 << 20).spawn(thread_main).is_err() {
        unsafe { libc::close(efd) };
        EFD.store(-1, Relaxed);
        return false;
    }
    STARTED.store(true, Relaxed);
    true
}

/// The descriptor the main loop polls, or -1 if the service is not running.
pub fn event_fd() -> i32 {
    EFD.load(Relaxed)
}

/// Something changed: consumes the notification and gives the status JSON to `f`.
pub fn dispatch(f: impl FnOnce(&[u8])) {
    let fd = EFD.load(Relaxed);
    let mut v: u64 = 0;
    if fd < 0 || unsafe { libc::read(fd, (&mut v as *mut u64).cast(), 8) } != 8 {
        return;
    }
    if let Some(json) = status_json() {
        f(&json);
    }
}

// ---- API (main thread) ----

pub fn status_json() -> Option<Vec<u8>> {
    if !started() {
        return None;
    }
    let g = SVC.lock();
    let dest = Path::new(&g.cfg.dest);
    let avail = !g.cfg.dest.is_empty() && std::fs::metadata(dest).is_ok_and(|m| m.is_dir());
    let (mut freeb, mut totalb) = (-1.0f64, -1.0f64);
    if avail {
        let c = std::ffi::CString::new(dest.as_os_str().as_bytes()).unwrap_or_default();
        let mut v: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut v) } == 0 {
            freeb = v.f_bavail as f64 * v.f_frsize as f64;
            totalb = v.f_blocks as f64 * v.f_frsize as f64;
        }
    }
    let p = |c: &std::sync::atomic::AtomicU64| Progress::get(c);
    let b = |v: bool| if v { "true" } else { "false" };
    let mut out: Vec<u8> = Vec::new();
    let _ = write!(out, "{{\"state\":\"{}\",\"battery\":{},\"userPause\":{},\"dest\":", g.state.name(), b(g.on_batt_pause), b(g.user_pause));
    out.extend_from_slice(&quote(g.cfg.dest.as_bytes()));
    let _ = write!(
        out,
        ",\"destAvailable\":{},\"destFree\":{:.0},\"destTotal\":{:.0},\"next\":{},\"lastOk\":{},\"progress\":{{\"files\":{},\"totalFiles\":{},\"bytes\":{},\"totalBytes\":{},\"copied\":{},\"linked\":{},\"file\":",
        b(avail),
        freeb,
        totalb,
        g.next_due,
        g.last_ok,
        p(&RUN.files),
        p(&RUN.total_files),
        p(&RUN.bytes),
        p(&RUN.total_bytes),
        p(&RUN.copied),
        p(&RUN.linked)
    );
    out.extend_from_slice(&quote(&g.cur_file));
    let _ = write!(
        out,
        "}},\"last\":{{\"when\":{},\"ok\":{},\"id\":\"{}\",\"files\":{},\"copied\":{},\"linked\":{},\"bytes\":{},\"errors\":{},\"secs\":{:.1},\"error\":",
        g.last.when, g.last.ok, g.last.id, g.last.files, g.last.copied, g.last.linked, g.last.bytes, g.last.errors, g.last.secs
    );
    out.extend_from_slice(&quote(&g.last_error));
    let _ = write!(
        out,
        "}},\"restore\":{{\"done\":{},\"ok\":{},\"id\":\"{}\",\"files\":{},\"keptBoth\":{},\"replaced\":{},\"skipped\":{},\"errors\":{},\"error\":",
        b(g.rres.done),
        b(g.rres.ok),
        g.rres.id,
        g.rres.files,
        g.rres.kept,
        g.rres.replaced,
        g.rres.skipped,
        g.rres.errors
    );
    out.extend_from_slice(&quote(&g.rres.error));
    let _ = write!(
        out,
        "}},\"encryption\":true,\"destEncrypted\":{},\"encrypted\":{},\"locked\":{},\"unlock\":{{\"done\":{},\"ok\":{},\"error\":\"{}\"}}}}",
        b(g.dest_enc),
        b(g.dest_enc || g.cfg.encrypt),
        b((g.dest_enc || g.cfg.encrypt) && !g.have_key),
        b(g.ures_done),
        b(g.ures_ok),
        g.ures_error
    );
    Some(out)
}

pub fn config_json() -> Option<Vec<u8>> {
    started().then(|| config_text(&SVC.lock().cfg))
}

fn not_running() -> Vec<u8> {
    errcode::code("backup.notRunning")
}

pub fn configure(json: &[u8]) -> Result<(), Vec<u8>> {
    if !started() {
        return Err(not_running());
    }
    if json.len() > 60000 {
        return Err(errcode::code("backup.configTooLarge"));
    }
    let c = parse_config(json, true)?;
    if !save_config(&c) {
        return Err(errcode::code("backup.saveFailed"));
    }
    let mut g = SVC.lock();
    let changed_dest = c.dest != g.cfg.dest;
    g.cfg = c;
    if changed_dest {
        g.last_ok = 0;
        g.last.ok = 1;
        g.last_try = 0;
        if g.state == State::NoDest || g.state == State::Locked {
            g.state = State::Idle;
        }
        crypt::wipe(&mut g.key); // the key belonged to another destination
        g.have_key = false;
        g.dest_enc = false;
    }
    SVC.notify_all();
    emit();
    Ok(())
}

pub fn run_now() -> Result<(), Vec<u8>> {
    if !started() {
        return Err(not_running());
    }
    let mut g = SVC.lock();
    if g.cfg.dest.is_empty() {
        return Err(errcode::code("backup.noDestination"));
    }
    if g.state == State::Running || g.state == State::Restoring {
        return Err(errcode::code("backup.alreadyRunning"));
    }
    g.want_run = true;
    g.user_pause = false;
    if g.state == State::NoDest {
        g.state = State::Idle;
    }
    g.last_try = 0;
    SVC.notify_all();
    Ok(())
}

pub fn pause(on: bool) {
    if !started() {
        return;
    }
    let mut g = SVC.lock();
    g.user_pause = on;
    SVC.notify_all();
    emit();
}

pub fn cancel() {
    if !started() {
        return;
    }
    let mut g = SVC.lock();
    CANCEL.store(true, Relaxed);
    g.user_pause = false;
    g.want_run = false;
    SVC.notify_all();
}

fn machine_now() -> Option<PathBuf> {
    machine_dir(&SVC.lock().cfg)
}

pub fn list_json() -> Option<Vec<u8>> {
    if !started() {
        return None;
    }
    let ids = machine_now().map(|m| engine::list(&m, MAX_SNAPS)).unwrap_or_default();
    let mut out = b"[".to_vec();
    for (i, id) in ids.iter().enumerate() {
        let _ = write!(out, "{}{{\"id\":\"{id}\",\"time\":{}}}", if i > 0 { "," } else { "" }, engine::id_time(id.as_bytes()).unwrap_or(-1));
    }
    out.push(b']');
    Some(out)
}

/// Real path of a snapshot (to open it in Files).
pub fn snapshot_path(id: &str) -> Option<PathBuf> {
    if !started() || !engine::id_valid(id.as_bytes()) {
        return None;
    }
    std::fs::canonicalize(machine_now()?.join(id)).ok()
}

pub fn ls_json(id: &str, rel: &str) -> Result<Vec<u8>, Vec<u8>> {
    if !engine::rel_valid(rel.as_bytes()) {
        return Err(errcode::code("backup.badPath"));
    }
    let Some(snap) = snapshot_path(id) else { return Err(errcode::code("backup.noSuchBackup")) };
    let dir = if rel.is_empty() { snap.clone() } else { snap.join(rel) };
    let real = std::fs::canonicalize(&dir).ok().filter(|r| {
        let (r, s) = (r.as_os_str().as_bytes(), snap.as_os_str().as_bytes());
        r.starts_with(s) && (r.len() == s.len() || r[s.len()] == b'/')
    });
    let Some(real) = real else { return Err(errcode::code("backup.badPath")) };
    let Ok(read) = std::fs::read_dir(&real) else { return Err(errcode::code("backup.readFailed")) };
    let enc = SVC.lock().dest_enc;
    struct Ent {
        name: Vec<u8>,
        dir: bool,
        link: bool,
        size: f64,
        mtime: f64,
    }
    let mut ents: Vec<Ent> = vec![];
    for e in read.flatten() {
        if ents.len() >= 20000 {
            break;
        }
        let Ok(st) = std::fs::symlink_metadata(e.path()) else { continue };
        let mut size = st.size();
        if enc && st.is_file() {
            size = crypt::plain_size(st.size()).unwrap_or(0);
        }
        ents.push(Ent { name: e.file_name().into_vec(), dir: st.is_dir(), link: st.file_type().is_symlink(), size: size as f64, mtime: st.mtime() as f64 });
    }
    ents.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    let mut out = b"[".to_vec();
    for (i, e) in ents.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"name\":");
        out.extend_from_slice(&quote(&e.name));
        let _ = write!(out, ",\"dir\":{},\"link\":{},\"size\":{:.0},\"mtime\":{:.0}}}", e.dir, e.link, e.size, e.mtime);
    }
    out.push(b']');
    Ok(out)
}

pub fn restore(id: &str, rel: &str, replace: bool, suffix: &[u8], to: &Path) -> Result<(), Vec<u8>> {
    if !started() {
        return Err(not_running());
    }
    if !engine::id_valid(id.as_bytes()) || !engine::rel_valid(rel.as_bytes()) || rel.len() >= 2048 {
        return Err(errcode::code("backup.badPath"));
    }
    if suffix.is_empty() || suffix.len() >= 80 || suffix.contains(&b'/') {
        return Err(errcode::code("backup.badSuffix"));
    }
    let mut target = PathBuf::new();
    if !to.as_os_str().is_empty() {
        let policy = policy::current();
        match std::fs::canonicalize(to) {
            Ok(t) if policy.allowed(&t) && !policy.protected(&t) => target = t,
            _ => return Err(errcode::code("backup.destNotAllowed")),
        }
    }
    let mut g = SVC.lock();
    if matches!(g.state, State::Running | State::Restoring | State::Deleting) || g.rjob.is_some() {
        return Err(errcode::code("backup.busy"));
    }
    g.rjob = Some(RJob { id: id.into(), rel: rel.into(), suffix: suffix.to_vec(), to: target, replace });
    g.rres.done = false;
    SVC.notify_all();
    Ok(())
}

pub fn delete(id: &str) -> Result<(), Vec<u8>> {
    if !started() {
        return Err(not_running());
    }
    if !engine::id_valid(id.as_bytes()) {
        return Err(errcode::code("backup.badId"));
    }
    let mut g = SVC.lock();
    if g.state != State::Idle && g.state != State::NoDest {
        return Err(errcode::code("backup.busy"));
    }
    g.djob = Some(id.into());
    SVC.notify_all();
    Ok(())
}

pub fn unlock(pass: &[u8], create: bool) -> Result<(), Vec<u8>> {
    if !started() {
        return Err(not_running());
    }
    let min = if create { 8 } else { 1 };
    if pass.len() < min || pass.len() >= 256 {
        return Err(errcode::code(if create { "backup.passphraseShort" } else { "backup.badPassphrase" }));
    }
    let mut g = SVC.lock();
    if g.cfg.dest.is_empty() {
        return Err(errcode::code("backup.noDestination"));
    }
    if matches!(g.state, State::Running | State::Restoring | State::Unlocking) || g.ujob.is_some() {
        return Err(errcode::code("backup.busy"));
    }
    g.ujob = Some(UJob { create, pass: pass.to_vec() });
    g.ures_done = false;
    SVC.notify_all();
    Ok(())
}

pub fn lock() {
    if !started() {
        return;
    }
    let mut g = SVC.lock();
    crypt::wipe(&mut g.key);
    g.have_key = false;
    emit();
}
