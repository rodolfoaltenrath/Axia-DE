//! Opt-in, bounded timing trace. Never accepts key codes, text or modifiers.
//! Only a key explicitly marked traceable by Wayland receives a sequence number.
use std::{
    collections::VecDeque,
    ffi::{c_int, OsStr},
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

const MAX_KEYS: usize = 32;
const GIVE_UP_MS: f64 = 2000.0;
const WINDOW_MS: f64 = 120000.0;
const MAX_LINES: u32 = 4000;

#[derive(Clone, Copy, Default)]
struct Key {
    sequence: u32,
    press: bool,
    event: f64,
    received: f64,
    sent: f64,
    flushed: f64,
    committed: f64,
    rendered: f64,
}
#[derive(Default)]
struct Trace {
    enabled: bool,
    keys: VecDeque<Key>,
    pending: Option<Key>,
    sequence: u32,
    lines: u32,
    started: f64,
    output: Option<File>,
}

fn allowed(greeter: c_int, env: Option<&OsStr>) -> bool {
    greeter == 0 && env.is_some_and(|value| value.as_bytes() == b"1")
}
fn relative(time: f64, event: f64) -> f64 {
    if time > 0.0 {
        time - event
    } else {
        -1.0
    }
}
fn clock_ms(clock: libc::clockid_t) -> f64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(clock, &mut time);
    }
    time.tv_sec as f64 * 1e3 + time.tv_nsec as f64 / 1e6
}
fn mono_ms() -> f64 {
    clock_ms(libc::CLOCK_MONOTONIC)
}

impl Trace {
    fn stop(&mut self, reason: &str) {
        self.enabled = false;
        self.keys.clear();
        self.pending = None;
        if let Some(mut output) = self.output.take() {
            let _ = writeln!(output, "keylag stop: {reason}");
        }
        eprintln!("keylag: trace stopped ({reason})");
    }

    fn open_output(&mut self, runtime: Option<&OsStr>) -> io::Result<()> {
        if self.output.is_some() {
            return Ok(());
        }
        let runtime = runtime
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?
            .as_bytes();
        let suffix = b"/axia-keylag.log";
        if runtime.first() != Some(&b'/') || runtime.len() + suffix.len() >= 512 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let mut bytes = runtime.to_vec();
        bytes.extend_from_slice(suffix);
        let path = PathBuf::from(OsStr::from_bytes(&bytes));
        // Remove only the old directory entry; create_new + NOFOLLOW prevents
        // following a final symlink introduced between unlink and creation.
        let _ = std::fs::remove_file(&path);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let _ = writeln!(
            output,
            "keylag start: times in ms after the kernel's evdev timestamp"
        );
        self.output = Some(output);
        Ok(())
    }

    fn print(&mut self, key: Key, shown: f64) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let line = format!(
            "keylag n={} ev={:.3} {} rx={:.2} sent={:.2} flush={:.2} commit={:.2} shown={:.2}\n",
            key.sequence,
            key.event,
            if key.press { "press" } else { "release" },
            relative(key.received, key.event),
            relative(key.sent, key.event),
            relative(key.flushed, key.event),
            relative(key.committed, key.event),
            relative(shown, key.event)
        );
        let _ = output.write_all(line.as_bytes());
        self.lines += 1;
        if self.lines >= MAX_LINES {
            self.stop("line limit");
        }
    }

    fn expire(&mut self, now: f64) {
        while self.enabled
            && self
                .keys
                .front()
                .is_some_and(|key| now - key.event > GIVE_UP_MS)
        {
            let key = *self.keys.front().unwrap();
            self.print(key, 0.0);
            if self.enabled {
                self.keys.pop_front();
            }
        }
        if self.enabled && self.started != 0.0 && now - self.started > WINDOW_MS {
            self.stop("120 s window over");
        }
    }

    fn receive(&mut self, value: c_int, event: &libc::timeval, now: f64, real: f64) {
        self.pending = None;
        if !self.enabled || value == 2 {
            return;
        }
        let event_ms = event.tv_sec as f64 * 1e3 + event.tv_usec as f64 / 1e3;
        self.pending = Some(Key {
            press: value == 1,
            event: now - (real - event_ms),
            received: now,
            ..Key::default()
        });
    }

    fn sent(&mut self, traceable: bool, mut now: impl FnMut() -> f64, runtime: Option<&OsStr>) {
        if !self.enabled {
            return;
        }
        let Some(mut key) = self.pending.take() else {
            return;
        };
        if !traceable {
            return;
        }
        let now = now();
        self.expire(now);
        if !self.enabled {
            return;
        }
        if self.open_output(runtime).is_err() {
            self.stop("cannot create the trace file");
            return;
        }
        if self.started == 0.0 {
            self.started = now;
        }
        if self.keys.len() == MAX_KEYS {
            self.print(*self.keys.front().unwrap(), 0.0);
            if !self.enabled {
                return;
            }
            self.keys.pop_front();
        }
        self.sequence = self.sequence.wrapping_add(1);
        key.sequence = self.sequence;
        key.sent = now;
        self.keys.push_back(key);
    }

    fn stamp(&mut self, stage: Stage, mut now: impl FnMut() -> f64) {
        if !self.enabled {
            return;
        }
        let mut time = None;
        for key in &mut self.keys {
            let field = match stage {
                Stage::Flush if key.flushed == 0.0 => &mut key.flushed,
                Stage::Commit if key.flushed != 0.0 && key.committed == 0.0 => &mut key.committed,
                Stage::Render if key.committed != 0.0 && key.rendered == 0.0 => &mut key.rendered,
                _ => continue,
            };
            *field = *time.get_or_insert_with(&mut now);
        }
    }

    fn presented(&mut self, flip_ns: u64, mut now: impl FnMut() -> f64) {
        if !self.enabled {
            return;
        }
        let shown = if flip_ns != 0 {
            flip_ns as f64 / 1e6
        } else {
            now()
        };
        let count = self.keys.len();
        for _ in 0..count {
            let key = self.keys.pop_front().unwrap();
            if key.rendered != 0.0 {
                self.print(key, shown);
            } else {
                self.keys.push_back(key);
            }
            if !self.enabled {
                return;
            }
        }
        self.expire(now());
    }
}
#[derive(Clone, Copy)]
enum Stage {
    Flush,
    Commit,
    Render,
}
static ENABLED: AtomicBool = AtomicBool::new(false);
static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();
fn trace() -> &'static Mutex<Trace> {
    TRACE.get_or_init(|| Mutex::new(Trace::default()))
}
fn with_trace(action: impl FnOnce(&mut Trace)) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut trace = trace().lock().unwrap();
    action(&mut trace);
    ENABLED.store(trace.enabled, Ordering::Relaxed);
}

/// Atomic query replacing the C-owned mutable global.
#[no_mangle]
pub extern "C" fn keylag_enabled() -> c_int {
    ENABLED.load(Ordering::Relaxed) as c_int
}
#[no_mangle]
pub extern "C" fn keylag_init(greeter: c_int) {
    let enabled = allowed(greeter, std::env::var_os("LORD_INPUT_DEBUG").as_deref());
    trace().lock().unwrap().enabled = enabled;
    ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        eprintln!("keylag: key latency trace on (LORD_INPUT_DEBUG=1), in $XDG_RUNTIME_DIR/axia-keylag.log");
    }
}
/// # Safety
/// When tracing is enabled and value is not autorepeat, `event` must point to a
/// readable timeval. It is never read for disabled tracing or autorepeat.
#[no_mangle]
pub unsafe extern "C" fn keylag_rx(value: c_int, event: *const libc::timeval) {
    with_trace(|trace| {
        trace.pending = None;
        if !trace.enabled || value == 2 {
            return;
        }
        trace.receive(value, &*event, mono_ms(), clock_ms(libc::CLOCK_REALTIME));
    });
}
#[no_mangle]
pub extern "C" fn keylag_sent_wl(traceable: c_int) {
    with_trace(|trace| {
        trace.sent(
            traceable != 0,
            mono_ms,
            std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        )
    });
}
#[no_mangle]
pub extern "C" fn keylag_handled() {
    // Also clear a pending event when called directly while disabled.
    trace().lock().unwrap().pending = None;
}
#[no_mangle]
pub extern "C" fn keylag_flushed() {
    with_trace(|trace| trace.stamp(Stage::Flush, mono_ms));
}
#[no_mangle]
pub extern "C" fn keylag_commit() {
    with_trace(|trace| trace.stamp(Stage::Commit, mono_ms));
}
#[no_mangle]
pub extern "C" fn keylag_render() {
    with_trace(|trace| trace.stamp(Stage::Render, mono_ms));
}
#[no_mangle]
pub extern "C" fn keylag_presented(flip_ns: u64) {
    with_trace(|trace| trace.presented(flip_ns, mono_ms));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::{fd::AsRawFd, unix::fs::PermissionsExt},
        sync::atomic::AtomicUsize,
    };

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "axia-keylatency-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn runtime(&self) -> Option<&OsStr> {
            Some(self.0.as_os_str())
        }
        fn log(&self) -> String {
            std::fs::read_to_string(self.0.join("axia-keylag.log")).unwrap()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn receive(trace: &mut Trace, value: i32) {
        trace.receive(
            value,
            &libc::timeval {
                tv_sec: 10,
                tv_usec: 0,
            },
            1000.0,
            10000.0,
        );
    }
    fn enabled() -> Trace {
        Trace {
            enabled: true,
            ..Trace::default()
        }
    }

    #[test]
    fn protected_handled_repeat_and_greeter_events_never_get_counted() {
        assert!(!allowed(1, Some(OsStr::new("1"))));
        for env in [
            None,
            Some(OsStr::new("")),
            Some(OsStr::new("0")),
            Some(OsStr::new("true")),
            Some(OsStr::new("1 ")),
        ] {
            assert!(!allowed(0, env));
        }
        assert!(allowed(0, Some(OsStr::new("1"))));
        let directory = Directory::new();
        let mut trace = enabled();
        receive(&mut trace, 1);
        trace.sent(
            false,
            || panic!("protected events must not query time"),
            directory.runtime(),
        );
        assert_eq!(trace.sequence, 0);
        assert!(trace.output.is_none());
        assert!(trace.keys.is_empty());
        receive(&mut trace, 1);
        trace.pending = None; // keylag_handled: the shell consumed this event.
        trace.sent(
            true,
            || panic!("handled events must not query time"),
            directory.runtime(),
        );
        receive(&mut trace, 2);
        trace.sent(
            true,
            || panic!("autorepeat must not query time"),
            directory.runtime(),
        );
        assert!(!directory.0.join("axia-keylag.log").exists());
        receive(&mut trace, 1);
        trace.sent(true, || 1001.0, directory.runtime());
        assert_eq!(trace.sequence, 1);
        assert_eq!(trace.keys.len(), 1);
    }

    #[test]
    fn timings_file_permissions_and_line_schema_match_the_legacy_trace() {
        let directory = Directory::new();
        let mut trace = enabled();
        receive(&mut trace, 1);
        trace.sent(true, || 1001.0, directory.runtime());
        let output = trace.output.as_ref().unwrap();
        assert_eq!(
            output.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_ne!(
            unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        trace.stamp(Stage::Commit, || panic!("cannot commit before flushing"));
        trace.stamp(Stage::Render, || panic!("cannot render before committing"));
        trace.stamp(Stage::Flush, || 1002.0);
        trace.stamp(Stage::Flush, || panic!("do not replace the first flush"));
        trace.stamp(Stage::Commit, || 1003.0);
        trace.stamp(Stage::Render, || 1004.0);
        trace.presented(1005000000, || 1005.0);
        assert_eq!(directory.log(), "keylag start: times in ms after the kernel's evdev timestamp\nkeylag n=1 ev=1000.000 press rx=0.00 sent=1.00 flush=2.00 commit=3.00 shown=5.00\n");
        assert!(trace.keys.is_empty());
    }

    #[test]
    fn queue_expiry_and_window_are_bounded_with_missing_stages() {
        let directory = Directory::new();
        let mut trace = enabled();
        for _ in 0..MAX_KEYS + 1 {
            receive(&mut trace, 0);
            trace.sent(true, || 1001.0, directory.runtime());
        }
        assert_eq!(trace.keys.len(), MAX_KEYS);
        assert_eq!(trace.lines, 1);
        let mut clock_calls = 0;
        trace.stamp(Stage::Flush, || {
            clock_calls += 1;
            1002.0
        });
        assert_eq!(clock_calls, 1);
        assert!(trace.keys.iter().all(|key| key.flushed == 1002.0));
        trace.expire(3000.0);
        assert_eq!(trace.keys.len(), MAX_KEYS);
        trace.expire(3000.1);
        assert!(trace.keys.is_empty());
        assert!(directory.log().contains("commit=-1.00 shown=-1.00"));
        trace.expire(121001.0);
        assert!(trace.enabled);
        trace.expire(121001.1);
        assert!(!trace.enabled);
        assert!(trace.output.is_none());
        assert!(directory
            .log()
            .ends_with("keylag stop: 120 s window over\n"));
    }

    #[test]
    fn line_limit_discards_all_remaining_state_and_closes_output() {
        let directory = Directory::new();
        let mut trace = enabled();
        trace.open_output(directory.runtime()).unwrap();
        receive(&mut trace, 1);
        trace.keys.push_back(Key::default());
        for index in 1..=MAX_LINES {
            trace.print(
                Key {
                    sequence: index,
                    ..Key::default()
                },
                0.0,
            );
        }
        assert!(!trace.enabled);
        assert!(trace.keys.is_empty());
        assert!(trace.pending.is_none());
        assert!(trace.output.is_none());
        let log = directory.log();
        assert_eq!(log.lines().count(), MAX_LINES as usize + 2);
        assert!(log.ends_with("keylag stop: line limit\n"));
    }

    #[test]
    fn stale_symlink_is_replaced_without_touching_its_target_and_bad_paths_fail() {
        let directory = Directory::new();
        let target = directory.0.join("untouched");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, directory.0.join("axia-keylag.log")).unwrap();
        let mut trace = enabled();
        trace.open_output(directory.runtime()).unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
        assert!(
            !std::fs::symlink_metadata(directory.0.join("axia-keylag.log"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let mut bad = enabled();
        assert!(bad.open_output(None).is_err());
        assert!(bad.open_output(Some(OsStr::new("relative"))).is_err());
        assert!(bad
            .open_output(Some(OsStr::new(&format!("/{}", "x".repeat(512)))))
            .is_err());
        receive(&mut bad, 1);
        bad.sent(
            true,
            || 1001.0,
            Some(directory.0.join("missing").as_os_str()),
        );
        assert!(!bad.enabled);
        assert!(bad.pending.is_none());
    }
}
