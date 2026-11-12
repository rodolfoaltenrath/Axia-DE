//! Help window, article navigation and welcome tour, mounted directly by Rust.
pub mod content;
use crate::native_view::icons;
mod markdown;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{i18n::I18n, NativeApp};
use content::{Article, Content, Metadata};
use serde_json::{json, Value};
use std::cell::RefCell;
thread_local! { static LAST_ARTICLE: RefCell<String> = RefCell::new("welcome".into()); }

pub struct App {
    content: Content,
    i18n: I18n,
    article: String,
    tour: bool,
    step: usize,
    query: String,
    hover: String,
    theme: Value,
    registry: Vec<Value>,
    lang: String,
    width: f64,
    args: Value,
    list: Vec<Metadata>,
    results: Option<Vec<Value>>,
    body: Option<Article>,
    document: Vec<markdown::Block>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            content: Content::default(),
            i18n: I18n::default(),
            article: LAST_ARTICLE.with(|s| s.borrow().clone()),
            tour: false,
            step: 0,
            query: String::new(),
            hover: String::new(),
            theme: Value::Null,
            registry: vec![],
            lang: "en".into(),
            width: 860.,
            args: Value::Null,
            list: vec![],
            results: None,
            body: None,
            document: vec![],
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.content = Content::new(assets);
        self.i18n = I18n::new(assets, home);
    }
    fn request(&mut self, input: &Value) -> Value {
        App::request(self, input)
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
}
impl App {
    fn language(&self) -> &str {
        self.lang.split('-').next().unwrap_or("en")
    }
    fn refresh_content(&mut self) {
        let lang = self.language().to_owned();
        self.list = self.content.list(&lang);
        self.results = (self.query.trim().encode_utf16().count() >= 2)
            .then(|| self.content.search(&lang, &self.query));
        self.body = self.content.read(&lang, &self.article);
        self.document = self
            .body
            .as_ref()
            .map(|a| markdown::parse(&a.body))
            .unwrap_or_default();
    }
    fn set_article(&mut self, id: &str) {
        self.article = id.chars().take(48).collect();
        LAST_ARTICLE.with(|s| *s.borrow_mut() = self.article.clone());
        self.tour = false;
        self.refresh_content();
    }
    fn open_tour(&mut self) {
        if !self.tour {
            self.step = 0;
        }
        self.tour = true;
    }
    fn app(&self, id: &str) -> Option<&Value> {
        self.registry.iter().find(|app| app["id"] == id)
    }
    fn rtl(&mut self) -> bool {
        let lang = self.language().to_owned();
        self.i18n
            .entry(&self.lang, "meta", "direction")
            .or_else(|| self.i18n.entry(&lang, "meta", "direction"))
            == Some(json!("rtl"))
    }
    fn finish(&mut self) -> Value {
        self.set_article("welcome");
        json!({"effect":{"kind":"settings","patch":{"welcomeShown":true}}})
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let key = input["key"].as_str().unwrap_or("");
        match input["cmd"].as_str().unwrap_or("") {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"]
                    .as_f64()
                    .filter(|w| w.is_finite())
                    .unwrap_or(860.);
                self.registry = input["registry"].as_array().cloned().unwrap_or_default();
                let lang = input["lang"].as_str().unwrap_or("en");
                if self.lang != lang || self.list.is_empty() {
                    self.lang = lang.into();
                    self.refresh_content();
                }
                if self.args != input["args"] {
                    self.args = input["args"].clone();
                    if self.args["tour"] == true {
                        self.open_tour();
                    } else if let Some(article) = self.args["article"].as_str().map(str::to_owned) {
                        self.set_article(&article);
                    }
                }
                return json!({"tree":ui::tree(self)});
            }
            "article" => self.set_article(key),
            "tour" => self.open_tour(),
            "step" => {
                if let Ok(step) = key.parse::<usize>() {
                    if step < 6 {
                        self.step = step;
                    }
                }
            }
            "back" => self.step = self.step.saturating_sub(1),
            "next" => {
                if self.step < 5 {
                    self.step += 1;
                } else {
                    return self.finish();
                }
            }
            "finish" => return self.finish(),
            "query" => {
                self.query = input["value"]
                    .as_str()
                    .unwrap_or(key)
                    .chars()
                    .take(4096)
                    .collect();
                let lang = self.language().to_owned();
                self.results = (self.query.trim().encode_utf16().count() >= 2)
                    .then(|| self.content.search(&lang, &self.query));
            }
            "submit" => {
                if let Some(id) = self
                    .results
                    .as_ref()
                    .and_then(|r| r.first())
                    .and_then(|r| r["id"].as_str())
                    .map(str::to_owned)
                {
                    self.set_article(&id);
                }
            }
            "hover" => self.hover = key.into(),
            "open" if self.app(key).is_some() => {
                return json!({"effect":{"kind":"openApp","app":key}})
            }
            "link" => {
                let target = content::link(key);
                if target["app"] == "help" {
                    if let Some(id) = target["args"]["article"].as_str() {
                        self.set_article(id);
                    }
                } else if target["app"] == "settings"
                    || target["app"]
                        .as_str()
                        .is_some_and(|id| self.app(id).is_some())
                {
                    return json!({"effect":{"kind":"openApp","app":target["app"],"args":target["args"]}});
                }
            }
            _ => {}
        }
        Value::Null
    }
}
