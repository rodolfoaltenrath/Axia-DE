//! Rust entry point for the isolated helper. Only the existing sandbox service
//! and the engine's not-yet-migrated renderer cross the subsystem ABI.
use super::*;
use libc::{c_char, c_int, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
const SOCK: i32 = 3;
#[repr(C)]
struct SandboxConfig {
    name: *const c_char,
    ro_paths: *const *const c_char,
    extra_syscalls: *const c_int,
    ioctls: c_int,
    mem_max_mb: libc::c_long,
    ro_fds: *const c_int,
    rw_fds: *const c_int,
    dev_files: *const *const c_char,
}
extern "C" {
    fn sandbox_apply_helper(config: *const SandboxConfig) -> c_int;
}
fn send(kind: i32, id: i32, value: f64, aux: f64) {
    let m = LbMsg {
        magic: LB_MAGIC,
        kind: kind as u32,
        id: id as u32,
        pad: 0,
        value,
        aux,
    };
    if unsafe {
        libc::send(
            SOCK,
            (&m as *const LbMsg).cast(),
            std::mem::size_of::<LbMsg>(),
            libc::MSG_NOSIGNAL,
        )
    } != std::mem::size_of::<LbMsg>() as isize
    {
        unsafe { libc::_exit(1) }
    }
}
fn skip(first: i32, last: i32, why: i32) {
    for id in first..=last {
        send(LB_MSG_SKIP, id, 0.0, why as f64)
    }
}
fn measure(id: i32, mut f: impl FnMut() -> f64) {
    send(LB_MSG_STEP, id, 0.0, 0.0);
    let mut values = [0.0; 3];
    for v in &mut values {
        *v = f();
        if !v.is_finite() || *v <= 0.0 {
            skip(id, id, LB_SKIP_FAILED);
            return;
        }
    }
    let mut spread = 0.0;
    let median = unsafe { lb_median(values.as_mut_ptr(), 3, &mut spread) };
    send(LB_MSG_RESULT, id, median, spread)
}
struct Request {
    go: LbGo,
    gpu: Option<OwnedFd>,
    disk: Option<OwnedFd>,
}
unsafe fn request(sock: i32) -> Option<Request> {
    let mut go = LbGo::default();
    let mut control = [0usize; 8];
    let mut iov = libc::iovec {
        iov_base: (&mut go as *mut LbGo).cast(),
        iov_len: std::mem::size_of::<LbGo>(),
    };
    let mut msg: libc::msghdr = std::mem::zeroed();
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    let n = libc::recvmsg(sock, &mut msg, libc::MSG_CMSG_CLOEXEC | libc::MSG_TRUNC);
    let mut fds = Vec::new();
    let mut c = libc::CMSG_FIRSTHDR(&msg);
    while !c.is_null() {
        if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
            let count = ((*c).cmsg_len - libc::CMSG_LEN(0) as usize) / std::mem::size_of::<i32>();
            for i in 0..count {
                fds.push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(
                    libc::CMSG_DATA(c).cast::<i32>().add(i),
                )))
            }
        }
        c = libc::CMSG_NXTHDR(&msg, c)
    }
    if n != std::mem::size_of::<LbGo>() as isize
        || msg.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0
        || go.magic != LB_MAGIC
        || !matches!(go.mode, 1..=3)
        || go.flags & !7 != 0
        || go.storage_mb > 1024
    {
        return None;
    }
    let want_gpu = go.flags & LB_GO_GPU as u32 != 0;
    let want_disk = go.flags & LB_GO_STORAGE as u32 != 0;
    if fds.len() != want_gpu as usize + want_disk as usize {
        return None;
    }
    if (go.mode != LB_MODE_FULL as u32 && want_disk)
        || (go.mode == LB_MODE_LOAD as u32 && (want_gpu || want_disk))
    {
        return None;
    }
    let mut fds = fds.into_iter();
    Some(Request {
        go,
        gpu: want_gpu.then(|| fds.next().unwrap()),
        disk: want_disk.then(|| fds.next().unwrap()),
    })
}
unsafe fn sandbox(gpu: bool) -> i32 {
    let ro = [
        c"/usr/lib".as_ptr(),
        c"/usr/lib64".as_ptr(),
        c"/lib".as_ptr(),
        c"/lib64".as_ptr(),
        c"/usr/share".as_ptr(),
        c"/etc/ld.so.cache".as_ptr(),
        c"/sys/dev/char".as_ptr(),
        c"/sys/devices".as_ptr(),
        c"/sys/firmware/devicetree".as_ptr(),
        c"/dev/dri".as_ptr(),
        std::ptr::null(),
    ];
    let extra = [
        libc::SYS_getcpu as i32,
        libc::SYS_kcmp as i32,
        libc::SYS_membarrier as i32,
        libc::SYS_sched_getparam as i32,
        libc::SYS_sched_getscheduler as i32,
        libc::SYS_futex_waitv as i32,
        -1,
    ];
    let cfg = SandboxConfig {
        name: c"lordos-bench".as_ptr(),
        ro_paths: ro.as_ptr(),
        extra_syscalls: extra.as_ptr(),
        ioctls: if gpu { 5 } else { 0 },
        mem_max_mb: 2048,
        ro_fds: std::ptr::null(),
        rw_fds: std::ptr::null(),
        dev_files: std::ptr::null(),
    };
    sandbox_apply_helper(&cfg)
}
unsafe fn sandbox_tries() {
    unsafe fn put(id: i32, denied: bool, e: i32) {
        send(
            LB_MSG_SANDBOX,
            id,
            denied as i32 as f64,
            if denied && (1..4096).contains(&e) {
                e as f64
            } else {
                0.0
            },
        )
    }
    unsafe fn fd(id: i32, n: i32) {
        let e = *libc::__errno_location();
        put(id, n < 0, e);
        if n >= 0 {
            libc::close(n);
        }
    }
    *libc::__errno_location() = 0;
    fd(
        LB_SBX_SOCKET,
        libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0),
    );
    *libc::__errno_location() = 0;
    fd(
        LB_SBX_INET,
        libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0),
    );
    let argv = [c"/usr/bin/true".as_ptr(), std::ptr::null()];
    *libc::__errno_location() = 0;
    let n = libc::syscall(
        libc::SYS_execve,
        c"/usr/bin/true".as_ptr(),
        argv.as_ptr(),
        1usize,
    );
    let e = *libc::__errno_location();
    put(LB_SBX_EXEC, n < 0 && e != libc::EFAULT, e);
    *libc::__errno_location() = 0;
    let pid = libc::fork();
    let e = *libc::__errno_location();
    if pid == 0 {
        libc::_exit(0)
    }
    if pid > 0 {
        while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0
            && *libc::__errno_location() == libc::EINTR
        {}
    }
    put(LB_SBX_FORK, pid < 0, e);
    *libc::__errno_location() = 0;
    fd(
        LB_SBX_NEWFILE,
        libc::open(
            c"/tmp".as_ptr(),
            libc::O_TMPFILE | libc::O_WRONLY | libc::O_CLOEXEC,
            0o600,
        ),
    );
    *libc::__errno_location() = 0;
    fd(
        LB_SBX_READ,
        libc::open(c"/etc/passwd".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC),
    );
}
#[derive(Default)]
struct Sustained {
    first: f64,
    last: f64,
    n: usize,
}
unsafe extern "C" fn sample(rate: f64, t: f64, ctx: *mut c_void) {
    let s = &mut *(ctx as *mut Sustained);
    if s.n == 0 {
        s.first = rate
    }
    s.last = rate;
    s.n += 1;
    send(LB_MSG_SAMPLE, 0, rate, t)
}
pub fn helper_main() -> i32 {
    unsafe {
        if std::env::args_os().len() != 1 {
            eprintln!("axia-bench: launched by axia-server (no arguments)");
            return 2;
        }
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
        if libc::getppid() == 1 {
            return 1;
        }
        libc::prctl(libc::PR_SET_NAME, c"axia-bench".as_ptr());
        libc::syscall(libc::SYS_close_range, 4u32, u32::MAX, 0u32);
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        let Some(Request { go, gpu, disk }) = request(SOCK) else {
            return 1;
        };
        let cpus = libc::sysconf(libc::_SC_NPROCESSORS_ONLN).clamp(1, 256) as i32;
        let threads = if go.threads == 0 || go.threads > cpus as u32 {
            cpus
        } else {
            go.threads as i32
        };
        let full = go.mode == LB_MODE_FULL as u32;
        if go.flags & LB_GO_LOW as u32 != 0 {
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13);
        }
        std::env::set_var("MESA_SHADER_CACHE_DISABLE", "true");
        let mask = sandbox(gpu.is_some());
        sandbox_tries();
        if go.mode == LB_MODE_LOAD as u32 {
            let mut s = Sustained::default();
            if (5..=120).contains(&go.sustained_s) {
                lb_sustained(
                    threads,
                    go.sustained_s as f64,
                    1.0,
                    Some(sample),
                    (&mut s as *mut Sustained).cast(),
                )
            }
            send(LB_MSG_DONE, 0, 0.0, mask.max(0) as f64);
            return 0;
        }
        let secs = if full { 1.5 } else { 0.5 };
        for (id, f) in [
            (LB_CPU_SHA1, lb_sha256 as extern "C" fn(i32, f64) -> f64),
            (LB_CPU_MAT1, lb_matmul),
            (LB_CPU_XZ1, lb_xz),
            (LB_CPU_DATA1, lb_data),
            (LB_CPU_SHAPE1, lb_shape),
        ] {
            measure(id, || f(1, secs));
            measure(id + 1, || f(threads, secs))
        }
        measure(LB_CPU_UI1, || lb_ui(1, secs));
        measure(LB_CPU_UIN, || lb_ui(threads, secs));
        let mb = if full { 128 } else { 32 };
        measure(LB_MEM_COPY, || lb_mem_copy(threads, mb, secs));
        measure(LB_MEM_TRIAD, || lb_mem_triad(threads, mb, secs));
        measure(LB_MEM_LAT, || {
            lb_mem_latency(64, if full { 10000000 } else { 2000000 })
        });
        #[cfg(feature = "gpu")]
        {
            if let Some(gpu) = gpu.as_ref() {
                let mut info = BenchGlInfo::default();
                if lb_gpu_open(gpu.as_raw_fd(), &mut info) == 0 {
                    let kind = match bytes(info.kind.as_ptr()) {
                        b"HARDWARE GPU" => 0.0,
                        b"VIRTUAL GPU (HOST)" => 1.0,
                        _ => 2.0,
                    };
                    send(LB_MSG_GPUINFO, 0, kind, 0.0);
                    for (id, f) in [
                        (
                            LB_GPU_SCENE,
                            lb_gpu_scene as unsafe extern "C" fn(f64) -> f64,
                        ),
                        (LB_GPU_FILL, lb_gpu_fill),
                        (LB_GPU_UPLOAD, lb_gpu_upload),
                        (LB_GPU_LAYERS, lb_gpu_layers),
                    ] {
                        measure(id, || f(secs))
                    }
                    lb_gpu_close()
                } else {
                    skip(LB_GPU_SCENE, LB_GPU_LAYERS, LB_SKIP_FAILED)
                }
            } else {
                skip(LB_GPU_SCENE, LB_GPU_LAYERS, LB_SKIP_NODEV)
            }
        }
        #[cfg(not(feature = "gpu"))]
        skip(LB_GPU_SCENE, LB_GPU_LAYERS, LB_SKIP_NODEV);
        drop(gpu);
        if let Some(disk) = disk {
            if go.storage_mb >= 8 {
                send(LB_MSG_STEP, LB_DISK_SEQW, 0.0, 0.0);
                let (mut w, mut r, mut i) = (0.0, 0.0, 0.0);
                if lb_disk(
                    disk.as_raw_fd(),
                    go.storage_mb as usize,
                    3.0,
                    &mut w,
                    &mut r,
                    &mut i,
                ) == 0
                {
                    for (id, v) in [(LB_DISK_SEQW, w), (LB_DISK_SEQR, r), (LB_DISK_RAND, i)] {
                        send(LB_MSG_RESULT, id, v, 0.0)
                    }
                } else {
                    skip(LB_DISK_SEQW, LB_DISK_RAND, LB_SKIP_FAILED)
                }
            } else {
                skip(LB_DISK_SEQW, LB_DISK_RAND, LB_SKIP_NODEV)
            }
        } else {
            skip(
                LB_DISK_SEQW,
                LB_DISK_RAND,
                if full { LB_SKIP_NODEV } else { LB_SKIP_MODE },
            )
        }
        if full && (20..=1800).contains(&go.sustained_s) {
            send(LB_MSG_STEP, LB_SUS_START, 0.0, 0.0);
            let mut s = Sustained::default();
            lb_sustained(
                threads,
                go.sustained_s as f64,
                10.0,
                Some(sample),
                (&mut s as *mut Sustained).cast(),
            );
            if s.n >= 2 && s.first > 0.0 && s.last > 0.0 {
                send(LB_MSG_RESULT, LB_SUS_START, s.first, 0.0);
                send(LB_MSG_RESULT, LB_SUS_END, s.last, 0.0)
            } else {
                skip(LB_SUS_START, LB_SUS_END, LB_SKIP_FAILED)
            }
        } else {
            skip(LB_SUS_START, LB_SUS_END, LB_SKIP_MODE)
        }
        send(LB_MSG_DONE, 0, 0.0, mask.max(0) as f64);
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_jobs_and_closes_unexpected_fds() {
        unsafe {
            for flags in [0, 8, LB_GO_GPU as u32] {
                let mut sv = [0; 2];
                assert_eq!(
                    libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, sv.as_mut_ptr()),
                    0
                );
                let a = OwnedFd::from_raw_fd(sv[0]);
                let b = OwnedFd::from_raw_fd(sv[1]);
                let go = LbGo {
                    magic: LB_MAGIC,
                    mode: 1,
                    flags,
                    ..LbGo::default()
                };
                libc::send(
                    b.as_raw_fd(),
                    (&go as *const LbGo).cast(),
                    std::mem::size_of::<LbGo>(),
                    0,
                );
                let got = request(a.as_raw_fd());
                assert_eq!(got.is_some(), flags == 0);
            }
        }
    }
    #[test]
    fn unexpected_received_descriptors_are_closed() {
        unsafe {
            let tag = format!("axia-bench-rights-{}", libc::getpid());
            let name = std::ffi::CString::new(tag.clone()).unwrap();
            let original =
                OwnedFd::from_raw_fd(libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC));
            let count = || {
                std::fs::read_dir("/proc/self/fd")
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter_map(|e| std::fs::read_link(e.path()).ok())
                    .filter(|p| p.to_string_lossy().contains(&tag))
                    .count()
            };
            let mut sv = [0; 2];
            assert_eq!(
                libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, sv.as_mut_ptr()),
                0
            );
            let a = OwnedFd::from_raw_fd(sv[0]);
            let b = OwnedFd::from_raw_fd(sv[1]);
            let mut go = LbGo {
                magic: LB_MAGIC,
                mode: 1,
                ..Default::default()
            };
            let mut iov = libc::iovec {
                iov_base: (&mut go as *mut LbGo).cast(),
                iov_len: std::mem::size_of::<LbGo>(),
            };
            let mut control = [0usize; 8];
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(4) as usize;
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(4) as usize;
            std::ptr::write_unaligned(libc::CMSG_DATA(c).cast::<i32>(), original.as_raw_fd());
            assert!(libc::sendmsg(b.as_raw_fd(), &msg, 0) > 0);
            assert!(request(a.as_raw_fd()).is_none());
            assert_eq!(count(), 1);
        }
    }
}
