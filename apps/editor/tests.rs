//! The window against the job protocol, with a scratch home folder.
use super::*;
use axia_fs::policy::Policy;
use std::{fs, path::PathBuf};

struct Home(PathBuf);
impl Home {
    fn new(name: &str) -> Home {
        let path = std::env::temp_dir().join(format!("axia-editor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Home(fs::canonicalize(path).unwrap())
    }
    fn file(&self, name: &str, text: impl AsRef<[u8]>) -> String {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The window with a shell that records every job and answers it.
struct Window {
    app: App,
    now: i64,
    token: u64,
    /// (method, arguments) of every job started.
    calls: Vec<(String, Value)>,
    /// What the next job of a method answers.
    answers: HashMap<String, Value>,
    events: Vec<Value>,
    effects: Vec<Value>,
    waiting: Vec<(u64, String)>,
    tree: Value,
    title: String,
}
impl Window {
    fn open(home: &Home, args: Value) -> Window {
        let mut app = App::default();
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        app.resources(assets, Some(home.0.to_str().unwrap()));
        app.policy = Some(Policy::new(&home.0, "tester"));
        let mut window = Window {
            app,
            now: 10_000,
            token: 0,
            calls: Vec::new(),
            answers: HashMap::new(),
            events: Vec::new(),
            effects: Vec::new(),
            waiting: Vec::new(),
            tree: Value::Null,
            title: String::new(),
        };
        let mut args = args;
        args["__nativeOwner"] = json!(7);
        args["__nativeWindow"] = json!("editor");
        window.sync_with(args);
        window
    }
    fn sync_with(&mut self, args: Value) {
        let input = json!({"cmd":"sync","lang":"en","args":args,"context":{"width":900,"height":600,"focused":true}});
        let out = self.app.request_at(&input, self.now);
        self.handle(out);
    }
    fn sync(&mut self) {
        let args = json!({"__nativeOwner":7,"__nativeWindow":"editor","path":self.app.args_seen[0],"nonce":self.app.args_seen[1]});
        self.now += 250;
        self.sync_with(args.clone());
        // like the host, which syncs again once the answers arrived
        self.now += 1;
        self.sync_with(args);
    }
    /// Runs the job protocol until nothing is left to start or to take.
    fn handle(&mut self, out: Value) {
        let mut out = out;
        for _ in 0..200 {
            if out["tree"].is_object() {
                self.tree = out["tree"].clone();
            }
            if let Some(title) = out["title"].as_str() {
                self.title = title.into();
            }
            self.effects.extend(out["effects"].as_array().into_iter().flatten().cloned());
            let effect = out["effect"].clone();
            let result = match effect["method"].as_str() {
                Some("start") => {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    self.calls.push((method.clone(), effect["args"][2].clone()));
                    self.waiting.push((self.token, method));
                    json!(self.token)
                }
                Some("take") => {
                    let packets: Vec<Value> = std::mem::take(&mut self.waiting)
                        .into_iter()
                        .map(|(token, method)| {
                            let value = if method == "windows.events" {
                                json!(std::mem::take(&mut self.events))
                            } else {
                                self.answers.remove(&method).unwrap_or(Value::Null)
                            };
                            json!({"token":token,"value":value})
                        })
                        .collect();
                    json!({"value":packets})
                }
                // an answer is still on its way: let the poll interval pass and ask again
                _ if !self.waiting.is_empty() => {
                    self.now += 60;
                    out = self.app.request_at(&json!({"cmd":"noop"}), self.now);
                    continue;
                }
                _ => return,
            };
            self.now += 60;
            out = self.app.request_at(&json!({"cmd":"next","result":result}), self.now);
        }
    }
    fn send(&mut self, input: Value) {
        let out = self.app.request_at(&input, self.now);
        self.handle(out);
        self.sync();
    }
    fn run(&mut self, command: &str) {
        self.send(json!({"cmd":"menu","key":command}));
    }
    fn key(&mut self, name: &str, text: &str, flags: i32) -> bool {
        let facts = KeyFacts { code: -1, base: if text.is_empty() { "" } else { text }, repeat: false, shift: flags & 1 != 0 };
        self.now += 20;
        let handled = self.app.handle_key(name, text, flags, &facts, self.now);
        if handled {
            let out = self.app.output(self.now);
            self.handle(out);
        }
        self.sync();
        handled
    }
    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let s = ch.to_string();
            assert!(self.key(&s, &s, 0), "typing {ch}");
        }
    }
    fn called(&self, method: &str) -> Vec<Value> {
        self.calls.iter().filter(|c| c.0 == method).map(|c| c.1.clone()).collect()
    }
    fn text(&self) -> String {
        self.app.tab().doc.text()
    }
    fn message(&self) -> String {
        self.app.message.clone().map(|m| m.1).unwrap_or_default()
    }
    fn shows(&self, needle: &str) -> bool {
        self.tree.to_string().contains(needle)
    }
}
const CTRL: i32 = 2;

#[test]
fn a_new_document_is_typed_and_saved_through_the_dialog() {
    let home = Home::new("save");
    let mut w = Window::open(&home, json!({}));
    assert_eq!(w.title, "Untitled");
    assert!(w.shows("Plain text") && w.shows("Line 1, column 1"));
    assert_eq!(w.called("inputMethod.context").last(), Some(&json!([true])));
    assert_eq!(w.called("windows.closeGuard").last(), Some(&json!(["editor", false])));
    assert_eq!(w.called("windows.menus").len(), 1);

    w.type_text("hi");
    assert_eq!(w.title, "Untitled •");
    assert_eq!(w.called("windows.closeGuard").last(), Some(&json!(["editor", true])));

    w.answers.insert("files.pick".into(), json!(home.path("note.md")));
    assert!(w.key("s", "s", CTRL));
    let pick = &w.called("files.pick")[0][0];
    assert_eq!((pick["mode"].as_str(), pick["title"].as_str()), (Some("save"), Some("Save as")));
    assert_eq!(fs::read_to_string(home.0.join("note.md")).unwrap(), "hi");
    assert_eq!(w.title, "note.md");
    assert_eq!(w.app.tab().language, "markdown");
    assert_eq!(w.message(), "Saved: note.md");
    assert_eq!(w.called("windows.closeGuard").last(), Some(&json!(["editor", false])));

    // with a path, saving writes at once and keeps the permissions
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(home.0.join("note.md"), fs::Permissions::from_mode(0o600)).unwrap();
    w.type_text("!");
    w.run("save");
    assert_eq!(fs::read_to_string(home.0.join("note.md")).unwrap(), "hi!");
    assert_eq!(fs::metadata(home.0.join("note.md")).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(w.called("files.pick").len(), 1);
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 1, "no temporary file is left");
}

#[test]
fn protected_places_links_and_unreadable_files_are_refused() {
    let home = Home::new("refuse");
    fs::create_dir_all(home.0.join(".config/axia")).unwrap();
    let policy = Policy::new(&home.0, "tester");
    let write = |name: &str| files::write(&policy, &home.0.join(name), "x");
    assert_eq!(write(".config/axia/settings.json"), Err("fs.protected".into()));
    assert_eq!(files::write(&policy, std::path::Path::new("/etc/axia-test"), "x"), Err("fs.outsideHome".into()));
    home.file("real.txt", "real");
    std::os::unix::fs::symlink(home.0.join("real.txt"), home.0.join("link.txt")).unwrap();
    assert_eq!(write("link.txt"), Err("fs.notRegular".into()));
    assert_eq!(fs::read_to_string(home.0.join("real.txt")).unwrap(), "real");

    let mut w = Window::open(&home, json!({}));
    for (name, bytes) in [("zero.bin", b"a\0b".to_vec()), ("latin1.txt", vec![b'a', 0xe7])] {
        let path = home.file(name, bytes);
        w.send(json!({"cmd":"drop","paths":[path]}));
        assert_eq!(w.message(), format!("{name}: looks like a binary file"));
        assert_eq!(w.app.tabs.len(), 1);
    }
    let big = home.file("big.txt", vec![b'a'; (files::MAX_OPEN + 1) as usize]);
    w.send(json!({"cmd":"drop","paths":[big]}));
    assert!(w.message().contains("is too big"), "{}", w.message());
    w.send(json!({"cmd":"drop","paths":[home.path("missing")]}));
    assert!(w.message().starts_with("missing: "));
    assert_eq!(w.app.tabs.len(), 1);
}

#[test]
fn open_with_tabs_and_the_project_panel() {
    let home = Home::new("tabs");
    let first = home.file("project/src/main.rs", "fn main() {}\n");
    home.file("project/src/lib.rs", "pub mod a;\n");
    home.file("project/.hidden", "");
    home.file("project/node_modules/skip.js", "");
    let mut w = Window::open(&home, json!({"path":first,"nonce":1}));
    // the empty tab was replaced and the project follows the file's folder
    assert_eq!((w.app.tabs.len(), w.app.root.as_str()), (1, "project/src"));
    assert_eq!(w.title, "main.rs");
    assert!(w.shows("\"lib.rs\"") && w.shows("Rust"));
    // the same file is not opened twice
    w.send(json!({"cmd":"drop","paths":[first]}));
    assert_eq!(w.app.tabs.len(), 1);
    w.send(json!({"cmd":"treeOpen","key":"project/src/lib.rs"}));
    assert_eq!((w.app.tabs.len(), w.title.as_str()), (2, "lib.rs"));
    assert_eq!(w.app.root, "project/src");
    assert!(w.key("Tab", "", CTRL));
    assert_eq!(w.title, "main.rs");

    // the folder dialog changes the project; folders open and close
    w.answers.insert("files.pick".into(), json!(home.path("project")));
    w.run("openFolder");
    assert_eq!(w.called("files.pick").last().unwrap()[0]["mode"], "folder");
    assert_eq!(w.app.root, "project");
    assert!(w.shows("tree:project/src") && !w.shows("tree:project/.hidden") && !w.shows("tree:project/src/lib.rs"));
    w.send(json!({"cmd":"treeToggle","key":"project/src"}));
    assert!(w.shows("tree:project/src/lib.rs"));
    w.run("toggleProject");
    assert!(!w.shows("\"sidebar\""));
    w.run("toggleProject");

    // go to file: letters in order, build folders left out
    w.run("gotoFile");
    for _ in 0..200 {
        if w.app.walking.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        w.sync();
    }
    assert_eq!(w.app.walked, ["src/lib.rs", "src/main.rs"]);
    assert!(!w.app.text_input(), "the list's field has the keyboard");
    w.send(json!({"cmd":"listQuery","value":"main"}));
    w.send(json!({"cmd":"listSubmit"}));
    assert_eq!((w.title.as_str(), w.app.tabs.len()), ("main.rs", 2));
    assert!(w.app.text_input());

    w.run("language");
    w.send(json!({"cmd":"listQuery","value":"pyth"}));
    w.send(json!({"cmd":"choose","key":0}));
    assert_eq!(w.app.tab().language, "python");

    // closing the active tab goes to the one before; the last one leaves an empty document
    w.run("close");
    assert_eq!(w.title, "lib.rs");
    w.run("close");
    assert_eq!((w.title.as_str(), w.app.tabs.len()), ("Untitled", 1));
}

#[test]
fn unsaved_changes_are_asked_about_when_a_tab_or_the_window_closes() {
    let home = Home::new("close");
    let path = home.file("a.txt", "a");
    let mut w = Window::open(&home, json!({"path":path,"nonce":1}));
    w.type_text("x");
    w.run("close");
    assert!(w.shows("Save changes to “a.txt”?"));
    assert!(!w.app.text_input());
    w.send(json!({"cmd":"overlayClose"}));
    assert_eq!((w.app.tabs.len(), w.text().as_str()), (1, "xa"));

    // a second, untitled document with changes
    w.run("new");
    w.type_text("b");
    w.events.push(json!({"kind":"closeRequested"}));
    w.sync();
    assert!(w.shows("Save changes to “a.txt”?"), "the first dirty tab is shown");
    w.send(json!({"cmd":"confirmSave"}));
    assert_eq!(fs::read_to_string(home.0.join("a.txt")).unwrap(), "xa");
    // the window goes on to the next unsaved document, which needs a name
    assert!(w.shows("Save changes to “Untitled”?"));
    assert!(w.called("windows.close").is_empty());
    w.answers.insert("files.pick".into(), json!(home.path("b.txt")));
    w.send(json!({"cmd":"confirmSave"}));
    assert_eq!(fs::read_to_string(home.0.join("b.txt")).unwrap(), "b");
    assert_eq!(w.called("windows.close"), [json!(["editor"])]);

    // discarding closes without writing; a cancelled dialog keeps the document
    let mut w = Window::open(&home, json!({}));
    w.type_text("lost");
    w.run("close");
    w.send(json!({"cmd":"confirmSave"}));
    assert_eq!(w.text(), "lost", "the cancelled dialog changed nothing");
    assert_eq!(w.app.overlay, Overlay::None);
    w.run("close");
    w.send(json!({"cmd":"discard"}));
    assert_eq!(w.text(), "");
}

#[test]
fn find_and_replace_keep_the_keyboard_in_the_field() {
    let home = Home::new("find");
    let path = home.file("t.txt", "one two one\none");
    let mut w = Window::open(&home, json!({"path":path,"nonce":1}));
    assert!(w.key("f", "f", CTRL));
    assert!(!w.app.text_input(), "the search field has the keyboard");
    assert!(!w.key("x", "x", 0), "plain keys belong to the field");
    w.send(json!({"cmd":"findQuery","value":"one"}));
    assert!(w.shows("1 of 3"));
    w.send(json!({"cmd":"findKey","key":"Enter"}));
    assert_eq!(w.app.tab().doc.primary(), Cursor::range(pos(0, 0), pos(0, 3)));
    w.send(json!({"cmd":"findKey","key":"Enter"}));
    assert_eq!(w.app.tab().doc.primary().start(), pos(0, 8));
    w.send(json!({"cmd":"findKey","key":"Enter","shift":true}));
    assert_eq!(w.app.tab().doc.primary().start(), pos(0, 0));
    assert!(!w.app.text_input());
    // a window shortcut still works from the field
    assert!(w.key("s", "s", CTRL));

    w.send(json!({"cmd":"findReplacement","value":"1"}));
    w.send(json!({"cmd":"replace"}));
    assert_eq!(w.text(), "1 two one\none");
    // a click on the button gives the keyboard back to the text
    w.send(json!({"cmd":"replaceAll","button":0,"x":1,"y":1}));
    assert_eq!(w.text(), "1 two 1\n1");
    assert_eq!(w.message(), "2 replacements");
    assert!(w.app.text_input());
    w.send(json!({"cmd":"findRegex","button":0}));
    w.send(json!({"cmd":"findQuery","value":"("}));
    assert!(w.shows("invalid regex"));
    w.send(json!({"cmd":"findClose"}));
    assert!(!w.shows("find-bar"));
    assert!(w.app.text_input());
}

#[test]
fn the_pointer_selects_and_go_to_line_jumps() {
    let home = Home::new("pointer");
    let body: String = (0..200).map(|i| format!("line {i} word\n")).collect();
    let path = home.file("long.txt", body);
    let mut w = Window::open(&home, json!({"path":path,"nonce":1}));
    let cell = w.app.cell;
    let x = |column: f64| SIDEBAR + w.app.gutter() + PAD + column * cell;
    let y = |line: f64| TABS + line * LINE + 3.;
    let press = |column: f64, line: f64| json!({"cmd":"areaDown","frame":[SIDEBAR, TABS, 680, 540],"x":x(column),"y":y(line),"button":0});
    let (first, drag_to, double) = (press(1.0, 0.0), json!({"cmd":"pointerMove","x":x(4.0),"y":y(1.0)}), press(9.0, 2.0));
    w.send(first);
    assert_eq!(w.app.tab().doc.primary(), Cursor::at(pos(0, 1)));
    w.send(drag_to);
    assert_eq!(w.app.tab().doc.primary(), Cursor::range(pos(0, 1), pos(1, 4)));
    w.send(json!({"cmd":"dragEnd"}));
    w.now += 1000;
    // two presses in quick succession
    for _ in 0..2 {
        w.now += 50;
        let out = w.app.request_at(&double, w.now);
        w.handle(out);
    }
    w.sync();
    assert_eq!(w.app.tab().doc.text_in(pos(2, 7), pos(2, 11)), "word");
    assert_eq!(w.app.tab().doc.primary(), Cursor::range(pos(2, 7), pos(2, 11)));
    assert!(w.shows("4 characters selected"));

    w.send(json!({"cmd":"areaWheel","deltaY":400}));
    assert_eq!(w.app.tab().top, 400.);
    assert!(!w.shows("\"line:0\""), "only the lines in view are drawn");

    w.run("gotoLine");
    w.send(json!({"cmd":"lineQuery","value":"150:3x"}));
    w.send(json!({"cmd":"lineSubmit"}));
    assert_eq!(w.app.tab().doc.primary(), Cursor::at(pos(149, 2)));
    assert!(w.shows("\"line:149\"") && w.shows("Line 150, column 3"));
}

#[test]
fn the_clipboard_printing_and_the_status_bar_actions() {
    let home = Home::new("status");
    let path = home.file("s.go", "a\tb\r\nc");
    let mut w = Window::open(&home, json!({"path":path,"nonce":1}));
    assert!(w.shows("Tabs: 4") && w.shows("CRLF"));
    w.key("a", "a", CTRL);
    w.key("c", "c", CTRL);
    assert!(w.effects.iter().any(|e| e["kind"] == "copy" && e["text"] == "a\tb\nc"));
    w.key("v", "v", CTRL);
    assert!(w.effects.iter().any(|e| e["kind"] == "paste"));
    assert!(w.app.paste("z").is_some());
    assert_eq!(w.text(), "z");
    w.send(json!({"cmd":"eol","button":0}));
    w.send(json!({"cmd":"tabSize","button":0}));
    w.send(json!({"cmd":"useTabs","button":0}));
    assert!(w.shows("Spaces: 8") && w.shows("\"LF\""));
    w.run("print");
    assert_eq!(w.called("print.open")[0], json!([{"title":"s.go","text":"z","mono":true}]));
    // the icon has no window
    let icon = w.app.request_at(&json!({"cmd":"icon","size":64}), 0);
    assert_eq!(icon["style"]["width"], 64.);
}
