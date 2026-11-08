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
        for s in name.split('\n').take(129) {
            if !basename(s) || files.len() >= 128 {
                return Err("sys.badArgument".into());
            }
            files.push(s);
        }
    }
    let patterns = args["patterns"].as_str().unwrap_or("");
    if patterns.len() > 16384 {
        return Err("sys.badArgument".into());
    }
    let mut globs = Vec::new();
    for line in patterns.lines().take(128) {
        if let Some(g) = line.strip_prefix("g:") {
            globs.push(glob(g));
        }
        if let Some(m) = line.strip_prefix("m:") {
            let patterns: &[&str] = match m {
                "image/*" => &[
                    "*.png", "*.jpg", "*.jpeg", "*.webp", "*.gif", "*.bmp", "*.svg", "*.tif",
                    "*.tiff", "*.avif",
                ],
                "image/png" => &["*.png"],
                "image/jpeg" => &["*.jpg", "*.jpeg"],
                "application/pdf" => &["*.pdf"],
                "text/plain" => &["*.txt", "*.text"],
                _ => &["*"],
            };
            globs.extend(patterns.iter().map(|p| glob(p)));
        }
    }
    let filter = if globs.is_empty() {
        String::new()
    } else {
        format!("^(?:{})$", globs.join("|"))
    };
    if !filter.is_empty() && regex::Regex::new(&filter).is_err() {
        return Err("sys.badArgument".into());
    }
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
        assert!(prepare(&json!({"patterns":"g:[z-a]"})).is_err());
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
}
