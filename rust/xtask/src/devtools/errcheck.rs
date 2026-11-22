//! Checks that the server reports errors and statuses as codes, not prose.
//!
//! Scans server/src (C, without comments) and reports:
//!   - codes used with admin_err/admin_ok/admin_log, lerr_set, js_throw_code, js_throw_line or
//!     put_line(…, "ERR", …) that have no English string in assets/locales/en/errors.json (ERR) or
//!     status.json (OK/LOG);
//!   - prose in ERR/OK/LOG lines (admin_say("ERR", "could not…")) and in JS exceptions
//!     (JS_ThrowTypeError(c, "…")): they must be js_throw_code / admin_err with a code;
//!   - Spanish prose in any string literal (the project is English-only; UI text goes in the
//!     locale files).
//! A line can opt out with a trailing /* i18n-ok */ comment. Files that other work still has to
//! convert are listed in PENDING: their findings are warnings. See docs/LANGUAGES.md, "Error and
//! status messages".

use super::{entries, line_of, ojson};
use crate::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

/// Out of this pass's scope (engine and app-mode work): warnings until converted.
const PENDING: [&str; 2] = ["ui/paint.c", "ui/text.c"];

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a fixed pattern"))
}

fn is_code(text: &str) -> bool {
    static CODE: OnceLock<Regex> = OnceLock::new();
    re(&CODE, r"^[a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9]+)+$").is_match(text)
}

fn is_spanish(text: &str) -> bool {
    static SPANISH: OnceLock<Regex> = OnceLock::new();
    re(
        &SPANISH,
        r"(?i)[áéíóúñ¿¡]|(?-u:\b)(?:no se pudo|no hay|falta|faltan|inválid[oa]s?|desconocid[oa]s?|archivo|carpeta|contraseña|usuario|paquete|también|esencial|ayudante|conectad[oa]|desconectad[oa]|error al|demasiad[oa]s?|todavía|ningún|ninguna)(?-u:\b)",
    )
    .is_match(text)
}

/// The source with comments replaced by spaces (line numbers stay), and the lines whose comment
/// says `i18n-ok`.
pub fn strip_comments(src: &str) -> (String, HashSet<usize>) {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut ok = HashSet::new();
    let (mut i, mut line) = (0, 1);
    while i < b.len() {
        let (ch, next) = (b[i], b.get(i + 1).copied());
        if ch == b'"' || ch == b'\'' {
            out.push(ch);
            i += 1;
            while i < b.len() && b[i] != ch && b[i] != b'\n' {
                if b[i] == b'\\' {
                    out.push(b[i]);
                    if let Some(&n) = b.get(i + 1) {
                        out.push(n);
                    }
                    i += 2;
                    continue;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push(b[i]);
                i += 1;
            }
            continue;
        }
        if ch == b'/' && next == Some(b'*') {
            let end = src[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            let body = &b[i..end];
            if String::from_utf8_lossy(body).contains("i18n-ok") {
                ok.insert(line);
            }
            for &c in body {
                if c == b'\n' {
                    out.push(b'\n');
                    line += 1;
                } else if c & 0xc0 != 0x80 {
                    out.push(b' '); // one space per character, not per byte
                }
            }
            i = end;
            continue;
        }
        if ch == b'/' && next == Some(b'/') {
            let end = src[i..].find('\n').map_or(b.len(), |e| i + e);
            if String::from_utf8_lossy(&b[i..end]).contains("i18n-ok") {
                ok.insert(line);
            }
            out.extend(std::iter::repeat(b' ').take(src[i..end].chars().count()));
            i = end;
            continue;
        }
        if ch == b'\n' {
            line += 1;
        }
        out.push(ch);
        i += 1;
    }
    (String::from_utf8_lossy(&out).into_owned(), ok)
}

/// The C string literal starting at `s[i]` (a `"`), with adjacent literals joined: (text, end).
pub fn literal(s: &str, mut i: usize) -> (String, usize) {
    let b = s.as_bytes();
    let mut text: Vec<u8> = Vec::new();
    while b.get(i) == Some(&b'"') {
        i += 1;
        while i < b.len() && b[i] != b'"' {
            if b[i] == b'\\' {
                text.push(b[i]);
                if let Some(&n) = b.get(i + 1) {
                    text.push(n);
                }
                i += 2;
            } else {
                text.push(b[i]);
                i += 1;
            }
        }
        i += 1;
        let window_end = (i + 200).min(b.len());
        let blanks = b[i.min(window_end)..window_end].iter().take_while(|c| c.is_ascii_whitespace()).count();
        if b.get(i + blanks) == Some(&b'"') {
            i += blanks;
        } else {
            break;
        }
    }
    (String::from_utf8_lossy(&text).into_owned(), i)
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in entries(dir) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("c" | "h")) {
            out.push(path);
        }
    }
}

fn load_keys(root: &Path, namespace: &str) -> HashSet<String> {
    let path = root.join(format!("assets/locales/en/{namespace}.json"));
    match fs::read_to_string(path).ok().and_then(|t| ojson::parse(&t).ok()) {
        Some(value) => value.fields().iter().map(|(k, _)| k.clone()).collect(),
        None => HashSet::new(),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct Report {
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub codes: usize,
}

struct Use {
    ns: &'static str,
    where_: String,
    pending: bool,
}

pub fn check_errors(root: &Path, quiet: bool) -> Report {
    let src_dir = root.join("server/src");
    let (errors, status) = (load_keys(root, "errors"), load_keys(root, "status"));
    let (mut problems, mut warnings) = (Vec::new(), Vec::new());
    let mut order: Vec<String> = Vec::new();
    let mut used: HashMap<String, Use> = HashMap::new();
    let mut files = Vec::new();
    walk(&src_dir, &mut files);

    static ADMIN: OnceLock<Regex> = OnceLock::new();
    static ADMIN_CODE: OnceLock<Regex> = OnceLock::new();
    static LERR: OnceLock<Regex> = OnceLock::new();
    static THROW: OnceLock<Regex> = OnceLock::new();
    static PUT_LINE: OnceLock<Regex> = OnceLock::new();
    static SAY: OnceLock<Regex> = OnceLock::new();
    static JS_THROW: OnceLock<Regex> = OnceLock::new();
    // each pattern ends right before the literal's opening quote, which is a look-ahead in the old tool
    let admin = re(&ADMIN, r#"\badmin_(err|ok|log)\s*\(\s*"#);
    let admin_code = re(&ADMIN_CODE, r#"\badmin_code\s*\(\s*"(ERR|OK|LOG)"\s*,\s*"#);
    let lerr = re(&LERR, r#"\blerr_set\s*\(\s*[^,;]+,\s*[^,;]+,\s*"#);
    let throw = re(&THROW, r#"\bjs_throw_(?:code|line)\s*\(\s*\w+\s*,\s*"#);
    let put_line = re(&PUT_LINE, r#"\bput_line\s*\([^,;]+,\s*"ERR"\s*,\s*"#);
    let say = re(&SAY, r#"\badmin_say\s*\(\s*"(ERR|OK|LOG)"\s*,\s*"#);
    let js_throw = re(&JS_THROW, r#"\bJS_Throw(?:Type|Range|Internal|Syntax|Reference)?Error\s*\(\s*\w+\s*,\s*"#);

    for file in files {
        let rel = file.strip_prefix(&src_dir).unwrap().to_string_lossy().replace('\\', "/");
        if rel == "errcode.h" || rel == "ui/js_err.h" {
            continue; // documentation examples
        }
        let pending = PENDING.iter().any(|p| if p.ends_with('/') { rel.starts_with(p) } else { rel == *p });
        let Ok(bytes) = fs::read(&file) else { continue };
        let (src, ok) = strip_comments(&String::from_utf8_lossy(&bytes));
        let mut report = |at: usize, msg: String| {
            let line = line_of(&src, at);
            if ok.contains(&line) {
                return;
            }
            (if pending { &mut warnings } else { &mut problems }).push(format!("server/src/{rel}:{line}: {msg}"));
        };
        let mut use_code = |at: usize, code: String, ns: &'static str, report: &mut dyn FnMut(usize, String)| {
            if !is_code(&code) {
                report(at, format!("\"{code}\" is not a code (module.camelCase)"));
                return;
            }
            if !used.contains_key(&code) {
                order.push(code.clone());
                used.insert(code, Use { ns, where_: format!("server/src/{rel}:{}", line_of(&src, at)), pending });
            }
        };
        // a match only counts when a literal follows (the end of the match is its opening quote)
        let quoted = |m: &regex::Match| src.as_bytes().get(m.end()) == Some(&b'"');

        for m in admin.captures_iter(&src) {
            let whole = m.get(0).unwrap();
            if !quoted(&whole) {
                continue;
            }
            let (code, _) = literal(&src, whole.end());
            let ns = if &m[1] == "err" { "errors" } else { "status" };
            use_code(whole.start(), code, ns, &mut report);
        }
        for m in admin_code.captures_iter(&src) {
            let whole = m.get(0).unwrap();
            if !quoted(&whole) {
                continue;
            }
            let (code, _) = literal(&src, whole.end());
            let ns = if &m[1] == "ERR" { "errors" } else { "status" };
            use_code(whole.start(), code, ns, &mut report);
        }
        for m in lerr.find_iter(&src).filter(|m| quoted(m)) {
            let (code, _) = literal(&src, m.end());
            use_code(m.start(), code, "any", &mut report);
        }
        for m in throw.find_iter(&src).filter(|m| quoted(m)) {
            let (code, _) = literal(&src, m.end());
            use_code(m.start(), code, "errors", &mut report);
        }
        for m in put_line.find_iter(&src).filter(|m| quoted(m)) {
            let (code, _) = literal(&src, m.end());
            if is_code(&code) {
                use_code(m.start(), code, "errors", &mut report);
            } else {
                report(m.start(), format!("prose in an ERR line: \"{code}\""));
            }
        }
        for m in say.captures_iter(&src) {
            let whole = m.get(0).unwrap();
            if !quoted(&whole) {
                continue;
            }
            let (text, _) = literal(&src, whole.end());
            if text == "%s" && &m[1] == "LOG" {
                continue; // a program's output, as is
            }
            let tag = &m[1];
            let hint = if tag == "ERR" { "err".to_string() } else { tag.to_lowercase() };
            report(whole.start(), format!("prose in an {tag} line: \"{text}\" (use admin_{hint} with a code)"));
        }
        for m in js_throw.find_iter(&src).filter(|m| quoted(m)) {
            let (text, _) = literal(&src, m.end());
            report(m.start(), format!("JS exception with prose: \"{text}\" (use js_throw_code / js_throw_line)"));
        }
        // Spanish in any string literal
        let b = src.as_bytes();
        let mut next = src.find('"');
        while let Some(i) = next {
            if i > 0 && b[i - 1] == b'\'' && b.get(i + 1) == Some(&b'\'') {
                next = src[i + 1..].find('"').map(|e| i + 1 + e); // '"'
                continue;
            }
            let (text, end) = literal(&src, i);
            if is_spanish(&text) {
                let shown = if text.chars().count() > 70 { format!("{}…", text.chars().take(70).collect::<String>()) } else { text.clone() };
                report(i, format!("Spanish text in a string: \"{shown}\""));
            }
            let from = end.max(i + 1).min(src.len());
            next = src[from..].find('"').map(|e| from + e);
        }
    }
    for code in &order {
        let u = &used[code];
        let ok = match u.ns {
            "errors" => errors.contains(code),
            "status" => status.contains(code),
            _ => errors.contains(code) || status.contains(code),
        };
        if !ok {
            let file = if u.ns == "any" { "errors.json or status.json".to_string() } else { format!("{}.json", u.ns) };
            (if u.pending { &mut warnings } else { &mut problems })
                .push(format!("{}: code {code} has no English string in {file}", u.where_));
        }
    }
    if !quiet {
        for w in &warnings {
            println!("  (pending) {w}");
        }
        for p in &problems {
            println!("✗ {p}");
        }
        let extra = if warnings.is_empty() { String::new() } else { format!(", {} warnings in files still pending", warnings.len()) };
        println!("errors: {} codes in the server, {} problems{extra}", order.len(), problems.len());
    }
    Report { problems, warnings, codes: order.len() }
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() {
        return Err("err-check takes no arguments".into());
    }
    Ok(ExitCode::from(u8::from(!check_errors(repo, false).problems.is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    #[test]
    fn comments_become_spaces_and_mark_their_line() {
        let (code, ok) = strip_comments("a(); /* x\n y i18n-ok */ b(\"//no\"); // tail i18n-ok\nc();");
        assert_eq!(code.lines().count(), 3);
        assert!(code.contains("b(\"//no\");") && !code.contains("tail") && !code.contains(" y "));
        // a block comment marks the line it starts on, a line comment its own line
        assert_eq!(ok, HashSet::from([1, 2]));
    }

    #[test]
    fn adjacent_literals_join_and_escapes_stay_raw() {
        let src = "f(\"ab\"  \"c\\\"d\" , \"z\")";
        let (text, end) = literal(src, 2);
        assert_eq!(text, "abc\\\"d");
        assert_eq!(&src[end..], " , \"z\")");
        assert_eq!(literal("\"x\" y", 0).0, "x");
    }

    #[test]
    fn codes_and_spanish() {
        assert!(is_code("fan.notFound") && is_code("a.b.c"));
        assert!(!is_code("fan") && !is_code("Fan.x") && !is_code("fan.not found"));
        assert!(is_spanish("no se pudo abrir") && is_spanish("Ñandú") && is_spanish("Archivo roto"));
        assert!(!is_spanish("the file is fine") && !is_spanish("faltante"));
    }

    fn server(name: &str, source: &str, errors: &str) -> TestDir {
        let dir = TestDir::new(name);
        fs::create_dir_all(dir.0.join("server/src/ui")).unwrap();
        fs::create_dir_all(dir.0.join("assets/locales/en")).unwrap();
        fs::write(dir.0.join("server/src/a.c"), source).unwrap();
        fs::write(dir.0.join("server/src/ui/paint.c"), "admin_say(\"ERR\", \"pending prose\");").unwrap();
        fs::write(dir.0.join("assets/locales/en/errors.json"), errors).unwrap();
        fs::write(dir.0.join("assets/locales/en/status.json"), "{\"disk.full\":\"Full\"}").unwrap();
        dir
    }

    #[test]
    fn a_fixture_tree_reports_each_kind_of_finding() {
        let dir = server(
            "err-fixture",
            "admin_err(\"fan.known\"); admin_err(\"fan.unknown\"); admin_ok(\"disk.full\");\n\
             admin_say(\"ERR\", \"could not open\");\n\
             JS_ThrowTypeError(c, \"needs JSON\");\n\
             f(\"el archivo no existe\"); /* i18n-ok */\n\
             g(\"no hay nada\");\n\
             admin_err(\"notacode\");\n",
            "{\"fan.known\":\"Known\"}",
        );
        let report = check_errors(&dir.0, true);
        assert_eq!(
            report.problems,
            [
                "server/src/a.c:6: \"notacode\" is not a code (module.camelCase)",
                "server/src/a.c:2: prose in an ERR line: \"could not open\" (use admin_err with a code)",
                "server/src/a.c:3: JS exception with prose: \"needs JSON\" (use js_throw_code / js_throw_line)",
                "server/src/a.c:5: Spanish text in a string: \"no hay nada\"",
                "server/src/a.c:1: code fan.unknown has no English string in errors.json",
            ]
        );
        assert_eq!(report.warnings, ["server/src/ui/paint.c:1: prose in an ERR line: \"pending prose\" (use admin_err with a code)"]);
        assert_eq!(report.codes, 3);
    }
}
