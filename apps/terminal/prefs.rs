//! The native Preferences window of the Terminal: the state it keeps per window, the commands its
//! controls send, the key capture of the shortcut editor, the import of other terminals'
//! profiles and the facts the scene is drawn from (`prefs_ui.rs`). Every change goes through
//! `pf_commit`: the settings are updated at once, saved with a `settings` effect and the
//! sessions of every window are configured again (once per request, however many fields changed).
use super::prefs_ui as pui;
use super::*;
use crate::iterm::{self, Imported, Input};
use std::cell::RefCell;

const SECTION_COUNT: usize = 7;
const KEYMAP_MAX: usize = 200;
const TRIGGERS_MAX: usize = 200;
const IMPORT_MAX: u64 = 8 * 1024 * 1024;
const IMPORT_DIRS: [&str; 7] = ["Downloads", "Descargas", "Documents", "Documentos", "Desktop", "Escritorio", "."];
/// Profile fields a control may not change through the generic setter.
const LOCKED: [&str; 5] = ["id", "itermGuid", "custom", "triggers", "keyMap"];
const COLOR_KEYS: [&str; 8] = ["fg", "bg", "bold", "link", "cursor", "cursorText", "selection", "selectionText"];
const TRIGGER_ACTIONS: [&str; 9] = ["notify", "bell", "attention", "send", "capture", "highlight", "mark", "title", "stop"];
const KEY_KINDS: [&str; 4] = ["text", "escape", "hex", "ignore"];
/// Plain captions the form needs as text (drop-down entries), not as `{key}` nodes.
const PLAIN: [&str; 11] = ["fontBuiltin", "same", "bell", "tr.notify", "tr.attention", "tr.send", "tr.capture", "tr.highlight", "tr.mark", "tr.title", "tr.stop"];

/// A file chosen for import: its path as typed, what was found, what it holds.
#[derive(Clone, Default)]
pub(super) struct Import {
    path: String,
    files: Vec<String>,
    error: Option<String>,
    result: Option<Imported>,
    pick: Vec<bool>,
}
/// The state of one window's Preferences: the profile and section shown and the one control
/// that is open or waiting at the moment.
#[derive(Clone)]
pub(super) struct Prefs {
    sel: String,
    section: usize,
    picker: Option<String>,
    color: Option<(String, String)>,
    capture: Option<Value>,
    notice: Option<Value>,
    import: Option<Import>,
}
/// A slider being dragged: its field, range and where its track is on screen.
pub(super) struct Slide {
    field: String,
    min: f64,
    max: f64,
    frame: [f64; 4],
}

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut digits = vec![];
    while n > 0 {
        digits.push(char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    digits.iter().rev().collect()
}
fn new_profile_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("p{}{}", base36(clock::now().max(0) as u64), base36(COUNTER.fetch_add(1, Ordering::Relaxed)))
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.) != 0.,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn is_hex(v: &str) -> bool {
    v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit())
}
/// The name a profile shows: the factory one is translated (also in old settings that called it "Predeterminado").
pub(super) fn profile_caption(p: &Value) -> Value {
    match p["name"].as_str() {
        Some(n) if !n.is_empty() && !(p["id"] == "default" && n == "Predeterminado") => json!(n),
        _ => json!({"key":"terminal.profile.default"}),
    }
}
fn profile_mut<'a>(cfg: &'a mut Value, sel: &str) -> Option<&'a mut Value> {
    let list = cfg["profiles"].as_array_mut()?;
    let i = list.iter().position(|p| p["id"] == sel).unwrap_or(0);
    list.get_mut(i)
}
/// One of the lists the editors change: the global key map, the profile's, or its triggers.
fn list_mut<'a>(cfg: &'a mut Value, sel: &str, what: &str) -> Option<&'a mut Vec<Value>> {
    let target = if what == "global" { &mut *cfg } else { profile_mut(cfg, sel)? };
    let name = if what == "triggers" { "triggers" } else { "keyMap" };
    if !target[name].is_array() {
        target[name] = json!([]);
    }
    target[name].as_array_mut()
}
fn scope_name(global: bool) -> &'static str {
    if global { "global" } else { "profile" }
}
/// The range and kind (fractional or whole) of a numeric profile field.
fn number_range(field: &str) -> Option<(f64, f64, bool)> {
    Some(match field {
        "fontSize" => (7., 40., false),
        "lineSpacing" => (0.8, 2., true),
        "padding" => (0., 40., false),
        "minimumContrast" => (0., 1., true),
        "transparency" => (0., 0.9, true),
        "columns" => (0., 400., false),
        "rows" => (0., 200., false),
        "scrollback" => (100., 10_000_000., false),
        _ => return None,
    })
}
/// A value for a profile field, or None when the field is unknown, locked or of another type.
fn clean_field(field: &str, value: &Value) -> Option<Value> {
    if LOCKED.contains(&field) {
        return None;
    }
    let defaults = model(&json!({"cmd":"catalog"}));
    let reference = defaults["profile"].get(field)?;
    match (reference, value) {
        (Value::Bool(_), Value::Bool(_)) => Some(value.clone()),
        (Value::Number(_), Value::Number(n)) => {
            let (lo, hi, fractional) = number_range(field)?;
            let x = n.as_f64().filter(|x| x.is_finite())?.clamp(lo, hi);
            Some(if fractional { json!((x * 100.).round() / 100.) } else { json!(x.round() as i64) })
        }
        (Value::String(_), Value::String(s)) => {
            let limit = if field == "wordChars" { 60 } else { 2000 };
            let s: String = s.chars().take(limit).collect();
            let allowed: Option<&[&str]> = match field {
                "cursorStyle" => Some(&["block", "underline", "beam"]),
                "workingDir" => Some(&["home", "reuse", "custom"]),
                "bell" => Some(&["visual", "none"]),
                _ => None,
            };
            if allowed.is_some_and(|list| !list.contains(&s.as_str())) {
                return None;
            }
            Some(json!(s))
        }
        _ => None,
    }
}
fn candidates(home: &str) -> Vec<String> {
    let mut out = vec![];
    if home.is_empty() {
        return out;
    }
    for dir in IMPORT_DIRS {
        let Ok(read) = std::fs::read_dir(std::path::Path::new(home).join(dir)) else { continue };
        let mut names: Vec<String> = read
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                let lower = n.to_lowercase();
                !n.starts_with('.') && [".json", ".itermcolors", ".plist"].iter().any(|ext| lower.ends_with(ext))
            })
            .collect();
        names.sort();
        out.extend(names.into_iter().take(50).map(|n| if dir == "." { n } else { format!("{dir}/{n}") }));
    }
    out
}
/// The installed family for a PostScript name: an exact match, or the family named before the dash.
fn font_for(fonts: &[Value], name: &str) -> Option<String> {
    let flat = |s: &str| s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_lowercase();
    let exact = fonts.iter().find(|f| f["ps"].as_str().is_some_and(|ps| ps.eq_ignore_ascii_case(name)));
    let found = exact.or_else(|| {
        let family = flat(name.split('-').next().unwrap_or(""));
        fonts.iter().find(|f| f["family"].as_str().is_some_and(|fam| flat(fam) == family))
    });
    found.and_then(|f| f["family"].as_str().map(str::to_owned))
}
fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    names.filter(|n| seen.insert(*n)).map(str::to_owned).collect()
}

impl App {
    fn prefs<R>(&self, f: impl FnOnce(&mut Prefs) -> R) -> Option<R> {
        self.ui(|u| u.prefs.as_mut().map(f))
    }
    fn selected(&self) -> String {
        self.prefs(|p| p.sel.clone()).unwrap_or_default()
    }
    pub(super) fn prefs_open(&mut self) {
        if self.ui(|u| u.prefs.is_some()) {
            return;
        }
        let default = self.default_profile();
        let sel = self.profile(&default)["id"].as_str().unwrap_or("default").to_owned();
        self.ui(|u| {
            u.menu = None;
            u.popup = None;
            u.prefs = Some(Prefs { sel, section: 0, picker: None, color: None, capture: None, notice: None, import: None });
        });
        if shell(|s| s.fonts.is_none() && !std::mem::replace(&mut s.fonts_asked, true)) {
            self.jobs.enqueue(Tag::Fonts, "fonts.list", json!([]));
        }
    }
    pub(super) fn prefs_close(&mut self) {
        self.ui(|u| u.prefs = None);
        if self.slide.take().is_some() {
            self.capture = Some(false);
        }
    }
    pub(super) fn prefs_open_now(&self) -> bool {
        self.ui(|u| u.prefs.is_some())
    }

    // ---- saving ----

    /// Changes the whole terminal settings (defaults filled in) and queues the save.
    fn pf_commit(&mut self, change: impl FnOnce(&mut Value)) {
        let mut cfg = self.terminal_settings();
        change(&mut cfg);
        self.settings = cfg;
        self.reconfig = true;
    }
    /// The same for the selected profile.
    fn pf_edit(&mut self, change: impl FnOnce(&mut Map<String, Value>)) {
        let sel = self.selected();
        self.pf_commit(|cfg| {
            if let Some(m) = profile_mut(cfg, &sel).and_then(Value::as_object_mut) {
                change(m);
            }
        });
    }
    fn pf_set(&mut self, field: &str, value: &Value) {
        if let Some(clean) = clean_field(field, value) {
            self.pf_edit(|m| {
                m.insert(field.to_owned(), clean);
            });
        }
    }
    /// Done once per request: the effect that saves the settings and the sessions configured again.
    pub(super) fn pf_flush(&mut self) {
        if !std::mem::take(&mut self.reconfig) {
            return;
        }
        self.effects.push(json!({"kind":"settings","patch":{"terminal":self.settings.clone()}}));
        for sid in shell(|s| s.sessions.keys().copied().collect::<Vec<Sid>>()) {
            self.configure(sid);
        }
    }
    fn pf_field(&self, field: &str) -> Value {
        let sel = self.selected();
        self.profile(&sel)[field].clone()
    }
    fn pf_idle(&self) {
        self.prefs(|p| {
            p.picker = None;
            p.color = None;
            p.capture = None;
        });
    }

    // ---- commands ----

    /// A command of the Preferences controls; false when the command is not one of theirs.
    pub(super) fn prefs_command(&mut self, cmd: &str, v: &Value) -> bool {
        if !cmd.starts_with("pf") {
            return false;
        }
        if !self.prefs_open_now() {
            return true;
        }
        let key = &v["key"];
        match cmd {
            "pfDone" => self.prefs_close(),
            "pfSection" => {
                if let Some(i) = key.as_u64().filter(|i| (*i as usize) < SECTION_COUNT) {
                    self.pf_idle();
                    self.prefs(|p| p.section = i as usize);
                }
            }
            "pfSelect" => {
                if let Some(id) = key.as_str().filter(|id| self.terminal_settings()["profiles"].as_array().is_some_and(|l| l.iter().any(|p| p["id"] == *id))) {
                    self.pf_idle();
                    self.prefs(|p| p.sel = id.to_owned());
                }
            }
            "pfAdd" | "pfDup" => self.pf_add(cmd == "pfDup"),
            "pfRemove" => self.pf_remove(),
            "pfFlip" => {
                if let Some(field) = key.as_str() {
                    let now = self.pf_field(field);
                    self.pf_set(field, &json!(!truthy(&now)));
                }
            }
            "pfVal" => {
                if let Some(field) = key[0].as_str() {
                    self.pf_set(field, &key[1]);
                }
            }
            "pfText" => {
                if let (Some(field), Some(text)) = (key.as_str(), v["value"].as_str()) {
                    self.pf_set(field, &json!(text));
                }
            }
            "pfRules" => {
                let text = v["value"].as_str().unwrap_or("");
                let rules = regex::Regex::new(r"\s*;\s*").map(|re| re.split(text).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                self.pf_set("rules", &json!(rules));
            }
            "pfSlide" => self.slide_start(v),
            "pfMakeDefault" => {
                let sel = self.selected();
                self.pf_commit(|cfg| cfg["defaultProfile"] = json!(sel));
            }
            "pfHotkey" => {
                if let Some(value) = key.as_str().filter(|k| ["F12", "Super+`", "off"].contains(k)) {
                    self.pf_commit(|cfg| cfg["hotkey"] = json!(value));
                }
            }
            "pfTabColor" => {
                if let Some(color) = key.as_str().filter(|c| c.is_empty() || is_hex(c)) {
                    self.pf_set("tabColor", &json!(color));
                }
            }
            "pfPicker" => {
                if let Some(id) = key.as_str() {
                    self.prefs(|p| p.picker = if p.picker.as_deref() == Some(id) { None } else { Some(id.to_owned()) });
                }
            }
            "pfPick" => self.pf_pick(key[0].as_str().unwrap_or(""), &key[1]),
            "pfColorEdit" => {
                if let Some(color) = key.as_str().filter(|k| self.color_known(k)) {
                    let value = self.color_value(color);
                    self.prefs(|p| p.color = Some((color.to_owned(), value)));
                }
            }
            "pfColorText" => {
                let text = v["value"].as_str().unwrap_or("").chars().take(16).collect::<String>();
                self.prefs(|p| {
                    if let Some((_, edit)) = p.color.as_mut() {
                        *edit = text;
                    }
                });
            }
            "pfColorOk" => {
                if let Some((color, text)) = self.prefs(|p| p.color.take()).flatten() {
                    if is_hex(&text) {
                        self.pf_color(&color, &text.to_lowercase());
                    }
                }
            }
            "pfColorCancel" => {
                self.prefs(|p| p.color = None);
            }
            "pfScheme" => {
                if let Some(id) = key.as_str().filter(|id| self.scheme_known(id)) {
                    self.pf_set("colors", &json!(id));
                }
            }
            "pfCapture" => {
                if self.capture_ok(key) {
                    let spec = key.clone();
                    self.prefs(|p| p.capture = if p.capture.as_ref() == Some(&spec) { None } else { Some(spec) });
                }
            }
            "pfReset" => {
                if let Some(action) = key.as_str() {
                    self.pf_commit(|cfg| {
                        if let Some(map) = cfg["shortcuts"].as_object_mut() {
                            map.remove(action);
                        }
                    });
                }
            }
            "pfMapAdd" | "pfMapDel" | "pfMapKind" | "pfMapText" => self.pf_map(cmd, v),
            "pfTrAdd" | "pfTrDel" | "pfTrToggle" | "pfTrText" => self.pf_trigger(cmd, v),
            "pfImportOpen" => {
                let files = candidates(&self.home);
                self.prefs(|p| p.import = Some(Import { files, ..Default::default() }));
            }
            "pfImportClose" => {
                self.prefs(|p| p.import = None);
            }
            "pfImportPath" => {
                let path = v["value"].as_str().unwrap_or("").chars().take(1000).collect::<String>();
                self.prefs(|p| {
                    if let Some(i) = p.import.as_mut() {
                        i.path = path;
                    }
                });
            }
            "pfImportFile" => {
                if let Some(name) = key.as_str() {
                    let path = format!("~/{name}");
                    self.prefs(|p| {
                        if let Some(i) = p.import.as_mut() {
                            i.path = path;
                        }
                    });
                    self.import_load();
                }
            }
            "pfImportLoad" => self.import_load(),
            "pfImportPick" => {
                if let Some(i) = key.as_u64() {
                    self.prefs(|p| {
                        if let Some(flag) = p.import.as_mut().and_then(|imp| imp.pick.get_mut(i as usize)) {
                            *flag = !*flag;
                        }
                    });
                }
            }
            "pfImportApply" => self.import_apply(key == &json!(true)),
            _ => {}
        }
        true
    }
    fn pf_add(&mut self, duplicate: bool) {
        let sel = self.selected();
        let id = new_profile_id();
        let copy_name = {
            let from = self.profile(&sel);
            let shown = self.i18n.caption(&profile_caption(&from));
            self.i18n.caption(&json!({"key":"terminal.prefs.copyOf","params":{"name":shown}}))
        };
        let new_name = self.i18n.caption(&json!({"key":"terminal.prefs.newProfile"}));
        let base = if duplicate { self.profile(&sel) } else { model(&json!({"cmd":"catalog"}))["profile"].clone() };
        let mut profile = base;
        profile["id"] = json!(id);
        profile["itermGuid"] = json!("");
        profile["name"] = json!(if duplicate { copy_name } else { new_name });
        self.pf_commit(|cfg| {
            if let Some(list) = cfg["profiles"].as_array_mut() {
                list.push(profile);
            }
        });
        self.pf_idle();
        self.prefs(|p| p.sel = id);
    }
    fn pf_remove(&mut self) {
        let sel = self.selected();
        let cfg = self.terminal_settings();
        let ids: Vec<String> = cfg["profiles"].as_array().map(|l| l.iter().filter_map(|p| p["id"].as_str().map(str::to_owned)).collect()).unwrap_or_default();
        if ids.len() < 2 {
            return;
        }
        let rest: Vec<String> = ids.into_iter().filter(|id| *id != sel).collect();
        let first = rest[0].clone();
        self.pf_commit(|cfg| {
            if let Some(list) = cfg["profiles"].as_array_mut() {
                list.retain(|p| p["id"] != sel);
            }
            if cfg["defaultProfile"] == sel {
                cfg["defaultProfile"] = json!(first);
            }
        });
        self.pf_idle();
        self.prefs(|p| p.sel = first);
    }
    fn scheme_known(&self, id: &str) -> bool {
        if id == "custom" {
            return false;
        }
        let presets = model(&json!({"cmd":"catalog"}))["presets"].clone();
        presets.as_array().is_some_and(|l| l.iter().any(|e| e[0] == id)) || self.terminal_settings()["schemes"].get(id).is_some_and(Value::is_object)
    }
    fn color_known(&self, key: &str) -> bool {
        COLOR_KEYS.contains(&key) || key.strip_prefix("ansi:").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n < 16)
    }
    /// The color a control edits now, from the profile's scheme.
    fn color_value(&self, key: &str) -> String {
        let scheme = self.scheme_of(&self.profile(&self.selected()));
        let fallback = scheme["fg"].as_str().unwrap_or("#ffffff");
        match key.strip_prefix("ansi:").and_then(|n| n.parse::<usize>().ok()) {
            Some(i) => scheme["ansi"][i].as_str().unwrap_or(fallback).to_owned(),
            None => scheme[key].as_str().filter(|s| !s.is_empty()).unwrap_or(fallback).to_owned(),
        }
    }
    fn scheme_of(&self, profile: &Value) -> Value {
        model(&json!({"cmd":"scheme","settings":{"schemes":self.raw()["schemes"]},"profile":profile}))
    }
    /// Editing one color makes the profile's scheme a custom one, a copy of the one in use.
    fn pf_color(&mut self, key: &str, value: &str) {
        if !self.color_known(key) {
            return;
        }
        let base = self.scheme_of(&self.profile(&self.selected()));
        let mut custom = base;
        custom["name"] = json!(self.i18n.caption(&json!({"key":"terminal.prefs.custom"})));
        let mut ansi: Vec<Value> = custom["ansi"].as_array().cloned().unwrap_or_default();
        ansi.resize(16, json!("#888888"));
        match key.strip_prefix("ansi:").and_then(|n| n.parse::<usize>().ok()) {
            Some(i) => ansi[i] = json!(value),
            None => custom[key] = json!(value),
        }
        custom["ansi"] = json!(ansi);
        self.pf_edit(|m| {
            m.insert("colors".into(), json!("custom"));
            m.insert("custom".into(), custom);
        });
    }
    fn pf_pick(&mut self, id: &str, value: &Value) {
        let text = value.as_str().unwrap_or("");
        match id {
            "font" => self.pf_set("font", &json!(text)),
            "nonAscii" => self.pf_set("nonAsciiFont", &json!(text)),
            _ => {
                // the key editors' action lists ("km<scope>-<i>-act") and the triggers' ("tr-<i>")
                if let Some(rest) = id.strip_prefix("km") {
                    let mut parts = rest.split('-');
                    let (scope, index) = (parts.next(), parts.next().and_then(|i| i.parse::<usize>().ok()));
                    if let (Some(scope), Some(index), Some("act")) = (scope, index, parts.next()) {
                        if self.action_ids().iter().any(|a| a == text) {
                            self.pf_list("keymap", scope == "1", index, |m| m["action"] = json!(text));
                        }
                    }
                } else if let Some(index) = id.strip_prefix("tr-").and_then(|i| i.parse::<usize>().ok()) {
                    if TRIGGER_ACTIONS.contains(&text) {
                        self.pf_list("triggers", false, index, |m| m["action"] = json!(text));
                    }
                }
            }
        }
        self.prefs(|p| p.picker = None);
    }
    fn action_ids(&self) -> Vec<String> {
        model(&json!({"cmd":"keyCatalog"}))["bindings"].as_array().map(|l| l.iter().filter_map(|e| e[0].as_str().map(str::to_owned)).collect()).unwrap_or_default()
    }
    fn capture_ok(&self, spec: &Value) -> bool {
        match spec[0].as_str() {
            Some("sc") => spec[1].as_str().is_some_and(|a| self.action_ids().iter().any(|x| x == a)),
            Some("map") => spec[1].is_boolean() && spec[2].as_u64().is_some(),
            _ => false,
        }
    }
    /// One entry of a list (a key map or the triggers) changed in place.
    fn pf_list(&mut self, what: &str, global: bool, index: usize, change: impl FnOnce(&mut Value)) {
        let sel = self.selected();
        let name = if what == "triggers" { "triggers" } else { scope_name(global) };
        self.pf_commit(|cfg| {
            if let Some(entry) = list_mut(cfg, &sel, name).and_then(|l| l.get_mut(index)) {
                change(entry);
            }
        });
    }
    fn pf_map(&mut self, cmd: &str, v: &Value) {
        let key = &v["key"];
        let sel = self.selected();
        match cmd {
            "pfMapAdd" => {
                let name = scope_name(key == &json!(true));
                self.pf_commit(|cfg| {
                    if let Some(list) = list_mut(cfg, &sel, name).filter(|l| l.len() < KEYMAP_MAX) {
                        list.push(json!({"combo":"","action":"text","param":""}));
                    }
                });
            }
            "pfMapDel" => {
                if let (Some(global), Some(i)) = (key[0].as_bool(), key[1].as_u64()) {
                    self.pf_commit(|cfg| {
                        if let Some(list) = list_mut(cfg, &sel, scope_name(global)).filter(|l| (i as usize) < l.len()) {
                            list.remove(i as usize);
                        }
                    });
                }
            }
            "pfMapKind" => {
                if let (Some(global), Some(i), Some(kind)) = (key[0].as_bool(), key[1].as_u64(), key[2].as_str()) {
                    if KEY_KINDS.contains(&kind) {
                        let param = if kind == "hex" { "0x1b" } else { "" };
                        self.pf_list("keymap", global, i as usize, |m| {
                            m["action"] = json!(kind);
                            m["param"] = json!(param);
                        });
                    } else if kind == "action" {
                        self.pf_list("keymap", global, i as usize, |m| {
                            m["action"] = json!("newTab");
                            m["param"] = json!("");
                        });
                    }
                }
            }
            _ => {
                if let (Some(global), Some(i), Some(text)) = (key[0].as_bool(), key[1].as_u64(), v["value"].as_str()) {
                    let text: String = text.chars().take(1000).collect();
                    self.pf_list("keymap", global, i as usize, |m| m["param"] = json!(text));
                }
            }
        }
    }
    fn pf_trigger(&mut self, cmd: &str, v: &Value) {
        let key = &v["key"];
        let sel = self.selected();
        match cmd {
            "pfTrAdd" => self.pf_commit(|cfg| {
                if let Some(list) = list_mut(cfg, &sel, "triggers").filter(|l| l.len() < TRIGGERS_MAX) {
                    list.push(json!({"regex":"error","action":"notify","param":"","enabled":true}));
                }
            }),
            "pfTrDel" => {
                if let Some(i) = key.as_u64() {
                    self.pf_commit(|cfg| {
                        if let Some(list) = list_mut(cfg, &sel, "triggers").filter(|l| (i as usize) < l.len()) {
                            list.remove(i as usize);
                        }
                    });
                }
            }
            "pfTrToggle" => {
                if let Some(i) = key.as_u64() {
                    self.pf_list("triggers", false, i as usize, |t| t["enabled"] = json!(t["enabled"] == false));
                }
            }
            _ => {
                if let (Some(i), Some(field), Some(text)) = (key[0].as_u64(), key[1].as_str().filter(|f| ["regex", "param"].contains(f)), v["value"].as_str()) {
                    let text: String = text.chars().take(1000).collect();
                    self.pf_list("triggers", false, i as usize, |t| t[field] = json!(text));
                }
            }
        }
    }

    // ---- sliders ----

    fn slide_start(&mut self, v: &Value) {
        let key = &v["key"];
        let frame = &v["frame"];
        let (Some(field), Some(min), Some(max)) = (key[0].as_str(), key[1].as_f64(), key[2].as_f64()) else { return };
        let frame = [number(&frame[0], 0.), number(&frame[1], 0.), number(&frame[2], 0.), number(&frame[3], 0.)];
        if frame[2] <= 0. || max <= min {
            return;
        }
        self.slide = Some(Slide { field: field.to_owned(), min, max, frame });
        self.capture = Some(true);
        self.slide_to(number(&v["x"], frame[0]));
    }
    pub(super) fn slide_to(&mut self, x: f64) {
        let Some(slide) = &self.slide else { return };
        let fraction = ((x - slide.frame[0]) / slide.frame[2]).clamp(0., 1.);
        let (field, value) = (slide.field.clone(), slide.min + fraction * (slide.max - slide.min));
        self.pf_set(&field, &json!(value));
    }

    // ---- keys ----

    /// A shortcut or key-map combination was captured.
    fn pf_captured(&mut self, spec: &Value, combo: &str) {
        match spec[0].as_str() {
            Some("sc") => {
                let Some(action) = spec[1].as_str() else { return };
                let current = self.bindings();
                let existing: Vec<String> = current
                    .as_array()
                    .and_then(|l| l.iter().find(|e| e[0] == action))
                    .and_then(|e| e[1].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()))
                    .unwrap_or_default();
                let mut combos = vec![combo.to_owned()];
                combos.extend(existing.into_iter().skip(1).filter(|c| c != combo));
                self.pf_commit(|cfg| {
                    if !cfg["shortcuts"].is_object() {
                        cfg["shortcuts"] = json!({});
                    }
                    // a combination belongs to one action only: the other actions lose it
                    for entry in current.as_array().into_iter().flatten() {
                        let (Some(other), Some(list)) = (entry[0].as_str(), entry[1].as_array()) else { continue };
                        if other == action || !combos.iter().any(|c| list.iter().any(|x| x == c)) {
                            continue;
                        }
                        let kept: Vec<Value> = list.iter().filter(|x| !combos.iter().any(|c| *x == c)).cloned().collect();
                        cfg["shortcuts"][other] = json!(kept);
                    }
                    cfg["shortcuts"][action] = json!(combos);
                });
            }
            Some("map") => {
                if let (Some(global), Some(i)) = (spec[1].as_bool(), spec[2].as_u64()) {
                    self.pf_list("keymap", global, i as usize, |m| m["combo"] = json!(combo));
                }
            }
            _ => {}
        }
    }
    /// Every key while the Preferences are open: they wait for a combination, or close on Escape.
    pub(super) fn prefs_key(&mut self, key: &str, flags: i32, facts: &KeyFacts, now: i64) -> Value {
        self.prefs_key_inner(key, flags, facts);
        // saved now: the next sync must not bring the old settings back before they are
        self.pf_flush();
        self.out(now)
    }
    fn prefs_key_inner(&mut self, key: &str, flags: i32, facts: &KeyFacts) {
        if MODIFIER_CODES.contains(&facts.code) {
            return;
        }
        let plain = flags & (2 | 4 | 16) == 0 && !facts.shift;
        if let Some(spec) = self.prefs(|p| p.capture.clone()).flatten() {
            if key == "Escape" && plain {
                self.prefs(|p| p.capture = None);
            } else {
                let event = json!({"code":facts.code,"key":key,"meta":flags & 16 != 0,"ctrl":flags & 2 != 0,"alt":flags & 4 != 0,"shift":facts.shift});
                if let Some(combo) = model(&json!({"cmd":"combo","event":event})).as_str() {
                    self.prefs(|p| p.capture = None);
                    self.pf_captured(&spec, combo);
                }
            }
            return;
        }
        if key == "Escape" {
            let closed = self.prefs(|p| {
                if p.import.is_some() {
                    p.import = None;
                } else if p.picker.is_some() || p.color.is_some() {
                    p.picker = None;
                    p.color = None;
                } else {
                    return true;
                }
                false
            });
            if closed == Some(true) {
                self.prefs_close();
            }
        }
    }

    // ---- import ----

    fn read_import(&mut self, path: &str) -> Result<Vec<u8>, String> {
        use std::path::{Path, PathBuf};
        if self.home.is_empty() {
            return Err("no home".into());
        }
        let path = path.trim();
        let home = Path::new(&self.home);
        let full: PathBuf = if path == "~" || path.starts_with("~/") {
            home.join(path[1..].trim_start_matches('/'))
        } else if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            home.join(path)
        };
        let (root, file) = (home.canonicalize().map_err(|e| e.to_string())?, full.canonicalize().map_err(|e| e.to_string())?);
        if !file.starts_with(&root) {
            return Err(self.i18n.caption(&json!({"key":"terminal.iterm.notIterm"})));
        }
        let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > IMPORT_MAX {
            return Err(self.i18n.caption(&json!({"key":"terminal.iterm.notIterm"})));
        }
        std::fs::read(&file).map_err(|e| e.to_string())
    }
    fn run_import(&mut self, bytes: &[u8], path: &str) -> Result<Imported, String> {
        let fonts = shell(|s| s.fonts.clone()).unwrap_or_default();
        let defaults = model(&json!({"cmd":"catalog"}))["profile"].clone();
        let i18n = RefCell::new(std::mem::take(&mut self.i18n));
        let result = {
            let tr = |key: &str, params: Value| i18n.borrow_mut().caption(&json!({"key":key,"params":params}));
            let font = |name: &str| font_for(&fonts, name);
            let mut next = new_profile_id;
            let mut env = iterm::Env { defaults: &defaults, tr: &tr, font: &font, id: &mut next };
            if bytes.starts_with(b"bplist00") {
                iterm::import(Input::Bytes(bytes), path, &mut env)
            } else {
                match std::str::from_utf8(bytes) {
                    Ok(text) => iterm::import(Input::Text(text), path, &mut env),
                    Err(_) => Err(tr("terminal.iterm.notIterm", json!({}))),
                }
            }
        };
        self.i18n = i18n.into_inner();
        result
    }
    fn import_load(&mut self) {
        let Some(path) = self.prefs(|p| p.import.as_ref().map(|i| i.path.clone())).flatten() else { return };
        let outcome = self.read_import(&path).and_then(|bytes| self.run_import(&bytes, &path));
        self.prefs(|p| {
            if let Some(i) = p.import.as_mut() {
                match outcome {
                    Ok(result) => {
                        i.pick = vec![true; result.profiles.len()];
                        i.result = Some(result);
                        i.error = None;
                    }
                    Err(error) => {
                        i.result = None;
                        i.pick = vec![];
                        i.error = Some(error);
                    }
                }
            }
        });
    }
    fn import_apply(&mut self, make_default: bool) {
        let Some((result, pick)) = self.prefs(|p| p.import.as_ref().and_then(|i| i.result.clone().map(|r| (r, i.pick.clone())))).flatten() else { return };
        let mut added = 0;
        let mut first: Option<String> = None;
        let schemes = result.schemes.len();
        let keys = result.key_map.len();
        let slug = regex::Regex::new("[^a-z0-9]+").ok();
        self.pf_commit(|cfg| {
            let mut profiles: Vec<Value> = cfg["profiles"].as_array().cloned().unwrap_or_default();
            for (i, np) in result.profiles.iter().enumerate() {
                if !pick.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let guid = np["itermGuid"].as_str().unwrap_or("");
                let at = (!guid.is_empty()).then(|| profiles.iter().position(|x| x["itermGuid"] == guid)).flatten();
                let mut profile = np.clone();
                match at {
                    Some(n) => {
                        profile["id"] = profiles[n]["id"].clone();
                        profiles[n] = profile.clone();
                    }
                    None => profiles.push(profile.clone()),
                }
                if make_default && !guid.is_empty() && result.default_guid.as_deref() == Some(guid) {
                    cfg["defaultProfile"] = profile["id"].clone();
                }
                first.get_or_insert_with(|| profile["id"].as_str().unwrap_or("").to_owned());
                added += 1;
            }
            cfg["profiles"] = json!(profiles);
            if !cfg["schemes"].is_object() {
                cfg["schemes"] = json!({});
            }
            for s in &result.schemes {
                let name = s["name"].as_str().unwrap_or("").to_lowercase();
                let slug = slug.as_ref().map_or(name.clone(), |re| re.replace_all(&name, "_").into_owned());
                cfg["schemes"][format!("it_{slug}")] = s.clone();
            }
            let mut map: Vec<Value> = cfg["keyMap"].as_array().cloned().unwrap_or_default();
            map.retain(|m| !result.key_map.iter().any(|k| k["combo"] == m["combo"]));
            map.extend(result.key_map.iter().cloned());
            cfg["keyMap"] = json!(map);
        });
        let notice = json!({"key":"terminal.prefs.import.done","params":{"p":added,"s":schemes,"k":keys}});
        let exists = first.filter(|id| self.terminal_settings()["profiles"].as_array().is_some_and(|l| l.iter().any(|p| p["id"] == *id)));
        self.prefs(|p| {
            p.import = None;
            p.notice = Some(notice);
            if let Some(id) = exists {
                p.sel = id;
            }
        });
    }

    // ---- the scene ----

    /// The Preferences drawn over the window, when they are open.
    pub(super) fn prefs_scene(&mut self) -> Option<Value> {
        let theme = self.theme.clone();
        let facts = self.prefs_facts(&theme)?;
        Some(pui::tree(&facts))
    }
    fn prefs_facts<'a>(&mut self, theme: &'a Value) -> Option<pui::Facts<'a>> {
        let (sel, section, picker, color_edit, capture, notice, import) =
            self.prefs(|p| (p.sel.clone(), p.section, p.picker.clone(), p.color.clone(), p.capture.clone(), p.notice.clone(), p.import.clone()))?;
        let cfg = self.terminal_settings();
        let list = cfg["profiles"].as_array().cloned().unwrap_or_default();
        let p = list.iter().find(|x| x["id"] == sel).or_else(|| list.first()).cloned().unwrap_or(Value::Null);
        let scheme = self.scheme_of(&p);
        let catalog = model(&json!({"cmd":"catalog"}));
        let presets: Vec<(String, Value)> = catalog["presets"].as_array().map(|l| l.iter().map(|e| (e[0].as_str().unwrap_or("").to_owned(), e[1].clone())).collect()).unwrap_or_default();
        let imported: Vec<(String, Value)> = cfg["schemes"]
            .as_object()
            .map(|m| m.iter().filter(|(id, _)| !presets.iter().any(|(pid, _)| pid == *id)).map(|(id, s)| (id.clone(), s.clone())).collect())
            .unwrap_or_default();
        let profiles: Vec<pui::ProfileRow> = list
            .iter()
            .map(|x| pui::ProfileRow { id: x["id"].as_str().unwrap_or("").to_owned(), name: profile_caption(x), default: x["id"] == cfg["defaultProfile"] })
            .collect();
        let fonts = shell(|s| s.fonts.clone()).unwrap_or_default();
        let mono = unique(fonts.iter().filter(|f| f["mono"] == true).filter_map(|f| f["family"].as_str()));
        let families = unique(fonts.iter().filter(|f| f["color"] != true).filter_map(|f| f["family"].as_str()));
        // actions, sorted by their label as the user reads it
        let ids = self.action_ids();
        let mut actions: Vec<(String, String)> = ids.iter().map(|a| (a.clone(), self.i18n.caption(&json!({"key":format!("terminal.action.{a}")})))).collect();
        actions.sort_by_key(|a| a.1.to_lowercase());
        let bindings = self.bindings();
        let mut labels: std::collections::HashMap<String, String> = Default::default();
        let mut label_of = |combo: &str| {
            if !combo.is_empty() && !labels.contains_key(combo) {
                let text = model(&json!({"cmd":"comboLabel","value":combo})).as_str().unwrap_or("").to_owned();
                labels.insert(combo.to_owned(), text);
            }
        };
        let shortcuts: Vec<pui::ShortcutRow> = ids
            .iter()
            .map(|a| {
                let combos: Vec<String> = bindings
                    .as_array()
                    .and_then(|l| l.iter().find(|e| e[0] == a.as_str()))
                    .and_then(|e| e[1].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()))
                    .unwrap_or_default();
                for c in &combos {
                    label_of(c);
                }
                pui::ShortcutRow {
                    action: a.clone(),
                    label: actions.iter().find(|(id, _)| id == a).map(|(_, l)| l.clone()).unwrap_or_default(),
                    first: combos.first().cloned(),
                    others: combos.iter().skip(1).map(|c| model(&json!({"cmd":"comboLabel","value":c})).as_str().unwrap_or("").to_owned()).collect(),
                    changed: cfg["shortcuts"].get(a).is_some_and(|v| !v.is_null()),
                }
            })
            .collect();
        for map in [&cfg["keyMap"], &p["keyMap"]] {
            for entry in map.as_array().into_iter().flatten() {
                if let Some(c) = entry["combo"].as_str() {
                    label_of(c);
                }
            }
        }
        for key in PLAIN {
            let full = format!("terminal.prefs.{key}");
            let text = self.i18n.caption(&json!({"key":full}));
            labels.insert(full, text);
        }
        let import = import.map(|i| self.import_facts(&i, &cfg));
        Some(pui::Facts {
            theme,
            width: self.width,
            cfg: cfg.clone(),
            p,
            scheme,
            profiles,
            presets,
            imported,
            section,
            picker,
            color_edit,
            capture,
            notice,
            import,
            mono,
            families,
            actions,
            shortcuts,
            labels,
        })
    }
    fn import_facts(&mut self, i: &Import, cfg: &Value) -> pui::ImportFacts {
        let result = i.result.as_ref().map(|r| {
            let profiles = r
                .profiles
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    let mut parts = vec![format!("{} {} pt", p["font"].as_str().filter(|f| !f.is_empty()).unwrap_or("JetBrains Mono"), p["fontSize"])];
                    if p["ligatures"] == true {
                        parts.push(self.i18n.caption(&json!({"key":"terminal.prefs.import.ligatures"})));
                    }
                    for (field, key) in [("triggers", "terminal.prefs.import.triggers"), ("keyMap", "terminal.prefs.import.keys")] {
                        let count = p[field].as_array().map_or(0, Vec::len);
                        if count > 0 {
                            parts.push(self.i18n.caption(&json!({"key":key,"params":{"count":count}})));
                        }
                    }
                    let guid = p["itermGuid"].as_str().unwrap_or("");
                    pui::ImportRow {
                        name: p["name"].as_str().unwrap_or("").to_owned(),
                        detail: parts.join(" · "),
                        picked: i.pick.get(n).copied().unwrap_or(false),
                        star: !guid.is_empty() && r.default_guid.as_deref() == Some(guid),
                        scheme: model(&json!({"cmd":"scheme","settings":{"schemes":cfg["schemes"]},"profile":p})),
                    }
                })
                .collect();
            pui::ImportResult { profiles, schemes: r.schemes.clone(), warnings: r.warnings.clone(), can_default: r.default_guid.is_some() }
        });
        pui::ImportFacts { path: i.path.clone(), files: i.files.clone(), error: i.error.clone(), result }
    }
}
