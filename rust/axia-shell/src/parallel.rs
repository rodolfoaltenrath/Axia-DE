//! Synchronous pixel-worker pool, with the compatibility ABI in server/src/par.h.
//!
//! Only one parallel job runs at a time. Nested and concurrent submissions run
//! serially on their caller. Every worker acknowledges completion before the
//! borrowed callback context can be released or another generation can start.
use std::{
    cell::Cell,
    ffi::{c_int, c_void},
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock,
    },
};

const MAX_THREADS: usize = 8;
const RT_PRIORITY: c_int = 2;
type Kernel = unsafe extern "C" fn(*mut c_void, c_int, c_int);

#[derive(Clone, Copy)]
struct Job {
    kernel: Kernel,
    // The caller lends this pointer until all workers acknowledge completion.
    // Its callback, not the pool, is responsible for disjoint row ownership.
    context: usize,
    first: i64,
    rows: i64,
    pieces: usize,
}

#[derive(Default)]
struct State {
    generation: u64,
    job: Option<Job>,
    pending_workers: usize,
}

#[derive(Default)]
struct Pool {
    state: Mutex<State>,
    go: Condvar,
    done: Condvar,
    next: AtomicUsize,
    workers: AtomicUsize,
    busy: AtomicBool,
}

static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
static RT_ON: AtomicBool = AtomicBool::new(false);
static RT_TIDS: [AtomicI32; MAX_THREADS] = [const { AtomicI32::new(0) }; MAX_THREADS];
thread_local! { static INSIDE: Cell<bool> = const { Cell::new(false) }; }

impl Job {
    fn bounds(self, piece: usize) -> (c_int, c_int) {
        let first = self.first + self.rows * piece as i64 / self.pieces as i64;
        let end = self.first + self.rows * (piece + 1) as i64 / self.pieces as i64;
        (first as c_int, end as c_int)
    }
}

impl Pool {
    fn run(&self, job: Job) {
        INSIDE.with(|inside| {
            let previous = inside.replace(true);
            loop {
                let piece = self.next.fetch_add(1, Ordering::Relaxed);
                if piece >= job.pieces {
                    break;
                }
                let (first, end) = job.bounds(piece);
                if end > first {
                    // SAFETY: submission waits for every worker; the C contract
                    // requires a callback valid for these disjoint row ranges.
                    unsafe { (job.kernel)(job.context as *mut c_void, first, end) };
                }
            }
            inside.set(previous);
        });
    }

    fn worker(&self) {
        let mut seen = 0;
        loop {
            let mut state = self.state.lock().unwrap();
            while state.generation == seen {
                state = self.go.wait(state).unwrap();
            }
            seen = state.generation;
            let job = state.job.unwrap();
            drop(state);
            self.run(job);
            let mut state = self.state.lock().unwrap();
            state.pending_workers -= 1;
            if state.pending_workers == 0 {
                self.done.notify_one();
            }
        }
    }

    unsafe fn rows(&self, first: c_int, end: c_int, kernel: Kernel, context: *mut c_void) {
        let rows = i64::from(end) - i64::from(first);
        let workers = self.workers.load(Ordering::Relaxed);
        if workers == 0
            || INSIDE.with(Cell::get)
            || rows < 2
            || self.busy.swap(true, Ordering::Acquire)
        {
            kernel(context, first, end);
            return;
        }
        let job = Job {
            kernel,
            context: context as usize,
            first: i64::from(first),
            rows,
            pieces: ((workers + 1) * 3).min(rows as usize),
        };
        {
            let mut state = self.state.lock().unwrap();
            state.job = Some(job);
            state.pending_workers = workers;
            self.next.store(0, Ordering::Relaxed);
            state.generation = state.generation.wrapping_add(1);
            self.go.notify_all();
        }
        self.run(job);
        let mut state = self.state.lock().unwrap();
        while state.pending_workers != 0 {
            state = self.done.wait(state).unwrap();
        }
        state.job = None;
        drop(state);
        self.busy.store(false, Ordering::Release);
    }
}

// This handler must remain lock-free and allocation-free. A worker that registers
// after the handler ran sees RT_ON=false and demotes itself before accepting work.
extern "C" fn rt_demote(_: c_int) {
    let was_on = RT_ON.swap(false, Ordering::SeqCst);
    let param = libc::sched_param { sched_priority: 0 };
    for tid in &RT_TIDS {
        let tid = tid.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe { libc::sched_setscheduler(tid, libc::SCHED_OTHER, &param) };
        }
    }
    if was_on {
        let message = b"axia-server: back to the normal priority (a thread ran too long on real time, or it could not be set)\n";
        unsafe { libc::write(2, message.as_ptr().cast(), message.len()) };
    }
}

unsafe fn rt_set(flags: c_int) -> c_int {
    libc::sched_setscheduler(
        0,
        libc::SCHED_RR | flags,
        &libc::sched_param {
            sched_priority: RT_PRIORITY,
        },
    )
}

fn initialize(threads: c_int) -> Arc<Pool> {
    let pool = Arc::new(Pool::default());
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_RTPRIO, &mut limit) == 0
            && limit.rlim_cur >= RT_PRIORITY as libc::rlim_t
        {
            libc::signal(libc::SIGXCPU, rt_demote as *const () as libc::sighandler_t);
            if rt_set(0) == 0 {
                RT_TIDS[0].store(libc::syscall(libc::SYS_gettid) as c_int, Ordering::SeqCst);
                RT_ON.store(true, Ordering::SeqCst);
            } else {
                eprintln!(
                    "axia-server: no real-time priority ({})",
                    std::io::Error::last_os_error()
                );
            }
        }
        let threads = if threads <= 0 {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            if libc::sched_getaffinity(0, std::mem::size_of_val(&set), &mut set) == 0 {
                libc::CPU_COUNT(&set) as usize
            } else {
                libc::sysconf(libc::_SC_NPROCESSORS_ONLN).max(1) as usize
            }
        } else {
            threads as usize
        }
        .clamp(1, MAX_THREADS);
        for index in 1..threads {
            let worker_pool = Arc::clone(&pool);
            let realtime = RT_ON.load(Ordering::SeqCst);
            let (ready, started) = mpsc::sync_channel(0);
            let worker = std::thread::Builder::new()
                .name(format!("axia-pixels-{index}"))
                .stack_size(256 * 1024)
                .spawn(move || {
                    if realtime {
                        RT_TIDS[index]
                            .store(libc::syscall(libc::SYS_gettid) as c_int, Ordering::SeqCst);
                        if !RT_ON.load(Ordering::SeqCst) {
                            libc::sched_setscheduler(
                                0,
                                libc::SCHED_OTHER,
                                &libc::sched_param { sched_priority: 0 },
                            );
                        }
                    }
                    let _ = ready.send(());
                    worker_pool.worker();
                });
            if worker.is_err() {
                break;
            }
            // Register inherited realtime TIDs before the parent changes policy.
            // Dropping the handle detaches these process-lifetime workers.
            drop(worker);
            if started.recv().is_err() {
                break;
            }
            pool.workers.fetch_add(1, Ordering::Relaxed);
        }
        if RT_ON.load(Ordering::SeqCst) {
            if rt_set(libc::SCHED_RESET_ON_FORK) != 0 {
                rt_demote(0);
            } else {
                eprintln!("axia-server: compositor at real-time priority {RT_PRIORITY} (SCHED_RR, {} threads)", pool.workers.load(Ordering::Relaxed) + 1);
            }
        }
        // Children must never be able to request realtime scheduling.
        limit.rlim_cur = 0;
        limit.rlim_max = 0;
        libc::setrlimit(libc::RLIMIT_RTPRIO, &limit);
    }
    pool
}

/// Initialize once, on the compositor thread, before launching any subprocesses.
#[no_mangle]
pub extern "C" fn par_init(threads: c_int) {
    POOL.get_or_init(|| initialize(threads));
}

#[no_mangle]
pub extern "C" fn par_realtime() -> c_int {
    RT_ON.load(Ordering::SeqCst) as c_int
}

#[no_mangle]
pub extern "C" fn par_threads() -> c_int {
    POOL.get()
        .map_or(1, |pool| pool.workers.load(Ordering::Relaxed) as c_int + 1)
}

/// # Safety
/// For a nonempty interval, `kernel` must be callable until this function returns.
/// `context` must remain valid and the kernel must only mutate its own disjoint
/// rows. Nested and concurrent calls are permitted and execute serially.
#[no_mangle]
pub unsafe extern "C" fn par_rows(first: c_int, end: c_int, kernel: Kernel, context: *mut c_void) {
    if end <= first {
        return;
    }
    if let Some(pool) = POOL.get() {
        pool.rows(first, end, kernel, context);
    } else {
        kernel(context, first, end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{atomic::AtomicU32, Barrier};

    struct Participation {
        threads: Mutex<HashSet<std::thread::ThreadId>>,
        barrier: Barrier,
    }

    unsafe extern "C" fn participate(context: *mut c_void, _: c_int, _: c_int) {
        let participation = &*(context as *const Participation);
        let first_piece = participation
            .threads
            .lock()
            .unwrap()
            .insert(std::thread::current().id());
        if first_piece {
            participation.barrier.wait();
        }
    }

    struct Rows {
        hits: Vec<AtomicU32>,
        nested: AtomicUsize,
        calls: AtomicUsize,
    }

    unsafe extern "C" fn nested(context: *mut c_void, first: c_int, end: c_int) {
        let rows = &*(context as *const Rows);
        rows.nested
            .fetch_add((end - first) as usize, Ordering::Relaxed);
    }

    unsafe extern "C" fn count(context: *mut c_void, first: c_int, end: c_int) {
        let rows = &*(context as *const Rows);
        rows.calls.fetch_add(1, Ordering::Relaxed);
        for row in first..end {
            rows.hits[row as usize].fetch_add(1, Ordering::Relaxed);
        }
        par_rows(0, 3, nested, context);
    }

    #[test]
    fn process_contract() {
        let Ok(threads) = std::env::var("AXIA_PIXEL_POOL_TEST_THREADS") else {
            for threads in ["1", "4", "128", "0"] {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "parallel::tests::process_contract",
                        "--nocapture",
                    ])
                    .env("AXIA_PIXEL_POOL_TEST_THREADS", threads)
                    .output()
                    .unwrap();
                assert!(
                    child.status.success(),
                    "threads={threads}: {} {}",
                    String::from_utf8_lossy(&child.stdout),
                    String::from_utf8_lossy(&child.stderr)
                );
            }
            return;
        };
        let rows = Arc::new(Rows {
            hits: (0..257).map(|_| AtomicU32::new(0)).collect(),
            nested: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let context = Arc::as_ptr(&rows) as *mut c_void;
        assert_eq!(par_threads(), 1);
        unsafe { par_rows(0, 257, count, context) };
        par_init(threads.parse().unwrap());
        let workers = par_threads();
        assert!((1..=8).contains(&workers));
        if threads == "1" {
            assert_eq!(workers, 1);
        }
        let mut limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        unsafe {
            assert_eq!(libc::getrlimit(libc::RLIMIT_RTPRIO, &mut limit), 0);
        }
        assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
        par_init(1);
        assert_eq!(par_threads(), workers);
        let participation = Participation {
            threads: Mutex::new(HashSet::new()),
            barrier: Barrier::new(workers as usize),
        };
        unsafe {
            par_rows(
                0,
                workers * 3,
                participate,
                &participation as *const Participation as *mut c_void,
            );
        }
        assert_eq!(
            participation.threads.lock().unwrap().len(),
            workers as usize
        );
        // Concurrent submissions, many generations, short and empty ranges.
        let barrier = Arc::new(Barrier::new(5));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let rows = Arc::clone(&rows);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let context = Arc::as_ptr(&rows) as *mut c_void;
                for _ in 0..200 {
                    unsafe {
                        par_rows(0, 257, count, context);
                        par_rows(10, 10, count, context);
                        par_rows(10, 9, count, context);
                    }
                }
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }
        unsafe {
            par_rows(256, 257, count, context);
        }
        for (row, hits) in rows.hits.iter().enumerate() {
            assert_eq!(
                hits.load(Ordering::Relaxed),
                if row == 256 { 802 } else { 801 }
            );
        }
        assert_eq!(
            rows.nested.load(Ordering::Relaxed),
            3 * rows.calls.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn split_covers_extreme_signed_endpoints_without_overflow() {
        let job = Job {
            kernel: nested,
            context: 0,
            first: i32::MIN as i64,
            rows: u32::MAX as i64,
            pieces: 24,
        };
        let mut previous = i32::MIN;
        for piece in 0..job.pieces {
            let (first, end) = job.bounds(piece);
            assert_eq!(first, previous);
            assert!(end > first);
            previous = end;
        }
        assert_eq!(previous, i32::MAX);
    }
}
