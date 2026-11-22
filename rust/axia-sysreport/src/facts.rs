//! What System Information's Checks tab reads about the running session, without root:
//! axia-server's own ids from /proc/self/status, its cgroup and who owns it, the app scopes beside it.
//! `root` prefixes every path ("" for this machine).
use std::ffi::{c_char, c_int, CStr};
use std::fs;
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// -1 unknown.
    pub uid: i32,
    pub euid: i32,
    /// The unified hierarchy path, "" unknown.
    pub cgroup: String,
    /// The cgroup and its parent are root-owned (-1 unknown).
    pub leaf_root: i32,
    pub parent_root: i32,
    /// app-*.scope folders beside the leaf.
    pub app_scopes: i32,
}

fn slurp(root: &str, path: &str, max: usize) -> Option<String> {
    let b = crate::fsutil::read_file_following(&format!("{root}{path}"), max)?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// 1 root-owned, 0 not, -1 missing.
fn root_owned(root: &str, path: &str) -> i32 {
    match fs::symlink_metadata(format!("{root}/sys/fs/cgroup{path}")) {
        Ok(m) if m.is_dir() => (m.uid() == 0) as i32,
        _ => -1,
    }
}

pub fn facts(root: &str) -> Facts {
    let mut f = Facts { uid: -1, euid: -1, cgroup: String::new(), leaf_root: -1, parent_root: -1, app_scopes: 0 };
    if let Some(st) = slurp(root, "/proc/self/status", 16384) {
        if let Some(i) = st.find("\nUid:") {
            let mut w = st[i + 5..].split_ascii_whitespace();
            if let (Some(Ok(a)), Some(Ok(b))) = (w.next().map(str::parse::<i32>), w.next().map(str::parse::<i32>)) {
                f.uid = a;
                f.euid = b;
            }
        }
    }
    // "0::/system.slice/lordos.service/lordos-session-1000/server" (cgroup v2)
    if let Some(cg) = slurp(root, "/proc/self/cgroup", 4096) {
        if let Some(i) = cg.find("0::").filter(|&i| i == 0 || cg.as_bytes()[i - 1] == b'\n') {
            let z = &cg[i + 3..];
            let n = z.find('\n').unwrap_or(z.len());
            let z = &z[..n];
            if n > 0 && n < 256 && z.starts_with('/') && !z.contains("..") {
                f.cgroup = z.to_string();
            }
        }
    }
    if !f.cgroup.is_empty() {
        f.leaf_root = root_owned(root, &f.cgroup);
        let parent = f.cgroup.clone();
        if let Some(sl) = parent.rfind('/').filter(|&sl| sl != 0) {
            let parent = &parent[..sl];
            f.parent_root = root_owned(root, parent);
            if let Ok(d) = fs::read_dir(format!("{root}/sys/fs/cgroup{parent}")) {
                for e in d.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if n.starts_with("app-") && n.len() > 10 && n.ends_with(".scope") {
                        f.app_scopes += 1;
                    }
                }
            }
        }
    }
    f
}

/// The C layout of `SrFacts` in server/src/sysreport/facts.h.
#[repr(C)]
pub struct SrFacts {
    pub uid: c_int,
    pub euid: c_int,
    pub cgroup: [c_char; 256],
    pub leaf_root: c_int,
    pub parent_root: c_int,
    pub app_scopes: c_int,
}

/// # Safety
/// `f` points to an SrFacts and `root` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn sr_facts(f: *mut SrFacts, root: *const c_char) {
    let root = if root.is_null() { "" } else { CStr::from_ptr(root).to_str().unwrap_or("") };
    let r = facts(root);
    let f = &mut *f;
    f.uid = r.uid;
    f.euid = r.euid;
    f.cgroup = [0; 256];
    for (i, b) in r.cgroup.bytes().take(255).enumerate() {
        f.cgroup[i] = b as c_char;
    }
    f.leaf_root = r.leaf_root;
    f.parent_root = r.parent_root;
    f.app_scopes = r.app_scopes;
}
