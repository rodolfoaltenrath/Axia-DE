//! Viewer actions and async result ownership, independent of React and helpers.
use serde_json::{json, Value};
const PX: f64 = 96. / 72.;
const GAP: f64 = 14.;
const ZOOMS: [f64; 16] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.,
];
struct Find {
    query: String,
    list: Vec<Value>,
    cur: usize,
    done: bool,
}
impl Find {
    fn json(&self) -> Value {
        json!({"q":self.query,"list":self.list,"cur":self.cur,"done":self.done})
    }
    fn current(&self) -> Value {
        self.list.get(self.cur).cloned().unwrap_or(Value::Null)
    }
}
#[derive(Default)]
pub(super) struct Viewer {
    search_id: u64,
    find: Option<Find>,
    selection_id: u64,
    selection: Value,
    wheel_at: f64,
}
fn num(v: &Value) -> Result<f64, &'static str> {
    v.as_f64()
        .filter(|v| v.is_finite())
        .ok_or("pdf.invalidViewerRequest")
}
pub(super) fn trim_query(value: &str) -> &str {
    // Match String.trim: Rust's Unicode White_Space set differs for BOM/NEL.
    value.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}
fn rects(value: &Value) -> Result<&Vec<Value>, &'static str> {
    let list = value.as_array().ok_or("pdf.invalidViewerRequest")?;
    for r in list {
        if r.as_array().is_none_or(|r| r.len() != 4) {
            return Err("pdf.invalidViewerRequest");
        }
        for i in 0..4 {
            num(&r[i])?;
        }
    }
    Ok(list)
}
fn key(input: &Value) -> Value {
    if input["editing"] == true {
        return Value::Null;
    }
    let e = &input["event"];
    let key = e["key"].as_str().unwrap_or("");
    let b = e["base"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| e["char"].as_str())
        .unwrap_or("")
        .to_lowercase();
    let shift = e["shift"] == true;
    let action = if e["meta"] == true || e["ctrl"] == true {
        match b.as_str() {
            "f" => Some("find"),
            "c" => Some("copy"),
            "a" if input["hasDoc"] == true => Some("selectAll"),
            "p" => Some("print"),
            "s" => Some("save"),
            "z" => Some("undo"),
            "r" => Some(if shift { "rotateLeft" } else { "rotateRight" }),
            "+" | "=" => Some("zoomIn"),
            "-" => Some("zoomOut"),
            "0" => Some("zoomActual"),
            "9" => Some("zoomWidth"),
            "g" if e["alt"] == true => Some("goto"),
            "g" => Some(if shift { "previousHit" } else { "nextHit" }),
            _ => None,
        }
    } else {
        None
    };
    let action = action.or(match key {
        "Escape" => Some("escape"),
        "PageDown" => Some("pageDown"),
        "PageUp" => Some("pageUp"),
        " " => Some(if shift { "pageUp" } else { "pageDown" }),
        "ArrowDown" => Some("down"),
        "ArrowUp" => Some("up"),
        "ArrowRight" => Some("nextPage"),
        "ArrowLeft" => Some("previousPage"),
        "Home" => Some("firstPage"),
        "End" => Some("lastPage"),
        _ => None,
    });
    action.map(|a| json!(a)).unwrap_or(Value::Null)
}
impl Viewer {
    pub(super) fn request(&mut self, input: &Value) -> Result<Value, &'static str> {
        match input["op"].as_str().unwrap_or("") {
            "scale" => {
                let (w, h) = (num(&input["size"][0])?, num(&input["size"][1])?);
                let (vw, vh) = (num(&input["view"]["w"])?, num(&input["view"]["h"])?);
                if w <= 0. || h <= 0. {
                    return Err("pdf.invalidViewerRequest");
                }
                let scale = match input["zoom"].as_str() {
                    Some("width") => ((vw - 2. * GAP - 12.) / w).max(0.1),
                    Some("page") => ((vw - 2. * GAP) / w).min((vh - 2. * GAP) / h).max(0.1),
                    _ => num(&input["zoom"])? * PX,
                };
                if !scale.is_finite() || scale <= 0. {
                    return Err("pdf.invalidViewerRequest");
                }
                let pct = (scale / PX * 100. + 0.5).floor();
                if !pct.is_finite() {
                    return Err("pdf.invalidViewerRequest");
                }
                Ok(json!({"scale":scale,"pct":pct}))
            }
            "zoom" => {
                let cur = num(&input["scale"])? / PX;
                let up = num(&input["step"])? > 0.;
                let next = if up {
                    ZOOMS
                        .iter()
                        .copied()
                        .find(|z| *z > cur + 0.01)
                        .unwrap_or(5.)
                } else {
                    ZOOMS
                        .iter()
                        .rev()
                        .copied()
                        .find(|z| *z < cur - 0.01)
                        .unwrap_or(0.25)
                };
                Ok(json!(next))
            }
            "rotate" => {
                let r = input["rot"].as_i64().ok_or("pdf.invalidViewerRequest")?;
                let d = input["delta"].as_i64().ok_or("pdf.invalidViewerRequest")?;
                Ok(json!((i128::from(r) + i128::from(d)).rem_euclid(4) as i64))
            }
            "scroll" | "keyScroll" => {
                let max = num(&input["max"])?;
                let delta = if input["op"] == "keyScroll" {
                    match input["action"].as_str() {
                        Some("pageDown") => num(&input["height"])? * 0.9,
                        Some("pageUp") => -num(&input["height"])? * 0.9,
                        Some("down") => 48.,
                        Some("up") => -48.,
                        _ => return Err("pdf.invalidViewerRequest"),
                    }
                } else {
                    num(&input["delta"])?
                };
                let value = num(&input["value"])? + delta;
                if max < 0. || !value.is_finite() {
                    return Err("pdf.invalidViewerRequest");
                }
                Ok(json!(value.min(max).max(0.)))
            }
            "wheel" => {
                let e = &input["event"];
                let delta = num(&e["deltaY"])?;
                if e["ctrl"] == true || e["meta"] == true {
                    return Ok(if e["precise"] != true || e["notches"] == true {
                        json!({"kind":"zoom","step":if delta<0. {1} else {-1}})
                    } else {
                        Value::Null
                    });
                }
                if e["shift"] == true {
                    return Ok(json!({"kind":"horizontal","delta":delta}));
                }
                if input["single"] == true {
                    let now = num(&input["now"])?;
                    if now - self.wheel_at < 250. {
                        return Ok(Value::Null);
                    }
                    self.wheel_at = now;
                    return Ok(json!({"kind":"page","delta":if delta>0. {1} else {-1}}));
                }
                Ok(json!({"kind":"vertical","delta":delta}))
            }
            "key" => Ok(key(input)),
            "hitItem" => {
                let x = num(&input["x"])?;
                let y = num(&input["y"])?;
                let items = input["items"]
                    .as_array()
                    .ok_or("pdf.invalidViewerRequest")?;
                for (i, item) in items.iter().enumerate() {
                    let r = &item["r"];
                    let (a, b, w, h) = (num(&r[0])?, num(&r[1])?, num(&r[2])?, num(&r[3])?);
                    if x >= a && x <= a + w && y >= b && y <= b + h {
                        return Ok(json!(i));
                    }
                }
                Ok(json!(-1))
            }
            "reset" => {
                self.search_id += 1;
                self.selection_id += 1;
                self.find = None;
                self.selection = Value::Null;
                Ok(Value::Null)
            }
            "findStart" => {
                let query = input["query"].as_str().ok_or("pdf.invalidViewerRequest")?;
                self.search_id += 1;
                self.find = if trim_query(query).is_empty() {
                    None
                } else {
                    Some(Find {
                        query: query.into(),
                        list: Vec::new(),
                        cur: 0,
                        done: false,
                    })
                };
                Ok(
                    json!({"token":self.search_id,"query":if self.find.is_some(){json!(trim_query(query))}else{Value::Null},"hits":self.find.as_ref().map(Find::json)}),
                )
            }
            "findAppend" => {
                if input["token"].as_u64() != Some(self.search_id) || self.find.is_none() {
                    return Ok(Value::Null);
                }
                let page = input["page"].as_u64().ok_or("pdf.invalidViewerRequest")?;
                let rects = rects(&input["rects"])?;
                let find = self.find.as_mut().unwrap();
                let first = find.list.is_empty();
                for rect in rects {
                    find.list.push(json!({"page":page,"rect":rect}));
                }
                Ok(
                    json!({"hits":find.json(),"go":if first && !rects.is_empty(){find.current()}else{Value::Null}}),
                )
            }
            "findFinish" => {
                if input["token"].as_u64() != Some(self.search_id) {
                    return Ok(Value::Null);
                }
                Ok(self
                    .find
                    .as_mut()
                    .map(|f| {
                        f.done = true;
                        f.json()
                    })
                    .unwrap_or(Value::Null))
            }
            "findNext" => {
                let delta = input["delta"].as_i64().ok_or("pdf.invalidViewerRequest")?;
                let Some(find) = self.find.as_mut().filter(|f| !f.list.is_empty()) else {
                    return Ok(Value::Null);
                };
                find.cur = (find.cur as i128 + i128::from(delta))
                    .rem_euclid(find.list.len() as i128) as usize;
                Ok(json!({"hits":find.json(),"go":find.current()}))
            }
            "selectionStart" => {
                self.selection_id += 1;
                Ok(json!(self.selection_id))
            }
            "selectionClear" => {
                self.selection_id += 1;
                self.selection = Value::Null;
                Ok(Value::Null)
            }
            "selectionReply" => {
                if input["token"].as_u64() != Some(self.selection_id) {
                    return Ok(Value::Null);
                }
                let page = input["page"].as_u64().ok_or("pdf.invalidViewerRequest")?;
                let text = input["result"]["text"]
                    .as_str()
                    .ok_or("pdf.invalidViewerRequest")?;
                let rects = rects(&input["result"]["rects"])?;
                self.selection = json!({"page":page,"rects":rects,"text":text});
                Ok(self.selection.clone())
            }
            _ => Err("pdf.invalidViewerRequest"),
        }
    }
}
