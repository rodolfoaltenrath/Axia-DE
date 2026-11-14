//! Sandbox checks run only for the verified system sysprobe, after sandbox_apply.
use super::*;
use serde_json::{json, Value};
use std::os::fd::FromRawFd;
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub(super) struct Header {
    pub reqid: u32,
    pub op: u16,
    pub status: u16,
    pub len: u32,
}
fn put(out: &mut Vec<Value>, id: &str, result: i64) {
    out.push(json!({"id":id,"denied":result<0,"errno":if result<0{errno()}else{0}}));
}
unsafe fn fd_try(out: &mut Vec<Value>, id: &str, fd: i32) {
    put(out, id, fd as i64);
    if fd >= 0 {
        libc::close(fd);
    }
}
unsafe fn kernel(out: &mut Vec<Value>, home: *const c_char, data: &CStr) {
    fd_try(
        out,
        "fsHome",
        libc::open(home, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC),
    );
    fd_try(
        out,
        "fsSystem",
        libc::open(c"/etc/passwd".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC),
    );
    fd_try(
        out,
        "cgroupEscape",
        libc::open(
            c"/sys/fs/cgroup/cgroup.procs".as_ptr(),
            libc::O_WRONLY | libc::O_CLOEXEC,
        ),
    );
    fd_try(
        out,
        "netInet",
        libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0),
    );
    fd_try(
        out,
        "netUnix",
        libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0),
    );
    put(
        out,
        "ptrace",
        libc::ptrace(
            libc::PTRACE_ATTACH,
            1,
            ptr::null_mut::<c_void>(),
            ptr::null_mut::<c_void>(),
        ) as i64,
    );
    put(out, "userns", libc::unshare(libc::CLONE_NEWUSER) as i64);
    put(
        out,
        "mount",
        libc::mount(
            c"none".as_ptr(),
            c"/tmp".as_ptr(),
            c"tmpfs".as_ptr(),
            0,
            ptr::null(),
        ) as i64,
    );
    let argv = [c"/usr/bin/true".as_ptr(), ptr::null()];
    let env = [ptr::null()];
    put(
        out,
        "exec",
        libc::execve(argv[0], argv.as_ptr(), env.as_ptr()) as i64,
    );
    let f = cpath(&path(data.as_ptr()).join(".probe"));
    let fd = libc::open(
        f.as_ptr(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
        0o600,
    );
    let ok = fd >= 0 && libc::write(fd, c"ok".as_ptr().cast(), 2) == 2;
    let e = errno();
    if fd >= 0 {
        libc::close(fd);
    }
    libc::unlink(f.as_ptr());
    out.push(json!({"id":"ownData","denied":!ok,"errno":if ok{0}else{e}}));
}
#[no_mangle]
pub unsafe extern "C" fn probe_helper_start(data: *const c_char, child: *mut i32) -> i32 {
    if child.is_null() || data.is_null() {
        return -1;
    }
    *child = -1;
    let mut pp = [-1; 2];
    if libc::pipe2(pp.as_mut_ptr(), libc::O_CLOEXEC) < 0 {
        return -1;
    }
    let pid = libc::fork();
    if pid == 0 {
        libc::close(pp[0]);
        let cfg = SandboxHelperCfg {
            name: c"lordos-sysprobe-helper".as_ptr(),
            ro_paths: ptr::null(),
            extra_syscalls: ptr::null(),
            allow_ioctl_classes: 0,
            mem_max_mb: 0,
            ro_dir_fds: ptr::null(),
            rw_dir_fds: ptr::null(),
            dev_files: ptr::null(),
        };
        sandbox_apply_helper(&cfg);
        let mut out = Vec::new();
        fd_try(
            &mut out,
            "helperSocket",
            libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0),
        );
        let argv = [c"/usr/bin/true".as_ptr(), ptr::null()];
        let env = [ptr::null()];
        put(
            &mut out,
            "helperExec",
            libc::execve(argv[0], argv.as_ptr(), env.as_ptr()) as i64,
        );
        let g = libc::fork();
        let e = errno();
        if g == 0 {
            libc::_exit(0);
        }
        if g > 0 {
            libc::waitpid(g, ptr::null_mut(), 0);
        }
        out.push(json!({"id":"helperFork","denied":g<0,"errno":if g<0{e}else{0}}));
        let f = cpath(&path(data).join(".probe-helper"));
        fd_try(
            &mut out,
            "helperData",
            libc::open(
                f.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_CLOEXEC,
                0o600,
            ),
        );
        let bytes = serde_json::to_vec(&out).unwrap();
        let mut off = 0;
        while off < bytes.len() {
            let n = libc::write(pp[1], bytes.as_ptr().add(off).cast(), bytes.len() - off);
            if n < 0 && errno() == libc::EINTR {
                continue;
            }
            if n <= 0 {
                libc::_exit(1);
            }
            off += n as usize;
        }
        libc::_exit(0);
    }
    libc::close(pp[1]);
    if pid < 0 {
        libc::close(pp[0]);
        return -1;
    }
    *child = pid;
    pp[0]
}
pub(super) unsafe fn ask(fd: i32, reqid: u32, op: u16, payload: &[u8]) -> i32 {
    let h = Header {
        reqid,
        op,
        status: 0xffff,
        len: payload.len() as u32,
    };
    let iov = [
        libc::iovec {
            iov_base: (&h as *const Header).cast_mut().cast(),
            iov_len: std::mem::size_of::<Header>(),
        },
        libc::iovec {
            iov_base: payload.as_ptr().cast_mut().cast(),
            iov_len: payload.len(),
        },
    ];
    let mut msg: libc::msghdr = std::mem::zeroed();
    msg.msg_iov = iov.as_ptr().cast_mut();
    msg.msg_iovlen = if payload.is_empty() { 1 } else { 2 };
    loop {
        if libc::sendmsg(fd, &msg, libc::MSG_NOSIGNAL) >= 0 {
            break;
        }
        if errno() != libc::EINTR {
            return -1;
        }
    }
    let mut buf = vec![0u8; 65536 + 12];
    for _ in 0..64 {
        let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        if n < 12 {
            return -1;
        }
        let r = ptr::read_unaligned(buf.as_ptr().cast::<Header>());
        if r.len as usize != n as usize - 12 {
            return -1;
        }
        if r.reqid == reqid {
            return r.status as i32;
        }
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn probe_main(
    home: *const c_char,
    data: *const c_char,
    mask: i32,
    helper_fd: i32,
    helper_pid: i32,
) -> i32 {
    if data.is_null() || home.is_null() {
        return 1;
    }
    let fd = std::env::var("LORD_BROKER_FD")
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|fd| *fd >= 0);
    let Some(fd) = fd else {
        return 1;
    };
    let mut out = Vec::new();
    kernel(&mut out, home, CStr::from_ptr(data));
    let mut got = Vec::new();
    if helper_fd >= 0 {
        use std::io::Read;
        let f = std::fs::File::from_raw_fd(helper_fd);
        let _ = f.take(1024).read_to_end(&mut got);
    }
    if helper_pid > 0 {
        while libc::waitpid(helper_pid, ptr::null_mut(), 0) < 0 && errno() == libc::EINTR {}
    }
    if let Ok(items) = serde_json::from_slice::<Vec<Value>>(&got) {
        out.extend(items);
    } else {
        out.push(json!({"id":"helperRan","denied":true,"errno":0}));
    }
    for (req, op, payload, id) in [
        (1, 2, &b"../../.bashrc"[..], "brokerFsEscape"),
        (2, 5, &b""[..], "brokerClipboard"),
        (3, 40, &b""[..], "brokerFido"),
        (4, 43, &b"list\0"[..], "brokerMail"),
    ] {
        let s = ask(fd, req, op, payload);
        out.push(json!({"id":id,"denied":s==1||(id=="brokerMail"&&s==2),"errno":0}));
    }
    let report = serde_json::to_vec(&json!({"v":1,"sandbox":mask,"tries":out})).unwrap();
    (ask(fd, 5, 45, &report) != 0) as i32
}
