//! Resident global search core. The host transports facts and draws Rust widgets.
mod matching;
mod math;
mod ui;
mod worker;
pub use matching::fold;
use matching::{best, score, trimmed};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use worker::{Config, Job, Usage, Work, Worker};
pub const SETTINGS: [&str; 12] = [
    "general",
    "appearance",
    "wallpaper",
    "network",
    "sound",
    "clipboard",
    "language",
    "keyboard",
    "users",
    "updates",
    "themes",
    "session",
];
const MAX_CLIENTS: usize = 8;
pub fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.
}
pub fn boost(usage: &Usage, key: &str) -> f64 {
    let usage = usage.lock().unwrap();
    usage
        .get(key)
        .map(|(n, at)| (*n as f64 * 4.).min(30.) * (1. - (now() - at) / 86400000. / 30.).max(0.2))
        .unwrap_or(0.)
}
pub fn item(
    key: &str,
    score: f64,
    title: Value,
    subtitle: Value,
    glyph: &str,
    action: Value,
) -> Value {
    json!({"key":key,"score":score,"title":title,"subtitle":subtitle,"glyph":glyph,"action":action})
}
pub fn section(id: &str, items: Vec<Value>, top: Option<f64>) -> Value {
    let top = top.unwrap_or_else(|| {
        items
            .iter()
            .map(|v| v["score"].as_f64().unwrap_or(0.))
            .fold(0., f64::max)
    });
    json!({"id":id,"items":items,"top":top})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
struct App {
    id: String,
    name: String,
    comment: String,
    kind: String,
    fields: Vec<String>,
}
#[derive(Default)]
struct Facts {
    settings: Vec<Value>,
    actions: Vec<Value>,
    clips: Vec<Value>,
    aliases: Value,
    lang: String,
    notes: bool,
    help: bool,
}
pub struct Client {
    id: u64,
    panel: bool,
    query: String,
    folded: String,
    version: u64,
    selected: usize,
    touched: bool,
    sections: Vec<Value>,
    extra: Vec<Value>,
    external: Vec<Value>,
    preview: Value,
    preview_key: String,
    generation: u64,
    preview_generation: u64,
    token: Arc<AtomicU64>,
    preview_token: Arc<AtomicU64>,
    pending: bool,
    preview_pending: bool,
    context: Value,
}
impl Client {
    fn flat(&self) -> Vec<&Value> {
        self.sections
            .iter()
            .filter_map(|s| s["items"].as_array())
            .flatten()
            .collect()
    }
    fn current(&self) -> Option<&Value> {
        self.flat().get(self.selected).copied()
    }
    fn cancel(&self) {
        self.token.fetch_add(1, Ordering::Relaxed);
        self.preview_token.fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub struct Search {
    config: Option<Config>,
    worker: Option<Worker>,
    apps: Vec<App>,
    facts: Facts,
    usage: Usage,
    clients: HashMap<u64, Client>,
    sequence: u64,
    version: u64,
}
impl Drop for Search {
    fn drop(&mut self) {
        for c in self.clients.values() {
            c.cancel();
        }
    }
}
impl Search {
    pub fn configure(&mut self, home: PathBuf, assets: PathBuf) {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".cache"))
            .join("axia/search");
        self.config = Some(Config {
            home,
            assets,
            cache,
        });
    }
    pub fn apps(&mut self, apps: impl Iterator<Item = Value>) {
        self.apps = apps
            .map(|a| {
                let name = s(&a, "name").to_owned();
                App {
                    id: s(&a, "id").to_owned(),
                    name: name.clone(),
                    comment: if s(&a, "comment").is_empty() {
                        s(&a, "genericName")
                    } else {
                        s(&a, "comment")
                    }
                    .to_owned(),
                    kind: s(&a, "kind").to_owned(),
                    fields: [
                        name.as_str(),
                        s(&a, "genericName"),
                        s(&a, "keywords"),
                        s(&a, "id"),
                        s(&a, "comment"),
                    ]
                    .into_iter()
                    .map(fold)
                    .collect(),
                }
            })
            .collect();
        self.apply_aliases();
        self.version = self.version.wrapping_add(1);
    }
    fn apply_aliases(&mut self) {
        for app in &mut self.apps {
            app.fields.truncate(5);
            if let Some(alias) = self
                .facts
                .aliases
                .get(format!("spotlight.keywords.{}", app.id))
                .and_then(Value::as_str)
            {
                app.fields.push(alias.to_owned());
            }
        }
    }
    fn start_worker(&mut self) {
        if self.worker.is_none() {
            if let Some(c) = &self.config {
                self.worker = Worker::new(c.clone(), self.usage.clone());
            }
        }
    }
    fn quick(&self, c: &Client) -> Vec<Value> {
        let q = &c.folded;
        if q.is_empty() {
            return vec![];
        }
        let mut sections = vec![];
        if let Some(conv) = math::convert(&c.query) {
            let display = json!({"number":conv["value"],"digits":if conv["value"].as_f64().unwrap_or(0.).abs()<1.{6}else{3}});
            let res = json!({"parts":[display," ",conv["to"]]});
            let mut it = item(
                "calc",
                1000.,
                res.clone(),
                json!({"parts":[{"number":conv["input"],"digits":6}," ",conv["from"]," → ",conv["to"]]}),
                "⇄",
                json!({"type":"copyNumber","value":conv["value"],"decimals":6}),
            );
            it["big"] = res;
            it["hint"] = key("shell.spotlight.copy");
            sections.push(section("calc", vec![it], Some(1000.)));
        } else if let Some(value) = math::calc(&c.query) {
            let res = json!({"number":value,"digits":10});
            let mut it = item(
                "calc",
                1000.,
                json!({"parts":["= ",res]}),
                json!(trimmed(&c.query)),
                "=",
                json!({"type":"copyNumber","value":value,"significant":12}),
            );
            it["big"] = res;
            it["hint"] = key("shell.spotlight.copy");
            sections.push(section("calc", vec![it], Some(1000.)));
        }
        let mut apps: Vec<(usize, f64)> = Vec::with_capacity(6);
        for (index, a) in self.apps.iter().enumerate() {
            let rank = best(q, &a.fields);
            if rank > 0. {
                let rank = rank + boost(&self.usage, &format!("app:{}", a.id));
                let at = apps
                    .iter()
                    .position(|(_, old)| *old < rank)
                    .unwrap_or(apps.len());
                if at < 6 {
                    apps.insert(at, (index, rank));
                    apps.truncate(6);
                }
            }
        }
        let items=apps.into_iter().take(6).map(|(i,rank)|{let a=&self.apps[i];let mut it=item(&format!("app:{}",a.id),rank,json!(a.name),if a.comment.is_empty(){key(&format!("store.kind.{}",a.kind))}else{json!(a.comment)},"",json!({"type":"app","id":a.id}));it["app"]=json!(a.id);it["preview"]=json!({"kind":"app","id":a.id,"name":a.name,"comment":a.comment,"appKind":a.kind});it}).collect::<Vec<_>>();
        if !items.is_empty() {
            sections.push(section("apps", items, None));
        }
        let mut settings = self
            .facts
            .settings
            .iter()
            .filter_map(|v| {
                let rank = score(s(v, "foldedTitle"), q, true)
                    .max(score(s(v, "foldedKeywords"), q, false) * 0.7);
                (rank > 0.).then(|| {
                    let id = s(v, "id");
                    item(
                        &format!("settings:{id}"),
                        rank,
                        v["title"].clone(),
                        key("shell.spotlight.inSettings"),
                        "⚙",
                        json!({"type":"settings","id":id}),
                    )
                })
            })
            .collect::<Vec<_>>();
        settings.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["score"].as_f64().unwrap())
        });
        settings.truncate(4);
        for it in &mut settings {
            it["score"] =
                json!(it["score"].as_f64().unwrap_or(0.) + boost(&self.usage, s(it, "key")));
        }
        if !settings.is_empty() {
            sections.push(section("settings", settings, None));
        }
        let actions = self
            .facts
            .actions
            .iter()
            .filter_map(|v| {
                let rank = score(s(v, "foldedTitle"), q, true);
                (rank >= 40.).then(|| {
                    item(
                        &format!("action:{}", s(v, "id")),
                        rank + 5.,
                        v["title"].clone(),
                        key("shell.spotlight.action.hint"),
                        "⏎",
                        json!({"type":"action","id":v["id"]}),
                    )
                })
            })
            .collect::<Vec<_>>();
        if !actions.is_empty() {
            sections.push(section("actions", actions, None));
        }
        let clips = self
            .facts
            .clips
            .iter()
            .filter(|v| s(v, "foldedText").contains(q))
            .take(4)
            .map(|v| {
                let id = if v["id"].is_null() {
                    v["time"].to_string()
                } else {
                    v["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v["id"].to_string())
                };
                let text = s(v, "text");
                let mut it = item(
                    &format!("clip:{id}"),
                    20.,
                    json!(text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(80)
                        .collect::<String>()),
                    key("shell.spotlight.fromClipboard"),
                    "📋",
                    json!({"type":"clipboard","id":v["id"],"time":v["time"]}),
                );
                it["preview"] =
                    json!({"kind":"text","text":text.chars().take(1500).collect::<String>()});
                it["hint"] = key("shell.spotlight.copy");
                it
            })
            .collect::<Vec<_>>();
        if !clips.is_empty() {
            sections.push(section("clipboard", clips, None));
        }
        sections
    }
    fn merge(&self, c: &mut Client) {
        let old = c.current().map(|v| s(v, "key").to_owned());
        c.sections = self.quick(c);
        c.sections.extend(c.extra.iter().cloned());
        c.sections.extend(c.external.iter().cloned());
        c.sections.sort_by(|a, b| {
            b["top"]
                .as_f64()
                .unwrap_or(0.)
                .total_cmp(&a["top"].as_f64().unwrap_or(0.))
        });
        if c.touched {
            if let Some(key) = old {
                if let Some(index) = c.flat().iter().position(|v| s(v, "key") == key) {
                    c.selected = index;
                }
            }
        }
        c.selected = c.selected.min(c.flat().len().saturating_sub(1));
    }
    fn query(&mut self, c: &mut Client, query: &str) {
        let query = query.chars().take(4096).collect::<String>();
        if c.query == query && c.version == self.version {
            return;
        }
        c.cancel();
        c.query = query;
        c.folded = fold(trimmed(&c.query));
        c.version = self.version;
        c.selected = 0;
        c.touched = false;
        c.extra.clear();
        c.external.clear();
        c.preview = Value::Null;
        c.preview_key.clear();
        c.preview_pending = false;
        if let Some(w) = &self.worker {
            w.submit(Job {
                id: c.id,
                generation: 0,
                token: c.preview_token.clone(),
                work: Work::DiscardPreview,
            });
        }
        c.generation = c.token.load(Ordering::Relaxed);
        self.merge(c);
        c.pending = false;
        if !c.folded.is_empty() {
            self.start_worker();
            if let Some(w) = &self.worker {
                c.pending = true;
                w.submit(Job {
                    id: c.id,
                    generation: c.generation,
                    token: c.token.clone(),
                    work: Work::Query {
                        q: c.folded.clone(),
                        lang: self.facts.lang.clone(),
                        notes: self.facts.notes,
                        help: self.facts.help,
                    },
                });
            }
        }
    }
    fn preview(&mut self, c: &mut Client) {
        let current = c.current().cloned();
        let key = current.as_ref().map(|v| s(v, "key")).unwrap_or("");
        if key == c.preview_key {
            return;
        }
        c.preview_token.fetch_add(1, Ordering::Relaxed);
        c.preview_generation = c.preview_token.load(Ordering::Relaxed);
        c.preview_key = key.to_owned();
        c.preview = Value::Null;
        c.preview_pending = false;
        if let Some(it) = current {
            if s(&it["preview"], "kind") == "file" && c.panel {
                if let Some(w) = &self.worker {
                    c.preview_pending = true;
                    w.submit(Job {
                        id: c.id,
                        generation: c.preview_generation,
                        token: c.preview_token.clone(),
                        work: Work::Preview(it["preview"]["file"].clone()),
                    });
                }
            } else if let Some(w) = &self.worker {
                w.submit(Job {
                    id: c.id,
                    generation: 0,
                    token: c.preview_token.clone(),
                    work: Work::DiscardPreview,
                });
            }
        }
    }
    fn reply(&mut self, c: &mut Client, effects: Vec<Value>) -> Value {
        self.preview(c);
        let tree = if c.panel { ui::render(c) } else { Value::Null };
        json!({"id":c.id,"query":c.query,"revision":c.version,"sections":c.sections,"selected":c.selected,"tree":tree,"effects":effects,"pending":c.pending||c.preview_pending})
    }
    fn close(&mut self, id: u64) {
        if let Some(c) = self.clients.remove(&id) {
            c.cancel();
            if let Some(w) = &self.worker {
                w.forget(id);
                w.submit(Job {
                    id,
                    generation: 0,
                    token: c.token,
                    work: Work::Release,
                });
            }
        }
        if self.clients.is_empty() {
            self.facts.clips = Vec::new();
        }
    }
    pub fn request(&mut self, v: &Value) -> Result<Value, &'static str> {
        match s(v, "cmd") {
            "calc" => return Ok(json!({"value":math::calc(s(v,"text"))})),
            "convert" => return Ok(json!({"value":math::convert(s(v,"text"))})),
            "fold" => return Ok(json!({"value":fold(s(v,"text"))})),
            "score" => {
                return Ok(
                    json!({"value":score(&fold(s(v,"text")),s(v,"query"),v["fuzzy"]!=false)}),
                )
            }
            "settings" => return Ok(json!({"ids":SETTINGS})),
            "best" => {
                return Ok(
                    json!({"value":best(s(v,"query"), &v["texts"].as_array().into_iter().flatten().take(16).map(|v|fold(v.as_str().unwrap_or(""))).collect::<Vec<_>>())}),
                )
            }
            _ => {}
        }
        if self.config.is_none() {
            return Err("search filesystem capability unavailable");
        }
        match s(v, "cmd") {
            "facts" => {
                self.facts.lang = s(v, "lang").to_owned();
                self.facts.notes = v["notes"] == true;
                self.facts.help = v["help"] == true;
                let mut aliases: Value =
                    serde_json::from_str(s(v, "fallbackShell")).unwrap_or_else(|_| json!({}));
                if !aliases.is_object() {
                    aliases = json!({});
                }
                if let Ok(Value::Object(own)) = serde_json::from_str::<Value>(s(v, "shell")) {
                    aliases.as_object_mut().unwrap().extend(own);
                }
                self.facts.aliases = json!(aliases
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| k.starts_with("spotlight.keywords."))
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), json!(fold(v)))))
                    .collect::<serde_json::Map<_, _>>());
                self.apply_aliases();
                self.facts.settings = v["settings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|a| SETTINGS.contains(&s(a, "id")))
                    .take(13)
                    .map(|a| {
                        let mut a = a.clone();
                        a["foldedTitle"] = json!(fold(s(&a, "title")));
                        a["foldedKeywords"] = json!(fold(s(&a, "keywords")));
                        a
                    })
                    .collect();
                self.facts.actions = v["actions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(2)
                    .map(|a| {
                        let mut a = a.clone();
                        a["foldedTitle"] = json!(fold(s(&a, "title")));
                        a
                    })
                    .collect();
                let mut bytes = 0;
                self.facts.clips=v["clipboard"].as_array().into_iter().flatten().take(500).filter_map(|a|{let text=s(a,"text");if text.is_empty()||bytes+text.len()>2*1024*1024{return None;}bytes+=text.len();Some(json!({"id":a["id"],"time":a["time"],"text":text,"foldedText":fold(text)}))}).collect();
                self.version = self.version.wrapping_add(1);
                Ok(json!({}))
            }
            "open" => {
                if self.clients.len() >= MAX_CLIENTS {
                    return Err("search client limit");
                }
                self.sequence = self.sequence.wrapping_add(1).max(1);
                let id = self.sequence;
                let mut c = Client {
                    id,
                    panel: v["panel"] == true,
                    query: String::new(),
                    folded: String::new(),
                    version: self.version,
                    selected: 0,
                    touched: false,
                    sections: vec![],
                    extra: vec![],
                    external: vec![],
                    preview: Value::Null,
                    preview_key: String::new(),
                    generation: 1,
                    preview_generation: 1,
                    token: Arc::new(AtomicU64::new(1)),
                    preview_token: Arc::new(AtomicU64::new(1)),
                    pending: false,
                    preview_pending: false,
                    context: v["context"].clone(),
                };
                let reply = self.reply(&mut c, vec![]);
                self.clients.insert(id, c);
                Ok(reply)
            }
            "close" => {
                self.close(v["id"].as_u64().unwrap_or(0));
                Ok(Value::Null)
            }
            "poll" => {
                let mut changed = Vec::new();
                if let Some(w) = &self.worker {
                    for reply in w.drain() {
                        if let Some(mut c) = self.clients.remove(&reply.id) {
                            if reply.preview && reply.generation == c.preview_generation {
                                c.preview = reply.value;
                                c.preview_pending = false;
                                changed.push(c.id);
                            } else if !reply.preview && reply.generation == c.generation {
                                c.extra = reply.value.as_array().cloned().unwrap_or_default();
                                c.pending = false;
                                self.merge(&mut c);
                                changed.push(c.id);
                            }
                            self.clients.insert(c.id, c);
                        }
                    }
                }
                changed.sort();
                changed.dedup();
                let mut updates = vec![];
                for id in changed {
                    if let Some(mut c) = self.clients.remove(&id) {
                        updates.push(self.reply(&mut c, vec![]));
                        self.clients.insert(id, c);
                    }
                }
                Ok(
                    json!({"updates":updates,"pending":self.clients.values().any(|c|c.pending||c.preview_pending)}),
                )
            }
            "stats" => Ok(
                json!({"clients":self.clients.len(),"worker":self.worker.is_some(),"queued":self.worker.as_ref().map(Worker::pending).unwrap_or(0),"activeBytes":self.clients.values().map(|c|c.sections.iter().map(|s|s.to_string().len()).sum::<usize>()+c.query.len()+c.preview.to_string().len()).sum::<usize>(),"clipboardBytes":self.facts.clips.iter().map(|v|s(v,"text").len()+s(v,"foldedText").len()).sum::<usize>(),"indexDiskLimit":worker::INDEX_BYTES}),
            ),
            "query" | "context" | "event" | "external" => {
                let id = v["id"].as_u64().ok_or("invalid search client")?;
                let mut c = self.clients.remove(&id).ok_or("unknown search client")?;
                let mut effects = vec![];
                match s(v, "cmd") {
                    "query" => self.query(&mut c, s(v, "query")),
                    "context" => c.context = v["context"].clone(),
                    "external" => {
                        if s(v, "query") == c.query {
                            c.external = v["sections"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .take(16)
                                .filter_map(|sec| {
                                    let items = sec["items"]
                                        .as_array()?
                                        .iter()
                                        .take(6)
                                        .filter(|it| {
                                            s(it, "key").len() <= 4096
                                                && it.to_string().len() <= 16384
                                        })
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    (!items.is_empty()).then(|| section(s(sec, "id"), items, None))
                                })
                                .collect();
                            self.merge(&mut c);
                        }
                    }
                    _ => {
                        let a = &v["action"];
                        let name = a[0].as_str().unwrap_or("");
                        let target = a[1].as_str().unwrap_or("");
                        match name {
                            "query" => self.query(&mut c, target),
                            "close" => effects.push(json!({"type":"close"})),
                            "select" => {
                                c.selected = a[1].as_u64().unwrap_or(0) as usize;
                                c.selected = c.selected.min(c.flat().len().saturating_sub(1));
                                c.touched = true;
                            }
                            "key" => {
                                match target {
                                    "ArrowDown" => {
                                        c.selected =
                                            (c.selected + 1).min(c.flat().len().saturating_sub(1));
                                        c.touched = true;
                                    }
                                    "ArrowUp" => {
                                        c.selected = c.selected.saturating_sub(1);
                                        c.touched = true;
                                    }
                                    "Tab" => {
                                        let current = c
                                            .current()
                                            .map(|v| s(v, "key").to_owned())
                                            .unwrap_or_default();
                                        let section = c
                                            .sections
                                            .iter()
                                            .position(|sec| {
                                                sec["items"].as_array().is_some_and(|items| {
                                                    items.iter().any(|it| s(it, "key") == current)
                                                })
                                            })
                                            .unwrap_or(0);
                                        if let Some(next) = c
                                            .sections
                                            .get((section + 1) % c.sections.len().max(1))
                                            .and_then(|s| s["items"].as_array())
                                            .and_then(|i| i.first())
                                        {
                                            c.selected = c
                                                .flat()
                                                .iter()
                                                .position(|v| v["key"] == next["key"])
                                                .unwrap_or(0);
                                        }
                                        c.touched = true;
                                    }
                                    _ => {}
                                }
                                if target == "Enter" {
                                    self.run(&c, c.current(), v["reveal"] == true, &mut effects);
                                }
                            }
                            "run" => {
                                let selected =
                                    c.flat().into_iter().find(|it| s(it, "key") == target);
                                self.run(&c, selected, v["reveal"] == true, &mut effects);
                            }
                            _ => {}
                        }
                    }
                }
                let mut reply = self.reply(&mut c, effects);
                reply["handled"] = json!(
                    s(v, "cmd") == "event"
                        && v["action"][0] == "key"
                        && ["ArrowDown", "ArrowUp", "Enter", "Tab"]
                            .contains(&v["action"][1].as_str().unwrap_or(""))
                );
                self.clients.insert(id, c);
                Ok(reply)
            }
            _ => Err("unknown search operation"),
        }
    }
    fn run(&mut self, c: &Client, item: Option<&Value>, reveal: bool, effects: &mut Vec<Value>) {
        if let Some(it) = item {
            let key = s(it, "key").to_owned();
            {
                let mut u = self.usage.lock().unwrap();
                let count = u.get(&key).map(|v| v.0).unwrap_or(0).saturating_add(1);
                u.insert(key, (count, now()));
                worker::trim_usage(&mut u);
            }
            self.start_worker();
            if let Some(w) = &self.worker {
                w.submit(Job {
                    id: 0,
                    generation: 0,
                    token: Arc::new(AtomicU64::new(0)),
                    work: Work::Save,
                });
            }
            if c.panel {
                effects.push(json!({"type":"close"}));
            }
            effects.push(json!({"type":"execute","action":it["action"],"reveal":reveal}));
        }
    }
}
#[cfg(test)]
mod tests;
