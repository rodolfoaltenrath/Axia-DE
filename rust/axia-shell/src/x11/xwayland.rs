//! Lazy XWayland process ownership. UI/Wayland thread only; stop before destroying
//! the display. All host calls run without a state borrow. The post-fork child
//! uses only libc and the existing scope entry point, then execve/_exit.
use crate::wayland::ffi::*;
use libc::{c_char, c_int, c_long, c_void, pid_t};
use std::{
    cell::{RefCell, UnsafeCell},
    ffi::CStr,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    ptr,
};

const BINARY: &[u8] = b"/usr/bin/Xwayland\0";
const SOCKET_DIR: &[u8] = b"/tmp/.X11-unix\0";
const EMPTY: &[u8] = b"\0";
fn cp(p: &[u8]) -> *const c_char {
    p.as_ptr().cast()
}
fn fixed<const N: usize>(parts: &[&[u8]]) -> [u8; N] {
    let mut out = [0; N];
    let mut at = 0;
    for part in parts {
        let n = part.len().min(N - 1 - at);
        out[at..at + n].copy_from_slice(&part[..n]);
        at += n;
    }
    out
}
fn paths(n: i32) -> ([u8; 64], [u8; 64]) {
    (
        fixed(&[format!("/tmp/.X{n}-lock").as_bytes()]),
        fixed(&[format!("/tmp/.X11-unix/X{n}").as_bytes()]),
    )
}
fn stat(path: &[u8], follow: bool) -> Option<libc::stat> {
    let mut s = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        if follow {
            libc::stat(cp(path), &mut s)
        } else {
            libc::lstat(cp(path), &mut s)
        }
    };
    (ok == 0).then_some(s)
}
fn kind(s: &libc::stat, kind: libc::mode_t) -> bool {
    s.st_mode & libc::S_IFMT == kind
}
unsafe fn owned(fd: c_int) -> Option<OwnedFd> {
    (fd >= 0).then(|| OwnedFd::from_raw_fd(fd))
}
fn now() -> i64 {
    unsafe { libc::time(ptr::null_mut()) as i64 }
}
fn display_taken(n: i32) -> bool {
    let (lock, socket) = paths(n);
    if let Some(s) = stat(&lock, false) {
        let mut bytes = [0u8; 16];
        if let Some(fd) = unsafe {
            owned(libc::open(
                cp(&lock),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            ))
        } {
            if unsafe { libc::read(fd.as_raw_fd(), bytes.as_mut_ptr().cast(), 15) } < 0 {
                bytes[0] = 0;
            }
        }
        let pid = unsafe { libc::strtol(cp(&bytes), ptr::null_mut(), 10) };
        if pid > 0
            && (unsafe { libc::kill(pid as pid_t, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
        {
            return true;
        }
        if s.st_uid != unsafe { libc::getuid() } || !kind(&s, libc::S_IFREG) {
            return true;
        }
        unsafe {
            libc::unlink(cp(&lock));
        }
        if let Some(s) = stat(&socket, false) {
            if s.st_uid == unsafe { libc::getuid() } && kind(&s, libc::S_IFSOCK) {
                unsafe {
                    libc::unlink(cp(&socket));
                }
            }
        }
    }
    stat(&socket, false).is_some()
}

#[derive(Default)]
struct RestartBudget {
    quick_fails: i32,
    disabled_until: i64,
}
impl RestartBudget {
    fn exited(&mut self, started: i64, status: c_int, time: i64) -> bool {
        let quick = time.wrapping_sub(started) < 4
            && !(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
        self.quick_fails = if quick { self.quick_fails + 1 } else { 0 };
        if self.quick_fails >= 3 {
            self.disabled_until = time + 60;
            self.quick_fails = 0;
            true
        } else {
            false
        }
    }
}
struct Source(*mut EventSource);
impl Drop for Source {
    fn drop(&mut self) {
        unsafe {
            wl_event_source_remove(self.0);
        }
    }
}
fn source(p: *mut EventSource) -> Option<Source> {
    (!p.is_null()).then(|| Source(p))
}

struct State {
    listen: Option<OwnedFd>,
    socket: [u8; 64],
    lock: [u8; 64],
    number: i32,
    event_loop: *mut EventLoop,
    listen_source: Option<Source>,
    pid: pid_t,
    pidfd: Option<OwnedFd>,
    pid_source: Option<Source>,
    client: *mut Client,
    started: i64,
    budget: RestartBudget,
}
impl Default for State {
    fn default() -> Self {
        Self {
            listen: None,
            socket: [0; 64],
            lock: [0; 64],
            number: -1,
            event_loop: ptr::null_mut(),
            listen_source: None,
            pid: 0,
            pidfd: None,
            pid_source: None,
            client: ptr::null_mut(),
            started: 0,
            budget: RestartBudget::default(),
        }
    }
}
struct Names {
    display: UnsafeCell<[u8; 16]>,
    socket: UnsafeCell<[u8; 64]>,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    // Separate from movable process state: libwayland retains this address.
    static GONE: Listener = Listener { link: UnsafeCell::new(List { prev: ptr::null_mut(), next: ptr::null_mut() }), notify: Some(client_gone) };
    // Borrowed C strings must survive unrelated mutable process-state accesses.
    static NAMES: Names = const { Names { display: UnsafeCell::new([0;16]), socket: UnsafeCell::new([0;64]) } };
}
fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}
fn listener() -> *mut Listener {
    GONE.with(|l| (l as *const Listener).cast_mut())
}

#[repr(C)]
struct CgroupLimits {
    mem_high_mb: c_long,
    mem_max_mb: c_long,
    cpu_percent: c_int,
}
extern "C" {
    fn wl__display() -> *mut Display;
    fn wlx_set_client(client: *mut Client);
    fn xwm_start(fd: c_int, event_loop: *mut EventLoop) -> c_int;
    fn xwm_stop();
    fn cgroup_enter(name: *const c_char, limits: *const CgroupLimits) -> c_int;
}

fn drain(fd: c_int) {
    for _ in 0..64 {
        let fd = unsafe {
            libc::accept4(
                fd,
                ptr::null_mut(),
                ptr::null_mut(),
                libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            )
        };
        if fd < 0 {
            break;
        }
        drop(unsafe { OwnedFd::from_raw_fd(fd) });
    }
}
fn watch_socket(on: bool) {
    if !on {
        drop(state(|s| s.listen_source.take()));
        return;
    }
    let target = state(|s| {
        if s.listen_source.is_none() {
            s.listen.as_ref().map(|fd| (fd.as_raw_fd(), s.event_loop))
        } else {
            None
        }
    });
    if let Some((fd, event_loop)) = target {
        let source = source(unsafe {
            wl_event_loop_add_fd(event_loop, fd, 1, Some(on_connect), ptr::null_mut())
        });
        state(|s| s.listen_source = source);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xwl_init() -> c_int {
    let display = wl__display();
    if display.is_null() || state(|s| s.listen.is_some()) {
        return -1;
    }
    if !stat(BINARY, true).is_some_and(|s| kind(&s, libc::S_IFREG)) {
        eprintln!("xwayland: not installed (the xwayland package): X11 apps can't open");
        return -1;
    }
    state(|s| s.event_loop = wl_display_get_event_loop(display));
    match stat(SOCKET_DIR, false) {
        None => {
            if libc::mkdir(cp(SOCKET_DIR), 0o1777) == 0 {
                libc::chmod(cp(SOCKET_DIR), 0o1777);
            }
        }
        Some(s) if !kind(&s, libc::S_IFDIR) || (s.st_uid != 0 && s.st_uid != libc::getuid()) => {
            eprintln!("xwayland: /tmp/.X11-unix isn't a directory of root's: no X11 apps");
            return -1;
        }
        _ => {}
    }
    for _ in 0..8 {
        let Some(n) = (0..64).find(|&n| !display_taken(n)) else {
            break;
        };
        let (lock, socket) = paths(n);
        state(|s| {
            s.lock = lock;
            s.socket = socket;
        });
        let Some(lf) = owned(libc::open(
            cp(&lock),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o444,
        )) else {
            continue;
        };
        let pid = format!("{:10}\n", libc::getpid());
        if libc::write(lf.as_raw_fd(), pid.as_ptr().cast(), pid.len()) != pid.len() as isize {
            drop(lf);
            libc::unlink(cp(&lock));
            continue;
        }
        drop(lf);
        let fd = owned(libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        ));
        let mut address: libc::sockaddr_un = std::mem::zeroed();
        address.sun_family = libc::AF_UNIX as _;
        for (dest, &src) in address.sun_path.iter_mut().zip(&socket) {
            *dest = src as c_char;
        }
        let old = libc::umask(0o177);
        let mut ok = fd.as_ref().is_some_and(|fd| {
            libc::bind(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                std::mem::size_of_val(&address) as _,
            ) == 0
        });
        libc::umask(old);
        if ok {
            ok = libc::chmod(cp(&socket), 0o600) == 0
                && libc::listen(fd.as_ref().unwrap().as_raw_fd(), 64) == 0;
        }
        if !ok {
            drop(fd);
            libc::unlink(cp(&lock));
            continue;
        }
        state(|s| {
            s.listen = fd;
            s.number = n;
        });
        break;
    }
    let number = state(|s| s.listen.as_ref().map(|_| s.number));
    let Some(number) = number else {
        eprintln!("xwayland: no free X display");
        return -1;
    };
    let name: [u8; 16] = fixed(&[format!(":{number}").as_bytes()]);
    let socket = state(|s| s.socket);
    NAMES.with(|names| {
        names.display.get().write(name);
        names.socket.get().write(socket);
    });
    libc::setenv(cp(b"DISPLAY\0"), cp(&name), 1);
    libc::setenv(cp(b"_JAVA_AWT_WM_NONREPARENTING\0"), cp(b"1\0"), 1);
    watch_socket(true);
    eprintln!(
        "xwayland: DISPLAY={} ({}), started on the first X11 connection",
        CStr::from_ptr(cp(&name)).to_string_lossy(),
        CStr::from_ptr(xwl_socket_path()).to_string_lossy()
    );
    0
}

unsafe fn child_exited() {
    let pid = state(|s| s.pid);
    let mut status = 0;
    if pid > 0 && libc::waitpid(pid, &mut status, libc::WNOHANG) <= 0 {
        return;
    }
    let (event, fd, started) = state(|s| (s.pid_source.take(), s.pidfd.take(), s.started));
    drop(event);
    drop(fd);
    eprintln!(
        "xwayland: exited ({} {})",
        if libc::WIFSIGNALED(status) {
            "signal"
        } else {
            "status"
        },
        if libc::WIFSIGNALED(status) {
            libc::WTERMSIG(status)
        } else {
            libc::WEXITSTATUS(status)
        }
    );
    state(|s| s.pid = 0);
    xwm_stop();
    let client = state(|s| {
        let c = s.client;
        s.client = ptr::null_mut();
        c
    });
    if !client.is_null() {
        wlx_set_client(ptr::null_mut());
        wl_list_remove((*listener()).link.get());
        wl_client_destroy(client);
    }
    let time = now();
    if state(|s| s.budget.exited(started, status, time)) {
        eprintln!("xwayland: it failed to start three times; X11 apps are refused for a minute");
    }
    watch_socket(true);
}
unsafe extern "C" fn on_pidfd(_: c_int, _: u32, _: *mut c_void) -> c_int {
    child_exited();
    0
}
unsafe extern "C" fn client_gone(l: *mut Listener, _: *mut c_void) {
    state(|s| s.client = ptr::null_mut());
    wlx_set_client(ptr::null_mut());
    let link = (*l).link.get();
    // Detach before resetting; the listener's storage stays alive for the whole UI thread.
    wl_list_remove(link);
    (*link).prev = link;
    (*link).next = link;
    xwm_stop();
    let pid = state(|s| s.pid);
    if pid > 0 {
        libc::kill(pid, libc::SIGTERM);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xwl_wm_lost() {
    let pid = state(|s| s.pid);
    if pid > 0 {
        libc::kill(pid, libc::SIGTERM);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xwl_idle() {
    let pid = state(|s| s.pid);
    if pid <= 0 {
        return;
    }
    eprintln!("xwayland: no X11 app for 10 s: stopping it (the next one starts it again)");
    state(|s| s.started = 0);
    libc::kill(pid, libc::SIGTERM);
}

fn binary_ok(s: &libc::stat) -> bool {
    kind(s, libc::S_IFREG) && s.st_uid == 0 && s.st_mode & 0o22 == 0 && s.st_mode & 0o111 != 0
}
fn open_log() -> Option<OwnedFd> {
    let home = unsafe { libc::getenv(cp(b"HOME\0")) };
    if home.is_null() {
        return None;
    }
    let path: [u8; 512] = fixed(&[
        unsafe { CStr::from_ptr(home).to_bytes() },
        b"/.cache/axia/xwayland.log",
    ]);
    let trunc = stat(&path, true).is_some_and(|s| s.st_size > 1 << 20);
    unsafe {
        owned(libc::open(
            cp(&path),
            libc::O_WRONLY
                | libc::O_CREAT
                | libc::O_APPEND
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | if trunc { libc::O_TRUNC } else { 0 },
            0o600,
        ))
    }
}
fn socket_pair() -> Option<[OwnedFd; 2]> {
    let mut fds = [-1; 2];
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    } != 0
    {
        for fd in fds {
            drop(unsafe { owned(fd) });
        }
        return None;
    }
    Some(unsafe { [OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])] })
}
fn env_value(name: &[u8], fallback: &[u8]) -> Vec<u8> {
    let p = unsafe { libc::getenv(cp(name)) };
    if p.is_null() {
        fallback.to_vec()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}
struct ChildPlan {
    display: [u8; 16],
    home: [u8; 600],
    runtime: [u8; 600],
    managed: bool,
    uid: [u8; 64],
    uid_len: usize,
    gid: [u8; 64],
    gid_len: usize,
}
impl ChildPlan {
    fn new(number: i32) -> Self {
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };
        let uid = format!("{uid} {uid} 1");
        let gid = format!("{gid} {gid} 1");
        Self {
            display: fixed(&[format!(":{number}").as_bytes()]),
            home: fixed(&[b"HOME=", &env_value(b"HOME\0", b"/")]),
            runtime: fixed(&[
                b"XDG_RUNTIME_DIR=",
                &env_value(b"XDG_RUNTIME_DIR\0", b"/tmp"),
            ]),
            managed: unsafe { !libc::getenv(cp(b"LORDOS_SESSIOND\0")).is_null() },
            uid: fixed(&[uid.as_bytes()]),
            uid_len: uid.len(),
            gid: fixed(&[gid.as_bytes()]),
            gid_len: gid.len(),
        }
    }
    unsafe fn exec(&self, listen: c_int, wayland: c_int, wm: c_int, log: c_int) -> ! {
        let argv = [
            cp(b"Xwayland\0"),
            cp(&self.display),
            cp(b"-rootless\0"),
            cp(b"-listenfd\0"),
            cp(b"3\0"),
            cp(b"-wm\0"),
            cp(b"5\0"),
            cp(b"-nolisten\0"),
            cp(b"tcp\0"),
            cp(b"-extension\0"),
            cp(b"XTEST\0"),
            ptr::null(),
        ];
        let env = [
            cp(&self.home),
            cp(&self.runtime),
            cp(b"PATH=/usr/bin:/bin\0"),
            cp(b"WAYLAND_SOCKET=4\0"),
            cp(b"LANG=C.UTF-8\0"),
            ptr::null(),
        ];
        libc::setsid();
        let mut none: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut none);
        libc::sigprocmask(libc::SIG_SETMASK, &none, ptr::null_mut());
        for s in 1..32 {
            libc::signal(s, libc::SIG_DFL);
        }
        let a = libc::fcntl(listen, libc::F_DUPFD_CLOEXEC, 100);
        let b = libc::fcntl(wayland, libc::F_DUPFD_CLOEXEC, 100);
        let c = libc::fcntl(wm, libc::F_DUPFD_CLOEXEC, 100);
        let null = libc::open(cp(b"/dev/null\0"), libc::O_RDWR | libc::O_CLOEXEC);
        if a < 0 || b < 0 || c < 0 || null < 0 {
            libc::_exit(126);
        }
        libc::dup2(null, 0);
        libc::dup2(if log >= 0 { log } else { null }, 1);
        libc::dup2(if log >= 0 { log } else { null }, 2);
        if libc::dup2(a, 3) != 3 || libc::dup2(b, 4) != 4 || libc::dup2(c, 5) != 5 {
            libc::_exit(126);
        }
        libc::syscall(libc::SYS_close_range, 6u32, u32::MAX, 0u32);
        let limits = CgroupLimits {
            mem_high_mb: 0,
            mem_max_mb: 0,
            cpu_percent: 0,
        };
        if cgroup_enter(cp(b"lordos-xwayland.scope\0"), &limits) != 0 && self.managed {
            let message = b"xwayland: could not enter lordos-xwayland.scope: not started\n";
            libc::write(2, message.as_ptr().cast(), message.len());
            libc::_exit(126);
        }
        libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0);
        if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) == 0 {
            write_map(b"/proc/self/setgroups\0", b"deny");
            write_map(b"/proc/self/uid_map\0", &self.uid[..self.uid_len]);
            write_map(b"/proc/self/gid_map\0", &self.gid[..self.gid_len]);
        }
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        libc::execve(cp(BINARY), argv.as_ptr(), env.as_ptr());
        libc::_exit(127);
    }
}
unsafe fn write_map(path: &[u8], value: &[u8]) {
    let fd = libc::open(cp(path), libc::O_WRONLY | libc::O_CLOEXEC);
    if fd >= 0 {
        libc::write(fd, value.as_ptr().cast(), value.len());
        libc::close(fd);
    }
}

unsafe fn spawn() {
    let (listen, number) = state(|s| (s.listen.as_ref().unwrap().as_raw_fd(), s.number));
    if !stat(BINARY, true).is_some_and(|s| binary_ok(&s)) {
        eprintln!("xwayland: /usr/bin/Xwayland is missing or not root's: not started");
        state(|s| s.budget.disabled_until = now() + 60);
        drain(listen);
        return;
    }
    let Some(wlp) = socket_pair() else {
        drain(listen);
        return;
    };
    let Some(wmp) = socket_pair() else {
        drop(wlp);
        drain(listen);
        return;
    };
    let log = open_log();
    let plan = ChildPlan::new(number);
    let pid = libc::fork();
    if pid == 0 {
        plan.exec(
            listen,
            wlp[1].as_raw_fd(),
            wmp[1].as_raw_fd(),
            log.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        );
    }
    let [wl, wl_child] = wlp;
    let [wm, wm_child] = wmp;
    drop(wl_child);
    drop(wm_child);
    drop(log);
    if pid < 0 {
        drop(wl);
        drop(wm);
        drain(listen);
        return;
    }
    state(|s| {
        s.pid = pid;
        s.started = now();
    });
    watch_socket(false);
    let client = wl_client_create(wl__display(), wl.as_raw_fd());
    state(|s| s.client = client);
    if client.is_null() {
        drop(wl);
        drop(wm);
        libc::kill(pid, libc::SIGTERM);
    } else {
        let _ = wl.into_raw_fd();
        wl_client_add_destroy_listener(client, listener());
        wlx_set_client(client);
        let event_loop = state(|s| s.event_loop);
        if xwm_start(wm.into_raw_fd(), event_loop) != 0 {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    let pidfd = owned(libc::syscall(libc::SYS_pidfd_open, pid, 0u32) as c_int);
    if let Some(fd) = pidfd {
        let event_loop = state(|s| s.event_loop);
        let source = source(wl_event_loop_add_fd(
            event_loop,
            fd.as_raw_fd(),
            1,
            Some(on_pidfd),
            ptr::null_mut(),
        ));
        state(|s| {
            s.pidfd = Some(fd);
            s.pid_source = source;
        });
    }
    eprintln!("xwayland: started (pid {pid}) for an X11 app on :{number}");
}
unsafe extern "C" fn on_connect(fd: c_int, _: u32, _: *mut c_void) -> c_int {
    if state(|s| s.pid > 0) {
        watch_socket(false);
        return 0;
    }
    if state(|s| now() < s.budget.disabled_until) {
        drain(fd);
        return 0;
    }
    spawn();
    0
}

#[no_mangle]
pub unsafe extern "C" fn xwl_stop() {
    watch_socket(false);
    let pid = state(|s| s.pid);
    if pid > 0 {
        libc::kill(pid, libc::SIGTERM);
        for _ in 0..20 {
            if libc::waitpid(pid, ptr::null_mut(), libc::WNOHANG) != 0 {
                break;
            }
            libc::usleep(10000);
        }
        state(|s| s.pid = 0);
    }
    let (event, fd) = state(|s| (s.pid_source.take(), s.pidfd.take()));
    drop(event);
    drop(fd);
    xwm_stop();
    let reserved = state(|s| s.listen.take().map(|fd| (fd, s.socket, s.lock)));
    if let Some((fd, socket, lock)) = reserved {
        drop(fd);
        libc::unlink(cp(&socket));
        libc::unlink(cp(&lock));
    }
    NAMES.with(|names| (*names.display.get())[0] = 0);
}
#[no_mangle]
pub extern "C" fn xwl_display() -> *const c_char {
    NAMES.with(|names| names.display.get().cast())
}
#[no_mangle]
pub extern "C" fn xwl_socket_path() -> *const c_char {
    if state(|s| s.listen.is_some()) {
        NAMES.with(|names| names.socket.get().cast())
    } else {
        cp(EMPTY)
    }
}
#[no_mangle]
pub extern "C" fn xwl_running() -> c_int {
    state(|s| i32::from(s.pid > 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_names_survive_unrelated_process_state_access() {
        NAMES.with(|names| unsafe {
            names.display.get().write(fixed(&[b":3"]));
        });
        let name = xwl_display();
        state(|s| s.budget.disabled_until = 123);
        assert_eq!(xwl_running(), 0);
        assert_eq!(unsafe { CStr::from_ptr(name).to_bytes() }, b":3");
        NAMES.with(|names| unsafe {
            names.display.get().write([0; 16]);
        });
    }
    #[test]
    fn restart_budget_preserves_quick_failure_and_idle_policy() {
        let mut budget = RestartBudget::default();
        assert!(!budget.exited(100, 1 << 8, 101));
        assert!(!budget.exited(101, libc::SIGTERM, 102));
        assert!(budget.exited(102, 127 << 8, 103));
        assert_eq!(budget.disabled_until, 163);
        assert!(!budget.exited(110, 1 << 8, 114));
        assert_eq!(budget.quick_fails, 0);
        assert!(!budget.exited(120, 1 << 8, 121));
        assert!(!budget.exited(121, 0, 122));
        assert_eq!(budget.quick_fails, 0);
        assert!(!budget.exited(0, libc::SIGTERM, 123));
        assert_eq!(budget.quick_fails, 0);
    }
    #[test]
    fn fixed_strings_and_binary_policy() {
        assert_eq!(fixed::<5>(&[b"abc", b"def"]), *b"abcd\0");
        assert_eq!(&paths(63).0[..15], b"/tmp/.X63-lock\0");
        let mut s: libc::stat = unsafe { std::mem::zeroed() };
        s.st_mode = libc::S_IFREG | 0o755;
        assert!(binary_ok(&s));
        s.st_mode |= 0o020;
        assert!(!binary_ok(&s));
        s.st_mode = libc::S_IFREG | 0o644;
        assert!(!binary_ok(&s));
        s.st_mode = libc::S_IFLNK | 0o755;
        assert!(!binary_ok(&s));
        s.st_mode = libc::S_IFREG | 0o755;
        s.st_uid = 1000;
        assert!(!binary_ok(&s));
    }
}
