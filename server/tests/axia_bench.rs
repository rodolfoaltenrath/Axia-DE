//! Axia benchmark engine and helper integration, with a Rust test driver.
#[path = "../src/bench/host.rs"]
mod host;
use axia_bench::*;
use std::{
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn fixed_scene_and_all_five_embedded_fonts() {
    unsafe {
        let (mut nodes, mut unknown) = (0, -1);
        assert_eq!(lb_ui_check(&mut nodes, &mut unknown), 0);
        assert_eq!(nodes, 529);
        assert_eq!(unknown, 0);
        let serial = lb_ui_checksum(1).unwrap();
        assert_ne!(serial, 0);
        for n in [4, 8, 1] {
            assert_eq!(
                lb_ui_checksum(n),
                Some(serial),
                "paint differs with {n} threads"
            );
        }
        for n in [1, 4, 1, 8] {
            assert!(lb_ui(n, 0.02) > 0.0)
        }
        for p in 0..5 {
            let (mut g, mut bad, mut advance) = (0, -1, 0);
            assert_eq!(lb_shape_check(p, &mut g, &mut bad, &mut advance), 0);
            assert!(g > 80);
            assert_eq!(bad, 0);
            assert!(advance > 0);
            let (mut g2, mut bad2, mut a2) = (0, 0, 0);
            assert_eq!(lb_shape_check(p, &mut g2, &mut bad2, &mut a2), 0);
            assert_eq!((g, bad, advance), (g2, bad2, a2));
        }
        assert_eq!(
            lb_shape_check(
                5,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            -1
        );
        for n in [1, 2] {
            assert!(lb_shape(n, 0.02) > 0.0)
        }
    }
}
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn coordinator_cancels_and_can_start_again() {
    // Cargo puts test executables in deps/, whereas installed consumers and the
    // helper share a directory. Reproduce that layout without changing lookup.
    let helper = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("axia-bench");
    struct Link(std::path::PathBuf);
    impl Drop for Link {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _link = if helper.exists() {
        // make also copies the harness beside build/axia-bench for existing
        // callers. That helper is a copy, so its canonical path differs.
        assert!(helper.is_file());
        None
    } else {
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_axia-bench"), &helper).unwrap();
        Some(Link(helper))
    };
    for _ in 0..2 {
        unsafe {
            assert_eq!(lbrun_start(c"quick".as_ptr(), 0, std::ptr::null()), 0);
            assert_eq!(lbrun_start(c"quick".as_ptr(), 0, std::ptr::null()), -1);
        }
        lbrun_cancel();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let p = lbrun_poll();
            assert!(!p.is_null());
            let v: serde_json::Value = unsafe {
                let text = std::ffi::CStr::from_ptr(p).to_bytes();
                let v = serde_json::from_slice(text).unwrap();
                libc::free(p.cast());
                v
            };
            if !matches!(v["state"].as_str(), Some("waiting" | "running")) {
                // A battery guard may stop a run before its first cancellation
                // check on a laptop. Both outcomes must release the run slot.
                assert!(v["state"] == "cancelled" || v["state"] == "stopped", "{v}");
                assert_eq!(v["result"]["lordbench"], 2);
                break;
            }
            assert!(Instant::now() < deadline, "cancellation deadline: {v}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(feature = "gpu")]
#[test]
fn shared_scene_renders_in_a_real_surfaceless_context() {
    unsafe {
        use libc::c_void;
        #[link(name = "EGL")]
        extern "C" {
            fn eglGetPlatformDisplay(
                platform: u32,
                native: *mut c_void,
                attrs: *const isize,
            ) -> *mut c_void;
            fn eglInitialize(d: *mut c_void, a: *mut i32, b: *mut i32) -> u32;
            fn eglBindAPI(api: u32) -> u32;
            fn eglChooseConfig(
                d: *mut c_void,
                a: *const i32,
                c: *mut *mut c_void,
                n: i32,
                count: *mut i32,
            ) -> u32;
            fn eglCreateContext(
                d: *mut c_void,
                c: *mut c_void,
                share: *mut c_void,
                a: *const i32,
            ) -> *mut c_void;
            fn eglCreatePbufferSurface(
                d: *mut c_void,
                c: *mut c_void,
                a: *const i32,
            ) -> *mut c_void;
            fn eglMakeCurrent(
                d: *mut c_void,
                a: *mut c_void,
                b: *mut c_void,
                c: *mut c_void,
            ) -> u32;
            fn eglDestroySurface(d: *mut c_void, s: *mut c_void) -> u32;
            fn eglDestroyContext(d: *mut c_void, c: *mut c_void) -> u32;
            fn eglTerminate(d: *mut c_void) -> u32;
        }
        #[link(name = "GLESv2")]
        extern "C" {
            fn glReadPixels(x: i32, y: i32, w: i32, h: i32, f: u32, t: u32, p: *mut c_void);
            fn glGetError() -> u32;
        }
        let d = eglGetPlatformDisplay(0x31dd, std::ptr::null_mut(), std::ptr::null());
        assert!(!d.is_null());
        assert_ne!(
            eglInitialize(d, std::ptr::null_mut(), std::ptr::null_mut()),
            0
        );
        assert_ne!(eglBindAPI(0x30a0), 0);
        let (mut config, mut n) = (std::ptr::null_mut(), 0);
        assert_ne!(
            eglChooseConfig(
                d,
                [0x3033, 1, 0x3040, 4, 0x3025, 16, 0x3038].as_ptr(),
                &mut config,
                1,
                &mut n
            ),
            0
        );
        assert!(n > 0);
        let context = eglCreateContext(
            d,
            config,
            std::ptr::null_mut(),
            [0x3098, 2, 0x3038].as_ptr(),
        );
        assert!(!context.is_null());
        let surface =
            eglCreatePbufferSurface(d, config, [0x3057, 128, 0x3056, 128, 0x3038].as_ptr());
        assert!(!surface.is_null());
        assert_ne!(eglMakeCurrent(d, surface, surface, context), 0);
        let mut info = BenchGlInfo::default();
        assert_eq!(bench_scene_init(&mut info), 0);
        eprintln!(
            "scene renderer: {}",
            std::ffi::CStr::from_ptr(info.renderer.as_ptr()).to_string_lossy()
        );
        assert!(bench_scene_draw(128, 128, 1000.0) >= 0.0);
        let mut pixels = vec![0u8; 128 * 128 * 4];
        glReadPixels(0, 0, 128, 128, 0x1908, 0x1401, pixels.as_mut_ptr().cast());
        assert_eq!(glGetError(), 0);
        let first = &pixels[..4];
        assert!(pixels.chunks_exact(4).filter(|p| *p != first).count() > 1000);
        bench_scene_free();
        bench_scene_free();
        eglMakeCurrent(
            d,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        eglDestroySurface(d, surface);
        eglDestroyContext(d, context);
        eglTerminate(d);
        assert_eq!(lb_gpu_open(-1, &mut info), -1);
        lb_gpu_close();
        lb_gpu_close();
    }
}
#[test]
fn real_helper_enforces_sandbox_and_finishes_load() {
    unsafe {
        let mut sv = [0; 2];
        assert_eq!(
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                sv.as_mut_ptr()
            ),
            0
        );
        let sock = OwnedFd::from_raw_fd(sv[0]);
        let peer = OwnedFd::from_raw_fd(sv[1]);
        let fd = peer.as_raw_fd();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_axia-bench"));
        cmd.stdin(Stdio::null()).stdout(Stdio::null());
        cmd.env_remove("LORDOS_SESSIOND");
        cmd.pre_exec(move || {
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
        let mut child = Process(cmd.spawn().unwrap());
        drop(peer);
        let go = LbGo {
            magic: LB_MAGIC,
            mode: LB_MODE_LOAD as u32,
            threads: 2,
            sustained_s: 5,
            ..LbGo::default()
        };
        assert_eq!(
            libc::send(
                sock.as_raw_fd(),
                (&go as *const LbGo).cast(),
                std::mem::size_of::<LbGo>(),
                libc::MSG_NOSIGNAL
            ),
            std::mem::size_of::<LbGo>() as isize
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut denied = [false; 6];
        let mut samples = 0;
        loop {
            assert!(Instant::now() < deadline, "helper deadline");
            let mut p = libc::pollfd {
                fd: sock.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if libc::poll(&mut p, 1, 250) <= 0 {
                continue;
            }
            let mut m = LbMsg::default();
            let n = libc::recv(
                sock.as_raw_fd(),
                (&mut m as *mut LbMsg).cast(),
                std::mem::size_of::<LbMsg>(),
                libc::MSG_TRUNC,
            );
            assert_eq!(
                n,
                std::mem::size_of::<LbMsg>() as isize,
                "helper exited: {:?}",
                child.0.try_wait()
            );
            assert_eq!(lb_msg_valid(&m), 1, "invalid helper record {m:?}");
            match m.kind as i32 {
                LB_MSG_SANDBOX => {
                    assert_eq!(
                        m.value, 1.0,
                        "sandbox allowed probe {} errno {}",
                        m.id, m.aux
                    );
                    denied[m.id as usize] = true
                }
                LB_MSG_SAMPLE => {
                    assert!(m.value > 0.0);
                    samples += 1
                }
                LB_MSG_DONE => {
                    assert!(m.aux as i32 & 8 != 0, "seccomp must be active");
                    break;
                }
                _ => panic!("unexpected load record"),
            }
        }
        assert!(denied.into_iter().all(|x| x));
        assert!(samples >= 4);
        assert!(child.0.wait().unwrap().success());
    }
}
