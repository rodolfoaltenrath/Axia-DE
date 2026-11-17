//! A single producer publishes 125 Hz pointer samples through a bounded ring.
//! The eventfd and absolute monotonic deadlines preserve device-like wakeups.
use libc::c_int;
use std::{
    cell::UnsafeCell,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

pub const LBI_HZ: u64 = 125;
const RING: usize = 256;
#[derive(Clone, Copy, Default, Debug)]
struct Event {
    ns: u64,
    dx: f32,
    dy: f32,
}
struct Ring {
    events: [UnsafeCell<Event>; RING],
    head: AtomicU32,
    tail: AtomicU32,
}
// Only the unique Producer writes slots; only the unique Consumer reads them.
// Release/acquire head publishes initialized slots; tail releases consumed slots.
// Each endpoint requires &mut self and cannot be cloned.
unsafe impl Sync for Ring {}
struct Producer(Arc<Ring>);
struct Consumer(Arc<Ring>);
fn channel() -> (Producer, Consumer) {
    let ring = Arc::new(Ring {
        events: std::array::from_fn(|_| UnsafeCell::new(Event::default())),
        head: AtomicU32::new(0),
        tail: AtomicU32::new(0),
    });
    (Producer(ring.clone()), Consumer(ring))
}
impl Producer {
    fn push(&mut self, event: Event) -> bool {
        let r = &self.0;
        let h = r.head.load(Ordering::Relaxed);
        if h.wrapping_sub(r.tail.load(Ordering::Acquire)) >= RING as u32 {
            return false;
        }
        // This slot is unoccupied; the consumer cannot read it before head moves.
        unsafe {
            *r.events[h as usize % RING].get() = event;
        }
        r.head.store(h.wrapping_add(1), Ordering::Release);
        true
    }
}
impl Consumer {
    fn drain(&mut self, mut take: impl FnMut(Event)) {
        let r = &self.0;
        let mut t = r.tail.load(Ordering::Relaxed);
        let h = r.head.load(Ordering::Acquire);
        while t != h {
            // Published and not released to the producer until the loop completes.
            let ev = unsafe { *r.events[t as usize % RING].get() };
            take(ev);
            t = t.wrapping_add(1);
        }
        r.tail.store(t, Ordering::Release);
    }
}
fn mono_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
    }
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}
fn path(rx: f32, ry: f32, period: f32, ms: f64) -> (f32, f32) {
    let k = 2. * std::f64::consts::PI * ms / period as f64;
    (
        (rx as f64 * k.sin()) as f32,
        (ry as f64 * (2. * k).sin() / 2.) as f32,
    )
}
#[no_mangle]
pub unsafe extern "C" fn lbi_path(
    rx: f32,
    ry: f32,
    period: f32,
    ms: f64,
    dx: *mut f32,
    dy: *mut f32,
) {
    let (x, y) = path(rx, ry, period, ms);
    if let Some(out) = dx.as_mut() {
        *out = x;
    }
    if let Some(out) = dy.as_mut() {
        *out = y;
    }
}
struct Run {
    thread: JoinHandle<()>,
    stop: Arc<AtomicBool>,
    fd: Arc<OwnedFd>,
    consumer: Consumer,
}
impl Run {
    fn start(rx: f32, ry: f32, period: f32) -> std::io::Result<Self> {
        if !(rx >= 0. && ry >= 0. && period >= 500.) {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        let raw = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
        let stop = Arc::new(AtomicBool::new(false));
        let (mut producer, consumer) = channel();
        let signal = fd.clone();
        let stop_thread = stop.clone();
        let thread = std::thread::Builder::new()
            .name("bench-pointer".into())
            .spawn(move || {
                let t0 = mono_ns();
                let mut next = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                unsafe {
                    libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut next);
                }
                while !stop_thread.load(Ordering::SeqCst) {
                    next.tv_nsec += (1_000_000_000 / LBI_HZ) as libc::c_long;
                    if next.tv_nsec >= 1_000_000_000 {
                        next.tv_nsec -= 1_000_000_000;
                        next.tv_sec += 1;
                    }
                    unsafe {
                        while libc::clock_nanosleep(
                            libc::CLOCK_MONOTONIC,
                            libc::TIMER_ABSTIME,
                            &next,
                            std::ptr::null_mut(),
                        ) == libc::EINTR
                        {}
                    }
                    if stop_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    let ns = mono_ns();
                    let (dx, dy) = path(rx, ry, period, (ns - t0) as f64 / 1e6);
                    if producer.push(Event { ns, dx, dy }) {
                        let one = 1u64;
                        unsafe {
                            libc::write(signal.as_raw_fd(), (&one as *const u64).cast(), 8);
                        }
                    }
                }
            })?;
        Ok(Self {
            thread,
            stop,
            fd,
            consumer,
        })
    }
    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.join();
    }
}
static RUN: Mutex<Option<Run>> = Mutex::new(None);
fn lock() -> std::sync::MutexGuard<'static, Option<Run>> {
    RUN.lock().unwrap_or_else(|e| e.into_inner())
}
#[no_mangle]
pub extern "C" fn lbi_start(rx: f32, ry: f32, period: f32) -> c_int {
    let mut run = lock();
    if let Some(old) = run.take() {
        old.stop();
    }
    match Run::start(rx, ry, period) {
        Ok(new) => {
            *run = Some(new);
            0
        }
        Err(_) => -1,
    }
}
#[no_mangle]
pub extern "C" fn lbi_stop() {
    if let Some(run) = lock().take() {
        run.stop();
    }
}
#[no_mangle]
pub extern "C" fn lbi_fd() -> c_int {
    lock().as_ref().map_or(-1, |r| r.fd.as_raw_fd())
}
#[no_mangle]
pub extern "C" fn lbi_drain() {
    if let Some(run) = lock().as_ref() {
        let mut n = 0u64;
        unsafe {
            libc::read(run.fd.as_raw_fd(), (&mut n as *mut u64).cast(), 8);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn lbi_take(dx: *mut f32, dy: *mut f32) -> c_int {
    let mut run = lock();
    let Some(run) = run.as_mut() else {
        return 0;
    };
    let mut took = false;
    run.consumer.drain(|ev| {
        super::lbf_input(ev.ns);
        if let Some(dx) = dx.as_mut() {
            *dx = ev.dx;
        }
        if let Some(dy) = dy.as_mut() {
            *dy = ev.dy;
        }
        took = true;
    });
    took as c_int
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn figure_eight_keeps_its_phase_and_amplitude() {
        let (x, y) = path(200., 100., 4000., 1000.);
        assert!((x - 200.).abs() < 0.01 && y.abs() < 0.01);
        let (x, y) = path(200., 100., 4000., 500.);
        assert!((x - 141.42).abs() < 0.01 && (y - 50.).abs() < 0.01);
        assert!(Run::start(10., 10., 100.).is_err());
        assert!(Run::start(f32::NAN, 10., 1000.).is_err());
    }
    #[test]
    fn ring_drops_newest_when_full_and_handles_counter_wrap() {
        let (mut p, mut c) = channel();
        p.0.head.store(u32::MAX - 10, Ordering::Relaxed);
        p.0.tail.store(u32::MAX - 10, Ordering::Relaxed);
        for ns in 0..256 {
            assert!(p.push(Event {
                ns,
                ..Default::default()
            }));
        }
        assert!(!p.push(Event::default()));
        let mut seen = Vec::new();
        c.drain(|e| seen.push(e.ns));
        assert_eq!(seen, (0..256).collect::<Vec<_>>());
        assert!(p.push(Event {
            ns: 1000,
            ..Default::default()
        }));
        seen.clear();
        c.drain(|e| seen.push(e.ns));
        assert_eq!(seen, [1000]);
    }
    #[test]
    fn thread_signals_eventfd_and_stops_cleanly() {
        let mut run = Run::start(200., 100., 2000.).unwrap();
        let fd = run.fd.as_raw_fd();
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(unsafe { libc::poll(&mut poll, 1, 1000) }, 1);
        let mut events = Vec::new();
        run.consumer.drain(|e| events.push(e));
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .all(|e| e.ns > 0 && e.dx.abs() <= 200. && e.dy.abs() <= 50.));
        let ownership = Arc::downgrade(&run.fd);
        run.stop();
        assert!(ownership.upgrade().is_none());
    }
}
