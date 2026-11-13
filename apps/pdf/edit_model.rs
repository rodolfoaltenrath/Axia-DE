//! Annotation/form drafts and a per-document mutation queue. The host executes
//! one returned helper effect, then acknowledges its ticket. Raster handles and
//! JavaScript helper objects never enter this model.
use super::viewer_model::trim_query;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

struct Ink {
    page: u32,
    strokes: Vec<Vec<f64>>,
    points: usize,
}
struct Mutation {
    ticket: u64,
    kind: &'static str,
    page: Option<u32>,
    payload: Value,
}
#[derive(Default)]
pub(super) struct Editor {
    enabled: bool,
    pages: Option<u32>,
    sequence: u64,
    epoch: u64,
    versions: HashMap<u32, u64>,
    fields: HashMap<u32, Value>,
    queue: VecDeque<Mutation>,
    active: Option<Mutation>,
    history: Vec<u32>,
    field_edited: bool,
    ink: Option<Ink>,
    note: Value,
    field: Value,
}
fn num(v: &Value) -> Result<f64, &'static str> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("pdf.invalidEditRequest")
}
fn index(v: &Value) -> Result<u32, &'static str> {
    v.as_u64()
        .and_then(|n| n.try_into().ok())
        .ok_or("pdf.invalidEditRequest")
}
fn rectangle(v: &Value) -> Result<(), &'static str> {
    if v.as_array().is_none_or(|v| v.len() != 4) {
        return Err("pdf.invalidEditRequest");
    }
    let (x, y, w, h) = (num(&v[0])?, num(&v[1])?, num(&v[2])?, num(&v[3])?);
    if w < 0. || h < 0. || !(x + w).is_finite() || !(y + h).is_finite() {
        return Err("pdf.invalidEditRequest");
    }
    Ok(())
}
fn color(v: &Value) -> Result<&str, &'static str> {
    let s = v.as_str().ok_or("pdf.invalidEditRequest")?;
    if s.len() != 7 || !s.starts_with('#') || !s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err("pdf.invalidEditRequest");
    }
    Ok(s)
}
fn text(v: &Value) -> Result<&str, &'static str> {
    v.as_str()
        .filter(|s| s.len() < 65536)
        .ok_or("pdf.invalidEditRequest")
}
impl Editor {
    pub(super) fn annotation_rejected(&mut self) -> Value {
        self.packet(
            Value::Null,
            Value::Null,
            json!({"key":"pdf.annotError","code":"bad-request"}),
            Value::Null,
        )
    }
    pub(super) fn reset(&mut self) {
        // Tickets are never reused: an acknowledgement from a previous document
        // cannot accidentally complete its replacement's first edit.
        let sequence = self.sequence;
        let epoch = self.epoch.saturating_add(1);
        *self = Self::default();
        self.sequence = sequence;
        self.epoch = epoch;
    }
    fn page(&self, v: &Value) -> Result<u32, &'static str> {
        let p = index(v)?;
        if self.pages.is_some_and(|n| p >= n) {
            return Err("pdf.invalidEditRequest");
        }
        Ok(p)
    }
    fn enqueue(
        &mut self,
        kind: &'static str,
        page: Option<u32>,
        payload: Value,
    ) -> Result<(), &'static str> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("pdf.invalidEditRequest")?;
        self.queue.push_back(Mutation {
            ticket: self.sequence,
            kind,
            page,
            payload,
        });
        Ok(())
    }
    fn annotation(&mut self, value: &Value) -> Result<(), &'static str> {
        let page = self.page(&value["page"])?;
        color(&value["color"])?;
        match value["type"].as_str() {
            Some("highlight") => {
                let rects = value["rects"]
                    .as_array()
                    .filter(|v| !v.is_empty() && v.len() <= 2000)
                    .ok_or("pdf.invalidEditRequest")?;
                for r in rects {
                    rectangle(r)?;
                }
            }
            Some("note") => {
                rectangle(&value["r"])?;
                text(&value["text"])?;
            }
            Some("ink") => {
                let strokes = value["strokes"]
                    .as_array()
                    .filter(|v| !v.is_empty() && v.len() <= 500)
                    .ok_or("pdf.invalidEditRequest")?;
                let mut points = 0;
                for stroke in strokes {
                    let s = stroke
                        .as_array()
                        .filter(|s| s.len() >= 2 && s.len() % 2 == 0)
                        .ok_or("pdf.invalidEditRequest")?;
                    for n in s {
                        num(n)?;
                    }
                    points += s.len() / 2;
                }
                if points > 200000 {
                    return Err("pdf.invalidEditRequest");
                }
                let width = num(&value["width"])?;
                if width <= 0.2 || width >= 40. {
                    return Err("pdf.invalidEditRequest");
                }
            }
            _ => return Err("pdf.invalidEditRequest"),
        }
        self.enqueue("annot", Some(page), value.clone())
    }
    fn start_next(&mut self) -> Option<Value> {
        if self.active.is_some() {
            return None;
        }
        while let Some(mut item) = self.queue.pop_front() {
            if item.kind == "undo" {
                let Some(page) = self.history.last().copied() else {
                    continue;
                };
                item.page = Some(page);
                item.payload = json!({"page":page});
            }
            let effect = json!({"ticket":item.ticket,"kind":item.kind,"page":item.page,"payload":item.payload});
            self.active = Some(item);
            return Some(effect);
        }
        None
    }
    fn field_value<'a>(&'a self, id: u32, fallback: &'a Value) -> &'a Value {
        self.queue
            .iter()
            .rev()
            .chain(self.active.iter())
            .find(|a| a.kind == "field" && a.payload["id"].as_u64() == Some(u64::from(id)))
            .map(|a| &a.payload["value"])
            .or_else(|| self.fields.get(&id))
            .unwrap_or(fallback)
    }
    fn packet(&mut self, ink: Value, invalidate: Value, error: Value, saved: Value) -> Value {
        let effect = self.start_next();
        json!({"state":{"history":self.history,"dirty":self.field_edited||!self.history.is_empty(),
            "note":self.note,"field":self.field,"busy":self.active.is_some(),"queued":self.queue.len()},
            "ink":ink,"effect":effect,"invalidate":invalidate,"notice":error,"saved":saved})
    }
    pub(super) fn request(&mut self, input: &Value) -> Result<Value, &'static str> {
        let op = input["op"].as_str().unwrap_or("");
        let mut ink_change = Value::Null;
        let mut invalidation = Value::Null;
        let mut error = Value::Null;
        let mut saved = Value::Null;
        if op == "open" {
            let pages = match input.get("pages").filter(|v| !v.is_null()) {
                Some(p) => Some(index(p)?),
                None => None,
            };
            self.reset();
            self.enabled = true;
            self.pages = pages;
            ink_change = json!({"kind":"replace","value":null});
        } else if op == "state" {
            ink_change = json!({"kind":"replace","value":self.ink.as_ref().map(|i|json!({"page":i.page,"strokes":i.strokes}))});
        } else if !self.enabled {
            return Ok(Value::Null);
        } else {
            match op {
                "infoStart" => {
                    let page = self.page(&input["page"])?;
                    return Ok(
                        json!({"epoch":self.epoch,"version":self.versions.get(&page).copied().unwrap_or(0)}),
                    );
                }
                "infoReply" => {
                    let page = self.page(&input["page"])?;
                    if input["stamp"]["epoch"].as_u64() != Some(self.epoch)
                        || input["stamp"]["version"].as_u64()
                            != Some(self.versions.get(&page).copied().unwrap_or(0))
                    {
                        return Ok(Value::Null);
                    }
                    let info = &input["info"];
                    if let Some(fields) = info["fields"].as_array() {
                        for fm in fields {
                            if let Ok(id) = index(&fm["f"]["id"]) {
                                if !self.queue.iter().chain(self.active.iter()).any(|a| {
                                    a.kind == "field"
                                        && a.payload["id"].as_u64() == Some(u64::from(id))
                                }) {
                                    self.fields.insert(id, fm["f"]["value"].clone());
                                }
                            }
                        }
                    }
                    return Ok(info.clone());
                }
                "annotate" => self.annotation(&input["annotation"])?,
                "undo" => self.enqueue("undo", None, Value::Null)?,
                "save" => {
                    let path = input["path"]
                        .as_str()
                        .filter(|p| !p.is_empty() && !p.contains('\0') && p.len() <= 4096)
                        .ok_or("pdf.invalidEditRequest")?;
                    let name = input["name"].as_str().ok_or("pdf.invalidEditRequest")?;
                    self.enqueue("save", None, json!({"path":path,"name":name}))?;
                }
                "complete" => {
                    let ticket = input["ticket"].as_u64().ok_or("pdf.invalidEditRequest")?;
                    if self.active.as_ref().is_none_or(|a| a.ticket != ticket) {
                        return Ok(Value::Null);
                    }
                    let ok = input["ok"].as_bool().ok_or("pdf.invalidEditRequest")?;
                    let item = self.active.take().unwrap();
                    if ok {
                        match item.kind {
                            "annot" => self.history.push(item.page.unwrap()),
                            "undo" => {
                                self.history.pop();
                            }
                            "field" => {
                                self.field_edited = true;
                                self.fields.insert(
                                    index(&item.payload["id"])?,
                                    item.payload["value"].clone(),
                                );
                            }
                            "save" => saved = item.payload["name"].clone(),
                            _ => unreachable!(),
                        }
                        if let Some(page) = item.page {
                            let version = self.versions.entry(page).or_default();
                            *version = version.saturating_add(1);
                            invalidation = json!({"page":page,"refresh":true});
                        }
                    } else if item.kind == "annot" || item.kind == "save" {
                        error = json!({"key":if item.kind=="save"{"pdf.saveError"}else{"pdf.annotError"},"message":input["message"].as_str().unwrap_or("")});
                    }
                }
                "inkStart" => {
                    let p = &input["point"];
                    let page = self.page(&p["page"])?;
                    let point = vec![num(&p["x"])?, num(&p["y"])?];
                    if let Some(ink) = self.ink.as_mut().filter(|i| i.page == page) {
                        if ink.strokes.len() >= 500 || ink.points >= 200000 {
                            return Err("pdf.invalidEditRequest");
                        }
                        ink.strokes.push(point.clone());
                        ink.points += 1;
                        ink_change = json!({"kind":"stroke","page":page,"point":point});
                    } else {
                        self.ink = Some(Ink {
                            page,
                            strokes: vec![point.clone()],
                            points: 1,
                        });
                        ink_change =
                            json!({"kind":"replace","value":{"page":page,"strokes":[point]}});
                    }
                }
                "inkAppend" => {
                    let p = &input["point"];
                    let page = self.page(&p["page"])?;
                    let point = [num(&p["x"])?, num(&p["y"])?];
                    let scale = num(&input["scale"])?;
                    if scale <= 0. {
                        return Err("pdf.invalidEditRequest");
                    }
                    let Some(ink) = self.ink.as_mut().filter(|i| i.page == page) else {
                        return Ok(Value::Null);
                    };
                    let stroke = ink.strokes.last_mut().unwrap();
                    let n = stroke.len();
                    let distance = (point[0] - stroke[n - 2]).hypot(point[1] - stroke[n - 1]);
                    if !distance.is_finite() || ink.points >= 200000 {
                        return Err("pdf.invalidEditRequest");
                    }
                    if distance < 1.2 / (scale / (96. / 72.)) {
                        return Ok(Value::Null);
                    }
                    stroke.extend(point);
                    ink.points += 1;
                    ink_change = json!({"kind":"point","point":point});
                }
                "inkFinish" => {
                    if let Some(ink) = &self.ink {
                        let a = json!({"page":ink.page,"type":"ink","strokes":ink.strokes,"color":color(&input["color"])? ,"width":2.2});
                        self.annotation(&a)?;
                    }
                    self.ink = None;
                    ink_change = json!({"kind":"replace","value":null});
                }
                "noteOpen" => {
                    let p = &input["point"];
                    let page = self.page(&p["page"])?;
                    self.note =
                        json!({"page":page,"x":num(&p["x"])? ,"y":num(&p["y"])? ,"text":""});
                }
                "noteChange" => {
                    let value = input["value"].as_str().ok_or("pdf.invalidEditRequest")?;
                    if self.note.is_null() {
                        return Ok(Value::Null);
                    }
                    self.note["text"] = json!(value);
                }
                "noteCancel" => self.note = Value::Null,
                "noteCommit" => {
                    if self.note.is_null() {
                        return Ok(Value::Null);
                    }
                    let body = trim_query(text(&self.note["text"])?);
                    if !body.is_empty() {
                        let a = json!({"page":self.note["page"],"type":"note","r":[self.note["x"],self.note["y"],22,22],"text":body,"color":"#ffd60a"});
                        self.annotation(&a)?;
                    }
                    self.note = Value::Null;
                }
                "fieldOpen" => {
                    let page = self.page(&input["page"])?;
                    let fm = &input["field"];
                    let f = &fm["f"];
                    if f["ro"] == true {
                        return Ok(Value::Null);
                    }
                    let id = index(&f["id"])?;
                    rectangle(&fm["r"])?;
                    let value = self.field_value(id, &f["value"]).clone();
                    let mut facts = f.clone();
                    facts["value"] = value.clone();
                    match f["type"].as_str() {
                        Some("check") | Some("radio") => {
                            let value = !value.as_bool().unwrap_or(false);
                            self.enqueue("field", Some(page), json!({"id":id,"value":value}))?;
                        }
                        Some("text") => {
                            self.field = json!({"page":page,"f":facts,"r":fm["r"],"value":value.as_str().unwrap_or("")})
                        }
                        Some("choice") => {
                            if !f["items"].is_array() {
                                return Err("pdf.invalidEditRequest");
                            }
                            self.field = json!({"page":page,"f":facts,"r":fm["r"],"value":value});
                        }
                        _ => return Ok(Value::Null),
                    }
                }
                "fieldChange" => {
                    if self.field.is_null() {
                        return Ok(Value::Null);
                    }
                    if self.field["f"]["type"] != "text" {
                        return Err("pdf.invalidEditRequest");
                    }
                    self.field["value"] =
                        json!(input["value"].as_str().ok_or("pdf.invalidEditRequest")?);
                }
                "fieldCancel" => self.field = Value::Null,
                "fieldCommit" => {
                    if self.field.is_null() {
                        return Ok(Value::Null);
                    }
                    let value = input.get("value").unwrap_or(&self.field["value"]);
                    if self.field["f"]["type"] == "text" {
                        text(value)?;
                    } else {
                        let i = index(value)? as usize;
                        if i >= self.field["f"]["items"].as_array().unwrap().len() {
                            return Err("pdf.invalidEditRequest");
                        }
                    }
                    let page = self.page(&self.field["page"])?;
                    let payload = json!({"id":self.field["f"]["id"],"value":value});
                    self.enqueue("field", Some(page), payload)?;
                    self.field = Value::Null;
                }
                _ => return Err("pdf.invalidEditRequest"),
            }
        }
        Ok(self.packet(ink_change, invalidation, error, saved))
    }
}
