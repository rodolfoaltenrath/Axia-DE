use super::{io_error, Result};
use std::ffi::{CString, OsStr};
use std::fs::{self, File};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{OnceLock, RwLock};

const PROTECTED: &[&str] = &[
    ".local/share/applications",
    ".config/autostart",
    ".local/share/axia/apps",
    ".config/axia/files.json",
    ".config/axia/settings.json",
    ".config/axia/permisos.json",
    ".config/axia/firewall.json",
    ".config/axia/fwlists.json",
    ".config/axia/fwlists.txt",
    ".cache/axia/fwlists",
    ".config/axia/backup.json",
    ".config/axia/mail",
    ".local/share/axia/mail",
    ".local/share/axia/vault",
];

#[derive(Clone)]
pub struct Policy {
    pub home: PathBuf,

    pub roots: Vec<PathBuf>,
}

fn state() -> &'static RwLock<Policy> {
    static POLICY: OnceLock<RwLock<Policy>> = OnceLock::new();
    POLICY.get_or_init(|| {
        RwLock::new(Policy::new(
            Path::new(&std::env::var("HOME").unwrap_or_default()),
            &std::env::var("USER").unwrap_or_default(),
        ))
    })
}

pub fn init(home: &Path, user: &str) {
    *state().write().unwrap_or_else(|e| e.into_inner()) = Policy::new(home, user);
}
pub fn current() -> Policy {
    state().read().unwrap_or_else(|e| e.into_inner()).clone()
}

impl Policy {
    pub fn new(home: &Path, user: &str) -> Self {
        let home = fs::canonicalize(home).unwrap_or_else(|_| home.to_owned());
        let user = if !user.is_empty() && !user.contains('/') {
            user
        } else {
            "-"
        };
        Self {
            roots: vec![
                home.clone(),
                PathBuf::from(format!("/media/{user}")),
                PathBuf::from(format!("/run/media/{user}")),
                PathBuf::from(format!("/run/user/{}/gvfs", unsafe { libc::getuid() })),
            ],
            home,
        }
    }
    pub fn allowed(&self, p: &Path) -> bool {
        p.is_absolute()
            && !p.components().any(|c| c == Component::ParentDir)
            && self
                .roots
                .iter()
                .any(|r| r.as_os_str().len() > 1 && p.starts_with(r))
    }
    pub fn readable(&self, p: &Path) -> bool {
        self.allowed(p)
            || (p.is_absolute()
                && !p.components().any(|c| c == Component::ParentDir)
                && !["/proc", "/sys", "/dev", "/run"]
                    .iter()
                    .any(|r| p.starts_with(r)))
    }
    pub fn protected(&self, p: &Path) -> bool {
        PROTECTED.iter().any(|r| p.starts_with(self.home.join(r)))
    }
    pub fn protected_below(&self, p: &Path) -> bool {
        PROTECTED.iter().any(|r| {
            let q = self.home.join(r);
            q != p && q.starts_with(p)
        })
    }
    pub fn absolute(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.into()
        } else {
            self.home.join(p)
        }
    }
    pub fn read(&self, p: &Path) -> Result<PathBuf> {
        let real = fs::canonicalize(self.absolute(p)).map_err(io_error)?;
        if !self.readable(&real) {
            return Err("fs.systemFolder".into());
        }
        Ok(real)
    }
    // Resolve only the parent: a final symlink is an object, never its target.
    pub fn target(&self, p: &Path, ancestor: bool) -> Result<PathBuf> {
        let absolute = self.absolute(p);
        if self.protected(&absolute) || (ancestor && self.protected_below(&absolute)) {
            return Err("fs.protected".into());
        }
        let name = absolute.file_name().ok_or("sys.badArgument")?;
        if name == "." || name == ".." {
            return Err("sys.badArgument".into());
        }
        let parent =
            fs::canonicalize(absolute.parent().ok_or("sys.badArgument")?).map_err(io_error)?;
        let target = parent.join(name);
        if !self.allowed(&target) {
            return Err("fs.outsideHome".into());
        }
        if self.protected(&target) || (ancestor && self.protected_below(&target)) {
            return Err("fs.protected".into());
        }
        Ok(target)
    }
}

pub fn c_name(s: &OsStr) -> std::io::Result<CString> {
    CString::new(s.as_bytes()).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))
}

pub fn open_dir(path: &Path) -> std::io::Result<File> {
    let mut fd = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => (),
            Component::Normal(name) => {
                fd = open_at(
                    &fd,
                    name,
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                    0,
                )?;
            }
            _ => return Err(std::io::Error::from_raw_os_error(libc::EINVAL)),
        }
    }
    Ok(fd)
}

pub fn open_at(
    parent: &File,
    name: &OsStr,
    flags: i32,
    mode: libc::mode_t,
) -> std::io::Result<File> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

pub fn anchored(path: &Path) -> std::io::Result<(File, CString)> {
    Ok((
        open_dir(
            path.parent()
                .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?,
        )?,
        c_name(
            path.file_name()
                .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?,
        )?,
    ))
}

pub fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    let (a, an) = anchored(from)?;
    let (b, bn) = anchored(to)?;
    let rc = unsafe {
        libc::renameat2(
            a.as_raw_fd(),
            an.as_ptr(),
            b.as_raw_fd(),
            bn.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_and_protected_ancestors() {
        let p = Policy::new(Path::new("/home/axia-test"), "axia-test");
        assert!(p.allowed(Path::new("/home/axia-test/Documents")));
        assert!(!p.allowed(Path::new("/home/axia-test-other")));
        assert!(!p.allowed(Path::new("/home/axia-test/../outside")));
        assert!(!p.readable(Path::new("/proc/self/mem")));
        assert!(p.readable(Path::new("/procs")));
        assert!(!p.readable(Path::new("/run/media/other")));
        assert!(p.protected_below(Path::new("/home/axia-test/.config")));
        assert!(p.protected(Path::new(
            "/home/axia-test/.local/share/applications/test.desktop"
        )));
    }
}
