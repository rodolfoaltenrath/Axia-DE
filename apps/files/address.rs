//! Folder addresses are data: never shell expanded or executed.
use super::Result;
use std::{
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};
pub fn folder(text: &str, base: &Path, home: &Path) -> Result<PathBuf> {
    let text = text.trim();
    if text.is_empty() || text.len() > 65536 || text.contains('\0') {
        return Err("files.invalidAddress".into());
    }
    if let Some(uri) = text.strip_prefix("file://") {
        let path = if uri.starts_with('/') {
            uri
        } else if let Some(p) = uri.strip_prefix("localhost/") {
            return decode(&format!("/{p}"));
        } else {
            return Err("files.invalidAddress".into());
        };
        return decode(path);
    }
    Ok(if text == "~" {
        home.into()
    } else if let Some(p) = text.strip_prefix("~/") {
        home.join(p)
    } else if Path::new(text).is_absolute() {
        text.into()
    } else {
        base.join(text)
    })
}
fn decode(path: &str) -> Result<PathBuf> {
    if path.contains(['?', '#']) {
        return Err("files.invalidAddress".into());
    }
    let mut bytes = Vec::new();
    let raw = path.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let b = if raw[i] == b'%' {
            if i + 2 >= raw.len() {
                return Err("files.invalidAddress".into());
            }
            let hi = (raw[i + 1] as char)
                .to_digit(16)
                .ok_or("files.invalidAddress")?;
            let lo = (raw[i + 2] as char)
                .to_digit(16)
                .ok_or("files.invalidAddress")?;
            i += 2;
            (hi * 16 + lo) as u8
        } else {
            raw[i]
        };
        if b == 0 {
            return Err("files.invalidAddress".into());
        }
        bytes.push(b);
        i += 1;
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_local_file_links_and_rejects_remote_hosts_and_bad_escapes() {
        let base = Path::new("/home/test/Docs");
        let home = Path::new("/home/test");
        assert_eq!(
            folder("file://localhost/home/test/A%20B", base, home).unwrap(),
            PathBuf::from("/home/test/A B")
        );
        assert_eq!(
            folder("../Downloads", base, home).unwrap(),
            base.join("../Downloads")
        );
        assert_eq!(
            folder("~/Downloads", base, home).unwrap(),
            home.join("Downloads")
        );
        for bad in [
            "file://server/share",
            "file:///bad%00",
            "file:///bad%xx",
            "file:///bad%",
            "file:///bad#fragment",
        ] {
            assert!(folder(bad, base, home).is_err());
        }
    }
}
