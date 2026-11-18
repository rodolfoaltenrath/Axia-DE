//! Native Contacts: window, editor, list, groups and PIM scheduling in Rust.
use crate::native_view::civil_date as date;
mod model;
mod state;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{date_format, i18n::I18n, NativeApp};
use model::*;
use serde_json::{json, Value};

pub struct App {
    state: state::State,
    i18n: I18n,
    theme: Value,
    region: String,
    rtl: bool,
    width: f64,
    height: f64,
    offset: f64,
    drag: Option<(f64, f64)>,
    picker: Value,
    picker_month: i64,
    notes_since: i64,
    selection: (String, String, String),
    last_tree: Value,
    locale_revision: u64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            state: state::State::default(),
            i18n: I18n::default(),
            theme: Value::Null,
            region: String::new(),
            rtl: false,
            width: 880.,
            height: 560.,
            offset: 0.,
            drag: None,
            picker: Value::Null,
            picker_month: 0,
            notes_since: 0,
            selection: Default::default(),
            last_tree: Value::Null,
            locale_revision: 0,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.state
            .booted
            .then_some(if self.state.busy() || self.state.notes_focus {
                50.
            } else {
                200.
            })
    }
    fn request(&mut self, v: &Value) -> Value {
        self.request_at(v, crate::clock::now())
    }
    fn text_input(&self) -> bool {
        self.state.notes_focus
    }
    fn blur(&mut self) {
        self.state.notes_focus = false;
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        self.paste_at(text, crate::clock::now())
    }
    fn key(&mut self, k: &str, c: &str, flags: i32) -> Option<Value> {
        self.key_at(k, c, flags, crate::clock::now())
    }
}
impl App {
    fn tr(&mut self, k: &str) -> String {
        self.i18n.caption(&json!({"key":format!("contacts.{k}")}))
    }
    fn birthday(&mut self, s: &str, editing: bool) -> String {
        let Some(ms) = date::parse(s) else {
            return s.into();
        };
        let tm = crate::clock::local(ms);
        if s.starts_with("--") && !editing {
            let month = self
                .i18n
                .caption(&json!({"key":format!("calendar.month.{}",tm.tm_mon)}));
            self.i18n.caption(
                &json!({"key":"contacts.dayMonth","params":{"day":tm.tm_mday,"month":month}}),
            )
        } else {
            date_format::date(
                &mut self.i18n,
                &self.region,
                &tm,
                if editing { "short" } else { "long" },
            )
        }
    }
    pub fn paste_at(&mut self, text: &str, now: i64) -> Option<Value> {
        if !self.state.notes_focus {
            return None;
        }
        let d = self.state.draft.as_mut().filter(|d| !d.busy)?;
        d.value["notes"] = json!(format!("{}{text}", string(&d.value["notes"])));
        self.notes_since = now;
        Some(self.request_at(&json!({"cmd":"noop"}), now))
    }
    pub fn key_at(&mut self, k: &str, c: &str, flags: i32, now: i64) -> Option<Value> {
        if self.state.notes_focus {
            if k == "Escape" {
                self.state.notes_focus = false;
            } else if flags & (2 | 16) != 0 {
                if k.eq_ignore_ascii_case("v") {
                    return Some(self.request_at(&json!({"cmd":"notesPaste"}), now));
                }
                return None;
            } else if let Some(d) = self.state.draft.as_mut().filter(|d| !d.busy) {
                let mut s = string(&d.value["notes"]).to_owned();
                if k == "Backspace" {
                    s.pop();
                } else if k == "Enter" {
                    s.push('\n');
                } else if !c.is_empty() {
                    s.push_str(c);
                } else {
                    return None;
                }
                d.value["notes"] = json!(s);
                self.notes_since = now;
            }
            return Some(self.request_at(&json!({"cmd":"noop"}), now));
        }
        if k == "Escape" {
            if !self.picker.is_null() {
                self.picker = Value::Null;
            } else if !self.state.menu.is_empty() {
                return Some(self.request_at(&json!({"cmd":"menu","key":""}), now));
            } else if !self.state.sheet.is_null() {
                return Some(self.request_at(&json!({"cmd":"closeSheet"}), now));
            } else {
                return None;
            }
            return Some(self.request_at(&json!({"cmd":"noop"}), now));
        }
        if self.state.draft.is_some() || !self.state.sheet.is_null() {
            return None;
        }
        if flags & (2 | 16) != 0 && k.eq_ignore_ascii_case("n") {
            return Some(self.request_at(&json!({"cmd":"new"}), now));
        }
        if k == "ArrowDown" || k == "ArrowUp" {
            let list = visible(&self.state.contacts, &self.state.source, &self.state.query);
            let i = list
                .iter()
                .position(|c| key(c) == self.state.selected)
                .map(|i| i as isize)
                .unwrap_or(-1);
            let j = (i + if k == "ArrowDown" { 1 } else { -1 }).max(0) as usize;
            let c = list.get(j.min(list.len().saturating_sub(1)))?;
            return Some(self.request_at(&json!({"cmd":"select","key":key(c)}), now));
        }
        None
    }
    pub fn request_at(&mut self, v: &Value, now: i64) -> Value {
        let cmd = string(&v["cmd"]);
        if cmd == "icon" {
            return ui::icon(v["size"].as_f64().unwrap_or(32.));
        }
        if cmd == "sync" {
            let revision = v["context"]["localeRevision"].as_u64().unwrap_or(0);
            if revision != self.locale_revision {
                self.i18n.invalidate();
                self.last_tree = Value::Null;
                self.locale_revision = revision;
            }
            self.i18n.lang = v["lang"].as_str().unwrap_or("en").into();
            self.theme = v["context"]["theme"].clone();
            self.region = v["context"]["region"]
                .as_str()
                .unwrap_or(string(&v["settings"]["region"]))
                .into();
            self.width = v["context"]["width"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 1.)
                .unwrap_or(self.width);
            self.height = v["context"]["height"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 1.)
                .unwrap_or(self.height);
            let lang = self.i18n.lang.clone();
            self.rtl = v["context"]["direction"]
                .as_str()
                .map(|s| s == "rtl")
                .unwrap_or_else(|| {
                    self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"))
                });
            self.state.default_book = self.tr("defaultBook");
            let regional = if self.region.is_empty() {
                self.i18n.lang.clone()
            } else {
                self.region.split('_').next().unwrap_or("en").to_owned()
            };
            let regions = self
                .i18n
                .entry(&regional, "format", "regions")
                .unwrap_or(Value::Null);
            self.i18n.decimal = regions[&self.region]["decimal"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    self.i18n
                        .entry(&regional, "format", "decimal")
                        .and_then(|s| s.as_str().map(str::to_owned))
                })
                .unwrap_or(".".into());
            self.i18n.group = regions[&self.region]["group"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    self.i18n
                        .entry(&regional, "format", "group")
                        .and_then(|s| s.as_str().map(str::to_owned))
                })
                .unwrap_or(",".into());
        }
        let mut input = v.clone();
        if v["key"].is_object() && v["key"]["kind"].is_string() {
            input["key"] = v["key"]["kind"].clone();
            input["index"] = v["key"]["index"].clone();
            input["field"] = v["key"]["field"].clone();
            if v["key"].get("value").is_some() {
                input["value"] = v["key"]["value"].clone();
            }
        }
        match cmd {
            "clearQuery" => input = json!({"cmd":"query","value":""}),
            "wheel" => {
                self.offset += v["deltaY"].as_f64().unwrap_or(0.);
            }
            "gesture" if v["kind"] == "scroll" && v["phase"] != "begin" => {
                self.offset += v["dy"].as_f64().unwrap_or(0.);
            }
            "scrollDrag" => self.drag = Some((v["y"].as_f64().unwrap_or(0.), self.offset)),
            "pointerMove" => {
                if let Some((y, off)) = self.drag {
                    let list = visible(&self.state.contacts, &self.state.source, &self.state.query);
                    let total = ListLayout::new(&list).height;
                    self.offset = off
                        + (v["y"].as_f64().unwrap_or(y) - y) * total / (self.height - 92.).max(1.);
                }
            }
            "dragEnd" => self.drag = None,
            "typeMenu" => {
                self.picker = v["key"].clone();
                self.state.notes_focus = false;
            }
            "row" => self.picker = Value::Null,
            "closePicker" => self.picker = Value::Null,
            "birthdayAdd" => {
                input = json!({"cmd":"field","key":"bday","value":"1990-01-01"});
            }
            "birthdayRemove" => {
                input = json!({"cmd":"field","key":"bday","value":""});
                self.picker = Value::Null;
            }
            "birthdayPicker" => {
                if let Some(d) = &self.state.draft {
                    self.picker = json!("birthday");
                    self.picker_month =
                        date::month(date::parse(string(&d.value["bday"])).unwrap_or(now), 0);
                }
            }
            "month" => {
                self.picker_month = if v["key"] == "today" {
                    date::month(now, 0)
                } else {
                    date::month(self.picker_month, string(&v["key"]).parse().unwrap_or(0))
                };
            }
            "pickDay" => {
                if let Some(ms) = v["key"].as_i64() {
                    input = json!({"cmd":"field","key":"bday","value":date::ymd(ms)});
                }
                self.picker = Value::Null;
            }
            "email" => {
                let i = v["key"].as_u64().unwrap_or(u64::MAX) as usize;
                if let Some(c) = self.state.selected_contact() {
                    if let Some(email) = c["emails"].get(i).and_then(|e| e["value"].as_str()) {
                        return self.state.open_mail(display_name(c), email.to_owned(), now);
                    }
                }
            }
            "notesFocus" => self.notes_since = now,
            "photoPick" | "photoRemove" => self.state.menu.clear(),
            "new" | "edit" | "source" | "select" | "cancel" => {
                self.picker = Value::Null;
                self.state.notes_focus = false;
            }
            "menu" | "save" | "groupNew" | "delete" => self.state.notes_focus = false,
            _ => {}
        }
        let mut out = self.state.request(&input, now);
        let list = visible(&self.state.contacts, &self.state.source, &self.state.query);
        let layout = ListLayout::new(&list);
        let h = (self.height - 92.).max(1.);
        if !self.offset.is_finite() {
            self.offset = 0.;
        }
        self.offset = self.offset.clamp(0., (layout.height - h).max(0.));
        let selection = (
            self.state.source.clone(),
            self.state.query.clone(),
            self.state.selected.clone(),
        );
        if selection != self.selection {
            if selection.0 != self.selection.0 || selection.1 != self.selection.1 {
                self.offset = 0.;
            }
            if let Some(i) = list.iter().position(|c| key(c) == self.state.selected) {
                self.offset = layout.reveal(i, self.offset, h);
            }
            self.selection = selection;
        }
        out["capture"] = json!(self.drag.is_some());
        if cmd == "sync" {
            let tree = ui::tree(self, now);
            if tree != self.last_tree {
                self.last_tree = tree.clone();
                out["tree"] = tree;
            } else {
                out["same"] = json!(true);
            }
        }
        out
    }
}
