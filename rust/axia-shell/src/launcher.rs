//! Cached application-menu policy. React owns icons/events; Rust returns only
//! source indices and the geometry of visible rows, never window/node pointers.
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

const MAX_APPS: usize = 4096;
const MAX_REQUEST: usize = 8 * 1024 * 1024;
const CATEGORIES: [&str; 11] = [
    "AudioVideo",
    "Development",
    "Education",
    "Game",
    "Graphics",
    "Network",
    "Office",
    "Science",
    "Settings",
    "System",
    "Utility",
];

fn category(explicit: &str, list: &str) -> &'static str {
    if let Some(c) = CATEGORIES.iter().find(|c| **c == explicit) {
        return c;
    }
    for entry in list.split(';') {
        if let Some(c) = CATEGORIES.iter().find(|c| **c == entry) {
            return c;
        }
    }
    for entry in list.split(';') {
        match entry {
            "Audio" | "Video" => return "AudioVideo",
            "TerminalEmulator" | "FileManager" => return "System",
            "WebBrowser" => return "Network",
            "TextEditor" => return "Utility",
            _ => {}
        }
    }
    "Utility"
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    // Prepared once by the host using the same Unicode tables as QuickJS's
    // localeCompare (NFC), toLowerCase and toUpperCase. No extra Unicode crate.
    sort_key: String,
    heading: String,
    search: Vec<String>,
    #[serde(default)]
    category: String,
    #[serde(default)]
    categories: String,
}
struct Entry {
    sort_key: String,
    heading: String,
    search: Vec<String>,
    category: &'static str,
}
struct ListRow {
    index: usize,
    top: f64,
    height: f64,
    heading: bool,
}
#[derive(Default)]
pub struct AxiaLauncher {
    entries: Vec<Entry>,
    order: Vec<usize>,
    categories: Vec<&'static str>,
    filter: Option<(String, String)>,
    matches: Vec<usize>,
    list_geometry: Option<(f64, f64)>,
    list_rows: Vec<ListRow>,
    list_height: f64,
}

impl AxiaLauncher {
    fn replace(&mut self, value: &Value) -> Result<Value, &'static str> {
        let inputs: Vec<Input> =
            serde_json::from_value(value.clone()).map_err(|_| "invalid catalog")?;
        if inputs.len() > MAX_APPS
            || inputs.iter().any(|a| {
                a.search.len() > 6
                    || [&a.sort_key, &a.heading, &a.category, &a.categories]
                        .into_iter()
                        .chain(a.search.iter())
                        .any(|s| s.len() > 16384)
            })
        {
            return Err("catalog limit exceeded");
        }
        let entries: Vec<_> = inputs
            .into_iter()
            .map(|a| Entry {
                category: category(&a.category, &a.categories),
                sort_key: a.sort_key,
                heading: a.heading,
                search: a.search,
            })
            .collect();
        let mut order: Vec<_> = (0..entries.len()).collect();
        // UTF-8 byte order equals Unicode scalar order for valid strings;
        // QuickJS's localeCompare compares NFC Unicode scalars. Stable ties.
        order.sort_by(|a, b| entries[*a].sort_key.cmp(&entries[*b].sort_key));
        let categories: Vec<_> = CATEGORIES
            .into_iter()
            .filter(|c| entries.iter().any(|a| a.category == *c))
            .collect();
        let reply = json!({"order":order,"categories":categories,
            "itemCategories":entries.iter().map(|a| a.category).collect::<Vec<_>>()});
        self.entries = entries;
        self.order = order;
        self.categories = categories;
        self.filter = None;
        self.matches.clear();
        self.list_geometry = None;
        self.list_rows.clear();
        Ok(reply)
    }

    fn select(&mut self, query: &str, cat: &str) {
        // A search intentionally ignores the selected category, as before.
        let cat = if query.is_empty() { cat } else { "" };
        if self
            .filter
            .as_ref()
            .is_some_and(|(q, c)| q == query && c == cat)
        {
            return;
        }
        self.matches.clear();
        self.list_geometry = None;
        self.matches.extend(self.order.iter().copied().filter(|i| {
            let a = &self.entries[*i];
            (cat.is_empty() || a.category == cat)
                && (query.is_empty() || a.search.iter().any(|s| s.contains(query)))
        }));
        self.filter = Some((query.to_owned(), cat.to_owned()));
    }

    fn view(&mut self, args: &Value) -> Result<Value, &'static str> {
        let query = args["query"].as_str().unwrap_or("");
        let cat = args["category"].as_str().unwrap_or("");
        if query.len() > 16384 {
            return Err("query limit exceeded");
        }
        let height = number(args, "height", 600., 1., 16384.);
        let offset = number(args, "offset", 0., 0., 1e9);
        self.select(query, cat);
        let mut items = Vec::new();
        let (content, offset) = match args["layout"].as_str().unwrap_or("grid") {
            "grid" => {
                let cols = args["columns"].as_u64().unwrap_or(6).clamp(1, 32) as usize;
                let item = number(args, "itemHeight", 98., 1., 1024.);
                let gap = number(args, "gap", 26., 0., 1024.);
                let pad = number(args, "padding", 16., 0., 1024.);
                let step = item + gap;
                let rows = self.matches.len().div_ceil(cols);
                let content = pad * 2. + rows as f64 * step - if rows > 0 { gap } else { 0. };
                let offset = offset.min((content - height).max(0.));
                let start = (((offset - pad).max(0.) / step).floor() as usize).saturating_sub(1);
                let end = (((offset + height - pad).max(0.) / step).ceil() as usize + 1).min(rows);
                for row in start..end {
                    items.push(json!({"row":row,"top":pad + row as f64 * step,
                        "indices":&self.matches[row * cols..((row + 1) * cols).min(self.matches.len())]}));
                }
                (content, offset)
            }
            "list" => {
                let item = number(args, "itemHeight", 44., 1., 1024.);
                let header = number(args, "headerHeight", 25., 0., 1024.);
                if self.list_geometry != Some((item, header)) {
                    self.list_rows.clear();
                    let mut top = 0.;
                    let mut letter = "";
                    for i in &self.matches {
                        let head = self.entries[*i].heading.as_str();
                        let heading = head != letter;
                        letter = head;
                        let size = item + if heading { header } else { 0. };
                        self.list_rows.push(ListRow {
                            index: *i,
                            top,
                            height: size,
                            heading,
                        });
                        top += size;
                    }
                    self.list_height = top;
                    self.list_geometry = Some((item, header));
                }
                let rows = &self.list_rows;
                let offset = offset.min((self.list_height - height).max(0.));
                let start = rows
                    .partition_point(|r| r.top + r.height <= offset)
                    .saturating_sub(1);
                let end = (rows.partition_point(|r| r.top < offset + height) + 1).min(rows.len());
                for row in &rows[start..end] {
                    let heading = row
                        .heading
                        .then_some(self.entries[row.index].heading.as_str());
                    items.push(json!({"index":row.index,"top":row.top,"height":row.height,"heading":heading}));
                }
                (self.list_height, offset)
            }
            _ => return Err("unknown layout"),
        };
        Ok(
            json!({"total":self.matches.len(),"first":self.matches.first(),
            "height":content,"offset":offset,"items":items,"categories":self.categories}),
        )
    }

    fn request(&mut self, args: &Value) -> Result<Value, &'static str> {
        match args["op"].as_str().unwrap_or("") {
            "catalog" => self.replace(&args["items"]),
            "view" => self.view(args),
            "category" => Ok(
                json!({"category":category(args["category"].as_str().unwrap_or(""),
                args["categories"].as_str().unwrap_or(""))}),
            ),
            _ => Err("unknown launcher operation"),
        }
    }
}

fn number(args: &Value, name: &str, default: f64, min: f64, max: f64) -> f64 {
    args[name]
        .as_f64()
        .filter(|v| v.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}

#[no_mangle]
pub extern "C" fn axia_launcher_new() -> *mut AxiaLauncher {
    Box::into_raw(Box::new(AxiaLauncher::default()))
}

/// # Safety
/// Null or a live handle from new(), freed exactly once on the UI thread.
#[no_mangle]
pub unsafe extern "C" fn axia_launcher_free(state: *mut AxiaLauncher) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

/// # Safety
/// State belongs to this UI context. Input is null or a NUL-terminated string
/// readable for this call. Neither input nor any host pointer is retained.
#[no_mangle]
pub unsafe extern "C" fn axia_launcher_request(
    state: *mut AxiaLauncher,
    input: *const c_char,
) -> *mut c_char {
    let result = (|| {
        let state = state.as_mut().ok_or("invalid launcher handle")?;
        if input.is_null() {
            return Err("missing launcher request");
        }
        let bytes = CStr::from_ptr(input).to_bytes();
        if bytes.len() > MAX_REQUEST {
            return Err("launcher request limit exceeded");
        }
        let args = serde_json::from_slice(bytes).map_err(|_| "invalid launcher JSON")?;
        state.request(&args)
    })();
    let value = result.unwrap_or_else(|error| json!({"error":error}));
    // JSON escapes embedded NULs, so this CString cannot contain one.
    CString::new(value.to_string()).unwrap().into_raw()
}

/// # Safety
/// Null or an unfreed reply allocated by axia_launcher_request.
#[no_mangle]
pub unsafe extern "C" fn axia_launcher_reply_free(reply: *mut c_char) {
    if !reply.is_null() {
        drop(CString::from_raw(reply));
    }
}

#[cfg(test)]
mod tests;
