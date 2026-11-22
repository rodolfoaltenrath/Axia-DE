//! Calendar's native day/week/month windows and window-owned PIM controller.
mod model;
mod state;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{civil_date as date, date_format, i18n::I18n, NativeApp};
use model::*;
use serde_json::{json, Value};
pub struct App {
    state: state::State,
    i18n: I18n,
    theme: std::rc::Rc<Value>,
    region: String,
    hour: String,
    rtl: bool,
    width: f64,
    height: f64,
    side_month: i64,
    cursor_month: i64,
    picker: String,
    picker_month: i64,
    notes_since: i64,
    last_tree: Value,
    locale_revision: u64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            state: state::State::default(),
            i18n: I18n::default(),
            theme: std::rc::Rc::new(Value::Null),
            region: String::new(),
            hour: String::new(),
            rtl: false,
            width: 960.,
            height: 600.,
            side_month: 0,
            cursor_month: i64::MIN,
            picker: String::new(),
            picker_month: 0,
            notes_since: 0,
            last_tree: Value::Null,
            locale_revision: 0,
        }
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.theme = theme; }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn uses_preferences(&self) -> bool { true }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.request_with_preferences_at(v, preferences, crate::clock::now())
    }
    fn request(&mut self, v: &Value) -> Value {
        self.request_at(v, crate::clock::now())
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
    fn text_input(&self) -> bool {
        self.state.notes_focus
    }
    fn blur(&mut self) {
        self.state.notes_focus = false;
    }
    fn paste(&mut self, t: &str) -> Option<Value> {
        self.paste_at(t, crate::clock::now())
    }
    fn key(&mut self, k: &str, c: &str, flags: i32) -> Option<Value> {
        self.key_at(k, c, flags, crate::clock::now())
    }
}
impl App {
    fn tr(&mut self, k: &str) -> String {
        self.i18n.caption(&json!({"key":format!("calendar.{k}")}))
    }
    fn month_title(&mut self, d: i64) -> String {
        let tm = crate::clock::local(d);
        let month = self.tr(&format!("month.{}", tm.tm_mon));
        self.i18n.caption(
            &json!({"key":"calendar.monthYear","params":{"month":month,"year":tm.tm_year+1900}}),
        )
    }
    fn date(&mut self, d: i64, style: &str) -> String {
        date_format::date(&mut self.i18n, &self.region, &crate::clock::local(d), style)
    }
    fn time(&mut self, d: i64) -> String {
        date_format::time(
            &mut self.i18n,
            &self.region,
            &self.hour,
            &crate::clock::local(d),
            false,
        )
    }
    fn weekday(&mut self, wd: usize) -> String {
        let lang = self.i18n.lang.clone();
        self.i18n
            .entry(&lang, "format", "weekdays")
            .or_else(|| self.i18n.entry("en", "format", "weekdays"))
            .and_then(|v| v[wd].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
    fn show_sidebar(&self) -> bool {
        if self.width < 760. {
            self.state.narrow_sidebar
        } else {
            self.state.sidebar
        }
    }
    pub fn paste_at(&mut self, t: &str, now: i64) -> Option<Value> {
        if !self.state.notes_focus {
            return None;
        }
        let e = self.state.event.as_mut().filter(|e| !e.busy)?;
        e.draft["notes"] = json!(format!("{}{t}", s(&e.draft["notes"])));
        self.notes_since = now;
        Some(self.request_at(&json!({"cmd":"noop"}), now))
    }
    pub fn key_at(&mut self, k: &str, c: &str, f: i32, now: i64) -> Option<Value> {
        if self.state.notes_focus {
            if k == "Escape" {
                self.state.notes_focus = false;
            } else if f & (2 | 16) != 0 {
                if k.eq_ignore_ascii_case("v") {
                    return Some(self.request_at(&json!({"cmd":"notesPaste"}), now));
                }
                return None;
            } else if let Some(e) = self.state.event.as_mut().filter(|e| !e.busy) {
                let mut text = s(&e.draft["notes"]).to_owned();
                if k == "Backspace" {
                    text.pop();
                } else if k == "Enter" {
                    text.push('\n');
                } else if !c.is_empty() {
                    text.push_str(c);
                } else {
                    return None;
                }
                e.draft["notes"] = json!(text);
                self.notes_since = now;
            }
            return Some(self.request_at(&json!({"cmd":"noop"}), now));
        }
        if k == "Escape" {
            if !self.picker.is_empty() {
                self.picker.clear();
                return Some(self.request_at(&json!({"cmd":"noop"}), now));
            } else if !self.state.menu.is_empty() {
                return Some(self.request_at(&json!({"cmd":"menu","key":""}), now));
            } else if self.state.event.is_some() || self.state.calendar.is_some() {
                return Some(self.request_at(&json!({"cmd":"close"}), now));
            }
            return None;
        }
        if self.state.event.is_some() || self.state.calendar.is_some() {
            return None;
        }
        let cmd = if f & (2 | 16) != 0 {
            match k.to_lowercase().as_str() {
                "t" => json!({"cmd":"today"}),
                "n" => json!({"cmd":"new"}),
                "1" => json!({"cmd":"view","key":"day"}),
                "2" => json!({"cmd":"view","key":"week"}),
                "3" => json!({"cmd":"view","key":"month"}),
                _ => return None,
            }
        } else if k == "ArrowLeft" || k == "ArrowRight" {
            json!({"cmd":"go","key":if (k=="ArrowLeft")^self.rtl{-1}else{1}})
        } else {
            return None;
        };
        Some(self.request_at(&cmd, now))
    }
    pub fn request_at(&mut self, v: &Value, now: i64) -> Value {
        self.request_with_preferences_at(v, &v["settings"], now)
    }
    fn request_with_preferences_at(&mut self, v: &Value, preferences: &Value, now: i64) -> Value {
        let cmd = s(&v["cmd"]);
        if cmd == "icon" {
            return ui::icon(v["size"].as_f64().unwrap_or(32.), now);
        }
        if cmd == "sync" {
            let revision = v["context"]["localeRevision"].as_u64().unwrap_or(0);
            if revision != self.locale_revision {
                self.i18n.invalidate();
                self.last_tree = Value::Null;
                self.locale_revision = revision;
            }
            self.i18n.lang = v["lang"].as_str().unwrap_or("en").into();
            if v["context"]["theme"].is_object() {
                self.theme = std::rc::Rc::new(v["context"]["theme"].clone());
            }
            self.region = v["context"]["region"]
                .as_str()
                .unwrap_or(s(&preferences["region"]))
                .into();
            self.hour = s(&preferences["clock"]["hour24"]).into();
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
                .map(|d| d == "rtl")
                .unwrap_or_else(|| {
                    self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"))
                });
            self.state.default_name = self.tr("defaultName");
        }
        let mut input = v.clone();
        if v["key"]["field"].is_string() {
            input["key"] = v["key"]["field"].clone();
            if let Some(value) = v["key"].get("value") {
                input["value"] = value.clone();
            }
        }
        match cmd {
            "sidebar" => input["narrow"] = json!(self.width < 760.),
            "picker" => {
                self.picker = s(&v["key"]).into();
                self.state.notes_focus = false;
                if ["sDay", "eDay"].contains(&self.picker.as_str()) {
                    self.picker_month = date::month(
                        self.state
                            .event
                            .as_ref()
                            .map(|e| num(&e.draft[&self.picker]))
                            .unwrap_or(now),
                        0,
                    );
                }
            }
            "pick" => {
                input["cmd"] = json!("draft");
                self.picker.clear();
            }
            "closePicker" => self.picker.clear(),
            "month" => {
                let key = &v["key"];
                let value = if key["delta"] == "today" {
                    date::month(now, 0)
                } else {
                    date::month(
                        if key["where"] == "side" {
                            self.side_month
                        } else {
                            self.picker_month
                        },
                        num(&key["delta"]).clamp(-120, 120) as i32,
                    )
                };
                if key["where"] == "side" {
                    self.side_month = value;
                } else {
                    self.picker_month = value;
                }
            }
            "delete" => {
                if self
                    .state
                    .event
                    .as_ref()
                    .is_some_and(|e| recurring(&e.original, e.new))
                {
                    self.picker = "delete".into();
                } else {
                    input["cmd"] = json!("eventDelete");
                }
            }
            "eventDelete" => self.picker.clear(),
            "close" | "new" | "event" | "calNew" | "calEdit" => {
                self.picker.clear();
                self.state.notes_focus = false;
            }
            "menu" | "save" | "calSave" | "allDay" => self.state.notes_focus = false,
            "notesFocus" => self.notes_since = now,
            _ => {}
        }
        let mut out = self.state.request(&input, now);
        let month = date::month(self.state.cursor, 0);
        if month != self.cursor_month {
            self.cursor_month = month;
            self.side_month = month;
        }
        if self.state.event.is_none() {
            self.picker.clear();
        }
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
