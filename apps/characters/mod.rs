//! Emoji & Symbols. Rust owns state, Unicode lookup, search and the scene tree.
mod data;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{i18n::I18n, NativeApp};
use data::{bare, data, key, with_tone, BLOCKS};
use serde_json::{json, Value};
pub struct App {
    pub tab: String,
    pub query: String,
    pub group: String,
    pub block: usize,
    pub tone: u8,
    pub pick: String,
    pub recent: Vec<String>,
    pub hover: String,
    pub theme: Value,
    lang: String,
    initialized: bool,
    recent_facts: Value,
    i18n: I18n,
    index: Option<Vec<(String, String)>>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            tab: "emoji".into(),
            query: String::new(),
            group: "smileys".into(),
            block: 0,
            tone: 0,
            pick: String::new(),
            recent: Vec::new(),
            hover: String::new(),
            theme: Value::Null,
            lang: "en".into(),
            initialized: false,
            recent_facts: Value::Null,
            i18n: I18n::default(),
            index: None,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.index = None;
    }
    fn request(&mut self, input: &Value) -> Value {
        App::request(self, input)
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
}
impl App {
    pub fn rtl(&mut self) -> bool {
        let lang = self.lang.clone();
        self.i18n
            .entry(&lang, "meta", "direction")
            .and_then(|v| v.as_str().map(str::to_owned))
            .is_some_and(|dir| dir == "rtl")
    }
    fn keywords(&mut self, lang: &str, ch: &str) -> String {
        self.i18n
            .entry(lang, "emojikw", &key(ch))
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
    pub fn name(&mut self, ch: &str) -> String {
        let lang = self
            .lang
            .split(['-', '_'])
            .next()
            .unwrap_or("en")
            .to_owned();
        let own = self.keywords(&lang, ch);
        let own = if own.is_empty() {
            self.keywords("en", ch)
        } else {
            own
        };
        own.split(" | ").next().unwrap_or("").to_owned()
    }
    fn search(&mut self) -> Vec<String> {
        if self.index.is_none() {
            self.index = Some(
                data()
                    .groups
                    .iter()
                    .flat_map(|(_, list)| list)
                    .map(|ch| {
                        let text = ["es", "en", "pt"]
                            .map(|lang| self.keywords(lang, ch))
                            .join(" | ");
                        (ch.clone(), crate::search::matching::fold(&text))
                    })
                    .collect(),
            );
        }
        let folded = crate::search::matching::fold(&self.query);
        let words: Vec<_> = folded.split_whitespace().collect();
        let mut found: Vec<_> = self
            .index
            .as_ref()
            .unwrap()
            .iter()
            .filter(|(_, text)| words.iter().all(|w| text.contains(w)))
            .map(|(ch, text)| {
                let starts = words.iter().all(|w| {
                    text.starts_with(w)
                        || text.contains(&format!(" {w}"))
                        || text.contains(&format!("|{w}"))
                });
                (!starts, ch.clone())
            })
            .collect();
        found.sort_by_key(|(score, _)| *score);
        found.into_iter().take(400).map(|(_, ch)| ch).collect()
    }
    pub fn list(&mut self) -> Vec<String> {
        if self.tab == "symbols" {
            let (_, a, b) = BLOCKS[self.block];
            return (a..=b)
                .filter_map(char::from_u32)
                .map(|c| c.to_string())
                .collect();
        }
        let list = if !self.query.trim().is_empty() {
            self.search()
        } else if self.group == "recent" {
            self.recent.clone()
        } else {
            data()
                .groups
                .iter()
                .find(|(id, _)| id == &self.group)
                .map(|(_, list)| list.clone())
                .unwrap_or_default()
        };
        list.into_iter()
            .map(|ch| with_tone(&ch, self.tone))
            .collect()
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let k = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.theme = input["context"]["theme"].clone();
                self.lang = input["lang"].as_str().unwrap_or("en").into();
                let recent = &input["settings"]["emojiRecent"];
                if recent.is_array() && *recent != self.recent_facts {
                    self.recent = recent
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .take(32)
                        .map(str::to_owned)
                        .collect();
                    self.recent_facts = recent.clone();
                }
                if !self.initialized {
                    self.initialized = true;
                    self.tone = input["settings"]["emojiTone"].as_u64().unwrap_or(0).min(5) as u8;
                    if !self.recent.is_empty() {
                        self.group = "recent".into();
                    }
                }
                return json!({"tree":ui::tree(self)});
            }
            "tab" if matches!(k, "emoji" | "symbols") => {
                self.tab = k.into();
                self.pick.clear();
            }
            "group" if k == "recent" || data().groups.iter().any(|(id, _)| id == k) => {
                self.group = k.into();
                self.query.clear();
            }
            "block" => {
                if let Ok(n) = k.parse::<usize>() {
                    if n < BLOCKS.len() {
                        self.block = n;
                    }
                }
            }
            "query" => {
                self.query = input["value"]
                    .as_str()
                    .unwrap_or(k)
                    .chars()
                    .take(512)
                    .collect();
            }
            "tone" => {
                if let Ok(n) = k.parse::<u8>() {
                    if n <= 5 {
                        self.tone = n;
                        return json!({"effect":{"kind":"settings","patch":{"emojiTone":n}}});
                    }
                }
            }
            "pick" => {
                // Only a cell in the current grid can be picked; reject arbitrary/oversized events.
                if k.len() <= 128 && self.list().iter().any(|ch| ch == k) {
                    self.pick = k.into();
                    if self.tab == "emoji" {
                        let ch = bare(k);
                        self.recent.retain(|x| x != &ch);
                        self.recent.insert(0, ch);
                        self.recent.truncate(32);
                        return json!({"effect":{"kind":"settings","patch":{"emojiRecent":self.recent}}});
                    }
                }
            }
            "copy" if !self.pick.is_empty() => {
                return json!({"effect":{"host":true,"kind":"copy","text":self.pick}})
            }
            "hover" => self.hover = k.into(),
            _ => {}
        }
        Value::Null
    }
}
