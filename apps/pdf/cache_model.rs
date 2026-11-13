//! Metadata only: the host owns and releases each raster handle exactly once.
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone)]
struct Render {
    token: u64,
    key: String,
    page: u32,
    width: u64,
    rot: i64,
    inv: bool,
    generation: u64,
}
struct Entry {
    render: Render,
    w: u32,
    h: u32,
    bytes: u64,
    used: u64,
    order: u64,
    ms: f64,
}
impl Entry {
    fn json(&self) -> Value {
        json!({"key":self.render.key,"page":self.render.page,"width":self.render.width,
            "rot":self.render.rot,"inv":self.render.inv,"gen":self.render.generation,
            "w":self.w,"h":self.h,"bytes":self.bytes,"used":self.used,"ms":self.ms})
    }
}
pub(super) struct Cache {
    entries: HashMap<String, Entry>,
    pending: HashMap<u64, Render>,
    pending_keys: HashMap<String, u64>,
    generations: HashMap<u32, u64>,
    budget: f64,
    bytes: u64,
    tick: u64,
    sequence: u64,
    first_at: f64,
    closed: bool,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            pending: HashMap::new(),
            pending_keys: HashMap::new(),
            generations: HashMap::new(),
            budget: 48. * 1024. * 1024.,
            bytes: 0,
            tick: 0,
            sequence: 0,
            first_at: 0.,
            closed: false,
        }
    }
}
fn finite(v: &Value) -> Result<f64, &'static str> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or("pdf.invalidCacheRequest")
}
fn page(v: &Value) -> Result<u32, &'static str> {
    v.as_u64()
        .and_then(|x| x.try_into().ok())
        .ok_or("pdf.invalidCacheRequest")
}
fn width(v: &Value) -> Result<u64, &'static str> {
    let n = ((finite(v)? / 32. + 0.5).floor() * 32.).max(32.);
    if n > u32::MAX as f64 {
        return Err("pdf.invalidCacheRequest");
    }
    Ok(n as u64)
}
impl Cache {
    fn generation(&self, page: u32) -> u64 {
        self.generations.get(&page).copied().unwrap_or(0)
    }
    fn render(&self, input: &Value) -> Result<Render, &'static str> {
        let page = page(&input["page"])?;
        let width = width(&input["width"])?;
        let rot = input["rot"].as_i64().ok_or("pdf.invalidCacheRequest")?;
        let inv = input["inv"].as_bool().ok_or("pdf.invalidCacheRequest")?;
        let generation = self.generation(page);
        Ok(Render {
            token: 0,
            key: format!("{page}|{width}|{rot}|{}|{generation}", u8::from(inv)),
            page,
            width,
            rot,
            inv,
            generation,
        })
    }
    fn remove(&mut self, keys: Vec<String>, release: &mut Vec<String>) {
        for key in keys {
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
                release.push(key);
            }
        }
    }
    fn stale(&mut self, page: u32, release: &mut Vec<String>) {
        let generation = self.generation(page);
        let mut keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.render.page == page && e.render.generation != generation)
            .map(|(k, e)| (e.order, k.clone()))
            .collect();
        keys.sort_by_key(|(order, _)| *order);
        self.remove(keys.into_iter().map(|(_, key)| key).collect(), release);
    }
    fn evict(&mut self, except: &str, release: &mut Vec<String>) {
        if self.bytes as f64 <= self.budget {
            return;
        }
        let mut keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(k, _)| k.as_str() != except)
            .map(|(k, e)| (e.used, e.order, k.clone()))
            .collect();
        keys.sort_by_key(|(used, order, _)| (*used, *order));
        for (_, _, key) in keys {
            if self.bytes as f64 <= self.budget * 0.8 {
                break;
            }
            self.remove(vec![key], release);
        }
    }
    fn finish(&mut self, token: u64) -> Option<Render> {
        let render = self.pending.remove(&token)?;
        self.pending_keys.remove(&render.key);
        Some(render)
    }
    pub(super) fn request(&mut self, input: &Value) -> Result<Value, &'static str> {
        let mut release = Vec::new();
        let value = match input["op"].as_str().unwrap_or("") {
            "configure" => {
                let budget = finite(&input["budget"])?;
                if budget < 0. {
                    return Err("pdf.invalidCacheRequest");
                }
                self.budget = budget;
                Value::Null
            }
            "key" | "get" => {
                let render = self.render(input)?;
                if input["op"] == "key" {
                    json!(render.key)
                } else if let Some(entry) = self.entries.get_mut(&render.key) {
                    self.tick += 1;
                    entry.used = self.tick;
                    entry.json()
                } else {
                    Value::Null
                }
            }
            "best" => {
                let page = page(&input["page"])?;
                let rot = input["rot"].as_i64().ok_or("pdf.invalidCacheRequest")?;
                let inv = input["inv"].as_bool().ok_or("pdf.invalidCacheRequest")?;
                let width = finite(&input["width"])?;
                let generation = self.generation(page);
                let score = |e: &Entry| if e.render.generation == generation { 0. } else { 1e6 } + (e.render.width as f64 - width).abs();
                self.entries
                    .values()
                    .filter(|e| e.render.page == page && e.render.rot == rot && e.render.inv == inv)
                    .min_by(|a, b| score(a).total_cmp(&score(b)).then(a.order.cmp(&b.order)))
                    .map(Entry::json)
                    .unwrap_or(Value::Null)
            }
            "begin" => {
                let mut render = self.render(input)?;
                if self.closed
                    || self.entries.contains_key(&render.key)
                    || self.pending_keys.contains_key(&render.key)
                {
                    Value::Null
                } else {
                    self.sequence += 1;
                    render.token = self.sequence;
                    let value = json!({"token":render.token,"key":render.key,"width":render.width});
                    self.pending_keys.insert(render.key.clone(), render.token);
                    self.pending.insert(render.token, render);
                    value
                }
            }
            "complete" => {
                // Validate before consuming a live ticket so malformed requests
                // cannot disturb a different in-flight render.
                let token = input["token"].as_u64().ok_or("pdf.invalidCacheRequest")?;
                let w = page(&input["w"])?;
                let h = page(&input["h"])?;
                let now = finite(&input["now"])?;
                let ms = finite(&input["ms"])?;
                if w > 32768 || h > 32768 {
                    return Err("pdf.invalidCacheRequest");
                }
                if let Some(render) = self.finish(token) {
                    if self.closed || render.generation != self.generation(render.page) {
                        Value::Null
                    } else {
                        self.tick += 1;
                        let bytes = u64::from(w) * u64::from(h) * 4;
                        let key = render.key.clone();
                        let page = render.page;
                        let entry = Entry {
                            render,
                            w,
                            h,
                            bytes,
                            used: self.tick,
                            order: self.tick,
                            ms,
                        };
                        let result = entry.json();
                        self.bytes += bytes;
                        self.entries.insert(key.clone(), entry);
                        if self.first_at == 0. {
                            self.first_at = now;
                        }
                        self.stale(page, &mut release);
                        self.evict(&key, &mut release);
                        result
                    }
                } else {
                    Value::Null
                }
            }
            "fail" => {
                self.finish(input["token"].as_u64().ok_or("pdf.invalidCacheRequest")?);
                Value::Null
            }
            "drop" => {
                let keep = input["keep"]
                    .as_array()
                    .ok_or("pdf.invalidCacheRequest")?
                    .iter()
                    .map(page)
                    .collect::<Result<Vec<_>, _>>()?;
                let mut tokens: Vec<_> = self
                    .pending
                    .values()
                    .filter(|r| !keep.contains(&r.page))
                    .map(|r| r.token)
                    .collect();
                tokens.sort_unstable();
                json!(tokens)
            }
            "invalidate" => {
                let page = page(&input["page"])?;
                self.generations.insert(page, self.generation(page) + 1);
                Value::Null
            }
            "purge" => {
                self.stale(page(&input["page"])?, &mut release);
                Value::Null
            }
            "evict" => {
                self.evict(input["except"].as_str().unwrap_or(""), &mut release);
                Value::Null
            }
            "clear" | "close" => {
                if input["op"] == "close" {
                    self.closed = true;
                }
                let mut keys: Vec<_> = self
                    .entries
                    .iter()
                    .map(|(k, e)| (e.order, k.clone()))
                    .collect();
                keys.sort_by_key(|(order, _)| *order);
                self.remove(keys.into_iter().map(|(_, key)| key).collect(), &mut release);
                Value::Null
            }
            "stats" => Value::Null,
            _ => return Err("pdf.invalidCacheRequest"),
        };
        Ok(
            json!({"value":value,"release":release,"stats":{"bytes":self.bytes,"tick":self.tick,
            "firstAt":self.first_at,"closed":self.closed}}),
        )
    }
}
