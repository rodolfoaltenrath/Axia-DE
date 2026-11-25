//! The native Preferences window against a fake session service: it opens over the panes,
//! changes the profile with validated commands, saves through a `settings` effect that the fake
//! shell feeds back, configures every session again and captures key combinations.
use super::*;

struct Service {
    token: u64,
    ready: std::collections::BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
    next_sid: i64,
    pending: Option<Value>,
    fx: Vec<Value>,
    tree: Value,
    /// What the shell holds as `settings.terminal`: every `settings` effect replaces it.
    settings: Value,
    home: String,
    fonts: Value,
}
impl Service {
    fn new() -> Self {
        Self {
            token: 0,
            ready: Default::default(),
            calls: vec![],
            next_sid: 100,
            pending: None,
            fx: vec![],
            tree: Value::Null,
            settings: json!({}),
            home: "/home/test".into(),
            fonts: json!([]),
        }
    }
    fn answer(&mut self, method: &str, args: &Value) -> Value {
        match method {
            "term.create" => {
                self.next_sid += 1;
                json!(self.next_sid)
            }
            "term.infos" => json!(args[0].as_array().unwrap().iter().filter_map(|s| {
                Some(json!({"sid":s.as_i64()?,"info":{"process":"","busy":false,"cols":80,"rows":24,"cwd":"/home/test","shell":"bash"}}))
            }).collect::<Vec<_>>()),
            "term.events" => json!([]),
            "fonts.list" => self.fonts.clone(),
            _ => Value::Null,
        }
    }
    fn absorb(&mut self, reply: &Value) {
        if let Some(effect) = reply.get("effect") {
            self.pending = Some(effect.clone());
        }
        if let Some(tree) = reply.get("tree") {
            self.tree = tree.clone();
        }
        for fx in reply["effects"].as_array().into_iter().flatten() {
            if fx["kind"] == "settings" {
                self.settings = fx["patch"]["terminal"].clone();
            }
            self.fx.push(fx.clone());
        }
    }
    fn count(&self, method: &str) -> usize {
        self.calls.iter().filter(|(m, _)| m == method).count()
    }
    fn sync_input(&self) -> Value {
        json!({"cmd":"sync","args":{"__nativeOwner":9,"__nativeWindow":"terminal"},
            "settings":{"terminal":self.settings},
            "context":{"width":900,"height":600,"focused":true,"home":self.home,
                "theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","sidebarBg":"#2a2a2e","hover":"#333333"}}})
    }
    fn resync(&mut self, app: &mut App, now: i64) {
        let reply = app.request_at(&self.sync_input(), now);
        self.absorb(&reply);
    }
    fn send(&mut self, app: &mut App, now: i64, input: Value) -> Value {
        let reply = app.request_at(&input, now);
        self.absorb(&reply);
        self.resync(app, now);
        reply
    }
    fn key(&mut self, app: &mut App, now: i64, key: &str, flags: i32, code: i32) -> Option<Value> {
        let reply = app.key_at(key, "", flags, &KeyFacts { code, base: "", repeat: false, shift: false }, now)?;
        self.absorb(&reply);
        self.resync(app, now);
        Some(reply)
    }
    fn drive(&mut self, app: &mut App, now: &mut i64) {
        for _ in 0..200 {
            *now += 60;
            let reply = match self.pending.take() {
                Some(effect) => json!({"effect":effect}),
                None => {
                    let reply = app.request_at(&self.sync_input(), *now);
                    self.absorb(&reply);
                    reply
                }
            };
            let Some(effect) = reply.get("effect") else {
                if !app.jobs.busy() {
                    return;
                }
                continue;
            };
            let result = match effect["method"].as_str() {
                Some("start") => {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.clone(), args.clone()));
                    let value = self.answer(&method, &args);
                    self.ready.insert(self.token, value);
                    json!(self.token)
                }
                _ => {
                    let packets: Vec<Value> = effect["args"][1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|t| {
                            let id = t.as_u64()?;
                            self.ready.remove(&id).map(|value| json!({"token":id,"value":value}))
                        })
                        .collect();
                    for token in effect["args"][2].as_array().unwrap() {
                        self.ready.remove(&token.as_u64().unwrap());
                    }
                    json!({"method":"take","value":packets})
                }
            };
            let reply = app.request_at(&json!({"cmd":"next","result":result}), *now);
            self.absorb(&reply);
            self.resync(app, *now);
        }
        panic!("the window never settled");
    }
    /// The saved profile (the first, unless the settings say otherwise).
    fn profile(&self, index: usize) -> Value {
        self.settings["profiles"][index].clone()
    }
}
fn start() -> (App, i64, Service) {
    let mut app = App::default();
    let mut service = Service::new();
    let reply = app.request_at(&service.sync_input(), 1000);
    service.absorb(&reply);
    let mut now = 1000;
    service.drive(&mut app, &mut now);
    (app, now, service)
}
fn opened() -> (App, i64, Service) {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    service.drive(&mut app, &mut now);
    (app, now, service)
}
fn cmd(service: &mut Service, app: &mut App, now: i64, name: &str, key: Value) -> Value {
    service.send(app, now, json!({"cmd":name,"key":key}))
}
fn tree(service: &Service) -> String {
    service.tree.to_string()
}
fn temp_home(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("axia-prefs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Downloads")).unwrap();
    dir.to_string_lossy().into_owned()
}

#[test]
fn the_preferences_open_over_the_window_and_ask_for_the_fonts_once() {
    let (mut app, mut now, mut service) = start();
    assert!(!tree(&service).contains("\"key\":\"preferences\""));
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    service.drive(&mut app, &mut now);
    let shown = tree(&service);
    assert!(shown.contains("\"key\":\"preferences\"") && shown.contains("terminal.prefs.sec.general"), "{shown}");
    assert!(shown.contains("terminal.prefs.name"), "the first section is General");
    assert_eq!(service.count("fonts.list"), 1);
    cmd(&mut service, &mut app, now, "pfDone", json!(null));
    assert!(!tree(&service).contains("\"key\":\"preferences\""), "Done closes them");
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    service.drive(&mut app, &mut now);
    assert_eq!(service.count("fonts.list"), 1, "the fonts are asked for once");
}

#[test]
fn the_menus_offer_the_preferences() {
    let (mut app, mut now, mut service) = start();
    service.drive(&mut app, &mut now);
    let menus = service.calls.iter().rev().find(|(m, _)| m == "windows.menus").map(|(_, a)| a[1].to_string()).unwrap_or_default();
    assert!(menus.contains("[\"do\",\"preferences\"]"), "{menus}");
    service.send(&mut app, now, json!({"cmd":"paneMenu","key":101,"x":10,"y":10}));
    assert!(tree(&service).contains("[\"do\",\"preferences\"]"), "the context menu has it too");
}

#[test]
fn a_toggle_saves_the_settings_and_configures_the_sessions_again() {
    let (mut app, mut now, mut service) = opened();
    let before = service.count("term.configure");
    cmd(&mut service, &mut app, now, "pfFlip", json!("confirmClose"));
    service.drive(&mut app, &mut now);
    assert_eq!(service.profile(0)["confirmClose"], false, "the save carries the change");
    assert_eq!(service.settings["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(service.count("term.configure"), before + 1, "every session is configured again, once");
    cmd(&mut service, &mut app, now, "pfFlip", json!("confirmClose"));
    assert_eq!(service.profile(0)["confirmClose"], true);
}

#[test]
fn values_are_clamped_and_unknown_or_locked_fields_are_refused() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfVal", json!(["fontSize", 99]));
    assert_eq!(service.profile(0)["fontSize"], 40);
    cmd(&mut service, &mut app, now, "pfVal", json!(["lineSpacing", 1.2345]));
    assert_eq!(service.profile(0)["lineSpacing"], 1.23);
    cmd(&mut service, &mut app, now, "pfVal", json!(["cursorStyle", "wavy"]));
    assert_eq!(service.profile(0)["cursorStyle"], "block", "an unknown option is refused");
    cmd(&mut service, &mut app, now, "pfVal", json!(["cursorStyle", "beam"]));
    assert_eq!(service.profile(0)["cursorStyle"], "beam");
    let saved = service.settings.clone();
    for (field, value) in [("id", json!("x")), ("itermGuid", json!("g")), ("triggers", json!([])), ("nothing", json!(1)), ("fontSize", json!("big")), ("blur", json!(3))] {
        cmd(&mut service, &mut app, now, "pfVal", json!([field, value]));
    }
    assert_eq!(service.settings, saved, "none of them changed anything");
    cmd(&mut service, &mut app, now, "pfText", json!("wordChars"));
    cmd(&mut service, &mut app, now, "pfText", json!("name"));
    let long = "x".repeat(200);
    service.send(&mut app, now, json!({"cmd":"pfText","key":"wordChars","value":long}));
    assert_eq!(service.profile(0)["wordChars"].as_str().unwrap().chars().count(), 60);
}

#[test]
fn profiles_can_be_added_duplicated_and_removed_but_not_the_last() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfFlip", json!("blur"));
    cmd(&mut service, &mut app, now, "pfDup", json!(null));
    let list = service.settings["profiles"].as_array().unwrap().clone();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1]["blur"], false, "a copy keeps the values");
    assert_eq!(list[1]["itermGuid"], "");
    let copy_id = list[1]["id"].as_str().unwrap().to_owned();
    assert_ne!(copy_id, "default");
    cmd(&mut service, &mut app, now, "pfText", json!("name"));
    service.send(&mut app, now, json!({"cmd":"pfText","key":"name","value":"Work"}));
    assert_eq!(service.profile(1)["name"], "Work", "the copy is the one selected and edited");
    assert_eq!(service.profile(0)["name"], "");
    cmd(&mut service, &mut app, now, "pfMakeDefault", json!(null));
    assert_eq!(service.settings["defaultProfile"], copy_id.as_str());
    cmd(&mut service, &mut app, now, "pfRemove", json!(null));
    let list = service.settings["profiles"].as_array().unwrap().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(service.settings["defaultProfile"], "default", "removing the default profile hands it to another");
    cmd(&mut service, &mut app, now, "pfRemove", json!(null));
    assert_eq!(service.settings["profiles"].as_array().unwrap().len(), 1, "the last profile stays");
    cmd(&mut service, &mut app, now, "pfAdd", json!(null));
    let list = service.settings["profiles"].as_array().unwrap().clone();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1]["blur"], true, "a new profile starts from the factory values");
}

#[test]
fn editing_a_color_makes_a_custom_scheme_and_a_preset_replaces_it() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfSection", json!(1));
    assert!(tree(&service).contains("terminal.prefs.schemes"));
    cmd(&mut service, &mut app, now, "pfColorEdit", json!("bg"));
    service.send(&mut app, now, json!({"cmd":"pfColorText","value":"#112233"}));
    cmd(&mut service, &mut app, now, "pfColorOk", json!(null));
    let p = service.profile(0);
    assert_eq!(p["colors"], "custom");
    assert_eq!(p["custom"]["bg"], "#112233");
    assert_eq!(p["custom"]["ansi"].as_array().unwrap().len(), 16);
    cmd(&mut service, &mut app, now, "pfColorEdit", json!("ansi:3"));
    service.send(&mut app, now, json!({"cmd":"pfColorText","value":"nope"}));
    cmd(&mut service, &mut app, now, "pfColorOk", json!(null));
    assert_ne!(service.profile(0)["custom"]["ansi"][3], "nope", "a value that is not #rrggbb is dropped");
    cmd(&mut service, &mut app, now, "pfColorEdit", json!("ansi:3"));
    service.send(&mut app, now, json!({"cmd":"pfColorText","value":"#ABCDEF"}));
    cmd(&mut service, &mut app, now, "pfColorOk", json!(null));
    assert_eq!(service.profile(0)["custom"]["ansi"][3], "#abcdef");
    cmd(&mut service, &mut app, now, "pfScheme", json!("custom"));
    cmd(&mut service, &mut app, now, "pfScheme", json!("nonexistent"));
    assert_eq!(service.profile(0)["colors"], "custom", "only known presets can be chosen");
    let preset = model(&json!({"cmd":"catalog"}))["presets"][1][0].as_str().unwrap().to_owned();
    cmd(&mut service, &mut app, now, "pfScheme", json!(preset));
    assert_eq!(service.profile(0)["colors"], preset.as_str());
}

#[test]
fn a_slider_follows_the_pointer_while_it_is_held() {
    let (mut app, now, mut service) = opened();
    let reply = service.send(&mut app, now, json!({"cmd":"pfSlide","key":["transparency", 0.0, 0.9],"x":200.0,"y":5.0,"frame":[100.0, 0.0, 200.0, 20.0]}));
    assert_eq!(reply["capture"], true, "the drag is captured");
    assert_eq!(service.profile(0)["transparency"], 0.45);
    service.send(&mut app, now, json!({"cmd":"pointerMove","x":900.0,"y":5.0}));
    assert_eq!(service.profile(0)["transparency"], 0.9, "past the end it stops at the maximum");
    let reply = service.send(&mut app, now, json!({"cmd":"dragEnd","x":100.0,"y":5.0}));
    assert_eq!(reply["capture"], false);
    assert_eq!(service.profile(0)["transparency"], 0.0, "the release sets the last position");
    service.send(&mut app, now, json!({"cmd":"pointerMove","x":300.0,"y":5.0}));
    assert_eq!(service.profile(0)["transparency"], 0.0, "after it the pointer is not followed");
}

#[test]
fn a_shortcut_is_captured_from_the_next_combination_and_takes_it_from_the_others() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfSection", json!(5));
    cmd(&mut service, &mut app, now, "pfCapture", json!(["sc", "newTab"]));
    assert!(tree(&service).contains("terminal.prefs.combo.press"), "the button waits for the keys");
    // a modifier alone does nothing, Escape cancels
    assert!(service.key(&mut app, now, "Super", 16, 125).is_some());
    assert!(tree(&service).contains("terminal.prefs.combo.press"));
    service.key(&mut app, now, "Escape", 0, 1);
    assert!(!tree(&service).contains("terminal.prefs.combo.press"));
    assert!(service.settings["shortcuts"].is_null() || service.settings["shortcuts"].as_object().is_some_and(|m| m.is_empty()));
    // Super+k (code 37) for New Tab
    cmd(&mut service, &mut app, now, "pfCapture", json!(["sc", "newTab"]));
    service.key(&mut app, now, "k", 16, 37);
    assert_eq!(service.settings["shortcuts"]["newTab"], json!(["Super+k", "Ctrl+Shift+t"]), "the first combination changes, the second stays");
    // Super+n belongs to New Window: capturing it for New Tab takes it from there
    cmd(&mut service, &mut app, now, "pfCapture", json!(["sc", "newTab"]));
    service.key(&mut app, now, "n", 16, 49);
    assert_eq!(service.settings["shortcuts"]["newTab"][0], "Super+n");
    assert_eq!(service.settings["shortcuts"]["newWindow"], json!(["Ctrl+Shift+n"]));
    // the window honors the new shortcut once the Preferences are closed
    cmd(&mut service, &mut app, now, "pfDone", json!(null));
    let creates = service.count("term.create");
    let mut later = now;
    service.key(&mut app, now, "n", 16, 49);
    service.drive(&mut app, &mut later);
    assert_eq!(service.count("term.create"), creates + 1, "Super+n opens a tab now");
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    cmd(&mut service, &mut app, now, "pfReset", json!("newTab"));
    assert!(service.settings["shortcuts"].get("newTab").is_none(), "Reset gives the factory shortcut back");
}

#[test]
fn the_key_map_editors_add_capture_and_change_entries() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfSection", json!(5));
    cmd(&mut service, &mut app, now, "pfMapAdd", json!(true));
    cmd(&mut service, &mut app, now, "pfMapAdd", json!(false));
    assert_eq!(service.settings["keyMap"], json!([{"combo":"","action":"text","param":""}]));
    assert_eq!(service.profile(0)["keyMap"].as_array().unwrap().len(), 1);
    cmd(&mut service, &mut app, now, "pfCapture", json!(["map", true, 0]));
    service.key(&mut app, now, "k", 2, 37);
    assert_eq!(service.settings["keyMap"][0]["combo"], "Ctrl+k");
    cmd(&mut service, &mut app, now, "pfMapKind", json!([true, 0, "hex"]));
    assert_eq!(service.settings["keyMap"][0]["action"], "hex");
    assert_eq!(service.settings["keyMap"][0]["param"], "0x1b");
    service.send(&mut app, now, json!({"cmd":"pfMapText","key":[true, 0],"value":"0x1b 0x5b 0x41"}));
    assert_eq!(service.settings["keyMap"][0]["param"], "0x1b 0x5b 0x41");
    cmd(&mut service, &mut app, now, "pfMapKind", json!([true, 0, "action"]));
    assert_eq!(service.settings["keyMap"][0]["action"], "newTab");
    cmd(&mut service, &mut app, now, "pfPick", json!(["km1-0-act", "splitRight"]));
    assert_eq!(service.settings["keyMap"][0]["action"], "splitRight");
    cmd(&mut service, &mut app, now, "pfPick", json!(["km1-0-act", "notAnAction"]));
    assert_eq!(service.settings["keyMap"][0]["action"], "splitRight", "an action that does not exist is refused");
    cmd(&mut service, &mut app, now, "pfMapKind", json!([true, 0, "frobnicate"]));
    assert_eq!(service.settings["keyMap"][0]["action"], "splitRight");
    cmd(&mut service, &mut app, now, "pfMapDel", json!([true, 0]));
    assert_eq!(service.settings["keyMap"], json!([]));
    cmd(&mut service, &mut app, now, "pfMapDel", json!([true, 7]));
    assert_eq!(service.profile(0)["keyMap"].as_array().unwrap().len(), 1, "the profile's own list is not touched");
}

#[test]
fn triggers_are_added_toggled_edited_and_removed() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfSection", json!(6));
    cmd(&mut service, &mut app, now, "pfTrAdd", json!(null));
    assert_eq!(service.profile(0)["triggers"], json!([{"regex":"error","action":"notify","param":"","enabled":true}]));
    cmd(&mut service, &mut app, now, "pfTrToggle", json!(0));
    assert_eq!(service.profile(0)["triggers"][0]["enabled"], false);
    cmd(&mut service, &mut app, now, "pfTrToggle", json!(0));
    assert_eq!(service.profile(0)["triggers"][0]["enabled"], true);
    service.send(&mut app, now, json!({"cmd":"pfTrText","key":[0, "regex"],"value":"fail(ed)?"}));
    service.send(&mut app, now, json!({"cmd":"pfTrText","key":[0, "action"],"value":"x"}));
    assert_eq!(service.profile(0)["triggers"][0]["regex"], "fail(ed)?");
    assert_eq!(service.profile(0)["triggers"][0]["action"], "notify", "only the regex and the parameter are free text");
    cmd(&mut service, &mut app, now, "pfPick", json!(["tr-0", "highlight"]));
    assert_eq!(service.profile(0)["triggers"][0]["action"], "highlight");
    cmd(&mut service, &mut app, now, "pfPick", json!(["tr-0", "format-disk"]));
    assert_eq!(service.profile(0)["triggers"][0]["action"], "highlight");
    cmd(&mut service, &mut app, now, "pfTrDel", json!(0));
    assert_eq!(service.profile(0)["triggers"], json!([]));
}

#[test]
fn pickers_open_one_at_a_time_and_choose_a_listed_font() {
    let (mut app, mut now, mut service) = start();
    service.fonts = json!([{"family":"Fira Code","ps":"FiraCode-Regular","mono":true},{"family":"Sans","ps":"Sans-Regular","mono":false},
        {"family":"Noto Color Emoji","ps":"NotoColorEmoji","mono":false,"color":true}]);
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    service.drive(&mut app, &mut now);
    cmd(&mut service, &mut app, now, "pfSection", json!(2));
    assert!(!tree(&service).contains("font-0"));
    cmd(&mut service, &mut app, now, "pfPicker", json!("font"));
    let open = tree(&service);
    assert!(open.contains("font-0") && open.contains("Fira Code") && !open.contains("\"Sans\""), "only monospaced families are offered: {open}");
    cmd(&mut service, &mut app, now, "pfPicker", json!("nonAscii"));
    let both = tree(&service);
    assert!(!both.contains("font-0") && both.contains("nonAscii-1") && !both.contains("Noto Color Emoji"), "one picker is open, color fonts are left out");
    cmd(&mut service, &mut app, now, "pfPicker", json!("font"));
    cmd(&mut service, &mut app, now, "pfPick", json!(["font", "Fira Code"]));
    assert_eq!(service.profile(0)["font"], "Fira Code");
    assert!(!tree(&service).contains("font-0"), "choosing closes the list");
}

#[test]
fn rules_are_kept_one_per_line() {
    let (mut app, now, mut service) = opened();
    service.send(&mut app, now, json!({"cmd":"pfRules","value":"~/prod/** ; root@*  ;  *.prod.com"}));
    assert_eq!(service.profile(0)["rules"], "~/prod/**\nroot@*\n*.prod.com");
}

#[test]
fn colors_of_a_preset_file_are_imported_with_the_picked_profiles() {
    let (mut app, mut now, mut service) = start();
    let home = temp_home("import");
    std::fs::write(
        std::path::Path::new(&home).join("Downloads").join("Work.json"),
        r#"{"Profiles":[{"Name":"Work","Guid":"G1","Normal Font":"Menlo-Regular 14"},{"Name":"Play","Guid":"G2"}],"Default Bookmark Guid":"G2"}"#,
    )
    .unwrap();
    std::fs::write(std::path::Path::new(&home).join("notes.txt"), "not listed").unwrap();
    service.home = home.clone();
    app.home = home.clone();
    service.send(&mut app, now, json!({"cmd":"do","key":"preferences"}));
    service.drive(&mut app, &mut now);
    cmd(&mut service, &mut app, now, "pfImportOpen", json!(null));
    let panel = tree(&service);
    assert!(panel.contains("import-panel") && panel.contains("Downloads/Work.json") && !panel.contains("notes.txt"), "{panel}");
    cmd(&mut service, &mut app, now, "pfImportFile", json!("Downloads/Work.json"));
    let shown = tree(&service);
    assert!(shown.contains("Work") && shown.contains("Play") && shown.contains("★"), "both profiles are listed, the default one starred: {shown}");
    cmd(&mut service, &mut app, now, "pfImportPick", json!(1));
    cmd(&mut service, &mut app, now, "pfImportApply", json!(true));
    let names: Vec<String> = service.settings["profiles"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap_or("").to_owned()).collect();
    assert_eq!(names, ["", "Work"], "the unpicked profile is not imported");
    assert_eq!(service.settings["defaultProfile"], "default", "the starred profile was not picked, so it does not become the default");
    let panel = tree(&service);
    assert!(!panel.contains("import-panel") && panel.contains("terminal.prefs.import.done"), "the panel closes and a notice says what happened");
    // Importing again replaces the profile with the same iTerm2 identity instead of adding a copy
    cmd(&mut service, &mut app, now, "pfImportOpen", json!(null));
    cmd(&mut service, &mut app, now, "pfImportFile", json!("Downloads/Work.json"));
    cmd(&mut service, &mut app, now, "pfImportPick", json!(1));
    cmd(&mut service, &mut app, now, "pfImportApply", json!(false));
    assert_eq!(service.settings["profiles"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_import_reads_only_inside_the_home_folder_and_reports_what_it_cannot_read() {
    let (mut app, now, mut service) = opened();
    let home = temp_home("outside");
    app.home = home.clone();
    cmd(&mut service, &mut app, now, "pfImportOpen", json!(null));
    service.send(&mut app, now, json!({"cmd":"pfImportPath","value":"/etc/passwd"}));
    cmd(&mut service, &mut app, now, "pfImportLoad", json!(null));
    assert!(tree(&service).contains("terminal.prefs.import.failed"), "a file outside the home is refused");
    service.send(&mut app, now, json!({"cmd":"pfImportPath","value":"~/Downloads/missing.json"}));
    cmd(&mut service, &mut app, now, "pfImportLoad", json!(null));
    assert!(tree(&service).contains("terminal.prefs.import.failed"));
    std::fs::write(std::path::Path::new(&home).join("Downloads").join("junk.json"), "this is not json").unwrap();
    service.send(&mut app, now, json!({"cmd":"pfImportPath","value":"~/Downloads/junk.json"}));
    cmd(&mut service, &mut app, now, "pfImportLoad", json!(null));
    assert!(tree(&service).contains("terminal.prefs.import.failed"), "a file that is not a profile export is an error, not a crash");
    assert_eq!(service.settings, json!({}), "nothing was saved");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn escape_closes_one_layer_at_a_time() {
    let (mut app, now, mut service) = opened();
    cmd(&mut service, &mut app, now, "pfImportOpen", json!(null));
    cmd(&mut service, &mut app, now, "pfSection", json!(2));
    cmd(&mut service, &mut app, now, "pfPicker", json!("font"));
    assert!(tree(&service).contains("import-panel"));
    service.key(&mut app, now, "Escape", 0, 1);
    assert!(!tree(&service).contains("import-panel"), "the import panel goes first");
    service.key(&mut app, now, "Escape", 0, 1);
    assert!(tree(&service).contains("\"key\":\"preferences\"") && !tree(&service).contains("font-0"), "then the open list");
    service.key(&mut app, now, "Escape", 0, 1);
    assert!(!tree(&service).contains("\"key\":\"preferences\""), "then the window");
}

#[test]
fn while_the_preferences_are_open_no_key_reaches_the_shell() {
    let (mut app, now, mut service) = opened();
    service.fx.clear();
    service.key(&mut app, now, "a", 0, 30);
    service.key(&mut app, now, "Enter", 0, 28);
    assert!(service.fx.iter().all(|f| f["kind"] != "term" || f["op"] == "focus"), "{:?}", service.fx);
}

#[test]
fn every_section_draws_with_the_factory_profile() {
    let (mut app, now, mut service) = opened();
    for section in 0..7 {
        cmd(&mut service, &mut app, now, "pfSection", json!(section));
        let shown = tree(&service);
        assert!(shown.contains(prefs_ui::SECTIONS[section].1), "section {section} is titled");
        assert!(!shown.contains("null"), "no control lost its text: section {section}");
    }
    cmd(&mut service, &mut app, now, "pfSection", json!(9));
    assert!(tree(&service).contains(prefs_ui::SECTIONS[6].1), "an unknown section is ignored");
}
