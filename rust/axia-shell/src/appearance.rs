//! External toolkit integration. No polling, rendering, subprocesses or extra
//! services: resolve on appearance changes and atomically merge only our keys.
use std::env;
use std::ffi::{c_char, CStr};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SETTINGS: u64 = 1 << 20;
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn absolute_var(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn data_roots() -> Vec<PathBuf> {
    let home = absolute_var("HOME");
    let mut roots = Vec::new();
    if let Some(data) =
        absolute_var("XDG_DATA_HOME").or_else(|| home.as_ref().map(|p| p.join(".local/share")))
    {
        roots.push(data.join("themes"));
    }
    if let Some(home) = home {
        roots.push(home.join(".themes"));
    }
    let dirs = env::var_os("XDG_DATA_DIRS")
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    roots.extend(
        env::split_paths(&dirs)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("themes")),
    );
    roots
}

fn theme_name(dark: bool, roots: &[PathBuf]) -> &'static str {
    let name = if dark { "Axia-dark" } else { "Axia" };
    if roots
        .iter()
        .any(|root| root.join(name).join("gtk-3.0/gtk.css").is_file())
    {
        name
    } else if dark {
        "Adwaita-dark"
    } else {
        "Adwaita"
    }
}

/// Preserve other groups, comments and app preferences. Replace all occurrences
/// of an owned key (including duplicates) so GLib cannot read a stale value.
fn set_key(source: &str, group: &str, key: &str, value: &str) -> String {
    let mut result =
        String::with_capacity(source.len() + group.len() + key.len() + value.len() + 8);
    let mut inside = false;
    let mut found_group = false;
    let mut written = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if inside && !written {
                if !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(&format!("{key}={value}\n"));
                written = true;
            }
            inside = &trimmed[1..trimmed.len() - 1] == group;
            found_group |= inside;
        }
        if inside
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            if !written {
                result.push_str(&format!("{key}={value}\n"));
                written = true;
            }
        } else {
            result.push_str(line);
        }
    }
    if !written {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        if !found_group {
            result.push_str(&format!("[{group}]\n"));
        }
        result.push_str(&format!("{key}={value}\n"));
    }
    result
}

fn merge(source: &str, dark: bool, theme: &str, accent: &str, buttons: &str) -> String {
    let group = "org/gnome/desktop/interface";
    let s = set_key(source, group, "gtk-theme", &format!("'{theme}'"));
    let s = set_key(
        &s,
        group,
        "color-scheme",
        if dark { "'prefer-dark'" } else { "'default'" },
    );
    let s = set_key(&s, group, "accent-color", &format!("'{accent}'"));
    set_key(
        &s,
        "org/gnome/desktop/wm/preferences",
        "button-layout",
        &format!("'{buttons}'"),
    )
}

fn write_settings(
    config: &Path,
    dark: bool,
    theme: &str,
    accent: &str,
    buttons: &str,
) -> io::Result<()> {
    let dir = config.join("glib-2.0/settings");
    let path = dir.join("keyfile");
    let mut source = String::new();
    match File::open(&path) {
        Ok(file) => {
            // Reject unreadable, oversized or invalid UTF-8 input instead of
            // replacing a user's preferences with an empty configuration.
            file.take(MAX_SETTINGS + 1).read_to_string(&mut source)?;
            if source.len() as u64 > MAX_SETTINGS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "settings file too large",
                ));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let merged = merge(&source, dark, theme, accent, buttons);
    if merged == source {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let temp = dir.join(format!(
        ".axia-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = file
        .write_all(merged.as_bytes())
        .and_then(|_| fs::rename(&temp, &path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[no_mangle]
pub extern "C" fn axia_appearance_theme(dark: bool) -> *const c_char {
    // Only static, NUL-terminated names cross FFI; C never has to free them.
    match theme_name(dark, &data_roots()) {
        "Axia" => c"Axia".as_ptr(),
        "Axia-dark" => c"Axia-dark".as_ptr(),
        "Adwaita" => c"Adwaita".as_ptr(),
        _ => c"Adwaita-dark".as_ptr(),
    }
}

// SAFETY: C supplies non-null NUL-terminated strings for this call only. Names
// are restricted before interpolation into the GVariant text keyfile.
#[no_mangle]
pub unsafe extern "C" fn axia_appearance_write(
    dark: bool,
    theme: *const c_char,
    accent: *const c_char,
    buttons: *const c_char,
) -> i32 {
    if theme.is_null() || accent.is_null() || buttons.is_null() {
        return -1;
    }
    let values = [theme, accent, buttons].map(|p| CStr::from_ptr(p).to_str());
    let [Ok(theme), Ok(accent), Ok(buttons)] = values else {
        return -1;
    };
    if !matches!(theme, "Axia" | "Axia-dark" | "Adwaita" | "Adwaita-dark")
        || accent.len() > 32
        || !accent.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        || buttons.len() > 64
        || !buttons
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b',' || b == b':')
    {
        return -1;
    }
    let Some(config) =
        absolute_var("XDG_CONFIG_HOME").or_else(|| absolute_var("HOME").map(|p| p.join(".config")))
    else {
        return -1;
    };
    match write_settings(&config, dark, theme, accent, buttons) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("appearance: could not update toolkit settings: {error}");
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = env::temp_dir().join(format!(
                "axia-appearance-test-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn theme_discovery_handles_install_roots_and_missing_variants() {
        let temp = Temp::new();
        assert_eq!(theme_name(true, &[temp.0.clone()]), "Adwaita-dark");
        let css = temp.0.join("Axia/gtk-3.0/gtk.css");
        fs::create_dir_all(css.parent().unwrap()).unwrap();
        fs::write(css, "/* theme */").unwrap();
        assert_eq!(theme_name(false, &[temp.0.clone()]), "Axia");
        assert_eq!(theme_name(true, &[temp.0.clone()]), "Adwaita-dark");
        let css = temp.0.join("Axia-dark/gtk-3.0/gtk.css");
        fs::create_dir_all(&css).unwrap(); // A directory is not a usable theme.
        assert_eq!(theme_name(true, &[temp.0.clone()]), "Adwaita-dark");
    }

    #[test]
    fn merge_preserves_application_preferences_and_is_idempotent() {
        let source = "# custom\n[org/gnome/desktop/interface]\nfont-name='Sans 12'\ngtk-theme='Old'\n[app/preferences]\ncustom=true";
        let out = merge(
            source,
            true,
            "Axia-dark",
            "blue",
            "close,minimize,maximize:",
        );
        assert!(out.contains("font-name='Sans 12'\n"));
        assert!(out.contains("[app/preferences]\ncustom=true\n"));
        assert!(out.contains("gtk-theme='Axia-dark'\n"));
        assert_eq!(
            merge(&out, true, "Axia-dark", "blue", "close,minimize,maximize:"),
            out
        );
        let light = merge(&out, false, "Axia", "red", ":minimize,maximize,close");
        assert!(light.contains("color-scheme='default'\n"));
        assert!(light.contains("button-layout=':minimize,maximize,close'\n"));
    }

    #[test]
    fn duplicate_and_whitespace_keys_do_not_override_new_preferences() {
        let out = set_key(
            "[g]\n key =old\nkey=older\n[g]\nkey=oldest",
            "g",
            "key",
            "new",
        );
        assert_eq!(out.matches("key=").count(), 1);
        assert!(!out.contains("old"));
        assert_eq!(set_key("[g]", "g", "key", "new"), "[g]\nkey=new\n");
    }

    #[test]
    fn atomic_write_preserves_other_settings_and_skips_unchanged_content() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temp = Temp::new();
        write_settings(&temp.0, true, "Axia-dark", "blue", "close:").unwrap();
        let path = temp.0.join("glib-2.0/settings/keyfile");
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        write_settings(&temp.0, true, "Axia-dark", "blue", "close:").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
        let mut source = fs::read_to_string(&path).unwrap();
        source.push_str("\n[app]\nkeep=42\n");
        fs::write(&path, source).unwrap();
        write_settings(&temp.0, false, "Axia", "green", ":close").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("keep=42"));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn malformed_or_large_settings_are_never_overwritten() {
        let temp = Temp::new();
        let path = temp.0.join("glib-2.0/settings/keyfile");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for input in [vec![0xff, 0xfe], vec![b'a'; MAX_SETTINGS as usize + 1]] {
            fs::write(&path, &input).unwrap();
            assert!(write_settings(&temp.0, true, "Axia-dark", "blue", "close:").is_err());
            assert_eq!(fs::read(&path).unwrap(), input);
        }
    }

    #[test]
    fn ffi_rejects_injected_settings_and_null_pointers() {
        unsafe {
            assert_eq!(
                axia_appearance_write(true, std::ptr::null(), c"blue".as_ptr(), c"close:".as_ptr()),
                -1
            );
            assert_eq!(
                axia_appearance_write(
                    true,
                    c"Axia".as_ptr(),
                    c"blue'\nkey='bad".as_ptr(),
                    c"close:".as_ptr()
                ),
                -1
            );
            assert_eq!(
                axia_appearance_write(
                    true,
                    c"../../unknown".as_ptr(),
                    c"blue".as_ptr(),
                    c"close:".as_ptr()
                ),
                -1
            );
        }
    }
}
