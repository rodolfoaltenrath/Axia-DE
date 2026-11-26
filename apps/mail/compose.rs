//! The compose window: From, To/Cc/Bcc with suggestions (contacts and people written to), the
//! subject, a Markdown body (with formatting buttons and a preview) or plain text, attachments,
//! the signature, drafts saved to the server, and sending now or later.
use super::editor::{Editor, Palette};
use super::ui::{button, click, clipped, field, on, rule, scroll, segmented, text, view, Looks, LINK, PRIMARY};
use super::{answer, civil_date, list_of, markdown, model, str_of, Base, Tag as AppTag};
use crate::native_view::KeyFacts;
use serde_json::{json, Value};

const AUTOSAVE_MS: i64 = 30_000;
const NOTICE_MS: i64 = 1500;
/// The window stays a moment on "Sent" before it closes.
const SENT_CLOSE_MS: i64 = 600;
const FIELDS: [&str; 3] = ["to", "cc", "bcc"];
const TOOLBAR: f64 = 46.;
const FROM: f64 = 32.;
const CHIP_LINE: f64 = 26.;
const SUBJECT: f64 = 34.;
const FORMATS: f64 = 36.;
const STATUS: f64 = 26.;
const LABEL: f64 = 54.;

pub(crate) enum Tag {
    Suggest { field: usize, generation: u64 },
    Draft { changes: u64, quiet: bool },
    Send { later: bool },
    Pick,
    /// The session's file dialog answered with a path.
    PickPath,
    Attached { id: u64 },
    Clipboard,
    Composition,
}

pub(crate) struct Attachment {
    id: u64,
    name: String,
    size: f64,
    /// `{file}` (a lease) or `{path}`; none while the shell is still opening it.
    source: Option<Value>,
}

#[derive(Default)]
pub(crate) struct Compose {
    pub account: String,
    /// What replying or an earlier save brought: inReplyTo, references, original, draftUid.
    draft: Value,
    pub subject: String,
    pub recipients: [Vec<Value>; 3],
    pub typed: [String; 3],
    pub suggestions: Vec<Value>,
    pub suggest_field: usize,
    suggest_index: usize,
    suggest_generation: u64,
    pub format: &'static str,
    pub editor: Editor,
    show_cc: bool,
    preview: bool,
    pub later: bool,
    pub attachments: Vec<Attachment>,
    next_attachment: u64,
    picking: bool,
    pub notice: String,
    notice_until: Option<i64>,
    pub error: String,
    changes: u64,
    saved: u64,
    saving: bool,
    save_again: bool,
    /// A send that waits for the draft being saved, so that it replaces the right one.
    send_after_save: Option<Option<i64>>,
    sending: bool,
    sent: bool,
    closing: bool,
    /// The last save failed: closing again gives the draft up.
    close_failed: bool,
    close_at: Option<i64>,
    autosave_at: i64,
    guard: Option<bool>,
    composition: Value,
    composition_pending: bool,
    input_context: bool,
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("")
}
fn field_of(key: &Value) -> Option<usize> {
    FIELDS.iter().position(|f| *key == *f)
}

impl Compose {
    pub fn new(base: &mut Base, draft: &Value, now: i64) -> Self {
        let accounts = base.accounts("mail");
        let wanted = str_of(&draft["acct"]);
        let account = accounts.iter().find(|a| a["id"] == wanted).or_else(|| accounts.first()).cloned().unwrap_or_default();
        let signature = if draft["noSignature"] == true { "" } else { str_of(&account["signature"]) };
        let mut compose = Compose {
            account: str_of(&account["id"]).to_owned(),
            subject: str_of(&draft["subject"]).to_owned(),
            format: match str_of(&draft["format"]) {
                "markdown" => "markdown",
                "plain" => "plain",
                _ => "rich",
            },
            preview: true,
            autosave_at: now + AUTOSAVE_MS,
            ..Compose::default()
        };
        for (i, name) in FIELDS.iter().enumerate() {
            compose.recipients[i] = list_of(&draft[*name]).to_vec();
        }
        compose.show_cc = !compose.recipients[1].is_empty() || !compose.recipients[2].is_empty();
        let mut kept = serde_json::Map::new();
        for key in ["inReplyTo", "references", "original", "draftUid", "draftFolder"] {
            if !draft[key].is_null() {
                kept.insert(key.into(), draft[key].clone());
            }
        }
        compose.draft = Value::Object(kept);
        compose.editor.reset(&model::initial_body(str_of(&draft["body"]), signature));
        // the cursor starts above the signature and the quoted original
        compose.editor.a = 0;
        compose.editor.b = 0;
        compose.editor.focused = !compose.recipients[0].is_empty();
        for item in list_of(&draft["attachments"]) {
            compose.attach(base, item);
        }
        compose
    }

    /// An attachment of the draft: a path, or a cached file of a forwarded message.
    fn attach(&mut self, base: &mut Base, item: &Value) {
        if self.attachments.len() >= model::MAX_ATTACHMENTS {
            return;
        }
        self.next_attachment += 1;
        let id = self.next_attachment;
        if let Some(path) = item.as_str() {
            self.attach_path(path);
            return;
        }
        let mut attachment = Attachment { id, name: str_of(&item["name"]).to_owned(), size: item["size"].as_f64().unwrap_or(0.), source: None };
        let cached = str_of(&item["cached"]);
        if cached.is_empty() {
            return;
        }
        if base.facts.isolated {
            base.service(AppTag::Compose(Tag::Attached { id }), "mail.attachmentFile", json!([cached]));
        } else if model::inside_cache(cached) {
            attachment.source = Some(json!({"path":format!("{}/{cached}", base.facts.cache_dir)}));
        } else {
            return;
        }
        self.attachments.push(attachment);
    }
    fn attach_path(&mut self, path: &str) {
        let source = json!({"path":path});
        if self.attachments.len() >= model::MAX_ATTACHMENTS || self.attachments.iter().any(|a| a.source.as_ref() == Some(&source)) {
            return;
        }
        self.next_attachment += 1;
        let size = std::fs::metadata(path).map_or(0., |m| m.len() as f64);
        self.attachments.push(Attachment { id: self.next_attachment, name: base_name(path).to_owned(), size, source: Some(source) });
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn dirty(&self) -> bool {
        self.changes != self.saved && !self.sent
    }
    fn touch(&mut self) {
        self.changes += 1;
        self.close_failed = false;
    }
    pub fn takes_text(&self) -> bool {
        self.editor.focused && !self.later
    }
    pub fn capturing(&self) -> bool {
        self.editor.drag.is_some()
    }
    pub fn blur(&mut self) {
        self.editor.focused = false;
        self.editor.drag = None;
    }
    pub fn paste(&mut self, pasted: &str, now: i64) {
        self.editor.insert(pasted, now);
        self.touch();
    }

    // ---------------------------------------------------------------- the message

    /// The request's fields and the files that go with it; none while a file is being opened.
    pub fn payload(&self) -> Option<(Value, Value)> {
        let body = &self.editor.body;
        let plain = self.format == "plain";
        let mut args = json!({
            "acct":self.account,"to":self.recipients[0],"cc":self.recipients[1],"bcc":self.recipients[2],"subject":self.subject,
            "text":if plain { body.clone() } else { model::markdown_text(body) },
            "attachments":self.attachments.iter().map(|a| json!({"name":a.name})).collect::<Vec<_>>(),
        });
        if !plain {
            args["html"] = json!(model::markdown_html(body));
        }
        for key in ["inReplyTo", "references", "original"] {
            if !self.draft[key].is_null() && self.draft[key] != "" {
                args[key] = self.draft[key].clone();
            }
        }
        if self.draft["draftUid"].as_u64().unwrap_or(0) > 0 {
            args["draftUid"] = self.draft["draftUid"].clone();
            args["draftFolder"] = self.draft["draftFolder"].clone();
        }
        let files: Option<Vec<Value>> = self.attachments.iter().map(|a| a.source.clone()).collect();
        Some((args, Value::Array(files?)))
    }

    pub fn save(&mut self, base: &mut Base, quiet: bool) {
        if self.account.is_empty() || self.sending || self.sent {
            return;
        }
        if self.saving {
            self.save_again = true;
            return;
        }
        let Some((mut args, files)) = self.payload() else { return };
        args["replace"] = json!(self.draft["draftUid"].as_u64().unwrap_or(0));
        args["format"] = json!(self.format);
        self.saving = true;
        base.request(AppTag::Compose(Tag::Draft { changes: self.changes, quiet }), "draft", args, files);
    }

    /// Addresses still being typed count too.
    fn commit_typed(&mut self) {
        for i in 0..3 {
            let typed = std::mem::take(&mut self.typed[i]);
            if !typed.trim().is_empty() {
                self.recipients[i].push(model::parse_recipient(&typed));
                self.touch();
            }
        }
        self.suggestions.clear();
    }

    pub fn send(&mut self, base: &mut Base, at: Option<i64>) {
        if self.sending || self.sent {
            return;
        }
        self.later = false;
        self.error.clear();
        self.commit_typed();
        let mut all = self.recipients.iter().flatten().peekable();
        if all.peek().is_none() {
            self.error = base.t("compose.noRecipients");
            return;
        }
        if let Some(bad) = all.find(|a| !model::email_ok(str_of(&a["email"]))) {
            let address = str_of(&bad["email"]).to_owned();
            self.error = base.tp("compose.badAddress", json!({"addr":address}));
            return;
        }
        if self.saving {
            self.send_after_save = Some(at);
            self.notice = base.t("compose.sending");
            return;
        }
        let Some((mut args, files)) = self.payload() else {
            self.error = base.t("working");
            return;
        };
        if let Some(at) = at {
            args["sendAt"] = json!(at);
        }
        self.sending = true;
        self.notice = base.t("compose.sending");
        self.notice_until = None;
        base.request(AppTag::Compose(Tag::Send { later: at.is_some() }), "send", args, files);
    }

    fn close(&mut self, base: &mut Base) {
        let window = base.window.clone();
        base.service(AppTag::Report, "windows.close", json!([window]));
    }

    /// The window was asked to close with changes: they are saved as a draft first.
    pub fn close_requested(&mut self, base: &mut Base, _now: i64) {
        if self.sending {
            return;
        }
        if !self.dirty() || self.close_failed || self.account.is_empty() {
            self.close(base);
            return;
        }
        self.closing = true;
        self.save(base, true);
    }

    pub fn sync_shell(&mut self, base: &mut Base) {
        if base.window.is_empty() {
            return;
        }
        let dirty = self.dirty();
        if self.guard != Some(dirty) {
            self.guard = Some(dirty);
            let window = base.window.clone();
            base.service(AppTag::Report, "windows.closeGuard", json!([window, dirty]));
        }
        let context = base.focused && self.takes_text();
        if context != self.input_context {
            self.input_context = context;
            if !context {
                self.composition = Value::Null;
            }
            base.service(AppTag::Ignore, "inputMethod.context", json!([context]));
        }
    }

    pub fn tick(&mut self, base: &mut Base, now: i64) {
        if now >= self.autosave_at {
            self.autosave_at = now + AUTOSAVE_MS;
            if self.dirty() && !self.saving {
                self.save(base, true);
            }
        }
        if self.notice_until.is_some_and(|at| now >= at) {
            self.notice_until = None;
            self.notice.clear();
        }
        if self.close_at.is_some_and(|at| now >= at) {
            self.close_at = None;
            self.close(base);
        }
        if self.input_context && !self.composition_pending {
            self.composition_pending = true;
            base.service(AppTag::Compose(Tag::Composition), "inputMethod.state", json!([]));
        }
    }

    pub fn done(&mut self, base: &mut Base, tag: Tag, result: Result<Value, String>, now: i64) {
        match tag {
            Tag::Suggest { field, generation } => {
                if generation != self.suggest_generation {
                    return;
                }
                let Ok(v) = answer(result) else { return };
                let mut seen: Vec<String> = self.recipients[field].iter().map(|a| str_of(&a["email"]).to_lowercase()).collect();
                self.suggestions = list_of(&v["items"])
                    .iter()
                    .filter(|x| {
                        let email = str_of(&x["email"]).to_lowercase();
                        let new = !email.is_empty() && !seen.contains(&email);
                        seen.push(email);
                        new
                    })
                    .take(8)
                    .cloned()
                    .collect();
                self.suggest_field = field;
                self.suggest_index = 0;
            }
            Tag::Draft { changes, quiet } => {
                self.saving = false;
                match answer(result) {
                    Ok(v) => {
                        self.saved = changes;
                        if v["uid"].as_u64().unwrap_or(0) > 0 {
                            self.draft["draftUid"] = v["uid"].clone();
                            if str_of(&self.draft["draftFolder"]).is_empty() {
                                self.draft["draftFolder"] = json!("Drafts");
                            }
                        }
                        if !quiet {
                            self.notice = base.t("compose.draftSaved");
                            self.notice_until = Some(now + NOTICE_MS);
                        }
                        if let Some(at) = self.send_after_save.take() {
                            self.save_again = false;
                            self.send(base, at);
                            return;
                        }
                        if self.closing && !self.dirty() {
                            self.close(base);
                            return;
                        }
                        if std::mem::take(&mut self.save_again) || self.closing {
                            self.save(base, quiet);
                        }
                    }
                    Err(failure) => {
                        self.save_again = false;
                        if let Some(at) = self.send_after_save.take() {
                            self.send(base, at);
                            return;
                        }
                        if self.closing {
                            self.closing = false;
                            self.close_failed = true;
                        }
                        if !quiet || self.close_failed {
                            self.error = failure.message;
                        }
                    }
                }
            }
            Tag::Send { later } => {
                self.sending = false;
                match answer(result) {
                    Ok(v) => {
                        self.sent = true;
                        let scheduled = v["scheduled"] == true || v["scheduled"] == 1 || later && v["scheduled"].is_null();
                        self.notice = base.t(if scheduled { "compose.scheduled" } else { "compose.sent" });
                        self.close_at = Some(now + SENT_CLOSE_MS);
                    }
                    Err(failure) => {
                        self.notice.clear();
                        self.error = format!("{} {}", base.t("compose.failed"), failure.message);
                    }
                }
            }
            // the system dialog of the isolated app: this process receives only the chosen file
            Tag::Pick => {
                self.picking = false;
                match result {
                    Ok(v) if v["file"].is_u64() => {
                        if self.attachments.len() < model::MAX_ATTACHMENTS {
                            self.next_attachment += 1;
                            self.attachments.push(Attachment {
                                id: self.next_attachment,
                                name: str_of(&v["name"]).to_owned(),
                                size: v["size"].as_f64().unwrap_or(0.),
                                source: Some(json!({"file":v["file"]})),
                            });
                            self.touch();
                        }
                    }
                    Ok(_) => {}
                    Err(error) => self.error = error,
                }
            }
            Tag::PickPath => {
                self.picking = false;
                // one path, or several of them
                let picked = result.unwrap_or_default();
                let paths = picked.as_str().into_iter().chain(list_of(&picked).iter().filter_map(Value::as_str));
                let before = self.attachments.len();
                for path in paths {
                    self.attach_path(path);
                }
                if self.attachments.len() != before {
                    self.touch();
                }
            }
            Tag::Attached { id } => {
                let file = result.ok().filter(|v| v["file"].is_u64());
                match file {
                    Some(v) => {
                        if let Some(a) = self.attachments.iter_mut().find(|a| a.id == id) {
                            a.source = Some(json!({"file":v["file"]}));
                        } else {
                            base.service(AppTag::Ignore, "mail.releaseFile", json!([v["file"]]));
                        }
                    }
                    None => self.attachments.retain(|a| a.id != id),
                }
            }
            Tag::Clipboard => {
                if let Some(pasted) = result.ok().as_ref().and_then(|v| v["text"].as_str()) {
                    if !pasted.is_empty() {
                        self.paste(pasted, now);
                    }
                }
            }
            Tag::Composition => {
                self.composition_pending = false;
                if let Ok(state) = result {
                    if self.takes_text() {
                        self.composition = state;
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- input

    fn add_recipient(&mut self, field: usize, address: Value) -> bool {
        if !model::email_ok(str_of(&address["email"])) {
            return false;
        }
        self.recipients[field].push(address);
        self.typed[field].clear();
        self.suggestions.clear();
        self.suggest_generation += 1;
        self.touch();
        true
    }
    fn suggestion(&self, field: usize) -> Option<Value> {
        if self.suggest_field != field {
            return None;
        }
        self.suggestions.get(self.suggest_index).map(|s| model::address(str_of(&s["name"]), str_of(&s["email"])))
    }
    /// Enter or Tab: the highlighted suggestion, or what was typed.
    fn accept_recipient(&mut self, field: usize) {
        let chosen = self.suggestion(field).or_else(|| Some(model::parse_recipient(&self.typed[field])).filter(|_| !self.typed[field].trim().is_empty()));
        if let Some(address) = chosen {
            self.add_recipient(field, address);
        }
    }
    fn recipient_typed(&mut self, base: &mut Base, field: usize, value: &str) {
        if let Some(before) = value.strip_suffix([',', ';']) {
            if !self.add_recipient(field, model::parse_recipient(before)) {
                self.typed[field] = before.to_owned();
            }
            return;
        }
        self.typed[field] = value.to_owned();
        self.suggest_generation += 1;
        base.jobs.cancel(|tag| matches!(tag, AppTag::Compose(Tag::Suggest { .. })));
        let query = value.trim();
        if query.is_empty() {
            self.suggestions.clear();
        } else {
            let tag = AppTag::Compose(Tag::Suggest { field, generation: self.suggest_generation });
            base.ask(tag, "autocomplete", json!({"q":query}));
        }
    }
    fn recipient_key(&mut self, field: usize, key: &str) {
        let count = if self.suggest_field == field { self.suggestions.len() } else { 0 };
        match key {
            "ArrowDown" if count > 0 => self.suggest_index = (self.suggest_index + 1) % count,
            "ArrowUp" if count > 0 => self.suggest_index = (self.suggest_index + count - 1) % count,
            "Tab" => self.accept_recipient(field),
            "Backspace" if self.typed[field].is_empty() && self.recipients[field].pop().is_some() => self.touch(),
            _ => {}
        }
    }

    pub fn command(&mut self, base: &mut Base, command: &str, v: &Value, now: i64) {
        let key = &v["key"];
        let value = str_of(&v["value"]);
        match command {
            "send" => self.send(base, None),
            "later" => self.later = !self.later,
            "sendLater" => {
                let at = match str_of(key) {
                    "in1h" => now + 3_600_000,
                    "tomorrow8" => civil_date::at_minutes(civil_date::add_days(now, 1), 8 * 60),
                    "monday8" => {
                        // the next Monday; a week ahead when today is Monday
                        let days = match (8 - crate::clock::local(now).tm_wday) % 7 {
                            0 => 7,
                            n => n,
                        };
                        civil_date::at_minutes(civil_date::add_days(now, days), 8 * 60)
                    }
                    _ => return,
                };
                self.send(base, Some(at));
            }
            "saveDraft" => self.save(base, false),
            "attach" => {
                if self.picking || self.attachments.len() >= model::MAX_ATTACHMENTS {
                    return;
                }
                self.picking = true;
                if base.facts.isolated {
                    base.service(AppTag::Compose(Tag::Pick), "mail.pickFile", json!([]));
                } else {
                    let title = base.t("compose.attachTitle");
                    base.service(AppTag::Compose(Tag::PickPath), "files.pick", json!([{"title":title,"multiple":true}]));
                }
            }
            "detach" => {
                let id = key.as_u64().unwrap_or(0);
                if let Some(position) = self.attachments.iter().position(|a| a.id == id) {
                    let attachment = self.attachments.remove(position);
                    if let Some(file) = attachment.source.as_ref().and_then(|s| s["file"].as_u64()) {
                        base.service(AppTag::Ignore, "mail.releaseFile", json!([file]));
                    }
                    self.touch();
                }
            }
            "format" => {
                self.format = match str_of(key) {
                    "markdown" => "markdown",
                    "plain" => "plain",
                    _ => "rich",
                };
                self.touch();
            }
            "preview" => self.preview = !self.preview,
            "style" => {
                self.editor.focused = true;
                self.editor.format(str_of(key), now);
                self.touch();
            }
            "from" => {
                if let Some(id) = key.as_str().filter(|id| base.account(id).is_some()) {
                    self.account = id.to_owned();
                    // a draft saved with the other account stays there
                    if let Some(draft) = self.draft.as_object_mut() {
                        draft.remove("draftUid");
                        draft.remove("draftFolder");
                    }
                    self.touch();
                }
            }
            "showCc" => self.show_cc = true,
            "subject" => {
                self.subject = value.to_owned();
                self.touch();
            }
            "recipient" => {
                if let Some(i) = field_of(key) {
                    self.recipient_typed(base, i, value);
                }
            }
            "recipientSubmit" => {
                if let Some(i) = field_of(key) {
                    self.accept_recipient(i);
                }
            }
            "toKey" | "ccKey" | "bccKey" => {
                if let Some(i) = FIELDS.iter().position(|f| command.strip_suffix("Key") == Some(*f)) {
                    self.recipient_key(i, str_of(key));
                }
            }
            "suggest" => {
                let (field, index) = (field_of(&key[0]), key[1].as_u64().unwrap_or(0) as usize);
                if let Some(field) = field.filter(|f| *f == self.suggest_field && index < self.suggestions.len()) {
                    self.suggest_index = index;
                    self.accept_recipient(field);
                }
            }
            "removeRecipient" => {
                let (field, index) = (field_of(&key[0]), key[1].as_u64().unwrap_or(0) as usize);
                if let Some(field) = field.filter(|f| index < self.recipients[*f].len()) {
                    self.recipients[field].remove(index);
                    self.touch();
                }
            }
            "editorDown" => {
                if let [Some(x), Some(y), Some(w), Some(h)] = [0, 1, 2, 3].map(|i| v["frame"][i].as_f64()) {
                    (self.editor.x, self.editor.y, self.editor.width, self.editor.height) = (x, y, w, h);
                    self.editor.layout();
                }
                self.editor.down(v["x"].as_f64().unwrap_or(0.), v["y"].as_f64().unwrap_or(0.), v["shift"] == true, now);
            }
            "editorWheel" => {
                self.editor.scroll = (self.editor.scroll + v["deltaY"].as_f64().unwrap_or(0.)).clamp(0., self.editor.max_scroll());
            }
            "acceptEmoji" => {
                self.editor.accept(key.as_u64().unwrap_or(0) as usize, now);
                self.touch();
            }
            "pointerMove" => self.editor.drag_to(v["x"].as_f64().unwrap_or(0.), v["y"].as_f64().unwrap_or(0.)),
            "dragEnd" => self.editor.drag = None,
            _ => {}
        }
    }

    fn edit_key(&mut self, base: &mut Base, key: &str, chr: &str, flags: i32, now: i64) {
        let before = self.editor.body.clone();
        if let Some(reply) = self.editor.key(key, chr, flags, now) {
            if let Some(copied) = reply["copy"].as_str() {
                base.copy(copied);
            }
            if reply["paste"] == true {
                base.service(AppTag::Compose(Tag::Clipboard), "clipboard.read", json!([]));
            }
            if reply["escape"] == true {
                self.editor.focused = false;
            }
        }
        if self.editor.body != before {
            self.touch();
        }
    }

    pub fn key(&mut self, base: &mut Base, key: &str, chr: &str, flags: i32, facts: &KeyFacts, now: i64) -> bool {
        let command_key = flags & (2 | 16) != 0;
        let letter = if facts.base.is_empty() { key } else { facts.base };
        if command_key && key == "Enter" {
            self.send(base, None);
            return true;
        }
        if command_key && letter.eq_ignore_ascii_case("s") {
            self.save(base, false);
            return true;
        }
        if self.later && key == "Escape" {
            self.later = false;
            return true;
        }
        if !self.takes_text() {
            return false;
        }
        // the shell reports Shift apart from the other modifiers
        self.edit_key(base, key, chr, flags | i32::from(facts.shift), now);
        true
    }

    // ---------------------------------------------------------------- drawing

    /// The height of a recipient row: its chips wrap before the text field.
    fn row_height(&self, field: usize, width: f64) -> f64 {
        let available = (width - LABEL - 30.).max(100.);
        let (mut lines, mut used) = (1, 0.);
        for address in &self.recipients[field] {
            let chip = crate::native_view::measure_text(model::address_short(address), 12., 400.) + 38.;
            if used + chip > available && used > 0. {
                lines += 1;
                used = 0.;
            }
            used += chip + 4.;
        }
        if used + 140. > available && used > 0. {
            lines += 1;
        }
        f64::from(lines) * CHIP_LINE + 6.
    }

    fn recipient_row(&mut self, base: &mut Base, field_index: usize, height: f64) -> Value {
        let name = FIELDS[field_index];
        let dim = base.token("textDim", "#8e8e96");
        let label = base.t(&format!("compose.{name}"));
        let mut chips: Vec<Value> = self.recipients[field_index]
            .iter()
            .enumerate()
            .map(|(i, address)| {
                let good = model::email_ok(str_of(&address["email"]));
                let background = if good { base.token("control", "rgba(127,127,127,0.2)") } else { "#ff453a55".to_owned() };
                view(&format!("chip-{name}-{i}"), json!({"flexDirection":"row","alignItems":"center","gap":4,"paddingHorizontal":8,"height":22,"borderRadius":11,"backgroundColor":background}), vec![
                    text(model::address_short(address), json!({"fontSize":12,"color":base.token("text", "#ececf0"),"whiteSpace":"nowrap"})),
                    click(view("remove", json!({"paddingHorizontal":2}), vec![text("✕", json!({"fontSize":11,"color":dim}))]), json!(["removeRecipient", [name, i]])),
                ])
            })
            .collect();
        let mut input = field(&format!("recipient-{name}"), &self.typed[field_index], "", json!(["recipient", name]));
        input["submit"] = json!(["recipientSubmit", name]);
        input["keepFocus"] = json!(true);
        input["style"] = json!({"flex":1,"minWidth":140,"height":24,"borderWidth":0,"backgroundColor":"transparent"});
        // only the keys that mean something now: the field keeps the others (Backspace edits it)
        let mut keys = Vec::new();
        if self.suggest_field == field_index && !self.suggestions.is_empty() {
            keys.extend(["ArrowDown", "ArrowUp"]);
        }
        if !self.typed[field_index].trim().is_empty() {
            keys.push("Tab");
        } else if !self.recipients[field_index].is_empty() {
            keys.push("Backspace");
        }
        input["keys"] = json!(keys);
        input["onKey"] = json!([format!("{name}Key")]);
        if field_index == 0 && self.recipients[0].is_empty() {
            input["autoFocus"] = json!(true);
        }
        chips.push(input);
        view(&format!("recipients-{name}"), json!({"height":height,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":12}), vec![
            text(label, json!({"width":LABEL,"fontSize":12,"color":dim})),
            view("chips", json!({"flex":1,"minWidth":0,"flexDirection":"row","flexWrap":"wrap","alignItems":"center","gap":4}), chips),
        ])
    }

    fn suggestion_list(&mut self, base: &mut Base, top: f64) -> Value {
        let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
        let contacts = base.t("compose.fromContacts");
        let name = FIELDS[self.suggest_field];
        let rows = self
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let here = i == self.suggest_index;
                let email = str_of(&s["email"]);
                let second = if s["src"] == "contacts" { format!("{email} · {contacts}") } else { email.to_owned() };
                let first = match str_of(&s["name"]) {
                    "" => email,
                    name => name,
                };
                on(view(&format!("suggestion-{i}"), json!({"padding":6,"borderRadius":6,"backgroundColor":if here { base.token("accent", "#0a84ff") } else { "transparent".to_owned() }}), vec![
                    clipped(text(first, json!({"fontSize":13,"color":if here { "#ffffff" } else { &foreground }}))),
                    clipped(text(second, json!({"fontSize":11,"color":if here { "#ffffffcc" } else { &dim }}))),
                ]), "onMouseDown", json!(["suggest", [name, i]]))
            })
            .collect();
        view("suggestions", json!({"position":"absolute","start":66,"top":top,"width":360,"padding":4,"borderRadius":8,"backgroundColor":base.token("windowBg", "#242428"),
            "borderWidth":1,"borderColor":base.token("separator", "#3a3a40"),"boxShadow":"0 10px 26px rgba(0,0,0,0.35)"}), rows)
    }

    fn format_bar(&mut self, base: &mut Base) -> Value {
        let color = base.token("text", "#ececf0");
        let control = base.token("control", "rgba(127,127,127,0.2)");
        let tool = |kind: &str, label: &str, weight: u32| {
            let mut kids = vec![text(label, json!({"fontSize":13,"fontWeight":weight,"color":color}))];
            if kind == "strike" {
                // there is no strike-through style: a hairline across the letter
                kids.push(view("line", json!({"position":"absolute","left":9,"right":9,"top":13,"height":1,"backgroundColor":color}), vec![]));
            }
            click(view(&format!("style-{kind}"), json!({"minWidth":28,"height":26,"paddingHorizontal":6,"borderRadius":6,"alignItems":"center","justifyContent":"center","backgroundColor":control}), kids),
                json!(["style", kind]))
        };
        let mut kids = vec![
            tool("bold", "B", 700),
            tool("italic", "I", 500),
            tool("strike", "S", 500),
            tool("bullet", "• ≡", 500),
            tool("num", "1. ≡", 500),
            tool("quote", "❝", 500),
            tool("webLink", "🔗", 500),
            tool("inlineCode", "</>", 500),
            view("format-space", json!({"flex":1}), vec![]),
        ];
        if self.format == "rich" {
            let label = base.t(if self.preview { "compose.hidePreview" } else { "compose.showPreview" });
            kids.push(button("toggle-preview", &label, json!(["preview"]), LINK));
        }
        view("formats", json!({"height":FORMATS,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":4,"paddingHorizontal":12}), kids)
    }

    pub fn tree(&mut self, base: &mut Base, now: i64) -> Value {
        let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
        let separator = base.token("separator", "#3a3a40");
        let accent = base.token("accent", "#0a84ff");
        let width = base.width;
        let accounts = base.accounts("mail");

        let (send, later, attach, save) = (base.t("compose.send"), base.t("compose.later"), base.t("compose.attach"), base.t("compose.saveDraft"));
        let (rich, markdown_label, plain) = (base.t("compose.rich"), base.t("compose.markdown"), base.t("compose.plain"));
        let mut formats = segmented(base, "format-choice", &[
            (json!(["format", "rich"]), &rich),
            (json!(["format", "markdown"]), &markdown_label),
            (json!(["format", "plain"]), &plain),
        ], ["rich", "markdown", "plain"].iter().position(|f| *f == self.format).unwrap_or(0));
        formats["style"]["alignSelf"] = json!("center");
        let busy = Looks { disabled: self.sending || self.sent, ..PRIMARY };
        let toolbar = view("compose-toolbar", json!({"height":TOOLBAR,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12}), vec![
            button("send", &format!("➤ {send}"), json!(["send"]), busy),
            button("send-later", &later, json!(["later"]), Looks { disabled: busy.disabled, ..Looks::default() }),
            button("attach", &format!("📎 {attach}"), json!(["attach"]), Looks::default()),
            button("save-draft", &save, json!(["saveDraft"]), Looks::default()),
            view("toolbar-space", json!({"flex":1}), vec![]),
            formats,
        ]);
        let mut kids = vec![toolbar, rule(base, "toolbar-rule")];
        let mut top = TOOLBAR + 1.;
        if accounts.len() > 1 {
            let from = base.t("compose.from");
            let mut chips = vec![text(from, json!({"width":LABEL,"fontSize":12,"color":dim}))];
            for account in &accounts {
                let id = str_of(&account["id"]);
                let here = id == self.account;
                let background = if here { accent.clone() } else { base.token("control", "rgba(127,127,127,0.2)") };
                chips.push(click(view(&format!("from-{id}"), json!({"paddingHorizontal":8,"height":22,"borderRadius":11,"justifyContent":"center","backgroundColor":background}), vec![
                    text(str_of(&account["email"]), json!({"fontSize":12,"whiteSpace":"nowrap","color":if here { "#ffffff" } else { &foreground }})),
                ]), json!(["from", id])));
            }
            kids.push(view("from", json!({"height":FROM,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":12,"overflow":"hidden"}), chips));
            kids.push(rule(base, "from-rule"));
            top += FROM + 1.;
        }
        let cc_top = top;
        let shown = if self.show_cc { 3 } else { 1 };
        let mut suggestions_top = 0.;
        for i in 0..shown {
            let height = self.row_height(i, width);
            kids.push(self.recipient_row(base, i, height));
            kids.push(rule(base, &format!("recipients-rule-{i}")));
            top += height + 1.;
            if i == self.suggest_field {
                suggestions_top = top - 1.;
            }
        }
        let subject_label = base.t("compose.subject");
        let mut subject = field("subject", &self.subject, "", json!(["subject"]));
        subject["style"] = json!({"flex":1,"minWidth":0,"height":26,"borderWidth":0,"backgroundColor":"transparent"});
        kids.push(view("subject-row", json!({"height":SUBJECT,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":12}), vec![
            text(subject_label, json!({"width":LABEL,"fontSize":12,"color":dim})),
            subject,
        ]));
        kids.push(rule(base, "subject-rule"));
        top += SUBJECT + 1.;
        if self.format != "plain" {
            kids.push(self.format_bar(base));
            top += FORMATS;
        }
        let preview = self.format == "rich" && self.preview;
        let attachment_height = if self.attachments.is_empty() { 0. } else { 48. };
        let status = if !self.error.is_empty() { Some((self.error.clone(), true)) } else { Some((self.notice.clone(), false)).filter(|(n, _)| !n.is_empty()) };
        let bottom = attachment_height + if status.is_some() { STATUS } else { 0. };
        let editor_width = if preview { ((width - 1.) / 2.).floor() } else { width };
        self.editor.palette = Palette {
            text: foreground.clone(),
            heading: foreground.clone(),
            code: accent.clone(),
            quote: dim.clone(),
            caret: accent.clone(),
            selection: "rgba(10,132,255,0.28)".into(),
            popup: base.token("windowBg", "#242428"),
            popup_text: foreground.clone(),
            border: separator.clone(),
        };
        self.editor.x = if preview && base.rtl { width - editor_width } else { 0. };
        self.editor.y = top;
        self.editor.width = editor_width;
        self.editor.height = (base.height - top - bottom).max(40.);
        self.editor.padding = 14.;
        self.editor.size = 14.;
        self.editor.layout();
        let mut body = self.editor.render(now);
        body["key"] = json!("body-editor");
        let mut columns = vec![view("body-column", json!({"width":editor_width,"flexShrink":0,"flexDirection":"column"}), vec![body])];
        if preview {
            columns.push(view("body-divider", json!({"width":1,"flexShrink":0,"backgroundColor":separator}), vec![]));
            let rendered = markdown::render(&markdown::parse(&self.editor.body), (width - editor_width - 30.).max(100.), &base.theme);
            let paper = if base.dark { "rgba(0,0,0,0.13)" } else { "#f7f7f7" };
            columns.push(view("preview", json!({"flex":1,"minWidth":0,"backgroundColor":paper}), vec![
                scroll("preview-scroll", json!({"flex":1,"minHeight":0}), vec![view("preview-content", json!({"padding":14}), vec![rendered])]),
            ]));
        }
        kids.push(view("body", json!({"flex":1,"minHeight":0,"flexDirection":"row"}), columns));
        if !self.attachments.is_empty() {
            let cards = self
                .attachments
                .iter()
                .map(|a| {
                    let size = if a.source.is_some() { model::format_size(a.size) } else { base.t("working") };
                    view(&format!("file-{}", a.id), json!({"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":10,"height":28,"borderRadius":8,"flexShrink":1,"minWidth":0,
                        "backgroundColor":base.token("control", "rgba(127,127,127,0.2)")}), vec![
                        clipped(text(format!("📄 {}", a.name), json!({"fontSize":12,"flexShrink":1,"color":foreground}))),
                        text(size, json!({"fontSize":11,"color":dim,"whiteSpace":"nowrap"})),
                        click(view("detach", json!({"paddingHorizontal":2}), vec![text("✕", json!({"fontSize":11,"color":dim}))]), json!(["detach", a.id])),
                    ])
                })
                .collect();
            kids.push(view("files", json!({"height":attachment_height,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":10,"overflow":"hidden"}), cards));
        }
        if let Some((message, failed)) = status {
            let background = if failed { "#ff453a22".to_owned() } else { base.token("control", "rgba(127,127,127,0.2)") };
            kids.push(view("compose-status", json!({"height":STATUS,"flexShrink":0,"justifyContent":"center","paddingHorizontal":12,"backgroundColor":background}), vec![
                clipped(text(message, json!({"fontSize":12,"color":if failed { "#ff453a" } else { &foreground }}))),
            ]));
        }
        if !self.show_cc {
            let label = base.t("compose.ccBcc");
            kids.push(click(view("show-cc", json!({"position":"absolute","end":14,"top":cc_top + 8.}), vec![text(label, json!({"fontSize":12,"color":accent}))]), json!(["showCc"])));
        }
        if self.later {
            let options = [("in1h", base.t("compose.in1h")), ("tomorrow8", base.t("compose.tomorrow8")), ("monday8", base.t("compose.monday8"))];
            let rows = options
                .into_iter()
                .map(|(id, label)| {
                    let mut node = click(view(id, json!({"padding":6,"borderRadius":6}), vec![text(label, json!({"fontSize":13,"color":foreground}))]), json!(["sendLater", id]));
                    node["hoverStyle"] = json!({"backgroundColor":base.token("hover", "rgba(127,127,127,0.14)")});
                    node
                })
                .collect();
            kids.push(view("later-menu", json!({"position":"absolute","start":110,"top":TOOLBAR - 2.,"padding":6,"gap":2,"borderRadius":8,"backgroundColor":base.token("windowBg", "#242428"),
                "borderWidth":1,"borderColor":separator,"boxShadow":"0 10px 26px rgba(0,0,0,0.35)"}), rows));
        }
        if !self.suggestions.is_empty() && self.suggest_field < shown {
            kids.push(self.suggestion_list(base, suggestions_top));
        }
        if let Some(node) = self.composition_view(base) {
            kids.push(node);
        }
        view("compose", json!({"flex":1,"position":"relative","overflow":"hidden","backgroundColor":base.token("windowBg", "#242428")}), kids)
    }

    /// The input method's composition and candidates, under the caret.
    fn composition_view(&self, base: &Base) -> Option<Value> {
        let preedit = str_of(&self.composition["preedit"]);
        let candidates = list_of(&self.composition["candidates"]);
        if !self.takes_text() || preedit.is_empty() && candidates.is_empty() {
            return None;
        }
        let foreground = base.token("text", "#ececf0");
        let selected = self.composition["selected"].as_u64().unwrap_or(0) as usize;
        let mut kids = Vec::new();
        if !preedit.is_empty() {
            kids.push(text(preedit, json!({"fontSize":15,"color":foreground})));
        }
        let first = selected / 9 * 9;
        let list: Vec<Value> = candidates
            .iter()
            .enumerate()
            .skip(first)
            .take(9)
            .map(|(i, word)| {
                text(format!("{} {}", i - first + 1, str_of(word)), json!({"fontSize":14,"padding":4,"borderRadius":4,
                    "backgroundColor":if i == selected { base.token("accent", "#0a84ff") } else { "transparent".to_owned() },
                    "color":if i == selected { "#ffffff" } else { &foreground }}))
            })
            .collect();
        if !list.is_empty() {
            kids.push(view("candidates", json!({"flexDirection":"row","gap":2}), list));
        }
        let e = &self.editor;
        let x = (e.x + e.padding + e.caret_x(e.b)).clamp(0., (base.width - 260.).max(0.));
        let y = (e.y + e.padding + (e.row_of(e.b) + 1) as f64 * e.lh() - e.scroll).clamp(0., (base.height - 60.).max(0.));
        Some(view("composition", json!({"position":"absolute","left":x,"top":y,"padding":6,"gap":4,"borderRadius":6,"pointerEvents":"none",
            "backgroundColor":base.token("windowBg", "#242428"),"borderWidth":1,"borderColor":base.token("separator", "#3a3a40")}), kids))
    }
}
