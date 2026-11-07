use super::*;
use std::sync::{Mutex, MutexGuard};
static LOCK: Mutex<()> = Mutex::new(());
struct Test {
    _guard: MutexGuard<'static, ()>,
    runtime: PathBuf,
}
impl Test {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        axia_native_reset();
        let runtime =
            std::env::temp_dir().join(format!("axia-native-test-{}", crate::clipboard::next_id()));
        fs::create_dir_all(&runtime).unwrap();
        NATIVE.with(|n| n.borrow_mut().runtime = Some(runtime.clone()));
        Self {
            _guard: guard,
            runtime,
        }
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        axia_native_reset();
        let _ = fs::remove_dir_all(&self.runtime);
    }
}
fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) },
        0
    );
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}
fn send(fd: &OwnedFd, bytes: &[u8]) {
    assert_eq!(
        unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) },
        bytes.len() as isize
    );
}
unsafe fn text() -> Vec<u8> {
    CStr::from_ptr(axia_native_text()).to_bytes().to_vec()
}
#[test]
fn text_metadata_unicode_binary_and_channel_generations() {
    let _t = Test::new();
    unsafe {
        assert!(axia_native_set_text("Olá 👋".as_ptr(), "Olá 👋".len()));
        assert_eq!(text(), "Olá 👋".as_bytes());
        assert_eq!(axia_native_text_gen(), 1);
        axia_native_mark(c"passwords".as_ptr(), true);
        assert!(axia_native_sensitive());
        assert_eq!(CStr::from_ptr(axia_native_origin()), c"passwords");
        assert!(axia_native_set_primary(b"primary".as_ptr(), 7));
        assert_eq!(text(), "Olá 👋".as_bytes());
        let (mut len, mut gen) = (0, 0);
        assert_eq!(
            CStr::from_ptr(axia_native_primary(&mut len, &mut gen)),
            c"primary"
        );
        assert_eq!((len, gen), (7, 1));
        assert!(axia_native_set_text(b"a\0tail".as_ptr(), 6));
        assert_eq!(text(), b"a");
        assert!(!axia_native_sensitive());
        assert_eq!(axia_native_text_gen(), 2);
        assert!(!axia_native_set_text(ptr::null(), 1));
        assert_eq!(text(), b"a");
    }
}
#[test]
fn nonblocking_reader_eof_stale_epochs_and_failure_preserve_previous_copy() {
    let _t = Test::new();
    unsafe {
        axia_native_set_text(b"old".as_ptr(), 3);
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            0,
            c"browser".as_ptr(),
            true,
        );
        assert!(!reader.is_null());
        assert_eq!(axia_native_read_poll(reader), 0);
        send(&w, b"new ");
        assert_eq!(axia_native_read_poll(reader), 0);
        assert_eq!(text(), b"old");
        send(&w, "ação".as_bytes());
        drop(w);
        assert_eq!(axia_native_read_poll(reader), 1);
        axia_native_read_free(reader);
        assert_eq!(text(), "new ação".as_bytes());
        assert!(axia_native_sensitive());
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            0,
            c"old-owner".as_ptr(),
            false,
        );
        send(&w, b"late");
        axia_native_set_text(b"latest".as_ptr(), 6);
        drop(w);
        assert_eq!(axia_native_read_poll(reader), -1);
        axia_native_read_free(reader);
        assert_eq!(text(), b"latest");
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            1,
            c"bad-png".as_ptr(),
            false,
        );
        send(&w, b"not a PNG");
        drop(w);
        assert_eq!(axia_native_read_poll(reader), -1);
        axia_native_read_free(reader);
        assert!(!axia_native_is_image());
        assert_eq!(text(), b"latest");
    }
}
#[test]
fn writer_snapshots_survive_new_copies_without_a_second_payload_allocation() {
    let _t = Test::new();
    unsafe {
        let body = vec![b'x'; 1 << 20];
        assert!(axia_native_set_text(body.as_ptr(), body.len()));
        let used = MEMORY_USED.load(Ordering::Relaxed);
        let (r, w) = pipe();
        let writer = axia_native_write_new(std::os::fd::IntoRawFd::into_raw_fd(w), 0);
        assert!(!writer.is_null());
        assert_eq!(MEMORY_USED.load(Ordering::Relaxed), used);
        assert!(axia_native_set_text(b"next".as_ptr(), 4));
        let mut collected = Vec::new();
        let mut buf = [0; 8192];
        loop {
            let status = axia_native_write_poll(writer);
            loop {
                let n = libc::read(r.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len());
                if n <= 0 {
                    break;
                }
                collected.extend_from_slice(&buf[..n as usize]);
            }
            if status != 0 {
                assert_eq!(status, 1);
                break;
            }
        }
        axia_native_write_free(writer);
        assert_eq!(collected, body);
        assert_eq!(text(), b"next");
    }
}
#[test]
fn transfer_limits_close_rejected_descriptors_and_release_budget() {
    let _t = Test::new();
    unsafe {
        let mut readers = Vec::new();
        for _ in 0..32 {
            let (r, w) = pipe();
            drop(w);
            let reader = axia_native_read_new(
                std::os::fd::IntoRawFd::into_raw_fd(r),
                0,
                ptr::null(),
                false,
            );
            assert!(!reader.is_null());
            readers.push(reader);
        }
        let (r, w) = pipe();
        drop(w);
        let fd = std::os::fd::IntoRawFd::into_raw_fd(r);
        assert!(axia_native_read_new(fd, 0, ptr::null(), false).is_null());
        assert_eq!(libc::fcntl(fd, libc::F_GETFD), -1);
        for r in readers {
            axia_native_read_free(r);
        }
        assert_eq!(TRANSFERS.load(Ordering::Relaxed), 0);
        let before = MEMORY_USED.load(Ordering::Relaxed);
        {
            let mut b = Bytes::default();
            assert!(b.append(b"secret"));
            assert!(!b.reserve(LIMIT + 2));
        }
        assert_eq!(MEMORY_USED.load(Ordering::Relaxed), before);
    }
}
#[test]
fn mime_preference_and_secret_hint_follow_existing_conventions() {
    let _t = Test::new();
    unsafe {
        let m = [
            c"image/png".as_ptr(),
            c"STRING".as_ptr(),
            c"text/plain;charset=utf-8".as_ptr(),
            c"x-kde-passwordManagerHint".as_ptr(),
        ];
        assert_eq!(axia_native_mime_choose(m.as_ptr(), m.len()), 2);
        assert!(axia_native_mime_secret(m.as_ptr(), m.len()));
        assert_eq!(axia_native_mime_rank(c"text/html".as_ptr()), 0);
        assert_eq!(axia_native_mime_rank(c"UTF8_STRING".as_ptr()), 3);
    }
}
#[test]
fn primary_reader_is_independent_of_clipboard_cancellation() {
    let _t = Test::new();
    unsafe {
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            2,
            ptr::null(),
            false,
        );
        send(&w, b"selection");
        axia_native_set_text(b"clipboard".as_ptr(), 9);
        drop(w);
        assert_eq!(axia_native_read_poll(reader), 1);
        axia_native_read_free(reader);
        let (mut len, mut gen) = (0, 0);
        assert_eq!(
            CStr::from_ptr(axia_native_primary(&mut len, &mut gen)),
            c"selection"
        );
        assert_eq!(text(), b"clipboard");
    }
}

#[test]
fn x11_incr_latin1_snapshots_and_newer_copies_preserve_shared_storage() {
    let _t = Test::new();
    unsafe {
        let acc = axia_native_acc_new(false);
        assert!(!acc.is_null());
        assert!(axia_native_acc_append(acc, b"caf".as_ptr(), 3));
        assert!(axia_native_acc_append(acc, [0xe9].as_ptr(), 1));
        assert_eq!(
            axia_native_acc_finish(acc, 0, true, c"x11.fixture".as_ptr()),
            5
        );
        axia_native_acc_free(acc);
        assert_eq!(text(), "café".as_bytes());
        let before = MEMORY_USED.load(Ordering::Relaxed);
        let snapshot = axia_native_snapshot_new(0, false);
        assert!(!snapshot.is_null());
        assert_eq!(MEMORY_USED.load(Ordering::Relaxed), before);
        let mut len = 0;
        assert_eq!(
            std::slice::from_raw_parts(axia_native_snapshot_data(snapshot, &mut len), 5),
            "café".as_bytes()
        );
        assert_eq!(len, 5);
        axia_native_set_text("new 👋".as_ptr(), "new 👋".len());
        assert_eq!(
            std::slice::from_raw_parts(axia_native_snapshot_data(snapshot, &mut len), len),
            "café".as_bytes()
        );
        axia_native_snapshot_free(snapshot);
        let latin = axia_native_snapshot_new(0, true);
        let p = axia_native_snapshot_data(latin, &mut len);
        assert_eq!(std::slice::from_raw_parts(p, len), b"new ?");
        axia_native_snapshot_free(latin);
        let acc = axia_native_acc_new(false);
        axia_native_set_text(b"latest".as_ptr(), 6);
        assert!(!axia_native_acc_append(acc, b"stale".as_ptr(), 5));
        assert_eq!(axia_native_acc_finish(acc, 0, false, c"x11".as_ptr()), 0);
        axia_native_acc_free(acc);
        assert_eq!(text(), b"latest");
        assert!(axia_native_set_text([0xff, 0xe2, 0x82].as_ptr(), 3));
        let invalid = axia_native_snapshot_new(0, true);
        let p = axia_native_snapshot_data(invalid, &mut len);
        assert_eq!(std::slice::from_raw_parts(p, len), b"??");
        axia_native_snapshot_free(invalid);
        axia_native_mark(c"passwords".as_ptr(), true);
        assert!(axia_native_snapshot_new(0, false).is_null());
    }
}
#[test]
fn exact_size_and_oversized_transfers_never_publish_truncated_text() {
    let _t = Test::new();
    unsafe {
        let path =
            std::env::temp_dir().join(format!("axia-native-read-{}", crate::clipboard::next_id()));
        for extra in [0, 1] {
            fs::write(&path, vec![b'x'; LIMIT + extra]).unwrap();
            let fd = FileForTest::open(&path).unwrap();
            let reader = axia_native_read_new(
                std::os::fd::IntoRawFd::into_raw_fd(fd),
                0,
                c"large.fixture".as_ptr(),
                false,
            );
            assert!(!reader.is_null());
            let mut status = 0;
            let mut turns = 0;
            while status == 0 {
                status = axia_native_read_poll(reader);
                turns += 1;
                assert!(turns < 100);
            }
            assert_eq!(status, if extra == 0 { 1 } else { -1 });
            axia_native_read_free(reader);
            assert_eq!(CStr::from_ptr(axia_native_text()).to_bytes().len(), LIMIT);
        }
        fs::remove_file(path).unwrap();
    }
}
use std::fs::File as FileForTest;

#[test]
fn png_lifetime_secret_marker_and_runtime_previews_are_shared() {
    let t = Test::new();
    unsafe {
        let png = b"\x89PNG\r\n\x1a\nfixture bytes";
        assert!(axia_native_set_png(
            png.as_ptr(),
            png.len(),
            c"browser".as_ptr(),
            true
        ));
        assert!(axia_native_is_image());
        assert!(axia_native_image_sensitive());
        assert!(axia_native_snapshot_new(1, false).is_null());
        let path = PathBuf::from(CStr::from_ptr(axia_native_image_path()).to_str().unwrap());
        assert!(path.starts_with(&t.runtime));
        assert_eq!(fs::read(&path).unwrap(), png);
        let before = MEMORY_USED.load(Ordering::Relaxed);
        let (r, w) = pipe();
        let writer = axia_native_write_new(std::os::fd::IntoRawFd::into_raw_fd(w), 1);
        assert!(!writer.is_null());
        assert_eq!(MEMORY_USED.load(Ordering::Relaxed), before);
        assert!(axia_native_set_text(b"text after image".as_ptr(), 16));
        assert!(!axia_native_is_image());
        assert_eq!(axia_native_write_poll(writer), 1);
        let mut got = [0; 64];
        let n = libc::read(r.as_raw_fd(), got.as_mut_ptr().cast(), got.len());
        assert_eq!(&got[..n as usize], png);
        axia_native_write_free(writer);
    }
}

#[test]
fn stale_and_idle_engines_can_be_retired_without_descriptor_readiness() {
    let _t = Test::new();
    unsafe {
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            0,
            c"stalled".as_ptr(),
            false,
        );
        assert!(!axia_native_read_expired(reader));
        axia_native_invalidate(false);
        assert!(axia_native_read_expired(reader));
        axia_native_read_free(reader);
        drop(w);
        axia_native_set_text(b"copy".as_ptr(), 4);
        let (r, w) = pipe();
        let writer = axia_native_write_new(std::os::fd::IntoRawFd::into_raw_fd(w), 0);
        assert!(!axia_native_write_expired(writer));
        (*writer).progress = Instant::now() - Duration::from_secs(11);
        assert!(axia_native_write_expired(writer));
        axia_native_write_free(writer);
        drop(r);
        let (r, w) = pipe();
        let reader = axia_native_read_new(
            std::os::fd::IntoRawFd::into_raw_fd(r),
            2,
            ptr::null(),
            false,
        );
        (*reader).progress = Instant::now() - Duration::from_secs(11);
        assert!(axia_native_read_expired(reader));
        axia_native_read_free(reader);
        drop(w);
    }
}
