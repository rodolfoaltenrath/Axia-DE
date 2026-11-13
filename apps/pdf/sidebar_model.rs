//! Retained thumbnail layout and bookmark expansion, independent per document.
//! The host only supplies viewport changes and paints the returned rows.
use super::geometry::shown;
use serde_json::{json, Value};
use std::collections::HashMap;

const WIDTH: f64 = 116.;
const ERROR: &str = "pdf.invalidSidebar";
#[derive(Default)]
pub(super) struct Sidebar {
    thumbnails: Vec<Thumbnail>,
    total: f64,
    height: f64,
    offset: f64,
    page: Option<usize>,
    outline: Vec<Bookmark>,
    index: HashMap<String, usize>,
}
struct Thumbnail {
    top: f64,
    height: f64,
}
struct Bookmark {
    key: String,
    title: String,
    page: i64,
    depth: usize,
    end: usize,
    has_kids: bool,
    open: bool,
}
fn number(v: &Value) -> Result<f64, &'static str> {
    v.as_f64().filter(|n| n.is_finite()).ok_or(ERROR)
}
fn bookmarks(
    items: &Value,
    path: &str,
    depth: usize,
    out: &mut Vec<Bookmark>,
) -> Result<(), &'static str> {
    let items = items.as_array().ok_or(ERROR)?;
    if depth > 64 || items.len() > 100_000 {
        return Err(ERROR);
    }
    for (i, item) in items.iter().enumerate() {
        if out.len() >= 100_000 {
            return Err(ERROR);
        }
        let key = format!("{path}{i}");
        let start = out.len();
        let page = item["page"].as_i64().unwrap_or(-1);
        let has_kids = item
            .get("kids")
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false));
        out.push(Bookmark {
            key: key.clone(),
            title: item["title"].as_str().ok_or(ERROR)?.to_owned(),
            page,
            depth,
            end: 0,
            has_kids,
            open: item["open"].as_bool().unwrap_or(false),
        });
        if has_kids {
            bookmarks(&item["kids"], &format!("{key}."), depth + 1, out)?;
        }
        out[start].end = out.len();
    }
    Ok(())
}
impl Sidebar {
    fn clamp(&mut self) {
        self.offset = self.offset.max(0.).min((self.total - self.height).max(0.));
    }
    fn thumbnails(&self) -> Value {
        // Non-overlapping, ordered rows permit logarithmic skipping for long PDFs.
        let first = self
            .thumbnails
            .partition_point(|t| t.top + t.height + 28. < self.offset);
        let frames: Vec<_> = self.thumbnails.iter().enumerate().skip(first)
            .take_while(|(_, t)| t.top <= self.offset + self.height)
            .map(|(page, t)| json!({"page":page,"top":t.top-self.offset,"height":t.height,"selected":Some(page)==self.page}))
            .collect();
        json!({"offset":self.offset,"total":self.total,"width":WIDTH,"frames":frames})
    }
    fn outline(&self) -> Value {
        let mut rows = Vec::new();
        let mut i = 0;
        while let Some(item) = self.outline.get(i) {
            rows.push(json!({"key":item.key,"title":item.title,"page":item.page,"depth":item.depth,"hasKids":item.has_kids,"open":item.open}));
            i = if item.open { i + 1 } else { item.end };
        }
        json!({"rows":rows})
    }
    pub(super) fn request(&mut self, input: &Value) -> Result<Value, &'static str> {
        match input["op"].as_str().unwrap_or("") {
            "configure" => {
                let sizes = input["sizes"]
                    .as_array()
                    .filter(|a| a.len() <= 100_000)
                    .ok_or(ERROR)?;
                let rot = input["rot"].as_i64().ok_or(ERROR)?;
                let mut thumbnails = Vec::with_capacity(sizes.len());
                let mut top = 8.;
                for size in sizes {
                    let [w, h] = shown([number(&size[0])?, number(&size[1])?], rot);
                    if w <= 0. || h <= 0. {
                        return Err(ERROR);
                    }
                    let height = (WIDTH * h / w).round();
                    let next = top + height + 28.;
                    if !height.is_finite() || !(next + 8.).is_finite() {
                        return Err(ERROR);
                    }
                    thumbnails.push(Thumbnail { top, height });
                    top = next;
                }
                self.thumbnails = thumbnails;
                self.total = top + 8.;
                self.clamp();
                Ok(Value::Null)
            }
            "viewport" => {
                let height = number(&input["height"])?;
                let page = input["page"].as_u64().ok_or(ERROR)? as usize;
                if height < 0. {
                    return Err(ERROR);
                }
                self.height = height;
                if self.page != Some(page) {
                    if let Some(t) = self.thumbnails.get(page) {
                        if t.top < self.offset {
                            self.offset = (t.top - 8.).max(0.);
                        } else if t.top + t.height + 28. > self.offset + height {
                            self.offset = t.top + t.height + 28. - height + 8.;
                        }
                    }
                    self.page = Some(page);
                }
                self.clamp();
                Ok(self.thumbnails())
            }
            "scroll" => {
                let offset = self.offset + number(&input["delta"])?;
                if !offset.is_finite() {
                    return Err(ERROR);
                }
                self.offset = offset;
                self.clamp();
                Ok(self.thumbnails())
            }
            "outline" => {
                let mut outline = Vec::new();
                bookmarks(&input["items"], "", 0, &mut outline)?;
                self.index = outline
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (b.key.clone(), i))
                    .collect();
                self.outline = outline;
                Ok(self.outline())
            }
            "toggle" => {
                let key = input["key"].as_str().ok_or(ERROR)?;
                let i = *self.index.get(key).ok_or(ERROR)?;
                if self.outline[i].has_kids {
                    self.outline[i].open = !self.outline[i].open;
                }
                Ok(self.outline())
            }
            "pick" => {
                let key = input["key"].as_str().ok_or(ERROR)?;
                let i = *self.index.get(key).ok_or(ERROR)?;
                Ok(if self.outline[i].page >= 0 {
                    json!(self.outline[i].page)
                } else {
                    Value::Null
                })
            }
            _ => Err(ERROR),
        }
    }
}
