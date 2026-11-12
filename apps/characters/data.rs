//! Unicode 15.1 data shared with the shell picker; no parallel generated catalog.
use serde::Deserialize;
use std::{collections::HashSet, sync::OnceLock};
#[derive(Deserialize)]
struct Source {
    groups: Vec<(String, String)>,
    toned: String,
}
pub struct Data {
    pub groups: Vec<(String, Vec<String>)>,
    toned: HashSet<String>,
}
pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        let source: Source = serde_json::from_str(include_str!("../../assets/emoji.json"))
            .expect("bundled Unicode emoji data");
        Data {
            groups: source
                .groups
                .into_iter()
                .map(|(id, list)| (id, list.split_whitespace().map(str::to_owned).collect()))
                .collect(),
            toned: source.toned.split_whitespace().map(str::to_owned).collect(),
        }
    })
}
pub fn bare(ch: &str) -> String {
    ch.chars()
        .filter(|c| !('\u{1f3fb}'..='\u{1f3ff}').contains(c))
        .collect()
}
pub fn key(ch: &str) -> String {
    bare(ch)
        .chars()
        .filter(|c| *c != '\u{fe0f}')
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("_")
}
pub fn with_tone(ch: &str, tone: u8) -> String {
    if !(1..=5).contains(&tone) || !data().toned.contains(ch) {
        return ch.to_owned();
    }
    let mut chars = ch.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest = chars
        .as_str()
        .strip_prefix('\u{fe0f}')
        .unwrap_or(chars.as_str());
    format!(
        "{}{}{}",
        first,
        char::from_u32(0x1f3fa + tone as u32).unwrap(),
        rest
    )
}
pub const BLOCKS: [(&str, u32, u32); 20] = [
    ("arrows", 0x2190, 0x21ff),
    ("math", 0x2200, 0x22ff),
    ("technical", 0x2300, 0x23ff),
    ("currency", 0x20a0, 0x20c0),
    ("letterlike", 0x2100, 0x214f),
    ("numberForms", 0x2150, 0x218b),
    ("punctuation", 0x2010, 0x205e),
    ("superSub", 0x2070, 0x209c),
    ("enclosed", 0x2460, 0x24ff),
    ("boxDrawing", 0x2500, 0x257f),
    ("blockElements", 0x2580, 0x259f),
    ("shapes", 0x25a0, 0x25ff),
    ("miscSymbols", 0x2600, 0x26ff),
    ("dingbats", 0x2700, 0x27bf),
    ("suppArrows", 0x27f0, 0x27ff),
    ("braille", 0x2800, 0x28ff),
    ("greek", 0x391, 0x3c9),
    ("latin1", 0xa1, 0xff),
    ("cjkSymbols", 0x3000, 0x303f),
    ("halfwidth", 0xff01, 0xff5e),
];
