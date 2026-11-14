use super::*;
#[test]
fn manifest_dimensions_are_top_level_integer_pairs() {
    assert_eq!(
        client::manifest_pair(br#"{"size":[860,560],"minSize":[560,360]}"#, "size"),
        Some((860, 560))
    );
    for text in [
        r#"{"size":[159,100]}"#,
        r#"{"size":[640,99]}"#,
        r#"{"size":[8193,100]}"#,
        r#"{"other":{"size":[640,480]}}"#,
        r#"{"name":"size","x":[640,480]}"#,
        r#"{"size":[640.5,480]}"#,
        r#"{"size":[640,480,1]}"#,
        r#"{bad}"#,
    ] {
        assert_eq!(
            client::manifest_pair(text.as_bytes(), "size"),
            None,
            "{text}"
        );
    }
}
#[test]
fn buffers_are_bounded_and_frame_waiting_requires_all_configured_windows() {
    assert_eq!(client::pixels(640, 480), Some(640 * 480 * 4));
    for (w, h) in [(0, 480), (-1, 480), (i32::MAX, 8192), (640, 8193)] {
        assert_eq!(client::pixels(w, h), None);
    }
    assert!(!client::waiting([].into_iter()));
    assert!(!client::waiting([(false, true)].into_iter()));
    assert!(client::waiting([(true, true), (false, false)].into_iter()));
    assert!(!client::waiting([(true, true), (true, false)].into_iter()));
    assert_eq!(client::size(0, 0), (640, 480));
    assert_eq!(client::size(8192, 100), (8192, 100));
}
#[test]
fn titles_are_bounded_valid_utf8_and_vault_is_only_for_system_passwords() {
    let value = client::title("😀".repeat(80).as_bytes(), c"notes");
    assert!(value.len() <= 256);
    assert_eq!(value.last(), Some(&0));
    assert!(std::str::from_utf8(&value[..value.len() - 1]).is_ok());
    assert_eq!(client::title(b"", c"notes"), b"notes\0");
    assert!(spawn::wants_vault(b"passwords", true));
    assert!(!spawn::wants_vault(b"passwords", false));
    assert!(!spawn::wants_vault(b"notes", true));
}
#[test]
fn cache_header_matches_c_abi_and_rejects_changed_source_and_truncation() {
    let hash = [42; 32];
    let mut bytes = bytecode::header(12, hash, 3).to_vec();
    bytes.extend([1, 2, 3]);
    assert!(bytecode::valid(&bytes, 12, &hash));
    assert!(!bytecode::valid(&bytes, 11, &hash));
    assert!(!bytecode::valid(&bytes, 12, &[43; 32]));
    assert!(!bytecode::valid(&bytes[..bytes.len() - 1], 12, &hash));
    bytes[0] ^= 1;
    assert!(!bytecode::valid(&bytes, 12, &hash));
}
#[test]
fn cache_is_sealed_against_writes_and_resize() {
    use std::os::fd::AsRawFd;
    let fd = bytecode::sealed(12, [0; 32], b"bytecode").unwrap();
    unsafe {
        assert_eq!(libc::write(fd.as_raw_fd(), b"x".as_ptr().cast(), 1), -1);
        assert_eq!(errno(), libc::EPERM);
        assert_eq!(libc::ftruncate(fd.as_raw_fd(), 0), -1);
        assert_eq!(errno(), libc::EPERM);
    }
}
#[test]
fn child_environment_has_only_explicit_channels_and_no_display() {
    let env = spawn::child_environment(13, -1, 17);
    let values = env
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(values.contains(&"LORD_BROKER_FD=13".into()));
    assert!(values.contains(&"LORD_APPHOST_BC_FD=17".into()));
    assert!(!values
        .iter()
        .any(|s| s.starts_with("DISPLAY=") || s.starts_with("LORD_VAULT_FD=")));
}
#[test]
fn broker_probe_skips_events_and_matches_reply_id() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    unsafe {
        let mut f = [-1; 2];
        assert_eq!(
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                f.as_mut_ptr()
            ),
            0
        );
        let a = OwnedFd::from_raw_fd(f[0]);
        let b = OwnedFd::from_raw_fd(f[1]);
        let worker = std::thread::spawn(move || {
            let mut buf = [0u8; 128];
            let n = libc::recv(b.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0);
            assert_eq!(n, 25);
            let got = ptr::read_unaligned(buf.as_ptr().cast::<probe::Header>());
            assert_eq!((got.reqid, got.op, got.status, got.len), (7, 2, 0xffff, 13));
            assert_eq!(&buf[12..25], b"../../.bashrc");
            for (reqid, status) in [(0, 0), (7, 1)] {
                let h = probe::Header {
                    reqid,
                    op: 2,
                    status,
                    len: 0,
                };
                assert_eq!(
                    libc::send(
                        b.as_raw_fd(),
                        (&h as *const probe::Header).cast(),
                        12,
                        libc::MSG_NOSIGNAL
                    ),
                    12
                );
            }
        });
        assert_eq!(probe::ask(a.as_raw_fd(), 7, 2, b"../../.bashrc"), 1);
        worker.join().unwrap();
    }
}
#[test]
fn system_package_precedes_user_and_rejects_world_writable_system_folder() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = std::env::temp_dir().join(format!("axia-isolated-package-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let assets = dir.join("assets");
    let root = dir.join("packages");
    let sys = assets.join("sysapps/notes");
    fs::create_dir_all(&sys).unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(sys.join("manifest.json"), "{}").unwrap();
    fs::set_permissions(&sys, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        spawn::package(c"notes", &assets, &root),
        Some((fs::canonicalize(&sys).unwrap(), true))
    );
    fs::set_permissions(&sys, fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        spawn::package(c"notes", &assets, &root),
        Some((root.join("notes"), false))
    );
    assert!(spawn::package(c"../notes", &assets, &root).is_none());
    fs::remove_dir_all(dir).unwrap();
}
