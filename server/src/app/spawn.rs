use super::*;
use std::{
    fs,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
};
pub(super) fn package(id: &CStr, assets: &Path, root: &Path) -> Option<(PathBuf, bool)> {
    if unsafe { lpk_valid_id(id.as_ptr()) } == 0 {
        return None;
    }
    let name = OsStr::from_bytes(id.to_bytes());
    if !assets.as_os_str().is_empty() {
        if let Ok(real) = fs::canonicalize(assets.join("sysapps").join(name)) {
            if let Ok(m) = fs::metadata(&real) {
                if m.is_dir()
                    && m.mode() & 2 == 0
                    && (m.uid() == 0 || m.uid() == unsafe { libc::getuid() })
                    && real.join("manifest.json").is_file()
                {
                    return Some((real, true));
                }
            }
        }
    }
    if !root.as_os_str().is_empty() {
        let p = root.join(name);
        if p.is_dir() {
            return Some((p, false));
        }
    }
    None
}
#[no_mangle]
pub unsafe extern "C" fn app_package_dir(
    id: *const c_char,
    assets: *const c_char,
    out: *mut c_char,
    n: usize,
) -> c_int {
    if id.is_null() || out.is_null() || n == 0 {
        return -1;
    }
    let Some((p, system)) = package(CStr::from_ptr(id), &path(assets), &path(lpk_root())) else {
        return -1;
    };
    let b = p.as_os_str().as_bytes();
    if b.len() >= n {
        return -1;
    }
    ptr::copy_nonoverlapping(b.as_ptr(), out.cast(), b.len());
    *out.add(b.len()) = 0;
    system as c_int
}
pub(super) fn wants_vault(id: &[u8], system: bool) -> bool {
    system && id == b"passwords"
}
fn fd_pair(socket: bool) -> io::Result<[OwnedFd; 2]> {
    let mut f = [-1; 2];
    let r = unsafe {
        if socket {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                f.as_mut_ptr(),
            )
        } else {
            libc::pipe2(f.as_mut_ptr(), libc::O_CLOEXEC)
        }
    };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { [OwnedFd::from_raw_fd(f[0]), OwnedFd::from_raw_fd(f[1])] })
}
pub(super) fn child_environment(broker: i32, vault: i32, bc: i32) -> Vec<CString> {
    let mut vars = std::env::vars_os()
        .filter(|(k, _)| {
            ![
                "DISPLAY",
                "LORD_BROKER_FD",
                "LORD_VAULT_FD",
                "LORD_APPHOST_BC_FD",
            ]
            .iter()
            .any(|s| k == OsStr::new(s))
        })
        .filter_map(|(k, v)| {
            let mut b = k.as_bytes().to_vec();
            b.push(b'=');
            b.extend(v.as_bytes());
            CString::new(b).ok()
        })
        .collect::<Vec<_>>();
    for (name, fd) in [
        ("LORD_BROKER_FD", broker),
        ("LORD_VAULT_FD", vault),
        ("LORD_APPHOST_BC_FD", bc),
    ] {
        if fd >= 0 {
            vars.push(CString::new(format!("{name}={fd}")).unwrap());
        }
    }
    vars
}
unsafe fn write_pid(fd: i32, pid: libc::pid_t) -> bool {
    let b = std::slice::from_raw_parts(
        (&pid as *const libc::pid_t).cast::<u8>(),
        std::mem::size_of_val(&pid),
    );
    let mut off = 0;
    while off < b.len() {
        let n = libc::write(fd, b.as_ptr().add(off).cast(), b.len() - off);
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        if n <= 0 {
            return false;
        }
        off += n as usize;
    }
    true
}
#[no_mangle]
pub unsafe extern "C" fn app_spawn(id: *const c_char) -> c_int {
    if id.is_null() || lpk_valid_id(id) == 0 {
        return -1;
    }
    let mut dir = [0i8; 4096];
    let sys = app_package_dir(id, js_sys_assets(), dir.as_mut_ptr(), dir.len());
    if sys < 0 {
        return -1;
    }
    if brokersrv_new_window(id) == 0 {
        return 0;
    }
    let Ok(exe) = fs::read_link("/proc/self/exe") else {
        return -1;
    };
    let exe = cpath(&exe);
    let Ok([server, child]) = fd_pair(true) else {
        return -1;
    };
    let Ok([read, write]) = fd_pair(false) else {
        return -1;
    };
    let vault = if wants_vault(text(id), sys == 1) {
        let fd = vc_attach_app();
        (fd >= 0).then(|| OwnedFd::from_raw_fd(fd))
    } else {
        None
    };
    let bfd = super::bytecode::apphost_bc_dup_ro();
    let bc = (bfd >= 0).then(|| OwnedFd::from_raw_fd(bfd));
    let vfd = vault.as_ref().map_or(-1, AsRawFd::as_raw_fd);
    let bcfd = bc.as_ref().map_or(-1, AsRawFd::as_raw_fd);
    // Every allocation and environment lookup happens before fork. The child
    // performs only libc syscalls before exec: no allocator or inherited Rust locks.
    let env = child_environment(child.as_raw_fd(), vfd, bcfd);
    let mut ep = env.iter().map(|v| v.as_ptr()).collect::<Vec<_>>();
    ep.push(ptr::null());
    let argv = [exe.as_ptr(), c"--app".as_ptr(), id, ptr::null()];
    let mut none: libc::sigset_t = std::mem::zeroed();
    libc::sigemptyset(&mut none);
    let pid = libc::fork();
    if pid == 0 {
        libc::close(read.as_raw_fd());
        libc::close(server.as_raw_fd());
        libc::setsid();
        let g = libc::fork();
        if g != 0 {
            if g < 0 || !write_pid(write.as_raw_fd(), g) {
                libc::_exit(1);
            }
            libc::_exit(0);
        }
        libc::close(write.as_raw_fd());
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGHUP, libc::SIG_DFL);
        libc::sigprocmask(libc::SIG_SETMASK, &none, ptr::null_mut());
        for fd in [child.as_raw_fd(), vfd, bcfd] {
            if fd >= 0 && libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                libc::_exit(127);
            }
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            libc::dup2(null, 0);
            if null > 2 {
                libc::close(null);
            }
        }
        libc::execve(c"/proc/self/exe".as_ptr(), argv.as_ptr(), ep.as_ptr());
        libc::_exit(127);
    }
    drop(child);
    drop(write);
    drop(vault);
    drop(bc);
    if pid < 0 {
        return -1;
    }
    while libc::waitpid(pid, ptr::null_mut(), 0) < 0 {
        if errno() != libc::EINTR {
            return -1;
        }
    }
    let mut r = fs::File::from(read);
    let mut bytes = [0; std::mem::size_of::<libc::pid_t>()];
    if r.read_exact(&mut bytes).is_err() {
        return -1;
    }
    let app = libc::pid_t::from_ne_bytes(bytes);
    if app <= 0 {
        return -1;
    }
    if brokersrv_add(server.as_raw_fd(), id, app) != 0 {
        return -1;
    }
    if vfd >= 0 {
        brokersrv_set_vault(server.as_raw_fd());
    }
    if sys == 1 {
        brokersrv_set_system(server.as_raw_fd());
    }
    use std::os::fd::IntoRawFd;
    let _ = server.into_raw_fd();
    eprintln!(
        "axia-server: app '{}' started as an isolated process (pid {app})",
        CStr::from_ptr(id).to_string_lossy()
    );
    0
}
