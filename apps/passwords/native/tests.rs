use super::*;
use std::collections::BTreeMap;

/// The window's async bridge and axia-vault, faked: `reply` answers each call.
struct Bridge<F: FnMut(&str, &Value) -> Result<Value, String>> {
    reply: F,
    next: u64,
    started: BTreeMap<u64, (String, Value)>,
    now: i64,
    calls: Vec<(String, Value)>,
}
impl<F: FnMut(&str, &Value) -> Result<Value, String>> Bridge<F> {
    fn new(reply: F) -> Self {
        Self { reply, next: 0, started: BTreeMap::new(), now: 1000, calls: vec![] }
    }
    /// Runs every effect the app asks for until it is idle.
    fn run(&mut self, app: &mut App, mut out: Value) {
        for _ in 0..200 {
            let effect = out["effect"].clone();
            if effect.is_null() {
                self.now += 60;
                if !app.jobs.busy() {
                    return;
                }
                out = app.request_at(&json!({"cmd":"sync","args":{"__nativeOwner":1}}), self.now);
                continue;
            }
            let result = if effect["method"] == "start" {
                self.next += 1;
                let method = effect["args"][1].as_str().unwrap().to_string();
                let args = effect["args"][2].clone();
                self.calls.push((method.clone(), args.clone()));
                self.started.insert(self.next, (method, args));
                json!(self.next)
            } else {
                let mut ready = vec![];
                for token in effect["args"][1].as_array().unwrap() {
                    let token = token.as_u64().unwrap();
                    if let Some((method, args)) = self.started.remove(&token) {
                        ready.push(match (self.reply)(&method, &args) {
                            Ok(v) => json!({"token":token,"value":v}),
                            Err(e) => json!({"token":token,"error":e}),
                        });
                    }
                }
                json!({"value":ready})
            };
            out = app.request_at(&json!({"cmd":"next","result":result}), self.now);
        }
        panic!("the app never settled");
    }
    fn send(&mut self, app: &mut App, input: Value) {
        let out = app.request_at(&input, self.now);
        self.run(app, out);
    }
    fn ops(&self) -> Vec<String> {
        self.calls.iter().filter(|(m, _)| m == "vault.request").map(|(_, a)| a[0].as_str().unwrap().to_string()).collect()
    }
}

fn boot<F: FnMut(&str, &Value) -> Result<Value, String>>(b: &mut Bridge<F>, app: &mut App) {
    b.send(app, json!({"cmd":"sync","lang":"en","context":{"theme":{"mode":"dark"}},"args":{"__nativeOwner":1,"__nativeWindow":"passwords#1"}}));
}

fn locked_vault(method: &str, args: &Value) -> Result<Value, String> {
    match (method, args[0].as_str().unwrap_or("")) {
        ("vault.events", _) => Ok(json!([])),
        ("windows.menus", _) => Ok(Value::Null),
        ("vault.request", "status") => Ok(json!({"main":{"exists":true,"unlocked":false,"slots":[{"type":"keyfile"},{"type":"recovery"}]}})),
        ("vault.request", "unlock") => Err("bad-key".into()),
        _ => Err("unexpected".into()),
    }
}

#[test]
fn a_locked_vault_starts_on_its_first_method_and_reports_errors() {
    let mut app = App::default();
    let mut b = Bridge::new(locked_vault);
    boot(&mut b, &mut app);
    assert_eq!(app.phase, Phase::Locked);
    assert_eq!(app.lock.method, "keyfile");
    // without a key file it never calls the vault
    b.send(&mut app, json!({"cmd":"go"}));
    assert_eq!(app.lock.err, k("unlock.needKeyfile"));
    assert_eq!(b.ops(), ["status"]);
    app.lock.keyfile = Some(Picked { id: 7, name: "a.key".into() });
    b.send(&mut app, json!({"cmd":"go"}));
    assert_eq!(app.lock.err, error_text("bad-key"));
    assert!(!app.lock.busy);
    assert!(app.lock.keyfile.is_none(), "a descriptor is used once");
    let unlock = b.calls.iter().find(|(m, a)| m == "vault.request" && a[0] == "unlock").unwrap();
    assert_eq!(unlock.1[2], json!([7]));
    // the switch only takes the vault's methods
    b.send(&mut app, json!({"cmd":"method","key":"password"}));
    assert_eq!(app.lock.method, "keyfile");
    b.send(&mut app, json!({"cmd":"method","key":"recovery"}));
    assert_eq!(app.lock.method, "recovery");
    let tree = app.tree(b.now).to_string();
    assert!(tree.contains("passwords.unlock.title") && tree.contains("XXXXX-XXXXX"));
}

#[test]
fn creating_checks_the_form_then_shows_the_recovery_key() {
    let mut app = App::default();
    let mut created = false;
    let mut b = Bridge::new(move |method: &str, args: &Value| match (method, args[0].as_str().unwrap_or("")) {
        ("vault.events", _) => Ok(json!([])),
        ("windows.menus", _) => Ok(Value::Null),
        ("vault.request", "status") => Ok(json!({"main":{"exists":created,"unlocked":created}})),
        ("vault.request", "strength") => Ok(json!({"score":3,"bits":61})),
        ("vault.request", "create") => {
            created = true;
            Ok(json!({"recovery":"AAAAA-BBBBB"}))
        }
        ("vault.request", "list") => Ok(json!({"meta":{},"cats":[],"tags":[],"entries":[{"id":"x","title":"Mail"}]})),
        ("file.save", _) => Ok(json!(true)),
        _ => Err("unexpected".into()),
    });
    boot(&mut b, &mut app);
    assert_eq!(app.phase, Phase::None);
    assert_eq!(app.lock.method, "password");
    b.send(&mut app, json!({"cmd":"pw","value":"short"}));
    b.send(&mut app, json!({"cmd":"go"}));
    assert_eq!(app.lock.err, k("create.short"));
    b.send(&mut app, json!({"cmd":"pw","value":"long enough"}));
    assert_eq!(app.lock.score.as_ref().unwrap()["bits"], 61);
    b.send(&mut app, json!({"cmd":"go"}));
    assert_eq!(app.lock.err, k("create.mismatch"));
    b.send(&mut app, json!({"cmd":"pw2","value":"long enough"}));
    b.send(&mut app, json!({"cmd":"go"}));
    assert_eq!(app.lock.recovery.as_deref(), Some("AAAAA-BBBBB"));
    assert!(app.lock.pw.is_empty() && app.lock.pw2.is_empty());
    b.send(&mut app, json!({"cmd":"saveRecovery"}));
    let save = b.calls.iter().find(|(m, _)| m == "file.save").unwrap();
    assert!(save.1[1].as_str().unwrap().contains("AAAAA-BBBBB"));
    assert_eq!(app.toast.as_ref().unwrap().0, k("recovery.saved"));
    // Done only once the user confirmed having kept the key
    b.send(&mut app, json!({"cmd":"done"}));
    assert!(app.lock.recovery.is_some());
    b.send(&mut app, json!({"cmd":"confirm"}));
    b.send(&mut app, json!({"cmd":"done"}));
    assert_eq!(app.phase, Phase::Unlocked);
    assert_eq!(app.entries.len(), 1);
}

#[test]
fn vault_events_lock_the_window_and_menus_follow_the_phase() {
    let mut app = App::default();
    let mut events = vec![json!([]), json!([{"kind":"vault","ev":"touch"}]), json!([{"kind":"vault","ev":"locked","v":"main"}])];
    let mut b = Bridge::new(move |method: &str, args: &Value| match (method, args[0].as_str().unwrap_or("")) {
        ("vault.events", _) => Ok(if events.is_empty() { json!([]) } else { events.remove(0) }),
        ("windows.menus", _) => Ok(Value::Null),
        ("vault.request", "status") => Ok(json!({"main":{"exists":true,"unlocked":true}})),
        ("vault.request", "list") => Ok(json!({"entries":[{"id":"a"}]})),
        _ => Err("unexpected".into()),
    });
    boot(&mut b, &mut app);
    assert_eq!(app.phase, Phase::Unlocked);
    for _ in 0..3 {
        b.now += 300;
        b.send(&mut app, json!({"cmd":"sync","args":{"__nativeOwner":1}}));
    }
    assert_eq!(app.phase, Phase::Locked);
    assert!(app.entries.is_empty());
    let menus: Vec<_> = b.calls.iter().filter(|(m, _)| m == "windows.menus").collect();
    assert_eq!(menus.len(), 2, "sent when unlocked and again when locked");
    assert_eq!(menus[0].1[0], "passwords#1");
    assert_eq!(menus[1].1[1][0]["items"][0]["disabled"], true);
}

#[test]
fn errors_unknown_codes_and_icon() {
    assert_eq!(error_text("fido-timeout"), k("err.fido-timeout"));
    assert_eq!(error_text("weird"), kp("err.other", json!({"code":"weird"})));
    assert_eq!(error_text(""), kp("err.other", json!({"code":"?"})));
    let mut app = App::default();
    assert_eq!(app.request_at(&json!({"cmd":"icon","size":64}), 0)["style"]["width"], 64.);
    // nothing reaches the vault before the window has a bridge
    assert!(app.request_at(&json!({"cmd":"go"}), 0).get("effect").is_none());
}

/// An unlocked vault with a few items, faked; it remembers what was saved.
fn vault_with_items() -> impl FnMut(&str, &Value) -> Result<Value, String> {
    let mut saved: Vec<Value> = vec![];
    move |method: &str, args: &Value| {
        let op = args[0].as_str().unwrap_or("");
        let a = &args[1];
        match (method, op) {
            ("vault.events", _) => Ok(json!([])),
            ("windows.menus", _) | ("clipboard.secret", _) | ("clipboard.text", _) => Ok(Value::Null),
            ("file.pick", _) => Ok(json!({"id":9,"name":"/home/u/backup.csv"})),
            ("vault.request", "status") => Ok(json!({"main":{"exists":true,"unlocked":true,"slots":[{"id":1,"type":"password"},{"id":2,"type":"recovery"}]}})),
            ("vault.request", "list") if a["v"] == "login" => Ok(json!({"entries":[{"id":"k1","title":"GNOME keyring","sub":"login"}]})),
            ("vault.request", "list") => Ok(json!({"meta":{"clip":45},"cats":[{"id":"u1","name":"Mine","icon":"key","color":"#ff0000"}],"tags":[],
                "entries":[
                    {"id":"a","title":"Bank","sub":"me","cat":"internet","url":"https://www.bank.com","tags":["money"],"modified":10,"fav":true},
                    {"id":"b","title":"aws","sub":"ops","cat":"internet","totp":true,"modified":30},
                    {"id":"c","title":"Old","cat":"internet","trash":true,"modified":5},
                    {"id":"d","title":"Home wifi","cat":"wifi","modified":20}]})),
            ("vault.request", "report") => Ok(json!({"weak":[{"id":"a","score":1,"bits":20}],"reused":[["a","b"]],"old":[]})),
            ("vault.request", "search") => Ok(json!({"ids":["b"],"ms":0.4})),
            ("vault.request", "get") => Ok(json!({"entry":{"id":a["id"],"title":"Bank","cat":"internet","created":1,"modified":2,
                "f":[{"k":"username","t":"text","v":"me"},{"k":"password","t":"hidden","has":true},{"k":"url","t":"url","v":"https://bank.com"}]}})),
            ("vault.request", "reveal") => Ok(json!({"value":"s3cret"})),
            ("vault.request", "strength") => Ok(json!({"score":2,"bits":40})),
            ("vault.request", "generate") => Ok(json!({"password":format!("gen{}", a["length"]),"score":4,"bits":120})),
            ("vault.request", "save") => {
                saved.push(a["entry"].clone());
                Ok(json!({"id":"new1","item":{"id":"new1","title":a["entry"]["title"],"cat":a["entry"]["cat"],"modified":99}}))
            }
            ("vault.request", "set-meta" | "set-cats" | "set-tags" | "rename-tag") => Ok(json!({})),
            ("vault.request", "export-csv") => Ok(json!({"count":3})),
            _ => Err(format!("unexpected {method} {op}")),
        }
    }
}

fn unlocked() -> (App, Bridge<impl FnMut(&str, &Value) -> Result<Value, String>>) {
    let mut app = App::default();
    let mut b = Bridge::new(vault_with_items());
    boot(&mut b, &mut app);
    // the first ticks: the report's count and the search
    b.now += 100;
    b.send(&mut app, json!({"cmd":"sync","args":{"__nativeOwner":1}}));
    (app, b)
}

#[test]
fn the_window_lists_filters_searches_and_counts() {
    let (mut app, mut b) = unlocked();
    assert_eq!(app.phase, Phase::Unlocked);
    assert_eq!(app.login.len(), 1);
    assert_eq!(app.win.issues, 3, "one weak + a reused pair");
    // "all": live, not archived, by title (case-insensitive)
    let ids: Vec<_> = app.visible_list().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["b", "a", "d"]);
    b.send(&mut app, json!({"cmd":"sort"}));
    let ids: Vec<_> = app.visible_list().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["b", "d", "a"]);
    b.send(&mut app, json!({"cmd":"sel","key":"trash"}));
    assert_eq!(app.visible_list().len(), 1);
    b.send(&mut app, json!({"cmd":"sel","key":"all"}));
    b.send(&mut app, json!({"cmd":"q","value":"aw"}));
    b.now += 100;
    b.send(&mut app, json!({"cmd":"sync","args":{"__nativeOwner":1}}));
    assert_eq!(app.visible_list().len(), 1);
    assert_eq!(app.win.search_ms, Some(0.4));
    let tree = app.tree(b.now).to_string();
    assert!(tree.contains("passwords.list.found") && tree.contains("side:tag:money") && tree.contains("side:cat:u1"));
    b.send(&mut app, json!({"cmd":"qClear"}));
    b.now += 100;
    b.send(&mut app, json!({"cmd":"sync","args":{"__nativeOwner":1}}));
    assert_eq!(app.visible_list().len(), 3);
    // ↓ walks the list (newest first now)
    let out = app.key("ArrowDown", "", 0).unwrap();
    b.run(&mut app, out);
    assert_eq!(app.win.cur.as_deref(), Some("b"));
    let out = app.key("ArrowDown", "", 0).unwrap();
    b.run(&mut app, out);
    assert_eq!(app.win.cur.as_deref(), Some("d"));
    assert_eq!(app.viewing.as_ref().unwrap().e.as_ref().unwrap()["title"], "Bank");
}

#[test]
fn an_item_reveals_and_copies_its_fields() {
    let (mut app, mut b) = unlocked();
    b.send(&mut app, json!({"cmd":"open","key":"a"}));
    assert_eq!(app.viewing.as_ref().unwrap().e.as_ref().unwrap()["title"], "Bank");
    b.send(&mut app, json!({"cmd":"reveal","key":{"k":"password","i":0}}));
    assert_eq!(app.viewing.as_ref().unwrap().shown["password#0"], "s3cret");
    assert!(app.tree(b.now).to_string().contains("s3cret"));
    b.send(&mut app, json!({"cmd":"reveal","key":{"k":"password","i":0}}));
    assert!(app.viewing.as_ref().unwrap().shown.is_empty());
    // a hidden value is revealed then copied as a secret, with the vault's delay
    b.send(&mut app, json!({"cmd":"copyField","key":{"k":"password","i":0}}));
    let copy = b.calls.iter().rev().find(|(m, _)| m == "clipboard.secret").unwrap();
    assert_eq!(copy.1, json!(["s3cret", 45]));
    assert_eq!(app.toast.as_ref().unwrap().0["key"], "passwords.copied");
    b.send(&mut app, json!({"cmd":"copyField","key":{"k":"username","i":0}}));
    assert_eq!(b.calls.iter().rev().find(|(m, _)| m == "clipboard.text").unwrap().1, json!(["me"]));
    // the editor keeps the hidden value unless it is replaced
    b.send(&mut app, json!({"cmd":"edit"}));
    let ed = app.editor.as_ref().unwrap();
    assert_eq!(ed.fields[1]["keep"], true);
    b.send(&mut app, json!({"cmd":"title","value":" Bank 2 "}));
    b.send(&mut app, json!({"cmd":"editSave"}));
    let save = b.calls.iter().rev().find(|(m, a)| m == "vault.request" && a[0] == "save").unwrap();
    assert_eq!(save.1[1]["entry"]["title"], "Bank 2");
    assert_eq!(save.1[1]["entry"]["id"], "a");
    assert_eq!(save.1[1]["entry"]["f"][1], json!({"k":"password","t":"hidden","keep":true}));
    assert_eq!(app.win.cur.as_deref(), Some("new1"));
}

#[test]
fn a_new_item_uses_its_template_the_generator_and_notes() {
    let (mut app, mut b) = unlocked();
    b.send(&mut app, json!({"cmd":"new"}));
    let n = app.editor.as_ref().unwrap().fields.len();
    assert!(n > 0);
    let pw = app.editor.as_ref().unwrap().fields.iter().position(|f| f["k"] == "password").unwrap();
    // the generator's length slider: the pointer over its frame
    b.send(&mut app, json!({"cmd":"fgen","key":pw}));
    let out = app.request_at(&json!({"cmd":"genLen","x":250.,"frame":[100.,0.,150.,20.]}), b.now);
    assert_eq!(out["capture"], true);
    b.run(&mut app, out);
    let res = match &app.sheet { Some(Sheet::Gen { res, o, .. }) => (res.clone(), o["length"].clone()), _ => panic!() };
    assert_eq!(res.1, 64);
    assert_eq!(res.0.unwrap()["password"], "gen64");
    b.send(&mut app, json!({"cmd":"genUse"}));
    assert!(app.sheet.is_none());
    assert_eq!(app.editor.as_ref().unwrap().fields[pw]["v"], "gen64");
    assert_eq!(app.editor.as_ref().unwrap().strength.as_ref().unwrap()["bits"], 40);
    // notes are typed into the app's own multi-line field
    b.send(&mut app, json!({"cmd":"taFocus","key":"notes"}));
    assert!(app.text_input());
    app.key("a", "a", 0);
    app.key("Enter", "", 0);
    app.paste("bc");
    app.key("Backspace", "", 0);
    assert_eq!(app.editor.as_ref().unwrap().notes, "a\nb");
    // a pop-up list: the category
    b.send(&mut app, json!({"cmd":"choice","key":"cat"}));
    assert!(app.tree(b.now).to_string().contains("\"anchorKey\":\"choice:cat\""));
    b.send(&mut app, json!({"cmd":"pick","key":{"id":"cat","v":"u1"}}));
    assert_eq!(app.editor.as_ref().unwrap().cat, "u1");
    assert!(app.choice.is_none());
    b.send(&mut app, json!({"cmd":"addOpen"}));
    b.send(&mut app, json!({"cmd":"addField"}));
    assert_eq!(app.editor.as_ref().unwrap().fields.last().unwrap()["k"], "c1");
}

#[test]
fn sheets_categories_tags_settings_and_export() {
    let (mut app, mut b) = unlocked();
    b.send(&mut app, json!({"cmd":"newCat"}));
    b.send(&mut app, json!({"cmd":"catSave"}));
    assert!(matches!(&app.sheet, Some(Sheet::Cat(c)) if c.err == k("cats.needName")));
    b.send(&mut app, json!({"cmd":"catName","value":"Games"}));
    b.send(&mut app, json!({"cmd":"catIcon","key":"apps"}));
    b.send(&mut app, json!({"cmd":"catSave"}));
    assert!(app.sheet.is_none());
    assert_eq!(app.cats.as_array().unwrap().len(), 2);
    assert_eq!(app.toast.as_ref().unwrap().0, k("cats.saved"));
    // a category in use cannot go
    b.send(&mut app, json!({"cmd":"editTag","key":"money"}));
    b.send(&mut app, json!({"cmd":"tagName","value":"cash"}));
    b.send(&mut app, json!({"cmd":"tagSave"}));
    let rename = b.calls.iter().find(|(m, a)| m == "vault.request" && a[0] == "rename-tag").unwrap();
    assert_eq!(rename.1[1], json!({"from":"money","to":"cash"}));
    let tags = b.calls.iter().find(|(m, a)| m == "vault.request" && a[0] == "set-tags").unwrap();
    assert_eq!(tags.1[1]["tags"][0]["name"], "cash");
    assert!(app.sheet.is_none());
    // settings: the auto-lock delay goes to the vault's meta
    b.send(&mut app, json!({"cmd":"settings"}));
    b.send(&mut app, json!({"cmd":"setTab","key":"prefs"}));
    b.send(&mut app, json!({"cmd":"choice","key":"autolock"}));
    b.send(&mut app, json!({"cmd":"pick","key":{"id":"autolock","v":900}}));
    assert_eq!(app.meta["autolock"], 900);
    assert_eq!(app.meta["clip"], 45);
    b.send(&mut app, json!({"cmd":"setTab","key":"unlock"}));
    b.send(&mut app, json!({"cmd":"setAdd","key":"change"}));
    b.send(&mut app, json!({"cmd":"setPw","value":"short"}));
    b.send(&mut app, json!({"cmd":"setSubmit"}));
    assert!(matches!(&app.sheet, Some(Sheet::Settings(s)) if s.err == k("create.short")));
    // export as CSV needs the confirmation, then a file
    b.send(&mut app, json!({"cmd":"export"}));
    b.send(&mut app, json!({"cmd":"expKind","key":"csv"}));
    b.send(&mut app, json!({"cmd":"expGo"}));
    assert!(!b.calls.iter().any(|(m, _)| m == "file.pick"));
    b.send(&mut app, json!({"cmd":"expOk"}));
    b.send(&mut app, json!({"cmd":"expGo"}));
    let exp = b.calls.iter().rev().find(|(m, a)| m == "vault.request" && a[0] == "export-csv").unwrap();
    assert_eq!(exp.1[2], json!([9]));
    assert!(app.sheet.is_none());
    assert_eq!(app.toast.as_ref().unwrap().0["key"], "passwords.exp.doneCsv");
}

#[test]
fn menus_and_shortcuts_reach_the_window_and_locking_clears_it() {
    let (mut app, mut b) = unlocked();
    let out = app.key("g", "g", 16).unwrap();
    b.run(&mut app, out);
    assert!(matches!(&app.sheet, Some(Sheet::Gen { res: Some(_), .. })));
    b.send(&mut app, json!({"cmd":"sheetClose"}));
    let out = app.key("n", "n", 16).unwrap();
    b.run(&mut app, out);
    assert!(app.editor.is_some());
    app.event(&json!({"kind":"action","action":["security"]}));
    assert_eq!(app.win.sel, "security");
    b.now += 100;
    b.send(&mut app, json!({"cmd":"sync","args":{"__nativeOwner":1}}));
    assert!(app.security.rep.is_some());
    assert!(app.tree(b.now).to_string().contains("passwords.report.title"));
    app.event(&json!({"kind":"vault","ev":"locked","v":"main"}));
    assert_eq!(app.phase, Phase::Locked);
    assert!(app.editor.is_none() && app.sheet.is_none() && app.security.rep.is_none());
}

#[test]
fn widgets_qr_base64_and_tag_colors() {
    let (side, px) = ui::qr_pixels(&json!({"size":1,"rows":["8"]}), 2).unwrap();
    assert_eq!(side, 18);
    let dark = ((4 * 2) * side + 4 * 2) * 4;
    assert_eq!(&px[dark..dark + 4], &[0, 0, 0, 255]);
    assert_eq!(px[0], 255);
    assert!(ui::qr_pixels(&json!({"size":0}), 2).is_none());
    assert_eq!(ui::b64("aGk="), b"hi");
    assert_eq!(ui::b64("AAEC"), [0, 1, 2]);
    assert_eq!(ui::tag_color(&json!([{"name":"x","color":"#123456"}]), "x"), "#123456");
    // as the JS hash: "a" = 97 → 97 % 8 = 1
    assert_eq!(ui::tag_color(&json!([]), "a"), "#5e5ce6");
    assert_eq!(radix36(35), "z");
    assert_eq!(radix36(36), "10");
}
