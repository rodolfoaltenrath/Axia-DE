//! Filesystem and environment policy; tests supply private homes and roots.
use crate::Environment;
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn value<'a>(env: &'a Environment, key: &str) -> Option<&'a OsStr> {
    env.get(OsStr::new(key))
        .filter(|s| !s.is_empty())
        .map(OsString::as_os_str)
}

pub fn home(env: &Environment) -> io::Result<PathBuf> {
    if let Some(home) = value(env, "HOME") {
        return Ok(home.into());
    }
    let mut buffer = vec![0u8; 65536];
    let mut passwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            passwd.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code));
    }
    if found.is_null() {
        return Err(io::Error::other("HOME is unavailable"));
    }
    let directory = unsafe { std::ffi::CStr::from_ptr((*found).pw_dir) };
    Ok(Path::new(OsStr::from_bytes(directory.to_bytes())).into())
}

pub fn sockets(runtime: &Path) -> BTreeSet<OsString> {
    let Ok(entries) = fs::read_dir(runtime) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_name().as_bytes().starts_with(b"wayland-")
                && entry.metadata().is_ok_and(|m| m.file_type().is_socket())
        })
        .map(|entry| entry.file_name())
        .collect()
}

pub fn theme(env: &Environment, root: &Path) -> Option<PathBuf> {
    // Match the compositor settings location, which currently uses HOME.
    let file = File::open(home(env).ok()?.join(".config/axia/settings.json")).ok()?;
    let mut text = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut text).ok()?;
    if text.len() > 1024 * 1024 {
        return None;
    }
    let prefs: serde_json::Value = serde_json::from_slice(&text).ok()?;
    let name = prefs.get("theme")?.as_str()?;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    let bundle = root.join("themes").join(name).join("theme.bundle.js");
    let name = std::ffi::CString::new(bundle.as_os_str().as_bytes()).ok()?;
    (bundle.is_file() && unsafe { libc::access(name.as_ptr(), libc::R_OK) } == 0).then_some(bundle)
}

pub fn session_log(env: &Environment) -> io::Result<File> {
    let cache = value(env, "XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home(env)?.join(".cache"));
    let directory = cache.join("axia");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    let path = directory.join("session.log");
    match fs::symlink_metadata(&path) {
        Ok(_) => fs::rename(&path, directory.join("session.previous.log"))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
}

pub fn executable_replaced(pid: u32) -> bool {
    let executable = PathBuf::from(format!("/proc/{pid}/exe"));
    let Ok(target) = fs::read_link(&executable) else {
        return false;
    };
    let Some(installed) = target.as_os_str().as_bytes().strip_suffix(b" (deleted)") else {
        return false;
    };
    let (Ok(running), Ok(installed)) = (
        fs::metadata(executable),
        fs::metadata(Path::new(OsStr::from_bytes(installed))),
    ) else {
        return false;
    };
    (running.dev(), running.ino()) != (installed.dev(), installed.ino())
}

pub fn x_display(pid: u32, temporary: &Path, uid: u32) -> OsString {
    let Ok(entries) = fs::read_dir(temporary) else {
        return OsString::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(number) = name
            .as_bytes()
            .strip_prefix(b".X")
            .and_then(|s| s.strip_suffix(b"-lock"))
        else {
            continue;
        };
        if number.is_empty() || !number.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(lock) = entry.metadata() else {
            continue;
        };
        if lock.uid() != uid
            || fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                != Some(pid)
        {
            continue;
        }
        let socket = temporary
            .join(".X11-unix")
            .join(OsString::from_vec([b"X".as_slice(), number].concat()));
        if fs::metadata(socket).is_ok_and(|m| m.uid() == uid && m.file_type().is_socket()) {
            return OsString::from_vec([b":".as_slice(), number].concat());
        }
    }
    OsString::new()
}

pub fn update_message(env: &Environment) -> (&'static str, &'static str) {
    let language = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| value(env, key))
        .and_then(|s| s.to_str())
        .and_then(|s| s.get(..2));
    match language {
        Some("pt") => (
            "Axia DE foi atualizado",
            "Salve seu trabalho e saia e entre na sessão para usar a versão instalada.",
        ),
        Some("es") => (
            "Axia DE se ha actualizado",
            "Guarda tu trabajo y cierra e inicia sesión para usar la versión instalada.",
        ),
        _ => (
            "Axia DE was updated",
            "Save your work and log out and back in to use the installed version.",
        ),
    }
}

pub fn initialize_user_dirs(env: &Environment) -> io::Result<()> {
    let home = home(env)?;
    let config = value(env, "XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".config"));
    if config.join("user-dirs.dirs").exists() {
        return Ok(());
    }
    fs::create_dir_all(&config)?;
    let existing: [(&str, &[&str]); 6] = [
        (
            "DESKTOP",
            &[
                "Área de trabalho",
                "Área de Trabalho",
                "Desktop",
                "Escritorio",
            ],
        ),
        ("DOWNLOAD", &["Downloads", "Descargas", "Transferências"]),
        ("DOCUMENTS", &["Documentos", "Documents"]),
        ("PICTURES", &["Imagens", "Pictures", "Imágenes"]),
        ("MUSIC", &["Músicas", "Música", "Music"]),
        ("VIDEOS", &["Vídeos", "Videos"]),
    ];
    for (key, names) in existing {
        if let Some(path) = names
            .iter()
            .map(|name| home.join(name))
            .find(|p| p.is_dir())
        {
            super::supervisor::bounded(
                Command::new("xdg-user-dirs-update")
                    .args([OsStr::new("--set"), OsStr::new(key), path.as_os_str()])
                    .envs(env),
                Duration::from_secs(10),
                || Ok(()),
            )?;
        }
    }
    super::supervisor::bounded(
        Command::new("xdg-user-dirs-update").envs(env),
        Duration::from_secs(10),
        || Ok(()),
    )?;
    Ok(())
}
