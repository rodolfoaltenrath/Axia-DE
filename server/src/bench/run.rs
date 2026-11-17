//! Single-run coordinator. Jobs, children, descriptors and result buffers have
//! explicit owners; cancellation is atomic and never aliases a worker's state.
use super::*;
use libc::{c_char, c_int};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Waiting,
    Running,
    Done,
    Stopped,
    Failed,
    Cancelled,
}
impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    fn active(self) -> bool {
        matches!(self, Self::Waiting | Self::Running)
    }
}
struct Progress {
    state: State,
    mode: &'static str,
    step: i32,
    done: usize,
    expected: usize,
    result: Option<Value>,
}
static PROGRESS: Mutex<Progress> = Mutex::new(Progress {
    state: State::Idle,
    mode: "",
    step: -1,
    done: 0,
    expected: 0,
    result: None,
});
fn progress() -> std::sync::MutexGuard<'static, Progress> {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner())
}
static CANCEL: AtomicBool = AtomicBool::new(false);
struct Job {
    full: bool,
    desktop: bool,
    sustained: i32,
    extra: Option<Value>,
}
fn name(p: *const c_char) -> String {
    unsafe { String::from_utf8_lossy(bytes(p)).into_owned() }
}
fn build_version() -> String {
    let p: Value = serde_json::from_str(include_str!("../../../package.json")).unwrap();
    let version = p["version"].as_str().unwrap_or("0.0.0");
    let channel = p["channel"].as_str().unwrap_or("");
    format!(
        "{version}{}{channel}",
        if channel.is_empty() { "" } else { "-" }
    )
}
fn export(s: &str) -> *mut c_char {
    unsafe {
        let p = libc::malloc(s.len() + 1) as *mut c_char;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(s.as_ptr(), p.cast(), s.len());
            *p.add(s.len()) = 0
        }
        p
    }
}
fn condition() -> LbCond {
    let mut c = LbCond::default();
    unsafe { lb_cond_read(c"".as_ptr(), &mut c) }
    c
}
fn helper_path() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("axia-bench");
    super::video::trusted(&p).then_some(p)
}
fn state_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| PathBuf::from(p).join(".local/state"))
        })
        .map(|p| p.join("axia"))
}
const SAVED: [&str; 4] = [
    "lordbench",
    "lordbench-prev",
    "lordbench-desktop",
    "lordbench-desktop-prev",
];
fn read_bounded(p: &Path, trusted: bool) -> Option<Vec<u8>> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(p)
        .ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file()
        || m.len() > 256 * 1024
        || (trusted
            && (m.mode() & 0o022 != 0 || (m.uid() != 0 && m.uid() != unsafe { libc::getuid() })))
    {
        return None;
    }
    let mut bytes = Vec::new();
    f.take(256 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= 256 * 1024).then_some(bytes)
}
#[no_mangle]
pub extern "C" fn lbrun_saved(which: c_int) -> *mut c_char {
    SAVED
        .get(which as usize)
        .and_then(|n| read_bounded(&state_dir()?.join(format!("{n}.json")), false))
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|s| !s.is_empty())
        .map(|s| export(&s))
        .unwrap_or(std::ptr::null_mut())
}
fn save_result(result: &Value, desktop: bool) -> std::io::Result<()> {
    let dir = state_dir().ok_or_else(|| std::io::Error::other("state directory"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let n = if desktop { SAVED[2] } else { SAVED[0] };
    let a = dir.join(format!("{n}.json"));
    let b = dir.join(format!("{n}-prev.json"));
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = dir.join(format!(".{n}.{}.{nonce}", unsafe { libc::getpid() }));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)?;
    let outcome = (|| {
        serde_json::to_writer(&mut f, result)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        if a.exists() {
            std::fs::rename(&a, &b)?
        }
        std::fs::rename(&temp, &a)?;
        File::open(&dir)?.sync_all()
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    outcome
}
fn render_node() -> Option<OwnedFd> {
    (128..136).find_map(|i| {
        let p = CString::new(format!("/dev/dri/renderD{i}")).ok()?;
        let fd = unsafe { libc::open(p.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOCTTY) };
        (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
    })
}
fn scratch() -> Option<(OwnedFd, u32, PathBuf)> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| PathBuf::from(p).join(".cache"))
        })?;
    let _ = std::fs::create_dir(&dir);
    let p = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let fd = unsafe {
        libc::open(
            p.as_ptr(),
            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut v = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatvfs(fd.as_raw_fd(), &mut v) } != 0 {
        return None;
    }
    let mb = ((v.f_bavail as u128 * v.f_frsize as u128 / (1 << 20)) / 4).min(1024) as u32;
    (mb >= 64).then_some((fd, mb, dir))
}
struct Helper {
    child: Child,
    socket: OwnedFd,
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn spawn(path: &Path, go: &LbGo, fds: &[i32]) -> Option<Helper> {
    unsafe {
        let mut sv = [-1; 2];
        if libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            sv.as_mut_ptr(),
        ) != 0
        {
            return None;
        }
        let socket = OwnedFd::from_raw_fd(sv[0]);
        let peer = OwnedFd::from_raw_fd(sv[1]);
        let raw = peer.as_raw_fd();
        let mut cmd = Command::new(path);
        cmd.env_clear().env("LANG", "C.UTF-8");
        if std::env::var_os("LORDOS_SESSIOND").is_some() {
            cmd.env("LORDOS_SESSIOND", "1");
        }
        cmd.pre_exec(move || {
            if libc::dup2(raw, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut empty = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            for sig in 1..65 {
                libc::signal(sig, libc::SIG_DFL);
            }
            Ok(())
        });
        let child = cmd.spawn().ok()?;
        drop(peer);
        let helper = Helper { child, socket };
        let mut control = [0usize; 8];
        let mut iov = libc::iovec {
            iov_base: (go as *const LbGo).cast_mut().cast(),
            iov_len: std::mem::size_of::<LbGo>(),
        };
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if !fds.is_empty() {
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) as usize;
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as usize;
            std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(c).cast(), fds.len());
        }
        (libc::sendmsg(helper.socket.as_raw_fd(), &msg, libc::MSG_NOSIGNAL)
            == std::mem::size_of::<LbGo>() as isize)
            .then_some(helper)
    }
}
enum Record {
    Wait,
    Message(LbMsg),
    Error(&'static str),
}
fn receive(fd: i32) -> Record {
    unsafe {
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let pr = libc::poll(&mut p, 1, 250);
        if pr == 0 {
            return Record::Wait;
        }
        if pr < 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                Record::Wait
            } else {
                Record::Error("helper")
            };
        }
        let mut m = LbMsg::default();
        let n = libc::recv(
            fd,
            (&mut m as *mut LbMsg).cast(),
            std::mem::size_of::<LbMsg>(),
            libc::MSG_DONTWAIT | libc::MSG_TRUNC,
        );
        if n < 0
            && matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR) | Some(libc::EAGAIN)
            )
        {
            return Record::Wait;
        }
        if n <= 0 {
            return Record::Error("helper");
        }
        if n != std::mem::size_of::<LbMsg>() as isize || lb_msg_valid(&m) == 0 {
            return Record::Error("protocol");
        }
        Record::Message(m)
    }
}
struct Results {
    value: [Option<(f64, f64)>; LB_COUNT as usize],
    skip: [i32; LB_COUNT as usize],
    gpu_kind: i32,
    sandbox: i32,
    samples: Vec<[Option<f64>; 3]>,
    throttle: Option<(f64, f64)>,
    rpi: LbRpiWatch,
    budget: f64,
    gap: f64,
    phases: [Option<LbPhaseStats>; 2],
    sbx: [Option<(bool, i32)>; LB_SBX_COUNT as usize],
}
impl Default for Results {
    fn default() -> Self {
        Self {
            value: [None; LB_COUNT as usize],
            skip: [0; LB_COUNT as usize],
            gpu_kind: -1,
            sandbox: 0,
            samples: Vec::new(),
            throttle: None,
            rpi: LbRpiWatch::default(),
            budget: 0.0,
            gap: 0.0,
            phases: [None, None],
            sbx: [None; LB_SBX_COUNT as usize],
        }
    }
}
impl Results {
    fn put(&mut self, id: i32, v: f64, s: f64) {
        let i = id as usize;
        if self.value[i].is_none() && self.skip[i] == 0 {
            progress().done += 1
        }
        self.value[i] = Some((v, s));
    }
    fn skip(&mut self, id: i32, why: i32) {
        let i = id as usize;
        if self.value[i].is_none() && self.skip[i] == 0 {
            progress().done += 1
        }
        self.skip[i] = why;
    }
    fn sandbox(&mut self, m: &LbMsg) {
        self.sbx[m.id as usize] = Some((m.value == 1.0, m.aux as i32));
    }
    fn frames(&mut self, first: i32) -> bool {
        unsafe {
            let mut fs = LbFrameStats::default();
            if lbf_end(&mut fs) != 0 || fs.shown < 30 {
                for i in 0..6 {
                    self.skip(first + i, LB_SKIP_NOFRAMES)
                }
                return false;
            }
            for (i, v) in [fs.fps, fs.work_p50, fs.work_p99, fs.missed_pct]
                .into_iter()
                .enumerate()
            {
                self.put(first + i as i32, v, 0.0)
            }
            if fs.inputs >= 30 {
                self.put(first + 4, fs.in_p50, 0.0);
                self.put(first + 5, fs.in_p99, 0.0)
            } else {
                self.skip(first + 4, LB_SKIP_FAILED);
                self.skip(first + 5, LB_SKIP_FAILED)
            }
            self.budget = fs.budget_ms;
            self.gap = self.gap.max(fs.gap_max);
            let mut ph = LbPhaseStats::default();
            lbf_phases(&mut ph);
            if ph.n > 0 {
                self.phases[(first == LB_DESK_FPS) as usize] = Some(ph)
            }
            true
        }
    }
}
fn set_step(id: i32) {
    let mut p = progress();
    p.state = State::Running;
    p.step = id;
}
struct Guard {
    before: LbCond,
    sustained: bool,
    next: Instant,
}
type Stop = (State, &'static str);
impl Guard {
    fn check(&mut self, r: &mut Results) -> Result<(), Stop> {
        if CANCEL.load(Ordering::Acquire) {
            return Err((State::Cancelled, ""));
        }
        if Instant::now() < self.next {
            return Ok(());
        }
        self.next = Instant::now() + Duration::from_secs(1);
        let c = condition();
        let ev = unsafe { lb_rpi_watch(&mut r.rpi, &c) };
        if self.sustained
            && r.throttle.is_none()
            && ((self.before.throttle_known != 0
                && c.throttle_events > self.before.throttle_events)
                || ev & LB_RPI_SLOWED != 0)
        {
            r.throttle = Some((r.samples.last().and_then(|s| s[0]).unwrap_or(0.0), c.temp_c));
        }
        let reason = unsafe { lb_cond_stop(&self.before, &c, ev, (!self.sustained) as i32) };
        if reason.is_null() {
            return Ok(());
        }
        let why = match unsafe { bytes(reason) } {
            b"battery" => "battery",
            b"thermal" => "thermal",
            b"hot" => "hot",
            b"throttled" => "throttled",
            _ => "thermal",
        };
        Err((State::Stopped, why))
    }
    fn wait(&mut self, r: &mut Results, ms: u64) -> Result<(), Stop> {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.check(r)?;
            thread::sleep(
                Duration::from_millis(100).min(end.saturating_duration_since(Instant::now())),
            )
        }
        Ok(())
    }
}
fn ticks() -> Option<(u64, u64)> {
    let (mut b, mut t) = (0, 0);
    (unsafe { lb_cpu_ticks(c"".as_ptr(), &mut b, &mut t) } == 0).then_some((b, t))
}
fn busy(a: (u64, u64), b: (u64, u64)) -> f64 {
    if b.1 > a.1 && b.0 >= a.0 {
        100.0 * (b.0 - a.0) as f64 / (b.1 - a.1) as f64
    } else {
        0.0
    }
}
struct Recording;
impl Drop for Recording {
    fn drop(&mut self) {
        unsafe {
            lbf_end(&mut LbFrameStats::default());
        }
    }
}
fn desktop(
    job: &Job,
    r: &mut Results,
    g: &mut Guard,
    path: &Path,
    threads: u32,
) -> Result<(), Stop> {
    set_step(LB_COMP_FPS);
    g.wait(r, 2500)?;
    lbf_begin();
    let recording = Recording;
    g.wait(r, 10000)?;
    std::mem::forget(recording);
    r.frames(LB_COMP_FPS);
    set_step(LB_DESK_FPS);
    let go = LbGo {
        magic: LB_MAGIC,
        mode: LB_MODE_LOAD as u32,
        flags: LB_GO_LOW as u32,
        threads,
        storage_mb: 0,
        sustained_s: 14,
    };
    let helper = spawn(path, &go, &[]).ok_or((State::Failed, "helper"))?;
    let end = Instant::now() + Duration::from_secs(40);
    let mut recording = None;
    let (mut work, mut n) = (0.0, 0);
    loop {
        g.check(r)?;
        if Instant::now() > end {
            return Err((State::Failed, "timeout"));
        }
        match receive(helper.socket.as_raw_fd()) {
            Record::Wait => (),
            Record::Error(s) => return Err((State::Failed, s)),
            Record::Message(m) => match m.kind as i32 {
                LB_MSG_SAMPLE if m.aux >= 2.0 => {
                    if recording.is_none() {
                        lbf_begin();
                        recording = Some(Recording)
                    } else {
                        work += m.value;
                        n += 1
                    }
                }
                LB_MSG_SANDBOX => r.sandbox(&m),
                LB_MSG_DONE => {
                    r.sandbox = m.aux as i32;
                    break;
                }
                _ => (),
            },
        }
    }
    let framed = if let Some(recording) = recording.take() {
        std::mem::forget(recording);
        r.frames(LB_DESK_FPS)
    } else {
        // No load sample started a recorder: never reuse the baseline's frames.
        for i in 0..6 {
            r.skip(LB_DESK_FPS + i, LB_SKIP_NOFRAMES);
        }
        false
    };
    if framed && n > 0 {
        r.put(LB_DESK_WORK, work / n as f64, 0.0)
    } else {
        r.skip(
            LB_DESK_WORK,
            if framed {
                LB_SKIP_FAILED
            } else {
                LB_SKIP_NOFRAMES
            },
        )
    }
    let _ = job;
    Ok(())
}
struct Video<'a> {
    r: &'a mut Results,
    g: &'a mut Guard,
    stop: Option<Stop>,
}
unsafe extern "C" fn video_record(ctx: *mut libc::c_void, k: i32, id: i32, v: f64, a: f64) {
    let vctx = &mut *(ctx as *mut Video<'_>);
    match k {
        LB_MSG_STEP => set_step(id),
        LB_MSG_RESULT => vctx.r.put(id, v, a),
        _ => vctx.r.skip(id, a as i32),
    }
}
unsafe extern "C" fn video_tick(ctx: *mut libc::c_void) -> i32 {
    let v = &mut *(ctx as *mut Video<'_>);
    v.stop = v.g.check(v.r).err();
    v.stop.is_some() as i32
}
fn peak(
    job: &Job,
    r: &mut Results,
    g: &mut Guard,
    path: &Path,
    threads: u32,
    storage: &mut (String, u32),
) -> Result<(), Stop> {
    let gpu = render_node();
    let disk = if job.full { scratch() } else { None };
    if let Some((_, mb, dir)) = &disk {
        let p = CString::new(dir.as_os_str().as_bytes()).unwrap();
        *storage = (
            name(unsafe { lb_storage_kind(c"".as_ptr(), p.as_ptr()) }),
            *mb,
        )
    }
    let go = LbGo {
        magic: LB_MAGIC,
        mode: if job.full {
            LB_MODE_FULL
        } else {
            LB_MODE_QUICK
        } as u32,
        flags: (if gpu.is_some() { LB_GO_GPU } else { 0 }
            | if disk.is_some() { LB_GO_STORAGE } else { 0 }) as u32,
        threads,
        storage_mb: storage.1,
        sustained_s: job.sustained as u32,
    };
    let fds: Vec<_> = gpu
        .iter()
        .map(AsRawFd::as_raw_fd)
        .chain(disk.iter().map(|(f, _, _)| f.as_raw_fd()))
        .collect();
    let helper = spawn(path, &go, &fds).ok_or((State::Failed, "helper"))?;
    drop(gpu);
    drop(disk);
    progress().state = State::Running;
    let end = Instant::now()
        + Duration::from_secs(if job.full {
            600 + job.sustained as u64
        } else {
            180
        });
    loop {
        g.check(r)?;
        if Instant::now() > end {
            return Err((State::Failed, "timeout"));
        }
        match receive(helper.socket.as_raw_fd()) {
            Record::Wait => (),
            Record::Error(s) => return Err((State::Failed, s)),
            Record::Message(m) => match m.kind as i32 {
                LB_MSG_STEP => {
                    set_step(m.id as i32);
                    if m.id == LB_SUS_START as u32 {
                        g.sustained = true
                    }
                }
                LB_MSG_RESULT => r.put(m.id as i32, m.value, m.aux),
                LB_MSG_SKIP => r.skip(m.id as i32, m.aux as i32),
                LB_MSG_SAMPLE => {
                    if r.samples.len() < 200 {
                        let c = condition();
                        r.samples.push([
                            Some(m.aux),
                            Some(m.value),
                            (c.temp_c > -270.0).then_some(c.temp_c),
                        ])
                    }
                }
                LB_MSG_GPUINFO => r.gpu_kind = m.value as i32,
                LB_MSG_SANDBOX => r.sandbox(&m),
                LB_MSG_DONE => {
                    r.sandbox = m.aux as i32;
                    break;
                }
                _ => (),
            },
        }
    }
    drop(helper);
    g.sustained = false;
    let mut vc = Video { r, g, stop: None };
    let vr = unsafe {
        lbv_run(
            job.full as i32,
            Some(video_record),
            Some(video_tick),
            (&mut vc as *mut Video<'_>).cast(),
        )
    };
    if let Some(stop) = vc.stop {
        return Err(stop);
    }
    if vr != 0 {
        for id in LB_VID_FIRST..=LB_VID_LAST {
            if vc.r.value[id as usize].is_none() && vc.r.skip[id as usize] == 0 {
                vc.r.skip(
                    id,
                    if vr == -1 {
                        LB_SKIP_NODEV
                    } else {
                        LB_SKIP_FAILED
                    },
                )
            }
        }
    }
    Ok(())
}
fn cond_json(c: &LbCond) -> Value {
    let mut v = json!({"power":if c.on_battery<0{"unknown"}else if c.on_battery!=0{"battery"}else{"mains"}});
    for (k, x, yes) in [
        ("tempC", c.temp_c, c.temp_c > -270.0),
        ("passiveC", c.passive_c, c.passive_c > 0.0),
        ("criticalC", c.critical_c, c.critical_c > 0.0),
        ("freqMHz", c.freq_mhz, c.freq_mhz > 0.0),
        ("freqMaxMHz", c.freq_max_mhz, c.freq_max_mhz > 0.0),
    ] {
        if yes {
            v[k] = json!(x)
        }
    }
    for (k, x) in [
        ("zone", &c.zone),
        ("governor", &c.governor),
        ("platformProfile", &c.platform_profile),
    ] {
        if x[0] != 0 {
            v[k] = json!(name(x.as_ptr()))
        }
    }
    if c.throttle_known != 0 {
        v["throttleEvents"] = json!(c.throttle_events)
    }
    for (k, n) in [
        ("rpiThrottled", c.rpi_throttled),
        ("rpiUnderVoltageAlarm", c.rpi_uv_alarm),
        ("batteryPct", c.battery_pct),
    ] {
        if n >= 0 {
            v[k] = json!(n)
        }
    }
    v
}
fn phases(p: &LbPhaseStats) -> Value {
    let mut v = json!({"frames":p.n,"uploadMpx":p.up_mpx,"gpuDragUploadMpx":p.gdrag_mpx,"catchUpMpx":p.feed_mpx,"onTime":p.on_time,"cpuLate":p.cpu_late,"gpuLate":p.gpu_late,"otherLate":p.other_late,"gpuKnown":p.gpu_known,"paintThreads":p.par,"unshown":p.unshown,"prepared":p.piped});
    for (k, x) in [
        ("wakeMs", p.wake),
        ("beforeRenderMs", p.pre),
        ("renderMs", p.build),
        ("paintMs", p.paint),
        ("uploadMs", p.up),
        ("gpuDragUploadMs", p.gdragup),
        ("swapMs", p.swap),
        ("flipIoctlMs", p.submit_call),
        ("gpuMs", p.gpu),
        ("vblankWaitMs", p.wait),
        ("wakeToFlipMs", p.chain),
        ("catchUpMs", p.feed),
        ("heldMs", p.held),
    ] {
        v[k] = json!([x.p50, x.p99])
    }
    let mut refused = json!({});
    for (i, n) in p.gdrag_refused.iter().enumerate() {
        let k = lbf_gdrag_r_name(i as i32);
        if *n != 0 && !k.is_null() {
            refused[name(k)] = json!(n)
        }
    }
    let mut layer = json!({});
    for (i, n) in p.gdrag_layer.iter().enumerate() {
        let k = lbf_gdrag_l_name(i as i32);
        if *n != 0 && !k.is_null() {
            layer[name(k)] = json!(n)
        }
    }
    v["gpuDrag"] = json!({"tried":p.gdrag_tried,"taken":p.gdrag_taken,"wholeUploads":p.gdrag_full,"wholeMpx":p.gdrag_full_px as f64/1e6,"refused":refused,"layer":layer});
    v
}
fn build_result(
    job: &Job,
    st: State,
    why: &str,
    r: &Results,
    before: &LbCond,
    after: &LbCond,
    busy: f64,
    waited: f64,
    quiet: bool,
    threads: u32,
    storage: &(String, u32),
    secs: f64,
) -> Value {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    unsafe { libc::uname(&mut u) };
    let mut v = json!({"lordbench":LB_VERSION,"mode":if job.desktop{"desktop"}else if job.full{"full"}else{"quick"},"at":unsafe{libc::time(std::ptr::null_mut())},"seconds":secs.round(),"status":st.name(),"stopReason":why,"lordos":build_version(),"kernel":name(u.release.as_ptr()),"arch":name(u.machine.as_ptr()),"live":Path::new("/run/live").exists(),"threads":threads,"cpus":unsafe{libc::sysconf(libc::_SC_NPROCESSORS_ONLN)},"quiet":{"quiet":quiet,"busyPct":busy,"waitedS":waited},"before":cond_json(before),"after":cond_json(after),"storage":{"kind":storage.0,"mb":storage.1},"gpuKind":match r.gpu_kind{0=>"hardware",1=>"virtual",2=>"software",_=>""},"sandbox":r.sandbox,"results":{},"skipped":{}});
    if let Some(extra) = &job.extra {
        v["session"] = extra.clone()
    }
    for i in 0..LB_COUNT {
        let k = name(lb_name(i));
        if let Some((val, s)) = r.value[i as usize] {
            let mut result = json!({"value":val,"spread":s,"unit":name(lb_unit(i))});
            if lb_lower_better(i) != 0 {
                result["lowerBetter"] = json!(true)
            }
            v["results"][k] = result
        } else if let Some(why) = [
            "", "nodev", "failed", "mode", "memory", "nocodec", "noframes",
        ]
        .get(r.skip[i as usize] as usize)
        .filter(|v| !v.is_empty())
        {
            v["skipped"][k] = json!(why)
        }
    }
    if r.rpi.rpi != 0 {
        let w = &r.rpi;
        let mut p = json!({"flags":w.bits!=0,"voltageAlarm":w.alarm!=0,"tempLimit":w.seen&LB_RPI_HOT!=0,"tempLimitC":LB_RPI_FW_TEMP_C});
        if w.bits != 0 || w.alarm != 0 {
            p["underVoltage"] = json!(w.seen & LB_RPI_UV != 0)
        }
        if w.bits != 0 {
            p["capped"] = json!(w.seen & LB_RPI_CAPPED != 0);
            p["throttled"] = json!(w.seen & LB_RPI_THROTTLED != 0);
            p["softLimit"] = json!(w.seen & LB_RPI_SOFTTEMP != 0)
        }
        v["piFirmware"] = p
    }
    let sbx:Vec<_>=r.sbx.iter().enumerate().filter_map(|(i,s)|s.map(|(denied,errno)|json!({"id":name(lb_sbx_name(i as i32)),"denied":denied,"errno":errno}))).collect();
    if !sbx.is_empty() {
        v["sandboxTries"] = json!(sbx)
    }
    if r.budget > 0.0 {
        let mut f = json!({"budgetMs":r.budget,"gapMaxMs":r.gap});
        for (i, p) in r.phases.iter().enumerate() {
            if let Some(p) = p {
                if f.get("phases").is_none() {
                    f["phases"] = json!({})
                }
                f["phases"][if i == 0 { "alone" } else { "load" }] = phases(p)
            }
        }
        v["frames"] = f
    }
    if !r.samples.is_empty() {
        v["sustained"] = json!({"samples":r.samples});
        if let Some((t, c)) = r.throttle {
            v["sustained"]["throttleAtS"] = json!(t);
            v["sustained"]["throttleTempC"] = json!(c)
        }
    }
    v
}
fn run(job: Job) {
    let start = Instant::now();
    let before = condition();
    let mut r = Results::default();
    unsafe { lb_rpi_watch_start(&mut r.rpi, &before) };
    let mut g = Guard {
        before: before.clone(),
        sustained: false,
        next: Instant::now(),
    };
    let (mut busy_pct, mut waited, mut quiet) = (0.0, 0.0, false);
    let threads = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }.clamp(1, 256) as u32;
    let mut storage = ("none".into(), 0);
    let outcome = (|| {
        if unsafe { lb_cond_battery_low(&before) } != 0 {
            return Err((State::Stopped, "battery"));
        }
        let w0 = Instant::now();
        let first = ticks();
        let mut prev = first;
        let mut calm = 0;
        while w0.elapsed() < Duration::from_secs(30) {
            g.wait(&mut r, 1000)?;
            let Some(now) = ticks() else { break };
            if let Some(prev) = prev {
                calm = if busy(prev, now) < 15.0 { calm + 1 } else { 0 }
            }
            prev = Some(now);
            if calm >= 10 {
                quiet = true;
                break;
            }
        }
        waited = w0.elapsed().as_secs_f64();
        if let (Some(a), Some(b)) = (first, ticks()) {
            busy_pct = busy(a, b)
        }
        g.check(&mut r)?;
        let path = helper_path().ok_or((State::Failed, "helper"))?;
        if job.desktop {
            desktop(&job, &mut r, &mut g, &path, threads)
        } else {
            peak(&job, &mut r, &mut g, &path, threads, &mut storage)
        }
    })();
    let (st, why) = outcome.err().unwrap_or((State::Done, ""));
    let after = condition();
    unsafe { lb_rpi_watch(&mut r.rpi, &after) };
    let result = build_result(
        &job,
        st,
        why,
        &r,
        &before,
        &after,
        busy_pct,
        waited,
        quiet,
        threads,
        &storage,
        start.elapsed().as_secs_f64(),
    );
    if matches!(st, State::Done | State::Stopped) && r.value.iter().any(Option::is_some) {
        let _ = save_result(&result, job.desktop);
    }
    let mut p = progress();
    p.result = Some(result);
    p.state = st;
}
#[no_mangle]
pub unsafe extern "C" fn lbrun_start(
    mode: *const c_char,
    sustained: c_int,
    extra: *const c_char,
) -> c_int {
    let mode = match bytes(mode) {
        b"quick" => "quick",
        b"full" => "full",
        b"desktop" => "desktop",
        _ => return -1,
    };
    if sustained != 0 && !(20..=1800).contains(&sustained) {
        return -1;
    }
    let extra = if extra.is_null() {
        None
    } else {
        let b = bytes(extra);
        if b.len() > 64 * 1024 {
            return -1;
        }
        let Ok(v) = serde_json::from_slice(b) else {
            return -1;
        };
        Some(v)
    };
    if helper_path().is_none() {
        return -1;
    }
    let mut p = progress();
    if p.state.active() {
        return -1;
    }
    p.state = State::Waiting;
    p.mode = mode;
    p.step = -1;
    p.done = 0;
    p.expected = if mode == "desktop" { 13 } else { 32 };
    p.result = None;
    CANCEL.store(false, Ordering::Release);
    let job = Job {
        full: mode == "full",
        desktop: mode == "desktop",
        sustained: if mode == "full" { sustained } else { 0 },
        extra,
    };
    let spawned = thread::Builder::new()
        .name("lordbench".into())
        .spawn(move || {
            if std::panic::catch_unwind(|| run(job)).is_err() {
                let mut p = PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
                p.state = State::Failed;
            }
        });
    if spawned.is_err() {
        p.state = State::Idle;
        return -1;
    }
    0
}
#[no_mangle]
pub extern "C" fn lbrun_cancel() {
    if progress().state.active() {
        CANCEL.store(true, Ordering::Release)
    }
}
#[no_mangle]
pub extern "C" fn lbrun_poll() -> *mut c_char {
    let p = progress();
    let mut v = json!({"state":p.state.name(),"mode":p.mode,"step":if p.step>=0{name(lb_name(p.step))}else{String::new()},"progress":if p.expected>0{(p.done as f64/p.expected as f64).min(1.0)}else{0.0}});
    if !p.state.active() {
        if let Some(result) = &p.result {
            v["result"] = result.clone()
        }
    }
    export(&v.to_string())
}

fn boot_history(data: &[u8]) -> Value {
    struct Version {
        v: String,
        s: Vec<f64>,
        first: Option<f64>,
        t: Vec<f64>,
        of: i64,
        at: i64,
    }
    let mut versions: Vec<Version> = Vec::new();
    let mut last = String::new();
    for line in data.split(|b| *b == b'\n') {
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let (Some(ver), Some(s)) = (v["version"].as_str(), v["seconds"].as_f64()) else {
            continue;
        };
        if ver.is_empty()
            || ver.len() >= 40
            || !ver
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-+~".contains(&b))
            || !(s > 0.0 && s < 3600.0)
        {
            continue;
        }
        let i = versions
            .iter()
            .position(|x| x.v == ver)
            .unwrap_or(versions.len());
        if i == versions.len() && i < 32 {
            versions.push(Version {
                v: ver.into(),
                s: Vec::new(),
                first: None,
                t: Vec::new(),
                of: 0,
                at: 0,
            })
        }
        if let Some(x) = versions.get_mut(i) {
            let test = v["test"].as_i64().unwrap_or(0);
            let of = v["testOf"].as_i64().unwrap_or(0);
            if (1..=BT_MAX as i64).contains(&test) && (BT_MIN as i64..=BT_MAX as i64).contains(&of)
            {
                if test == 1 {
                    x.t.clear();
                    x.of = of;
                    x.at = v["at"].as_i64().unwrap_or(0)
                }
                if x.of == of && x.t.len() == test as usize - 1 {
                    x.t.push(s)
                }
            }
            if last != ver && x.first.is_none() {
                x.first = Some(s)
            } else if x.s.len() < 200 {
                x.s.push(s)
            }
        }
        last = ver.into();
    }
    fn stats(v: &mut Vec<f64>) -> Value {
        v.sort_by(f64::total_cmp);
        let n = v.len();
        json!({"median":if n%2==1{v[n/2]}else{(v[n/2-1]+v[n/2])/2.0},"min":v[0],"max":v[n-1]})
    }
    Value::Array(
        versions
            .into_iter()
            .map(|mut x| {
                let mut v = json!({"version":x.v,"boots":x.s.len()});
                if !x.s.is_empty() {
                    v.as_object_mut()
                        .unwrap()
                        .extend(stats(&mut x.s).as_object().unwrap().clone())
                }
                if let Some(s) = x.first {
                    v["first"] = json!(s)
                }
                if !x.t.is_empty() {
                    let mut t = stats(&mut x.t);
                    t["boots"] = json!(x.t.len());
                    t["of"] = json!(x.of);
                    t["at"] = json!(x.at);
                    v["test"] = t
                }
                v
            })
            .collect(),
    )
}
#[no_mangle]
pub unsafe extern "C" fn lbrun_boot_history(path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    let p = Path::new(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
    read_bounded(p, true)
        .map(|b| export(&boot_history(&b).to_string()))
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boot_versions_first_boot_and_latest_test() {
        let v = boot_history(
            br#"{"version":"0.8.4-beta","seconds":30}
{"version":"0.8.4-beta","seconds":20}
{"version":"0.8.4-beta","seconds":22}
{"version":"0.8.5-beta","seconds":40}
{"version":"0.8.5-beta","seconds":18,"test":1,"testOf":3,"at":1}
{"version":"0.8.5-beta","seconds":20,"test":2,"testOf":3}
{"version":"0.8.5-beta","seconds":29,"test":1,"testOf":4,"at":12}
{"version":"0.8.5-beta","seconds":33,"test":2,"testOf":4}
{"version":"0.8.5-beta","seconds":99,"test":4,"testOf":4}
{"version":"0.8.5\"evil","seconds":40}
"#,
        );
        assert_eq!(v[0]["first"], 30.0);
        assert_eq!(v[0]["median"], 21.0);
        assert_eq!(v[1]["test"]["boots"], 2);
        assert_eq!(v[1]["test"]["median"], 31.0);
        assert_eq!(v[1]["test"]["at"], 12);
        assert_eq!(v.as_array().unwrap().len(), 2);
    }
    #[test]
    fn packet_size_and_protocol_are_checked() {
        unsafe {
            let mut sv = [0; 2];
            assert_eq!(
                libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, sv.as_mut_ptr()),
                0
            );
            let a = OwnedFd::from_raw_fd(sv[0]);
            let b = OwnedFd::from_raw_fd(sv[1]);
            let m = LbMsg {
                magic: LB_MAGIC,
                kind: LB_MSG_DONE as u32,
                ..LbMsg::default()
            };
            libc::send(
                b.as_raw_fd(),
                (&m as *const LbMsg).cast(),
                std::mem::size_of::<LbMsg>(),
                0,
            );
            assert!(matches!(receive(a.as_raw_fd()), Record::Message(_)));
            let bad = [0u8; 33];
            libc::send(b.as_raw_fd(), bad.as_ptr().cast(), bad.len(), 0);
            assert!(matches!(receive(a.as_raw_fd()), Record::Error("protocol")));
        }
    }
    #[test]
    fn rejects_invalid_start_without_changing_state() {
        unsafe {
            assert_eq!(lbrun_start(c"invalid".as_ptr(), 0, std::ptr::null()), -1);
            assert_eq!(lbrun_start(c"quick".as_ptr(), 19, std::ptr::null()), -1);
            assert_eq!(
                lbrun_start(c"quick".as_ptr(), 0, c"{} garbage".as_ptr()),
                -1
            );
        }
    }
}
