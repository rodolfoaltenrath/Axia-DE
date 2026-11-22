//! The data of the emoji picker, from Unicode and CLDR.
//!
//!   cargo xtask gen-emoji FOLDER
//!
//! The folder must contain (downloaded once, not kept in the repository):
//!   emoji-test.txt                             https://unicode.org/Public/emoji/15.1/emoji-test.txt
//!   ann-{en,es,pt}.json, der-{en,es,pt}.json   cldr-json 45: cldr-annotations-full and
//!                                              cldr-annotations-derived-full (annotations.json)
//! Writes:
//!   assets/emoji.json                     shared category and skin-tone data (Rust and JS)
//!   sdk/emojidata.js                      compatibility exports for the shell picker
//!   assets/locales/{en,es,pt}/emojikw.json   name and keywords of each emoji per language
//! Data license: Unicode License v3 (Unicode and CLDR), see docs/DOCUMENTS.md.

use super::ojson::{self, Json};
use crate::{at, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const GROUPS: [(&str, &str); 9] = [
    ("Smileys & Emotion", "smileys"),
    ("People & Body", "people"),
    ("Animals & Nature", "animals"),
    ("Food & Drink", "food"),
    ("Travel & Places", "travel"),
    ("Activities", "activities"),
    ("Objects", "objects"),
    ("Symbols", "symbols"),
    ("Flags", "flags"),
];
const TONES: [&str; 5] = ["1f3fb", "1f3fc", "1f3fd", "1f3fe", "1f3ff"];

fn emoji(points: &[&str]) -> String {
    points.iter().filter_map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32)).collect()
}

/// What `emoji-test.txt` lists: the fully-qualified emojis by group, and which ones have skin tones.
#[derive(Default, Debug, PartialEq)]
pub struct Emojis {
    pub groups: Vec<(String, Vec<String>)>,
    pub toned: Vec<String>,
}

/// The code points and the emoji of a fully-qualified line: `1F600 ; fully-qualified # 😀 E1.0 grinning face`.
fn line_points(line: &str) -> Option<Vec<String>> {
    let (left, right) = line.split_once(';')?;
    let rest = right.trim_start().strip_prefix("fully-qualified")?;
    let after = rest.trim_start().strip_prefix('#')?;
    after.split_whitespace().next()?; // the emoji itself
    let left = left.trim_end();
    if left.is_empty() || !left.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b) || b == b' ') {
        return None;
    }
    Some(left.to_lowercase().split(' ').map(String::from).collect())
}

pub fn parse_list(text: &str) -> Emojis {
    let mut out = Emojis::default();
    let mut group: Option<&str> = None;
    let mut all: HashSet<String> = HashSet::new();
    for line in text.split('\n') {
        if let Some(name) = line.strip_prefix("# group: ") {
            group = GROUPS.iter().find(|(n, _)| *n == name).map(|(_, id)| *id);
            continue;
        }
        let (Some(points), Some(group)) = (line_points(line), group) else { continue };
        let refs: Vec<&str> = points.iter().map(String::as_str).collect();
        if refs.iter().any(|c| TONES.contains(c)) {
            // toned variant: the base (without any tone) is marked and it is not listed separately
            let base: Vec<&str> = refs.iter().copied().filter(|c| !TONES.contains(c)).collect();
            let ch = emoji(&base);
            if all.contains(&ch) {
                if !out.toned.contains(&ch) {
                    out.toned.push(ch);
                }
            } else {
                // the base may carry an FE0F that the variant lacks
                let mut alt_points = vec![base[0], "fe0f"];
                alt_points.extend_from_slice(&base[1..]);
                let alt = emoji(&alt_points);
                if all.contains(&alt) && !out.toned.contains(&alt) {
                    out.toned.push(alt);
                }
            }
            continue;
        }
        let ch = emoji(&refs);
        match out.groups.iter_mut().find(|(g, _)| g == group) {
            Some((_, list)) => list.push(ch.clone()),
            None => out.groups.push((group.to_string(), vec![ch.clone()])),
        }
        all.insert(ch);
    }
    out
}

/// `fe0f`-less hex of each code point, joined by `_`: the key of an emoji in the keyword files.
pub fn key(ch: &str) -> String {
    ch.chars().map(|c| format!("{:x}", c as u32)).filter(|h| h != "fe0f").collect::<Vec<_>>().join("_")
}

/// The annotation of each emoji: `[name, keywords...]` from the "tts" and "default" lists.
type Annotations = HashMap<String, (String, Vec<String>)>;

fn load_annotations(src: &Path, lang: &str) -> Result<Annotations> {
    let mut merged = Annotations::new();
    for (file, outer, inner) in [(format!("der-{lang}.json"), "annotationsDerived", "annotations"), (format!("ann-{lang}.json"), "annotations", "annotations")] {
        let path = src.join(file);
        let json = ojson::parse(&at(&path, fs::read_to_string(&path))?).map_err(|e| format!("{}: {e}", path.display()))?;
        let table = json.get(outer).and_then(|o| o.get(inner)).ok_or_else(|| format!("{}: no {outer}.{inner}", path.display()))?;
        for (ch, a) in table.fields() {
            let name = a.get("tts").map_or(&[][..], Json::items).first().and_then(Json::as_str).unwrap_or("").to_string();
            let defaults = a.get("default").map_or(&[][..], Json::items).iter().filter_map(|k| k.as_str().map(String::from)).collect();
            merged.insert(ch.clone(), (name, defaults));
        }
    }
    Ok(merged)
}

/// `"a | b | c"` for an emoji, or `None`.
fn describe(ann: &Annotations, en: &Annotations, ch: &str) -> Option<String> {
    let bare = ch.replace('\u{fe0f}', "");
    let (name, keywords) = ann.get(ch).or_else(|| ann.get(&bare)).or_else(|| en.get(ch)).or_else(|| en.get(&bare))?;
    let mut parts = vec![name.as_str()];
    parts.extend(keywords.iter().map(String::as_str).filter(|k| k != name));
    Some(parts.join(" | "))
}

/// Object keys in the order of a JavaScript object: integer-like keys first, ascending, then the
/// rest in insertion order.
fn js_order(entries: Vec<(String, String)>) -> Vec<(String, String)> {
    let index = |k: &str| -> Option<u64> {
        let canonical = k == "0" || (!k.starts_with('0') && !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()));
        canonical.then(|| k.parse::<u64>().ok()).flatten().filter(|&n| n < 4_294_967_295)
    };
    let mut numbers: Vec<(u64, (String, String))> = Vec::new();
    let mut others = Vec::new();
    for entry in entries {
        match index(&entry.0) {
            Some(n) => numbers.push((n, entry)),
            None => others.push(entry),
        }
    }
    numbers.sort_by_key(|(n, _)| *n);
    numbers.into_iter().map(|(_, e)| e).chain(others).collect()
}

/// `JSON.stringify(out).replace(/","/g, '",\n"')` of an object of strings.
fn keyword_file(entries: &[(String, String)]) -> String {
    let rows: Vec<String> = entries.iter().map(|(k, v)| format!("{}:{}", ojson::compact(&Json::str(k)), ojson::compact(&Json::str(v)))).collect();
    format!("{{{}}}\n", rows.join(",\n"))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let Some(src) = args.first().map(Path::new) else {
        eprintln!("usage: cargo xtask gen-emoji <folder with emoji-test.txt and the CLDR annotations>");
        return Ok(ExitCode::from(1));
    };
    let list_path = src.join("emoji-test.txt");
    let parsed = parse_list(&at(&list_path, fs::read_to_string(&list_path))?);
    let data = Json::Object(vec![
        ("groups".into(), Json::Array(parsed.groups.iter().map(|(id, list)| Json::Array(vec![Json::str(id), Json::Str(list.join(" "))])).collect())),
        ("toned".into(), Json::Str(parsed.toned.join(" "))),
    ]);
    let file = repo.join("assets/emoji.json");
    at(&file, fs::write(&file, format!("{}\n", ojson::pretty(&data))))?;
    let js = repo.join("sdk/emojidata.js");
    at(
        &js,
        fs::write(
            &js,
            "// Shared Unicode data, generated by cargo xtask gen-emoji; also compiled into Rust.\nimport data from '../assets/emoji.json';\nexport const EMOJI_GROUPS = data.groups;\nexport const TONED = new Set(data.toned.split(' '));\n",
        ),
    )?;
    let all: Vec<&String> = parsed.groups.iter().flat_map(|(_, l)| l).collect();
    let en = load_annotations(src, "en")?;
    for lang in ["en", "es", "pt"] {
        let own;
        let ann = if lang == "en" {
            &en
        } else {
            own = load_annotations(src, lang)?;
            &own
        };
        let mut out: Vec<(String, String)> = Vec::new();
        for ch in &all {
            let Some(text) = describe(ann, &en, ch) else { continue };
            let k = key(ch);
            match out.iter_mut().find(|(key, _)| *key == k) {
                Some(slot) => slot.1 = text,
                None => out.push((k, text)),
            }
        }
        let out = js_order(out);
        let target = repo.join(format!("assets/locales/{lang}/emojikw.json"));
        at(&target, fs::write(&target, keyword_file(&out)))?;
        println!("{lang}: {} emojis", out.len());
    }
    let groups: Vec<String> = parsed.groups.iter().map(|(g, l)| format!("{g} {}", l.len())).collect();
    println!("groups: {}; toned: {}", groups.join(", "), parsed.toned.len());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# group: Smileys & Emotion\n\
        1F600                                                  ; fully-qualified     # 😀 E1.0 grinning face\n\
        263A FE0F                                              ; fully-qualified     # ☺️ E0.6 smiling face\n\
        263A                                                   ; unqualified         # ☺ E0.6 smiling face\n\
        # group: People & Body\n\
        1F44B                                                  ; fully-qualified     # 👋 E0.6 waving hand\n\
        1F44B 1F3FB                                            ; fully-qualified     # 👋🏻 E1.0 waving hand: light skin tone\n\
        1F9D1 1F3FC 200D 1F91D 200D 1F9D1 1F3FD                ; fully-qualified     # 🧑🏼‍🤝‍🧑🏽 E12.0 people holding hands\n\
        # group: Component\n\
        1F3FB                                                  ; component           # 🏻 E1.0 light skin tone\n\
        # group: Flags\n\
        1F1E7 1F1F7                                            ; fully-qualified     # 🇧🇷 E0.6 flag: Brazil\n";

    #[test]
    fn the_list_gives_groups_in_order_and_marks_the_toned_bases() {
        let parsed = parse_list(SAMPLE);
        let groups: Vec<(&str, usize)> = parsed.groups.iter().map(|(g, l)| (g.as_str(), l.len())).collect();
        assert_eq!(groups, [("smileys", 2), ("people", 1), ("flags", 1)]);
        assert_eq!(parsed.groups[0].1, ["😀", "☺\u{fe0f}"]);
        assert_eq!(parsed.toned, ["👋"]);
    }

    #[test]
    fn a_line_must_be_fully_qualified_hex_and_followed_by_the_emoji() {
        assert_eq!(line_points("1F600 ; fully-qualified # 😀 x"), Some(vec!["1f600".to_string()]));
        for bad in ["1F600 ; minimally-qualified # 😀 x", "1F600 ; fully-qualified #", "1f600 ; fully-qualified # x", "zz ; fully-qualified # x", "; fully-qualified # x", "plain text"] {
            assert_eq!(line_points(bad), None, "{bad}");
        }
    }

    #[test]
    fn keys_drop_fe0f_and_object_order_puts_integer_keys_first() {
        assert_eq!(key("☺\u{fe0f}"), "263a");
        assert_eq!(key("🧑\u{200d}🤝\u{200d}🧑"), "1f9d1_200d_1f91d_200d_1f9d1");
        let ordered = js_order(vec![("1f600".into(), "a".into()), ("2122".into(), "b".into()), ("263a".into(), "c".into()), ("21".into(), "d".into()), ("0123".into(), "e".into())]);
        assert_eq!(ordered.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["21", "2122", "1f600", "263a", "0123"]);
        assert_eq!(keyword_file(&ordered[..2]), "{\"21\":\"d\",\n\"2122\":\"b\"}\n");
        assert_eq!(keyword_file(&[]), "{}\n");
    }

    #[test]
    fn descriptions_use_the_name_first_and_fall_back_to_english() {
        let mut es = Annotations::new();
        es.insert("😀".into(), ("cara sonriente".into(), vec!["cara".into(), "cara sonriente".into(), "sonrisa".into()]));
        let mut en = Annotations::new();
        en.insert("👋".into(), ("waving hand".into(), vec!["hand".into(), "wave".into()]));
        en.insert("☺".into(), ("smiling face".into(), vec![]));
        assert_eq!(describe(&es, &en, "😀").as_deref(), Some("cara sonriente | cara | sonrisa"));
        assert_eq!(describe(&es, &en, "👋").as_deref(), Some("waving hand | hand | wave"));
        assert_eq!(describe(&es, &en, "☺\u{fe0f}").as_deref(), Some("smiling face"));
        assert_eq!(describe(&es, &en, "🛸"), None);
    }
}
