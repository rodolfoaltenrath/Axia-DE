// Share the safe decoder without importing the codecs' C exports into a second static library.
#[path = "../../../rust/axia-codecs/src/xz_core.rs"]
mod xz;

// Native workloads. Every invocation owns its workers and scratch memory.
use super::bench_now_ms;
use libc::{c_int, c_long, c_void};
use std::{
    hint::black_box,
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub(crate) fn count(n: c_int) -> usize {
    n.clamp(1, 256) as usize
}
fn duration(secs: f64) -> Option<Duration> {
    (secs.is_finite() && (0.0..=1800.0).contains(&secs)).then(|| Duration::from_secs_f64(secs))
}

/// Workers acknowledge the gate and are joined before the borrowed callback expires.
/// On a spawn failure no callback runs. There is no shared static job storage.
#[no_mangle]
pub unsafe extern "C" fn lb_parallel(
    n: c_int,
    f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
    arg: *mut c_void,
) -> f64 {
    let Some(f) = f else { return -1.0 };
    let arg = arg as usize;
    parallel(count(n), move |i| unsafe {
        f(arg as *mut c_void, i as c_int)
    })
    .map(|(_, ms)| ms)
    .unwrap_or(-1.0)
}
pub(crate) fn parallel<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Option<(Vec<T>, f64)> {
    thread::scope(|scope| {
        let gate = Arc::new(AtomicI32::new(0));
        let mut workers = Vec::new();
        for i in 1..n {
            let worker_gate = gate.clone();
            let f = &f;
            match thread::Builder::new().spawn_scoped(scope, move || {
                while worker_gate.load(Ordering::Acquire) == 0 {
                    thread::yield_now();
                }
                (worker_gate.load(Ordering::Acquire) > 0).then(|| f(i))
            }) {
                Ok(w) => workers.push(w),
                Err(_) => {
                    gate.store(-1, Ordering::Release);
                    for w in workers {
                        let _ = w.join();
                    }
                    return None;
                }
            }
        }
        let t0 = bench_now_ms();
        gate.store(1, Ordering::Release);
        let mut results = vec![f(0)];
        for w in workers {
            results.push(w.join().ok()??);
        }
        Some((results, bench_now_ms() - t0))
    })
}
pub(crate) fn timed<S>(
    threads: c_int,
    secs: f64,
    setup: impl Fn() -> Option<S> + Sync,
    unit: impl Fn(&mut S) -> bool + Sync,
) -> f64 {
    let Some(duration) = duration(secs) else {
        return -1.0;
    };
    let Some((runs, _)) = parallel(count(threads), |_| {
        let mut state = setup()?;
        let start = std::time::Instant::now();
        let mut n = 0u64;
        loop {
            if !unit(&mut state) {
                return None;
            }
            black_box(&state);
            n += 1;
            if start.elapsed() >= duration {
                break;
            }
        }
        Some(n as f64 / start.elapsed().as_secs_f64())
    }) else {
        return -1.0;
    };
    runs.into_iter()
        .try_fold(0.0, |a, r| r.map(|r| a + r))
        .unwrap_or(-1.0)
}
fn buffer<T: Default + Clone>(n: usize) -> Option<Vec<T>> {
    let mut b = Vec::new();
    b.try_reserve_exact(n).ok()?;
    b.resize(n, T::default());
    Some(b)
}
#[no_mangle]
pub extern "C" fn lb_sha256(threads: c_int, secs: f64) -> f64 {
    let r = timed(
        threads,
        secs,
        || {
            let mut b = buffer::<u8>(1 << 20)?;
            for (i, b) in b.iter_mut().enumerate() {
                *b = ((i as u32).wrapping_mul(2654435761) >> 13) as u8;
            }
            Some(b)
        },
        |b| {
            b[0] ^= axia_crypto::vcrypto::sha256_digest(b)[0];
            true
        },
    );
    if r < 0.0 {
        r
    } else {
        r * (1 << 20) as f64 / 1e6
    }
}
const MAT_N: usize = 256;
const MAT_FLOP: f64 = (2 * MAT_N * MAT_N * MAT_N) as f64;
struct Matrix {
    a: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
}
impl Matrix {
    fn new() -> Option<Self> {
        let mut a = buffer(MAT_N * MAT_N)?;
        let mut b = buffer(MAT_N * MAT_N)?;
        for i in 0..a.len() {
            a[i] = (i % 17) as f64 * 0.25;
            b[i] = (i % 13) as f64 * 0.5;
        }
        Some(Self {
            a,
            b,
            c: buffer(MAT_N * MAT_N)?,
        })
    }
    fn unit(&mut self) {
        self.c.fill(0.0);
        for i in 0..MAT_N {
            for k in 0..MAT_N {
                let aik = self.a[i * MAT_N + k];
                let b = &self.b[k * MAT_N..(k + 1) * MAT_N];
                let c = &mut self.c[i * MAT_N..(i + 1) * MAT_N];
                for j in 0..MAT_N {
                    c[j] += aik * b[j];
                }
            }
        }
        self.a[0] = black_box(self.c[MAT_N * MAT_N - 1]) * 1e-12;
    }
}
#[no_mangle]
pub extern "C" fn lb_matmul(threads: c_int, secs: f64) -> f64 {
    let r = timed(threads, secs, Matrix::new, |m| {
        m.unit();
        true
    });
    if r < 0.0 {
        r
    } else {
        r * MAT_FLOP / 1e9
    }
}
const CORPUS: &[u8] = include_bytes!("corpus.xz");
#[no_mangle]
pub extern "C" fn lb_xz(threads: c_int, secs: f64) -> f64 {
    let r = timed(
        threads,
        secs,
        || Some(()),
        |_| xz::decode(CORPUS, (2 << 20) + 1).is_some_and(|b| black_box(b.len()) == 2 << 20),
    );
    if r < 0.0 {
        r
    } else {
        r * (2 << 20) as f64 / 1e6
    }
}
/// Strings, JSON, sorting, regular expressions and maps executed entirely in Rust.
/// This has new measurement names; historical QuickJS scores are not comparable.
#[no_mangle]
pub extern "C" fn lb_data(threads: c_int, secs: f64) -> f64 {
    timed(
        threads,
        secs,
        || regex::Regex::new(r"item-(\d+)-(\d+)").ok(),
        |re| {
            let list: Vec<_> = (0..2000).map(|i| serde_json::json!({"id":i,"name":format!("item-{i}-{}", i*7919%1000),"tags":[format!("item-{i}-{}",i*7919%1000).len(),i%7]})).collect();
            let j = serde_json::to_string(&serde_json::json!({"list":list})).unwrap();
            let mut p: serde_json::Value = serde_json::from_str(&j).unwrap();
            let list = p["list"].as_array_mut().unwrap();
            list.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            let n: usize = list
                .iter()
                .filter_map(|v| re.captures(v["name"].as_str().unwrap()))
                .map(|m| m[2].parse::<usize>().unwrap())
                .sum();
            let map: std::collections::HashMap<_, _> = list
                .iter()
                .enumerate()
                .map(|(i, v)| (v["name"].as_str().unwrap(), i))
                .collect();
            black_box(n + j.len() + map.len());
            true
        },
    )
}
fn bandwidth(threads: c_int, mb: usize, secs: f64, triad: bool) -> f64 {
    let Some(duration) = duration(secs) else {
        return -1.0;
    };
    let Some(n) = mb
        .checked_mul(1 << 20)
        .filter(|n| *n > 0 && *n <= 512 << 20)
        .map(|n| n / 8)
    else {
        return -1.0;
    };
    let Some(mut a) = buffer::<f64>(n) else {
        return -1.0;
    };
    let Some(mut b) = buffer::<f64>(n) else {
        return -1.0;
    };
    let Some(mut c) = buffer::<f64>(n) else {
        return -1.0;
    };
    let threads = count(threads);
    // Disjoint row slices are owned by their worker; allocations are never aliased.
    let start = std::time::Instant::now();
    let bytes = thread::scope(|scope| {
        let mut workers = Vec::new();
        let chunk = n.div_ceil(threads);
        for ((a, b), c) in a
            .chunks_mut(chunk)
            .zip(b.chunks_mut(chunk))
            .zip(c.chunks_mut(chunk))
        {
            workers.push(
                thread::Builder::new()
                    .spawn_scoped(scope, move || {
                        a.fill(1.0);
                        b.fill(2.0);
                        c.fill(0.0);
                        let t = std::time::Instant::now();
                        let mut bytes = 0usize;
                        loop {
                            if triad {
                                for i in 0..a.len() {
                                    a[i] = b[i] + 3.0 * c[i];
                                }
                            } else {
                                c.copy_from_slice(a);
                            }
                            black_box((&*a, &*b, &*c));
                            bytes += a.len() * 8 * if triad { 3 } else { 2 };
                            if t.elapsed() >= duration {
                                break;
                            }
                        }
                        bytes as f64
                    })
                    .ok()?,
            );
        }
        workers
            .into_iter()
            .try_fold(0.0, |sum, w| w.join().ok().map(|n| sum + n))
    });
    bytes
        .map(|bytes| bytes / 1e9 / start.elapsed().as_secs_f64())
        .unwrap_or(-1.0)
}
#[no_mangle]
pub extern "C" fn lb_mem_copy(n: c_int, mb: usize, secs: f64) -> f64 {
    bandwidth(n, mb, secs, false)
}
#[no_mangle]
pub extern "C" fn lb_mem_triad(n: c_int, mb: usize, secs: f64) -> f64 {
    bandwidth(n, mb, secs, true)
}
fn xs(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}
#[repr(align(64))]
#[derive(Default, Clone)]
struct Line {
    next: usize,
}
#[no_mangle]
pub extern "C" fn lb_mem_latency(mb: usize, hops: c_long) -> f64 {
    let Some(n) = mb
        .checked_mul(1 << 20)
        .filter(|n| *n <= 512 << 20)
        .map(|n| n / 64)
        .filter(|n| *n >= 2)
    else {
        return -1.0;
    };
    if hops <= 0 {
        return -1.0;
    }
    let Some(mut buf) = buffer::<Line>(n) else {
        return -1.0;
    };
    let Some(mut perm) = buffer::<usize>(n) else {
        return -1.0;
    };
    for (i, p) in perm.iter_mut().enumerate() {
        *p = i;
    }
    let mut seed = 0x9e3779b97f4a7c15;
    for i in (1..n).rev() {
        perm.swap(i, (xs(&mut seed) % i as u64) as usize);
    }
    for i in 0..n {
        buf[perm[i]].next = perm[(i + 1) % n];
    }
    let mut p = 0;
    for _ in 0..hops / 10 {
        p = buf[p].next;
    }
    let t0 = bench_now_ms();
    for _ in 0..hops {
        p = buf[p].next;
    }
    black_box(p);
    (bench_now_ms() - t0) * 1e6 / hops as f64
}
#[no_mangle]
pub unsafe extern "C" fn lb_disk(
    fd: c_int,
    mb: usize,
    secs: f64,
    w: *mut f64,
    r: *mut f64,
    iops: *mut f64,
) -> c_int {
    if w.is_null() || r.is_null() || iops.is_null() {
        return -1;
    }
    *w = -1.0;
    *r = -1.0;
    *iops = -1.0;
    let Some(duration) = duration(secs) else {
        return -1;
    };
    if !(8..=1024).contains(&mb) {
        return -1;
    }
    let Some(mut b) = buffer::<u8>(1 << 20) else {
        return -1;
    };
    let mut seed = 0x1234567;
    for c in b.chunks_exact_mut(8) {
        c.copy_from_slice(&xs(&mut seed).to_ne_bytes());
    }
    let result = (|| {
        if libc::ftruncate(fd, 0) != 0 {
            return None;
        }
        let start = std::time::Instant::now();
        for i in 0..mb {
            b[0] = i as u8;
            if libc::pwrite(fd, b.as_ptr().cast(), b.len(), (i * b.len()) as _) != b.len() as isize
            {
                return None;
            }
        }
        if libc::fdatasync(fd) != 0 {
            return None;
        }
        *w = mb as f64 / start.elapsed().as_secs_f64();
        libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
        let start = std::time::Instant::now();
        for i in 0..mb {
            if libc::pread(fd, b.as_mut_ptr().cast(), b.len(), (i * b.len()) as _)
                != b.len() as isize
            {
                return None;
            }
        }
        *r = mb as f64 / start.elapsed().as_secs_f64();
        libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
        let start = std::time::Instant::now();
        let mut n = 0u64;
        loop {
            let page = xs(&mut seed) % (mb * 256) as u64;
            if libc::pread(fd, b.as_mut_ptr().cast(), 4096, (page * 4096) as _) != 4096 {
                return None;
            }
            n += 1;
            if start.elapsed() >= duration {
                break;
            }
        }
        *iops = n as f64 / start.elapsed().as_secs_f64();
        Some(())
    })();
    if libc::ftruncate(fd, 0) == 0 && result.is_some() {
        0
    } else {
        -1
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_sustained(
    threads: c_int,
    secs: f64,
    interval: f64,
    sample: Option<unsafe extern "C" fn(f64, f64, *mut c_void)>,
    ctx: *mut c_void,
) {
    let (Some(duration), Some(interval), Some(sample)) = (
        duration(secs),
        duration(interval).filter(|d| !d.is_zero()),
        sample,
    ) else {
        return;
    };
    let stop = AtomicBool::new(false);
    let done = std::sync::atomic::AtomicU64::new(0);
    thread::scope(|scope| {
        let Ok(worker) = thread::Builder::new().spawn_scoped(scope, || {
            parallel(count(threads), |_| {
                if let Some(mut m) = Matrix::new() {
                    while !stop.load(Ordering::Relaxed) {
                        m.unit();
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        }) else {
            return;
        };
        let start = std::time::Instant::now();
        let mut last_t = 0.0;
        let mut last = 0;
        while start.elapsed() < duration {
            thread::sleep(interval.min(duration.saturating_sub(start.elapsed())));
            let now = start.elapsed().as_secs_f64();
            let n = done.load(Ordering::Relaxed);
            if now > last_t {
                sample(
                    (n - last) as f64 * MAT_FLOP / 1e9 / (now - last_t),
                    now,
                    ctx,
                );
            }
            last = n;
            last_t = now;
        }
        stop.store(true, Ordering::Relaxed);
        let _ = worker.join();
    });
}
#[no_mangle]
pub unsafe extern "C" fn lb_median(v: *mut f64, n: c_int, spread: *mut f64) -> f64 {
    if !spread.is_null() {
        *spread = 0.0;
    }
    if n <= 0 || v.is_null() {
        return -1.0;
    }
    let v = std::slice::from_raw_parts_mut(v, n as usize);
    v.sort_by(f64::total_cmp);
    if !spread.is_null() {
        *spread = v[v.len() - 1] - v[0];
    }
    if n % 2 == 1 {
        v[v.len() / 2]
    } else {
        (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_fonts_and_compressed_corpus() {
        let digest = axia_crypto::vcrypto::sha256_digest(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ));
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82"
        );
        assert_eq!(
            axia_crypto::vcrypto::sha256_digest(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        assert_eq!(xz::decode(CORPUS, 2 << 20).unwrap().len(), 2 << 20);
        assert!(xz::decode(CORPUS, (2 << 20) - 1).is_none());
    }
    #[test]
    fn sustained_joins_workers_and_clamps_the_last_interval() {
        unsafe extern "C" fn collect(v: f64, t: f64, p: *mut c_void) {
            assert!(v >= 0.0 && v.is_finite());
            (*(p as *mut Vec<f64>)).push(t)
        }
        let mut values = Vec::<f64>::new();
        unsafe {
            lb_sustained(
                2,
                0.03,
                0.01,
                Some(collect),
                (&mut values as *mut Vec<f64>).cast(),
            )
        };
        assert!(!values.is_empty());
        assert!(values.windows(2).all(|v| v[1] > v[0]));
        assert!(values.last().unwrap() >= &0.03);
    }
    #[test]
    fn workers_are_reentrant_and_borrow_until_join() {
        let (v, _) = parallel(4, |i| parallel(2, |j| i * 2 + j).unwrap().0).unwrap();
        assert_eq!(v, vec![vec![0, 1], vec![2, 3], vec![4, 5], vec![6, 7]]);
    }
    #[test]
    fn workloads_and_invalid_arguments() {
        for f in [lb_sha256, lb_matmul, lb_xz, lb_data] {
            assert!(f(2, 0.0) > 0.0);
            assert_eq!(f(1, f64::NAN), -1.0);
        }
        assert!(lb_mem_copy(2, 1, 0.001) > 0.0);
        assert!(lb_mem_triad(2, 1, 0.001) > 0.0);
        assert!(lb_mem_latency(1, 10000) > 0.0);
        assert_eq!(lb_mem_latency(0, 0), -1.0);
        assert_eq!(lb_mem_copy(1, usize::MAX, 0.0), -1.0);
    }
    #[test]
    fn disk_is_truncated_and_rates_are_positive() {
        use std::os::fd::AsRawFd;
        let f = std::fs::File::from(unsafe {
            std::os::fd::OwnedFd::from_raw_fd(libc::memfd_create(
                c"bench-test".as_ptr(),
                libc::MFD_CLOEXEC,
            ))
        });
        let (mut w, mut r, mut i) = (0.0, 0.0, 0.0);
        assert_eq!(
            unsafe { lb_disk(f.as_raw_fd(), 8, 0.001, &mut w, &mut r, &mut i) },
            0
        );
        assert!(w > 0.0 && r > 0.0 && i > 0.0);
        assert_eq!(f.metadata().unwrap().len(), 0);
        assert_eq!(unsafe { lb_disk(-1, 8, 0.0, &mut w, &mut r, &mut i) }, -1);
    }
    use std::os::fd::FromRawFd;
}
