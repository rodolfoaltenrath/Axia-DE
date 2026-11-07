use super::*;
use desktop::{DesktopApp, Entry};
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};

const NO_CAMERA: u32 = 1 << 4;
const NO_MIC: u32 = 1 << 5;
const NO_USB: u32 = 1 << 7;
const NO_BLUETOOTH: u32 = 1 << 8;
const NO_FILES: u32 = 1 << 9;
const VIEWED: u32 = NO_CAMERA | NO_MIC | NO_USB | NO_BLUETOOTH | NO_FILES;
// Layout of sec/sandbox.h. Strings and pointer tables are fully owned by Plan
// and remain at stable addresses until after the launcher has forked.
#[repr(C)]
struct SandboxFilesCfg {
    home: *const c_char,
    runtime_dir: *const c_char,
    rw: [*const c_char; 49],
    ro: [*const c_char; 49],
    sockets: [*const c_char; 8],
    cwd: *const c_char,
    opened: *const c_char,
    opened_dev: u64,
    opened_ino: u64,
    keep_home: i32,
}
#[cfg(not(test))]
extern "C" {
    fn restrict_blocks(id: *const c_char, flags: u32) -> i32;
    fn sandbox_landlock_abi() -> i32;
    fn sandbox_files_isolate(cfg: *const SandboxFilesCfg) -> i32;
    fn sandbox_apply_files(cfg: *const SandboxFilesCfg) -> i32;
    fn sandbox_apply_scope() -> i32;
    fn app_scope_enter(id: *const c_char) -> i32;
}
extern "C" {
    fn fsops_allowed(path: *const c_char) -> i32;
    fn fsops_protected(path: *const c_char) -> i32;
    static mut environ: *mut *mut c_char;
}
// Unit tests exercise policy with deterministic host replies; existing launcher
// integration tests link the real C sandbox/restriction/cgroup implementations.
#[cfg(test)]
mod host {
    use super::*;
    thread_local! {pub static FLAGS:std::cell::Cell<u32>=const{std::cell::Cell::new(0)};}
    pub unsafe fn restrict_blocks(_: *const c_char, f: u32) -> i32 {
        FLAGS.with(|v| (v.get() & f != 0) as i32)
    }
    pub unsafe fn sandbox_landlock_abi() -> i32 {
        0
    }
    pub unsafe fn sandbox_files_isolate(_: *const SandboxFilesCfg) -> i32 {
        -1
    }
    pub unsafe fn sandbox_apply_files(_: *const SandboxFilesCfg) -> i32 {
        -1
    }
    pub unsafe fn sandbox_apply_scope() -> i32 {
        -1
    }
    pub unsafe fn app_scope_enter(_: *const c_char) -> i32 {
        0
    }
}
#[cfg(test)]
use host::*;
#[cfg(test)]
pub(super) fn test_restrictions(flags: u32) {
    host::FLAGS.with(|v| v.set(flags));
}

pub(super) fn split(exec: &[u8], e: &Entry, file: Option<&[u8]>) -> Option<(Vec<Vec<u8>>, bool)> {
    let (mut args, mut p, mut total, mut used) = (Vec::new(), 0, 0, false);
    while p < exec.len() {
        while p < exec.len() && b" \t".contains(&exec[p]) {
            p += 1;
        }
        if p == exec.len() {
            break;
        }
        // A malformed oversized command must never execute a truncated argv.
        if args.len() >= 61 {
            return None;
        }
        let (mut arg, mut quoted, mut empty, mut icon) = (Vec::new(), false, false, false);
        while p < exec.len() && (quoted || !b" \t".contains(&exec[p])) {
            if total + arg.len() + 4 >= 4096 {
                return None;
            }
            match exec[p] {
                b'"' => {
                    quoted = !quoted;
                    empty = true;
                    p += 1;
                }
                b'\\' if quoted && p + 1 < exec.len() && b"\"`$\\".contains(&exec[p + 1]) => {
                    arg.push(exec[p + 1]);
                    p += 2;
                }
                b'%' if p + 1 < exec.len() => {
                    let c = exec[p + 1];
                    p += 2;
                    match c {
                        b'%' => arg.push(b'%'),
                        b'c' => arg.extend_from_slice(value(&e.app.name)),
                        b'k' => arg.extend_from_slice(value(&e.app.file)),
                        b'i' => icon = !e.icon.is_empty(),
                        b'f' | b'F' | b'u' | b'U' if file.is_some() && !used => {
                            let f = file?;
                            if b"uU".contains(&c) && f.starts_with(b"/") {
                                arg.extend_from_slice(b"file://");
                                for b in f {
                                    if b.is_ascii_alphanumeric() || b"/-_.~".contains(b) {
                                        arg.push(*b);
                                    } else {
                                        arg.extend_from_slice(format!("%{b:02X}").as_bytes());
                                    }
                                }
                            } else {
                                arg.extend_from_slice(f);
                            }
                            used = true;
                            empty = true;
                        }
                        _ => {
                            icon = false;
                        }
                    }
                }
                b => {
                    arg.push(b);
                    p += 1;
                }
            }
            if total + arg.len() + 1 >= 4096 {
                return None;
            }
        }
        if quoted {
            return None;
        }
        if icon && arg.is_empty() {
            args.push(b"--icon".to_vec());
            args.push(e.icon.clone());
            total += e.icon.len() + 1;
        } else if !arg.is_empty() || empty {
            total += arg.len() + 1;
            args.push(arg);
        }
    }
    Some((args, used))
}
fn opens(id: &[u8], mime_type: &[u8], declared: bool) -> bool {
    if !mime::desktop_valid(id) {
        return false;
    }
    let Some(a) = desktop::find(&id[..id.len() - 8]) else {
        return false;
    };
    if value(&a.mime).is_empty() || (declared && !list_has(value(&a.mime), mime_type)) {
        return false;
    }
    let Some(e) = desktop::read(value(&a.file), &desktop::language()) else {
        return false;
    };
    if !e.is_app || e.terminal || e.hidden {
        return false;
    }
    split(&e.exec, &e, Some(b"/")).is_some_and(|(args, used)| {
        used && args
            .first()
            .is_some_and(|p| desktop::program(p, a.user != 0).is_some())
    })
}
pub(super) fn default_for(m: &[u8]) -> Option<(i32, Vec<u8>)> {
    if !mime::valid(m) {
        return None;
    }
    let files = defaults::load();
    for grp in [b"Default Applications".as_slice(), b"Added Associations"] {
        for (i, (text, user)) in files.iter().enumerate() {
            for id in mime::entries(text, grp, m) {
                if defaults::is_removed(&files, i, m, id) {
                    continue;
                }
                if grp == b"Default Applications" && *user && id.starts_with(b"lordos-") {
                    return Some((1, id.to_vec()));
                }
                if opens(id, m, false) {
                    return Some((
                        if grp == b"Default Applications" && *user {
                            1
                        } else {
                            2
                        },
                        id.to_vec(),
                    ));
                }
            }
        }
    }
    let mut caches = Vec::new();
    if let Some(h) = defaults::home() {
        caches.push(h.join(".local/share/applications/mimeinfo.cache"));
    }
    caches.push(defaults::system(b"/usr/share/applications/mimeinfo.cache"));
    for path in caches {
        if let Some(t) = defaults::read(&path) {
            for id in mime::entries(&t, b"MIME Cache", m) {
                if !defaults::is_removed(&files, files.len(), m, id) && opens(id, m, true) {
                    return Some((3, id.to_vec()));
                }
            }
        }
    }
    let mut ids: Vec<_> = desktop::list(&desktop::language())
        .into_iter()
        .filter(|a| list_has(value(&a.mime), m))
        .map(|a| value(&a.id).to_vec())
        .collect();
    ids.sort();
    for mut id in ids {
        id.extend_from_slice(b".desktop");
        if !defaults::is_removed(&files, files.len(), m, &id) && opens(&id, m, true) {
            return Some((3, id));
        }
    }
    None
}

const DENY: &[&str] = &[
    "axia",
    "lordos",
    "ssh",
    "gnupg",
    "autostart",
    "applications",
    "systemd",
    "environment.d",
    "keyrings",
    "dconf",
    "git",
    "trash",
    "mime",
    "fontconfig",
    "pulse",
    "pipewire",
    "wireplumber",
    "user-dirs.dirs",
    "mimeapps.list",
    "gtk-3.0",
    "gtk-4.0",
    "fish",
    "bash",
    "zsh",
    "nvim",
    "vim",
    "emacs",
    "nano",
    "mc",
    "flatpak",
    "containers",
    "thumbnails",
    "recently-used.xbel",
    "icons",
    "themes",
    "fonts",
    "dbus-1",
    "user-tmpfiles.d",
    "xdg-desktop-portal",
    "menus",
    "kio",
    "plasma-workspace",
    "tmux",
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "mozilla",
    "chromium",
    "google-chrome",
    "BraveSoftware",
    "Code",
    "keepassxc",
    "evolution",
    "Electron",
    "gstreamer-1.0",
    "mesa_shader_cache",
    "mesa_shader_cache_db",
    "radv_builtin_shaders",
    "bin",
    "applications-merged",
    "desktop-directories",
    "kservices5",
    "kservices6",
    "speech-dispatcher",
    "environment",
    "profile.d",
];
const SENSITIVE: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".config/systemd",
    ".local/share/systemd",
    ".local/share/dbus-1",
    ".config/environment.d",
    ".config/user-tmpfiles.d",
    ".local/bin",
    "bin",
    ".config/autostart",
    ".local/share/applications",
    ".config/axia",
    ".local/share/axia",
];
const STARTUP: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".bash_aliases",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".zlogout",
    ".xprofile",
    ".xsession",
    ".xsessionrc",
    ".xinitrc",
    ".Xresources",
    ".pam_environment",
    ".gitconfig",
    ".ssh",
    ".gnupg",
];
pub(super) fn own_dir_ok(id: &[u8]) -> bool {
    let lower = id.to_ascii_lowercase();
    !id.is_empty()
        && id.len() <= 100
        && !b".-".contains(&id[0])
        && id
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(b))
        && !lower.starts_with(b"axia")
        && !lower.starts_with(b"lordos")
        && !DENY.iter().any(|d| d.as_bytes().eq_ignore_ascii_case(id))
}
fn grant_ok(path: &Path, home: &Path) -> bool {
    if unsafe { fsops_protected(cstring(path.as_os_str().as_bytes()).as_ptr()) } != 0
        || path == home
    {
        return false;
    }
    if SENSITIVE.iter().any(|p| {
        let p = home.join(p);
        path.starts_with(&p) || p.starts_with(path)
    }) {
        return false;
    }
    if path
        .file_name()
        .is_some_and(|b| STARTUP.iter().any(|s| b.as_bytes() == s.as_bytes()))
    {
        return false;
    }
    !fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.nlink() > 1)
}
struct Plan {
    cfg: SandboxFilesCfg,
    strings: Vec<CString>,
}
impl Plan {
    fn string(&mut self, s: &[u8]) -> *const c_char {
        self.strings.push(cstring(s));
        self.strings.last().unwrap().as_ptr()
    }
    fn add(&mut self, path: &Path, rw: bool) {
        let p = path.as_os_str().as_bytes();
        if p.len() >= 4096 {
            return;
        }
        let list = if rw { &self.cfg.rw } else { &self.cfg.ro };
        if list
            .iter()
            .take_while(|p| !p.is_null())
            .any(|s| unsafe { bytes(*s) } == p)
        {
            return;
        }
        let Some(i) = list.iter().take(48).position(|p| p.is_null()) else {
            return;
        };
        let ptr = self.string(p);
        if rw {
            self.cfg.rw[i] = ptr;
        } else {
            self.cfg.ro[i] = ptr;
        }
    }
    fn new(a: &DesktopApp, file: Option<&[u8]>, cwd: &[u8], keep: bool) -> Option<Self> {
        let home = fs::canonicalize(defaults::home()?).ok()?;
        if home == Path::new("/") {
            return None;
        }
        let runtime = env("XDG_RUNTIME_DIR")
            .filter(|p| p.starts_with(b"/"))
            .and_then(|p| fs::canonicalize(Path::new(OsStr::from_bytes(&p))).ok())
            .or_else(|| {
                fs::canonicalize(format!("/run/user/{}", unsafe { libc::getuid() })).ok()
            })?;
        let nil = std::ptr::null();
        let mut p = Self {
            cfg: SandboxFilesCfg {
                home: nil,
                runtime_dir: nil,
                rw: [nil; 49],
                ro: [nil; 49],
                sockets: [nil; 8],
                cwd: nil,
                opened: nil,
                opened_dev: 0,
                opened_ino: 0,
                keep_home: keep as i32,
            },
            strings: Vec::new(),
        };
        p.cfg.home = p.string(home.as_os_str().as_bytes());
        p.cfg.runtime_dir = p.string(runtime.as_os_str().as_bytes());
        let wd = env("WAYLAND_DISPLAY")
            .filter(|w| !w.is_empty() && !w.contains(&b'/') && w.len() < 64)
            .unwrap_or_else(|| b"wayland-0".to_vec());
        p.cfg.sockets[0] = p.string(&wd);
        p.cfg.sockets[1] = p.string(b"pipewire-0");
        p.cfg.sockets[2] = p.string(b"pulse/native");
        if cwd.starts_with(b"/") {
            p.cfg.cwd = p.string(cwd);
        }
        if keep {
            return Some(p);
        }
        let id = value(&a.id);
        if own_dir_ok(id) {
            for parent in [
                ".config",
                ".cache",
                ".local",
                ".local/share",
                ".local/state",
            ] {
                let _ = fs::DirBuilder::new().mode(0o700).create(home.join(parent));
            }
            for root in [".config", ".cache", ".local/share", ".local/state"] {
                let dir = home.join(root).join(OsStr::from_bytes(id));
                let _ = fs::DirBuilder::new().mode(0o700).create(&dir);
                if fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) && grant_ok(&dir, &home) {
                    p.add(&dir, true);
                }
            }
        }
        if id == b"firefox" || id == b"firefox-esr" {
            for root in [".mozilla", ".cache/mozilla"] {
                let dir = home.join(root);
                let _ = fs::DirBuilder::new().mode(0o700).create(&dir);
                if fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) && grant_ok(&dir, &home) {
                    p.add(&dir, true);
                }
            }
        }
        for cache in [
            "fontconfig",
            "mesa_shader_cache",
            "mesa_shader_cache_db",
            "radv_builtin_shaders",
            "gstreamer-1.0",
        ] {
            p.add(&home.join(".cache").join(cache), false);
        }
        for look in [
            ".config/gtk-3.0",
            ".config/gtk-4.0",
            ".config/fontconfig",
            ".config/dconf",
            ".config/mimeapps.list",
            ".config/user-dirs.dirs",
            ".config/user-dirs.locale",
            ".config/kdeglobals",
            ".config/Trolltech.conf",
            ".config/qt5ct",
            ".config/qt6ct",
            ".config/pulse",
            ".local/share/fonts",
            ".local/share/icons",
            ".local/share/themes",
            ".local/share/mime",
            ".local/share/applications",
            ".fonts",
            ".icons",
            ".themes",
        ] {
            p.add(&home.join(look), false);
        }
        if let Some(file) = file {
            let path = Path::new(OsStr::from_bytes(file));
            let m = fs::metadata(path).ok()?;
            p.add(path, grant_ok(path, &home));
            p.cfg.opened = p.string(file);
            p.cfg.opened_dev = m.dev();
            p.cfg.opened_ino = m.ino();
        }
        Some(p)
    }
}
thread_local! {static ERROR:RefCell<CString>=RefCell::new(CString::default());}
fn error(s: &[u8]) {
    ERROR.with(|e| *e.borrow_mut() = cstring(s));
}
#[no_mangle]
pub extern "C" fn apps_last_error() -> *const c_char {
    ERROR.with(|e| e.borrow().as_ptr())
}

fn launch(id: &[u8], file: Option<&[u8]>, action: Option<&[u8]>) -> i32 {
    error(b"");
    if file.is_some() && action.is_some() {
        return -1;
    }
    let Some(a) = desktop::find(id) else {
        return -1;
    };
    let Some(e) = desktop::read(value(&a.file), &desktop::language()) else {
        return -1;
    };
    if !e.is_app || e.terminal || e.hidden || (file.is_some() && value(&e.app.mime).is_empty()) {
        return -1;
    }
    let exec = match action {
        Some(name) => match e.action(name) {
            Some(a) => a.exec.as_slice(),
            None => return -1,
        },
        None => &e.exec,
    };
    let Some((args, used)) = split(exec, &e, file) else {
        return -1;
    };
    if file.is_some() && !used {
        return -1;
    }
    let Some(prog) = args.first().and_then(|p| desktop::program(p, a.user != 0)) else {
        return -1;
    };
    let fwid = cstring(&[b"desktop:".as_slice(), id].concat());
    let restrictions = unsafe { restrict_blocks(fwid.as_ptr(), VIEWED) } != 0;
    if restrictions && unsafe { sandbox_landlock_abi() } < 6 {
        error(b"apps.restrictKernel");
        return -1;
    }
    let plan = if restrictions {
        match Plan::new(
            &a,
            file,
            &e.cwd,
            unsafe { restrict_blocks(fwid.as_ptr(), NO_FILES) } == 0,
        ) {
            Some(p) => Some(p),
            None => return -1,
        }
    } else {
        None
    };
    // All Rust allocations, lookups and environment reads happen BEFORE fork.
    // The child uses libc and the existing C sandbox/cgroup entrypoints only.
    let prog = cstring(&prog);
    let id = cstring(id);
    let args: Vec<_> = args.iter().map(|a| cstring(a)).collect();
    let mut argv: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
    argv.push(std::ptr::null());
    let cwd = cstring(if e.cwd.starts_with(b"/") { &e.cwd } else { b"" });
    let home = cstring(&env("HOME").unwrap_or_default());
    let strict = env("LORDOS_SESSIOND").is_some();
    let log = defaults::home().map(|h| {
        let cache = h.join(".cache/axia");
        let _ = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&cache);
        let path = cache.join("apps.log");
        let truncate = fs::symlink_metadata(&path).is_ok_and(|m| m.len() > 1024 * 1024);
        (cstring(path.as_os_str().as_bytes()), truncate)
    });
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            libc::setsid();
            if libc::fork() != 0 {
                libc::_exit(0);
            }
            let mut none = std::mem::zeroed();
            libc::sigemptyset(&mut none);
            libc::sigprocmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
            for s in 1..32 {
                libc::signal(s, libc::SIG_DFL);
            }
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if null >= 0 {
                for d in 0..3 {
                    libc::dup2(null, d);
                }
            }
            if let Some((log, truncate)) = &log {
                let fd = libc::open(
                    log.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_APPEND
                        | libc::O_NOFOLLOW
                        | if *truncate { libc::O_TRUNC } else { 0 },
                    0o600,
                );
                if fd >= 0 {
                    libc::dup2(fd, 1);
                    libc::dup2(fd, 2);
                }
            }
            libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);
            if cwd.as_bytes().is_empty() || libc::chdir(cwd.as_ptr()) != 0 {
                if !home.as_bytes().is_empty() {
                    libc::chdir(home.as_ptr());
                }
            }
            if id.as_bytes() == b"lordos-rclone"
                || id.as_bytes() == b"lordos-rdesk"
                || (app_scope_enter(id.as_ptr()) != 0 && strict)
            {
                libc::_exit(126);
            }
            if let Some(p) = &plan {
                libc::unsetenv(c"DISPLAY".as_ptr());
                if sandbox_files_isolate(&p.cfg) != 0
                    || (if p.cfg.keep_home != 0 {
                        sandbox_apply_scope()
                    } else {
                        sandbox_apply_files(&p.cfg)
                    }) != 0
                {
                    libc::_exit(126);
                }
            }
            libc::execve(prog.as_ptr(), argv.as_ptr(), environ.cast());
            libc::_exit(127);
        }
        if pid < 0 {
            return -1;
        }
        while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0 {
            if *libc::__errno_location() != libc::EINTR {
                return -1;
            }
        }
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn apps_launch(id: *const c_char) -> i32 {
    launch(bytes(id), None, None)
}
#[no_mangle]
pub unsafe extern "C" fn apps_launch_action(id: *const c_char, action: *const c_char) -> i32 {
    error(b"");
    if !desktop::action_valid(bytes(action)) {
        return -1;
    }
    launch(bytes(id), None, Some(bytes(action)))
}
#[no_mangle]
pub unsafe extern "C" fn apps_launch_with(id: *const c_char, file: *const c_char) -> i32 {
    let Ok(real) = fs::canonicalize(path(file)) else {
        return -1;
    };
    let real = real.as_os_str().as_bytes();
    if fsops_allowed(cstring(real).as_ptr()) == 0 {
        return -1;
    }
    let Ok(m) = fs::metadata(Path::new(OsStr::from_bytes(real))) else {
        return -1;
    };
    if !m.is_file() && !m.is_dir() {
        return -1;
    }
    let mut chosen = bytes(id).to_vec();
    if !id.is_null() && chosen.is_empty() {
        let mut mime = [0; 128];
        if defaults::defaults_guess(cstring(real).as_ptr(), mime.as_mut_ptr(), mime.len()) != 0 {
            return -1;
        }
        let Some((_, mut def)) = default_for(value(&mime)) else {
            return -1;
        };
        if def.starts_with(b"lordos-") {
            return -1;
        }
        def.truncate(def.len() - 8);
        chosen = def;
    }
    launch(&chosen, Some(real), None)
}
#[no_mangle]
pub unsafe extern "C" fn apps_launch_uri(id: *const c_char, uri: *const c_char) -> i32 {
    let uri = bytes(uri);
    if !(7..=300).contains(&uri.len())
        || !uri
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b":/.-_@[]".contains(b))
    {
        return -1;
    }
    for scheme in [b"http".as_slice(), b"https"] {
        let prefix = [scheme, b"://"].concat();
        if let Some(host) = uri.strip_prefix(prefix.as_slice()) {
            if host.is_empty() || host[0] == b'-' {
                return -1;
            }
            let Some(a) = desktop::find(bytes(id)) else {
                return -1;
            };
            if !list_has(
                value(&a.mime),
                &[b"x-scheme-handler/".as_slice(), scheme].concat(),
            ) {
                return -1;
            }
            return launch(bytes(id), Some(uri), None);
        }
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn apps_default_for(m: *const c_char, out: *mut c_char, n: usize) -> i32 {
    if !mime::valid(bytes(m)) || n < 160 || out.is_null() {
        return -1;
    }
    match default_for(bytes(m)) {
        Some((kind, id)) => {
            output(out, n, &id);
            kind
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_plan_contract_child() {
        if std::env::var_os("AXIA_APPLICATIONS_TEST_ROOT").is_none() {
            return;
        }
        let home = defaults::home().unwrap();
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(home.join(".ssh")).unwrap();
        extern "C" {
            fn fsops_init(home: *const c_char, user: *const c_char);
        }
        unsafe {
            fsops_init(
                cstring(home.as_os_str().as_bytes()).as_ptr(),
                c"axia-test".as_ptr(),
            );
        }
        let a = DesktopApp {
            id: field(b"org.example.Editor"),
            ..DesktopApp::default()
        };
        // A symlink named after the app must not grant its target for writing.
        let config = home.join(".config");
        fs::create_dir_all(&config).unwrap();
        std::os::unix::fs::symlink(home.join(".ssh"), config.join("org.example.Editor")).unwrap();
        let file = home.join("document");
        fs::write(&file, b"document").unwrap();
        let plan = Plan::new(
            &a,
            Some(file.as_os_str().as_bytes()),
            home.as_os_str().as_bytes(),
            false,
        )
        .unwrap();
        let strings = |list: &[*const c_char]| {
            list.iter()
                .take_while(|p| !p.is_null())
                .map(|p| unsafe { bytes(*p).to_vec() })
                .collect::<Vec<_>>()
        };
        let rw = strings(&plan.cfg.rw);
        let ro = strings(&plan.cfg.ro);
        let sockets = strings(&plan.cfg.sockets);
        assert!(rw.contains(&file.as_os_str().as_bytes().to_vec()));
        assert!(!rw.contains(
            &config
                .join("org.example.Editor")
                .as_os_str()
                .as_bytes()
                .to_vec()
        ));
        assert!(!rw.contains(&home.as_os_str().as_bytes().to_vec()));
        assert!(ro.contains(
            &home
                .join(".cache/fontconfig")
                .as_os_str()
                .as_bytes()
                .to_vec()
        ));
        assert!(sockets.contains(&b"pipewire-0".to_vec()));
        assert!(sockets.contains(&b"pulse/native".to_vec()));
        assert!(!sockets.contains(&b"bus".to_vec()));
        assert!(!sockets.contains(&b"pipewire-0-manager".to_vec()));
        let m = fs::metadata(&file).unwrap();
        assert_eq!(plan.cfg.opened_dev, m.dev());
        assert_eq!(plan.cfg.opened_ino, m.ino());
        // A second name of a startup file is read-only, too.
        let startup = home.join(".bashrc");
        fs::write(&startup, b"preserve").unwrap();
        let alias = home.join("innocent-name");
        fs::hard_link(&startup, &alias).unwrap();
        assert!(!grant_ok(&alias, &home));
        assert!(!grant_ok(&config, &home));
        let keep = Plan::new(&a, None, b"", true).unwrap();
        assert_eq!(keep.cfg.keep_home, 1);
        assert!(strings(&keep.cfg.rw).is_empty());
    }
}
