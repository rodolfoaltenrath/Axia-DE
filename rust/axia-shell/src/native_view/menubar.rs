//! The menu bar's menus (logo, app name, the app's own menus, Window, Help) as a native view
//! mounted in the bar by the shell (`shell:menubar`). The shell sends the facts
//! `args.bar = {"items":[{"id","label","bold"} | {"id","glyph":"logo"}], "height"}` and the theme;
//! Rust lays the items out, lights them under the pointer, anchors the open menu below its item and
//! keeps the keyboard row. What stays with the shell is what it alone knows: the items of a menu
//! (built when it opens, with their callbacks) and running one. The two effects:
//! `{"kind":"barOpen","name"}` (answer: `{"open":{"name","items":[…]}}`, an item's `action` is
//! `["run", n]`) and `{"kind":"barRun","id":n}`.
use super::{menu::measure, NativeApp};
use serde_json::{json, Value};

const ITEM_H: f64 = 22.;
const PAD: f64 = 9.;
const FONT: f64 = 13.;
const GLYPH: f64 = 16.;
const MENU_W: f64 = 260.;
const GAP: f64 = 2.;

struct Open {
    name: String,
    items: Vec<Value>,
    x: f64,
}
#[derive(Default)]
pub struct MenuBar {
    open: Option<Open>,
    /// The name asked of the shell whose items have not come back.
    asked: Option<(String, f64)>,
    /// The row under the pointer or the keyboard, and the row whose submenu is open.
    row: i64,
    sub: i64,
    sub_row: i64,
    last: Value,
}

fn selectable(items: &[Value], i: i64) -> bool {
    usize::try_from(i)
        .ok()
        .and_then(|i| items.get(i))
        .is_some_and(|it| it.is_object() && it["header"].is_null() && it["disabled"] != true)
}
fn advance(items: &[Value], from: i64, dir: i64) -> Option<i64> {
    let n = items.len() as i64;
    (1..=n).map(|s| (from + dir * s).rem_euclid(n)).find(|i| selectable(items, *i))
}
/// Light ink (a dark bar) takes the light glyph, as `AxiaGlyph` does.
fn light(color: &str) -> bool {
    let hex = color.trim().trim_start_matches('#');
    if hex.len() == 6 {
        if let Ok(n) = u32::from_str_radix(hex, 16) {
            let (r, g, b) = ((n >> 16) as f64, ((n >> 8) & 255) as f64, (n & 255) as f64);
            return 0.299 * r + 0.587 * g + 0.114 * b > 140.;
        }
    }
    let c = color.to_ascii_lowercase();
    c.contains("white") || c.starts_with("rgb(2") || c.starts_with("rgba(2")
}

impl MenuBar {
    fn close(&mut self) {
        self.open = None;
        self.asked = None;
        self.row = -1;
        self.sub = -1;
        self.sub_row = -1;
    }
    fn width(item: &Value) -> f64 {
        let inner = if item["glyph"].is_string() {
            GLYPH
        } else {
            measure(item["label"].as_str().unwrap_or(""), FONT, if item["bold"] == true { 700. } else { 500. })
        };
        inner + 2. * PAD
    }
    /// The items' left edges (the window's own coordinates) and the width the bar uses.
    fn places(items: &[Value], width: f64, rtl: bool) -> Vec<(f64, f64)> {
        let mut used = 0.;
        items
            .iter()
            .map(|it| {
                let w = Self::width(it);
                let left = if rtl { width - used - w } else { used };
                used += w;
                (left, w)
            })
            .collect()
    }
    fn tree(&self, input: &Value) -> Value {
        let theme = &input["context"]["theme"];
        let ink = theme["menuText"].as_str().unwrap_or("#ffffff");
        let hover = theme["menuHover"].clone();
        let bar = &input["args"]["bar"];
        let items = bar["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let width = input["context"]["width"].as_f64().unwrap_or(0.);
        let height = input["context"]["height"].as_f64().filter(|h| *h > 1.).or(bar["height"].as_f64()).unwrap_or(ITEM_H);
        let rtl = matches!(input["lang"].as_str().unwrap_or(""), "ar" | "he" | "fa" | "ur");
        let top = ((height - ITEM_H) / 2.).max(0.);
        let tone = if light(ink) { "light" } else { "dark" };
        let mut children: Vec<Value> = Self::places(items, width, rtl)
            .into_iter()
            .zip(items)
            .map(|((left, w), item)| {
                let id = item["id"].as_str().unwrap_or("");
                let active = self.open.as_ref().map(|o| o.name.as_str()) == Some(id)
                    || self.asked.as_ref().map(|a| a.0.as_str()) == Some(id);
                let content = if item["glyph"].is_string() {
                    json!({"kind":"image","src":format!("asset:icons/axia-glyph-{tone}-16.png"),"style":{"width":GLYPH,"height":GLYPH}})
                } else {
                    json!({"kind":"text","label":item["label"],
                        "style":{"fontSize":FONT,"fontWeight":if item["bold"]==true {700} else {500},"color":ink,"whiteSpace":"nowrap"}})
                };
                json!({"kind":"view","key":id,
                    "style":{"position":"absolute","left":left,"top":top,"width":w,"height":ITEM_H,"borderRadius":5,
                        "justifyContent":"center","alignItems":"center",
                        "backgroundColor":if active {hover.clone()} else {json!("transparent")}},
                    "hoverStyle":{"backgroundColor":hover},
                    "events":{"onClick":["bar",id]},
                    "children":[content]})
            })
            .collect();
        if let Some(open) = &self.open {
            let x = if rtl { (open.x - MENU_W).max(0.) } else { open.x };
            children.push(json!({"kind":"menu","key":"menu","x":x,"y":height + GAP,"width":MENU_W,"layer":"popover",
                "items":open.items,"active":self.row,"sub":self.sub,"subActive":self.sub_row,
                "events":{"onDismiss":["dismiss"]}}));
        }
        json!({"kind":"view","key":"menubar","style":{"position":"absolute","left":0,"top":0,"width":width,"height":height},"children":children})
    }
    fn items(&self) -> &[Value] {
        self.open.as_ref().map(|o| o.items.as_slice()).unwrap_or(&[])
    }
    fn run(&mut self, id: &Value) -> Value {
        self.close();
        json!({"effects":[{"kind":"barRun","id":id}]})
    }
}

impl NativeApp for MenuBar {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                let tree = self.tree(input);
                if tree == self.last {
                    return json!({"same":true});
                }
                self.last = tree.clone();
                json!({"tree":tree})
            }
            "bar" => {
                let name = input["key"].as_str().unwrap_or("").to_owned();
                if self.open.as_ref().map(|o| o.name.as_str()) == Some(name.as_str()) {
                    self.close();
                    return json!({});
                }
                let bar = &input["args"]["bar"];
                let items = bar["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                let width = input["context"]["width"].as_f64().unwrap_or(0.);
                let rtl = matches!(input["lang"].as_str().unwrap_or(""), "ar" | "he" | "fa" | "ur");
                let place = Self::places(items, width, rtl)
                    .into_iter()
                    .zip(items)
                    .find(|(_, it)| it["id"].as_str() == Some(name.as_str()))
                    .map(|((left, w), _)| if rtl { left + w } else { left })
                    .unwrap_or(0.);
                self.close();
                self.asked = Some((name.clone(), place));
                json!({"effects":[{"kind":"barOpen","name":name}]})
            }
            // the shell's answer to barOpen
            "next" => {
                let open = &input["result"]["open"];
                if let (Some((name, x)), Some(items)) = (self.asked.take(), open["items"].as_array()) {
                    if open["name"].as_str() == Some(name.as_str()) {
                        self.open = Some(Open { name, items: items.clone(), x });
                        self.row = -1;
                        self.sub = -1;
                        self.sub_row = -1;
                    }
                }
                json!({})
            }
            "dismiss" => {
                self.close();
                json!({})
            }
            "run" => self.run(&input["key"]),
            "menuHover" => {
                let i = input["key"].as_i64().unwrap_or(-1);
                let has_sub = usize::try_from(i).ok().and_then(|i| self.items().get(i)).is_some_and(|it| it["submenu"].is_array());
                let row = if selectable(self.items(), i) { i } else { -1 };
                let sub = if has_sub { i } else { -1 };
                if (row, sub) != (self.row, self.sub) {
                    self.row = row;
                    self.sub = sub;
                    self.sub_row = -1;
                }
                json!({})
            }
            "menuSubHover" => {
                let i = input["key"].as_i64().unwrap_or(-1);
                let sub = usize::try_from(self.sub).ok().and_then(|s| self.items().get(s)).and_then(|it| it["submenu"].as_array());
                self.sub_row = if sub.is_some_and(|s| selectable(s, i)) { i } else { -1 };
                json!({})
            }
            _ => Value::Null,
        }
    }
    /// Arrows, Enter and Escape while a menu is open; None otherwise (the shell keeps its keys).
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        self.open.as_ref()?;
        let in_sub = self.sub >= 0;
        let list: Vec<Value> = if in_sub {
            self.items().get(self.sub as usize).and_then(|it| it["submenu"].as_array()).cloned().unwrap_or_default()
        } else {
            self.items().to_vec()
        };
        let active = if in_sub { self.sub_row } else { self.row };
        match key {
            "ArrowDown" | "ArrowUp" | "Home" | "End" => {
                let dir = if matches!(key, "ArrowUp" | "End") { -1 } else { 1 };
                let from = if matches!(key, "Home" | "End") || active < 0 { if dir > 0 { -1 } else { 0 } } else { active };
                if let Some(next) = advance(&list, from, dir) {
                    if in_sub {
                        self.sub_row = next;
                    } else {
                        self.row = next;
                        self.sub = -1;
                    }
                }
            }
            "ArrowRight" if !in_sub => {
                if let Some(sub) = usize::try_from(self.row).ok().and_then(|i| list.get(i)).and_then(|it| it["submenu"].as_array()) {
                    self.sub = self.row;
                    self.sub_row = advance(sub, -1, 1).unwrap_or(-1);
                }
            }
            "ArrowLeft" if in_sub => {
                self.sub = -1;
                self.sub_row = -1;
            }
            "ArrowLeft" => {}
            "Escape" => self.close(),
            "Enter" | " " | "Space" => {
                if selectable(&list, active) {
                    let item = list[active as usize].clone();
                    if !in_sub && item["submenu"].is_array() {
                        self.sub = self.row;
                        self.sub_row = advance(item["submenu"].as_array().unwrap(), -1, 1).unwrap_or(-1);
                    } else if item["action"][0] == "run" {
                        return Some(self.run(&item["action"][1]));
                    }
                }
            }
            _ => return None,
        }
        Some(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(cmd: &str, key: Value, lang: &str) -> Value {
        json!({"cmd":cmd,"key":key,"lang":lang,
            "context":{"width":800.,"height":22.,"theme":{"menuText":"#ffffff","menuHover":"rgba(255,255,255,0.2)"}},
            "args":{"bar":{"height":22,"items":[{"id":"logo","glyph":"logo"},{"id":"app","label":"Files","bold":true},{"id":"window","label":"Window"}]}}})
    }
    fn menu(items: Value) -> Value {
        json!({"open":{"name":"app","items":items}})
    }
    fn two() -> Value {
        json!([{"label":"About","action":["run",1]},"-",{"label":"Recent","submenu":[{"label":"a","action":["run",3]}]},{"label":"Quit","action":["run",4]}])
    }
    fn opened(app: &mut MenuBar) {
        app.request(&input("bar", json!("app"), "en"));
        app.request(&json!({"cmd":"next","result":menu(two())}));
    }

    #[test]
    fn lays_the_items_out_from_the_left_or_the_right() {
        let ltr = MenuBar::places(input("sync", Value::Null, "en")["args"]["bar"]["items"].as_array().unwrap(), 800., false);
        assert_eq!(ltr[0], (0., 34.));
        assert_eq!(ltr[1].0, 34.);
        let rtl = MenuBar::places(input("sync", Value::Null, "ar")["args"]["bar"]["items"].as_array().unwrap(), 800., true);
        assert_eq!(rtl[0], (766., 34.));
        assert!(rtl[1].0 + rtl[1].1 <= 766.);
    }
    #[test]
    fn syncs_the_tree_once_and_reports_no_change_after() {
        let mut app = MenuBar::default();
        let first = app.request(&input("sync", Value::Null, "en"));
        let items = first["tree"]["children"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["children"][0]["kind"], "image");
        assert_eq!(items[0]["children"][0]["src"], "asset:icons/axia-glyph-light-16.png");
        assert_eq!(items[1]["children"][0]["style"]["fontWeight"], 700);
        assert_eq!(app.request(&input("sync", Value::Null, "en"))["same"], true);
    }
    #[test]
    fn opening_asks_the_shell_then_hangs_the_menu_below_the_item() {
        let mut app = MenuBar::default();
        let reply = app.request(&input("bar", json!("app"), "en"));
        assert_eq!(reply["effects"][0], json!({"kind":"barOpen","name":"app"}));
        let pending = app.request(&input("sync", Value::Null, "en"));
        assert!(pending["tree"]["children"].as_array().unwrap().iter().all(|c| c["kind"] != "menu"), "no menu before its items");
        app.request(&json!({"cmd":"next","result":menu(two())}));
        let tree = app.request(&input("sync", Value::Null, "en"))["tree"].clone();
        let menu = tree["children"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(menu["kind"], "menu");
        assert_eq!(menu["x"], 34.);
        assert_eq!(menu["y"], 24.);
        assert_eq!(menu["layer"], "popover");
        let active = &tree["children"][1]["style"]["backgroundColor"];
        assert_eq!(active, "rgba(255,255,255,0.2)", "the open item stays lit");
    }
    #[test]
    fn a_second_click_on_the_same_item_closes_and_another_item_switches() {
        let mut app = MenuBar::default();
        opened(&mut app);
        assert_eq!(app.request(&input("bar", json!("app"), "en")), json!({}));
        assert!(app.open.is_none());
        opened(&mut app);
        let reply = app.request(&input("bar", json!("window"), "en"));
        assert_eq!(reply["effects"][0]["name"], "window");
        assert!(app.open.is_none());
    }
    #[test]
    fn an_answer_for_a_menu_no_longer_asked_is_ignored() {
        let mut app = MenuBar::default();
        app.request(&input("bar", json!("window"), "en"));
        app.request(&json!({"cmd":"next","result":menu(two())}));
        assert!(app.open.is_none());
    }
    #[test]
    fn running_an_item_closes_the_menu_and_tells_the_shell() {
        let mut app = MenuBar::default();
        opened(&mut app);
        let reply = app.request(&input("run", json!(4), "en"));
        assert_eq!(reply["effects"][0], json!({"kind":"barRun","id":4}));
        assert!(app.open.is_none());
        opened(&mut app);
        app.request(&input("dismiss", Value::Null, "en"));
        assert!(app.open.is_none());
    }
    #[test]
    fn the_keyboard_moves_over_rows_and_submenus() {
        let mut app = MenuBar::default();
        assert!(app.key("ArrowDown", "", 0).is_none(), "closed: the shell keeps its keys");
        opened(&mut app);
        app.key("ArrowDown", "", 0);
        assert_eq!(app.row, 0);
        app.key("ArrowDown", "", 0);
        assert_eq!(app.row, 2, "the separator is skipped");
        app.key("ArrowRight", "", 0);
        assert_eq!((app.sub, app.sub_row), (2, 0));
        app.key("ArrowLeft", "", 0);
        assert_eq!(app.sub, -1);
        app.key("ArrowDown", "", 0);
        assert_eq!(app.row, 3);
        let run = app.key("Enter", "", 0).unwrap();
        assert_eq!(run["effects"][0]["id"], 4);
        assert!(app.open.is_none());
        opened(&mut app);
        assert_eq!(app.key("Escape", "", 0), Some(json!({})));
        assert!(app.open.is_none());
        assert!(app.key("a", "a", 0).is_none(), "closed again: the shell keeps its keys");
    }
    #[test]
    fn hover_tracks_the_row_and_its_submenu() {
        let mut app = MenuBar::default();
        opened(&mut app);
        app.request(&input("menuHover", json!(2), "en"));
        assert_eq!((app.row, app.sub), (2, 2));
        app.request(&input("menuSubHover", json!(0), "en"));
        assert_eq!(app.sub_row, 0);
        app.request(&input("menuHover", json!(1), "en"));
        assert_eq!((app.row, app.sub), (-1, -1), "a separator lights nothing");
    }
    #[test]
    fn the_glyph_follows_the_ink() {
        assert!(light("#ffffff") && light("rgba(255,255,255,0.9)"));
        assert!(!light("#1d1d1f") && !light("rgba(0,0,0,0.8)"));
    }
}
