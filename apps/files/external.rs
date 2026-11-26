//! Validation/normalization for external applications using the native chooser.
use super::{policy, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
fn basename(s: &str) -> bool {
    !s.is_empty() && s.len() <= 255 && !s.contains(['/', '\0']) && s != "." && s != ".."
}
fn glob(pattern: &str) -> String {
    let mut result = String::from("(?:");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => result.push_str(".*"),
            '?' => result.push('.'),
            '[' => {
                let mut probe = chars.clone();
                let mut class = String::new();
                let mut closed = false;
                for c in probe.by_ref() {
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    class.push(c);
                }
                if closed && !class.is_empty() {
                    result.push('[');
                    for (i, c) in class.chars().enumerate() {
                        if i == 0 && c == '!' && class.chars().count() > 1 {
                            result.push('^');
                        } else if c == '-' {
                            result.push(c);
                        } else {
                            result.push_str(&regex::escape(&c.to_string()));
                        }
                    }
                    result.push(']');
                    chars = probe;
                } else {
                    result.push_str("\\[");
                }
            }
            _ => result.push_str(&regex::escape(&c.to_string())),
        }
    }
    result.push(')');
    result
}

/// The compiled size a file filter may take, here and where the dialog builds it.
pub(super) const FILTER_SIZE: usize = 8 * 1024 * 1024;
/// More lines than this and every file is shown.
const MOST_LINES: usize = 2048;

/// One expression for many globs. Those that start with `*` (extensions, almost all of them)
/// share it, so hundreds of formats stay small. None when it cannot be built.
fn expression(globs: &[&str]) -> Option<String> {
    let (mut endings, mut whole) = (Vec::new(), Vec::new());
    for pattern in globs {
        match pattern.strip_prefix('*') {
            Some(ending) => endings.push(glob(ending)),
            None => whole.push(glob(pattern)),
        }
    }
    if !endings.is_empty() {
        whole.push(format!(".*(?:{})", endings.join("|")));
    }
    let filter = format!("^(?:{})$", whole.join("|"));
    regex::RegexBuilder::new(&filter).size_limit(FILTER_SIZE).build().ok().map(|_| filter)
}

/// The globs a `m:` line stands for. A type we do not know accepts every file.
fn mime_globs(mime: &str) -> &'static [&'static str] {
    match mime {
        "image/*" => &["*.png", "*.jpg", "*.jpeg", "*.webp", "*.gif", "*.bmp", "*.svg", "*.tif", "*.tiff", "*.avif"],
        "image/png" => &["*.png"],
        "image/jpeg" => &["*.jpg", "*.jpeg"],
        "application/pdf" => &["*.pdf"],
        "text/plain" => &["*.txt", "*.text"],
        _ => &["*"],
    }
}

/// The dialog's filter for an application's patterns. A filter that cannot be honored in full
/// shows every file: hiding files the application accepts would be worse.
fn filter_for(patterns: &str) -> String {
    if patterns.len() > 65536 || patterns.lines().count() > MOST_LINES {
        return String::new();
    }
    let mut globs = Vec::new();
    for line in patterns.lines() {
        if let Some(g) = line.strip_prefix("g:") {
            globs.push(g);
        }
        if let Some(m) = line.strip_prefix("m:") {
            globs.extend_from_slice(mime_globs(m));
        }
    }
    if globs.is_empty() || globs.contains(&"*") {
        return String::new();
    }
    expression(&globs).unwrap_or_default()
}

/// The arguments of the dialog for another application's request. Only an unknown mode or a file
/// name that would leave the chosen folder is an error: everything else falls back to something
/// that still opens.
pub fn prepare(args: &Value) -> Result<Value> {
    let mode = args["mode"].as_str().unwrap_or("open");
    if !["open", "save", "folder", "saveFiles"].contains(&mode) {
        return Err("sys.badArgument".into());
    }
    let p = policy::current();
    let start = args["start"]
        .as_str()
        .filter(|s| s.len() <= 4096)
        .and_then(|s| p.read(Path::new(s)).ok())
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| p.home.clone());
    let name = args["name"].as_str().unwrap_or("");
    let mut files = Vec::new();
    if mode == "saveFiles" {
        for s in name.split('\n').filter(|s| !s.is_empty()).take(129) {
            if !basename(s) || files.len() >= 128 {
                return Err("sys.badArgument".into());
            }
            files.push(s);
        }
    }
    let filter = filter_for(args["patterns"].as_str().unwrap_or(""));
    Ok(
        json!({"mode":if mode=="saveFiles"{"folder"}else{mode},"start":start,
        "name":if mode=="save" && basename(name){name}else{""},"files":files,
        "multiple":mode=="open" && args["multiple"]==true,"filter":filter,
        "title":args["title"].as_str().unwrap_or("").chars().take(256).collect::<String>()}),
    )
}
fn uri(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("files.filenameEncoding")?;
    let mut uri = String::from("file://");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            uri.push(b as char)
        } else {
            uri.push_str(&format!("%{b:02X}"))
        }
    }
    Ok(uri)
}
pub fn uris(args: &Value) -> Result<Value> {
    let p = policy::current();
    let paths = args["paths"].as_array().ok_or("sys.badArgument")?;
    if paths.len() > 1024 {
        return Err("sys.badArgument".into());
    }
    let mut out = Vec::new();
    if let Some(files) = args["files"].as_array().filter(|v| !v.is_empty()) {
        if paths.len() != 1 || files.len() > 128 {
            return Err("sys.badArgument".into());
        }
        let folder = p.read(Path::new(paths[0].as_str().ok_or("sys.badArgument")?))?;
        if !folder.is_dir() {
            return Err("sys.badArgument".into());
        }
        let mut chosen = std::collections::HashSet::new();
        for file in files {
            let name = file
                .as_str()
                .filter(|s| basename(s))
                .ok_or("sys.badArgument")?;
            let mut target = super::model::free_name(&folder, name);
            let mut n = 2;
            while !chosen.insert(target.clone()) {
                target = super::model::free_name(&folder, &format!("{n}-{name}"));
                n += 1;
            }
            p.target(&target, true)?;
            out.push(uri(&target)?);
        }
    } else {
        for path in paths {
            let path = PathBuf::from(path.as_str().ok_or("sys.badArgument")?);
            let path = if args["save"] == true {
                p.target(&path, true)?
            } else {
                p.read(&path)?
            };
            out.push(uri(&path)?);
        }
    }
    Ok(json!({"uris":out}))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::tests::Home;
    #[test]
    fn requests_are_normalized_and_uri_metacharacters_are_escaped() {
        let h = Home::new();
        let file = h.file("foto #1%.png", "");
        let prepared=prepare(&json!({"patterns":"g:*.[pP][nN][gG]\ng:*.jpg","start":"/does-not-exist","mode":"open","multiple":true})).unwrap();
        let re = regex::Regex::new(prepared["filter"].as_str().unwrap()).unwrap();
        assert!(re.is_match("PHOTO.PNG"));
        assert!(!re.is_match("picture.pdf"));
        let broken = prepare(&json!({"patterns":"g:[z-a]"})).unwrap();
        assert_eq!(broken["filter"], "", "a filter that cannot be built shows every file");
        let sensitive = prepare(&json!({"patterns":"g:*.png"})).unwrap();
        let re = regex::Regex::new(sensitive["filter"].as_str().unwrap()).unwrap();
        assert!(!re.is_match("photo.PNG"));
        assert_eq!(Path::new(prepared["start"].as_str().unwrap()), h.path(""));
        let result = uris(&json!({"paths":[file]})).unwrap();
        assert!(result["uris"][0]
            .as_str()
            .unwrap()
            .ends_with("foto%20%231%25.png"));
        assert!(prepare(&json!({"mode":"saveFiles","name":"../bad"})).is_err());
    }
    /// PhotoCraft's "All Formats": three spellings of dozens of extensions.
    #[test]
    fn hundreds_of_formats_are_all_kept_and_the_dialog_accepts_the_filter() {
        let _h = Home::new();
        let mut patterns = String::new();
        for i in 0..150 {
            let classes: String = format!("ext{i}").chars().map(|c| if c.is_ascii_alphabetic() { format!("[{c}{}]", c.to_ascii_uppercase()) } else { c.to_string() }).collect();
            patterns.push_str(&format!("g:*.ext{i}\ng:*.EXT{i}\ng:*.{classes}\n"));
        }
        let prepared = prepare(&json!({"patterns":patterns})).unwrap();
        let filter = prepared["filter"].as_str().unwrap();
        let re = regex::RegexBuilder::new(filter).size_limit(FILTER_SIZE).build().unwrap();
        for name in ["a.ext0", "b.ext127", "c.eXt128", "d.EXT149"] {
            assert!(re.is_match(name), "{name}");
        }
        assert!(!re.is_match("a.txt") && !re.is_match("ext0"));
        // too many to honor: everything is shown instead of an arbitrary part
        let huge = "g:*.a\n".repeat(MOST_LINES + 1);
        assert_eq!(prepare(&json!({"patterns":huge})).unwrap()["filter"], "");
        assert_eq!(prepare(&json!({"patterns":"g:*.png\ng:*"})).unwrap()["filter"], "");
        assert_eq!(prepare(&json!({"patterns":"m:application/x-unknown"})).unwrap()["filter"], "");
        assert_eq!(prepare(&json!({"mode":"saveFiles","name":""})).unwrap()["files"], json!([]));
    }
}
