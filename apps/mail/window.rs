//! The three-pane window: folders, the virtual message list and the reading pane, with the actions
//! on messages. It holds what was asked for and what came back; `ui.rs` draws it.
use super::html::{Picture, Wanted};
use super::model::{self, Reply, Source};
use super::sheets::Sheet;
use super::{answer, list_of, str_of, Base, Tag as AppTag};
use crate::native_view::KeyFacts;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const ROW: f64 = model::ROW;
/// How long a message stays open before it counts as read.
const SEEN_AFTER_MS: i64 = 1200;
/// Mail events arrive in bursts; the folders and the list are read once for each.
const RELOAD_AFTER_MS: i64 = 400;
/// The shell refuses more than 32 jobs of a window at once.
const PICTURES_AT_ONCE: usize = 6;

pub(crate) enum Tag {
    Folders,
    List { generation: u64, first: usize },
    Open { key: String },
    Thread { key: String },
    /// Another message of the open conversation.
    Sibling { key: String },
    Invite { key: String },
    Answer { key: String, partstat: &'static str },
    Picture { message: String, source: String },
    /// The cached file of an attachment, to open or to save.
    Attachment { name: String, save: bool },
    Attached,
    /// One of the files a forwarded message takes along.
    Forward { generation: u64, slot: usize },
    CreateArchive { targets: Value },
    Moved,
    Deleted { next: Option<Value> },
    /// What the selected row looks like after a flag change.
    Patch { key: String, patch: Value },
    Marked,
}

#[derive(Default)]
pub(crate) struct List {
    /// The source the rows belong to.
    pub source: Option<Source>,
    version: Value,
    pub total: usize,
    pub items: BTreeMap<usize, Value>,
    pub loading: bool,
    generation: u64,
    pending: bool,
    /// What was asked last (source, first row, count, revision): never asked twice in a row.
    asked: Option<(Source, usize, usize, u64)>,
}

#[derive(Default)]
pub(crate) struct Invite {
    pub data: Option<Value>,
    /// The answer given here ("ACCEPTED"…) or the error that came back.
    pub done: String,
    pub error: String,
    pub busy: bool,
}

struct Forward {
    generation: u64,
    draft: Value,
    files: Vec<Option<Value>>,
    sources: Vec<(Value, String, f64)>,
    next: usize,
}

#[derive(Default)]
pub(crate) struct Main {
    pub folders: Vec<Value>,
    folders_pending: bool,
    folders_stale: bool,
    loaded_folders: bool,
    pub source: Source,
    pub query: String,
    /// Changes to make the search field forget what was typed.
    pub search_field: u64,
    revision: u64,
    pub list: List,
    pub offset: f64,
    bar_drag: Option<(f64, f64)>,
    /// The row that was opened.
    pub selected: Option<Value>,
    pub message: Option<Value>,
    pub thread: Vec<Value>,
    pub expanded: BTreeSet<String>,
    pub siblings: BTreeMap<String, Value>,
    pub load_error: String,
    /// Messages whose remote images the user asked for.
    pub remote: BTreeSet<String>,
    granted: BTreeSet<String>,
    pub pictures: BTreeMap<String, HashMap<String, Picture>>,
    pub invites: BTreeMap<String, Invite>,
    /// The attachment being fetched: message and index.
    pub attaching: Option<(String, i64)>,
    seen: Vec<(i64, Value)>,
    reload_at: Option<i64>,
    forward: Option<Forward>,
    forwards: u64,
    pub sheet: Sheet,
    /// Certificate warnings that were closed: they stay behind their account's ⚠.
    dismissed: BTreeSet<String>,
    arguments: Value,
}

fn fingerprint(certificate: &Value) -> String {
    format!("{}|{}", str_of(&certificate["acct"]), str_of(&certificate["sha256"]))
}

/// The list's height: the window without the toolbar, the title, the search field and the rule.
pub(crate) fn list_height(base: &Base) -> f64 {
    (base.height - super::ui::TOOLBAR - super::ui::LIST_TITLE - super::ui::SEARCH - 1.).max(ROW)
}

impl Main {
    pub fn capturing(&self) -> bool {
        self.bar_drag.is_some()
    }

    // ---------------------------------------------------------------- what arrives

    pub fn arguments(&mut self, base: &mut Base, args: &Value, now: i64) {
        let wanted = json!([args["acct"], args["folder"], args["uid"], args["nonce"]]);
        if wanted == self.arguments {
            return;
        }
        self.arguments = wanted;
        let (Some(account), Some(folder)) = (args["acct"].as_str(), args["folder"].as_str()) else { return };
        self.show(Source::Folder { account: account.into(), folder: folder.into() });
        if !args["uid"].is_null() {
            self.open(base, json!({"acct":account,"folder":folder,"uid":args["uid"]}), now);
        }
    }

    pub fn facts_changed(&mut self, base: &mut Base) {
        if !self.loaded_folders {
            self.loaded_folders = true;
            self.load_folders(base);
        }
        self.sheet.facts_changed(base);
    }

    pub fn mail_event(&mut self, event: &Value, now: i64) {
        if matches!(str_of(&event["ev"]), "changed" | "folders" | "status" | "ready") && self.reload_at.is_none() {
            self.reload_at = Some(now + RELOAD_AFTER_MS);
        }
    }

    pub fn tick(&mut self, base: &mut Base, now: i64) {
        if self.reload_at.is_some_and(|at| now >= at) {
            self.reload_at = None;
            self.load_folders(base);
            self.revision += 1;
        }
        while let Some(position) = self.seen.iter().position(|(at, _)| now >= *at) {
            let (_, targets) = self.seen.remove(position);
            base.ask(AppTag::Main(Tag::Marked), "flag", targets);
        }
        // a certificate that did not verify asks by itself, once nothing else is open
        if matches!(self.sheet, Sheet::None) {
            let waiting = list_of(&base.facts.status["certs"]).iter().find(|c| !self.dismissed.contains(&fingerprint(c)));
            if let Some(certificate) = waiting {
                self.sheet = Sheet::Certificate(certificate.clone());
            }
        }
        self.load_rows(base);
    }

    pub fn load_folders(&mut self, base: &mut Base) {
        if self.folders_pending {
            self.folders_stale = true;
            return;
        }
        self.folders_pending = true;
        base.ask(AppTag::Main(Tag::Folders), "folders", json!({}));
    }

    // ---------------------------------------------------------------- the list

    /// The mail accounts exist and the helper is there: the list has something to show.
    pub fn has_list(&self, base: &Base) -> bool {
        base.facts.ready && base.facts.available && !base.accounts("mail").is_empty()
    }

    fn load_rows(&mut self, base: &mut Base) {
        if self.list.pending || !self.has_list(base) {
            return;
        }
        let height = list_height(base);
        let first = ((self.offset / ROW).floor() as usize).saturating_sub(10);
        let last = ((self.offset + height) / ROW).ceil() as usize + 20;
        let fresh = self.list.source.as_ref() != Some(&self.source);
        let search = matches!(self.source, Source::Search(_));
        let shown = (self.offset / ROW).floor() as usize;
        let end = (((self.offset + height) / ROW).ceil() as usize + 1).min(self.list.total);
        let missing = !search && (shown..end).any(|i| !self.list.items.contains_key(&i));
        let stale = self.list.asked.as_ref().is_none_or(|(source, _, _, revision)| *source != self.source || *revision != self.revision);
        if !(missing || stale) {
            return;
        }
        let (first, count) = if search { (0, 1000) } else { (first, last - first) };
        let asked = (self.source.clone(), first, count, self.revision);
        if self.list.asked.as_ref() == Some(&asked) {
            return;
        }
        let (op, args) = self.source.request(first, count);
        self.list.asked = Some(asked);
        self.list.pending = true;
        self.list.loading |= fresh;
        base.ask(AppTag::Main(Tag::List { generation: self.list.generation, first }), op, args);
    }

    pub fn show(&mut self, source: Source) {
        if !matches!(source, Source::Search(_)) && !self.query.is_empty() {
            self.query.clear();
            self.search_field += 1;
        }
        if source == self.source {
            return;
        }
        self.source = source;
        self.offset = 0.;
        self.load_error.clear();
        self.list.generation += 1;
        self.list.pending = false;
        self.list.asked = None;
        self.list.loading = true;
    }

    fn rows_arrived(&mut self, first: usize, reply: &Value) {
        let search = matches!(self.source, Source::Search(_));
        let same = self.list.source.as_ref() == Some(&self.source) && self.list.version == reply["version"];
        if search || !same {
            self.list.items.clear();
        }
        let offset = if search { 0 } else { first };
        for (i, item) in list_of(&reply["items"]).iter().enumerate() {
            self.list.items.insert(offset + i, item.clone());
        }
        let total = reply["total"].as_u64().map_or(self.list.items.len(), |n| n as usize);
        self.list.total = if search { self.list.items.len() } else { total };
        self.list.version = reply["version"].clone();
        self.list.source = Some(self.source.clone());
    }

    pub fn max_offset(&self, base: &Base) -> f64 {
        (self.list.total as f64 * ROW - list_height(base)).max(0.)
    }
    fn scroll(&mut self, base: &Base, offset: f64) {
        self.offset = if offset.is_finite() { offset.clamp(0., self.max_offset(base)) } else { 0. };
    }

    /// The row `step` rows away from the one with `key`, scrolled into view.
    fn neighbour(&mut self, base: &Base, key: &str, step: isize) -> Option<Value> {
        let at = self.list.items.iter().find(|(_, item)| model::item_key(item) == key).map(|(i, _)| *i)?;
        let next = at.checked_add_signed(step)?;
        let item = self.list.items.get(&next)?.clone();
        let (top, height) = (next as f64 * ROW, list_height(base));
        if top < self.offset {
            self.scroll(base, top);
        } else if top + ROW > self.offset + height {
            self.scroll(base, top + ROW - height);
        }
        Some(item)
    }

    pub fn selected_key(&self) -> Option<String> {
        self.selected.as_ref().map(model::item_key)
    }

    // ---------------------------------------------------------------- reading

    pub fn open(&mut self, base: &mut Base, item: Value, _now: i64) {
        base.jobs.cancel(|tag| matches!(tag, AppTag::Main(Tag::Open { .. } | Tag::Thread { .. } | Tag::Sibling { .. } | Tag::Invite { .. })));
        self.load_error.clear();
        self.expanded.clear();
        self.siblings.clear();
        let key = model::item_key(&item);
        let args = json!({"acct":item["acct"],"folder":item["folder"],"uid":item["uid"]});
        self.selected = Some(item);
        base.ask(AppTag::Main(Tag::Open { key }), "open", args);
    }

    fn close_message(&mut self, base: &mut Base) {
        self.selected = None;
        self.message = None;
        self.thread.clear();
        self.expanded.clear();
        self.siblings.clear();
        self.forget_pictures(base);
    }

    /// Releases the images of messages that are not on screen any more.
    fn forget_pictures(&mut self, base: &mut Base) {
        let mut shown: BTreeSet<String> = self.message.iter().map(model::item_key).collect();
        shown.extend(self.expanded.iter().cloned());
        let gone: Vec<String> = self.pictures.keys().filter(|k| !shown.contains(*k)).cloned().collect();
        for key in gone {
            base.jobs.cancel(|tag| matches!(tag, AppTag::Main(Tag::Picture { message, .. }) if *message == key));
            for picture in self.pictures.remove(&key).unwrap_or_default().into_values() {
                if let Picture::Ready { handle, .. } = picture {
                    base.service(AppTag::Ignore, "pim.imageRelease", json!([handle]));
                }
            }
        }
        self.remote.retain(|k| shown.contains(k));
        self.granted.retain(|k| shown.contains(k));
        self.invites.retain(|k, _| shown.contains(k));
    }

    pub fn sender_of(message: &Value) -> String {
        str_of(&message["from"][0]["email"]).to_lowercase()
    }
    pub fn shows_remote(&self, base: &Base, message: &Value) -> bool {
        let sender = Self::sender_of(message);
        self.remote.contains(&model::item_key(message))
            || !sender.is_empty() && list_of(&base.facts.config["trustedSenders"]).iter().any(|s| *s == sender.as_str())
    }
    /// The images a drawn message asked for. The hosts of its remote ones are allowed first.
    pub fn want_pictures(&mut self, base: &mut Base, message: &Value, remote: bool, wanted: Vec<Wanted>) {
        let key = model::item_key(message);
        let hosts = &message["html"]["remote"];
        if remote && hosts.as_array().is_some_and(|a| !a.is_empty()) && self.granted.insert(key.clone()) {
            base.service(AppTag::Report, "mail.allowImages", json!([hosts]));
        }
        let mut loading = self.pictures.values().flat_map(HashMap::values).filter(|p| **p == Picture::Loading).count();
        for picture in wanted {
            if loading >= PICTURES_AT_ONCE {
                break;
            }
            let known = self.pictures.entry(key.clone()).or_default();
            if known.contains_key(&picture.source) {
                continue;
            }
            known.insert(picture.source.clone(), Picture::Loading);
            loading += 1;
            let args = json!({"acct":message["acct"],"folder":message["folder"],"uid":message["uid"],"src":picture.source,"maxw":picture.max_width});
            base.service(AppTag::Main(Tag::Picture { message: key.clone(), source: picture.source }), "pim.image", json!(["image", args]));
        }
    }

    /// The opened message or an expanded one of its conversation.
    pub fn message_named(&self, key: &str) -> Option<&Value> {
        self.message.as_ref().filter(|m| model::item_key(m) == key).or_else(|| self.siblings.get(key))
    }

    // ---------------------------------------------------------------- answers

    pub fn done(&mut self, base: &mut Base, tag: Tag, result: Result<Value, String>, now: i64) {
        // pim.image answers with the image itself
        if let Tag::Picture { message, source } = tag {
            let picture = match &result {
                Ok(v) if v["image"].as_i64().is_some_and(|h| h > 0) => Picture::Ready {
                    handle: v["image"].as_i64().unwrap_or(0),
                    width: v["w"].as_f64().unwrap_or(0.),
                    height: v["h"].as_f64().unwrap_or(0.),
                },
                _ => Picture::Failed,
            };
            if let Some(slot) = self.pictures.get_mut(&message).and_then(|m| m.get_mut(&source)) {
                *slot = picture;
            }
            return;
        }
        let reply = answer(result);
        match tag {
            Tag::Folders => {
                self.folders_pending = false;
                if let Ok(v) = reply {
                    self.folders = list_of(&v["accounts"]).to_vec();
                }
                if std::mem::take(&mut self.folders_stale) {
                    self.load_folders(base);
                }
            }
            Tag::List { generation, first } => {
                if generation != self.list.generation {
                    return;
                }
                self.list.pending = false;
                self.list.loading = false;
                if let Ok(v) = reply {
                    self.rows_arrived(first, &v);
                    self.scroll(base, self.offset);
                }
            }
            Tag::Open { key } => {
                if self.selected_key().as_deref() != Some(&key) {
                    return;
                }
                match reply {
                    Ok(message) => self.opened(base, key, message, now),
                    Err(failure) => {
                        self.message = None;
                        self.forget_pictures(base);
                        self.load_error = if failure.code == "offline" { base.t("offlineNotCached") } else { failure.message };
                    }
                }
            }
            Tag::Thread { key } => {
                if self.selected_key().as_deref() == Some(&key) {
                    self.thread = reply.map(|v| list_of(&v["items"]).to_vec()).unwrap_or_default();
                }
            }
            Tag::Sibling { key } => {
                if let Ok(message) = reply {
                    if message["invite"] == true {
                        self.ask_invite(base, &message);
                    }
                    self.siblings.insert(key, message);
                }
            }
            Tag::Invite { key } => {
                if let (Some(invite), Ok(v)) = (self.invites.get_mut(&key), reply) {
                    invite.data = Some(v);
                }
            }
            Tag::Answer { key, partstat } => {
                if let Some(invite) = self.invites.get_mut(&key) {
                    invite.busy = false;
                    match reply {
                        Ok(_) => invite.done = partstat.into(),
                        Err(failure) => invite.error = failure.message,
                    }
                }
            }
            Tag::Attachment { name, save } => self.attachment_ready(base, name, save, reply),
            Tag::Attached => self.attaching = None,
            Tag::Forward { generation, slot } => self.forward_file(base, generation, slot, reply.ok()),
            Tag::CreateArchive { targets } => self.move_to(base, targets, "Archive"),
            Tag::Moved => {
                if let Err(failure) = reply {
                    self.load_error = failure.message;
                } else {
                    self.close_message(base);
                }
                self.revision += 1;
            }
            Tag::Deleted { next } => {
                self.revision += 1;
                if let Err(failure) = reply {
                    self.load_error = failure.message;
                    return;
                }
                match next {
                    Some(item) => self.open(base, item, now),
                    None => self.close_message(base),
                }
            }
            Tag::Patch { key, patch } => {
                if reply.is_ok() && self.selected_key().as_deref() == Some(&key) {
                    if let (Some(selected), Some(patch)) = (self.selected.as_mut(), patch.as_object()) {
                        for (field, value) in patch {
                            selected[field.as_str()] = value.clone();
                        }
                    }
                    self.revision += 1;
                }
            }
            Tag::Marked => {}
            Tag::Picture { .. } => unreachable!("handled above"),
        }
    }

    fn opened(&mut self, base: &mut Base, key: String, message: Value, now: i64) {
        let item = self.selected.clone().unwrap_or_default();
        self.thread.clear();
        if item["n"].as_f64().unwrap_or(0.) > 1. && !item["tk"].is_null() {
            base.ask(AppTag::Main(Tag::Thread { key }), "thread", json!({"acct":item["acct"],"tk":item["tk"]}));
        }
        if message["invite"] == true {
            self.ask_invite(base, &message);
        }
        // marked as read after a moment, like other mail apps
        if message["seen"] != true {
            let targets = json!({"acct":item["acct"],"folder":item["folder"],"uids":[item["uid"]],"add":["seen"]});
            self.seen.push((now + SEEN_AFTER_MS, targets));
        }
        self.message = Some(message);
        self.forget_pictures(base);
    }

    fn ask_invite(&mut self, base: &mut Base, message: &Value) {
        let key = model::item_key(message);
        self.invites.insert(key.clone(), Invite::default());
        let args = json!({"acct":message["acct"],"folder":message["folder"],"uid":message["uid"]});
        base.ask(AppTag::Main(Tag::Invite { key }), "invite", args);
    }

    fn attachment_ready(&mut self, base: &mut Base, name: String, save: bool, reply: Result<Value, super::Failure>) {
        let relative = match reply {
            Ok(v) => str_of(&v["path"]).to_owned(),
            Err(failure) => {
                self.attaching = None;
                self.load_error = failure.message;
                return;
            }
        };
        if !save {
            base.service(AppTag::Main(Tag::Attached), "mail.openAttachment", json!([relative, name]));
        } else if base.facts.isolated {
            base.service(AppTag::Main(Tag::Attached), "mail.saveAttachment", json!([relative, name]));
        } else {
            self.attaching = None;
            match model::save_to_downloads(base.facts.cache_dir.as_ref(), &relative, &name, base.home.as_ref()) {
                Ok(path) => base.service(AppTag::Report, "mail.reveal", json!([path.to_string_lossy()])),
                Err(error) => self.load_error = error.to_string(),
            }
        }
    }

    // ---------------------------------------------------------------- actions

    fn special(&self, account: &Value, kind: &str) -> Option<String> {
        let folders = self.folders.iter().find(|a| a["id"] == *account)?;
        list_of(&folders["folders"]).iter().find(|f| f["special"] == kind).map(|f| str_of(&f["name"]).to_owned())
    }
    fn targets(&self) -> Option<Value> {
        self.selected.as_ref().map(model::targets)
    }
    fn move_to(&mut self, base: &mut Base, mut targets: Value, folder: &str) {
        if targets["folder"] == folder {
            return;
        }
        targets["to"] = json!(folder);
        base.ask(AppTag::Main(Tag::Moved), "move", targets);
    }
    pub fn archive(&mut self, base: &mut Base) {
        let Some(targets) = self.targets() else { return };
        match self.special(&targets["acct"], "archive") {
            Some(folder) => self.move_to(base, targets, &folder),
            None => {
                let account = targets["acct"].clone();
                base.ask(AppTag::Main(Tag::CreateArchive { targets }), "create-folder", json!({"acct":account,"name":"Archive"}));
            }
        }
    }
    pub fn remove(&mut self, base: &mut Base) {
        let Some(targets) = self.targets() else { return };
        let key = self.selected_key().unwrap_or_default();
        let next = self.neighbour(base, &key, 1);
        base.ask(AppTag::Main(Tag::Deleted { next }), "delete", targets);
    }
    pub fn junk(&mut self, base: &mut Base) {
        let Some(targets) = self.targets() else { return };
        if let Some(folder) = self.special(&targets["acct"], "junk") {
            self.move_to(base, targets, &folder);
        }
    }
    /// Adds or removes one flag of the selected conversation; `patch` is how its row changes.
    fn flag(&mut self, base: &mut Base, add: bool, flag: String, patch: Value) {
        let (Some(mut targets), Some(key)) = (self.targets(), self.selected_key()) else { return };
        targets[if add { "add" } else { "remove" }] = json!([flag]);
        base.ask(AppTag::Main(Tag::Patch { key, patch }), "flag", targets);
    }
    pub fn toggle_flagged(&mut self, base: &mut Base) {
        let Some(on) = self.selected.as_ref().map(|s| !model::is_flagged(s)) else { return };
        self.flag(base, on, "flagged".into(), json!({"anyFlag":on,"flagged":on}));
    }
    pub fn toggle_unread(&mut self, base: &mut Base) {
        let Some(unread) = self.selected.as_ref().map(model::is_unread) else { return };
        // an unread message becomes read, and the other way around
        self.flag(base, unread, "seen".into(), json!({"seen":unread,"unreadN":i32::from(!unread)}));
    }
    pub fn toggle_label(&mut self, base: &mut Base, label: u64) {
        let Some(selected) = &self.selected else { return };
        let mut labels: Vec<u64> = list_of(&selected["labels"]).iter().filter_map(Value::as_u64).collect();
        let has = labels.contains(&label);
        if has {
            labels.retain(|l| *l != label);
        } else {
            labels.push(label);
        }
        self.flag(base, !has, format!("label{label}"), json!({"labels":labels}));
    }

    pub fn reply(&mut self, base: &mut Base, kind: Reply) {
        let Some(message) = self.message.clone() else { return };
        let date = message["date"].as_i64().unwrap_or(0);
        let when = base.full_date(date);
        let from = &message["from"][0];
        let name = model::address_short(from).to_owned();
        let words = model::DraftWords {
            quote_head: base.tp("quoteHead", json!({"date":when,"name":name})),
            forwarded: base.t("forwarded"),
            from: base.t("from"),
            date: base.t("date"),
            subject: base.t("subject"),
            to: base.t("to"),
            when,
        };
        let me = base.account(str_of(&message["acct"])).map_or("", |a| str_of(&a["email"])).to_owned();
        let draft = model::reply_draft(&message, kind, &me, &words);
        if kind != Reply::Forward {
            base.compose(draft);
            return;
        }
        // the original's attachments go along: first each one into the cache
        let sources: Vec<(Value, String, f64)> = list_of(&message["atts"])
            .iter()
            .take(model::MAX_ATTACHMENTS)
            .map(|a| {
                let args = json!({"acct":message["acct"],"folder":message["folder"],"uid":message["uid"],"idx":a["idx"]});
                (args, model::safe_name(str_of(&a["name"])), a["size"].as_f64().unwrap_or(0.))
            })
            .collect();
        if sources.is_empty() {
            base.compose(draft);
            return;
        }
        self.forwards += 1;
        let first = sources[0].0.clone();
        let generation = self.forwards;
        self.forward = Some(Forward { generation, draft, files: vec![None; sources.len()], sources, next: 0 });
        base.ask(AppTag::Main(Tag::Forward { generation, slot: 0 }), "attachment", first);
    }
    fn forward_file(&mut self, base: &mut Base, generation: u64, slot: usize, reply: Option<Value>) {
        let Some(forward) = self.forward.as_mut().filter(|f| f.generation == generation && f.next == slot) else { return };
        if let Some(v) = reply {
            let (_, name, size) = &forward.sources[slot];
            forward.files[slot] = Some(json!({"cached":v["path"],"name":name,"size":size}));
        }
        forward.next += 1;
        if let Some((args, _, _)) = forward.sources.get(forward.next) {
            let (args, slot) = (args.clone(), forward.next);
            base.ask(AppTag::Main(Tag::Forward { generation, slot }), "attachment", args);
            return;
        }
        if let Some(mut forward) = self.forward.take() {
            forward.draft["attachments"] = Value::Array(forward.files.into_iter().flatten().collect());
            base.compose(forward.draft);
        }
    }

    pub fn follow_link(&mut self, base: &mut Base, link: Value) {
        let url = str_of(&link["h"]);
        if url.is_empty() {
            return;
        }
        match model::mailto_draft(url) {
            Some(draft) => base.compose(draft),
            None => self.sheet = Sheet::Link(link),
        }
    }

    fn trust_sender(&mut self, base: &mut Base, sender: &str) {
        if sender.is_empty() {
            return;
        }
        let mut config = base.facts.config.clone();
        let mut trusted = list_of(&config["trustedSenders"]).to_vec();
        if !trusted.iter().any(|s| s == sender) {
            trusted.push(json!(sender));
        }
        config["trustedSenders"] = Value::Array(trusted);
        base.save_config(config);
    }

    fn attachment(&mut self, base: &mut Base, key: &Value, save: bool) {
        let (Some(message_key), Some(index)) = (key[0].as_str(), key[1].as_i64()) else { return };
        if self.attaching.is_some() {
            return;
        }
        let Some(message) = self.message_named(message_key) else { return };
        let Some(file) = list_of(&message["atts"]).iter().find(|a| a["idx"] == index) else { return };
        let name = model::safe_name(str_of(&file["name"]));
        let args = json!({"acct":message["acct"],"folder":message["folder"],"uid":message["uid"],"idx":index});
        self.attaching = Some((message_key.to_owned(), index));
        base.ask(AppTag::Main(Tag::Attachment { name, save }), "attachment", args);
    }

    // ---------------------------------------------------------------- input

    pub fn command(&mut self, base: &mut Base, command: &str, v: &Value, now: i64) {
        if command == "sheetClose" {
            if let Sheet::Certificate(certificate) = &self.sheet {
                self.dismissed.insert(fingerprint(certificate));
            }
        }
        if let Some(reload) = self.sheet.command(base, command, v) {
            if reload {
                self.load_folders(base);
                self.revision += 1;
            }
            return;
        }
        let key = &v["key"];
        match command {
            "compose" => base.compose(Value::Null),
            "addAccount" => self.sheet = Sheet::setup(),
            "accounts" => self.sheet = Sheet::accounts(base),
            "sync" => base.ask(AppTag::Report, "sync", json!({})),
            "reply" => self.reply(base, Reply::Sender),
            "replyAll" => self.reply(base, Reply::All),
            "forward" => self.reply(base, Reply::Forward),
            "archive" => self.archive(base),
            "delete" => self.remove(base),
            "junk" => self.junk(base),
            "flag" => self.toggle_flagged(base),
            "unread" => self.toggle_unread(base),
            "labels" if self.selected.is_some() => self.sheet = Sheet::Labels,
            "label" => {
                if let Some(label) = key.as_u64().filter(|l| (1..=6).contains(l)) {
                    self.toggle_label(base, label);
                    self.sheet = Sheet::None;
                }
            }
            "unified" => {
                let filter = match str_of(key) {
                    "unread" => "unread",
                    "flagged" => "flagged",
                    _ => "all",
                };
                self.show(Source::Unified { filter, label: 0 });
            }
            "category" => {
                if let Some(label) = key.as_u64().filter(|l| (1..=6).contains(l)) {
                    self.show(Source::Unified { filter: "all", label: label as u8 });
                }
            }
            "folder" => {
                if let (Some(account), Some(folder)) = (key[0].as_str(), key[1].as_str()) {
                    self.show(Source::Folder { account: account.into(), folder: folder.into() });
                }
            }
            "accountWarning" => {
                if let Some(id) = key.as_str() {
                    self.sheet = Sheet::for_warning(base, id);
                }
            }
            "query" => {
                self.query = str_of(&v["value"]).to_owned();
                let trimmed = self.query.trim().to_owned();
                if trimmed.chars().count() >= 2 {
                    self.show(Source::Search(trimmed));
                } else if trimmed.is_empty() && matches!(self.source, Source::Search(_)) {
                    self.show(Source::default());
                }
            }
            "queryClear" => {
                self.query.clear();
                self.search_field += 1;
                if matches!(self.source, Source::Search(_)) {
                    self.show(Source::default());
                }
            }
            "row" | "rowMenu" => {
                if let Some(item) = key.as_u64().and_then(|i| self.list.items.get(&(i as usize))).cloned() {
                    if self.selected_key() != Some(model::item_key(&item)) {
                        self.open(base, item, now);
                    }
                    if command == "rowMenu" {
                        self.sheet = Sheet::Labels;
                    }
                }
            }
            "listWheel" => self.scroll(base, self.offset + v["deltaY"].as_f64().unwrap_or(0.)),
            "barDown" => self.bar_drag = Some((v["y"].as_f64().unwrap_or(0.), self.offset)),
            "pointerMove" => {
                if let Some((from, offset)) = self.bar_drag {
                    let height = list_height(base);
                    let moved = v["y"].as_f64().unwrap_or(from) - from;
                    self.scroll(base, offset + moved * self.list.total as f64 * ROW / height);
                }
            }
            "dragEnd" => self.bar_drag = None,
            "expand" => {
                let name = str_of(key).to_owned();
                if let Some(item) = self.thread.iter().find(|t| model::item_key(t) == name) {
                    let args = json!({"acct":item["acct"],"folder":item["folder"],"uid":item["uid"]});
                    self.expanded.insert(name.clone());
                    if !self.siblings.contains_key(&name) {
                        base.ask(AppTag::Main(Tag::Sibling { key: name }), "open", args);
                    }
                }
            }
            "collapse" => {
                self.expanded.remove(str_of(key));
                self.forget_pictures(base);
            }
            "loadImages" => {
                self.remote.insert(str_of(key).to_owned());
                // what failed or waited for the permission is asked again
                if let Some(pictures) = self.pictures.get_mut(str_of(key)) {
                    pictures.retain(|source, p| !(source.starts_with("r:") && *p == Picture::Failed));
                }
            }
            "trustImages" => {
                let Some(sender) = self.message_named(str_of(key)).map(Self::sender_of) else { return };
                self.remote.insert(str_of(key).to_owned());
                self.trust_sender(base, &sender);
            }
            "link" => {
                let link = self.message_named(str_of(&key[0])).and_then(|m| {
                    let i = key[1].as_u64()? as usize;
                    m["html"]["links"].get(i).cloned()
                });
                if let Some(link) = link {
                    self.follow_link(base, link);
                }
            }
            "url" => self.follow_link(base, json!({"h":key})),
            "attachmentOpen" => self.attachment(base, key, false),
            "attachmentSave" => self.attachment(base, key, true),
            "invite" => {
                let name = str_of(&key[0]).to_owned();
                let partstat = match str_of(&key[1]) {
                    "ACCEPTED" => "ACCEPTED",
                    "TENTATIVE" => "TENTATIVE",
                    "DECLINED" => "DECLINED",
                    _ => return,
                };
                let Some(message) = self.message_named(&name) else { return };
                let args = json!({"acct":message["acct"],"folder":message["folder"],"uid":message["uid"],"partstat":partstat});
                if let Some(invite) = self.invites.get_mut(&name).filter(|i| !i.busy) {
                    invite.busy = true;
                    invite.error.clear();
                    base.ask(AppTag::Main(Tag::Answer { key: name, partstat }), "invite-respond", args);
                }
            }
            "inviteShow" => {
                if let Some(event) = self.invites.get(str_of(key)).and_then(|i| i.data.as_ref()).map(|d| &d["event"]) {
                    let args = json!({"at":event["start"],"uid":event["uid"],"nonce":now});
                    base.open_app("calendar", json!({"args":args}));
                }
            }
            _ => {}
        }
    }

    pub fn sheet_done(&mut self, base: &mut Base, tag: super::sheets::Tag, result: Result<Value, String>, now: i64) {
        self.sheet.done(base, tag, result, now);
    }

    pub fn key(&mut self, base: &mut Base, key: &str, flags: i32, facts: &KeyFacts, now: i64) -> bool {
        if !matches!(self.sheet, Sheet::None) {
            if key == "Escape" {
                self.command(base, "sheetClose", &Value::Null, now);
                return true;
            }
            return false;
        }
        let command_key = flags & (2 | 16) != 0;
        let letter = if facts.base.is_empty() { key } else { facts.base }.to_lowercase();
        if command_key {
            let command = match (letter.as_str(), facts.shift) {
                ("n", _) => "compose",
                ("r", false) => "reply",
                ("r", true) => "replyAll",
                ("j", _) => "forward",
                ("e", _) => "archive",
                _ if key == "Backspace" => "delete",
                _ => return false,
            };
            self.command(base, command, &Value::Null, now);
            return true;
        }
        match key {
            "Delete" => self.remove(base),
            "ArrowDown" | "ArrowUp" => {
                let Some(selected) = self.selected_key() else { return false };
                if let Some(item) = self.neighbour(base, &selected, if key == "ArrowDown" { 1 } else { -1 }) {
                    self.open(base, item, now);
                }
            }
            _ => return false,
        }
        true
    }
}
