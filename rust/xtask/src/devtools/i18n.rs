//! The language catalogs: `i18n-check` compares them with English and with the keys the code uses,
//! and `i18n-pack` builds a language pack (a single JSON file) to translate or share.
//!
//!   cargo xtask i18n-check              every language in assets/locales, the error codes and brands
//!   cargo xtask i18n-check pt           just one (or a pack: path to lordos-lang-xx.json)
//!   cargo xtask i18n-pack new fr "Français" French one-other fr_FR
//!                                       lordos-lang-fr.json with every key in English, to translate
//!   cargo xtask i18n-pack export pt     lordos-lang-pt.json with the pt language as it is
//!
//! A pack is installed by copying it to ~/Downloads and using Settings → Language and region → "Add".
//! Plural rules: one-other, zero-one-other, none, east-slavic, polish, czech-slovak, arabic
//! (sdk/i18n.js). The direction ("rtl" for Arabic, Hebrew, Persian, Urdu…) comes from the id.

use super::ojson::{self, Json};
use super::{brand, entries, errcheck};
use crate::Result;
use regex::Regex;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

const REF: &str = "en";

struct Lang {
    id: String,
    cats: Vec<(String, Json)>,
}

fn read_json(path: &Path) -> Result<Json> {
    let text = crate::at(path, fs::read_to_string(path))?;
    ojson::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_lang(root: &Path, id: &str) -> Result<Lang> {
    if id.ends_with(".json") && Path::new(id).exists() {
        let pack = read_json(Path::new(id))?;
        let meta_id = pack.get("meta").and_then(|m| m.get("id")).and_then(Json::as_str).unwrap_or("").to_string();
        let cats = pack.get("catalogs").ok_or("the pack has no catalogs")?.fields();
        // format.json is not texts
        return Ok(Lang { id: meta_id, cats: cats.iter().filter(|(k, _)| k != "format").cloned().collect() });
    }
    let dir = root.join("assets/locales").join(id);
    if !dir.is_dir() {
        return Err(format!("{}: no such language", dir.display()));
    }
    let mut cats = Vec::new();
    for entry in entries(&dir) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".json") {
            if name != "meta.json" && name != "format.json" {
                cats.push((stem.to_string(), read_json(&entry.path())?));
            }
        }
    }
    Ok(Lang { id: id.to_string(), cats })
}

/// "namespace.key" → value, in catalog order.
fn flat(cats: &[(String, Json)]) -> Vec<(String, &Json)> {
    cats.iter().flat_map(|(ns, o)| o.fields().iter().map(move |(k, v)| (format!("{ns}.{k}"), v))).collect()
}

/// The `{name}` variables of a text, or of every form of a plural, sorted and joined by commas.
fn vars(value: &Json) -> String {
    static VAR: OnceLock<Regex> = OnceLock::new();
    let var = VAR.get_or_init(|| Regex::new(r"\{[A-Za-z0-9_]+\}").unwrap());
    let text = match value {
        Json::Object(fields) => fields.iter().map(|(_, v)| plain(v)).collect::<Vec<_>>().join(" "),
        other => plain(other),
    };
    let set: BTreeSet<&str> = var.find_iter(&text).map(|m| m.as_str()).collect();
    set.into_iter().collect::<Vec<_>>().join(",")
}

fn plain(value: &Json) -> String {
    match value {
        Json::Str(s) | Json::Number(s) => s.clone(),
        Json::Bool(b) => b.to_string(),
        Json::Null => "null".into(),
        Json::Array(items) => items.iter().map(plain).collect::<Vec<_>>().join(","),
        Json::Object(_) => "[object Object]".into(),
    }
}

/// Keys used in the code, `t('namespace.key')`, with the first file in order and the last file that uses it.
fn used_keys(root: &Path) -> Vec<(String, String)> {
    static USE: OnceLock<Regex> = OnceLock::new();
    static COMMENT: OnceLock<Regex> = OnceLock::new();
    let key = USE.get_or_init(|| {
        Regex::new(r"(?-u:\b)(?:t|T|tr)\(\s*'([a-z][A-Za-z0-9_-]*\.[A-Za-z0-9_.-]+)'").unwrap()
    });
    let comment = COMMENT.get_or_init(|| Regex::new(r"(?m)^\s*//.*$").unwrap());
    let mut order: Vec<String> = Vec::new();
    let mut files: HashMap<String, String> = HashMap::new();
    fn walk(root: &Path, dir: &Path, key: &Regex, comment: &Regex, order: &mut Vec<String>, files: &mut HashMap<String, String>) {
        for entry in entries(dir) {
            let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
            if path.is_dir() {
                if name != "node_modules" && name != "assets" {
                    walk(root, &path, key, comment, order, files);
                }
                continue;
            }
            let script = name.ends_with(".js") || name.ends_with(".jsx") || name.ends_with(".mjs");
            if !script || name.ends_with(".bundle.js") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let src = comment.replace_all(&String::from_utf8_lossy(&bytes), "").into_owned();
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            for m in key.captures_iter(&src) {
                let k = m[1].to_string();
                if !files.contains_key(&k) {
                    order.push(k.clone());
                }
                files.insert(k, rel.clone());
            }
        }
    }
    for dir in ["apps", "themes", "sdk"] {
        walk(root, &root.join(dir), key, comment, &mut order, &mut files);
    }
    order.into_iter().map(|k| { let f = files[&k].clone(); (k, f) }).collect()
}

pub fn check(root: &Path, args: &[String]) -> Result<ExitCode> {
    if args.len() > 1 {
        return Err("usage: i18n-check [LANG | lordos-lang-xx.json]".into());
    }
    let reference = load_lang(root, REF)?;
    let reference_flat = flat(&reference.cats);
    let ref_map: HashMap<&str, &Json> = reference_flat.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let mut bad = 0usize;
    for (k, file) in used_keys(root) {
        if !ref_map.contains_key(k.as_str()) {
            println!("✗ {k} (used in {file}) is not in {REF}");
            bad += 1;
        }
    }
    let langs: Vec<String> = match args.first() {
        Some(one) => vec![one.clone()],
        None => entries(&root.join("assets/locales"))
            .iter()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|d| d != REF && root.join("assets/locales").join(d).join("meta.json").exists())
            .collect(),
    };
    for id in langs {
        let lang = load_lang(root, &id)?;
        let cur = flat(&lang.cats);
        let cur_map: HashMap<&str, &Json> = cur.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let missing: Vec<&str> = reference_flat.iter().map(|(k, _)| k.as_str()).filter(|k| !cur_map.contains_key(k)).collect();
        let extra: Vec<&str> = cur.iter().map(|(k, _)| k.as_str()).filter(|k| !ref_map.contains_key(k)).collect();
        let var_diff: Vec<&str> = reference_flat
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| cur_map.get(k).is_some_and(|v| vars(v) != vars(ref_map[k])))
            .collect();
        let mut line = format!("{}: {}/{} keys", lang.id, cur.len(), reference_flat.len());
        if !missing.is_empty() {
            line += &format!(", {} missing", missing.len());
        }
        if !extra.is_empty() {
            line += &format!(", {} extra", extra.len());
        }
        if !var_diff.is_empty() {
            line += &format!(", different variables in {}", var_diff.len());
        }
        println!("{line}");
        for k in &missing {
            println!("  missing {k}");
        }
        for k in &extra {
            println!("  extra   {k}");
        }
        for k in &var_diff {
            println!("  vars    {k}: {} ≠ {}", vars(ref_map[k]), vars(cur_map[k]));
        }
        bad += missing.len() + var_diff.len();
    }
    if args.is_empty() {
        bad += errcheck::check_errors(root, false).problems.len();
        bad += brand::check_brands(root, false).len();
    }
    Ok(ExitCode::from(u8::from(bad > 0)))
}

/// Languages written right to left: the whole interface is mirrored.
const RTL: [&str; 11] = ["ar", "he", "fa", "ur", "yi", "ps", "dv", "sd", "ug", "ckb", "syr"];

fn valid_id(id: &str) -> bool {
    let (base, region) = match id.split_once('-') {
        Some((b, r)) => (b, Some(r)),
        None => (id, None),
    };
    (2..=3).contains(&base.len())
        && base.bytes().all(|b| b.is_ascii_lowercase())
        && region.is_none_or(|r| (2..=4).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphabetic()))
}

/// Reads a bundled language as `{ meta, catalogs }`.
fn read_pack(root: &Path, id: &str) -> Result<Json> {
    let dir = root.join("assets/locales").join(id);
    let (mut meta, mut cats) = (Json::Object(Vec::new()), Vec::new());
    for entry in entries(&dir) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let value = read_json(&entry.path())?;
        if name == "meta.json" {
            meta = value;
        } else {
            cats.push((name.get(..name.len().saturating_sub(5)).unwrap_or("").to_string(), value));
        }
    }
    Ok(Json::Object(vec![("meta".into(), meta), ("catalogs".into(), Json::Object(cats))]))
}

pub fn build_pack(root: &Path, cmd: &str, id: &str, name: &str, english: &str, plural: &str, region: &str) -> Result<Json> {
    match cmd {
        "export" => read_pack(root, id),
        "new" => {
            let en = read_pack(root, "en")?;
            let direction = if RTL.contains(&id.split('-').next().unwrap_or("")) { "rtl" } else { "ltr" };
            let s = |v: &str| Json::Str(v.to_string());
            let meta = Json::Object(vec![
                ("id".into(), s(id)),
                ("name".into(), s(if name.is_empty() { id } else { name })),
                ("english".into(), s(if english.is_empty() { id } else { english })),
                ("plural".into(), s(plural)),
                ("region".into(), s(region)),
                ("direction".into(), s(direction)),
            ]);
            Ok(Json::Object(vec![("meta".into(), meta), ("catalogs".into(), en.get("catalogs").cloned().unwrap())]))
        }
        _ => Err("unknown command".into()),
    }
}

pub fn pack(root: &Path, args: &[String]) -> Result<ExitCode> {
    let arg = |i: usize| args.get(i).map(String::as_str);
    let (cmd, id) = (arg(0).unwrap_or(""), arg(1).unwrap_or(""));
    if !valid_id(id) {
        eprintln!("usage: i18n-pack new <id> \"<name>\" <English name> [plural] [region]  |  export <id>");
        return Ok(ExitCode::from(2));
    }
    if cmd != "new" && cmd != "export" {
        eprintln!("unknown command");
        return Ok(ExitCode::from(2));
    }
    let value = build_pack(root, cmd, id, arg(2).unwrap_or(""), arg(3).unwrap_or(""), arg(4).unwrap_or("one-other"), arg(5).unwrap_or(""))?;
    let out = PathBuf::from(format!("lordos-lang-{id}.json"));
    crate::at(&out, fs::write(&out, format!("{}\n", ojson::pretty(&value))))?;
    let strings: usize = value.get("catalogs").map_or(0, |c| c.fields().iter().map(|(_, v)| v.fields().len()).sum());
    println!("{}: {strings} strings", out.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    fn lang(dir: &Path, id: &str, files: &[(&str, &str)]) {
        let path = dir.join("assets/locales").join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("meta.json"), format!("{{\"id\":\"{id}\"}}")).unwrap();
        for (name, text) in files {
            fs::write(path.join(name), text).unwrap();
        }
    }

    #[test]
    fn variables_are_a_sorted_set_over_every_plural_form() {
        let v = ojson::parse("{\"one\":\"{n} file in {dir}\",\"other\":\"{n} files\"}").unwrap();
        assert_eq!(vars(&v), "{dir},{n}");
        assert_eq!(vars(&Json::Str("no variables { x } {a_b}".into())), "{a_b}");
    }

    #[test]
    fn ids_are_two_or_three_letters_with_an_optional_region() {
        for good in ["pt", "fil", "pt-BR", "zh-Hans"] {
            assert!(valid_id(good), "{good}");
        }
        for bad in ["", "p", "pt_BR", "PT", "pt-", "pt-B", "abcd"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_new_pack_copies_english_and_sets_the_direction() {
        let dir = TestDir::new("i18n-pack");
        lang(&dir.0, "en", &[("apps.json", "{\"b\":\"B {x}\",\"a\":\"A\"}"), ("format.json", "{\"d\":\"x\"}")]);
        let pack = build_pack(&dir.0, "new", "ar-EG", "العربية", "Arabic", "arabic", "ar_EG").unwrap();
        assert_eq!(pack.get("meta").unwrap().get("direction"), Some(&Json::Str("rtl".into())));
        let text = ojson::pretty(&pack);
        assert!(text.starts_with("{\n  \"meta\": {\n    \"id\": \"ar-EG\",\n    \"name\": \"العربية\",\n    \"english\": \"Arabic\",\n    \"plural\": \"arabic\",\n    \"region\": \"ar_EG\",\n    \"direction\": \"rtl\"\n  },\n  \"catalogs\": {\n    \"apps\": {\n      \"b\": \"B {x}\",\n      \"a\": \"A\"\n    },"), "{text}");
        let exported = build_pack(&dir.0, "export", "en", "", "", "", "").unwrap();
        assert_eq!(exported.get("meta").unwrap().get("id"), Some(&Json::Str("en".into())));
        assert!(build_pack(&dir.0, "other", "en", "", "", "", "").is_err());
    }

    #[test]
    fn the_check_flags_used_keys_missing_keys_and_changed_variables() {
        let dir = TestDir::new("i18n-check");
        lang(&dir.0, "en", &[("apps.json", "{\"a\":\"A {n}\",\"b\":\"B\"}")]);
        lang(&dir.0, "pt", &[("apps.json", "{\"a\":\"A\",\"z\":\"Z\"}")]);
        fs::create_dir_all(dir.0.join("apps/x")).unwrap();
        fs::write(dir.0.join("apps/x/index.jsx"), "// t('apps.skipped')\nt('apps.a'); tr('apps.nope'); xt('apps.no')").unwrap();
        assert_eq!(used_keys(&dir.0), [("apps.a".to_string(), "apps/x/index.jsx".to_string()), ("apps.nope".to_string(), "apps/x/index.jsx".to_string())]);
        let en = load_lang(&dir.0, "en").unwrap();
        let pt = load_lang(&dir.0, "pt").unwrap();
        let (en_flat, pt_flat) = (flat(&en.cats), flat(&pt.cats));
        assert_eq!(en_flat.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["apps.a", "apps.b"]);
        assert_ne!(vars(en_flat[0].1), vars(pt_flat[0].1));
        assert_eq!(check(&dir.0, &["pt".to_string()]).unwrap(), ExitCode::from(1));
    }
}
