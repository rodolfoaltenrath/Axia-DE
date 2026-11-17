use super::model::*;
use crate::native_view::async_jobs::{Completed, Jobs};
use serde_json::{json, Value};

#[derive(Clone)]
struct Write {
    sequence: u64,
    contact: Value,
    photo: Value,
    book: String,
    group: Option<Value>,
}
#[derive(Clone)]
struct Import {
    file: Value,
    book: String,
}
struct GroupWrite {
    value: Value,
    sheet_sequence: u64,
    new: bool,
}
enum Pending {
    Write(Write),
    Import(Import),
    Group(GroupWrite),
}
enum Tag {
    Books(u64),
    Contacts(u64),
    Watch,
    Events,
    Ignore,
    CreateBook(Pending),
    Save(Write),
    Join(Write, String),
    Group(GroupWrite),
    Mutate,
    Delete(String, u64),
    Import(Value),
    PickPhoto(u64),
    PickImport(String),
    Image(u64, String),
    Paste(u64),
}
pub(super) struct Draft {
    pub value: Value,
    pub photo: Value,
    pub busy: bool,
    pub error: String,
    sequence: u64,
    join_group: Option<Value>,
}
pub(super) struct State {
    jobs: Jobs<Tag>,
    pub books: Vec<Value>,
    pub contacts: Vec<Value>,
    pub source: String,
    pub selected: String,
    pub query: String,
    pub draft: Option<Draft>,
    pub sheet: Value,
    pub menu: String,
    pub default_book: String,
    pub unavailable: bool,
    pub booted: bool,
    pub notes_focus: bool,
    pub photo_handle: i64,
    photo_target: (u64, String),
    window: String,
    epoch: u64,
    sequence: u64,
    books_loaded: bool,
    contacts_loaded: bool,
    books_error: bool,
    contacts_error: bool,
    events_pending: bool,
    events_at: i64,
    reload_at: Option<i64>,
    waiting_selection: bool,
    sheet_sequence: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            books: vec![],
            contacts: vec![],
            source: "all".into(),
            selected: String::new(),
            query: String::new(),
            draft: None,
            sheet: Value::Null,
            menu: String::new(),
            default_book: String::new(),
            unavailable: false,
            booted: false,
            notes_focus: false,
            photo_handle: 0,
            photo_target: (0, String::new()),
            window: String::new(),
            epoch: 0,
            sequence: 0,
            books_loaded: false,
            contacts_loaded: false,
            books_error: false,
            contacts_error: false,
            events_pending: false,
            events_at: 0,
            reload_at: None,
            waiting_selection: false,
            sheet_sequence: 0,
        }
    }
}
impl State {
    pub fn open_mail(&mut self, name: String, email: String, now: i64) -> Value {
        self.jobs.enqueue(
            Tag::Ignore,
            "windows.open",
            json!(["mail",{"new":true,"args":{"compose":{"to":[{"name":name,"email":email}]}}}]),
        );
        self.request(&json!({"cmd":"noop"}), now)
    }
    pub fn busy(&self) -> bool {
        self.jobs.busy()
    }
    pub fn selected_contact(&self) -> Option<&Value> {
        self.contacts.iter().find(|c| key(c) == self.selected)
    }
    fn call(&mut self, tag: Tag, op: &str, args: Value) {
        self.jobs.enqueue(tag, "pim.request", json!([op, args]));
    }
    fn file_call(&mut self, tag: Tag, op: &str, args: Value, file: &Value) {
        self.jobs
            .enqueue(tag, "pim.request", json!([op, args, [file]]));
    }
    fn release(&mut self, file: Value) {
        if file.is_u64() {
            self.jobs
                .enqueue(Tag::Ignore, "pim.fileRelease", json!([file]));
        }
    }
    fn set_sheet(&mut self, sheet: Value) {
        self.sheet_sequence += 1;
        self.sheet = sheet;
    }
    fn reload(&mut self) {
        self.epoch += 1;
        self.jobs
            .cancel(|tag| matches!(tag, Tag::Books(_) | Tag::Contacts(_)));
        self.call(Tag::Books(self.epoch), "book-list", json!({}));
        self.call(Tag::Contacts(self.epoch), "contact-list", json!({}));
    }
    fn remember(&mut self) {
        self.jobs.enqueue(Tag::Ignore, "windows.state", json!([self.window,{"src":self.source,"cur":if self.selected.is_empty(){Value::Null}else{json!(self.selected)}}]));
    }
    fn reconcile(&mut self) {
        if self.draft.is_some() || self.waiting_selection {
            return;
        }
        let list = visible(&self.contacts, &self.source, &self.query);
        if !list.iter().any(|c| key(c) == self.selected) {
            self.selected = list.first().map(|c| key(c)).unwrap_or_default();
            self.remember();
        }
    }
    fn cancel_draft(&mut self) {
        self.notes_focus = false;
        self.sequence += 1;
        if let Some(d) = self.draft.take() {
            self.release(d.photo);
        }
    }
    fn edit(&mut self, value: Value) {
        self.cancel_draft();
        let join_group = (value["_new"] == true)
            .then(|| source_group(&self.contacts, &self.source).cloned())
            .flatten();
        self.draft = Some(Draft {
            value,
            photo: json!("keep"),
            busy: false,
            error: String::new(),
            sequence: self.sequence,
            join_group,
        });
        self.menu.clear();
    }
    fn target(&self) -> Option<String> {
        target_book(&self.books, source_book(&self.contacts, &self.source)).map(str::to_owned)
    }
    fn run_pending(&mut self, pending: Pending) {
        match pending {
            Pending::Write(mut w) => {
                // A deleted address book may cause target selection to fall
                // back to another book. Membership never crosses that boundary.
                w.group = w.group.filter(|g| g["book"] == w.book);
                let file = w.photo.clone();
                let args = json!({"book":w.book,"contact":w.contact,"photo":if file.is_u64(){json!("new")}else{file.clone()}});
                w.contact["book"] = json!(w.book);
                if file.is_u64() {
                    self.file_call(Tag::Save(w), "contact-save", args, &file);
                } else {
                    self.call(Tag::Save(w), "contact-save", args);
                }
            }
            Pending::Import(i) => {
                self.file_call(
                    Tag::Import(i.file.clone()),
                    "contact-import",
                    json!({"book":i.book}),
                    &i.file,
                );
            }
            Pending::Group(group) => {
                // The book was chosen before creating the group. This snapshot
                // also survives a source change while the book-create runs.
                let args = json!({"book":group.value["book"],"contact":group.value,"photo":"keep"});
                self.call(Tag::Group(group), "contact-save", args);
            }
        }
    }
    fn with_book(&mut self, mut pending: Pending) {
        let book = match &pending {
            Pending::Write(w) if !w.book.is_empty() => Some(w.book.clone()),
            Pending::Import(i) if !i.book.is_empty() => Some(i.book.clone()),
            Pending::Group(g) if !string(&g.value["book"]).is_empty() => {
                Some(string(&g.value["book"]).into())
            }
            _ => self.target(),
        };
        if let Some(book) = book {
            match &mut pending {
                Pending::Write(w) => w.book = book,
                Pending::Import(i) => i.book = book,
                Pending::Group(g) => g.value["book"] = json!(book),
            }
            self.run_pending(pending);
        } else {
            self.call(
                Tag::CreateBook(pending),
                "book-create",
                json!({"name":self.default_book}),
            );
        }
    }
    fn save(&mut self) {
        let Some(d) = &mut self.draft else {
            return;
        };
        if d.busy {
            return;
        }
        let clean = match build(&d.value) {
            Ok(c) => c,
            Err(e) => {
                d.error = e.into();
                return;
            }
        };
        d.busy = true;
        d.error.clear();
        let new = d.value["_new"] == true;
        if new {
            // Creating a group while the contact editor is open changes its
            // destination. Capture membership at save time, then retain it for
            // retries after the card itself has already been written.
            d.join_group = source_group(&self.contacts, &self.source).cloned();
        }
        let w = Write {
            sequence: d.sequence,
            contact: clean,
            photo: d.photo.clone(),
            book: if new {
                String::new()
            } else {
                string(&d.value["book"]).into()
            },
            group: d.join_group.clone(),
        };
        self.with_book(Pending::Write(w));
    }
    fn fail_write(&mut self, seq: u64) {
        if let Some(d) = self.draft.as_mut().filter(|d| d.sequence == seq) {
            d.busy = false;
            d.error = "contacts.saveFailed".into();
        }
    }
    fn saved(&mut self, w: Write, uid: &str, now: i64) {
        if self
            .draft
            .as_ref()
            .is_some_and(|d| d.sequence == w.sequence)
        {
            self.selected = format!("{}/{uid}", w.book);
            self.waiting_selection = true;
            self.cancel_draft();
            self.remember();
        }
        self.reload_at = Some(now);
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Image(epoch, id) => {
                let image = done
                    .result
                    .ok()
                    .and_then(|v| v["image"].as_i64())
                    .unwrap_or(0);
                if (epoch, id) == self.photo_target {
                    self.photo_handle = image.max(0);
                } else if image > 0 {
                    self.jobs
                        .enqueue(Tag::Ignore, "pim.imageRelease", json!([image]));
                }
            }
            Tag::Paste(seq) => {
                if self.notes_focus {
                    if let (Some(d), Ok(v)) = (
                        self.draft.as_mut().filter(|d| d.sequence == seq && !d.busy),
                        done.result,
                    ) {
                        d.value["notes"] = json!(format!(
                            "{}{}",
                            string(&d.value["notes"]),
                            string(&v["text"])
                        ));
                    }
                }
            }
            Tag::Books(e) if e == self.epoch => {
                match done.result {
                    Ok(v) => {
                        self.books = v["books"].as_array().cloned().unwrap_or_default();
                        self.books_loaded = true;
                        self.books_error = false;
                    }
                    Err(_) => self.books_error = !self.books_loaded,
                }
                self.unavailable = self.books_error || self.contacts_error;
            }
            Tag::Contacts(e) if e == self.epoch => {
                match done.result {
                    Ok(v) => {
                        self.contacts = v["contacts"].as_array().cloned().unwrap_or_default();
                        self.contacts_loaded = true;
                        self.contacts_error = false;
                        self.waiting_selection = false;
                        self.reconcile();
                    }
                    Err(_) => self.contacts_error = !self.contacts_loaded,
                }
                self.unavailable = self.books_error || self.contacts_error;
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 200;
                if done.result.ok().as_ref().is_some_and(|v| {
                    v.as_array().is_some_and(|a| {
                        a.iter()
                            .any(|e| ["ready", "crash", "pim-changed"].contains(&string(&e["ev"])))
                    })
                }) {
                    self.reload_at = Some(now + 50);
                }
            }
            Tag::CreateBook(mut pending) => {
                let book = done
                    .result
                    .as_ref()
                    .ok()
                    .map(|v| string(&v["uid"]))
                    .unwrap_or("");
                if book.is_empty() {
                    match pending {
                        Pending::Write(w) => self.fail_write(w.sequence),
                        Pending::Import(i) => {
                            self.release(i.file);
                            self.sheet = json!({"notice":"contacts.importFailed"});
                        }
                        Pending::Group(g) => {
                            if self.sheet_sequence == g.sheet_sequence && !self.sheet.is_null() {
                                self.sheet["busy"] = json!(false);
                                self.sheet["error"] = json!("contacts.saveFailed");
                            }
                        }
                    }
                } else {
                    match &mut pending {
                        Pending::Write(w) => w.book = book.into(),
                        Pending::Import(i) => i.book = book.into(),
                        Pending::Group(g) => g.value["book"] = json!(book),
                    }
                    self.run_pending(pending);
                }
            }
            Tag::Save(mut w) => {
                let uid = done
                    .result
                    .as_ref()
                    .ok()
                    .map(|v| string(&v["uid"]))
                    .filter(|s| !s.is_empty());
                if let Some(uid) = uid {
                    // If joining a group fails, retry must update this same card
                    // rather than creating a second contact.
                    if let Some(d) = self.draft.as_mut().filter(|d| d.sequence == w.sequence) {
                        d.value["uid"] = json!(uid);
                        d.value["book"] = json!(w.book);
                        d.value["_new"] = json!(false);
                    }
                    if let Some(mut group) = w.group.take() {
                        let mut members: Vec<_> =
                            group["members"].as_array().cloned().unwrap_or_default();
                        if !members.iter().any(|m| m == uid) {
                            members.push(json!(uid));
                        }
                        group["members"] = json!(members);
                        self.call(
                            Tag::Join(w, uid.into()),
                            "contact-save",
                            json!({"book":group["book"],"contact":group,"photo":"keep"}),
                        );
                    } else {
                        self.saved(w, uid, now);
                    }
                } else {
                    self.fail_write(w.sequence);
                }
            }
            Tag::Join(w, uid) => {
                if done.result.is_ok() {
                    self.saved(w, &uid, now);
                } else {
                    self.fail_write(w.sequence);
                    self.reload_at = Some(now);
                }
            }
            Tag::Group(g) => {
                if let Ok(v) = done.result {
                    if self.sheet_sequence == g.sheet_sequence {
                        self.set_sheet(Value::Null);
                        if g.new && !string(&v["uid"]).is_empty() {
                            self.source =
                                format!("group:{}/{}", string(&g.value["book"]), string(&v["uid"]));
                            self.remember();
                        }
                    }
                    self.reload_at = Some(now);
                } else if self.sheet_sequence == g.sheet_sequence && !self.sheet.is_null() {
                    self.sheet["busy"] = json!(false);
                    self.sheet["error"] = json!("contacts.saveFailed");
                }
            }
            Tag::Mutate => {
                if done.result.is_ok() {
                    self.reload_at = Some(now);
                }
            }
            Tag::Delete(id, seq) => {
                if done.result.is_ok() {
                    if self.source == format!("group:{id}") {
                        self.source = "all".into();
                    }
                    if self.draft.as_ref().is_some_and(|d| key(&d.value) == id) {
                        self.cancel_draft();
                    }
                    if self.sheet_sequence == seq {
                        self.set_sheet(Value::Null);
                    }
                    self.reload_at = Some(now);
                    self.remember();
                } else if self.sheet_sequence == seq {
                    self.set_sheet(json!({"notice":"contacts.saveFailed"}));
                }
            }
            Tag::Import(file) => {
                self.release(file);
                self.sheet = match done.result {
                    Ok(v) => {
                        self.reload_at = Some(now);
                        json!({"notice":"contacts.imported","count":v["added"].as_u64().unwrap_or(0)})
                    }
                    Err(_) => json!({"notice":"contacts.importFailed"}),
                };
            }
            Tag::PickPhoto(seq) => {
                if let Ok(v) = done.result {
                    if let Some(file) = v["file"].as_u64() {
                        if let Some(d) =
                            self.draft.as_mut().filter(|d| d.sequence == seq && !d.busy)
                        {
                            let old = std::mem::replace(&mut d.photo, json!(file));
                            self.release(old);
                        } else {
                            self.release(json!(file));
                        }
                    }
                }
            }
            Tag::PickImport(book) => match done.result {
                Ok(v) => {
                    if v["file"].is_u64() {
                        self.with_book(Pending::Import(Import {
                            file: v["file"].clone(),
                            book,
                        }));
                    }
                }
                Err(_) => self.sheet = json!({"notice":"contacts.importFailed"}),
            },
            _ => {}
        }
    }
    pub fn request(&mut self, v: &Value, now: i64) -> Value {
        let k = string(&v["key"]);
        match string(&v["cmd"]) {
            "sync" if !self.booted => {
                if let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) {
                    self.booted = true;
                    self.jobs.bind(owner);
                    self.window = string(&v["args"]["__nativeWindow"]).into();
                    self.source = v["args"]["__nativeState"]["src"]
                        .as_str()
                        .unwrap_or("all")
                        .into();
                    self.selected = string(&v["args"]["__nativeState"]["cur"]).into();
                    self.jobs.enqueue(
                        Tag::Watch,
                        "pim.watch",
                        json!([["pim-changed", "ready", "crash"]]),
                    );
                    self.reload();
                }
            }
            "next" => {
                for done in self.jobs.complete(&v["result"], now) {
                    self.apply(done, now);
                }
            }
            "source" => {
                self.cancel_draft();
                self.source = k.into();
                self.menu.clear();
                self.reconcile();
                self.remember();
            }
            "select" => {
                if self
                    .contacts
                    .iter()
                    .any(|c| c["kind"] != "group" && key(c) == k)
                {
                    self.cancel_draft();
                    self.selected = k.into();
                    self.remember();
                }
            }
            "query" => {
                self.query = string(&v["value"]).into();
                self.reconcile();
            }
            "new" if !self.unavailable => {
                let mut c = blank();
                c["_new"] = json!(true);
                self.edit(c);
            }
            "edit" => {
                if let Some(c) = self.selected_contact().cloned() {
                    self.edit(c);
                }
            }
            "cancel" => {
                self.cancel_draft();
                self.reconcile();
            }
            "field" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    if ["org", "title", "notes", "bday"].contains(&k) {
                        d.value[k] = v["value"].clone();
                    }
                }
            }
            "name" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    if ["prefix", "given", "middle", "family", "suffix"].contains(&k) {
                        d.value["n"][k] = v["value"].clone();
                    }
                }
            }
            "row" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    if KINDS.contains(&k) {
                        if let Some(row) = v["index"].as_u64().and_then(|i| {
                            d.value[k]
                                .as_array_mut()
                                .and_then(|a| a.get_mut(i as usize))
                        }) {
                            let field = string(&v["field"]);
                            if [
                                "type", "value", "street", "city", "region", "code", "country",
                            ]
                            .contains(&field)
                            {
                                row[field] = v["value"].clone();
                            }
                        }
                    }
                }
            }
            "addRow" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    if KINDS.contains(&k) {
                        if !d.value[k].is_array() {
                            d.value[k] = json!([]);
                        }
                        d.value[k]
                            .as_array_mut()
                            .unwrap()
                            .push(json!({"type":if k=="phones"{"cell"}else{"home"},"value":""}));
                    }
                }
            }
            "removeRow" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    if KINDS.contains(&k) {
                        if let Some(a) = d.value[k].as_array_mut() {
                            if let Some(i) = v["index"]
                                .as_u64()
                                .map(|i| i as usize)
                                .filter(|i| *i < a.len())
                            {
                                a.remove(i);
                            }
                        }
                    }
                }
            }
            "save" => self.save(),
            "notesFocus" => self.notes_focus = self.draft.as_ref().is_some_and(|d| !d.busy),
            "notesPaste" => {
                if let Some(d) = self.draft.as_ref().filter(|d| !d.busy && self.notes_focus) {
                    self.jobs
                        .enqueue(Tag::Paste(d.sequence), "clipboard.read", json!([]));
                }
            }
            "photoRemove" => {
                if let Some(d) = self.draft.as_mut().filter(|d| !d.busy) {
                    let old = std::mem::replace(&mut d.photo, json!("remove"));
                    self.release(old);
                }
            }
            "photoPick" => {
                if let Some(d) = self.draft.as_ref().filter(|d| !d.busy) {
                    self.jobs
                        .enqueue(Tag::PickPhoto(d.sequence), "pim.filePick", json!([{}]));
                }
            }
            "import" => {
                let book = self.target().unwrap_or_default();
                self.jobs
                    .enqueue(Tag::PickImport(book), "pim.filePick", json!([{}]));
            }
            "groupNew" => self.set_sheet(json!({"group":null,"name":""})),
            "groupDelete" => {
                if let Some(g) = self
                    .contacts
                    .iter()
                    .find(|c| c["kind"] == "group" && key(c) == k)
                    .cloned()
                {
                    self.call(
                        Tag::Delete(key(&g), self.sheet_sequence),
                        "contact-delete",
                        json!({"book":g["book"],"uid":g["uid"]}),
                    );
                }
                self.menu.clear();
            }
            "groupEdit" => {
                if let Some(g) = self
                    .contacts
                    .iter()
                    .find(|c| c["kind"] == "group" && key(c) == k)
                {
                    self.set_sheet(json!({"group":g,"name":display_name(g)}));
                    self.menu.clear();
                }
            }
            "groupName" => {
                if self.sheet.get("group").is_some() && self.sheet["busy"] != true {
                    self.sheet["name"] = v["value"].clone();
                }
            }
            "groupSave" => {
                let name = string(&self.sheet["name"]).trim().to_owned();
                if !name.is_empty()
                    && self.sheet.get("group").is_some()
                    && self.sheet["busy"] != true
                {
                    self.sheet["busy"] = json!(true);
                    let new = !self.sheet["group"].is_object();
                    let mut g = if !new {
                        self.sheet["group"].clone()
                    } else {
                        json!({"kind":"group","n":{},"members":[]})
                    };
                    g["fn"] = json!(name);
                    self.with_book(Pending::Group(GroupWrite {
                        value: g,
                        sheet_sequence: self.sheet_sequence,
                        new,
                    }));
                }
            }
            "groupToggle" => {
                if let (Some(g), Some(c)) = (
                    self.contacts
                        .iter()
                        .find(|c| c["kind"] == "group" && key(c) == k),
                    self.selected_contact(),
                ) {
                    if let Some(g) = toggle_member(g, c) {
                        self.call(
                            Tag::Mutate,
                            "contact-save",
                            json!({"book":g["book"],"contact":g,"photo":"keep"}),
                        );
                    }
                }
                self.menu.clear();
            }
            "delete" => {
                if let Some(c) = self.contacts.iter().find(|c| key(c) == k) {
                    self.set_sheet(json!({"confirm":c}));
                }
                self.menu.clear();
            }
            "confirmDelete" => {
                if self.sheet["confirm"].is_object() && self.sheet["busy"] != true {
                    let c = self.sheet["confirm"].clone();
                    self.sheet["busy"] = json!(true);
                    self.call(
                        Tag::Delete(key(&c), self.sheet_sequence),
                        "contact-delete",
                        json!({"book":c["book"],"uid":c["uid"]}),
                    );
                }
            }
            "closeSheet" => self.set_sheet(Value::Null),
            "menu" => self.menu = k.into(),
            "refresh" => self.reload_at = Some(now),
            _ => {}
        }
        if self.reload_at.is_some_and(|at| at <= now) {
            self.reload_at = None;
            self.reload();
        }
        if self.booted {
            self.update_photo();
        }
        if self.booted && !self.events_pending && now >= self.events_at {
            self.events_pending = true;
            self.jobs.enqueue(Tag::Events, "pim.events", json!([]));
        }
        self.jobs
            .pump(now)
            .map_or_else(|| json!({}), |effect| json!({"effect":effect}))
    }
    fn update_photo(&mut self) {
        let c = self
            .draft
            .as_ref()
            .map(|d| &d.value)
            .or_else(|| self.selected_contact());
        let wanted = c
            .filter(|c| {
                c["photo"] == true
                    && !string(&c["uid"]).is_empty()
                    && !string(&c["book"]).is_empty()
            })
            .filter(|_| self.draft.as_ref().is_none_or(|d| d.photo != "remove"))
            .cloned();
        let id = wanted.as_ref().map(key).unwrap_or_default();
        let target = (self.epoch, id);
        if target == self.photo_target {
            return;
        }
        self.photo_target = target.clone();
        self.jobs.cancel(|tag| matches!(tag, Tag::Image(_, _)));
        if self.photo_handle > 0 {
            self.jobs
                .enqueue(Tag::Ignore, "pim.imageRelease", json!([self.photo_handle]));
            self.photo_handle = 0;
        }
        if let Some(c) = wanted {
            self.jobs.enqueue(
                Tag::Image(target.0, target.1),
                "pim.image",
                json!(["contact-photo",{"book":c["book"],"uid":c["uid"],"size":192}]),
            );
        }
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
