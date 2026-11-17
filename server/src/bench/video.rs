//! Coordinator for the media subsystem's isolated video benchmark helper.
use super::*;
use libc::{c_char, c_int, c_void};
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::Path,
};

const VB_COUNT: u32 = 8;
const VBR_STEP: i64 = 0;
const VBR_RESULT: i64 = 1;
const VBR_SKIP: i64 = 2;
const MM_JOB: u32 = 1;
const MK_VBENCH: i64 = 7;
const MM_STATE: u32 = 68;
const MM_ERROR: u32 = 74;
const MM_LEVEL: u32 = 70;
const MM_BENCH: u32 = 77;
const MS_DONE: i64 = 5;
const PAYLOAD_MAX: usize = 60000;
#[repr(C)]
#[derive(Default)]
struct MediaMsg {
    kind: u32,
    id: u32,
    v: [i64; 6],
    len: u32,
    pad: u32,
}
extern "C" {
    fn mp_send(
        sock: c_int,
        kind: u32,
        id: u32,
        v: *const i64,
        payload: *const c_void,
        len: usize,
        fds: *const c_int,
        nfds: c_int,
    ) -> c_int;
    fn mp_recv(
        sock: c_int,
        m: *mut MediaMsg,
        payload: *mut c_char,
        cap: usize,
        fds: *mut c_int,
        block: c_int,
    ) -> c_int;
}
pub type LbvOn = Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, f64, f64)>;
pub type LbvTick = Option<unsafe extern "C" fn(*mut c_void) -> c_int>;
#[no_mangle]
pub extern "C" fn lbv_record_valid(id: u32, kind: i64, fps: i64, spread: i64, why: i64) -> c_int {
    (id < VB_COUNT
        && match kind {
            VBR_STEP => true,
            VBR_RESULT => fps > 0 && fps < 100_000_000 && (0..100_000_000).contains(&spread),
            VBR_SKIP => (1..=3).contains(&why),
            _ => false,
        }) as c_int
}
fn trusted(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| {
        m.is_file()
            && m.mode() & 0o100 != 0
            && m.mode() & 0o022 == 0
            && (m.uid() == 0 || m.uid() == unsafe { libc::getuid() })
    })
}
struct Child(libc::pid_t);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.0, libc::SIGKILL);
            while libc::waitpid(self.0, std::ptr::null_mut(), 0) < 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
            {}
        }
    }
}
unsafe fn spawn(path: &Path, fd: c_int) -> std::io::Result<Child> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut actions = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::uninit();
    let e = libc::posix_spawn_file_actions_init(actions.as_mut_ptr());
    if e != 0 {
        return Err(std::io::Error::from_raw_os_error(e));
    }
    let mut actions = actions.assume_init();
    let mut attr = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
    let e = libc::posix_spawnattr_init(attr.as_mut_ptr());
    if e != 0 {
        libc::posix_spawn_file_actions_destroy(&mut actions);
        return Err(std::io::Error::from_raw_os_error(e));
    }
    let mut attr = attr.assume_init();
    let result = (|| {
        let mut none = std::mem::zeroed();
        let mut all = std::mem::zeroed();
        libc::sigemptyset(&mut none);
        libc::sigfillset(&mut all);
        for e in [
            libc::posix_spawn_file_actions_adddup2(&mut actions, fd, 3),
            libc::posix_spawnattr_setsigmask(&mut attr, &none),
            libc::posix_spawnattr_setsigdefault(&mut attr, &all),
            libc::posix_spawnattr_setflags(
                &mut attr,
                (libc::POSIX_SPAWN_SETSIGMASK | libc::POSIX_SPAWN_SETSIGDEF) as libc::c_short,
            ),
        ] {
            if e != 0 {
                return Err(std::io::Error::from_raw_os_error(e));
            }
        }
        let argv = [c"axia-media".as_ptr().cast_mut(), std::ptr::null_mut()];
        let env = [
            c"LANG=C.UTF-8".as_ptr().cast_mut(),
            c"SVT_LOG=1".as_ptr().cast_mut(),
            std::ptr::null_mut(),
        ];
        let mut pid = 0;
        let e = libc::posix_spawn(
            &mut pid,
            path.as_ptr(),
            &actions,
            &attr,
            argv.as_ptr(),
            env.as_ptr(),
        );
        if e == 0 {
            Ok(Child(pid))
        } else {
            Err(std::io::Error::from_raw_os_error(e))
        }
    })();
    libc::posix_spawn_file_actions_destroy(&mut actions);
    libc::posix_spawnattr_destroy(&mut attr);
    result
}
#[no_mangle]
pub unsafe extern "C" fn lbv_run(full: c_int, on: LbvOn, tick: LbvTick, ctx: *mut c_void) -> c_int {
    let Some(on) = on else {
        return -1;
    };
    let Ok(mut path) = std::env::current_exe() else {
        return -1;
    };
    path.set_file_name("axia-media");
    if path.as_os_str().as_bytes().len() >= libc::PATH_MAX as usize || !trusted(&path) {
        return -1;
    }
    let mut pair = [-1; 2];
    if libc::socketpair(
        libc::AF_UNIX,
        libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
        0,
        pair.as_mut_ptr(),
    ) != 0
    {
        return -1;
    }
    let sock = OwnedFd::from_raw_fd(pair[0]);
    let child_sock = OwnedFd::from_raw_fd(pair[1]);
    let Ok(_child) = spawn(&path, child_sock.as_raw_fd()) else {
        return -1;
    };
    drop(child_sock);
    let sock = sock.as_raw_fd();
    let params = if full != 0 { b"full=1\n" } else { b"full=0\n" };
    if mp_send(
        sock,
        MM_JOB,
        0,
        [MK_VBENCH, 0, 0, 0, 0, 0].as_ptr(),
        params.as_ptr().cast(),
        params.len(),
        std::ptr::null(),
        0,
    ) != 0
    {
        return -1;
    }
    let deadline = bench_now_ms() + if full != 0 { 600_000. } else { 120_000. };
    let mut payload = vec![0u8; PAYLOAD_MAX + 1];
    loop {
        if tick.is_some_and(|tick| tick(ctx) != 0) {
            return -2;
        }
        if bench_now_ms() > deadline {
            return -4;
        }
        let mut poll = libc::pollfd {
            fd: sock,
            events: libc::POLLIN,
            revents: 0,
        };
        let pr = libc::poll(&mut poll, 1, 250);
        if pr < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return -3;
        }
        if pr <= 0 {
            continue;
        }
        let mut m = MediaMsg::default();
        let mut fds = [-1; 4];
        let r = mp_recv(
            sock,
            &mut m,
            payload.as_mut_ptr().cast(),
            payload.len(),
            fds.as_mut_ptr(),
            0,
        );
        if r == -1 {
            continue;
        }
        let mut unexpected = false;
        // Close every received descriptor, including malformed multi-fd records.
        for fd in fds {
            if fd >= 0 {
                drop(OwnedFd::from_raw_fd(fd));
                unexpected = true;
            }
        }
        if r <= 0 || unexpected {
            return -3;
        }
        match dispatch(&m, on, ctx) {
            Ok(true) => return 0,
            Ok(false) => {}
            Err(()) => return -3,
        }
    }
}
unsafe fn dispatch(
    m: &MediaMsg,
    on: unsafe extern "C" fn(*mut c_void, c_int, c_int, f64, f64),
    ctx: *mut c_void,
) -> Result<bool, ()> {
    if m.kind == MM_STATE && m.v[0] == MS_DONE {
        return Ok(true);
    }
    if m.kind == MM_LEVEL || m.kind == MM_ERROR {
        return Ok(false);
    }
    if m.kind != MM_BENCH || lbv_record_valid(m.id, m.v[0], m.v[1], m.v[2], m.v[3]) == 0 {
        return Err(());
    }
    let id = LB_VID_FIRST + m.id as c_int;
    match m.v[0] {
        VBR_STEP => on(ctx, LB_MSG_STEP, id, 0., 0.),
        VBR_RESULT => on(
            ctx,
            LB_MSG_RESULT,
            id,
            m.v[1] as f64 / 1000.,
            m.v[2] as f64 / 1000.,
        ),
        _ => on(
            ctx,
            LB_MSG_SKIP,
            id,
            0.,
            [0, LB_SKIP_NOCODEC, LB_SKIP_FAILED, LB_SKIP_MODE][m.v[3] as usize] as f64,
        ),
    }
    Ok(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_messages_map_to_lordbench_callbacks_and_reject_unknown_types() {
        unsafe extern "C" fn collect(
            ctx: *mut c_void,
            kind: c_int,
            id: c_int,
            value: f64,
            aux: f64,
        ) {
            (*(ctx as *mut Vec<(c_int, c_int, f64, f64)>)).push((kind, id, value, aux));
        }
        let mut got = Vec::<(c_int, c_int, f64, f64)>::new();
        let ctx = (&mut got as *mut Vec<_>).cast();
        let result = MediaMsg {
            kind: 77,
            id: 5,
            v: [1, 24500, 300, 0, 0, 0],
            ..Default::default()
        };
        unsafe {
            assert_eq!(dispatch(&result, collect, ctx), Ok(false));
            assert_eq!(got, [(LB_MSG_RESULT, LB_VID_HEVC_2160, 24.5, 0.3)]);
            assert_eq!(
                dispatch(
                    &MediaMsg {
                        kind: 68,
                        v: [5, 0, 0, 0, 0, 0],
                        ..Default::default()
                    },
                    collect,
                    ctx
                ),
                Ok(true)
            );
            assert_eq!(
                dispatch(
                    &MediaMsg {
                        kind: 70,
                        ..Default::default()
                    },
                    collect,
                    ctx
                ),
                Ok(false)
            );
            assert_eq!(
                dispatch(
                    &MediaMsg {
                        kind: 74,
                        ..Default::default()
                    },
                    collect,
                    ctx
                ),
                Ok(false)
            );
            assert_eq!(
                dispatch(
                    &MediaMsg {
                        kind: 999,
                        ..Default::default()
                    },
                    collect,
                    ctx
                ),
                Err(())
            );
        }
        assert_eq!(got.len(), 1);
    }
    #[test]
    fn media_records_accept_only_known_bounded_measurements() {
        assert_eq!(std::mem::size_of::<MediaMsg>(), 64);
        for id in 0..VB_COUNT {
            assert_eq!(lbv_record_valid(id, VBR_STEP, 0, 0, 0), 1);
            assert_eq!(lbv_record_valid(id, VBR_RESULT, 1, 0, 0), 1);
            assert_eq!(lbv_record_valid(id, VBR_SKIP, 0, 0, 3), 1);
        }
        for (id, kind, fps, spread, why) in [
            (8, 0, 0, 0, 0),
            (0, 99, 1, 0, 0),
            (0, 1, 0, 0, 0),
            (0, 1, 100_000_000, 0, 0),
            (0, 1, 1, -1, 0),
            (0, 1, 1, 100_000_000, 0),
            (0, 2, 0, 0, 0),
            (0, 2, 0, 0, 4),
        ] {
            assert_eq!(lbv_record_valid(id, kind, fps, spread, why), 0);
        }
    }
}
