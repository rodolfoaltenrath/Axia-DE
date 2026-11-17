//! Strict, root-owned restart-test state. Tests operate only in temporary paths.
use super::{bytes, status};
use libc::{c_char, c_int};
use std::{
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::Path,
};
pub const BT_MIN: i32 = 2;
pub const BT_MAX: i32 = 10;
pub const BT_SETTLE_S: i64 = 30;
pub const BT_STALE_S: i64 = 1800;
pub const BT_NONE: i32 = 0;
pub const BT_STALE: i32 = 1;
pub const BT_LAST: i32 = 2;
pub const BT_AGAIN: i32 = 3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct BootTest {
    pub total: c_int,
    pub left: c_int,
    pub at: i64,
}
impl BootTest {
    fn valid(&self) -> bool {
        (BT_MIN..=BT_MAX).contains(&self.total)
            && (1..=self.total).contains(&self.left)
            && self.at > 0
    }
    fn encode(&self) -> Option<String> {
        self.valid().then(|| {
            format!(
                "lordos-boot-test 1\ntotal {}\nleft {}\nat {}\n",
                self.total, self.left, self.at
            )
        })
    }
    fn parse(text: &[u8]) -> Option<Self> {
        let mut rest = text.strip_prefix(b"lordos-boot-test 1\n")?;
        fn field(rest: &mut &[u8], key: &[u8], max: i64) -> Option<i64> {
            let tail = rest.strip_prefix(key)?.strip_prefix(b" ")?;
            let count = tail.iter().position(|b| !b.is_ascii_digit())?;
            if count == 0 || count > 12 || tail[count] != b'\n' {
                return None;
            }
            let n = tail[..count]
                .iter()
                .fold(0i64, |n, b| n * 10 + (b - b'0') as i64);
            if n > max {
                return None;
            }
            *rest = &tail[count + 1..];
            Some(n)
        }
        let total = field(&mut rest, b"total", BT_MAX as i64)? as c_int;
        let left = field(&mut rest, b"left", BT_MAX as i64)? as c_int;
        let at = field(&mut rest, b"at", 999_999_999_999)?;
        let value = Self { total, left, at };
        (rest.is_empty() && value.valid()).then_some(value)
    }
}
#[no_mangle]
pub unsafe extern "C" fn bt_format(b: *const BootTest, out: *mut c_char, n: u32) -> c_int {
    let Some(text) = b.as_ref().and_then(BootTest::encode) else {
        return -1;
    };
    if !out.is_null() && n > 0 {
        let take = text.len().min(n as usize - 1);
        std::ptr::copy_nonoverlapping(text.as_ptr(), out.cast(), take);
        *out.add(take) = 0;
    }
    if !out.is_null() && text.len() < n as usize {
        0
    } else {
        -1
    }
}
#[no_mangle]
pub unsafe extern "C" fn bt_parse(input: *const c_char, out: *mut BootTest) -> c_int {
    let Some(value) = BootTest::parse(bytes(input)) else {
        return -1;
    };
    let Some(out) = out.as_mut() else {
        return -1;
    };
    *out = value;
    0
}
fn read(path: &Path) -> std::io::Result<BootTest> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != 0
        || meta.mode() & 0o022 != 0
        || meta.len() == 0
        || meta.len() > 256
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "untrusted boot state",
        ));
    }
    let mut buffer = Vec::with_capacity(256);
    Read::by_ref(&mut file).take(256).read_to_end(&mut buffer)?;
    BootTest::parse(&buffer)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid boot state"))
}
fn write(path: &Path, b: &BootTest) -> std::io::Result<()> {
    let text = b.encode().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid boot state")
    })?;
    let mut tmp = path.as_os_str().as_bytes().to_vec();
    tmp.extend_from_slice(b".new");
    if tmp.len() >= 256 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "boot state path too long",
        ));
    }
    let tmp = Path::new(std::ffi::OsStr::from_bytes(&tmp));
    let _ = std::fs::remove_file(tmp);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(tmp)?;
        f.write_all(text.as_bytes())?;
        f.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        f.sync_all()?;
        let raw = std::os::fd::IntoRawFd::into_raw_fd(f);
        if unsafe { libc::close(raw) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        std::fs::rename(tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
#[no_mangle]
pub unsafe extern "C" fn bt_read(path: *const c_char, b: *mut BootTest) -> c_int {
    let Some(out) = b.as_mut() else {
        return -1;
    };
    match read(Path::new(std::ffi::OsStr::from_bytes(bytes(path)))) {
        Ok(value) => {
            *out = value;
            0
        }
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn bt_write(path: *const c_char, b: *const BootTest) -> c_int {
    let Some(b) = b.as_ref() else {
        return -1;
    };
    status(write(
        Path::new(std::ffi::OsStr::from_bytes(bytes(path))),
        b,
    ))
}
#[no_mangle]
pub unsafe extern "C" fn bt_next(
    have: c_int,
    b: *const BootTest,
    now: i64,
    k: *mut c_int,
    after: *mut BootTest,
) -> c_int {
    if have == 0 {
        return BT_NONE;
    }
    let Some(b) = b.as_ref().filter(|b| b.valid()) else {
        return BT_STALE;
    };
    if now < b.at.saturating_sub(60) || now > b.at.saturating_add(BT_STALE_S) {
        return BT_STALE;
    }
    if let Some(k) = k.as_mut() {
        *k = b.total - b.left + 1;
    }
    if b.left <= 1 {
        return BT_LAST;
    }
    if let Some(after) = after.as_mut() {
        *after = BootTest {
            left: b.left - 1,
            at: now,
            ..*b
        };
    }
    BT_AGAIN
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_format_is_strict_and_truncated_output_is_rejected() {
        let state = BootTest {
            total: 3,
            left: 2,
            at: 123456,
        };
        let s = state.encode().unwrap();
        assert_eq!(BootTest::parse(s.as_bytes()), Some(state));
        for text in [
            "lordos-boot-test 1\ntotal 2\nleft 0\nat 1\n",
            "lordos-boot-test 1\ntotal 2\nleft 2\nat -1\n",
            "lordos-boot-test 1\ntotal 11\nleft 2\nat 1\n",
            "lordos-boot-test 1\ntotal 2\nleft 2\nat 1234567890123\n",
        ] {
            assert!(BootTest::parse(text.as_bytes()).is_none());
        }
        for suffix in ["\n", " ", "ignored"] {
            assert!(BootTest::parse(format!("{s}{suffix}").as_bytes()).is_none());
        }
        let mut short = [1i8; 4];
        unsafe {
            assert_eq!(bt_format(&state, short.as_mut_ptr(), 4), -1);
        }
        assert_eq!(short[3], 0);
    }
    #[test]
    fn restarts_are_bounded_stale_tests_never_restart() {
        unsafe {
            let mut state = BootTest {
                total: 3,
                left: 3,
                at: 1000,
            };
            let mut next = state;
            let mut k = 0;
            assert_eq!(bt_next(0, &state, 1000, &mut k, &mut next), BT_NONE);
            assert_eq!(bt_next(1, &state, 939, &mut k, &mut next), BT_STALE);
            assert_eq!(bt_next(1, &state, 2801, &mut k, &mut next), BT_STALE);
            for expected in 1..=3 {
                let code = bt_next(1, &state, 1000, &mut k, &mut next);
                assert_eq!(k, expected);
                assert_eq!(code, if expected == 3 { BT_LAST } else { BT_AGAIN });
                if code == BT_AGAIN {
                    assert_eq!(next.left, state.left - 1);
                    state = next;
                }
            }
        }
    }
    #[test]
    fn files_reject_links_untrusted_modes_and_invalid_contents() {
        let dir = std::env::temp_dir().join(format!(
            "axia-boot-state-{}-{}",
            std::process::id(),
            super::super::bench_now_ms()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("boot-test");
        let state = BootTest {
            total: 2,
            left: 2,
            at: 123,
        };
        write(&path, &state).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o644);
        if unsafe { libc::geteuid() } == 0 {
            assert_eq!(read(&path).unwrap(), state);
        }
        let link = dir.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read(&link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&path, b"invalid").unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
