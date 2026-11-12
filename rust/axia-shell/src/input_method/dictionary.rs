//! Bounded, owned dictionary data. Nonregular files are rejected without blocking.
use std::{
    ffi::{CStr, CString, OsStr},
    fs::OpenOptions,
    io::Read,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::PathBuf,
};
const LIMIT: u64 = 64 << 20;
pub(super) fn load(names: &[&str]) -> Option<Vec<u8>> {
    let home = std::env::var_os("HOME");
    load_with_home(names, home.as_deref())
}
pub(super) fn load_with_home(names: &[&str], home: Option<&OsStr>) -> Option<Vec<u8>> {
    for name in names {
        let mut path = if name.starts_with('/') {
            name.as_bytes().to_vec()
        } else if let Some(home) = home {
            let mut path = home.as_bytes().to_vec();
            path.extend_from_slice(b"/.local/share/axia/ime/");
            path.extend_from_slice(name.as_bytes());
            path
        } else {
            continue;
        };
        path.truncate(511);
        let path_c = CString::new(path.clone()).ok()?;
        if unsafe { libc::access(path_c.as_ptr(), libc::R_OK) } != 0 {
            continue;
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(PathBuf::from(OsStr::from_bytes(&path)))
            .ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() >= LIMIT {
            return None;
        }
        let mut data = Vec::with_capacity(metadata.len() as usize);
        file.take(LIMIT).read_to_end(&mut data).ok()?;
        return (!data.is_empty() && data.len() < LIMIT as usize).then_some(data);
    }
    None
}
fn convert(to: &CStr, from: &CStr, input: &[u8], capacity: usize) -> Option<Vec<u8>> {
    if capacity == 0 {
        return None;
    }
    unsafe {
        let handle = libc::iconv_open(to.as_ptr(), from.as_ptr());
        if handle as usize == usize::MAX {
            return None;
        }
        let mut out = vec![0u8; capacity - 1];
        let mut incoming = input.as_ptr() as *mut libc::c_char;
        let mut outgoing = out.as_mut_ptr().cast();
        let mut left = input.len();
        let mut available = out.len();
        let result = libc::iconv(
            handle,
            &mut incoming,
            &mut left,
            &mut outgoing,
            &mut available,
        );
        libc::iconv_close(handle);
        if result == usize::MAX {
            return None;
        }
        out.truncate(out.len() - available);
        Some(out)
    }
}
#[derive(Default)]
pub(super) struct Skk {
    data: Vec<u8>,
    lines: Vec<usize>,
    ari: usize,
}
impl Skk {
    pub(super) fn parse(data: Vec<u8>) -> Self {
        let mut dict = Self {
            data,
            ..Self::default()
        };
        let mut section = -1;
        let mut offset = 0;
        for line in dict.data.split(|&b| b == b'\n') {
            if line.starts_with(b";; okuri-ari entries.") {
                section = 0;
            } else if line.starts_with(b";; okuri-nasi entries.") {
                section = 1;
                dict.ari = dict.lines.len();
            } else if section >= 0
                && !line.is_empty()
                && line[0] != b';'
                && dict.lines.len() < 1 << 18
            {
                dict.lines.push(offset);
            }
            offset += line.len() + 1;
        }
        if section < 1 {
            dict.ari = 0;
        }
        dict
    }
    fn line(&self, reading: &[u8], okuri: bool) -> Option<(&[u8], usize)> {
        let key = convert(c"EUC-JP", c"UTF-8", reading, 256)?;
        let mut lo = if okuri { 0 } else { self.ari };
        let mut hi = if okuri { self.ari } else { self.lines.len() };
        while lo < hi {
            let mid = lo + (hi - lo - 1) / 2;
            let data = &self.data[self.lines[mid]..];
            let len = data
                .iter()
                .position(|&b| b == b' ' || b == b'\n')
                .unwrap_or(data.len());
            let mut order = data[..len].cmp(&key);
            if order.is_eq() {
                return Some((data, key.len()));
            }
            if okuri {
                order = order.reverse();
            }
            if order.is_lt() {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        None
    }
    pub(super) fn contains(&self, reading: &[u8], okuri: bool) -> bool {
        self.line(reading, okuri).is_some()
    }
    pub(super) fn candidates(
        &self,
        reading: &[u8],
        okuri: bool,
        suffix: &[u8],
        list: &mut Vec<Vec<u8>>,
        max: usize,
    ) {
        let Some((line, keylen)) = self.line(reading, okuri) else {
            return;
        };
        let end = line.iter().position(|&b| b == b'\n').unwrap_or(line.len());
        let line = &line[keylen..end];
        let Some(mut slash) = line.iter().position(|&b| b == b'/') else {
            return;
        };
        while slash < line.len() && list.len() < max {
            let Some(relative) = line[slash + 1..].iter().position(|&b| b == b'/') else {
                break;
            };
            let next = slash + 1 + relative;
            let candidate = &line[slash + 1..next];
            let candidate = &candidate[..candidate
                .iter()
                .position(|&b| b == b';')
                .unwrap_or(candidate.len())];
            if !candidate.is_empty() && candidate[0] != b'(' && candidate.len() < 80 {
                if let Some(mut value) = convert(c"UTF-8", c"EUC-JP", candidate, 96) {
                    super::append(&mut value, 96, suffix);
                    super::add_candidate(list, max, &value);
                }
            }
            slash = next;
        }
    }
}

pub(super) const SKK_NAMES: &[&str] = &[
    "SKK-JISYO.L",
    "SKK-JISYO",
    "/usr/share/skk/SKK-JISYO.L",
    "/usr/share/skk/SKK-JISYO",
];
pub(super) const PINYIN_NAMES: &[&str] = &[
    "pinyin_simp.dict.yaml",
    "/usr/share/rime-data/pinyin_simp.dict.yaml",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "axia-ime-dictionary-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn user_precedence_missing_fallback_and_invalid_selected_files() {
        let directory = Directory::new();
        let user = directory.0.join(".local/share/axia/ime");
        std::fs::create_dir_all(&user).unwrap();
        let fallback = directory.0.join("fallback");
        std::fs::write(&fallback, b"system").unwrap();
        let names = ["fixture", fallback.to_str().unwrap()];
        assert_eq!(
            load_with_home(&names, Some(directory.0.as_os_str())).unwrap(),
            b"system"
        );
        std::fs::write(user.join("fixture"), b"user").unwrap();
        assert_eq!(
            load_with_home(&names, Some(directory.0.as_os_str())).unwrap(),
            b"user"
        );
        std::fs::write(user.join("fixture"), b"").unwrap();
        assert!(load_with_home(&names, Some(directory.0.as_os_str())).is_none());
    }
    #[test]
    fn nonregular_and_oversized_dictionary_files_are_rejected_without_reading() {
        let directory = Directory::new();
        assert!(load_with_home(&[directory.0.to_str().unwrap()], None).is_none());
        let fifo = directory.0.join("fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        assert!(load_with_home(&[fifo.to_str().unwrap()], None).is_none());
        let huge = directory.0.join("huge");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(LIMIT)
            .unwrap();
        assert!(load_with_home(&[huge.to_str().unwrap()], None).is_none());
    }
    #[test]
    fn skk_annotations_expression_filters_and_broken_encodings_are_bounded() {
        let mut data = include_bytes!("../../tests/data/ime/SKK-JISYO.L").to_vec();
        if data.last() == Some(&b'\n') {
            data.pop();
        }
        let dictionary = Skk::parse(data);
        let mut candidates = Vec::new();
        dictionary.candidates("ねこ".as_bytes(), false, b"", &mut candidates, 24);
        assert_eq!(
            candidates,
            vec!["猫".as_bytes().to_vec(), "寝子".as_bytes().to_vec()]
        );
        let mut broken = b";; okuri-nasi entries.\n".to_vec();
        broken.extend(convert(c"EUC-JP", c"UTF-8", "ねこ".as_bytes(), 256).unwrap());
        broken.extend_from_slice(b" /\xff/(");
        let dictionary = Skk::parse(broken);
        let mut candidates = Vec::new();
        dictionary.candidates("ねこ".as_bytes(), false, b"", &mut candidates, 24);
        assert!(candidates.is_empty());
        for len in 0..32 {
            let data = include_bytes!("../../tests/data/ime/SKK-JISYO.L");
            let dictionary = Skk::parse(data[..len].to_vec());
            assert!(!dictionary.contains("ねこ".as_bytes(), false));
        }
    }
}
