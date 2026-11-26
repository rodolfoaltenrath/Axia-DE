//! Both windows against the job protocol, with a pretend helper that records what is asked.
use super::*;
use std::collections::{HashMap, VecDeque};

fn account() -> Value {
    json!({"id":"me","kind":"mail","name":"Me","email":"me@example.org","user":"me@example.org","signature":"Me",
        "imap":{"host":"imap.example.org","port":993},"smtp":{"host":"smtp.example.org","port":465}})
}
fn snapshot() -> Value {
    json!({"available":true,"isolated":true,"cacheDir":"","config":{"accounts":[account()]},
        "status":{"accounts":{"me":{"status":"idle"}},"unread":2,"certs":[],"authFailed":{}}})
}
fn folders() -> Value {
    json!({"accounts":[{"id":"me","name":"Me","status":"idle","folders":[
        {"name":"INBOX","special":"inbox","display":"Inbox","unread":2,"total":3},
        {"name":"Junk","special":"junk","display":"Junk","unread":0,"total":0},
        {"name":"Work","special":"","display":"Work","unread":0,"total":1}]}]})
}
fn item(uid: u32, subject: &str) -> Value {
    json!({"acct":"me","folder":"INBOX","uid":uid,"from":format!("Sender {uid}"),"subject":subject,"preview":"…","date":1_700_000_000_000_i64,"seen":false,"labels":[]})
}
fn message(uid: u32, subject: &str) -> Value {
    json!({"acct":"me","folder":"INBOX","uid":uid,"subject":subject,"seen":false,"date":1_700_000_000_000_i64,"msgid":format!("<{uid}@x>"),
        "from":[{"name":"Ann","email":"ann@example.org"}],"to":[{"name":"","email":"me@example.org"}],"text":format!("body of {subject}"),"atts":[]})
}

/// The window with a shell that records every job and answers it.
struct Window {
    app: App,
    args: Value,
    width: f64,
    now: i64,
    token: u64,
    /// (method, arguments) of every job started.
    calls: Vec<(String, Value)>,
    /// What a method (or `mail.request:op`) answers, in order; the last one stays.
    answers: HashMap<String, VecDeque<Value>>,
    events: Vec<Value>,
    effects: Vec<Value>,
    waiting: Vec<(u64, String, Value)>,
    /// Methods whose answer is held back until released.
    held: Vec<String>,
    tree: Value,
    title: Value,
}
impl Window {
    fn new() -> Window {
        let mut app = App::default();
        app.resources(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"), None);
        let mut window = Window {
            app,
            args: json!({}),
            width: 1120.,
            now: 1_700_000_100_000,
            token: 0,
            calls: Vec::new(),
            answers: HashMap::new(),
            events: Vec::new(),
            effects: Vec::new(),
            waiting: Vec::new(),
            held: Vec::new(),
            tree: Value::Null,
            title: Value::Null,
        };
        window.answer("mail.snapshot", snapshot());
        window.reply("folders", folders());
        window.reply("list", json!({"version":1,"total":3,"items":[item(1, "First"), item(2, "Second"), item(3, "Third")]}));
        window
    }
    fn answer(&mut self, method: &str, value: Value) {
        self.answers.insert(method.into(), VecDeque::from([value]));
    }
    /// What the helper replies to an operation.
    fn reply(&mut self, op: &str, value: Value) {
        self.answer(&format!("mail.request:{op}"), json!({"value":value}));
    }
    fn replies(&mut self, op: &str, values: Vec<Value>) {
        self.answers.insert(format!("mail.request:{op}"), values.into_iter().map(|v| json!({"value":v})).collect());
    }
    fn fail(&mut self, op: &str, code: &str) {
        self.answer(&format!("mail.request:{op}"), json!({"error":{"code":code,"message":format!("failed: {code}")}}));
    }
    fn start(mut self, args: Value) -> Window {
        let mut args = args;
        args["__nativeOwner"] = json!(7);
        args["__nativeWindow"] = json!("mail#2");
        self.args = args;
        self.sync();
        self
    }
    fn sync_once(&mut self) {
        let input = json!({"cmd":"sync","lang":"en","args":self.args,"context":{"width":self.width,"height":640,"focused":true}});
        let out = self.app.request_at(&input, self.now);
        self.handle(out);
    }
    /// A few refreshes, as the host does while answers arrive.
    fn sync(&mut self) {
        for _ in 0..4 {
            self.now += 250;
            self.sync_once();
        }
    }
    fn later(&mut self, ms: i64) {
        self.now += ms;
        self.sync();
    }
    fn lookup(&mut self, method: &str, args: &Value) -> Value {
        if method == "pim.events" {
            return json!(std::mem::take(&mut self.events));
        }
        let name = if method == "mail.request" { format!("{method}:{}", args[0].as_str().unwrap()) } else { method.to_owned() };
        match self.answers.get_mut(&name) {
            Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
            Some(queue) => queue.front().cloned().unwrap_or_default(),
            None if method == "mail.request" || method == "mail.account" => json!({"value":{}}),
            None => Value::Null,
        }
    }
    /// Runs the job protocol until nothing is left to start or to take.
    fn handle(&mut self, out: Value) {
        let mut out = out;
        for _ in 0..400 {
            if out["tree"].is_object() {
                self.tree = out["tree"].clone();
            }
            if !out["title"].is_null() {
                self.title = out["title"].clone();
            }
            self.effects.extend(out["effects"].as_array().into_iter().flatten().cloned());
            let effect = out["effect"].clone();
            let result = match effect["method"].as_str() {
                Some("start") => {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.clone(), args.clone()));
                    self.waiting.push((self.token, method, args));
                    json!(self.token)
                }
                Some("take") => {
                    let mut packets = Vec::new();
                    for (token, method, args) in std::mem::take(&mut self.waiting) {
                        let name = if method == "mail.request" { args[0].as_str().unwrap().to_owned() } else { method.clone() };
                        if self.held.contains(&name) {
                            self.waiting.push((token, method, args));
                        } else {
                            let value = self.lookup(&method, &args);
                            packets.push(json!({"token":token,"value":value}));
                        }
                    }
                    json!({"value":packets})
                }
                _ if self.waiting.iter().any(|(_, m, a)| !self.held.contains(&if m == "mail.request" { a[0].as_str().unwrap().to_owned() } else { m.clone() })) => {
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
    fn act(&mut self, command: &str, key: Value) {
        self.send(json!({"cmd":command,"key":key}));
    }
    fn typed(&mut self, command: &str, key: Value, value: &str) {
        self.send(json!({"cmd":command,"key":key,"value":value}));
    }
    fn key(&mut self, key: &str, base: &str, flags: i32, shift: bool) -> bool {
        let facts = KeyFacts { code: -1, base, repeat: false, shift };
        let chr = if key.chars().count() == 1 && flags == 0 { key } else { "" };
        match self.app.key_event(key, chr, flags, &facts) {
            Some(out) => {
                self.handle(out);
                self.sync();
                true
            }
            None => false,
        }
    }
    /// The arguments of the helper operations asked so far.
    fn asked(&self, op: &str) -> Vec<Value> {
        self.calls.iter().filter(|(m, a)| m == "mail.request" && a[0] == op).map(|(_, a)| a[1].clone()).collect()
    }
    fn called(&self, method: &str) -> Vec<Value> {
        self.calls.iter().filter(|(m, _)| m == method).map(|(_, a)| a.clone()).collect()
    }
    fn nodes<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
        out.push(node);
        for child in node["children"].as_array().into_iter().flatten() {
            Self::nodes(child, out);
        }
    }
    fn all(&self) -> Vec<&Value> {
        let mut out = Vec::new();
        Self::nodes(&self.tree, &mut out);
        out
    }
    fn find(&self, key: &str) -> Option<&Value> {
        self.all().into_iter().find(|n| n["key"] == key)
    }
    fn text(&self) -> String {
        self.all().iter().filter_map(|n| n["label"].as_str()).collect::<Vec<_>>().join("\n")
    }
}

fn opened() -> Window {
    let mut w = Window::new();
    w.reply("open", message(1, "First"));
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    w
}

#[test]
fn the_list_loads_a_row_opens_and_is_marked_read_after_a_moment() {
    let mut w = opened();
    assert!(w.text().contains("Sender 2"), "{}", w.text());
    assert_eq!(w.asked("list")[0]["folder"], "*inbox");
    assert_eq!(w.asked("list").len(), 1, "an unchanged list is not asked again");
    assert_eq!(w.asked("open"), vec![json!({"acct":"me","folder":"INBOX","uid":1})]);
    assert!(w.text().contains("body") && w.text().contains("ann@example.org"), "{}", w.text());
    w.later(1500);
    assert_eq!(w.asked("flag"), vec![json!({"acct":"me","folder":"INBOX","uids":[1],"add":["seen"]})]);
}

#[test]
fn helper_events_reload_folders_and_rows_once() {
    let mut w = Window::new().start(json!({}));
    let (folders_before, lists_before) = (w.asked("folders").len(), w.asked("list").len());
    w.events = vec![json!({"ev":"changed"}), json!({"ev":"folders"})];
    w.later(300);
    w.later(600);
    assert_eq!(w.asked("folders").len(), folders_before + 1);
    assert_eq!(w.asked("list").len(), lists_before + 1);
}

#[test]
fn searching_needs_two_characters_and_clearing_returns_to_the_inbox() {
    let mut w = Window::new();
    w.reply("search", json!({"version":9,"items":[item(7, "Found")]}));
    let mut w = w.start(json!({}));
    w.typed("query", Value::Null, "f");
    assert!(w.asked("search").is_empty());
    w.typed("query", Value::Null, " fo ");
    assert_eq!(w.asked("search"), vec![json!({"q":"fo","count":1000})]);
    assert!(w.text().contains("Found") && !w.text().contains("Second"), "{}", w.text());
    w.typed("query", Value::Null, "");
    assert!(w.text().contains("Second"), "{}", w.text());
}

#[test]
fn archiving_makes_the_folder_when_there_is_none_and_deleting_moves_on() {
    let mut w = opened();
    assert!(w.key("e", "e", 2, false));
    assert_eq!(w.asked("create-folder"), vec![json!({"acct":"me","name":"Archive"})]);
    assert_eq!(w.asked("move"), vec![json!({"acct":"me","folder":"INBOX","uids":[1],"to":"Archive"})]);
    assert!(w.find("reading-empty").is_some());

    let mut w = opened();
    w.reply("open", message(2, "Second"));
    assert!(w.key("Delete", "", 0, false));
    assert_eq!(w.asked("delete"), vec![json!({"acct":"me","folder":"INBOX","uids":[1]})]);
    assert_eq!(w.asked("open").last().unwrap()["uid"], 2, "the next message opens");
    assert!(w.key("ArrowUp", "", 0, false));
    assert_eq!(w.asked("open").last().unwrap()["uid"], 1);
}

#[test]
fn flags_labels_and_junk_apply_to_the_selection() {
    let mut w = opened();
    w.act("flag", Value::Null);
    assert_eq!(w.asked("flag").last().unwrap()["add"], json!(["flagged"]));
    w.act("flag", Value::Null);
    assert_eq!(w.asked("flag").last().unwrap()["remove"], json!(["flagged"]));
    w.act("labels", Value::Null);
    assert!(w.find("labels").is_some());
    w.act("label", json!(3));
    assert_eq!(w.asked("flag").last().unwrap()["add"], json!(["label3"]));
    assert!(w.find("labels").is_none() && w.find("chip-3").is_some());
    w.act("junk", Value::Null);
    assert_eq!(w.asked("move")[0]["to"], "Junk");
}

#[test]
fn remote_images_wait_for_the_user_and_their_hosts_are_allowed_first() {
    let mut w = Window::new();
    let mut html = message(1, "First");
    html["html"] = json!({"blocks":[{"t":"img","src":"r:0","w":100,"h":50}],"links":[],"remote":["https://img.example.com/a.png"],"stats":{"remote":1,"trackers":0}});
    w.reply("open", html);
    w.answer("pim.image", json!({"image":41,"w":200,"h":100}));
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    assert!(w.find("images-banner").is_some());
    assert!(w.called("pim.image").is_empty() && w.called("mail.allowImages").is_empty());
    w.act("loadImages", json!("me|INBOX|1"));
    let order: Vec<&str> = w.calls.iter().map(|(m, _)| m.as_str()).filter(|m| matches!(*m, "mail.allowImages" | "pim.image")).collect();
    assert_eq!(order, ["mail.allowImages", "pim.image"]);
    assert_eq!(w.called("pim.image")[0], json!(["image",{"acct":"me","folder":"INBOX","uid":1,"src":"r:0","maxw":200}]));
    assert!(w.all().iter().any(|n| n["kind"] == "mailImage" && n["image"] == 41), "{}", w.tree);
    // another message: the picture is released
    w.reply("open", message(2, "Second"));
    w.act("row", json!(1));
    assert_eq!(w.called("pim.imageRelease"), vec![json!([41])]);
}

#[test]
fn trusting_a_sender_saves_it_and_links_show_their_real_address() {
    let mut w = Window::new();
    let mut html = message(1, "First");
    html["html"] = json!({"blocks":[{"t":"x","x":"click","l":0}],"links":[{"h":"https://evil.example/x","w":1,"txt":"bank.com"}],"stats":{"remote":0}});
    w.reply("open", html);
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    assert!(w.find("links-banner").is_some());
    w.act("link", json!(["me|INBOX|1", 0]));
    assert!(w.text().contains("https://evil.example/x") && w.text().contains("evil.example"));
    w.act("linkCopy", Value::Null);
    assert!(w.effects.contains(&json!({"host":true,"kind":"copy","text":"https://evil.example/x"})));
    w.act("trustImages", json!("me|INBOX|1"));
    assert_eq!(w.called("mail.saveConfig")[0][0]["trustedSenders"], json!(["ann@example.org"]));
    w.act("url", json!("mailto:bo%40example.org?subject=Hi%20there"));
    let options = &w.called("windows.open")[0];
    assert_eq!(options[1]["args"]["compose"], json!({"to":[{"name":"","email":"bo@example.org"}],"subject":"Hi there"}));
}

#[test]
fn an_error_and_an_offline_message_are_explained() {
    let mut w = Window::new();
    w.fail("open", "offline");
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    assert!(w.text().contains("cannot be reached"), "{}", w.text());
}

#[test]
fn replying_and_forwarding_open_a_compose_window_with_the_attachments_cached() {
    let mut w = Window::new();
    let mut with_files = message(1, "First");
    with_files["atts"] = json!([{"idx":0,"name":"../a.txt","size":5,"type":"text/plain"},{"idx":1,"name":"b.pdf","size":9,"type":"application/pdf"}]);
    w.reply("open", with_files);
    w.replies("attachment", vec![json!({"path":"att/1/a.txt"}), json!({"path":"att/1/b.pdf"})]);
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    assert!(w.key("r", "r", 2, false));
    let draft = w.called("windows.open")[0][1]["args"]["compose"].clone();
    assert_eq!(draft["subject"], "Re: First");
    assert_eq!(draft["to"], json!([{"name":"Ann","email":"ann@example.org"}]));
    assert!(draft["body"].as_str().unwrap().contains("> body of First"));
    w.act("forward", Value::Null);
    let draft = w.called("windows.open")[1][1]["args"]["compose"].clone();
    assert_eq!(draft["subject"], "Fwd: First");
    assert_eq!(draft["attachments"], json!([{"cached":"att/1/a.txt","name":"a.txt","size":5.0},{"cached":"att/1/b.pdf","name":"b.pdf","size":9.0}]));
}

#[test]
fn an_attachment_is_opened_or_saved_by_the_server_in_the_isolated_app() {
    let mut w = Window::new();
    let mut with_files = message(1, "First");
    with_files["atts"] = json!([{"idx":4,"name":"b.pdf","size":9,"type":"application/pdf"}]);
    w.reply("open", with_files);
    w.reply("attachment", json!({"path":"att/1/b.pdf"}));
    let mut w = w.start(json!({}));
    w.act("row", json!(0));
    w.act("attachmentOpen", json!(["me|INBOX|1", 4]));
    w.act("attachmentSave", json!(["me|INBOX|1", 4]));
    assert_eq!(w.called("mail.openAttachment"), vec![json!(["att/1/b.pdf","b.pdf"])]);
    assert_eq!(w.called("mail.saveAttachment"), vec![json!(["att/1/b.pdf","b.pdf"])]);
}

#[test]
fn a_notification_opens_its_message() {
    let mut w = Window::new();
    w.reply("open", message(3, "Third"));
    let w = w.start(json!({"acct":"me","folder":"INBOX","uid":3,"nonce":1}));
    assert_eq!(w.asked("open"), vec![json!({"acct":"me","folder":"INBOX","uid":3})]);
    assert!(w.asked("list").iter().any(|a| a["folder"] == "INBOX"));
}

#[test]
fn a_certificate_warning_asks_once_and_trusts_exactly_that_certificate() {
    let mut w = Window::new();
    let mut facts = snapshot();
    facts["status"]["certs"] = json!([{"acct":"me","host":"imap.example.org","port":993,"sha256":"ab:cd","reason":"self-signed"}]);
    w.answer("mail.snapshot", facts);
    let mut w = w.start(json!({}));
    assert!(w.find("certificate").is_some() && w.text().contains("ab:cd"));
    w.act("sheetClose", Value::Null);
    assert!(w.find("certificate").is_none(), "a closed warning does not come back by itself");
    w.act("accountWarning", json!("me"));
    assert!(w.find("certificate").is_some());
    w.act("certificateTrust", Value::Null);
    assert_eq!(w.called("mail.account"), vec![json!(["trustCertificate","me","imap","ab:cd"])]);
    assert!(w.find("certificate").is_none());
}

#[test]
fn account_setup_finds_the_provider_and_adds_calendar_and_contacts_too() {
    let mut w = Window::new();
    w.answer("mail.account", json!({"value":"someone-icloud-com"}));
    let mut w = w.start(json!({}));
    w.act("addAccount", Value::Null);
    w.typed("setupText", json!("email"), "not an address");
    w.act("setupNext", Value::Null);
    assert!(w.find("setup").is_some() && w.find("servers").is_none());
    w.typed("setupText", json!("email"), "someone@icloud.com");
    w.typed("setupText", json!("password"), "app-password");
    w.act("setupNext", Value::Null);
    assert!(w.find("servers").is_some());
    w.act("setupSecurity", json!(["smtp", "ssl"]));
    w.act("setupAdd", Value::Null);
    let calls = w.called("mail.account");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0][1]["imap"], json!({"host":"imap.mail.me.com","port":993,"security":"ssl"}));
    assert_eq!(calls[0][1]["smtp"], json!({"host":"smtp.mail.me.com","port":465,"security":"ssl"}));
    assert_eq!(calls[0][1]["id"], "someone-icloud-com");
    assert_eq!(calls[0][2], "app-password");
    assert_eq!(calls[1][1]["kind"], "dav");
    assert_eq!(calls[1][1]["url"], "https://caldav.icloud.com/");
}

#[test]
fn a_locked_keyring_is_explained_and_the_password_stays_masked() {
    let mut w = Window::new();
    w.answer("mail.account", json!({"error":{"code":"locked","message":"locked"}}));
    let mut w = w.start(json!({}));
    w.act("addAccount", Value::Null);
    w.typed("setupText", json!("email"), "a@gmail.com");
    w.typed("setupText", json!("password"), "secret");
    assert_eq!(w.find("setup-password").unwrap()["secure"], true);
    w.act("setupNext", Value::Null);
    w.act("setupAdd", Value::Null);
    assert_eq!(w.called("mail.account").len(), 1, "Gmail's calendar needs OAuth: no second account");
    assert!(w.text().to_lowercase().contains("keyring"), "{}", w.text());
}

fn composing(draft: Value) -> Window {
    Window::new().start(json!({"compose":draft}))
}

#[test]
fn a_new_message_needs_a_valid_recipient_and_is_sent_with_html_and_text() {
    let mut w = composing(json!(true));
    assert_eq!(w.title, json!({"key":"mail.compose.newMessage"}));
    w.act("send", Value::Null);
    assert!(w.find("compose-status").is_some() && w.asked("send").is_empty());
    w.typed("recipient", json!("to"), "bo@example.org,");
    w.typed("recipient", json!("to"), "nobody");
    w.act("send", Value::Null);
    assert!(w.asked("send").is_empty() && w.text().contains("nobody"), "an address being typed counts too");
    w.act("removeRecipient", json!(["to", 1]));
    w.typed("subject", Value::Null, "Hello");
    assert_eq!(w.title, "Hello");
    w.reply("send", json!({}));
    w.act("send", Value::Null);
    let sent = &w.asked("send")[0];
    assert_eq!(sent["to"], json!([{"name":"","email":"bo@example.org"}]));
    assert_eq!(sent["subject"], "Hello");
    assert!(sent["text"].as_str().unwrap().contains("-- \nMe"));
    assert!(sent["html"].as_str().unwrap().starts_with("<div"));
    w.later(700);
    assert_eq!(w.called("windows.close"), vec![json!(["mail#2"])]);
}

#[test]
fn suggestions_are_deduplicated_and_chosen_with_the_keys() {
    let mut w = Window::new();
    w.reply("autocomplete", json!({"items":[{"name":"Bo","email":"bo@example.org","src":"contacts"},{"name":"","email":"BO@example.org"},{"name":"Bea","email":"bea@example.org"}]}));
    let mut w = w.start(json!({"compose":true}));
    w.typed("recipient", json!("to"), "b");
    assert!(w.find("suggestion-1").is_some() && w.find("suggestion-2").is_none());
    assert_eq!(w.find("recipient-to").unwrap()["keys"], json!(["ArrowDown","ArrowUp","Tab"]));
    w.act("toKey", json!("ArrowDown"));
    w.act("recipientSubmit", json!("to"));
    assert!(w.text().contains("Bea") && w.find("suggestions").is_none());
    // with the field empty, Backspace removes the last chip; otherwise it edits the text
    assert_eq!(w.find("recipient-to").unwrap()["keys"], json!(["Backspace"]));
    w.act("toKey", json!("Backspace"));
    assert!(!w.text().contains("Bea"));
}

#[test]
fn the_body_takes_keys_with_shift_and_format_buttons() {
    let mut w = composing(json!({"to":[{"name":"","email":"bo@example.org"}],"noSignature":true}));
    assert!(w.app.text_input(), "with a recipient the cursor starts in the body");
    for key in ["a", "b", "c"] {
        assert!(w.key(key, key, 0, false));
    }
    assert!(w.key("ArrowLeft", "", 0, true) && w.key("ArrowLeft", "", 0, true));
    w.act("style", json!("bold"));
    assert!(w.key("Enter", "", 2, false), "Ctrl+Enter sends");
    assert_eq!(w.asked("send")[0]["text"], "a*bc*");
}

#[test]
fn closing_saves_a_draft_and_a_failed_save_does_not_trap_the_window() {
    let mut w = composing(json!({"subject":"Plan"}));
    assert!(w.called("windows.closeGuard").iter().all(|a| a[1] == false));
    w.typed("subject", Value::Null, "Plan B");
    assert_eq!(w.called("windows.closeGuard").last().unwrap()[1], true);
    w.reply("draft", json!({"uid":55}));
    w.events = vec![json!({"kind":"closeRequested"})];
    w.later(300);
    assert_eq!(w.asked("draft")[0]["replace"], 0);
    assert_eq!(w.asked("draft")[0]["format"], "rich");
    assert_eq!(w.called("windows.close").len(), 1);

    let mut w = composing(json!(true));
    w.typed("subject", Value::Null, "Offline");
    w.fail("draft", "offline");
    w.events = vec![json!({"kind":"closeRequested"})];
    w.later(300);
    assert!(w.called("windows.close").is_empty() && w.text().contains("failed: offline"));
    w.events = vec![json!({"kind":"closeRequested"})];
    w.later(300);
    assert_eq!(w.called("windows.close").len(), 1);
}

#[test]
fn drafts_autosave_and_a_send_waits_for_the_save_it_replaces() {
    let mut w = composing(json!({"to":[{"name":"","email":"bo@example.org"}]}));
    w.typed("subject", Value::Null, "Slow");
    w.reply("draft", json!({"uid":8}));
    w.held.push("draft".into());
    w.later(31_000);
    assert_eq!(w.asked("draft").len(), 1, "autosaved after half a minute");
    w.act("send", Value::Null);
    assert!(w.asked("send").is_empty());
    w.held.clear();
    w.sync();
    assert_eq!(w.asked("send")[0]["draftUid"], 8);
    assert_eq!(w.asked("send")[0]["draftFolder"], "Drafts");
}

#[test]
fn attachments_travel_as_leases_and_can_be_removed() {
    let mut w = Window::new();
    w.answer("mail.attachmentFile", json!({"file":3}));
    w.answer("mail.pickFile", json!({"file":4,"name":"notes.txt","size":12}));
    let draft = json!({"to":[{"name":"","email":"bo@example.org"}],"attachments":[{"cached":"att/1/a.pdf","name":"a.pdf","size":9}]});
    let mut w = w.start(json!({"compose":draft}));
    w.act("attach", Value::Null);
    assert!(w.text().contains("a.pdf") && w.text().contains("notes.txt"));
    w.act("detach", json!(1));
    assert_eq!(w.called("mail.releaseFile"), vec![json!([3])]);
    w.act("send", Value::Null);
    let call = w.called("mail.request").into_iter().find(|a| a[0] == "send").unwrap();
    assert_eq!(call[1]["attachments"], json!([{"name":"notes.txt"}]));
    assert_eq!(call[2], json!([{"file":4}]));
}

#[test]
fn mail_in_the_session_attaches_several_files_by_path() {
    let mut w = Window::new();
    let mut facts = snapshot();
    facts["isolated"] = json!(false);
    facts["cacheDir"] = json!("/cache/mail");
    w.answer("mail.snapshot", facts);
    w.answer("files.pick", json!(["/home/u/one.txt","/home/u/two.txt"]));
    let draft = json!({"to":[{"name":"","email":"bo@example.org"}],"attachments":[{"cached":"att/1/a.pdf","name":"a.pdf","size":9},{"cached":"../escape","name":"x","size":1}]});
    let mut w = w.start(json!({"compose":draft}));
    w.act("attach", Value::Null);
    assert_eq!(w.called("files.pick")[0][0]["multiple"], true);
    w.act("send", Value::Null);
    let call = w.called("mail.request").into_iter().find(|a| a[0] == "send").unwrap();
    assert_eq!(call[1]["attachments"], json!([{"name":"a.pdf"},{"name":"one.txt"},{"name":"two.txt"}]));
    assert_eq!(call[2], json!([{"path":"/cache/mail/att/1/a.pdf"},{"path":"/home/u/one.txt"},{"path":"/home/u/two.txt"}]));
    assert!(w.called("mail.attachmentFile").is_empty() && w.called("mail.pickFile").is_empty());
}

#[test]
fn without_the_helper_or_accounts_the_window_says_so() {
    let mut w = Window::new();
    let mut facts = snapshot();
    facts["available"] = json!(false);
    w.answer("mail.snapshot", facts);
    let w = w.start(json!({}));
    assert!(w.find("unavailable").is_some() && w.asked("list").is_empty());

    let mut w = Window::new();
    let mut facts = snapshot();
    facts["config"] = json!({"accounts":[]});
    w.answer("mail.snapshot", facts);
    let w = w.start(json!({}));
    assert!(w.find("no-accounts").is_some() && w.asked("list").is_empty());
}

#[test]
fn every_caption_the_windows_use_exists_in_english() {
    let strings: Value = serde_json::from_str(include_str!("../../assets/locales/en/mail.json")).unwrap();
    let sources = [include_str!("ui.rs"), include_str!("compose.rs"), include_str!("window.rs"), include_str!("sheets.rs"), include_str!("mod.rs")];
    let pattern = regex::Regex::new(r#"(?:\.tp?\(|\.tp\(\s*)"([a-zA-Z0-9.]+)""#).unwrap();
    let mut seen = 0;
    for source in sources {
        for found in pattern.captures_iter(source) {
            let key = &found[1];
            assert!(!strings[key].is_null(), "missing caption: {key}");
            seen += 1;
        }
    }
    assert!(seen > 80, "{seen}");
}
