//! Application catalog and complete menu state/widget descriptions. The host
//! retains only rendering, callback objects and access to shared shell services.
mod session;
mod dock_model;
mod osk_model;
mod ui;
mod unicode;
use serde_json::{json, Value};
use std::collections::HashMap;
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

struct Entry {
    sort_key: String,
    heading: String,
    search: Vec<String>,
    category: &'static str,
    metadata: Value,
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
    generation: u64,
    menus: HashMap<u64, session::Menu>,
    sequence: u64,
    search: crate::search::Search,
}

impl AxiaLauncher {
    #[cfg(test)]
    pub(crate) fn menu_count(&self) -> usize {
        self.menus.len()
    }
    pub(crate) fn replace(&mut self, value: &Value) -> Result<Value, &'static str> {
        let inputs = value.as_array().ok_or("invalid catalog")?;
        if inputs.len() > MAX_APPS {
            return Err("catalog limit exceeded");
        }
        let mut entries = Vec::with_capacity(inputs.len());
        for raw in inputs {
            let name = raw["name"].as_str().ok_or("invalid application name")?;
            let id = raw["id"].as_str().ok_or("invalid application id")?;
            if id.is_empty() || id.len() > 256 || !raw.is_object() {
                return Err("invalid application id");
            }
            let fields = [
                name,
                id,
                string(raw, "genericName"),
                string(raw, "keywords"),
                string(raw, "comment"),
                string(raw, "displayName"),
            ];
            if fields
                .iter()
                .chain([string(raw, "category"), string(raw, "categories")].iter())
                .any(|s| s.len() > 16384)
            {
                return Err("catalog limit exceeded");
            }
            let cat = category(string(raw, "category"), string(raw, "categories"));
            let mut metadata = raw.clone();
            metadata["launcherCategory"] = json!(cat);
            metadata["dockId"] = json!(session::dock_id(&metadata));
            entries.push(Entry {
                sort_key: unicode::nfc(name),
                heading: name
                    .chars()
                    .next()
                    .unwrap_or('#')
                    .to_uppercase()
                    .to_string(),
                search: fields.iter().map(|s| s.to_lowercase()).collect(),
                category: cat,
                metadata,
            });
        }
        let mut order: Vec<_> = (0..entries.len()).collect();
        // UTF-8 byte order equals Unicode scalar order for valid strings;
        // QuickJS's localeCompare compares NFC Unicode scalars. Stable ties.
        order.sort_by(|a, b| entries[*a].sort_key.cmp(&entries[*b].sort_key));
        let categories: Vec<_> = CATEGORIES
            .into_iter()
            .filter(|c| entries.iter().any(|a| a.category == *c))
            .collect();
        self.entries = entries;
        self.order = order;
        self.categories = categories;
        self.filter = None;
        self.matches.clear();
        self.list_geometry = None;
        self.list_rows.clear();
        self.generation = self.generation.wrapping_add(1);
        self.search.apps(self.order.iter().map(|i| self.entries[*i].metadata.clone()));
        Ok(self.catalog())
    }

    fn catalog(&self) -> Value {
        json!({"order":self.order,"categories":self.categories,"generation":self.generation,
            "source":self.entries.iter().map(|a| &a.metadata).collect::<Vec<_>>()})
    }

    fn sync(&mut self, args: &Value) -> Result<Value, &'static str> {
        let own = args["native"].as_array().ok_or("invalid native catalog")?;
        let linux = args["linux"].as_array().ok_or("invalid Linux catalog")?;
        if own.len() + linux.len() > MAX_APPS {
            return Err("catalog limit exceeded");
        }
        let mut source = Vec::with_capacity(own.len() + linux.len());
        let names: Value = serde_json::from_str(string(args, "names")).unwrap_or(Value::Null);
        let fallback: Value =
            serde_json::from_str(string(args, "fallbackNames")).unwrap_or(Value::Null);
        for original in own {
            if !original.is_object() {
                return Err("invalid native application");
            }
            if truthy(&original["builtin"])
                || truthy(&original["hidden"])
                || (truthy(&original["live"]) && !truthy(&args["live"]))
            {
                continue;
            }
            let mut a = original.clone();
            let id = string(&a, "id");
            if let Some(name) = names[id].as_str().or_else(|| fallback[id].as_str()) {
                a["name"] = json!(name);
            }
            a["kind"] = json!(if truthy(&a["package"]) {
                "package"
            } else {
                "lordos"
            });
            source.push(a);
        }
        for original in linux {
            if !original.is_object() {
                return Err("invalid Linux application");
            }
            let mut a = original.clone();
            let id = string(&a, "id").to_owned();
            a["desktopId"] = json!(id);
            a["id"] = json!(format!("desktop:{id}"));
            a["kind"] = json!("linux");
            let key = string(&a, "id");
            a["displayName"] = names[key]
                .as_str()
                .or_else(|| fallback[key].as_str())
                .map(|name| json!(name))
                .unwrap_or_else(|| a["name"].clone());
            source.push(a);
        }
        self.replace(&json!(source))
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
        let normalized = unicode::query(args["query"].as_str().unwrap_or(""));
        let query = normalized.as_str();
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

    pub(crate) fn request(&mut self, args: &Value) -> Result<Value, &'static str> {
        match args["op"].as_str().unwrap_or("") {
            "dock" => dock_model::request(args),
            "osk" => osk_model::request(args),
            "search" => self.search.request(args),
            "sync" => self.sync(args),
            "catalog" => Ok(self.catalog()),
            "categories" => Ok(json!({"categories":CATEGORIES})),
            "view" => self.view(args),
            "placement" => Ok(session::placement(args)),
            "open" | "context" | "event" | "scroll" | "close" => self.menu_request(args),
            "window" => {
                let want = string(args, "id").to_lowercase();
                Ok(
                    json!({"id": self.entries.iter().find(|a| string(&a.metadata,"kind")=="linux" && !want.is_empty() &&
                    (string(&a.metadata,"desktopId").to_lowercase()==want || string(&a.metadata,"wmClass").to_lowercase()==want ||
                        string(&a.metadata,"desktopId").to_lowercase().ends_with(&format!(".{want}")))).map(|a| string(&a.metadata,"id"))}),
                )
            }
            "category" => Ok(
                json!({"category":category(args["category"].as_str().unwrap_or(""),
                args["categories"].as_str().unwrap_or(""))}),
            ),
            _ => Err("unknown launcher operation"),
        }
    }
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.),
        _ => true,
    }
}

fn number(args: &Value, name: &str, default: f64, min: f64, max: f64) -> f64 {
    args[name]
        .as_f64()
        .filter(|v| v.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}

/// The launcher of the shell (the first one made): the native views of the shell read the same
/// catalog and menu state as the JS bridge. Both run on the UI thread, never inside each other.
static SHELL: std::sync::atomic::AtomicPtr<AxiaLauncher> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// A request to the shell's launcher, as `lord.launcher.request` makes it.
pub(crate) fn shell_request(args: &Value) -> Result<Value, &'static str> {
    // SAFETY: the pointer is the live handle that axia_launcher_new gave the registrar, cleared by
    // axia_launcher_free before it is dropped.
    let state = unsafe { SHELL.load(std::sync::atomic::Ordering::Acquire).as_mut() };
    state.ok_or("no launcher")?.request(args)
}

#[no_mangle]
pub extern "C" fn axia_launcher_new() -> *mut AxiaLauncher {
    let state = Box::into_raw(Box::new(AxiaLauncher::default()));
    let _ = SHELL.compare_exchange(std::ptr::null_mut(), state, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire);
    state
}

/// Session-only capability, set by the C registrar, never by JSON requests.
/// # Safety
/// State is null or a live UI-thread handle; roots are readable NUL-terminated
/// strings for the duration of this call. No caller pointers are retained.
#[no_mangle]
pub unsafe extern "C" fn axia_launcher_enable_search(
    state: *mut AxiaLauncher,
    home: *const c_char,
    assets: *const c_char,
) {
    if let Some(state) = state.as_mut() {
        if !home.is_null() && !assets.is_null() {
            state.search.configure(
                CStr::from_ptr(home).to_string_lossy().as_ref().into(),
                CStr::from_ptr(assets).to_string_lossy().as_ref().into(),
            );
        }
    }
}

/// # Safety
/// Null or a live handle from new(), freed exactly once on the UI thread.
#[no_mangle]
pub unsafe extern "C" fn axia_launcher_free(state: *mut AxiaLauncher) {
    let _ = SHELL.compare_exchange(state, std::ptr::null_mut(), std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire);
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
