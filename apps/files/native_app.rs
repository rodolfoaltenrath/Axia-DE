//! Files as a native-view app: a session of the Files service driven by the host, with no React in
//! between. `native.rs` translates the widget tree; this is the conversation.
//!
//! - `sync` creates the window's session the first time (with the window's arguments, the settings of
//!   Files, the theme and the installed apps) and polls it after: finished listings, jobs, the clock.
//! - Any other command is an action of the interface (`["select", 3]`, `["open"]`…): it goes to the
//!   session as an `event` with what the pointer or the field said (position, modifiers, value).
//! - The context menu's keyboard row and open submenu are kept here (`menuHover`, the arrows, Enter):
//!   the React `Menu` kept them in its own state.
//! - Effects (opening a file, mounting a disk…) go out to the shell as they are, with the menus and
//!   the frame the window shows, for the shell's glue to carry out.
use super::{
    ffi,
    native::{convert, Env},
};
use crate::native_view::NativeApp;
use serde_json::{json, Map, Value};

#[derive(Default)]
pub struct FilesView {
    /// The session's id; 0 before the first sync.
    id: u64,
    /// The last tree of the session as Files wrote it, to translate again when only the menu changed.
    raw: Value,
    /// A tree the host has not been given yet.
    fresh: bool,
    /// The context menu's keyboard row and open submenu; -1 for none.
    menu: (i64, i64),
    sub_active:i64,
    /// The items of the context menu being shown, for the keyboard.
    menu_items: Vec<Value>,
    menus: Value,
    chrome: Value,
    /// A listing or a job is being worked: poll faster.
    loading: bool,
    title: Value,
    capture: bool,
    catalog: Value,
    resources: super::native_resources::Resources,
}

fn apps(input: &Value) -> Value {
    Value::Array(
        input["catalog"]
            .as_array()
            .into_iter()
            .flatten()
            .take(512)
            .map(|a| {
                let desktop = a["desktopId"].as_str().filter(|d| !d.is_empty());
                json!({"id":a["id"],"name":a["name"],"kind":a["kind"],
                    "desktopId":match desktop { Some(d) if d.ends_with(".desktop") => d.to_owned(), Some(d) => format!("{d}.desktop"),
                        None => format!("lordos-{}.desktop", a["id"].as_str().unwrap_or("")) },
                    "mimeTypes":match &a["mimeTypes"] { Value::Array(l) => Value::Array(l.clone()),
                        Value::String(s) => Value::Array(s.split(';').filter(|m| !m.is_empty()).map(|m| json!(m)).collect()), _ => json!([]) }})
            })
            .collect(),
    )
}

fn selectable(items:&[Value],index:i64)->bool {
    usize::try_from(index).ok().and_then(|i|items.get(i)).is_some_and(|it|it.is_object() && it["header"].is_null() && it["disabled"]!=true)
}
fn advance(items:&[Value],from:i64,direction:i64)->Option<i64> {
    let count=items.len() as i64;
    (1..=count).map(|step|(from+direction*step).rem_euclid(count)).find(|i|selectable(items,*i))
}

impl FilesView {
    fn theme(input: &Value) -> Value {
        let mut theme = input["context"]["theme"].clone();
        if !theme.is_object() {
            theme = json!({});
        }
        theme["nativeAppearance"] = json!(input["settings"]["nativeAppearance"].as_str().unwrap_or("adwaita"));
        theme
    }
    fn size(input: &Value) -> (f64, f64) {
        (input["context"]["width"].as_f64().unwrap_or(960.).max(1.), input["context"]["height"].as_f64().unwrap_or(600.).max(1.))
    }
    /// What the session answered, as the host's reply: its tree translated when there is a new one,
    /// the title, the effects, and the menus and frame when they changed.
    fn reply(&mut self, files: &Value, input: &Value, theme: &Value) -> Value {
        let mut out = Map::new();
        let effects = self.absorb(files);
        if self.fresh {
            out.insert("tree".into(), self.translate(input, theme));
            self.fresh = false;
        } else {
            out.insert("same".into(), json!(true));
        }
        if !self.title.is_null() {
            out.insert("title".into(), self.title.clone());
        }
        out.insert("capture".into(),json!(self.capture));
        if !effects.is_empty() {
            out.insert("effects".into(), Value::Array(effects));
        }
        Value::Object(out)
    }
    /// Takes what the session answered: a new tree (kept until the host is given it), and the effects
    /// to carry out, with the menus and the frame when they changed and the close when it asked.
    fn absorb(&mut self, files: &Value) -> Vec<Value> {
        if !files["title"].is_null() { self.title=files["title"].clone(); }
        if let Some(capture)=files["capture"].as_bool() {self.capture=capture;}
        if let Some(on) = files["loading"].as_bool() {
            self.loading = on;
        }
        if files["tree"].is_object() {
            self.raw = files["tree"].clone();
            self.fresh = true;
            self.index_menu();
        }
        let mut effects: Vec<Value> = files["effects"].as_array().cloned().unwrap_or_default();
        effects.retain(|effect| {
            if effect["kind"]=="closePreview" {self.resources.close(effect);false} else {true}
        });
        for (key, slot) in [("menus", &mut self.menus), ("chrome", &mut self.chrome)] {
            if files.get(key).is_some() && files[key] != *slot {
                *slot = files[key].clone();
                effects.push(json!({"kind":key,"value":files[key]}));
            }
        }
        if files["closed"] == true {
            effects.push(json!({"kind":"close"}));
        }
        effects
    }
    fn translate(&self, input: &Value, theme: &Value) -> Value {
        let lang = input["lang"].as_str().unwrap_or("en");
        convert(&self.raw, &Env { theme, lang, menu: self.menu, sub_active:self.sub_active })
    }
    /// The context menu of the new tree: its items (for the keyboard), and the row and submenu back to
    /// none when there is no menu or it is another one.
    fn index_menu(&mut self) {
        fn find(v: &Value) -> Option<&Value> {
            if v["type"] == "contextMenu" {
                return Some(v);
            }
            v["children"].as_array()?.iter().find_map(find)
        }
        let items = find(&self.raw).and_then(|m| m["items"].as_array().cloned()).unwrap_or_default();
        if items != self.menu_items {
            self.menu = (-1, -1);
            self.sub_active=-1;
        }
        self.menu_items = items;
    }
    fn selectable(&self, i: i64) -> bool {
        usize::try_from(i).ok().and_then(|i| self.menu_items.get(i)).is_some_and(|it| it.is_object() && it["header"].is_null() && it["disabled"] != true)
    }
    fn session(&mut self, input: &Value, theme: &Value) -> Value {
        let (width, height) = Self::size(input);
        let request = if self.id == 0 {
            let args = &input["args"];
            json!({"op":"create","native":true,"mode":args["mode"].as_str().unwrap_or("browser"),"args":args,
                "title":"","start":"","name":"","multiple":false,"filter":"","ignoreCase":false,
                "preferences":input["settings"]["files"],"theme":theme,"apps":apps(input),"width":width,"height":height})
        } else {
            json!({"op":"poll","native":true,"id":self.id,"width":width,"height":height,"theme":theme,
                "locale":{"lang":input["lang"]}})
        };
        let files = ffi::request(&request);
        if self.id == 0 {
            self.id = files["id"].as_u64().unwrap_or(0);
        }
        files
    }
    /// An action of the interface, with what the host knows of the event.
    fn event(&mut self, action: Value, extra: &Map<String, Value>) -> Value {
        let mut request = json!({"op":"event","native":true,"id":self.id,"action":action});
        for (k, v) in extra {
            request[k] = v.clone();
        }
        ffi::request(&request)
    }
    /// The keyboard of the open context menu: arrows move the row, Right opens a submenu, Left closes
    /// it, Enter and Space choose. None when there is no menu, or the key isn't one of these.
    fn menu_key(&mut self, key: &str) -> Option<Value> {
        if self.menu_items.is_empty() { return None; }
        let open=self.menu.1>=0;
        let items=if open {
            self.menu_items.get(self.menu.1 as usize).and_then(|it|it["submenu"].as_array()).map(Vec::as_slice).unwrap_or(&[])
        } else { &self.menu_items };
        let active=if open {self.sub_active} else {self.menu.0};
        match key {
            "ArrowDown"|"ArrowUp"|"Home"|"End" => {
                let dir=if matches!(key,"ArrowUp"|"End") {-1} else {1};
                let from=if matches!(key,"Home"|"End") || active<0 {if dir>0 {-1} else {0}} else {active};
                if let Some(next)=advance(items,from,dir) {
                    if open {self.sub_active=next;} else {self.menu=(next,-1);self.sub_active=-1;}
                }
            }
            "ArrowRight" => {
                if !open && self.selectable(self.menu.0) {
                    if let Some(sub)=self.menu_items[self.menu.0 as usize]["submenu"].as_array() {
                        self.menu.1=self.menu.0;self.sub_active=advance(sub,-1,1).unwrap_or(-1);
                    }
                }
            }
            "ArrowLeft"|"Escape" if open => {self.menu.1=-1;self.sub_active=-1;}
            "ArrowLeft" => (),
            "Escape" => {
                let files=self.event(json!(["closeContext"]),&Map::new());
                return Some(json!({"effects":self.absorb(&files)}));
            }
            "Enter"|" "|"Space" => {
                if selectable(items,active) {
                    let item=items[active as usize].clone();
                    if !open && item["submenu"].is_array() {
                        self.menu.1=self.menu.0;
                        self.sub_active=advance(item["submenu"].as_array().unwrap(),-1,1).unwrap_or(-1);
                    } else if item["action"].is_array() {
                        let files=self.event(item["action"].clone(),&Map::new());
                        return Some(json!({"effects":self.absorb(&files)}));
                    }
                }
            }
            _=>return None,
        }
        self.fresh=true;
        Some(json!({}))
    }

}

impl NativeApp for FilesView {
    /// Listings and jobs come from workers: polled at 16 ms while one is being worked, 100 ms idle.
    fn refresh_ms(&self) -> Option<f64> {
        Some(if self.loading { 16. } else { 100. })
    }
    fn request(&mut self, input: &Value) -> Value {
        let theme = Self::theme(input);
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                let mut pending=Vec::new();
                if input["catalog"]!=self.catalog {
                    self.catalog=input["catalog"].clone();
                    if self.id!=0 {
                        let extra=Map::from_iter([("apps".into(),apps(input))]);
                        let changed=self.event(json!(["apps"]),&extra);
                        pending=self.absorb(&changed);
                    }
                }
                let files = self.session(input, &theme);
                let mut reply=self.reply(&files, input, &theme);
                if !pending.is_empty() {
                    pending.extend(reply["effects"].as_array().into_iter().flatten().cloned());
                    reply["effects"]=Value::Array(pending);
                }
                reply
            }
            "close" => {
                if self.id!=0 {ffi::request(&json!({"op":"close","id":self.id}));self.id=0;}
                self.resources.clear(); json!({})
            }
            "icon" => crate::files::native_icon(input["size"].as_f64().unwrap_or(32.)),
            "menuHover" => {
                let i = input["key"].as_i64().unwrap_or(-1);
                // the row under the pointer; a submenu opens beside its row, any other row closes it
                let has_sub = usize::try_from(i).ok().and_then(|i| self.menu_items.get(i)).is_some_and(|it| it["submenu"].is_array());
                let next = (if self.selectable(i) { i } else { -1 }, if has_sub { i } else { -1 });
                if next != self.menu {
                    self.menu = next;
                    self.sub_active=-1;
                    self.fresh = true;
                }
                json!({})
            }
            "menuSubHover" => {
                let i=input["key"].as_i64().unwrap_or(-1);
                let sub=self.menu_items.get(self.menu.1 as usize).and_then(|item|item["submenu"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
                let next=if selectable(sub,i) {i} else {-1};
                if next!=self.sub_active {self.sub_active=next;self.fresh=true;}
                json!({})
            }
            // what the shell made of an effect (kind, generation and its result): a service event
            "next" => {
                let extra = input["result"].as_object().cloned().unwrap_or_default();
                self.resources.observe(&input["result"]);
                let mut effects=Vec::new();
                if extra.get("disks").is_some() {
                    let changed=self.event(json!(["disks"]),&extra);
                    effects.extend(self.absorb(&changed));
                }
                let files = self.event(json!(["service"]), &extra);
                effects.extend(self.absorb(&files));
                json!({"effects":effects})
            }
            cmd => {
                let mut extra = input.as_object().cloned().unwrap_or_default();
                for k in ["cmd", "key"] {
                    extra.remove(k);
                }
                let action = if input["key"].is_null() { json!([cmd]) } else { json!([cmd, input["key"]]) };
                let files = self.event(action, &extra);
                // Host::send queues effects and calls sync afterwards. Keep the
                // fresh tree until that sync; consuming it here loses immediate
                // selection, renaming, menu and drop-target updates.
                let effects=self.absorb(&files);
                json!({"effects":effects,"capture":self.capture})
            }
        }
    }
    /// Keys the window has: the context menu's, and everything Files does with the keyboard. None when
    /// it did nothing with the key, so the shell keeps its shortcuts.
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        if self.id == 0 {
            return None;
        }
        if let Some(out) = self.menu_key(key) {
            return Some(out);
        }
        let mut extra = Map::new();
        for (k, bit) in [("shift", 1), ("ctrl", 2), ("alt", 4), ("meta", 16)] {
            extra.insert(k.into(), json!(flags & bit != 0));
        }
        extra.insert("key".into(), json!(key));
        extra.insert("char".into(), json!(chr));
        let files = self.event(json!(["key"]), &extra);
        let changed = files["tree"].is_object() || files["effects"].as_array().is_some_and(|e| !e.is_empty());
        let effects = self.absorb(&files);
        changed.then(|| json!({"effects":effects}))
    }
}

impl Drop for FilesView {
    fn drop(&mut self) {
        if self.id != 0 {
            ffi::request(&json!({"op":"close","id":self.id}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::tests::Home;
    use crate::native_view::Host;
    use crate::runtime::native as scene;
    use std::time::Duration;

    fn facts(path: &std::path::Path) -> Value {
        json!({"args":{"path":path},"settings":{"files":{}},"catalog":[],"registry":[],"lang":"en","focused":true,
            "context":{"theme":{"mode":"dark","text":"#ffffff","textDim":"#aaaaaa","accent":"#0a84ff","control":"#333333","separator":"#444444"}}})
    }
    /// The listing comes from a worker: tick the host until `text` is on screen.
    fn wait_for(host: &mut Host, text: &str) {
        for _ in 0..300 {
            host.tick(1e18);
            if host.text().contains(text) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{text:?} never showed: {}", host.text());
    }

    #[test]
    fn a_files_window_lists_a_folder_and_acts_on_clicks_with_no_react() {
        let home = Home::new();
        home.file("report.txt", "x");
        std::fs::create_dir_all(home.path("Folder")).unwrap();
        home.file("Folder/inner.txt", "y");
        let mount = scene::create(scene::VIEW);
        let mut host = Host::new(mount, Box::new(FilesView::default()), "", None);
        host.facts(facts(&home.path("")));
        wait_for(&mut host, "report.txt");
        assert!(host.text().contains("Folder"), "{}", host.text());
        // double-clicking a folder opens it: its own file is listed and the old ones are gone
        let folder = host.find_text("Folder");
        assert_ne!(folder, 0, "the folder's name is on screen");
        for kind in ["move", "down", "up", "down", "up"] {
            host.pointer(kind, folder, 5., 5., 0);
        }
        wait_for(&mut host, "inner.txt");
        assert!(!host.text().contains("report.txt"), "the listing changed: {}", host.text());
        drop(host);
        scene::destroy(mount);
    }
    #[test]
    fn actions_keep_the_new_tree_for_sync_and_services_update_the_native_window() {
        let home=Home::new();
        home.file("report.txt","x");
        let mut view=FilesView::default();
        let mut input=facts(&home.path("")); input["cmd"]=json!("sync");
        view.request(&input);
        for _ in 0..300 {
            view.request(&input);
            if view.raw.to_string().contains("report.txt") {break;}
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(view.raw.to_string().contains("report.txt"));
        let action=view.request(&json!({"cmd":"select","key":0}));
        assert!(action["tree"].is_null());
        assert!(view.fresh,"an action must not consume the unpainted tree");
        let reply=view.request(&input);
        assert!(reply["tree"].is_object(),"{reply}");
        assert!(!view.fresh);
        view.request(&json!({"cmd":"disks","disks":[{"name":"sdb","transport":"usb","fstype":"vfat","label":"USB native"}]}));
        assert!(view.request(&input)["tree"].to_string().contains("USB native"));
        view.request(&json!({"cmd":"place","key":"apps"}));
        input["catalog"]=json!([{"id":"test","name":"New native app","desktopId":"test","mimeTypes":"text/plain;video/mp4;"}]);
        view.request(&input);
        for _ in 0..300 {
            if view.raw.to_string().contains("New native app") {break;}
            std::thread::sleep(Duration::from_millis(20)); view.request(&input);
        }
        assert!(view.raw.to_string().contains("New native app"));
        assert_eq!(apps(&input)[0]["desktopId"],"test.desktop");
        assert_eq!(apps(&input)[0]["mimeTypes"],json!(["text/plain","video/mp4"]));
    }

    #[test]
    fn submenu_keyboard_skips_disabled_rows_and_hover_keeps_the_parent_open() {
        let mut view=FilesView::default();
        view.menu_items=vec![json!({"label":"Open with","submenu":["-",{"label":"Disabled","disabled":true},{"label":"Editor","action":["with","editor"]},{"label":"Player","action":["with","player"]}]})];
        view.menu=(-1,-1);
        assert!(view.menu_key("ArrowDown").is_some());
        view.menu_key("ArrowRight");
        assert_eq!((view.menu,view.sub_active),((0,0),2));
        view.menu_key("ArrowDown"); assert_eq!(view.sub_active,3);
        view.menu_key("ArrowDown"); assert_eq!(view.sub_active,2);
        view.menu_key("End"); assert_eq!(view.sub_active,3);
        view.request(&json!({"cmd":"menuSubHover","key":2}));
        assert_eq!((view.menu,view.sub_active),((0,0),2));
        view.menu_key("Escape"); assert_eq!(view.menu,(0,-1));
        view.menu_key("Enter"); assert_eq!((view.menu,view.sub_active),((0,0),2));
        view.menu_key("ArrowLeft"); assert_eq!(view.menu,(0,-1));
    }

    #[test]
    fn native_folder_receives_external_copies_and_explicit_internal_moves() {
        let home=Home::new();
        home.file("source.txt","copied bytes");home.file("move.txt","moved bytes");
        std::fs::create_dir_all(home.path("Destination")).unwrap();
        let mount=scene::create(scene::VIEW);
        let mut host=Host::new(mount,Box::new(FilesView::default()),"",None);
        host.facts(facts(&home.path("Destination")));
        wait_for(&mut host,"Destination");
        // Wait for the directory's writable status, which is obtained by the worker.
        let mut token=0;
        for _ in 0..300 {
            host.tick(1e18);
            let title=host.find_text("Destination");
            token=unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(title))};
            if token!=0 {break;}
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(token,0);
        assert!(host.file_drop(token,&json!([home.path("source.txt")]),false));
        wait_for(&mut host,"source.txt");
        assert_eq!(std::fs::read_to_string(home.path("Destination/source.txt")).unwrap(),"copied bytes");
        assert!(home.path("source.txt").exists());
        assert!(host.file_drop(token,&json!([home.path("move.txt")]),true));
        wait_for(&mut host,"move.txt");
        assert_eq!(std::fs::read_to_string(home.path("Destination/move.txt")).unwrap(),"moved bytes");
        assert!(!home.path("move.txt").exists());
        drop(host);scene::destroy(mount);
    }

}
