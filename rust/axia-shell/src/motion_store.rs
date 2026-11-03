//! User-scoped catalog downloads. Network I/O runs off the compositor thread.
//! HTTPS is supplied by Fedora's curl; SHA-256 by the existing C crypto primitive.
use std::{
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    fs::{self, OpenOptions},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, OnceLock,
    },
};
type CheckHash = unsafe extern "C" fn(*const c_char, *const c_char) -> bool;
type PrepareChild = unsafe extern "C" fn(u32) -> bool;
struct Job {
    result: Mutex<Option<Result<PathBuf, ()>>>,
    temp: PathBuf,
    size: u64,
}
static JOBS: OnceLock<Mutex<HashMap<u32, Arc<Job>>>> = OnceLock::new();
static NEXT: AtomicU32 = AtomicU32::new(1);
fn jobs() -> &'static Mutex<HashMap<u32, Arc<Job>>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn sha_ok(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
unsafe fn string(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        CStr::from_ptr(p).to_str().ok().map(str::to_owned)
    }
}
fn root(home: &str, data: Option<&str>) -> PathBuf {
    data.filter(|s| Path::new(s).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share"))
        .join("axia/motion")
}
unsafe fn checked(path: &Path, sha: &str, size: u64, check: CheckHash) -> bool {
    if fs::metadata(path)
        .map(|m| m.is_file() && m.len() == size)
        .unwrap_or(false)
    {
        if let (Ok(p), Ok(s)) = (
            CString::new(path.to_string_lossy().as_bytes()),
            CString::new(sha),
        ) {
            return check(p.as_ptr(), s.as_ptr());
        }
    }
    false
}
#[no_mangle]
pub unsafe extern "C" fn axia_motion_download(
    url: *const c_char,
    sha: *const c_char,
    size: u64,
    home: *const c_char,
    data: *const c_char,
    check: Option<CheckHash>,
    prepare: Option<PrepareChild>,
) -> u32 {
    let (Some(url), Some(sha), Some(home), Some(check), Some(prepare)) =
        (string(url), string(sha), string(home), check, prepare)
    else {
        return 0;
    };
    if !url.starts_with("https://")
        || !sha_ok(&sha)
        || size == 0
        || size > 100 * 1024 * 1024
        || !Path::new(&home).is_absolute()
    {
        return 0;
    }
    let data = string(data);
    let dir = root(&home, data.as_deref());
    if fs::create_dir_all(&dir).is_err()
        || fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).is_err()
    {
        return 0;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
        return 0;
    }
    let temp = dir.join(format!(".download-{}-{id}", std::process::id()));
    let final_path = dir.join(format!("{sha}.mp4"));
    let job = Arc::new(Job {
        result: Mutex::new(None),
        temp: temp.clone(),
        size,
    });
    {
        let mut all = jobs().lock().unwrap_or_else(|e| e.into_inner());
        if all.len() >= 8
            || all
                .values()
                .filter(|j| j.result.lock().unwrap_or_else(|e| e.into_inner()).is_none())
                .count()
                >= 2
        {
            return 0;
        }
        all.insert(id, job.clone());
    }
    if std::thread::Builder::new()
        .name("axia-wallpaper-download".into())
        .spawn(move || {
            let result = (|| {
                if unsafe { checked(&final_path, &sha, size, check) } {
                    return Ok(final_path);
                }
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temp)
                    .map_err(|_| ())?;
                let owner = std::process::id();
                let mut command = Command::new("curl");
                // Only the C async-signal-safe reset callback runs after fork.
                unsafe {
                    command.pre_exec(move || {
                        if prepare(owner) {
                            Ok(())
                        } else {
                            Err(std::io::Error::from_raw_os_error(4))
                        }
                    });
                }
                let status = command
                    .args([
                        "--disable",
                        "--fail",
                        "--location",
                        "--silent",
                        "--proto",
                        "=https",
                        "--proto-redir",
                        "=https",
                        "--connect-timeout",
                        "10",
                        "--max-time",
                        "120",
                        "--max-filesize",
                        &size.to_string(),
                        &url,
                    ])
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .stdout(Stdio::from(file))
                    .status()
                    .map_err(|_| ())?;
                if !status.success() || !unsafe { checked(&temp, &sha, size, check) } {
                    return Err(());
                }
                fs::rename(&temp, &final_path).map_err(|_| ())?;
                Ok(final_path)
            })();
            let _ = fs::remove_file(temp);
            *job.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        })
        .is_err()
    {
        jobs().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        return 0;
    }
    id
}
// -1 failed, 0 pending, 1 ready. Terminal results are consumed. No JS callbacks on workers.
#[no_mangle]
pub unsafe extern "C" fn axia_motion_poll(
    id: u32,
    out: *mut c_char,
    len: usize,
    percent: *mut u32,
) -> i32 {
    if out.is_null() || percent.is_null() {
        return -1;
    }
    let mut all = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = all.get(&id).cloned() else {
        return -1;
    };
    let mut result = job.result.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(result) = result.take() {
        all.remove(&id);
        if let Ok(path) = result {
            if let Some(path) = path.to_str() {
                if path.len() + 1 <= len {
                    ptr::copy_nonoverlapping(path.as_ptr(), out.cast(), path.len());
                    *out.add(path.len()) = 0;
                    *percent = 100;
                    return 1;
                }
            }
        }
        return -1;
    }
    *percent = (fs::metadata(&job.temp)
        .map(|m| m.len().saturating_mul(100) / job.size)
        .unwrap_or(0)
        .min(99)) as u32;
    0
}
#[no_mangle]
pub unsafe extern "C" fn axia_motion_remove(
    sha: *const c_char,
    home: *const c_char,
    data: *const c_char,
) -> bool {
    let (Some(sha), Some(home)) = (string(sha), string(home)) else {
        return false;
    };
    if !sha_ok(&sha) || !Path::new(&home).is_absolute() {
        return false;
    }
    let data = string(data);
    let path = root(&home, data.as_deref()).join(format!("{sha}.mp4"));
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}
// Only exact hash-named entries in this user's canonical store may bypass the
// ordinary home/mount media boundary (XDG_DATA_HOME can legitimately be elsewhere).
#[no_mangle]
pub unsafe extern "C" fn axia_motion_path_allowed(
    path: *const c_char,
    home: *const c_char,
    data: *const c_char,
) -> i32 {
    let (Some(path), Some(home)) = (string(path), string(home)) else {
        return 0;
    };
    let data = string(data);
    managed_path(Path::new(&path), &home, data.as_deref()) as i32
}
fn managed_path(path: &Path, home: &str, data: Option<&str>) -> bool {
    let Some(name) = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_suffix(".mp4"))
    else {
        return false;
    };
    sha_ok(name) && root(home, data).canonicalize().ok().as_deref() == path.parent()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_data_root_accepts_only_its_hash_named_entries() {
        let data = std::env::temp_dir().join(format!("axia-motion-root-{}", std::process::id()));
        let store = root("/home/u", data.to_str());
        fs::create_dir_all(&store).unwrap();
        let canonical = store.canonicalize().unwrap();
        assert!(managed_path(
            &canonical.join(format!("{}.mp4", "a".repeat(64))),
            "/home/u",
            data.to_str()
        ));
        assert!(!managed_path(
            &canonical.join("arbitrary.mp4"),
            "/home/u",
            data.to_str()
        ));
        assert!(!managed_path(
            &data.join(format!("{}.mp4", "a".repeat(64))),
            "/home/u",
            data.to_str()
        ));
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn hash_names_cannot_escape_storage() {
        for bad in ["../a", "", &"a".repeat(63), &"G".repeat(64)] {
            assert!(!sha_ok(bad));
        }
        assert!(sha_ok(&"a".repeat(64)));
        assert_eq!(
            root("/home/u", Some("relative")),
            PathBuf::from("/home/u/.local/share/axia/motion")
        );
    }
    #[test]
    fn missing_job_fails_without_touching_output() {
        let mut out = [0i8; 16];
        let mut pct = 0;
        assert_eq!(
            unsafe { axia_motion_poll(u32::MAX, out.as_mut_ptr(), out.len(), &mut pct) },
            -1
        );
    }
}
