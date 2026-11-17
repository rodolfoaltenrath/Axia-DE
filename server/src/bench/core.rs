use super::bytes;
use libc::{c_char, c_int, c_ulong, FILE};
pub const BENCH_HIST: usize = 240;
#[repr(C)]
pub struct BenchFrames {
    pub iv: [f64; BENCH_HIST],
    pub n: c_int,
    pub frames: c_ulong,
    pub first: f64,
    pub last: f64,
}
impl Default for BenchFrames {
    fn default() -> Self {
        Self {
            iv: [0.; BENCH_HIST],
            n: 0,
            frames: 0,
            first: 0.,
            last: 0.,
        }
    }
}
#[derive(Default, Debug)]
#[repr(C)]
pub struct BenchStats {
    pub avg: f64,
    pub max: f64,
    pub p50: f64,
    pub p99: f64,
}
#[derive(Default)]
#[repr(C)]
pub struct BenchPresent {
    pub presented: c_ulong,
    pub discarded: c_ulong,
    pub vblank_skips: c_ulong,
    pub zero_copy: c_ulong,
    pub refresh_ns: u32,
    pub last_ms: f64,
    pub last_msc: u64,
}
#[derive(Default)]
#[repr(C)]
pub struct BenchLatency {
    pub last: f64,
    pub sum: f64,
    pub n: c_ulong,
    pub lost: c_ulong,
}

#[no_mangle]
pub extern "C" fn bench_now_ms() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
    }
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6
}
#[no_mangle]
pub unsafe extern "C" fn bench_frames_add(f: *mut BenchFrames, t: f64) {
    let Some(f) = f.as_mut() else {
        return;
    };
    if f.frames != 0 {
        let i = if f.n < BENCH_HIST as c_int {
            let i = f.n as usize;
            f.n += 1;
            i
        } else {
            f.frames as usize % BENCH_HIST
        };
        f.iv[i] = t - f.last;
    } else {
        f.first = t;
    }
    f.last = t;
    f.frames = f.frames.wrapping_add(1);
}
#[no_mangle]
pub unsafe extern "C" fn bench_frames_fps(f: *const BenchFrames) -> f64 {
    let Some(f) = f.as_ref() else {
        return 0.;
    };
    let span = f.last - f.first;
    if span > 0. && f.frames > 1 {
        (f.frames - 1) as f64 * 1000. / span
    } else {
        0.
    }
}
#[no_mangle]
pub unsafe extern "C" fn bench_frames_stats(f: *const BenchFrames, out: *mut BenchStats) {
    let Some(out) = out.as_mut() else {
        return;
    };
    *out = BenchStats::default();
    let Some(f) = f.as_ref() else {
        return;
    };
    let n = f.n.clamp(0, BENCH_HIST as c_int) as usize;
    if n == 0 {
        return;
    }
    let mut sorted = [0.; BENCH_HIST];
    sorted[..n].copy_from_slice(&f.iv[..n]);
    let sum: f64 = sorted[..n].iter().sum();
    sorted[..n].sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    *out = BenchStats {
        avg: sum / n as f64,
        max: sorted[n - 1],
        p50: sorted[(n - 1) / 2],
        p99: sorted[((99 * n + 99) / 100).max(1) - 1],
    };
}
#[no_mangle]
pub unsafe extern "C" fn bench_frames_missed(f: *const BenchFrames, budget: f64) -> c_int {
    let Some(f) = f.as_ref() else {
        return 0;
    };
    if budget <= 0. {
        return 0;
    }
    f.iv[..f.n.clamp(0, BENCH_HIST as c_int) as usize]
        .iter()
        .filter(|v| **v > budget * 1.5)
        .count() as c_int
}
#[no_mangle]
pub unsafe extern "C" fn bench_present(
    p: *mut BenchPresent,
    t: f64,
    msc: u64,
    refresh_ns: u32,
    zero_copy: c_int,
) {
    let Some(p) = p.as_mut() else {
        return;
    };
    p.presented = p.presented.wrapping_add(1);
    if refresh_ns != 0 {
        p.refresh_ns = refresh_ns;
    }
    if zero_copy != 0 {
        p.zero_copy = p.zero_copy.wrapping_add(1);
    }
    if p.last_msc != 0 && msc > p.last_msc.wrapping_add(1) {
        p.vblank_skips = p
            .vblank_skips
            .wrapping_add((msc - p.last_msc - 1) as c_ulong);
    } else if msc == 0 && p.presented > 1 && p.refresh_ns != 0 {
        let gap = (t - p.last_ms) / (p.refresh_ns as f64 / 1e6);
        if gap > 1.5 {
            p.vblank_skips = p.vblank_skips.wrapping_add((gap + 0.5) as c_ulong - 1);
        }
    }
    p.last_msc = msc;
    p.last_ms = t;
}
#[no_mangle]
pub unsafe extern "C" fn bench_refresh_hz(p: *const BenchPresent) -> f64 {
    p.as_ref()
        .filter(|p| p.refresh_ns != 0)
        .map(|p| 1e9 / p.refresh_ns as f64)
        .unwrap_or(0.)
}
#[no_mangle]
pub unsafe extern "C" fn bench_latency_add(l: *mut BenchLatency, ms: f64) {
    let Some(l) = l.as_mut() else {
        return;
    };
    if ms < 0. || ms >= 2000. {
        l.lost = l.lost.wrapping_add(1);
        return;
    }
    l.last = ms;
    l.sum += ms;
    l.n = l.n.wrapping_add(1);
}
#[no_mangle]
pub unsafe extern "C" fn bench_latency_avg(l: *const BenchLatency) -> f64 {
    l.as_ref()
        .filter(|l| l.n != 0)
        .map(|l| l.sum / l.n as f64)
        .unwrap_or(0.)
}
#[no_mangle]
pub unsafe extern "C" fn bench_gl_kind(renderer: *const c_char) -> *const c_char {
    let r = bytes(renderer);
    let contains = |needle: &[u8]| r.windows(needle.len()).any(|w| w == needle);
    let value: &[u8] = if r.is_empty() {
        b"\0"
    } else if [
        b"llvmpipe".as_slice(),
        b"softpipe",
        b"swrast",
        b"SwiftShader",
    ]
    .iter()
    .any(|v| contains(v))
    {
        b"SOFTWARE (CPU)\0"
    } else if [b"virgl".as_slice(), b"VirGL", b"SVGA", b"virtio"]
        .iter()
        .any(|v| contains(v))
    {
        b"VIRTUAL GPU (HOST)\0"
    } else {
        b"HARDWARE GPU\0"
    };
    value.as_ptr().cast()
}
#[no_mangle]
pub unsafe extern "C" fn bench_json_str(out: *mut FILE, text: *const c_char) {
    if out.is_null() {
        return;
    }
    libc::fputc(b'"' as c_int, out);
    for &b in bytes(text) {
        match b {
            b'"' | b'\\' => {
                libc::fputc(b'\\' as c_int, out);
                libc::fputc(b as c_int, out);
            }
            0..=31 => {
                let escaped = format!("\\u{b:04x}");
                libc::fwrite(escaped.as_ptr().cast(), 1, escaped.len(), out);
            }
            _ => {
                libc::fputc(b as c_int, out);
            }
        }
    }
    libc::fputc(b'"' as c_int, out);
}
#[no_mangle]
pub unsafe extern "C" fn bench_save(
    name: *const c_char,
    write: Option<unsafe extern "C" fn(*mut FILE)>,
) -> c_int {
    save(
        bytes(name),
        write,
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
    )
}

unsafe fn save(
    name: &[u8],
    write: Option<unsafe extern "C" fn(*mut FILE)>,
    state: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> c_int {
    if name.is_empty()
        || name.len() > 32
        || !name
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    {
        return -1;
    }
    let Some(write) = write else {
        return -1;
    };
    use std::os::unix::ffi::OsStrExt;
    let state = state.filter(|p| p.as_bytes().first() == Some(&b'/'));
    let Some(dir) = state
        .map(|p| std::path::PathBuf::from(p).join("axia"))
        .or_else(|| {
            home.filter(|p| p.as_bytes().first() == Some(&b'/'))
                .map(|p| {
                    let local = std::path::PathBuf::from(p).join(".local");
                    mkdir_private(&local);
                    let state = local.join("state");
                    mkdir_private(&state);
                    state.join("axia")
                })
        })
    else {
        return -1;
    };
    let name = std::str::from_utf8(name).unwrap();
    let tmp = dir.join(format!(".{name}.json.tmp"));
    let path = dir.join(format!("{name}.json"));
    mkdir_private(&dir);
    let Ok(tmp_c) = std::ffi::CString::new(tmp.as_os_str().as_bytes()) else {
        return -1;
    };
    let f = libc::fopen(tmp_c.as_ptr(), b"we\0".as_ptr().cast());
    if f.is_null() {
        return -1;
    }
    write(f);
    if libc::fclose(f) != 0 {
        let _ = std::fs::remove_file(tmp);
        return -1;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        return -1;
    }
    0
}

fn mkdir_private(path: &std::path::Path) {
    use std::os::unix::fs::DirBuilderExt;
    let _ = std::fs::DirBuilder::new().mode(0o700).create(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    #[test]
    fn json_escapes_control_bytes_without_changing_utf8() {
        unsafe {
            let mut data = std::ptr::null_mut();
            let mut len = 0;
            let stream = libc::open_memstream(&mut data, &mut len);
            assert!(!stream.is_null());
            bench_json_str(stream, b"a\"b\\c\n\x01\xc3\xa9\0".as_ptr().cast());
            assert_eq!(libc::fclose(stream), 0);
            assert_eq!(
                std::slice::from_raw_parts(data.cast::<u8>(), len),
                b"\"a\\\"b\\\\c\\u000a\\u0001\xc3\xa9\""
            );
            libc::free(data.cast());
        }
    }

    #[test]
    fn report_paths_names_and_atomic_replacement() {
        use std::os::unix::fs::PermissionsExt;
        unsafe extern "C" fn hello(f: *mut FILE) {
            libc::fputs(b"{\"x\":1}\n\0".as_ptr().cast(), f);
        }
        let root = std::env::temp_dir().join(format!("axia-report-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        unsafe {
            let state = Some(root.clone().into_os_string());
            assert_eq!(save(b"testbench", Some(hello), state.clone(), None), 0);
            let path = root.join("axia/testbench.json");
            assert_eq!(std::fs::read(&path).unwrap(), b"{\"x\":1}\n");
            assert_eq!(
                std::fs::metadata(root.join("axia"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            std::fs::write(&path, b"old report").unwrap();
            assert_eq!(save(b"testbench", Some(hello), state.clone(), None), 0);
            assert_eq!(std::fs::read(&path).unwrap(), b"{\"x\":1}\n");
            assert!(!root.join("axia/.testbench.json.tmp").exists());
            for bad in [b"../x".as_slice(), b"A", b"", &[b'a'; 33]] {
                assert_eq!(save(bad, Some(hello), state.clone(), None), -1);
            }
            assert_eq!(save(b"x", None, state.clone(), None), -1);
            assert_eq!(
                save(
                    b"x",
                    Some(hello),
                    Some(root.join("missing").into_os_string()),
                    None
                ),
                -1
            );
            assert!(!root.join("missing").exists());
            assert_eq!(save(b"x", Some(hello), Some("relative".into()), None), -1);
            assert_eq!(
                save(b"home", Some(hello), Some("relative".into()), state),
                0
            );
            assert_eq!(
                std::fs::read(root.join(".local/state/axia/home.json")).unwrap(),
                b"{\"x\":1}\n"
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn frame_quantiles_and_ring_preserve_the_measurement_definition() {
        unsafe {
            let mut f = BenchFrames::default();
            let mut t = 1000.;
            for i in 0..100 {
                bench_frames_add(&mut f, t);
                t += if i == 50 { 50. } else { 16. };
            }
            let mut s = BenchStats::default();
            bench_frames_stats(&f, &mut s);
            assert_eq!((f.frames, f.n), (100, 99));
            assert_eq!((s.max, s.p50, s.p99), (50., 16., 50.));
            assert!((s.avg - (98. * 16. + 50.) / 99.).abs() < 1e-6);
            assert_eq!(bench_frames_missed(&f, 1000. / 60.), 1);
            assert_eq!(bench_frames_missed(&f, 1000. / 144.), 99);
            assert!((bench_frames_fps(&f) - 99000. / (98. * 16. + 50.)).abs() < 1e-6);
            let mut f = BenchFrames::default();
            for i in 0..=1000 {
                bench_frames_add(&mut f, i as f64 * 10. + if i > 990 { 30. } else { 0. });
            }
            bench_frames_stats(&f, &mut s);
            assert_eq!((f.n, s.max, s.p50), (240, 40., 10.));
        }
    }
    #[test]
    fn one_stall_in_199_intervals_stays_out_of_p99() {
        unsafe {
            let mut f = BenchFrames::default();
            for i in 0..200 {
                bench_frames_add(&mut f, i as f64 * 16. + if i >= 100 { 34. } else { 0. });
            }
            let mut s = BenchStats::default();
            bench_frames_stats(&f, &mut s);
            assert_eq!((f.n, s.p99, s.max), (199, 16., 50.));
            let empty = BenchFrames::default();
            bench_frames_stats(&empty, &mut s);
            assert_eq!((s.avg, s.p99, bench_frames_fps(&empty)), (0., 0., 0.));
        }
    }
    #[test]
    fn presentation_uses_vblank_counter_and_time_fallback() {
        unsafe {
            let mut p = BenchPresent::default();
            bench_present(&mut p, 0., 100, 16666667, 0);
            bench_present(&mut p, 16.7, 101, 0, 1);
            bench_present(&mut p, 66.7, 104, 0, 0);
            assert_eq!((p.presented, p.vblank_skips, p.zero_copy), (3, 2, 1));
            assert!((bench_refresh_hz(&p) - 60.).abs() < 0.01);
            let mut p = BenchPresent::default();
            bench_present(&mut p, 0., 0, 16666667, 0);
            bench_present(&mut p, 50., 0, 0, 0);
            bench_present(&mut p, 66.7, 0, 0, 0);
            assert_eq!(p.vblank_skips, 2);
        }
    }
    #[test]
    fn latency_and_renderer_classes_keep_the_original_boundaries() {
        unsafe {
            let mut l = BenchLatency {
                last: -1.,
                ..Default::default()
            };
            for ms in [20., 30., -1., 2000.] {
                bench_latency_add(&mut l, ms);
            }
            assert_eq!(
                (l.n, l.lost, l.last, bench_latency_avg(&l)),
                (2, 2, 30., 25.)
            );
            for (input, kind) in [
                ("llvmpipe (LLVM 19)", "SOFTWARE (CPU)"),
                ("virgl (AMD)", "VIRTUAL GPU (HOST)"),
                ("V3D 7.1.7.0", "HARDWARE GPU"),
                ("", ""),
            ] {
                let s = std::ffi::CString::new(input).unwrap();
                assert_eq!(
                    CStr::from_ptr(bench_gl_kind(s.as_ptr())).to_str().unwrap(),
                    kind
                );
            }
        }
    }
}
