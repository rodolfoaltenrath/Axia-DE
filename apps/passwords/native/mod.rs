//! Passwords drawn by Rust (a one-to-one port of the former React window), in the app's own isolated
//! process (axia-server --app passwords): secrets reach this process only as they did the JS
//! version (a value the user asks to see, copy or edit), never the shell. Every vault call goes
//! through the window's async bridge (runtime/native_vault_services.js) to axia-vault; files are
//! descriptors the system dialog opened, kept by the bridge and named here by an id.
mod detail;
mod glyphs;
mod lock;
mod main;
mod security;
mod sheets;
mod ui;
#[cfg(test)]
mod tests;

use crate::native_view::{async_jobs::Jobs, date_format, i18n::I18n, NativeApp};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use ui::{k, kp, Theme};

pub const ID: &str = "passwords";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Loading,
    None,
    Locked,
    Unlocked,
    Unavailable,
}

/// A file the system dialog opened for this window (the bridge holds its descriptor).
#[derive(Clone, Debug, PartialEq)]
pub struct Picked {
    pub id: u64,
    pub name: String,
}

/// The lock screens' form (unlock, create and the recovery key).
#[derive(Default)]
pub struct Lock {
    pub method: String,
    pub pw: String,
    pub pw2: String,
    pub rec: String,
    pub pin: String,
    pub keyfile: Option<Picked>,
    pub err: Value,
    pub busy: bool,
    pub ms: Option<Value>,
    pub score: Option<Value>,
    pub recovery: Option<String>,
    pub confirmed: bool,
    /// The method switch follows the slots until the user picks one.
    first: String,
}
impl Lock {
    fn wipe(&mut self) {
        self.pw.clear();
        self.pw2.clear();
        self.rec.clear();
        self.pin.clear();
    }
}

/// The unlocked window: selection, search and list.
pub struct Win {
    pub sel: String,
    pub cur: Option<String>,
    pub q: String,
    pub hits: Option<Vec<String>>,
    pub search_ms: Option<f64>,
    search_at: Option<i64>,
    search_gen: u64,
    pub sort_title: bool,
    pub issues: i64,
    issues_for: Option<usize>,
    pub open: HashMap<String, bool>,
}
impl Default for Win {
    fn default() -> Self {
        Self { sel: "all".into(), cur: None, q: String::new(), hits: None, search_ms: None, search_at: None, search_gen: 0, sort_title: true, issues: 0,
            issues_for: None, open: HashMap::new() }
    }
}

/// The item open in the detail pane.
pub struct Viewing {
    pub v: String,
    pub id: String,
    pub e: Option<Value>,
    pub err: Value,
    /// The Wi-Fi QR (its side in pixels), drawn from `rasters["qr"]`.
    pub qr: Option<(usize, ())>,
    pub qr_gen: u64,
    /// Revealed hidden values, by "key#index".
    pub shown: HashMap<String, String>,
    pub totp: Vec<Value>,
    pub totp_at: f64,
    totp_server: f64,
    totp_loading: bool,
    pub hist_open: bool,
    pub hist_full: Option<Value>,
}

/// The item being edited (or created).
pub struct Editor {
    pub v: String,
    pub entry: Option<Value>,
    pub cat: String,
    pub title: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub tag_in: String,
    pub rotate: i64,
    /// {k, t, l, v, keep, multiline, choices, custom, show}
    pub fields: Vec<Value>,
    pub adding: bool,
    pub new_type: String,
    pub err: Value,
    pub busy: bool,
    pub strength: Option<Value>,
}

#[derive(Default)]
pub struct SetState {
    pub tab: String,
    pub add: Option<String>,
    pub pw: String,
    pub pw2: String,
    pub old: String,
    pub pin: String,
    pub path: Option<Picked>,
    pub rec: Option<String>,
    pub rec_ok: bool,
    pub busy: bool,
    pub err: Value,
}
#[derive(Default)]
pub struct ImpState {
    pub file: Option<Picked>,
    pub key: Option<Picked>,
    pub pw: String,
    pub busy: bool,
    pub res: Option<Value>,
    pub err: Value,
}
#[derive(Default)]
pub struct ExpState {
    pub kind: String,
    pub ok: bool,
    pub err: Value,
}
pub struct CatState {
    pub orig: Option<Value>,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub parent: String,
    pub template: String,
    pub fields: Vec<Value>,
    pub err: Value,
}
pub struct TagState {
    pub tag: String,
    pub name: String,
    pub color: String,
}

pub enum Sheet {
    Gen { o: Value, res: Option<Value>, for_field: Option<usize>, drag: Option<(f64, f64)> },
    Settings(SetState),
    Import(ImpState),
    Export(ExpState),
    Cat(CatState),
    Tag(TagState),
    Att { name: String, image: Option<(usize, usize, u64)>, text: String },
}

#[derive(Default)]
pub struct SecState {
    pub rep: Option<Value>,
    pub busy: bool,
    for_len: Option<usize>,
}
#[derive(Default)]
pub struct CodesState {
    pub codes: Vec<Value>,
    pub at: f64,
    for_len: Option<usize>,
    loading: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PickFor {
    UnlockKey,
    SetKey,
    ImpFile,
    ImpKey,
    Hibp,
    AttAdd,
    AttSave(Value),
    QrScan(usize),
    Export(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Status { after_unlock: bool },
    List,
    Login,
    Unlock,
    Create,
    Strength,
    Pick(PickFor),
    SaveRecovery,
    Events,
    Lock,
    Menus,
    Search(u64),
    Issues,
    Report,
    Codes,
    Get(String),
    Totp(String),
    Reveal { id: String, key: String, copy: Option<Value> },
    Act { op: String, id: String },
    Qr(String),
    AttPreview(String),
    AttText(String),
    AttOp(&'static str),
    HistFull(String),
    Save,
    EdStrength,
    Generate,
    Clip(Value),
    Quiet,
    /// A step of a chain whose failure is told (categories, tags, settings).
    Step { toast: Option<Value>, close: bool, reload: bool },
    SetErr { toast: Option<Value>, status: bool },
    RecNew,
    Calibrate,
    Import,
    Export(String),
    QrScan(usize),
    EmptyTrash,
}

pub struct App {
    pub i18n: I18n,
    /// Dates are formatted while drawing (a second catalog, so drawing borrows the app only).
    fmt: RefCell<I18n>,
    pub theme: Value,
    pub rtl: bool,
    pub available: bool,
    pub width: f64,
    region: String,
    booted: bool,
    window: String,
    jobs: Jobs<Tag>,
    pub phase: Phase,
    pub status: Value,
    pub meta: Value,
    pub cats: Value,
    pub tags: Value,
    pub entries: Vec<Value>,
    pub login: Vec<Value>,
    pub login_unlocked: bool,
    pub toast: Option<(Value, i64)>,
    pub touch: bool,
    pub lock: Lock,
    pub win: Win,
    pub viewing: Option<Viewing>,
    pub editor: Option<Editor>,
    pub sheet: Option<Sheet>,
    pub security: SecState,
    pub codes: CodesState,
    /// The open Choice list.
    pub choice: Option<String>,
    /// The TextArea with the keyboard.
    pub ta: Option<String>,
    rasters: HashMap<String, (usize, usize, Vec<u8>)>,
    raster_gen: u64,
    catalog: Value,
    events_pending: bool,
    events_at: i64,
    menus_sent: Option<bool>,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            i18n: I18n::new("", None),
            fmt: RefCell::new(I18n::new("", None)),
            theme: Value::Null,
            rtl: false,
            available: true,
            width: 1040.,
            region: String::new(),
            booted: false,
            window: String::new(),
            jobs: Jobs::default(),
            phase: Phase::Loading,
            status: Value::Null,
            meta: json!({}),
            cats: json!([]),
            tags: json!([]),
            entries: vec![],
            login: vec![],
            login_unlocked: false,
            toast: None,
            touch: false,
            lock: Lock::default(),
            win: Win::default(),
            viewing: None,
            editor: None,
            sheet: None,
            security: SecState::default(),
            codes: CodesState::default(),
            choice: None,
            ta: None,
            rasters: HashMap::new(),
            raster_gen: 0,
            catalog: crate::passwords_model::request(&json!({"cmd":"catalog"})),
            events_pending: false,
            events_at: 0,
            menus_sent: None,
            last_tree: Value::Null,
        }
    }
}

/// The readable message of an axia-vault error code (api.js errorText).
pub fn error_text(code: &str) -> Value {
    const KNOWN: [&str; 33] = [
        "bad-key", "locked", "not-found", "corrupt", "truncated", "not-a-vault", "unsupported-version", "no-device", "keyfile-read", "keyfile-short",
        "keyfile-write", "unknown-format", "bad-password", "encrypted-export", "unsupported-cipher", "unsupported-kdf", "too-big", "no-qr",
        "not-an-image", "last-slot", "exists", "busy", "file", "fido-timeout", "fido-pin-required", "fido-pin-invalid", "fido-pin-blocked",
        "fido-no-credential", "fido-no-hmac", "fido-denied", "fido-restricted", "unavailable", "bad-keyfile",
    ];
    if KNOWN.contains(&code) {
        k(&format!("err.{code}"))
    } else {
        kp("err.other", json!({"code":if code.is_empty() { "?" } else { code }}))
    }
}

impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.fmt = RefCell::new(I18n::new(assets, home));
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(if self.jobs.busy() { 50. } else { 250. })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        // the vault locks on its own (timer, screen lock): the window follows while in the back
        self.booted.then_some(1000.)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        self.rasters.get(id).map(|(w, h, d)| (*w, *h, d.as_slice()))
    }
    fn text_input(&self) -> bool {
        self.ta.is_some()
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        let id = self.ta.clone()?;
        let v = format!("{}{text}", self.ta_value(&id));
        self.ta_set(&id, v);
        Some(json!({}))
    }
    fn blur(&mut self) {
        self.ta = None;
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        let now = crate::clock::now();
        let (shift, ctrl, meta) = (flags & 1 != 0, flags & 2 != 0, flags & 16 != 0);
        if let Some(id) = self.ta.clone() {
            let v = self.ta_value(&id);
            match key {
                "Backspace" => {
                    let mut c: Vec<char> = v.chars().collect();
                    c.pop();
                    self.ta_set(&id, c.into_iter().collect());
                }
                "Enter" => self.ta_set(&id, format!("{v}\n")),
                "Tab" => self.ta_set(&id, format!("{v}  ")),
                "Escape" => self.ta = None,
                _ if !chr.is_empty() && !ctrl && !meta && !chr.chars().any(char::is_control) => self.ta_set(&id, format!("{v}{chr}")),
                _ => return None,
            }
            return Some(self.pumped(now));
        }
        if self.phase != Phase::Unlocked {
            return None;
        }
        if !meta && !ctrl && (key == "ArrowDown" || key == "ArrowUp") && self.editor.is_none() && self.sheet.is_none() {
            let list = self.visible_list();
            if list.is_empty() {
                return None;
            }
            let i = list.iter().position(|e| e["id"].as_str() == self.win.cur.as_deref());
            let n = if key == "ArrowDown" { i.map_or(0, |i| (i + 1).min(list.len() - 1)) } else { i.map_or(0, |i| i.saturating_sub(1)) };
            let id = list[n]["id"].as_str().unwrap_or("").to_string();
            self.open_entry(&id);
            return Some(self.pumped(now));
        }
        if !meta {
            return None;
        }
        let base = if chr.is_empty() { key.to_lowercase() } else { chr.to_lowercase() };
        match base.as_str() {
            "n" => self.new_entry(),
            "g" => self.open_generator(None),
            "," => self.open_settings(),
            "l" if shift => self.lock_now(),
            _ => return None,
        }
        Some(self.pumped(now))
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn lc_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

impl App {
    fn t(&mut self, key: &str) -> String {
        self.i18n.caption(&k(key))
    }
    fn pumped(&mut self, now: i64) -> Value {
        self.jobs.pump(now).map(|effect| json!({"effect":effect})).unwrap_or_else(|| json!({}))
    }
    pub fn say(&mut self, caption: Value, now: i64) {
        self.toast = Some((caption, now + 2600));
    }

    // ---- the category catalog (apps/passwords/model.rs) ----

    pub fn catalog(&self) -> &Value {
        &self.catalog
    }
    pub fn all_cats(&self) -> Value {
        crate::passwords_model::request(&json!({"cmd":"categories","users":self.cats}))
    }
    pub fn builtin(&self, id: &str) -> Option<Value> {
        Some(crate::passwords_model::request(&json!({"cmd":"builtin","id":id}))).filter(|v| v.is_object())
    }
    pub fn cat_name(&self, cat: &Value) -> Value {
        if cat.is_null() {
            return json!("");
        }
        let id = s(&cat["id"]);
        let name = s(&cat["name"]);
        if cat["builtin"] == true || (self.builtin(id).is_some() && name.is_empty()) {
            return k(&format!("cat.{id}"));
        }
        json!(if name.is_empty() { id } else { name })
    }
    /// An item's category (its tile and name) and the built-in one it is.
    pub fn cat_of(&self, e: &Value) -> (Value, Option<Value>) {
        let id = s(&e["cat"]);
        let cats = self.all_cats();
        let cat = cats.as_array().into_iter().flatten().find(|c| c["id"] == id).cloned().or_else(|| self.builtin(id)).unwrap_or_else(|| json!({"id":id}));
        let b = self.builtin(s(&cat["id"]));
        (cat, b)
    }
    fn metadata(entries: &[Value]) -> Value {
        json!(entries.iter().map(|e| json!({"cat":e["cat"],"trash":e["trash"],"arch":e["arch"],"fav":e["fav"],"totp":e["totp"],"tags":e["tags"]})).collect::<Vec<_>>())
    }
    pub fn summary(&self, cats: &Value) -> Value {
        crate::passwords_model::request(&json!({"cmd":"summary","entries":Self::metadata(&self.entries),"categories":cats}))
    }
    pub fn live_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e["trash"] != true)
            .flat_map(|e| e["tags"].as_array().cloned().unwrap_or_default())
            .filter_map(|t| t.as_str().map(String::from))
            .collect();
        tags.sort_by(|a, b| lc_cmp(a, b));
        tags.dedup();
        tags
    }
    fn template_fields(&self, cat_id: &str) -> Vec<Value> {
        let cats = self.all_cats();
        let cat = cats.as_array().into_iter().flatten().find(|c| c["id"] == cat_id).cloned().unwrap_or(Value::Null);
        crate::passwords_model::request(&json!({"cmd":"fields","category":cat,"categories":cats})).as_array().cloned().unwrap_or_default()
    }
    pub fn field_label(&self, f: &Value) -> Value {
        if let Some(l) = f["l"].as_str().filter(|l| !l.is_empty()) {
            return json!(l);
        }
        let key = s(&f["k"]);
        let custom = key.len() > 1 && key.starts_with('c') && key[1..].bytes().all(|b| b.is_ascii_digit());
        if custom {
            k("field.custom")
        } else {
            k(&format!("field.{key}"))
        }
    }
    pub fn field_removable(&self, ed: &Editor, i: usize) -> bool {
        let f = &ed.fields[i];
        f["custom"] == true || !self.template_fields(&ed.cat).iter().any(|x| x["k"] == f["k"])
    }
    /// The items of the list: the selection, the search and the order.
    pub fn visible_list(&self) -> Vec<Value> {
        let sel = self.win.sel.as_str();
        let base = if sel == "login" { &self.login } else { &self.entries };
        let cats = self.all_cats();
        let idx = crate::passwords_model::request(&json!({"cmd":"filter","entries":Self::metadata(base),"selection":sel,"categories":cats}));
        let mut list: Vec<Value> = idx.as_array().into_iter().flatten().filter_map(|i| base.get(i.as_u64()? as usize).cloned()).collect();
        if !self.win.q.trim().is_empty() {
            if sel == "login" {
                let q = self.win.q.to_lowercase();
                list.retain(|e| format!("{} {} {}", s(&e["title"]), s(&e["sub"]), s(&e["url"])).to_lowercase().contains(&q));
            } else if let Some(hits) = &self.win.hits {
                list.retain(|e| hits.iter().any(|h| e["id"] == h.as_str()));
            }
        }
        if self.win.sort_title {
            list.sort_by(|a, b| lc_cmp(s(&a["title"]), s(&b["title"])));
        } else {
            list.sort_by(|a, b| b["modified"].as_f64().unwrap_or(0.).total_cmp(&a["modified"].as_f64().unwrap_or(0.)));
        }
        list
    }
    fn tm(secs: f64) -> libc::tm {
        crate::clock::local((secs * 1000.) as i64)
    }
    pub fn date(&self, secs: f64) -> String {
        date_format::date(&mut self.fmt.borrow_mut(), &self.region, &Self::tm(secs), "short")
    }
    pub fn date_time(&self, secs: f64) -> String {
        let tm = Self::tm(secs);
        let mut i = self.fmt.borrow_mut();
        let d = date_format::date(&mut i, &self.region, &tm, "short");
        format!("{d} {}", date_format::time(&mut i, &self.region, "auto", &tm, false))
    }

    // ---- vault calls ----

    fn vault(&mut self, tag: Tag, op: &str, args: Value, files: &[&Picked], fido: bool) {
        let ids: Vec<u64> = files.iter().map(|f| f.id).collect();
        self.jobs.enqueue(tag, "vault.request", json!([op, args, ids, fido]));
    }
    fn refresh_status(&mut self, after_unlock: bool) {
        self.vault(Tag::Status { after_unlock }, "status", json!({}), &[], false);
    }
    fn reload(&mut self) {
        self.vault(Tag::List, "list", json!({}), &[], false);
    }
    fn reload_login(&mut self) {
        self.vault(Tag::Login, "list", json!({"v":"login"}), &[], false);
    }
    fn pick(&mut self, purpose: PickFor, save: bool, suggest: &str) {
        self.jobs.enqueue(Tag::Pick(purpose), "file.pick", json!([save, suggest]));
    }
    /// Copying a secret: marked sensitive and cleared after the vault's delay if unchanged.
    fn copy_secret(&mut self, value: &str, label: Value) {
        let secs = self.meta["clip"].as_i64().filter(|n| *n > 0).unwrap_or(30);
        self.jobs.enqueue(Tag::Clip(kp("copied", json!({"what":label,"secs":secs}))), "clipboard.secret", json!([value, secs]));
    }
    fn copy_text(&mut self, value: &str, label: Value) {
        self.jobs.enqueue(Tag::Clip(kp("copiedPlain", json!({"what":label}))), "clipboard.text", json!([value]));
    }
    fn set_meta(&mut self, patch: Value, toast_err: bool) {
        let mut meta = if self.meta.is_object() { self.meta.clone() } else { json!({}) };
        for (k, v) in patch.as_object().into_iter().flatten() {
            meta[k] = v.clone();
        }
        self.meta = meta.clone();
        let tag = if toast_err { Tag::Step { toast: None, close: false, reload: false } } else { Tag::Quiet };
        self.vault(tag, "set-meta", json!({"meta":meta}), &[], false);
    }

    fn set_phase(&mut self, phase: Phase) {
        if phase != self.phase {
            self.phase = phase;
            self.lock.err = Value::Null;
            if phase != Phase::Unlocked {
                self.win.cur = None;
                self.viewing = None;
                self.editor = None;
                self.sheet = None;
                self.choice = None;
                self.ta = None;
                self.win.issues_for = None;
                self.security = SecState::default();
                self.codes = CodesState::default();
                self.rasters.clear();
            }
        }
    }
    fn sync_method(&mut self) {
        let first = match self.phase {
            Phase::Locked => lock::first_method(&self.status).to_string(),
            Phase::None => "password".into(),
            _ => return,
        };
        let key = format!("{:?}{first}", self.phase);
        if self.lock.first != key {
            self.lock.first = key;
            self.lock.method = first;
        }
    }

    // ---- the window ----

    fn select(&mut self, sel: &str) {
        self.win.sel = sel.into();
        self.win.cur = None;
        self.viewing = None;
        self.editor = None;
        self.choice = None;
        if sel == "login" {
            self.reload_login();
        }
        if sel == "security" {
            self.security.for_len = None;
        }
        if sel == "codes" {
            self.codes.for_len = None;
        }
        self.win.search_at = Some(0);
    }
    fn open_entry(&mut self, id: &str) {
        self.editor = None;
        self.win.cur = Some(id.into());
        let v = if self.win.sel == "login" { "login" } else { "main" };
        self.viewing = Some(Viewing { v: v.into(), id: id.into(), e: None, err: Value::Null, qr: None, qr_gen: 0, shown: HashMap::new(), totp: vec![], totp_at: 0.,
            totp_server: 0., totp_loading: false, hist_open: false, hist_full: None });
        self.rasters.remove("qr");
        self.load_entry();
    }
    fn load_entry(&mut self) {
        let Some(v) = &self.viewing else { return };
        let (vv, id) = (v.v.clone(), v.id.clone());
        self.vault(Tag::Get(id.clone()), "get", json!({"v":vv,"id":id,"hist":true}), &[], false);
    }
    fn load_totp(&mut self) {
        let Some(v) = &mut self.viewing else { return };
        if v.totp_loading {
            return;
        }
        v.totp_loading = true;
        let (vv, id) = (v.v.clone(), v.id.clone());
        self.vault(Tag::Totp(id.clone()), "totp", json!({"v":vv,"ids":[id]}), &[], false);
    }
    fn new_entry(&mut self) {
        let sel = self.win.sel.clone();
        let cat = sel.strip_prefix("cat:").unwrap_or("internet").to_string();
        let fields = self.fields_for(&cat);
        let v = if sel == "login" { "login" } else { "main" };
        self.editor = Some(Editor { v: v.into(), entry: None, cat, title: String::new(), notes: String::new(), tags: vec![], tag_in: String::new(), rotate: 0, fields,
            adding: false, new_type: "text".into(), err: Value::Null, busy: false, strength: None });
        self.win.cur = None;
        self.viewing = None;
    }
    fn fields_for(&self, cat: &str) -> Vec<Value> {
        self.template_fields(cat)
            .into_iter()
            .map(|mut f| {
                f["v"] = if f["t"] == "bool" { json!("false") } else if let Some(c) = f["choices"].as_array().and_then(|c| c.first()) { c.clone() } else { json!("") };
                f
            })
            .collect()
    }
    fn edit_entry(&mut self, e: &Value, v: &str) {
        let tpl = self.template_fields(s(&e["cat"]));
        let fields = e["f"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let tf = tpl.iter().find(|x| x["k"] == f["k"]);
                let mut n = f.clone();
                n["multiline"] = tf.map_or(Value::Null, |x| x["multiline"].clone());
                n["choices"] = tf.map_or(Value::Null, |x| x["choices"].clone());
                n["keep"] = json!(detail::hidden(f) && f["has"] == true && f["v"].is_null());
                if f["v"].is_null() {
                    n["v"] = json!("");
                }
                n["custom"] = json!(tf.is_none() && !s(&f["l"]).is_empty());
                n
            })
            .collect();
        self.editor = Some(Editor {
            v: v.into(),
            entry: Some(e.clone()),
            cat: s(&e["cat"]).into(),
            title: s(&e["title"]).into(),
            notes: s(&e["notes"]).into(),
            tags: e["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
            tag_in: String::new(),
            rotate: e["rotate"].as_i64().unwrap_or(0),
            fields,
            adding: false,
            new_type: "text".into(),
            err: Value::Null,
            busy: false,
            strength: None,
        });
        self.ed_strength();
    }
    fn ed_strength(&mut self) {
        let Some(ed) = &self.editor else { return };
        let pw = ed.fields.iter().find(|f| f["k"] == "password" && f["keep"] != true).map(|f| s(&f["v"]).to_string()).unwrap_or_default();
        self.jobs.cancel(|t| *t == Tag::EdStrength);
        if pw.is_empty() {
            if let Some(ed) = &mut self.editor {
                ed.strength = None;
            }
        } else {
            self.vault(Tag::EdStrength, "strength", json!({"password":pw}), &[], false);
        }
    }
    fn upsert(&mut self, item: &Value) {
        if !item.is_object() {
            return;
        }
        match self.entries.iter_mut().find(|e| e["id"] == item["id"]) {
            Some(e) => *e = item.clone(),
            None => self.entries.push(item.clone()),
        }
    }
    fn lock_now(&mut self) {
        self.vault(Tag::Lock, "lock", json!({}), &[], false);
    }
    fn open_generator(&mut self, for_field: Option<usize>) {
        let mut o = sheets::gen_defaults();
        for (k, v) in self.meta["gen"].as_object().into_iter().flatten() {
            o[k] = v.clone();
        }
        self.sheet = Some(Sheet::Gen { o, res: None, for_field, drag: None });
        self.generate();
    }
    fn open_settings(&mut self) {
        self.sheet = Some(Sheet::Settings(SetState { tab: "unlock".into(), ..SetState::default() }));
    }
    fn generate(&mut self) {
        let Some(Sheet::Gen { o, .. }) = &self.sheet else { return };
        let args = if o["mode"] == "words" {
            json!({"words":o["words"],"sep":o["sep"],"capitalize":o["capitalize"],"addDigit":o["addDigit"]})
        } else {
            json!({"length":o["length"],"upper":o["upper"],"lower":o["lower"],"digits":o["digits"],"symbols":o["symbols"],"noAmbiguous":o["noAmbiguous"]})
        };
        self.jobs.cancel(|t| *t == Tag::Generate);
        self.vault(Tag::Generate, "generate", args, &[], false);
    }
    fn gen_set(&mut self, key: &str, value: Value) {
        if let Some(Sheet::Gen { o, .. }) = &mut self.sheet {
            if o[key] == value {
                return;
            }
            o[key] = value;
        }
        self.generate();
    }
    fn gen_len_from(&mut self, x: f64) {
        let Some(Sheet::Gen { drag: Some((left, width)), .. }) = &self.sheet else { return };
        let f = ((x - left) / width.max(1.)).clamp(0., 1.);
        self.gen_set("length", json!((8. + f * 56.).round() as i64));
    }

    /// The window's menus (File, Tools), with what they can do now.
    fn menus(&mut self) {
        let unlocked = self.phase == Phase::Unlocked;
        if self.phase == Phase::Loading || self.window.is_empty() || self.menus_sent == Some(unlocked) {
            return;
        }
        self.menus_sent = Some(unlocked);
        let mut item = |label: &str, hint: Option<&str>, action: &str| {
            let mut i = json!({"label":self.i18n.caption(&k(label)),"action":[action],"disabled":!unlocked});
            if let Some(h) = hint {
                i["hint"] = json!(h);
            }
            i
        };
        let file = vec![
            item("menu.new", Some("❖N"), "new"), item("menu.newCat", None, "newCat"), json!("-"),
            item("menu.import", None, "import"), item("menu.export", None, "export"), json!("-"),
            item("menu.lock", Some("❖⇧L"), "lock"),
        ];
        let tools = vec![item("menu.generator", Some("❖G"), "generator"), item("menu.security", None, "security"), item("menu.settings", Some("❖,"), "settings")];
        let menus = json!([{"label":self.t("menu.file"),"items":file}, {"label":self.t("menu.tools"),"items":tools}]);
        self.jobs.enqueue(Tag::Menus, "windows.menus", json!([self.window, menus]));
    }
    /// A command of the window's menus or toolbar.
    fn menu(&mut self, cmd: &str) {
        if self.phase != Phase::Unlocked {
            return;
        }
        match cmd {
            "new" => self.new_entry(),
            "newCat" => self.open_cat(None),
            "import" => self.sheet = Some(Sheet::Import(ImpState::default())),
            "export" => self.sheet = Some(Sheet::Export(ExpState { kind: "vault".into(), ..ExpState::default() })),
            "lock" => self.lock_now(),
            "generator" => self.open_generator(None),
            "security" => self.select("security"),
            "settings" => self.open_settings(),
            _ => {}
        }
    }
    fn open_cat(&mut self, cat: Option<Value>) {
        let first_color = s(&self.catalog["colors"][0]).to_string();
        let c = cat.clone().unwrap_or(Value::Null);
        self.sheet = Some(Sheet::Cat(CatState {
            name: s(&c["name"]).into(),
            icon: glyphs::icon_kind(s(&c["icon"]), "folder"),
            color: c["color"].as_str().map(String::from).unwrap_or(first_color),
            parent: s(&c["parent"]).into(),
            template: c["template"].as_str().unwrap_or("general").into(),
            fields: c["fields"].as_array().cloned().unwrap_or_default(),
            err: Value::Null,
            orig: cat,
        }));
    }

    // ---- results ----

    fn apply(&mut self, tag: Tag, result: Result<Value, String>, now: i64) {
        match tag {
            Tag::Status { after_unlock } => {
                match result {
                    Ok(st) => {
                        let main = &st["main"];
                        let phase = if main["unlocked"] == true {
                            Phase::Unlocked
                        } else if main["exists"] == true || main["unreadable"] == true {
                            Phase::Locked
                        } else {
                            Phase::None
                        };
                        self.status = st;
                        if phase == Phase::Unlocked {
                            self.reload();
                        } else {
                            self.set_phase(phase);
                        }
                    }
                    Err(_) => self.set_phase(Phase::Unavailable),
                }
                if after_unlock {
                    self.lock.busy = false;
                    self.touch = false;
                }
                self.sync_method();
            }
            Tag::List => {
                if let Ok(r) = result {
                    self.meta = if r["meta"].is_object() { r["meta"].clone() } else { json!({}) };
                    self.cats = if r["cats"].is_array() { r["cats"].clone() } else { json!([]) };
                    self.tags = if r["tags"].is_array() { r["tags"].clone() } else { json!([]) };
                    self.entries = r["entries"].as_array().cloned().unwrap_or_default();
                    self.set_phase(Phase::Unlocked);
                    self.reload_login();
                    self.win.search_at = Some(0);
                }
            }
            Tag::Login => match result {
                Ok(r) => {
                    self.login = r["entries"].as_array().cloned().unwrap_or_default();
                    self.login_unlocked = true;
                }
                Err(_) => {
                    self.login.clear();
                    self.login_unlocked = false;
                }
            },
            Tag::Unlock => {
                // a descriptor is used up by the request: pick the key file again next time
                self.lock.keyfile = None;
                match result {
                    Ok(r) => {
                        self.lock.ms = (!r["ms"].is_null()).then(|| r["ms"].clone());
                        self.lock.wipe();
                        self.refresh_status(true);
                    }
                    Err(e) => {
                        self.lock.err = error_text(&e);
                        self.lock.busy = false;
                        self.touch = false;
                    }
                }
            }
            Tag::Create => {
                self.lock.keyfile = None;
                self.lock.busy = false;
                match result {
                    Ok(r) => {
                        self.lock.pw.clear();
                        self.lock.pw2.clear();
                        self.lock.score = None;
                        self.lock.recovery = Some(s(&r["recovery"]).to_string());
                        self.lock.confirmed = false;
                    }
                    Err(e) => self.lock.err = error_text(&e),
                }
            }
            Tag::Strength => {
                if let Ok(r) = result {
                    if !self.lock.pw.is_empty() {
                        self.lock.score = Some(r);
                    }
                }
            }
            Tag::Pick(purpose) => {
                let picked = result.ok().and_then(|r| Some(Picked { id: r["id"].as_u64()?, name: s(&r["name"]).into() }));
                if let Some(p) = picked {
                    self.picked(purpose, p);
                }
            }
            Tag::SaveRecovery => match result {
                Ok(Value::Null) => {}
                Ok(v) if v == true => self.say(k("recovery.saved"), now),
                _ => self.say(error_text("file"), now),
            },
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 250;
                if let Ok(list) = result {
                    for ev in list.as_array().into_iter().flatten() {
                        self.event(ev);
                    }
                }
            }
            Tag::Lock | Tag::EmptyTrash => self.refresh_status(false),
            Tag::Menus | Tag::Quiet => {}
            Tag::Search(gen) => {
                if gen == self.win.search_gen {
                    match result {
                        Ok(r) => {
                            self.win.hits = Some(r["ids"].as_array().into_iter().flatten().filter_map(|i| i.as_str().map(String::from)).collect());
                            self.win.search_ms = r["ms"].as_f64();
                        }
                        Err(_) => self.win.hits = None,
                    }
                }
            }
            Tag::Issues => {
                if let Ok(r) = result {
                    self.win.issues = security::issues(&r);
                }
            }
            Tag::Report => {
                self.security.busy = false;
                match result {
                    Ok(r) => self.security.rep = Some(r),
                    Err(e) => self.say(error_text(&e), now),
                }
            }
            Tag::Codes => {
                self.codes.loading = false;
                self.codes.at = now as f64 / 1000.;
                self.codes.codes = result.ok().and_then(|r| r["codes"].as_array().cloned()).unwrap_or_default();
            }
            Tag::Get(id) => {
                let Some(v) = self.viewing.as_mut().filter(|v| v.id == id) else { return };
                match result {
                    Ok(r) => {
                        let has_totp = r["entry"]["f"].as_array().into_iter().flatten().any(|f| f["t"] == "totp");
                        v.e = Some(r["entry"].clone());
                        v.err = Value::Null;
                        if has_totp && v.totp.is_empty() {
                            self.load_totp();
                        }
                    }
                    Err(e) => {
                        v.err = error_text(&e);
                        v.e = None;
                    }
                }
            }
            Tag::Totp(id) => {
                let Some(v) = self.viewing.as_mut().filter(|v| v.id == id) else { return };
                v.totp_loading = false;
                match result {
                    Ok(r) => {
                        v.totp_at = now as f64 / 1000.;
                        v.totp_server = r["now"].as_f64().unwrap_or(v.totp_at);
                        v.totp = r["codes"].as_array().cloned().unwrap_or_default();
                    }
                    Err(_) => v.totp.clear(),
                }
            }
            Tag::Reveal { id, key, copy } => match result {
                Ok(r) => {
                    let value = s(&r["value"]).to_string();
                    match copy {
                        Some(label) => self.copy_secret(&value, label),
                        None => {
                            if let Some(v) = self.viewing.as_mut().filter(|v| v.id == id) {
                                v.shown.insert(key, value);
                            }
                        }
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::Act { op, id } => match result {
                Ok(r) => {
                    if op == "purge" {
                        self.entries.retain(|e| e["id"] != id.as_str());
                        self.win.cur = None;
                        self.viewing = None;
                        return;
                    }
                    self.upsert(&r["item"]);
                    self.load_entry();
                    if op == "trash" {
                        self.say(k("toast.trashed"), now);
                        self.win.cur = None;
                        self.viewing = None;
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::Qr(id) => match result {
                Ok(qr) => {
                    let pane = (self.width - if self.width < 960. { 450. } else { 510. }).max(200.);
                    let scale = ((pane - 80.).min(240.) / (qr["size"].as_f64().unwrap_or(21.) + 8.)).floor().max(3.) as usize;
                    if let (Some((side, data)), Some(v)) = (ui::qr_pixels(&qr, scale), self.viewing.as_mut().filter(|v| v.id == id)) {
                        self.raster_gen += 1;
                        v.qr = Some((side, ()));
                        v.qr_gen = self.raster_gen;
                        self.rasters.insert("qr".into(), (side, side, data));
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::AttPreview(name) => match result {
                Ok(r) => {
                    let (w, h) = (r["w"].as_u64().unwrap_or(0) as usize, r["h"].as_u64().unwrap_or(0) as usize);
                    let data = ui::b64(s(&r["rgba"]));
                    if w > 0 && h > 0 && w <= 4096 && h <= 4096 && data.len() >= w * h * 4 {
                        self.raster_gen += 1;
                        self.rasters.insert("att".into(), (w, h, data));
                        self.sheet = Some(Sheet::Att { name, image: Some((w, h, self.raster_gen)), text: String::new() });
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::AttText(name) => match result {
                Ok(r) => self.sheet = Some(Sheet::Att { name, image: None, text: s(&r["text"]).into() }),
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::AttOp(kind) => match result {
                Ok(r) => {
                    if kind != "saved" {
                        self.upsert(&r["item"]);
                        self.load_entry();
                    }
                    match kind {
                        "added" => self.say(k("att.added"), now),
                        "saved" => self.say(k("att.saved"), now),
                        _ => {}
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::HistFull(id) => match result {
                Ok(r) => {
                    if let Some(v) = self.viewing.as_mut().filter(|v| v.id == id) {
                        v.hist_full = Some(r["entry"]["hist"].clone());
                    }
                }
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::Save => {
                let Some(ed) = &mut self.editor else { return };
                ed.busy = false;
                match result {
                    Ok(r) => {
                        self.upsert(&r["item"]);
                        let id = s(&r["id"]).to_string();
                        self.editor = None;
                        self.open_entry(&id);
                    }
                    Err(e) => ed.err = error_text(&e),
                }
            }
            Tag::EdStrength => {
                if let (Ok(r), Some(ed)) = (result, &mut self.editor) {
                    ed.strength = Some(r);
                }
            }
            Tag::Generate => {
                if let Some(Sheet::Gen { res, .. }) = &mut self.sheet {
                    *res = result.ok();
                }
            }
            Tag::Clip(caption) => match result {
                Ok(_) => self.say(caption, now),
                Err(e) => self.say(error_text(&e), now),
            },
            Tag::Step { toast, close, reload } => match result {
                Ok(_) => {
                    if let Some(t) = toast {
                        self.say(t, now);
                    }
                    if close {
                        self.sheet = None;
                    }
                    if reload {
                        self.reload();
                    }
                }
                Err(e) => {
                    self.jobs.cancel(|t| matches!(t, Tag::Step { .. }));
                    match &mut self.sheet {
                        Some(Sheet::Cat(c)) => c.err = error_text(&e),
                        _ => self.say(error_text(&e), now),
                    }
                }
            },
            Tag::SetErr { toast, status } => {
                let ok = result.is_ok();
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    st.busy = false;
                    match &result {
                        Ok(_) => {
                            st.add = None;
                            st.pw.clear();
                            st.pw2.clear();
                            st.old.clear();
                            st.pin.clear();
                            st.path = None;
                            st.err = Value::Null;
                        }
                        Err(e) => st.err = error_text(e),
                    }
                }
                if ok {
                    if status {
                        self.refresh_status(false);
                    }
                    if let Some(t) = toast {
                        self.say(t, now);
                    }
                }
            }
            Tag::RecNew => {
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    st.busy = false;
                    match result {
                        Ok(r) => {
                            st.rec = Some(s(&r["recovery"]).into());
                            st.rec_ok = false;
                        }
                        Err(e) => st.err = error_text(&e),
                    }
                }
                self.refresh_status(false);
            }
            Tag::Calibrate => {
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    st.busy = false;
                }
                match result {
                    Ok(r) => self.say(kp("set.measured", json!({"m":(r["m"].as_f64().unwrap_or(0.) / 1024.).round(),"t":r["t"],"ms":r["ms"]})), now),
                    Err(e) => {
                        if let Some(Sheet::Settings(st)) = &mut self.sheet {
                            st.err = error_text(&e);
                        }
                    }
                }
            }
            Tag::Import => {
                let mut ok = false;
                if let Some(Sheet::Import(st)) = &mut self.sheet {
                    st.busy = false;
                    // a picked file is a descriptor the request used up: pick again to retry
                    st.file = None;
                    st.key = None;
                    match result {
                        Ok(r) => {
                            st.res = Some(r);
                            ok = true;
                        }
                        Err(e) => st.err = error_text(&e),
                    }
                }
                if ok {
                    self.reload();
                }
            }
            Tag::Export(kind) => match result {
                Ok(r) => {
                    self.say(if kind == "csv" { kp("exp.doneCsv", json!({"count":r["count"]})) } else { k("exp.doneVault") }, now);
                    self.sheet = None;
                }
                Err(e) => {
                    if let Some(Sheet::Export(st)) = &mut self.sheet {
                        st.err = error_text(&e);
                    }
                }
            },
            Tag::QrScan(i) => match result {
                Ok(r) => {
                    if r["totp"] != true {
                        self.say(k("totp.notOtp"), now);
                        return;
                    }
                    if let Some(ed) = &mut self.editor {
                        if let Some(f) = ed.fields.get_mut(i) {
                            f["v"] = r["text"].clone();
                            f["keep"] = json!(false);
                        }
                        if ed.title.is_empty() {
                            if let Some(issuer) = r["issuer"].as_str().filter(|s| !s.is_empty()) {
                                ed.title = issuer.into();
                            }
                        }
                    }
                    let who = r["issuer"].as_str().filter(|s| !s.is_empty()).or(r["account"].as_str()).unwrap_or("").to_string();
                    self.say(kp("totp.added", json!({"issuer":who})), now);
                }
                Err(e) => self.say(error_text(&e), now),
            },
        }
    }
    /// A file the system dialog picked, for what asked for it.
    fn picked(&mut self, purpose: PickFor, p: Picked) {
        match purpose {
            PickFor::UnlockKey => self.lock.keyfile = Some(p),
            PickFor::SetKey => {
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    st.path = Some(p);
                }
            }
            PickFor::ImpFile | PickFor::ImpKey => {
                if let Some(Sheet::Import(st)) = &mut self.sheet {
                    if purpose == PickFor::ImpFile {
                        st.file = Some(p);
                    } else {
                        st.key = Some(p);
                    }
                }
            }
            PickFor::Hibp => {
                self.security.busy = true;
                self.vault(Tag::Report, "report", json!({}), &[&p], false);
            }
            PickFor::AttAdd => {
                let Some(v) = &self.viewing else { return };
                let name = p.name.rsplit('/').next().unwrap_or("").to_string();
                let args = json!({"v":v.v,"id":v.id,"name":name,"mime":detail::mime_of(&name)});
                self.vault(Tag::AttOp("added"), "att-add", args, &[&p], false);
            }
            PickFor::AttSave(att) => {
                let Some(v) = &self.viewing else { return };
                let args = json!({"v":v.v,"id":v.id,"att":att});
                self.vault(Tag::AttOp("saved"), "att-get", args, &[&p], false);
            }
            PickFor::QrScan(i) => self.vault(Tag::QrScan(i), "qr-scan", json!({}), &[&p], false),
            PickFor::Export(kind) => {
                let op = if kind == "csv" { "export-csv" } else { "export-vault" };
                self.vault(Tag::Export(kind), op, json!({}), &[&p], false);
            }
        }
    }
    /// axia-vault's events (locking, changes from elsewhere, the key's touch) and the menus' commands.
    fn event(&mut self, ev: &Value) {
        if ev["kind"] == "action" {
            let cmd = s(&ev["action"][0]).to_string();
            return self.menu(&cmd);
        }
        match s(&ev["ev"]) {
            "locked" if ev["v"] == "main" => {
                if self.phase == Phase::Unlocked {
                    self.set_phase(Phase::Locked);
                }
                self.entries.clear();
                self.touch = false;
            }
            "changed" if ev["v"] == "login" && self.phase == Phase::Unlocked => self.reload_login(),
            "touch" => self.touch = true,
            "crash" => {
                self.phase = Phase::Loading;
                self.refresh_status(false);
            }
            _ => {}
        }
    }

    // ---- the lock screens ----

    fn go(&mut self) {
        if self.lock.busy {
            return;
        }
        self.lock.err = Value::Null;
        match self.phase {
            Phase::Locked => self.unlock(),
            Phase::None if self.lock.recovery.is_none() => self.create(),
            _ => {}
        }
    }
    fn unlock(&mut self) {
        let f = &self.lock;
        let m = f.method.clone();
        let mut args = json!({"method":m});
        if m == "password" || m == "passkey" {
            args["password"] = json!(f.pw);
        }
        if m == "recovery" {
            args["recovery"] = json!(f.rec);
        }
        let mut files = vec![];
        if m == "keyfile" || m == "passkey" {
            match &f.keyfile {
                Some(p) => files.push(p.clone()),
                None => {
                    self.lock.err = k("unlock.needKeyfile");
                    return;
                }
            }
        }
        if m == "fido" && !f.pin.is_empty() {
            args["pin"] = json!(f.pin);
        }
        self.lock.busy = true;
        self.touch = false;
        let refs: Vec<&Picked> = files.iter().collect();
        self.vault(Tag::Unlock, "unlock", args, &refs, m == "fido");
    }
    fn create(&mut self) {
        let f = &self.lock;
        let m = f.method.clone();
        let need_pw = m == "password" || m == "passkey";
        let need_key = m == "keyfile" || m == "passkey";
        if need_pw && f.pw.chars().count() < 8 {
            self.lock.err = k("create.short");
            return;
        }
        if need_pw && f.pw != f.pw2 {
            self.lock.err = k("create.mismatch");
            return;
        }
        if need_key && f.keyfile.is_none() {
            self.lock.err = k("create.needKeyPath");
            return;
        }
        let mut args = json!({"method":m});
        if need_pw {
            args["password"] = json!(f.pw);
        }
        let files: Vec<Picked> = if need_key {
            args["newkey"] = json!(true);
            f.keyfile.iter().cloned().collect()
        } else {
            vec![]
        };
        if m == "fido" && !f.pin.is_empty() {
            args["pin"] = json!(f.pin);
        }
        self.lock.busy = true;
        let refs: Vec<&Picked> = files.iter().collect();
        self.vault(Tag::Create, "create", args, &refs, m == "fido");
    }
    fn save_recovery(&mut self, code: &str) {
        let text = format!("{}\n\n{code}\n\n{}\n", self.t("recovery.printTitle"), self.t("recovery.printBody"));
        self.jobs.enqueue(Tag::SaveRecovery, "file.save", json!(["clave-de-recuperacion.txt", text]));
    }

    // ---- multi-line fields and pop-up lists ----

    fn ta_value(&self, id: &str) -> String {
        let ed = self.editor.as_ref();
        if id == "notes" {
            return ed.map(|e| e.notes.clone()).unwrap_or_default();
        }
        let i: usize = id.strip_prefix("f:").and_then(|n| n.parse().ok()).unwrap_or(usize::MAX);
        ed.and_then(|e| e.fields.get(i)).map(|f| s(&f["v"]).to_string()).unwrap_or_default()
    }
    fn ta_set(&mut self, id: &str, v: String) {
        let Some(ed) = &mut self.editor else { return };
        if id == "notes" {
            ed.notes = v;
            return;
        }
        if let Some(f) = id.strip_prefix("f:").and_then(|n| n.parse::<usize>().ok()).and_then(|i| ed.fields.get_mut(i)) {
            f["v"] = json!(v);
            f["keep"] = json!(false);
        }
    }
    /// What the open pop-up list shows: its current value, the choices and its width.
    fn choice_spec(&self, id: &str) -> Option<(Value, Vec<(Value, Value)>, f64)> {
        let idx = |p: &str| id.strip_prefix(p).and_then(|n| n.parse::<usize>().ok());
        let types = || self.catalog["types"].as_array().cloned().unwrap_or_default().into_iter().map(|x| {
            let label = k(&format!("ftype.{}", s(&x)));
            (x, label)
        }).collect::<Vec<_>>();
        if let Some(ed) = &self.editor {
            match id {
                "cat" => {
                    let cats = self.all_cats();
                    let opts = cats.as_array().into_iter().flatten().map(|c| (c["id"].clone(), self.cat_name(c))).collect();
                    return Some((json!(ed.cat), opts, 200.));
                }
                "ftype" => return Some((json!(ed.new_type), types(), 170.)),
                "rotate" => return Some((json!(ed.rotate), detail::ROTATE.iter().map(|n| (json!(n), detail::rotate_caption(*n))).collect(), 170.)),
                _ => {}
            }
            if let Some(i) = idx("fch:") {
                let f = ed.fields.get(i)?;
                let fk = s(&f["k"]);
                let opts = f["choices"].as_array()?.iter().map(|c| {
                    let cs = s(c);
                    let label = if fk == "doc_type" { k(&format!("doc.{cs}")) } else if fk == "security" && cs == "none" { k("wifi.open") } else { c.clone() };
                    (c.clone(), label)
                }).collect();
                return Some((f["v"].clone(), opts, 240.));
            }
        }
        match (&self.sheet, id) {
            (Some(Sheet::Settings(_)), "autolock") => Some((json!(self.meta["autolock"].as_i64().unwrap_or(300)),
                sheets::AUTOLOCK.iter().map(|n| (json!(n), sheets::autolock_caption(*n))).collect(), 190.)),
            (Some(Sheet::Settings(_)), "clip") => Some((json!(self.meta["clip"].as_i64().unwrap_or(30)),
                sheets::CLIP.iter().map(|n| (json!(n), kp("set.sec", json!({"n":n})))).collect(), 190.)),
            (Some(Sheet::Cat(c)), "parent") => {
                let cats = self.all_cats();
                let own = c.orig.as_ref().map(|o| o["id"].clone()).unwrap_or(Value::Null);
                let mut opts = vec![(json!(""), k("cats.noParent"))];
                opts.extend(cats.as_array().into_iter().flatten().filter(|x| x["id"] != own).map(|x| (x["id"].clone(), self.cat_name(x))));
                Some((json!(c.parent), opts, 220.))
            }
            (Some(Sheet::Cat(c)), "template") => Some((json!(c.template),
                self.catalog["builtin"].as_array().into_iter().flatten().map(|b| (b["id"].clone(), k(&format!("cat.{}", s(&b["id"]))))).collect(), 220.)),
            (Some(Sheet::Cat(c)), _) => {
                let f = c.fields.get(idx("cft:")?)?;
                Some((json!(if f["multiline"] == true { "multiline" } else { f["t"].as_str().unwrap_or("text") }), types(), 150.))
            }
            _ => None,
        }
    }
    fn choose(&mut self, id: &str, v: Value) {
        let idx = |p: &str| id.strip_prefix(p).and_then(|n| n.parse::<usize>().ok());
        if self.editor.is_some() {
            match id {
                "cat" => {
                    let c = s(&v).to_string();
                    let fields = self.fields_for(&c);
                    if let Some(ed) = &mut self.editor {
                        // a new item takes the new category's fields while nothing was typed
                        let fresh = ed.entry.is_none() && !ed.fields.iter().any(|f| !s(&f["v"]).is_empty() && f["t"] != "bool" && f["choices"].is_null());
                        ed.cat = c;
                        if fresh {
                            ed.fields = fields;
                        }
                    }
                    return;
                }
                "ftype" | "rotate" => {
                    if let Some(ed) = &mut self.editor {
                        if id == "ftype" {
                            ed.new_type = s(&v).into();
                        } else {
                            ed.rotate = v.as_i64().unwrap_or(0);
                        }
                    }
                    return;
                }
                _ => {}
            }
            if let Some(f) = idx("fch:").and_then(|i| self.editor.as_mut()?.fields.get_mut(i)) {
                f["v"] = v;
                f["keep"] = json!(false);
                return;
            }
        }
        match id {
            "autolock" => self.set_meta(json!({"autolock":v}), true),
            "clip" => self.set_meta(json!({"clip":v}), false),
            _ => {
                if let Some(Sheet::Cat(c)) = &mut self.sheet {
                    match id {
                        "parent" => c.parent = s(&v).into(),
                        "template" => c.template = s(&v).into(),
                        _ => {
                            if let Some(f) = idx("cft:").and_then(|i| c.fields.get_mut(i)) {
                                let ty = s(&v).to_string();
                                f["t"] = json!(if ty == "multiline" { "text" } else { ty.as_str() });
                                f["multiline"] = json!(ty == "multiline");
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- drawing ----

    pub fn tree(&mut self, now: i64) -> Value {
        let theme = self.theme.clone();
        let t = Theme(&theme);
        if self.toast.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.toast = None;
        }
        let body = if !self.available || self.phase == Phase::Unavailable {
            ui::empty(&t, ui::g("lock", 40., &t.dim()), k("unavailable"), Value::Null, json!({"backgroundColor":t.bg()}))
        } else {
            match self.phase {
                Phase::None => lock::create(self, &t),
                Phase::Locked => lock::unlock(self, &t),
                Phase::Unlocked => main::window(self, &t, now),
                _ => ui::view(json!({"flex":1,"backgroundColor":t.bg()}), vec![]),
            }
        };
        let mut kids = vec![body];
        if let Some(id) = self.choice.clone() {
            match self.choice_spec(&id) {
                Some((value, opts, width)) => kids.extend(ui::choice_list(&t, &id, &value, &opts, width)),
                None => self.choice = None,
            }
        }
        if let Some((caption, _)) = &self.toast {
            kids.push(ui::toast(&t, caption.clone()));
        }
        ui::keyed("passwords", json!({"flex":1,"direction":if self.rtl {"rtl"} else {"ltr"}}), kids)
    }

    /// Timed work: the debounced search, the counts of the security report and the 2FA countdowns.
    fn timers(&mut self, now: i64) {
        if self.phase != Phase::Unlocked {
            return;
        }
        let len = self.entries.len();
        if self.win.issues_for != Some(len) {
            self.win.issues_for = Some(len);
            self.vault(Tag::Issues, "report", json!({}), &[], false);
        }
        if let Some(at) = self.win.search_at {
            if now >= at {
                self.win.search_at = None;
                if self.win.q.trim().is_empty() || self.win.sel == "login" {
                    self.win.hits = None;
                } else {
                    self.win.search_gen += 1;
                    let q = self.win.q.clone();
                    self.jobs.cancel(|t| matches!(t, Tag::Search(_)));
                    self.vault(Tag::Search(self.win.search_gen), "search", json!({"q":q}), &[], false);
                }
            }
        }
        if self.win.sel == "security" && self.security.for_len != Some(len) {
            self.security.for_len = Some(len);
            self.security.busy = true;
            self.vault(Tag::Report, "report", json!({}), &[], false);
        }
        if self.win.sel == "codes" {
            let el = now as f64 / 1000. - self.codes.at;
            let expired = self.codes.codes.iter().any(|c| c["period"].as_f64().unwrap_or(0.) > 0. && c["remaining"].as_f64().unwrap_or(0.) - el <= 0.);
            if !self.codes.loading && (self.codes.for_len != Some(len) || expired) {
                self.codes.for_len = Some(len);
                self.codes.loading = true;
                self.vault(Tag::Codes, "totp", json!({}), &[], false);
            }
        }
        // the open item's 2FA code: a new one when the period turns
        let reload = self.viewing.as_ref().is_some_and(|v| {
            let Some(c) = v.totp.first() else { return false };
            let period = c["period"].as_f64().unwrap_or(0.);
            if period <= 0. || v.totp_loading {
                return false;
            }
            let n = now as f64 / 1000.;
            let srv = v.totp_server + (n - v.totp_at);
            let left = period - (srv.floor() % period);
            left == period || left > c["remaining"].as_f64().unwrap_or(0.) + 1. + (n - v.totp_at)
        });
        if reload {
            self.load_totp();
        }
    }

    pub fn request_at(&mut self, input: &Value, now: i64) -> Value {
        let cmd_owned = s(&input["cmd"]).to_string();
        let cmd = cmd_owned.as_str();
        let arg = input["key"].clone();
        let a = s(&arg).to_string();
        let value = s(&input["value"]).to_string();
        if !matches!(cmd, "sync" | "next" | "icon" | "taFocus" | "pointerMove" | "dragEnd") {
            self.ta = None;
        }
        if !matches!(cmd, "sync" | "next" | "icon" | "choice" | "pick" | "pointerMove" | "dragEnd") {
            self.choice = None;
        }
        // the index after "name:" of a per-field command
        let at = |p: &str| cmd.strip_prefix(p).and_then(|n| n.parse::<usize>().ok());
        let idx = arg.as_u64().map(|n| n as usize);
        match cmd {
            "icon" => return ui::app_icon(input["size"].as_f64().filter(|n| n.is_finite()).unwrap_or(32.).clamp(1., 512.)),
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.fmt.borrow_mut().lang = self.i18n.lang.clone();
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"].as_f64().filter(|w| w.is_finite() && *w > 1.).unwrap_or(1040.);
                self.region = s(&input["args"]["region"]).into();
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction").or_else(|| self.i18n.entry(lang.split('-').next().unwrap_or(&lang), "meta", "direction"))
                    == Some(json!("rtl"));
                self.available = input["args"]["available"] != false;
                if !self.booted {
                    if let Some(owner) = input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0) {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.window = s(&input["args"]["__nativeWindow"]).into();
                        self.jobs.enqueue(Tag::Quiet, "vault.watch", json!([]));
                        self.refresh_status(false);
                    }
                }
                if self.booted && !self.events_pending && now >= self.events_at {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "vault.events", json!([]));
                }
                if self.booted {
                    self.menus();
                    self.timers(now);
                }
                self.sync_method();
                let tree = self.tree(now);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree})
                };
                if let Some(effect) = self.jobs.pump(now) {
                    reply["effect"] = effect;
                }
                return reply;
            }
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    self.apply(done.tag, done.result, now);
                }
            }

            // ---- lock screens ----
            "method" => {
                let ok = match self.phase {
                    Phase::Locked => lock::unlock_methods(&self.status).contains(&a.as_str()),
                    Phase::None => lock::CREATE_METHODS.contains(&a.as_str()),
                    _ => false,
                };
                if ok && !self.lock.busy {
                    self.lock.method = a.clone();
                    self.lock.err = Value::Null;
                }
            }
            "pw" => {
                self.lock.pw = value;
                if self.phase == Phase::None {
                    self.jobs.cancel(|t| *t == Tag::Strength);
                    if self.lock.pw.is_empty() {
                        self.lock.score = None;
                    } else {
                        let pw = self.lock.pw.clone();
                        self.vault(Tag::Strength, "strength", json!({"password":pw}), &[], false);
                    }
                }
            }
            "pw2" => self.lock.pw2 = value,
            "rec" => self.lock.rec = value,
            "pin" => self.lock.pin = value,
            "go" => self.go(),
            "pickKey" => {
                if !self.lock.busy && matches!(self.phase, Phase::Locked | Phase::None) {
                    let save = self.phase == Phase::None;
                    self.pick(PickFor::UnlockKey, save, if save { "contrasenas.key" } else { "" });
                }
            }
            "saveRecovery" => {
                let code = if a.is_empty() { self.lock.recovery.clone().unwrap_or_default() } else { a.clone() };
                if !code.is_empty() {
                    self.save_recovery(&code);
                }
            }
            "confirm" => self.lock.confirmed = !self.lock.confirmed,
            "done" => {
                if self.lock.confirmed && self.lock.recovery.is_some() {
                    self.lock = Lock::default();
                    self.refresh_status(false);
                }
            }
            _ if self.phase != Phase::Unlocked => {}

            // ---- the window ----
            "q" => {
                self.win.q = value;
                self.win.search_at = Some(now + 60);
            }
            "qClear" => {
                self.win.q.clear();
                self.win.search_at = Some(now);
            }
            "sel" => self.select(&a),
            "twisty" => {
                let open = self.win.open.get(&a).copied().unwrap_or(true);
                self.win.open.insert(a.clone(), !open);
            }
            "newCat" | "import" | "export" | "lock" | "generator" | "settings" | "new" => self.menu(cmd),
            "editCat" => {
                let c = self.cats.as_array().into_iter().flatten().find(|c| c["id"] == a.as_str()).cloned();
                if c.is_some() {
                    self.open_cat(c);
                }
            }
            "editTag" => {
                let color = self.tags.as_array().into_iter().flatten().find(|x| x["name"] == a.as_str()).and_then(|x| x["color"].as_str())
                    .or(self.catalog["colors"][0].as_str()).unwrap_or("#8e8e93").to_string();
                self.sheet = Some(Sheet::Tag(TagState { tag: a.clone(), name: a.clone(), color }));
            }
            "sort" => self.win.sort_title = !self.win.sort_title,
            "emptyTrash" => self.vault(Tag::EmptyTrash, "empty-trash", json!({}), &[], false),
            "open" => self.open_entry(&a),
            "openFrom" => {
                self.select("all");
                self.open_entry(&a);
            }

            // ---- an item ----
            "copyCode" => self.copy_secret(&a, k("totp.code")),
            "reveal" | "copyField" => self.field_action(cmd, &arg),
            "act" => {
                if let Some(v) = &self.viewing {
                    let op = s(&arg["op"]).to_string();
                    let mut args = json!({"v":v.v,"id":v.id});
                    if !arg["on"].is_null() {
                        args["on"] = arg["on"].clone();
                    }
                    let id = v.id.clone();
                    if ["fav", "archive", "trash", "restore", "purge"].contains(&op.as_str()) {
                        self.vault(Tag::Act { op: op.clone(), id }, &op, args, &[], false);
                    }
                }
            }
            "edit" => {
                if let Some((e, v)) = self.viewing.as_ref().and_then(|v| Some((v.e.clone()?, v.v.clone()))) {
                    self.edit_entry(&e, &v);
                }
            }
            "qr" => {
                if let Some(v) = &mut self.viewing {
                    if v.qr.take().is_none() {
                        let (vv, id) = (v.v.clone(), v.id.clone());
                        self.vault(Tag::Qr(id.clone()), "qr", json!({"v":vv,"id":id}), &[], false);
                    } else {
                        self.rasters.remove("qr");
                    }
                }
            }
            "attAdd" => self.pick(PickFor::AttAdd, false, ""),
            "attView" | "attSave" | "attDel" => self.att_action(cmd, &arg),
            "histToggle" => {
                if let Some(v) = &mut self.viewing {
                    v.hist_open = !v.hist_open;
                }
            }
            "histReveal" => {
                if let Some(v) = &self.viewing {
                    let (vv, id) = (v.v.clone(), v.id.clone());
                    self.vault(Tag::HistFull(id.clone()), "get", json!({"v":vv,"id":id,"hist":true,"reveal":true}), &[], false);
                }
            }

            // ---- the editor ----
            "title" => {
                if let Some(ed) = &mut self.editor {
                    ed.title = value;
                }
            }
            _ if at("fv:").is_some() || at("fl:").is_some() => {
                let mut pw_changed = false;
                if let Some(ed) = &mut self.editor {
                    let i = at("fv:").or(at("fl:")).unwrap_or(0);
                    if let Some(f) = ed.fields.get_mut(i) {
                        if cmd.starts_with("fv:") {
                            f["v"] = json!(value);
                            f["keep"] = json!(false);
                            pw_changed = f["k"] == "password";
                        } else {
                            f["l"] = json!(value);
                        }
                    }
                }
                if pw_changed {
                    self.ed_strength();
                }
            }
            "fbool" | "freplace" | "fremove" | "fshow" => {
                let i = idx.unwrap_or(usize::MAX);
                let removable = self.editor.as_ref().is_some_and(|ed| i < ed.fields.len() && self.field_removable(ed, i));
                if let Some(ed) = &mut self.editor {
                    if i < ed.fields.len() {
                        match cmd {
                            "fbool" => {
                                let f = &mut ed.fields[i];
                                f["v"] = json!(if f["v"] == "true" { "false" } else { "true" });
                            }
                            "freplace" => {
                                ed.fields[i]["v"] = json!("");
                                ed.fields[i]["keep"] = json!(false);
                            }
                            "fshow" => {
                                let on = ed.fields[i]["show"] != true;
                                ed.fields[i]["show"] = json!(on);
                            }
                            _ if removable => {
                                ed.fields.remove(i);
                            }
                            _ => {}
                        }
                    }
                }
                if cmd != "fshow" {
                    self.ed_strength();
                }
            }
            "fgen" => self.open_generator(idx),
            "fqr" => {
                if let Some(i) = idx {
                    self.pick(PickFor::QrScan(i), false, "");
                }
            }
            "addOpen" | "addCancel" => {
                if let Some(ed) = &mut self.editor {
                    ed.adding = cmd == "addOpen";
                }
            }
            "addField" => {
                if let Some(ed) = &mut self.editor {
                    let custom = |f: &Value| s(&f["k"]).strip_prefix('c').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
                    let mut n = ed.fields.iter().filter(|f| custom(f)).count() + 1;
                    while ed.fields.iter().any(|f| f["k"] == format!("c{n}").as_str()) {
                        n += 1;
                    }
                    let multi = ed.new_type == "multiline";
                    let ty = if multi { "text".to_string() } else { ed.new_type.clone() };
                    ed.fields.push(json!({"k":format!("c{n}"),"t":ty,"multiline":multi,"l":"","v":"","custom":true}));
                    ed.adding = false;
                }
            }
            "tagIn" => {
                if let Some(ed) = &mut self.editor {
                    ed.tag_in = value;
                }
            }
            "tagAdd" => {
                if let Some(ed) = &mut self.editor {
                    let tg = ed.tag_in.trim().trim_start_matches('#').to_string();
                    if !tg.is_empty() && !ed.tags.contains(&tg) {
                        ed.tags.push(tg);
                    }
                    ed.tag_in.clear();
                }
            }
            "tagDel" => {
                if let Some(ed) = &mut self.editor {
                    ed.tags.retain(|t| *t != a);
                }
            }
            "editCancel" => self.editor = None,
            "editSave" => self.save_editor(),
            "taFocus" => self.ta = Some(a.clone()),
            "choice" => self.choice = if self.choice.as_deref() == Some(a.as_str()) { None } else { Some(a.clone()) },
            "pick" => {
                self.choice = None;
                let id = s(&arg["id"]).to_string();
                self.choose(&id, arg["v"].clone());
            }
            "choiceClose" => self.choice = None,

            // ---- sheets ----
            "sheetClose" => self.sheet = None,
            "genMode" => self.gen_set("mode", json!(a)),
            "genOpt" => {
                let on = matches!(&self.sheet, Some(Sheet::Gen { o, .. }) if o[a.as_str()] == true);
                self.gen_set(&a, json!(!on));
            }
            "genWords" => self.gen_set("words", json!(arg.as_i64().unwrap_or(5).clamp(3, 12))),
            "genSep" => self.gen_set("sep", json!(value.chars().take(3).collect::<String>())),
            "genLen" => {
                let frame = &input["frame"];
                if let Some(Sheet::Gen { drag, .. }) = &mut self.sheet {
                    *drag = Some((frame[0].as_f64().unwrap_or(0.), frame[2].as_f64().unwrap_or(150.)));
                }
                self.gen_len_from(input["x"].as_f64().unwrap_or(0.));
                let mut out = self.pumped(now);
                out["capture"] = json!(true);
                return out;
            }
            "pointerMove" => self.gen_len_from(input["x"].as_f64().unwrap_or(0.)),
            "dragEnd" => {
                if let Some(Sheet::Gen { drag, .. }) = &mut self.sheet {
                    *drag = None;
                }
                let mut out = self.pumped(now);
                out["capture"] = json!(false);
                return out;
            }
            "genAgain" => self.generate(),
            "genCopy" | "genUse" => {
                if let Some(Sheet::Gen { o, res: Some(r), for_field, .. }) = &self.sheet {
                    let pw = s(&r["password"]).to_string();
                    let (o, for_field) = (o.clone(), *for_field);
                    if cmd == "genCopy" {
                        self.copy_secret(&pw, k("gen.password"));
                    } else if let Some(i) = for_field {
                        self.set_meta(json!({"gen":o}), false);
                        if let Some(f) = self.editor.as_mut().and_then(|ed| ed.fields.get_mut(i)) {
                            f["v"] = json!(pw);
                            f["keep"] = json!(false);
                        }
                        self.sheet = None;
                        self.ed_strength();
                    }
                }
            }
            "setTab" | "setAdd" | "setOld" | "setPw" | "setPw2" | "setPin" | "setReset" | "recConfirm" | "recClose" => {
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    match cmd {
                        "setTab" if a == "unlock" || a == "prefs" => st.tab = a.clone(),
                        "setAdd" if ["password", "keyfile", "passkey", "fido", "change"].contains(&a.as_str()) => st.add = Some(a.clone()),
                        "setOld" => st.old = value,
                        "setPw" => st.pw = value,
                        "setPw2" => st.pw2 = value,
                        "setPin" => st.pin = value,
                        "setReset" => {
                            st.add = None;
                            st.pw.clear();
                            st.pw2.clear();
                            st.old.clear();
                            st.pin.clear();
                            st.path = None;
                            st.err = Value::Null;
                        }
                        "recConfirm" => st.rec_ok = !st.rec_ok,
                        "recClose" => st.rec = None,
                        _ => {}
                    }
                }
            }
            "setPick" => self.pick(PickFor::SetKey, true, "contrasenas.key"),
            "setSubmit" => self.settings_submit(),
            "recNew" | "slotDel" | "calibrate" => {
                if let Some(Sheet::Settings(st)) = &mut self.sheet {
                    if !st.busy {
                        st.busy = true;
                        st.err = Value::Null;
                        match cmd {
                            "recNew" => self.vault(Tag::RecNew, "recovery-new", json!({}), &[], false),
                            "slotDel" => self.vault(Tag::SetErr { toast: Some(k("set.removed")), status: true }, "slot-del", json!({"slot":arg}), &[], false),
                            _ => self.vault(Tag::Calibrate, "calibrate", json!({"targetMs":700}), &[], false),
                        }
                    }
                }
            }
            "impPick" => self.pick(if a == "key" { PickFor::ImpKey } else { PickFor::ImpFile }, false, ""),
            "impPw" => {
                if let Some(Sheet::Import(st)) = &mut self.sheet {
                    st.pw = value;
                }
            }
            "impGo" => self.import(),
            "expKind" | "expOk" => {
                if let Some(Sheet::Export(st)) = &mut self.sheet {
                    if cmd == "expOk" {
                        st.ok = !st.ok;
                    } else if a == "vault" || a == "csv" {
                        st.kind = a.clone();
                        st.ok = false;
                    }
                }
            }
            "expGo" => {
                if let Some(Sheet::Export(st)) = &mut self.sheet {
                    if st.kind == "vault" || st.ok {
                        st.err = Value::Null;
                        let kind = st.kind.clone();
                        let suggest = if kind == "csv" { "contrasenas.csv" } else { "contrasenas-copia.lvault" };
                        self.pick(PickFor::Export(kind), true, suggest);
                    }
                }
            }
            "catName" | "catIcon" | "catColor" | "cfDel" | "cfAdd" => {
                if let Some(Sheet::Cat(c)) = &mut self.sheet {
                    match cmd {
                        "catName" => c.name = value,
                        "catIcon" if glyphs::GLYPH_KINDS.contains(&a.as_str()) => c.icon = a.clone(),
                        "catColor" => c.color = a.clone(),
                        "cfDel" => {
                            if let Some(i) = idx.filter(|i| *i < c.fields.len()) {
                                c.fields.remove(i);
                            }
                        }
                        "cfAdd" => {
                            let n = c.fields.len() + 1;
                            c.fields.push(json!({"k":format!("c{n}"),"t":"text","l":""}));
                        }
                        _ => {}
                    }
                }
            }
            _ if at("cfl:").is_some() => {
                if let Some(Sheet::Cat(c)) = &mut self.sheet {
                    if let Some(f) = at("cfl:").and_then(|i| c.fields.get_mut(i)) {
                        f["l"] = json!(value);
                    }
                }
            }
            "catSave" => self.cat_save(now),
            "catDelete" => self.cat_delete(),
            "tagName" | "tagColor" => {
                if let Some(Sheet::Tag(tg)) = &mut self.sheet {
                    if cmd == "tagName" {
                        tg.name = value;
                    } else {
                        tg.color = a.clone();
                    }
                }
            }
            "tagSave" | "tagDelete" => self.tag_save(cmd == "tagDelete"),
            "reportAgain" => {
                self.security.busy = true;
                self.vault(Tag::Report, "report", json!({}), &[], false);
            }
            "hibp" => self.pick(PickFor::Hibp, false, ""),
            _ => {}
        }
        self.pumped(now)
    }

    fn field_action(&mut self, cmd: &str, arg: &Value) {
        let (fk, i) = (s(&arg["k"]).to_string(), arg["i"].as_u64().unwrap_or(0) as usize);
        let Some(v) = &self.viewing else { return };
        let key = format!("{fk}#{i}");
        let Some(f) = v.e.as_ref().and_then(|e| e["f"].as_array()?.iter().filter(|f| f["k"] == fk.as_str()).nth(i).cloned()) else { return };
        let label = self.field_label(&f);
        let (vv, id, shown) = (v.v.clone(), v.id.clone(), v.shown.get(&key).cloned());
        let args = json!({"v":vv,"id":id,"k":fk,"i":i});
        if cmd == "reveal" {
            if shown.is_some() {
                if let Some(v) = &mut self.viewing {
                    v.shown.remove(&key);
                }
            } else {
                self.vault(Tag::Reveal { id, key, copy: None }, "reveal", args, &[], false);
            }
        } else if detail::hidden(&f) {
            match shown {
                Some(val) => self.copy_secret(&val, label),
                None => self.vault(Tag::Reveal { id, key, copy: Some(label) }, "reveal", args, &[], false),
            }
        } else {
            self.copy_text(s(&f["v"]), label);
        }
    }
    fn att_action(&mut self, cmd: &str, arg: &Value) {
        let Some(v) = &self.viewing else { return };
        let Some(att) = v.e.as_ref().and_then(|e| e["att"].as_array()?.iter().find(|x| x["id"] == *arg).cloned()) else { return };
        let (vv, id) = (v.v.clone(), v.id.clone());
        let name = s(&att["name"]).to_string();
        match cmd {
            "attView" if detail::is_image(&att) => self.vault(Tag::AttPreview(name), "att-preview", json!({"v":vv,"id":id,"att":arg,"max":520}), &[], false),
            "attView" if detail::is_text(&att) => self.vault(Tag::AttText(name), "att-text", json!({"v":vv,"id":id,"att":arg}), &[], false),
            "attView" | "attSave" => self.pick(PickFor::AttSave(arg.clone()), true, &name),
            _ => self.vault(Tag::AttOp("deleted"), "att-del", json!({"v":vv,"id":id,"att":arg}), &[], false),
        }
    }
    fn save_editor(&mut self) {
        let custom_label = self.t("field.custom");
        let Some(ed) = &mut self.editor else { return };
        if ed.busy {
            return;
        }
        ed.busy = true;
        ed.err = Value::Null;
        let f: Vec<Value> = ed
            .fields
            .iter()
            .filter(|f| f["keep"] == true || !s(&f["v"]).is_empty() || f["custom"] == true)
            .map(|f| {
                if f["keep"] == true {
                    let mut o = json!({"k":f["k"],"t":f["t"],"keep":true});
                    if !s(&f["l"]).is_empty() {
                        o["l"] = f["l"].clone();
                    }
                    o
                } else {
                    let ty = if f["multiline"] == true && !detail::hidden(f) { json!("multiline") } else { f["t"].clone() };
                    let mut o = json!({"k":f["k"],"t":ty,"v":s(&f["v"])});
                    if f["custom"] == true {
                        o["l"] = json!(if s(&f["l"]).is_empty() { custom_label.as_str() } else { s(&f["l"]) });
                    }
                    o
                }
            })
            .collect();
        let mut out = json!({"cat":ed.cat,"title":ed.title.trim(),"notes":ed.notes,"tags":ed.tags,"rotate":ed.rotate,"f":f});
        if let Some(e) = &ed.entry {
            out["id"] = e["id"].clone();
            out["fav"] = e["fav"].clone();
            out["arch"] = e["arch"].clone();
        }
        let v = ed.v.clone();
        self.vault(Tag::Save, "save", json!({"v":v,"entry":out}), &[], false);
    }
    fn import(&mut self) {
        let Some(Sheet::Import(st)) = &mut self.sheet else { return };
        let (Some(file), false) = (st.file.clone(), st.busy) else { return };
        st.busy = true;
        st.err = Value::Null;
        st.res = None;
        let name = file.name.rsplit('/').next().unwrap_or("").to_string();
        let pw = st.pw.clone();
        let files: Vec<Picked> = std::iter::once(file).chain(st.key.clone()).collect();
        let refs: Vec<&Picked> = files.iter().collect();
        if name.to_lowercase().ends_with(".lvault") {
            self.vault(Tag::Import, "import-vault", json!({"password":pw,"recovery":""}), &refs, false);
        } else {
            let mut args = json!({"name":name});
            if !pw.is_empty() {
                args["password"] = json!(pw);
            }
            self.vault(Tag::Import, "import", args, &refs, false);
        }
    }
    fn settings_submit(&mut self) {
        let Some(Sheet::Settings(st)) = &mut self.sheet else { return };
        let Some(add) = st.add.clone() else { return };
        if st.busy {
            return;
        }
        st.err = Value::Null;
        if ["password", "passkey", "change"].contains(&add.as_str()) && (st.pw.chars().count() < 8 || st.pw != st.pw2) {
            st.err = k(if st.pw.chars().count() < 8 { "create.short" } else { "create.mismatch" });
            return;
        }
        if add == "change" {
            st.busy = true;
            let args = json!({"old":st.old,"password":st.pw});
            self.vault(Tag::SetErr { toast: Some(k("set.changed")), status: true }, "password-change", args, &[], false);
            return;
        }
        let mut args = json!({"method":add,"label":""});
        if add == "password" || add == "passkey" {
            args["password"] = json!(st.pw);
        }
        let mut files = vec![];
        if add == "keyfile" || add == "passkey" {
            match st.path.take() {
                Some(p) => files.push(p),       // the descriptor goes with the request
                None => {
                    st.err = k("create.needKeyPath");
                    return;
                }
            }
            args["newkey"] = json!(true);
        }
        if add == "fido" && !st.pin.is_empty() {
            args["pin"] = json!(st.pin);
        }
        st.busy = true;
        let refs: Vec<&Picked> = files.iter().collect();
        self.vault(Tag::SetErr { toast: Some(k("set.added")), status: true }, "slot-add", args, &refs, add == "fido");
    }
    fn cat_save(&mut self, now: i64) {
        let Some(Sheet::Cat(c)) = &mut self.sheet else { return };
        if c.name.trim().is_empty() {
            c.err = k("cats.needName");
            return;
        }
        let id = c.orig.as_ref().and_then(|o| o["id"].as_str().map(String::from)).unwrap_or_else(|| format!("u{}", radix36(now.max(0) as u64)));
        let mut cat = json!({"id":id,"name":c.name.trim(),"icon":c.icon,"color":c.color,"template":c.template});
        if !c.parent.is_empty() {
            cat["parent"] = json!(c.parent);
        }
        if !c.fields.is_empty() {
            cat["fields"] = json!(c.fields);
        }
        let mut list: Vec<Value> = self.cats.as_array().into_iter().flatten().filter(|x| x["id"] != id.as_str()).cloned().collect();
        list.push(cat);
        self.cats = json!(list);
        self.vault(Tag::Step { toast: Some(k("cats.saved")), close: true, reload: false }, "set-cats", json!({"cats":list}), &[], false);
    }
    fn cat_delete(&mut self) {
        let Some(Sheet::Cat(c)) = &mut self.sheet else { return };
        let Some(id) = c.orig.as_ref().and_then(|o| o["id"].as_str().map(String::from)) else { return };
        if self.entries.iter().any(|e| e["cat"] == id.as_str()) {
            c.err = k("cats.inUse");
            return;
        }
        let list: Vec<Value> = self.cats.as_array().into_iter().flatten().filter(|x| x["id"] != id.as_str() && x["parent"] != id.as_str()).cloned().collect();
        self.cats = json!(list);
        self.vault(Tag::Step { toast: None, close: true, reload: false }, "set-cats", json!({"cats":list}), &[], false);
    }
    fn tag_save(&mut self, delete: bool) {
        let Some(Sheet::Tag(tg)) = &self.sheet else { return };
        let (tag, name, color) = (tg.tag.clone(), tg.name.trim().to_string(), tg.color.clone());
        let step = |close: bool| Tag::Step { toast: None, close, reload: close };
        let mut list: Vec<Value> =
            self.tags.as_array().into_iter().flatten().filter(|x| x["name"] != tag.as_str() && (delete || x["name"] != name.as_str())).cloned().collect();
        if delete {
            self.vault(step(false), "rename-tag", json!({"from":tag,"to":""}), &[], false);
        } else {
            list.push(json!({"name":if name.is_empty() { tag.as_str() } else { name.as_str() },"color":color}));
            if !name.is_empty() && name != tag {
                self.vault(step(false), "rename-tag", json!({"from":tag,"to":name}), &[], false);
            }
        }
        self.tags = json!(list);
        self.vault(step(true), "set-tags", json!({"tags":list}), &[], false);
    }
}

fn radix36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = vec![];
    loop {
        out.push(D[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}
