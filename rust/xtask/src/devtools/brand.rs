//! Keeps third-party brand and product names out of the public text.
//!
//! Scans assets/locales/*/*.json, the Help articles, the app manifests and theme.json files, the
//! language packs in docs/packs, README.md and CHANGELOG.md. Axia uses its own names for its features
//! and generic wording for comparisons ("other operating systems"). Exit code 1 if a name shows up
//! outside the allow list.

use super::{entries, is_word, line_of};
use crate::Result;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

/// Names that must not appear (whole words, case-sensitive: "windows" as in app windows is fine).
const BRANDS: &[&str] = &[
    "Apple", "macOS", "Mac OS", "OS X", "Mac", "MacBook", "iMac", "iPhone", "iPad", "iOS", "iCloud", "Safari",
    "Xcode", "Finder", "Spotlight", "Launchpad", "Night Shift", "Quick Look", "Time Machine", "AirPrint",
    "AirDrop", "Mission Control", "Disk Utility", "TCC", "Windows", "Microsoft", "Outlook", "Hotmail",
    "Little Snitch", "iTerm", "iTerm2", "Parallels", "KeePass", "KeePassXC", "1Password", "Bitwarden", "LastPass",
    "Have I Been Pwned",
];

/// The brand starting at `at`: its length. A space in a name matches any run of whitespace. The
/// name must not follow a word character, a dot or a dash, nor be followed by a word character.
/// Alternatives are tried in order, as the regular expression of the old tool did.
fn brand_at(text: &str, at: usize) -> Option<usize> {
    if let Some(before) = text[..at].chars().next_back() {
        if is_word(before) || before == '.' || before == '-' {
            return None;
        }
    }
    'names: for name in BRANDS.iter() {
        let mut pos = at;
        let mut parts = name.split(' ').peekable();
        while let Some(part) = parts.next() {
            if !text[pos..].starts_with(part) {
                continue 'names;
            }
            pos += part.len();
            if parts.peek().is_some() {
                let blanks = text[pos..].len() - text[pos..].trim_start().len();
                if blanks == 0 {
                    continue 'names;
                }
                pos += blanks;
            }
        }
        if text[pos..].chars().next().is_some_and(is_word) {
            continue;
        }
        return Some(pos - at);
    }
    None
}

/// Every brand occurrence: (byte offset, length).
pub fn find_brands(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if !text.is_char_boundary(at) {
            at += 1;
            continue;
        }
        match brand_at(text, at) {
            Some(len) => {
                found.push((at, len));
                at += len;
            }
            None => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// Is the occurrence inside one of the phrases the project allows?
fn allowed(file: &str, text: &str, at: usize, len: usize) -> bool {
    let inside = |phrase: &str| text.match_indices(phrase).any(|(i, _)| at >= i && at + len <= i + phrase.len());
    if (file == "shell.json" || file.ends_with("/shell.json")) && inside("Show All Windows") {
        return true;
    }
    let help_windows = file
        .strip_prefix("assets/help/")
        .and_then(|rest| rest.strip_suffix("/windows.md"))
        .is_some_and(|lang| !lang.is_empty() && lang.bytes().all(|b| b.is_ascii_lowercase()));
    if help_windows {
        // a line that is exactly "title: Windows" or "# Windows"
        let mut start = 0;
        for line in text.split('\n') {
            let end = start + line.len();
            let body = line.strip_suffix('\r').unwrap_or(line);
            if (body == "title: Windows" || body == "# Windows") && at >= start && at + len <= start + body.len() {
                return true;
            }
            start = end + 1;
        }
    }
    file.starts_with("assets/help/") && inside("[Windows](help:windows)")
}

fn walk(root: &Path, dir: &str, keep: &dyn Fn(&str) -> bool, out: &mut Vec<String>) {
    for entry in entries(&root.join(dir)) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{dir}/{name}");
        if entry.path().is_dir() {
            if name != "node_modules" {
                walk(root, &rel, keep, out);
            }
        } else if keep(&rel) {
            out.push(rel);
        }
    }
}

fn files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, "assets/locales", &|p| p.ends_with(".json"), &mut out);
    walk(root, "assets/help", &|p| p.ends_with(".md"), &mut out);
    walk(root, "apps", &|p| p.ends_with("/manifest.json"), &mut out);
    walk(root, "themes", &|p| p.ends_with("/theme.json"), &mut out);
    walk(root, "docs/packs", &|p| p.ends_with(".json"), &mut out);
    for name in ["README.md", "CHANGELOG.md"] {
        if root.join(name).exists() {
            out.push(name.to_string());
        }
    }
    out
}

pub struct Problem {
    pub file: String,
    pub line: usize,
    pub name: String,
}

pub fn check_brands(root: &Path, quiet: bool) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in files(root) {
        let Ok(bytes) = fs::read(root.join(&file)) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        for (at, len) in find_brands(&text) {
            if allowed(&file, &text, at, len) {
                continue;
            }
            problems.push(Problem { file: file.clone(), line: line_of(&text, at), name: text[at..at + len].to_string() });
        }
    }
    if !quiet {
        for p in &problems {
            println!("✗ brand name \"{}\" in {}:{}", p.name, p.file, p.line);
        }
        println!("brands: {}", if problems.is_empty() { "ok".to_string() } else { format!("{} problem(s)", problems.len()) });
    }
    problems
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() {
        return Err("brand-check takes no arguments".into());
    }
    Ok(ExitCode::from(u8::from(!check_brands(repo, false).is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<&str> {
        find_brands(text).into_iter().map(|(at, len)| &text[at..at + len]).collect()
    }

    #[test]
    fn whole_words_only_and_the_longest_listed_alternative_that_fits() {
        assert_eq!(names("Like macOS and Windows, not windows or Macro or iOSx"), ["macOS", "Windows"]);
        assert_eq!(names("a MacBook, a Mac, a Mac OS and a Mac  OS"), ["MacBook", "Mac", "Mac OS", "Mac  OS"]);
        assert_eq!(names("Time\tMachine and Have I Been Pwned"), ["Time\tMachine", "Have I Been Pwned"]);
        assert_eq!(names("iTerm2 then iTerm"), ["iTerm2", "iTerm"]);
    }

    #[test]
    fn a_word_dot_or_dash_in_front_hides_the_name() {
        assert_eq!(names("x-Windows .Windows aWindows (Windows) \"Windows\""), ["Windows", "Windows"]);
        assert_eq!(names("é Windows"), ["Windows"]);
    }

    #[test]
    fn the_allow_list_covers_only_its_phrases() {
        let shell = "{\"a\": \"Show All Windows\", \"b\": \"Windows\"}";
        let first = shell.find("Windows").unwrap();
        assert!(allowed("assets/locales/en/shell.json", shell, first, 7));
        let second = shell.rfind("Windows").unwrap();
        assert!(!allowed("assets/locales/en/shell.json", shell, second, 7));
        assert!(!allowed("assets/locales/en/other.json", shell, first, 7));
        let help = "title: Windows\n# Windows\nAbout Windows\n";
        assert!(allowed("assets/help/en/windows.md", help, 7, 7));
        assert!(allowed("assets/help/pt/windows.md", help, 17, 7));
        assert!(!allowed("assets/help/en/windows.md", help, 31, 7));
        assert!(!allowed("assets/help/en/other.md", help, 7, 7));
        let link = "see [Windows](help:windows) now";
        assert!(allowed("assets/help/en/x.md", link, 5, 7));
        assert!(!allowed("docs/x.md", link, 5, 7));
    }
}
