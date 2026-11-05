//! Small, reversible application adapters. No renderer changes, polling, app
//! patches or extension-host code: browser chrome CSS and VS Code's icon API.
use serde_json::Value;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 1024 * 1024;
const CSS: &str = include_str!("browser_controls.css");
const IMPORT: &str = "/* axia:window-controls:start */\n@import url(\"axia-window-controls.css\");\n/* axia:window-controls:end */\n";
const PREF: &str = "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", true); // axia:window-controls\n";
const CODE: &str = "\n    // axia:window-controls:start\n    \"window.controlsStyle\": \"custom\",\n    // axia:window-controls:end\n";
const PACKAGE: &str = include_str!("../../../assets/vscode/axia-window-controls/package.json");
const ICONS: &[u8] = include_bytes!("../../../assets/vscode/axia-window-controls/icons.json");
const FONT: &[u8] = include_bytes!("../../../assets/vscode/axia-window-controls/controls.ttf");
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn bad() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "custom or unsupported appearance configuration",
    )
}

// Reject symlinks (including parents) and directories owned by another user.
// Never follow profile/extension links into unrelated files.
fn directory(path: &Path, create: bool) -> io::Result<bool> {
    if !path.is_absolute() {
        return Err(bad());
    }
    let mut part = PathBuf::new();
    for component in path.components() {
        if !matches!(
            component,
            std::path::Component::RootDir | std::path::Component::Normal(_)
        ) {
            return Err(bad());
        }
        part.push(component);
        match fs::symlink_metadata(&part) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => (),
            Ok(_) => return Err(bad()),
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                fs::DirBuilder::new().mode(0o700).create(&part)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    if fs::metadata(path)?.uid() != unsafe { libc::geteuid() } {
        return Err(bad());
    }
    Ok(true)
}

fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => {
            let m = f.metadata()?;
            if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.len() > LIMIT {
                return Err(bad());
            }
            let mut out = Vec::new();
            f.take(LIMIT + 1).read_to_end(&mut out)?;
            if out.len() as u64 > LIMIT {
                return Err(bad());
            }
            Ok(Some(out))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn text(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?.unwrap_or_default()).map_err(|_| bad())
}

fn write(path: &Path, bytes: &[u8], backup: bool) -> io::Result<()> {
    let parent = path.parent().ok_or_else(bad)?;
    directory(parent, true)?;
    let previous = read(path)?;
    if previous.as_deref() == Some(bytes) {
        return Ok(());
    }
    if backup {
        if let Some(old) = previous.as_ref() {
            let backup = path.with_file_name(format!(
                "{}.axia-backup",
                path.file_name().unwrap().to_string_lossy()
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(backup)
            {
                Ok(mut f) => f.write_all(old)?,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
    }
    let temp = parent.join(format!(
        ".axia-style-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(bytes)?;
        // Refuse to replace a file edited while we prepared this update.
        if read(path)? != previous {
            return Err(bad());
        }
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn remove_owned(path: &Path, expected: &[u8]) -> io::Result<()> {
    match read(path)? {
        Some(data) if data == expected => fs::remove_file(path),
        Some(_) => Err(bad()),
        None => Ok(()),
    }
}

fn browser(profile: &Path, enabled: bool) -> io::Result<()> {
    let chrome = profile.join("chrome");
    if !directory(profile, false)? || !directory(&chrome, enabled)? {
        return Ok(());
    }
    let user_path = profile.join("user.js");
    let user = text(&user_path)?;
    let own_pref = user.contains(PREF);
    let clean_user = user.replace(PREF, "");
    // Explicit user preferences always win, including an intentional opt-out.
    if enabled && clean_user.contains("toolkit.legacyUserProfileCustomizations.stylesheets") {
        let re=regex::Regex::new(r#"(?m)^\s*user_pref\(\s*["']toolkit\.legacyUserProfileCustomizations\.stylesheets["']\s*,\s*true\s*\)\s*;"#).unwrap();
        if !re.is_match(&clean_user) {
            // If an opt-out was added after our first setup, retire our old
            // preference as well so its later line cannot override the user.
            return browser(profile, false);
        }
    }
    let css_path = chrome.join("userChrome.css");
    let source = text(&css_path)?;
    if source.contains("axia:window-controls") && !source.contains(IMPORT) {
        return Err(bad());
    }
    let asset = chrome.join("axia-window-controls.css");
    if read(&asset)?
        .as_deref()
        .is_some_and(|b| b != CSS.as_bytes())
    {
        return Err(bad());
    }
    let clean = source.replace(IMPORT, "");
    if enabled {
        write(&asset, CSS.as_bytes(), false)?;
        // @charset must remain first; place the managed import before the user's
        // rules so their later !important customizations continue to win.
        let mut offset = if clean.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        if clean[offset..].starts_with("@charset ") {
            offset += clean[offset..].find(';').ok_or_else(bad)? + 1;
        }
        let merged = format!("{}{}{}", &clean[..offset], IMPORT, &clean[offset..]);
        write(&css_path, merged.as_bytes(), true)?;
        if !own_pref && !clean_user.contains("toolkit.legacyUserProfileCustomizations.stylesheets")
        {
            let sep = if user.is_empty() || user.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            write(&user_path, format!("{user}{sep}{PREF}").as_bytes(), true)?;
        }
    } else {
        if clean != source {
            write(&css_path, clean.as_bytes(), true)?;
        }
        if own_pref {
            write(&user_path, clean_user.as_bytes(), true)?;
        }
        remove_owned(&asset, CSS.as_bytes())?;
    }
    Ok(())
}

// Remove JSONC comments and trailing commas without altering byte offsets.
// Strings, including escaped quotes and comment-like URLs, remain untouched.
fn clean_jsonc(source: &str) -> io::Result<Vec<u8>> {
    let mut bytes = source.as_bytes().to_vec();
    let mut i = 0;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes[..3].fill(b' ');
    }
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    bytes[i] = b' ';
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                bytes[i] = b' ';
                bytes[i + 1] = b' ';
                i += 2;
                let mut ended = false;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        bytes[i] = b' ';
                        bytes[i + 1] = b' ';
                        i += 2;
                        ended = true;
                        break;
                    }
                    bytes[i] = b' ';
                    i += 1;
                }
                if !ended {
                    return Err(bad());
                }
            }
            _ => i += 1,
        }
    }
    i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
        } else if bytes[i] == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if matches!(bytes.get(j), Some(b'}' | b']')) {
                bytes[i] = b' ';
            }
        }
        i += 1;
    }
    Ok(bytes)
}

fn jsonc(source: &str) -> io::Result<Value> {
    let value: Value = serde_json::from_slice(&clean_jsonc(source)?).map_err(|_| bad())?;
    if !value.is_object() {
        return Err(bad());
    }
    Ok(value)
}

fn code_settings(source: &str, enabled: bool) -> io::Result<String> {
    if source.contains("axia:window-controls") && !source.contains(CODE) {
        return Err(bad());
    }
    let clean = source.replace(CODE, "");
    let empty = clean.trim().is_empty();
    let clean = if empty { "{}" } else { &clean };
    let value = jsonc(clean)?;
    if !enabled || value.get("window.controlsStyle").is_some() {
        return Ok(if empty {
            source.to_owned()
        } else {
            clean.to_owned()
        });
    }
    // Only a missing application preference is defaulted. Theme, file icons,
    // custom controls/hidden/native choices and all JSONC comments survive.
    let pos = clean_jsonc(clean)?
        .iter()
        .position(|&b| b == b'{')
        .ok_or_else(bad)?
        + 1;
    let result = format!("{}{CODE}{}", &clean[..pos], &clean[pos..]);
    jsonc(&result)?;
    Ok(result)
}

fn package(dark: bool, enabled: bool) -> Vec<u8> {
    let mut package: Value = serde_json::from_str(PACKAGE).unwrap();
    let colors =
        &mut package["contributes"]["configurationDefaults"]["workbench.colorCustomizations"];
    let (bg, fg, dim) = if dark {
        ("#2a2a2e", "#f5f5f7", "#99999f")
    } else {
        ("#f0f0f3", "#202024", "#6a6a70")
    };
    for (key, val) in [
        ("titleBar.activeBackground", bg),
        ("titleBar.inactiveBackground", bg),
        ("titleBar.activeForeground", fg),
        ("titleBar.inactiveForeground", dim),
    ] {
        colors[key] = val.into();
    }
    if !enabled {
        package["contributes"]
            .as_object_mut()
            .unwrap()
            .remove("configurationDefaults");
    }
    serde_json::to_vec_pretty(&package).unwrap()
}

fn code_index(extensions: &Path, ext: &Path) -> io::Result<()> {
    // VS Code's default-profile extension scanner uses this index once it
    // exists. Register only our entry and retain every other metadata field.
    // Invalid or concurrently edited indexes are left untouched by write().
    let path = extensions.join("extensions.json");
    let mut entries: Value = match read(&path)? {
        Some(data) => serde_json::from_slice(&data).map_err(|_| bad())?,
        // Without an index, the editor scans every folder itself. Creating a
        // partial index here would hide other themes copied before first use.
        None => return Ok(()),
    };
    let array = entries.as_array_mut().ok_or_else(bad)?;
    let folder = ext.to_str().ok_or_else(bad)?;
    for entry in array.iter() {
        if entry["identifier"]["id"]
            .as_str()
            .is_some_and(|id| id.eq_ignore_ascii_case("axia-de.axia-window-controls"))
        {
            if entry["location"]["path"].as_str() != Some(folder) {
                return Err(bad());
            }
            return Ok(());
        }
    }
    array.push(serde_json::json!({
        "identifier":{"id":"axia-de.axia-window-controls"},"version":"0.1.0",
        "location":{"$mid":1,"scheme":"file","path":folder},
        "relativeLocation":"axia-de.axia-window-controls-0.1.0"
    }));
    write(
        &path,
        &serde_json::to_vec(&entries).map_err(|_| bad())?,
        false,
    )
}

fn vscode(home: &Path, config: &Path, enabled: bool, dark: bool) -> io::Result<()> {
    let user = config.join("Code/User");
    if !directory(&user, false)? {
        return Ok(());
    }
    let path = user.join("settings.json");
    let source = text(&path)?;
    let merged = code_settings(&source, enabled)?;
    let extensions = home.join(".vscode/extensions");
    if !directory(&extensions, enabled)? {
        return Ok(());
    }
    let ext = extensions.join("axia-de.axia-window-controls-0.1.0");
    if !directory(&ext, enabled)? {
        return Ok(());
    }
    let manifest = ext.join("package.json");
    let old = read(&manifest)?;
    if old.as_deref().is_some_and(|b| {
        b != package(true, true) && b != package(false, true) && b != package(false, false)
    }) {
        return Err(bad());
    }
    for (name, bytes) in [("icons.json", ICONS), ("controls.ttf", FONT)] {
        let path = ext.join(name);
        if read(&path)?.as_deref().is_some_and(|b| b != bytes) {
            return Err(bad());
        }
    }
    if enabled {
        write(&ext.join("icons.json"), ICONS, false)?;
        write(&ext.join("controls.ttf"), FONT, false)?;
        // Manifest last: the editor never sees a half-installed icon theme.
        write(&manifest, &package(dark, true), false)?;
        code_index(&extensions, &ext)?;
        write(&path, merged.as_bytes(), true)?;
    } else {
        write(&path, merged.as_bytes(), true)?;
        // Keep the tiny inert resources available to already-open windows.
        // At the next launch the extension no longer supplies any defaults.
        if old.is_some() {
            write(&manifest, &package(false, false), false)?;
        }
    }
    Ok(())
}

fn sync(home: &Path, config: &Path, enabled: bool, dark: bool) -> usize {
    let mut errors = 0;
    let mut seen = std::collections::HashSet::new();
    let roots = [
        config.join("mozilla/firefox"),
        home.join(".mozilla/firefox"),
        config.join("librewolf/librewolf"),
        config.join("librewolf"),
        home.join(".librewolf"),
    ];
    for root in roots {
        let result = (|| -> io::Result<()> {
            if !directory(&root, false)? {
                return Ok(());
            }
            let ini = text(&root.join("profiles.ini"))?;
            let mut relative = false;
            let mut profile = false;
            let mut path = None;
            for line in ini.lines().chain(std::iter::once("[end]")) {
                let line = line.trim();
                if line.starts_with('[') {
                    if profile && relative {
                        if let Some(name) = path.take() {
                            let p = root.join(name);
                            if directory(&p, false)? {
                                let m = fs::metadata(&p)?;
                                if seen.len() < 64 && seen.insert((m.dev(), m.ino())) {
                                    if browser(&p, enabled).is_err() {
                                        errors += 1;
                                    }
                                }
                            }
                        }
                    }
                    profile = line.starts_with("[Profile");
                    relative = false;
                    path = None;
                } else if profile {
                    if let Some(v) = line.strip_prefix("IsRelative=") {
                        relative = v == "1";
                    }
                    if let Some(v) = line.strip_prefix("Path=") {
                        if !v.is_empty()
                            && v.len() <= 200
                            && v != "."
                            && v != ".."
                            && !v.contains('/')
                            && !v.chars().any(char::is_control)
                        {
                            path = Some(v.to_owned());
                        }
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            errors += 1;
        }
    }
    if vscode(home, config, enabled, dark).is_err() {
        errors += 1;
    }
    errors
}

#[no_mangle]
pub extern "C" fn axia_external_style(enabled: bool, dark: bool) -> i32 {
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    else {
        return -1;
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let errors = sync(&home, &config, enabled, dark);
    if errors > 0 {
        eprintln!(
            "appearance: kept {errors} custom or unreadable application configurations unchanged"
        );
    }
    errors as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "axia-style-test-{}-{}",
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
    fn code_jsonc_preserves_dracula_comments_urls_and_user_choices() {
        let src="{\n// my theme\n\"workbench.colorTheme\": \"Dracula Theme\",\n\"url\":\"https://a/*b*/\",/*keep*/\n}";
        let changed = code_settings(src, true).unwrap();
        assert!(changed.contains(src.trim_start_matches('{')));
        assert_eq!(jsonc(&changed).unwrap()["window.controlsStyle"], "custom");
        assert_eq!(code_settings(&changed, true).unwrap(), changed);
        assert_eq!(code_settings(&changed, false).unwrap(), src);
        for choice in ["native", "hidden", "custom"] {
            let src = format!("{{\"window.controlsStyle\":\"{choice}\"}}");
            assert_eq!(code_settings(&src, true).unwrap(), src);
        }
        assert!(code_settings("{invalid", true).is_err());
        assert!(code_settings("{/*broken", true).is_err());
        let leading = "// braces { in a comment\n\u{feff}{/* keep */\"a\":[1,2,],}";
        // UTF-8 BOM is accepted only at the start of a JSONC document.
        assert!(code_settings(leading, true).is_err());
        let leading = "\u{feff}// braces { in a comment\n{ /* keep */ \"a\": [1,2,], }";
        let merged = code_settings(leading, true).unwrap();
        assert_eq!(code_settings(&merged, false).unwrap(), leading);
        assert!(code_settings(&changed.replace("\"custom\"", "\"native\""), true).is_err());
    }
    #[test]
    fn browser_keeps_user_css_and_preferences_and_can_be_disabled() {
        let tmp = Temp::new();
        let profile = &tmp.0;
        fs::create_dir(profile.join("chrome")).unwrap();
        let css = "@charset \"UTF-8\";\n/* my layout */ #TabsToolbar{height:40px}";
        fs::write(profile.join("chrome/userChrome.css"), css).unwrap();
        fs::write(profile.join("user.js"), "user_pref(\"keep\",42);").unwrap();
        browser(profile, true).unwrap();
        let merged = text(&profile.join("chrome/userChrome.css")).unwrap();
        assert!(merged.starts_with("@charset \"UTF-8\";"));
        assert!(merged.contains(css.split_once(';').unwrap().1));
        assert_eq!(
            text(&profile.join("chrome/userChrome.css.axia-backup")).unwrap(),
            css
        );
        browser(profile, true).unwrap();
        assert_eq!(
            text(&profile.join("chrome/userChrome.css")).unwrap(),
            merged
        );
        browser(profile, false).unwrap();
        assert_eq!(text(&profile.join("chrome/userChrome.css")).unwrap(), css);
        assert!(text(&profile.join("user.js"))
            .unwrap()
            .contains("user_pref(\"keep\",42);"));
        assert!(!profile.join("chrome/axia-window-controls.css").exists());
    }
    #[test]
    fn browser_respects_explicit_opt_out_and_edited_owned_files() {
        let tmp = Temp::new();
        fs::write(
            tmp.0.join("user.js"),
            "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", false);",
        )
        .unwrap();
        browser(&tmp.0, true).unwrap();
        assert!(!tmp.0.join("chrome/userChrome.css").exists());
        fs::write(tmp.0.join("user.js"), "").unwrap();
        browser(&tmp.0, true).unwrap();
        let own = text(&tmp.0.join("user.js")).unwrap();
        fs::write(
            tmp.0.join("user.js"),
            format!(
                "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", false);\n{own}"
            ),
        )
        .unwrap();
        browser(&tmp.0, true).unwrap();
        assert!(!text(&tmp.0.join("user.js")).unwrap().contains(PREF));
        assert!(!tmp.0.join("chrome/axia-window-controls.css").exists());
        fs::write(tmp.0.join("user.js"), "").unwrap();
        browser(&tmp.0, true).unwrap();
        fs::write(
            tmp.0.join("chrome/axia-window-controls.css"),
            "/* my changes */",
        )
        .unwrap();
        assert!(browser(&tmp.0, false).is_err());
        assert_eq!(
            text(&tmp.0.join("chrome/axia-window-controls.css")).unwrap(),
            "/* my changes */"
        );
    }
    #[test]
    fn links_large_files_and_invalid_utf8_are_never_overwritten() {
        let tmp = Temp::new();
        let target = tmp.0.join("target");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, tmp.0.join("user.js")).unwrap();
        assert!(browser(&tmp.0, true).is_err());
        assert_eq!(text(&target).unwrap(), "keep");
        fs::remove_file(tmp.0.join("user.js")).unwrap();
        for data in [vec![0xff], vec![b'x'; LIMIT as usize + 1]] {
            fs::write(tmp.0.join("user.js"), &data).unwrap();
            assert!(browser(&tmp.0, true).is_err());
            assert_eq!(fs::read(tmp.0.join("user.js")).unwrap(), data);
        }
    }
    #[test]
    fn vscode_adapter_is_declarative_idempotent_and_reversible() {
        let tmp = Temp::new();
        let config = tmp.0.join(".config");
        let user = config.join("Code/User");
        fs::create_dir_all(&user).unwrap();
        let settings="{\"workbench.colorTheme\":\"Dracula Theme\",\"workbench.iconTheme\":\"material-icon-theme\"}";
        fs::write(user.join("settings.json"), settings).unwrap();
        let extensions = tmp.0.join(".vscode/extensions");
        fs::create_dir_all(&extensions).unwrap();
        let existing = serde_json::json!({"identifier":{"id":"user.other"},"version":"3.0","metadata":{"keep":42}});
        fs::write(
            extensions.join("extensions.json"),
            serde_json::to_vec(&vec![existing.clone()]).unwrap(),
        )
        .unwrap();
        vscode(&tmp.0, &config, true, true).unwrap();
        let index: Value =
            serde_json::from_slice(&fs::read(extensions.join("extensions.json")).unwrap()).unwrap();
        assert_eq!(index[0], existing);
        assert_eq!(index[1]["identifier"]["id"], "axia-de.axia-window-controls");
        let ext = tmp
            .0
            .join(".vscode/extensions/axia-de.axia-window-controls-0.1.0");
        let manifest = ext.join("package.json");
        let ino = fs::metadata(&manifest).unwrap().ino();
        let pkg = jsonc(&text(&manifest).unwrap()).unwrap();
        assert!(pkg.get("main").is_none());
        assert!(pkg.get("activationEvents").is_none());
        vscode(&tmp.0, &config, true, true).unwrap();
        assert_eq!(fs::metadata(&manifest).unwrap().ino(), ino);
        let index: Value =
            serde_json::from_slice(&fs::read(extensions.join("extensions.json")).unwrap()).unwrap();
        assert_eq!(index.as_array().unwrap().len(), 2);
        vscode(&tmp.0, &config, true, false).unwrap();
        assert_eq!(
            jsonc(&text(&manifest).unwrap()).unwrap()["contributes"]["configurationDefaults"]
                ["workbench.colorCustomizations"]["titleBar.activeBackground"],
            "#f0f0f3"
        );
        vscode(&tmp.0, &config, false, false).unwrap();
        assert_eq!(text(&user.join("settings.json")).unwrap(), settings);
        assert!(jsonc(&text(&manifest).unwrap()).unwrap()["contributes"]
            .get("configurationDefaults")
            .is_none());
        assert!(ext.join("controls.ttf").exists()); // Already-open windows retain their font.
        vscode(&tmp.0, &config, true, true).unwrap();
        assert_eq!(
            jsonc(&text(&manifest).unwrap()).unwrap()["contributes"]["configurationDefaults"]
                ["workbench.productIconTheme"],
            "axia-window-controls"
        );
    }
    #[test]
    fn extension_index_conflicts_and_invalid_metadata_do_not_change_user_preferences() {
        let tmp = Temp::new();
        let config = tmp.0.join(".config");
        let user = config.join("Code/User");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("settings.json"), "{}").unwrap();
        let extensions = tmp.0.join(".vscode/extensions");
        fs::create_dir_all(&extensions).unwrap();
        for invalid in [
            "{}",
            "invalid",
            r#"[{"identifier":{"id":"axia-de.axia-window-controls"},"location":{"path":"/other/custom"}}]"#,
        ] {
            fs::write(extensions.join("extensions.json"), invalid).unwrap();
            assert!(vscode(&tmp.0, &config, true, true).is_err());
            assert_eq!(text(&user.join("settings.json")).unwrap(), "{}");
            assert_eq!(text(&extensions.join("extensions.json")).unwrap(), invalid);
        }
    }
    #[test]
    fn relative_profiles_only_and_custom_code_config_stays_intact() {
        let tmp = Temp::new();
        let config = tmp.0.join(".config");
        let root = config.join("librewolf/librewolf");
        fs::create_dir_all(root.join("test.default")).unwrap();
        fs::write(root.join("profiles.ini"),"[Profile0]\nIsRelative=1\nPath=test.default\n[Profile1]\nIsRelative=1\nPath=../../escape\n").unwrap();
        assert_eq!(sync(&tmp.0, &config, true, true), 0);
        assert!(root
            .join("test.default/chrome/axia-window-controls.css")
            .exists());
        let user = config.join("Code/User");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("settings.json"), "broken").unwrap();
        assert_eq!(sync(&tmp.0, &config, true, true), 1);
        assert_eq!(text(&user.join("settings.json")).unwrap(), "broken");
    }
}
