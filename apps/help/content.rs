//! Shared Help content engine. The desktop bridge and the native app use the
//! same catalog, English fallback, accent-insensitive ranking and snippets.
#[cfg(test)]
#[path = "content_tests.rs"]
mod content_tests;

use crate::search::matching::fold;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    ffi::{c_char, CStr, CString},
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const MAX_ARTICLE: usize = 256 * 1024;
const MAX_ARTICLES: usize = 500;
const MAX_BODIES: usize = 64;
const MAX_LANGUAGES: usize = 8;

#[link(name = "glib-2.0")]
extern "C" {
    fn g_utf8_normalize(s: *const c_char, len: isize, mode: i32) -> *mut c_char;
    fn g_free(p: *mut libc::c_void);
}

// QuickJS localeCompare compares NFC code points (it has no Intl collator).
fn title_key(title: &str) -> String {
    unsafe {
        let p = g_utf8_normalize(title.as_ptr().cast(), title.len() as isize, 1);
        if p.is_null() {
            return title.into();
        }
        let key = CStr::from_ptr(p).to_string_lossy().into_owned();
        g_free(p.cast());
        key
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub id: String,
    pub title: String,
    pub app: String,
    pub section: String,
    pub order: i32,
    pub keywords: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Article {
    #[serde(flatten)]
    pub meta: Metadata,
    pub body: String,
}

fn valid(part: &str, max: usize) -> bool {
    !part.is_empty()
        && part.len() <= max
        && !part.starts_with('-')
        && part
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Front matter is deliberately a small key/value format, as in the SDK.
/// An unfinished header remains visible article text.
pub fn front(raw: &str) -> (BTreeMap<String, String>, &str) {
    let Some(head) = raw.strip_prefix("---\n") else {
        return (BTreeMap::new(), raw);
    };
    let Some(end) = head.match_indices("\n---").find_map(|(at, _)| {
        let tail = &head[at + 4..];
        (tail.is_empty() || tail.starts_with('\n')).then_some(at)
    }) else {
        return (BTreeMap::new(), raw);
    };
    let mut meta = BTreeMap::new();
    for line in head[..end].lines() {
        if let Some((key, value)) = line.split_once(':') {
            if !key.trim().is_empty() {
                meta.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
    }
    (
        meta,
        head[end + 4..]
            .strip_prefix('\n')
            .unwrap_or(&head[end + 4..]),
    )
}

fn parse(id: &str, raw: &str) -> Article {
    let (mut fields, body) = front(raw);
    let title = fields
        .remove("title")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.into());
    Article {
        meta: Metadata {
            id: id.into(),
            title,
            app: fields.remove("app").unwrap_or_default(),
            section: fields.remove("section").unwrap_or_default(),
            order: fields
                .remove("order")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            keywords: fields.remove("keywords").unwrap_or_default(),
        },
        body: body.into(),
    }
}

#[derive(Default)]
pub struct Content {
    assets: PathBuf,
    lists: BTreeMap<String, Vec<Metadata>>,
    // FIFO bounds the cache at 16 MiB even for unusually large help catalogs.
    bodies: VecDeque<(String, String, Article)>,
}

impl Content {
    pub fn new(assets: impl Into<PathBuf>) -> Self {
        Self {
            assets: assets.into(),
            ..Self::default()
        }
    }
    pub fn clear(&mut self) {
        self.lists.clear();
        self.bodies.clear();
    }
    pub fn raw_read(&self, lang: &str, id: &str) -> Option<String> {
        if self.assets.as_os_str().is_empty() || !valid(lang, 12) || !valid(id, 48) {
            return None;
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.assets.join("help").join(lang).join(format!("{id}.md")))
            .ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_ARTICLE as u64 {
            return None;
        }
        // Bound the read too, in case the file grows after metadata was read.
        let mut bytes = Vec::new();
        file.take(MAX_ARTICLE as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= MAX_ARTICLE).then(|| String::from_utf8_lossy(&bytes).into_owned())
    }
    pub fn raw_list(&mut self, lang: &str) -> Vec<Metadata> {
        if let Some(list) = self.lists.get(lang) {
            return list.clone();
        }
        if self.assets.as_os_str().is_empty() || !valid(lang, 12) {
            return Vec::new();
        }
        let mut list = Vec::new();
        if let Ok(entries) = fs::read_dir(self.assets.join("help").join(lang)) {
            // Only accepted articles count against the catalog limit, as in lord.help.list.
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(id) = name.to_str().and_then(|s| s.strip_suffix(".md")) else {
                    continue;
                };
                if let Some(raw) = self.raw_read(lang, id) {
                    list.push(parse(id, &raw).meta);
                    if list.len() >= MAX_ARTICLES {
                        break;
                    }
                }
            }
        }
        // Directory enumeration order must not decide the order of equal-score hits.
        list.sort_by(|a, b| a.id.cmp(&b.id));
        if self.lists.len() >= MAX_LANGUAGES {
            self.lists.clear();
        }
        self.lists.insert(lang.into(), list.clone());
        list
    }
    pub fn list(&mut self, lang: &str) -> Vec<Metadata> {
        if !valid(lang, 12) {
            return Vec::new();
        }
        let mut list: BTreeMap<_, _> = self
            .raw_list("en")
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        if lang != "en" {
            list.extend(self.raw_list(lang).into_iter().map(|a| (a.id.clone(), a)));
        }
        let mut list: Vec<_> = list.into_values().collect();
        list.sort_by_cached_key(|a| (a.order, title_key(&a.title)));
        list
    }
    pub fn read(&mut self, lang: &str, id: &str) -> Option<Article> {
        if !valid(lang, 12) || !valid(id, 48) {
            return None;
        }
        if let Some((_, _, a)) = self.bodies.iter().find(|(l, i, _)| l == lang && i == id) {
            return Some(a.clone());
        }
        let raw = self
            .raw_read(lang, id)
            .or_else(|| (lang != "en").then(|| self.raw_read("en", id)).flatten())?;
        let article = parse(id, &raw);
        if self.bodies.len() >= MAX_BODIES {
            self.bodies.pop_front();
        }
        self.bodies
            .push_back((lang.into(), id.into(), article.clone()));
        Some(article)
    }
    pub fn search(&mut self, lang: &str, query: &str) -> Vec<Value> {
        let q = fold(query);
        let q = crate::search::matching::trimmed(&q);
        if q.chars().count() < 2 || q.len() > 4096 {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for item in self.list(lang) {
            let Some(a) = self.read(lang, &item.id) else {
                continue;
            };
            let title = fold(&a.meta.title);
            let body = fold(&a.body);
            let score = if title.starts_with(q) {
                90
            } else if title.contains(q) {
                70
            } else if fold(&a.meta.keywords).contains(q) {
                55
            } else if body.contains(q) {
                30
            } else {
                continue;
            };
            hits.push(json!({"id":a.meta.id,"title":a.meta.title,"score":score,
                "snippet":snippet(&a.body,q,body.contains(q))}));
        }
        // Stable sorting retains the sidebar's ordering for equal scores.
        hits.sort_by_key(|hit| std::cmp::Reverse(hit["score"].as_i64().unwrap_or(0)));
        hits
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let s = |key: &str| input[key].as_str().unwrap_or("");
        let lang = s("lang");
        match s("cmd") {
            "rawList" => json!(self.raw_list(lang)),
            "rawRead" => json!(self.raw_read(lang, s("id"))),
            "list" => json!(self.list(lang)),
            "read" => json!(self.read(lang, s("id"))),
            "search" => json!(self.search(lang, s("query"))),
            "link" => link(s("url")),
            "clear" => {
                self.clear();
                Value::Null
            }
            _ => Value::Null,
        }
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn snippet(body: &str, q: &str, matched_body: bool) -> String {
    if matched_body {
        let raw = collapse(
            &body
                .chars()
                .filter(|c| !"#*`>[]()_".contains(*c))
                .collect::<String>(),
        );
        let normalized = fold(&raw);
        if let Some(at) = normalized.find(q) {
            // Index by characters, never by bytes: snippets must not split UTF-8.
            // Folding can remove combining marks or expand a character. Map
            // the normalized position back into the original string first.
            let mut boundaries: Vec<_> = raw.char_indices().map(|(at, _)| at).collect();
            boundaries.push(raw.len());
            let k = boundaries
                .partition_point(|&byte| fold(&raw[..byte]).len() <= at)
                .saturating_sub(1);
            let chars: Vec<_> = raw.chars().collect();
            let mut from = k.saturating_sub(60).min(chars.len());
            while from > 0 && from < k && chars.get(from - 1) != Some(&' ') {
                from += 1;
            }
            let result: String = chars
                .iter()
                .skip(from)
                .take(k.saturating_add(140).saturating_sub(from))
                .collect();
            let result = result.trim();
            if !result.is_empty() {
                return format!("{}{result}", if from > 0 { "…" } else { "" });
            }
        }
    }
    let mut skipped = false;
    let text = body
        .lines()
        .filter(|line| {
            if !skipped && line.starts_with('#') {
                skipped = true;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    collapse(&text).chars().take(180).collect()
}

/// Only the closed set of Help's internal URI schemes has desktop effects.
pub fn link(url: &str) -> Value {
    let Some((scheme, id)) = url.split_once(':') else {
        return Value::Null;
    };
    let valid = !id.is_empty()
        && id.bytes().all(|c| {
            c.is_ascii_lowercase() || (scheme != "settings" && (c.is_ascii_digit() || c == b'-'))
        });
    if !valid {
        return Value::Null;
    }
    match scheme {
        "settings" => json!({"app":"settings","args":{"section":id}}),
        "app" => json!({"app":id}),
        "help" => json!({"app":"help","args":{"article":id}}),
        _ => Value::Null,
    }
}

thread_local! { static SESSION: RefCell<Content> = RefCell::new(Content::default()); }

/// UI-thread bridge. The root comes from the compositor, never from request JSON.
#[no_mangle]
pub unsafe extern "C" fn axia_help_request(
    assets: *const c_char,
    input: *const c_char,
) -> *mut c_char {
    if assets.is_null() || input.is_null() {
        return std::ptr::null_mut();
    }
    let assets = CStr::from_ptr(assets).to_string_lossy();
    let input = CStr::from_ptr(input).to_bytes();
    if input.len() > 1 << 20 {
        return std::ptr::null_mut();
    }
    let Ok(input) = serde_json::from_slice::<Value>(input) else {
        return std::ptr::null_mut();
    };
    let result = SESSION.with(|service| {
        let mut service = service.borrow_mut();
        if service.assets != Path::new(assets.as_ref()) {
            *service = Content::new(assets.as_ref());
        }
        service.request(&input)
    });
    CString::new(result.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}
