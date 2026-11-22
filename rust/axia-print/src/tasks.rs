//! The requests of `lord.print`, each in its own thread: the compositor never waits. `task()`
//! returns an id at once and the result reaches JS with `poll()` as `[{id, kind, ok, data}]`.
use crate::{cups, discover, escl, jnum, jstr, Fail, Reply};
use serde_json::{json, Value};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_RUNNING: i32 = 8;

struct State {
    /// The finished ones, as JSON objects separated by commas.
    events: String,
    next_id: i32,
    running: i32,
}

static STATE: Mutex<State> = Mutex::new(State { events: String::new(), next_id: 1, running: 0 });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The PDF to print: its descriptor (the task closes it) and its length.
pub struct DocFd {
    pub fd: OwnedFd,
    pub len: usize,
}

/// Starts a request: kind = printers | jobs | cancel | submit | add | remove | default | discover |
/// scancaps | scan; `json` = its arguments. The id (> 0), or `None` if there are too many running
/// or the thread did not start.
pub fn task(kind: &str, json: Option<Vec<u8>>, doc: Option<DocFd>) -> Option<i32> {
    if kind.len() >= 16 {
        return None;
    }
    let id = {
        let mut s = state();
        if s.running >= MAX_RUNNING {
            return None;
        }
        s.running += 1;
        let id = s.next_id;
        s.next_id += 1;
        id
    };
    let kind = kind.to_string();
    // the thread takes no signals: they are for the compositor's own threads
    let (mut all, mut old): (libc::sigset_t, libc::sigset_t) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    // SAFETY: plain libc calls on local sigsets.
    unsafe {
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_BLOCK, &all, &mut old);
    }
    let spawned = std::thread::Builder::new().name("print".into()).stack_size(1 << 20).spawn(move || {
        let socket = cups::socket_path();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&kind, json.as_deref(), doc.as_ref(), &socket)));
        let (ok, data) = match result {
            Ok(Ok(v)) => (true, v),
            Ok(Err(f)) => (false, f.0),
            Err(_) => (false, Fail::new("internal", "").0),
        };
        finish(id, &kind, ok, &data);
    });
    // SAFETY: restores the mask saved above.
    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
    if spawned.is_err() {
        state().running -= 1;
        return None;
    }
    Some(id)
}

fn finish(id: i32, kind: &str, ok: bool, data: &Value) {
    let ev = json!({ "id": id, "kind": kind, "ok": ok, "data": data });
    let mut s = state();
    if !s.events.is_empty() {
        s.events.push(',');
    }
    s.events.push_str(&ev.to_string());
    s.running -= 1;
}

/// `[{id, kind, ok, data}]` with what finished since the last call, or `None`.
pub fn poll() -> Option<String> {
    let mut s = state();
    if s.events.is_empty() {
        return None;
    }
    let out = format!("[{}]", s.events);
    s.events.clear();
    s.events.shrink_to(0);
    Some(out)
}

/// How many are running.
pub fn pending() -> i32 {
    state().running
}

/// ~/.cache/axia/scan: where scans stay until they're saved.
fn scan_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return None;
    }
    let mut p = home;
    for part in [".cache", "axia", "scan"] {
        p.push(part);
        match std::fs::DirBuilder::new().mode_0700().create(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    Some(p)
}

trait Mode0700 {
    fn mode_0700(&mut self) -> &mut Self;
}
impl Mode0700 for std::fs::DirBuilder {
    fn mode_0700(&mut self) -> &mut Self {
        std::os::unix::fs::DirBuilderExt::mode(self, 0o700)
    }
}

fn run(kind: &str, json: Option<&[u8]>, doc: Option<&DocFd>, socket: &str) -> Reply {
    let a: Value = match json.filter(|j| !j.is_empty()) {
        Some(j) => serde_json::from_slice(j).map_err(|_| Fail(json!({ "error": "bad-request" })))?,
        None => Value::Null,
    };
    let addr = jstr(&a, "addr").unwrap_or("");
    let port = jnum(&a, "port", 80.0) as i32;
    let rs = jstr(&a, "rs").unwrap_or("eSCL");
    match kind {
        "printers" => cups::printers(socket),
        "jobs" => cups::jobs(socket),
        "cancel" => cups::cancel(socket, jnum(&a, "job", 0.0) as i32),
        "submit" => {
            let doc = doc.map(|d| cups::Doc { fd: &d.fd, len: d.len });
            cups::submit(socket, jstr(&a, "printer"), doc.as_ref(), a.get("options").unwrap_or(&Value::Null))
        }
        "add" => cups::add(socket, jstr(&a, "name"), jstr(&a, "uri"), jstr(&a, "info"), jstr(&a, "location")),
        "remove" => cups::remove(socket, jstr(&a, "name")),
        "default" => cups::set_default(socket, jstr(&a, "name")),
        "discover" => Ok(discover::discover()),
        "scancaps" => escl::caps(addr, port, rs),
        "scan" => {
            let Some(dir) = scan_dir() else { return Err(Fail(json!({ "error": "write" }))) };
            escl::scan(addr, port, rs, a.get("settings").unwrap_or(&Value::Null), &dir)
        }
        _ => Err(Fail(json!({ "error": "unknown" }))),
    }
}

/// Runs one request in the calling thread (the tests of the operations use it too).
pub fn run_now(kind: &str, json: Option<&[u8]>, doc: Option<&DocFd>) -> Reply {
    run(kind, json, doc, &cups::socket_path())
}
