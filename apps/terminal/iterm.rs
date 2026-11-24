//! Import of profiles and color schemes exported by other terminal emulators, a port of the
//! former `iterm.js`. Accepted: profile JSON (also Dynamic Profiles), `.itermcolors` presets
//! (XML plist) and the full iTerm2 preferences (XML or binary plist: every profile, the default
//! one and the global shortcuts). What the terminal does not have is reported in `warnings`,
//! never made up. Translation, font lookup and ids come from the caller.
use regex::Regex;
use serde_json::{json, Map, Value};

/// What the importer needs from its caller.
pub struct Env<'a> {
    /// The factory profile every imported one starts from.
    pub defaults: &'a Value,
    /// A translated caption: key and parameters.
    pub tr: &'a dyn Fn(&str, Value) -> String,
    /// The installed font family for a PostScript name, if any.
    pub font: &'a dyn Fn(&str) -> Option<String>,
    /// A fresh profile id.
    pub id: &'a mut dyn FnMut() -> String,
}
pub enum Input<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
}
#[derive(Default, Debug)]
pub struct Imported {
    pub profiles: Vec<Value>,
    pub schemes: Vec<Value>,
    pub key_map: Vec<Value>,
    pub default_guid: Option<String>,
    pub warnings: Vec<String>,
}
enum Fail {
    Key(&'static str),
    Raw(String),
}

// ---- JavaScript-like coercions (the exports are loosely typed) ----

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0. && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn to_num(v: &Value) -> f64 {
    match v {
        Value::Null => 0.,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0. } else { t.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
fn to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0. && f.abs() < 1e15 => format!("{}", f as i64),
            Some(f) => f.to_string(),
            None => n.to_string(),
        },
        Value::Array(a) => a.iter().map(to_str).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}
fn num(n: f64) -> Value {
    if n.is_nan() || n.is_infinite() {
        Value::Null
    } else if n.fract() == 0. && n.abs() < 9e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}
fn clamp01(v: &Value) -> f64 {
    let n = to_num(v);
    let n = if n.is_nan() || n == 0. { 0. } else { n };
    n.clamp(0., 1.)
}

// ---- plist XML ----

struct Tok {
    tag: String,
    close: bool,
    empty: bool,
    text: Option<String>,
}
fn unescape(s: &str) -> String {
    let named = s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
    let decimal = Regex::new(r"&#(\d+);").unwrap();
    let hex = Regex::new(r"(?i)&#x([0-9a-f]+);").unwrap();
    let step = decimal.replace_all(&named, |c: &regex::Captures| {
        c[1].parse::<u32>().ok().and_then(char::from_u32).map_or(String::new(), String::from)
    });
    let step = hex.replace_all(&step, |c: &regex::Captures| {
        u32::from_str_radix(&c[1], 16).ok().and_then(char::from_u32).map_or(String::new(), String::from)
    });
    step.replace("&amp;", "&")
}
struct Xml {
    tokens: Vec<Tok>,
    at: usize,
}
impl Xml {
    fn new(text: &str) -> Self {
        let re = Regex::new(r"<(/?)([a-zA-Z]+)(\s*/)?>|([^<]+)").unwrap();
        let mut tokens = vec![];
        for c in re.captures_iter(text) {
            if let Some(t) = c.get(4) {
                if !t.as_str().trim().is_empty() {
                    tokens.push(Tok { tag: String::new(), close: false, empty: false, text: Some(t.as_str().to_owned()) });
                }
            } else {
                tokens.push(Tok { tag: c[2].to_owned(), close: !c[1].is_empty(), empty: c.get(3).is_some(), text: None });
            }
        }
        Self { tokens, at: 0 }
    }
    fn is_close(&self, tag: &str) -> bool {
        self.tokens.get(self.at).is_some_and(|t| t.tag == tag && t.close)
    }
    fn text_until(&mut self, tag: &str) -> String {
        let mut s = String::new();
        while self.at < self.tokens.len() && !self.is_close(tag) {
            if let Some(t) = &self.tokens[self.at].text {
                s.push_str(t);
            }
            self.at += 1;
        }
        self.at += 1;
        unescape(&s)
    }
    fn value(&mut self, depth: usize) -> Value {
        if depth > 200 {
            return Value::Null;
        }
        while self.at < self.tokens.len() && (self.tokens[self.at].text.is_some() || matches!(self.tokens[self.at].tag.as_str(), "plist" | "xml")) {
            self.at += 1;
        }
        let Some(t) = self.tokens.get(self.at) else { return Value::Null };
        let (tag, empty) = (t.tag.clone(), t.empty);
        self.at += 1;
        if empty {
            return match tag.as_str() {
                "true" => json!(true),
                "false" => json!(false),
                "dict" => json!({}),
                "array" => json!([]),
                _ => json!(""),
            };
        }
        match tag.as_str() {
            "dict" => {
                let mut o = Map::new();
                while self.at < self.tokens.len() && !self.is_close("dict") {
                    if self.tokens[self.at].tag == "key" && !self.tokens[self.at].close {
                        self.at += 1;
                        let key = self.text_until("key");
                        let value = self.value(depth + 1);
                        o.insert(key, value);
                    } else {
                        self.at += 1;
                    }
                }
                self.at += 1;
                Value::Object(o)
            }
            "array" => {
                let mut a = vec![];
                while self.at < self.tokens.len() && !self.is_close("array") {
                    if self.tokens[self.at].text.is_some() {
                        self.at += 1;
                        continue;
                    }
                    a.push(self.value(depth + 1));
                }
                self.at += 1;
                Value::Array(a)
            }
            "string" | "date" | "data" => json!(self.text_until(&tag)),
            "real" | "integer" => num(self.text_until(&tag).trim().parse().unwrap_or(f64::NAN)),
            "true" => {
                self.at += 1;
                json!(true)
            }
            "false" => {
                self.at += 1;
                json!(false)
            }
            _ => Value::Null,
        }
    }
}

// ---- binary plist (bplist00) ----

struct Binary<'a> {
    d: &'a [u8],
    objects: u64,
    ref_size: usize,
    offset_size: usize,
    table: usize,
    seen: Vec<u64>,
}
impl Binary<'_> {
    fn byte(&self, o: usize) -> Result<u8, Fail> {
        self.d.get(o).copied().ok_or(Fail::Key("terminal.iterm.badRef"))
    }
    fn uint(&self, o: usize, size: usize) -> Result<u64, Fail> {
        let mut v = 0u64;
        for k in 0..size {
            v = v.wrapping_mul(256).wrapping_add(u64::from(self.byte(o + k)?));
        }
        Ok(v)
    }
    fn len(&self, o: usize, low: u8) -> Result<(usize, usize), Fail> {
        if low != 15 {
            return Ok((usize::from(low), o + 1));
        }
        let m = self.byte(o + 1)?;
        let size = 1usize << (m & 15).min(3);
        Ok((self.uint(o + 2, size)? as usize, o + 2 + size))
    }
    fn obj(&mut self, idx: u64) -> Result<Value, Fail> {
        if idx >= self.objects {
            return Err(Fail::Key("terminal.iterm.badRef"));
        }
        if self.seen.contains(&idx) {
            return Err(Fail::Key("terminal.iterm.circular"));
        }
        self.seen.push(idx);
        let out = self.read(idx);
        self.seen.pop();
        out
    }
    fn read(&mut self, idx: u64) -> Result<Value, Fail> {
        let o = self.uint(self.table + idx as usize * self.offset_size, self.offset_size)? as usize;
        let marker = self.byte(o)?;
        let (hi, low) = (marker >> 4, marker & 15);
        Ok(match hi {
            0 => match low { 8 => json!(false), 9 => json!(true), _ => Value::Null },
            1 => num(self.uint(o + 1, 1usize << low.min(3))? as f64),
            2 => {
                let bytes = (0..8).map(|k| self.byte(o + 1 + k).unwrap_or(0)).collect::<Vec<_>>();
                if low == 2 {
                    num(f64::from(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
                } else {
                    num(f64::from_be_bytes(bytes.try_into().unwrap()))
                }
            }
            3 => num(f64::from_be_bytes((0..8).map(|k| self.byte(o + 1 + k).unwrap_or(0)).collect::<Vec<_>>().try_into().unwrap())),
            4 => json!(format!("<{} bytes>", self.len(o, low)?.0)),
            5 => {
                let (len, s) = self.len(o, low)?;
                json!((0..len).map(|k| self.byte(s + k).map(char::from)).collect::<Result<String, _>>()?)
            }
            6 => {
                let (len, s) = self.len(o, low)?;
                let units = (0..len).map(|k| Ok(((self.byte(s + 2 * k)? as u16) << 8) | u16::from(self.byte(s + 2 * k + 1)?))).collect::<Result<Vec<u16>, Fail>>()?;
                json!(char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).collect::<String>())
            }
            8 => num(self.uint(o + 1, usize::from(low) + 1)? as f64),
            10 => {
                let (len, s) = self.len(o, low)?;
                let mut a = vec![];
                for k in 0..len {
                    let r = self.uint(s + k * self.ref_size, self.ref_size)?;
                    a.push(self.obj(r)?);
                }
                Value::Array(a)
            }
            13 => {
                let (len, s) = self.len(o, low)?;
                let mut map = Map::new();
                for k in 0..len {
                    let key = self.uint(s + k * self.ref_size, self.ref_size)?;
                    let val = self.uint(s + (len + k) * self.ref_size, self.ref_size)?;
                    let key = to_str(&self.obj(key)?);
                    let value = self.obj(val)?;
                    map.insert(key, value);
                }
                Value::Object(map)
            }
            _ => Value::Null,
        })
    }
}
fn parse_binary(d: &[u8]) -> Result<Value, Fail> {
    let n = d.len();
    if n < 40 {
        return Err(Fail::Key("terminal.iterm.short"));
    }
    let big = |o: usize| u64::from_be_bytes(d[o..o + 8].try_into().unwrap());
    let mut b = Binary {
        d,
        objects: big(n - 24),
        ref_size: usize::from(d[n - 25]).clamp(1, 8),
        offset_size: usize::from(d[n - 26]).clamp(1, 8),
        table: big(n - 8) as usize,
        seen: vec![],
    };
    let top = big(n - 16);
    b.obj(top)
}

// ---- colors ----

fn to_hex(r: f64, g: f64, b: f64) -> String {
    let channel = |v: f64| format!("{:02x}", js_round(v.clamp(0., 1.) * 255.) as u8);
    format!("#{}{}{}", channel(r), channel(g), channel(b))
}
/// Display P3 to sRGB: recent exports store their colors in P3.
fn p3_to_srgb(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let gam = |c: f64| if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1. / 2.4) - 0.055 };
    let (lr, lg, lb) = (lin(r), lin(g), lin(b));
    (
        gam((1.2249401 * lr - 0.2249404 * lg).clamp(0., 1.)),
        gam((-0.0420569 * lr + 1.0420571 * lg).clamp(0., 1.)),
        gam((-0.0196376 * lr - 0.0786361 * lg + 1.0982735 * lb).clamp(0., 1.)),
    )
}
fn color(c: &Value) -> Option<String> {
    if !c.is_object() {
        return None;
    }
    let (mut r, mut g, mut b) = (clamp01(&c["Red Component"]), clamp01(&c["Green Component"]), clamp01(&c["Blue Component"]));
    if c["Color Space"] == "P3" {
        (r, g, b) = p3_to_srgb(r, g, b);
    }
    Some(to_hex(r, g, b))
}
/// The colors of a profile; one with light and dark variants gives its dark one.
fn scheme_from(o: &Value, name: &str) -> Option<Value> {
    let dark = truthy(&o["Use Separate Colors for Light and Dark Mode"]);
    let get = |k: &str| -> Option<String> {
        let variant = if dark { &o[format!("{k} (Dark)")] } else { &Value::Null };
        color(if truthy(variant) { variant } else { &o[k] })
    };
    let mut ansi = vec![];
    for i in 0..16 {
        ansi.push(get(&format!("Ansi {i} Color"))?);
    }
    let fg = get("Foreground Color").unwrap_or_else(|| "#c7c7c7".into());
    let mut s = json!({"name":name,"fg":fg,"bg":get("Background Color").unwrap_or_else(|| "#000000".into()),"ansi":ansi});
    for (k, key) in [("cursor", "Cursor Color"), ("cursorText", "Cursor Text Color"), ("selection", "Selection Color"),
        ("selectionText", "Selected Text Color"), ("bold", "Bold Color"), ("link", "Link Color"), ("guide", "Cursor Guide Color")] {
        if let Some(v) = get(key) {
            s[k] = json!(v);
        }
    }
    if s.get("bold").is_some() && (o["Use Bold Color"] == false || (dark && o["Use Bold Color (Dark)"] == false)) {
        s.as_object_mut().unwrap().remove("bold");
    }
    if s.get("cursor").is_none() {
        s["cursor"] = s["fg"].clone();
    }
    if s.get("selection").is_none() {
        s["selection"] = json!("#b5d5ff");
    }
    Some(s)
}

// ---- keys ----

fn ns_key(code: u32) -> Option<String> {
    let named = match code {
        0xf700 => "ArrowUp",
        0xf701 => "ArrowDown",
        0xf702 => "ArrowLeft",
        0xf703 => "ArrowRight",
        0xf727 => "Insert",
        0xf728 => "Delete",
        0xf729 => "Home",
        0xf72b => "End",
        0xf72c => "PageUp",
        0xf72d => "PageDown",
        0x0d | 0x03 => "Enter",
        0x09 | 0x19 => "Tab",
        0x1b => "Escape",
        0x7f | 0x08 => "Backspace",
        0x20 => "Space",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.into());
    }
    (0xf704..0xf704 + 12).contains(&code).then(|| format!("F{}", code - 0xf704 + 1))
}
fn parse_hex(s: &str) -> Option<u64> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let digits: String = t.chars().take_while(char::is_ascii_hexdigit).collect();
    u64::from_str_radix(&digits, 16).ok()
}
/// `0xf702-0x280000` → `Alt+ArrowLeft`: a combination in the format of the key tables.
pub fn combo(spec: &str) -> Option<String> {
    let mut parts = spec.split('-');
    let code = parse_hex(parts.next()?)? as u32;
    let mods = parts.next().and_then(parse_hex).unwrap_or(0);
    let mut shift = mods & 0x20000 != 0;
    if code == 0x19 {
        shift = true;
    }
    let key = match ns_key(code) {
        Some(k) => k,
        None => {
            if !(0x21..=0x7e).contains(&code) {
                return None;
            }
            let mut ch = char::from_u32(code)?;
            const SHIFTED: [(char, char); 21] = [('{', '['), ('}', ']'), ('|', '\\'), (':', ';'), ('"', '\''), ('<', ','), ('>', '.'), ('?', '/'),
                ('~', '`'), ('!', '1'), ('@', '2'), ('#', '3'), ('$', '4'), ('%', '5'), ('^', '6'), ('&', '7'), ('*', '8'), ('(', '9'), (')', '0'),
                ('_', '-'), ('+', '=')];
            if let Some((_, base)) = SHIFTED.iter().find(|(s, _)| *s == ch) {
                ch = *base;
                shift = true;
            }
            if ch.is_ascii_uppercase() {
                ch = ch.to_ascii_lowercase();
                shift = true;
            }
            ch.to_string()
        }
    };
    let mut out = vec![];
    if mods & 0x100000 != 0 {
        out.push("Super".to_owned());
    }
    if mods & 0x40000 != 0 {
        out.push("Ctrl".to_owned());
    }
    if mods & 0x80000 != 0 {
        out.push("Alt".to_owned());
    }
    if shift {
        out.push("Shift".to_owned());
    }
    out.push(key);
    Some(out.join("+"))
}
fn iterm_action(code: i64) -> Option<&'static str> {
    Some(match code {
        0 => "nextTab",
        2 => "prevTab",
        4 => "scrollBottom",
        5 => "scrollTop",
        6 => "scrollLineDown",
        7 => "scrollLineUp",
        8 => "scrollPageDown",
        9 => "scrollPageUp",
        18 => "paneLeft",
        19 => "paneRight",
        20 => "paneUp",
        21 => "paneDown",
        23 => "fullscreen",
        31 => "moveTabLeft",
        32 => "moveTabRight",
        34 => "find",
        42 => "undoClose",
        _ => return None,
    })
}
fn key_map(map: &Value, warnings: &mut Vec<String>, place: &str, env: &Env) -> Vec<Value> {
    let (mut out, mut skipped) = (vec![], 0);
    for (spec, a) in map.as_object().into_iter().flatten() {
        let Some(combo) = combo(spec) else {
            skipped += 1;
            continue;
        };
        let act = to_num(&a["Action"]);
        let text = if a["Text"].is_null() { String::new() } else { to_str(&a["Text"]) };
        let entry = |action: &str, param: &str| json!({"combo":combo,"action":action,"param":param});
        if act == 10. {
            out.push(entry("escape", &text));
        } else if act == 11. {
            out.push(entry("hex", &text));
        } else if act == 12. || act == 36. {
            out.push(entry("text", &text));
        } else if act == 13. {
            out.push(entry("ignore", ""));
        } else if act == 16. {
            out.push(entry("hex", "0x08"));
        } else if act == 17. {
            out.push(entry("hex", "0x7f"));
        } else if let Some(action) = iterm_action(act as i64).filter(|_| act.fract() == 0.) {
            out.push(entry(action, ""));
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        warnings.push(format!("{place}: {}", (env.tr)("terminal.iterm.keysSkipped", json!({"n":skipped}))));
    }
    out
}

// ---- profile ----

fn trigger_action(name: &str) -> Option<&'static str> {
    Some(match name {
        "AlertTrigger" | "UserNotificationTrigger" => "notify",
        "BellTrigger" => "bell",
        "BounceTrigger" => "attention",
        "SendTextTrigger" => "send",
        "CaptureTrigger" => "capture",
        "HighlightTrigger" => "highlight",
        "MarkTrigger" => "mark",
        "SetTitleTrigger" => "title",
        "StopTrigger" => "stop",
        _ => return None,
    })
}
fn mac_path(p: &str) -> String {
    Regex::new(r"^/Users/[^/]+").unwrap().replace(p, "~").into_owned()
}
fn present(v: &Value) -> bool {
    !v.is_null()
}
/// One exported profile as a profile of this terminal.
pub fn convert_profile(o: &Value, env: &mut Env, warnings: &mut Vec<String>) -> Value {
    let name = if truthy(&o["Name"]) { to_str(&o["Name"]) } else { "Imported".to_owned() };
    let mut p = env.defaults.clone();
    p["id"] = json!((env.id)());
    p["name"] = json!(name);
    p["itermGuid"] = if truthy(&o["Guid"]) { o["Guid"].clone() } else { json!("") };
    let warn = |message: String, warnings: &mut Vec<String>| warnings.push(format!("{name}: {message}"));
    let tr = |key: &str, params: Value, env: &Env| (env.tr)(key, params);

    match scheme_from(o, &name) {
        Some(scheme) => {
            p["colors"] = json!("custom");
            p["custom"] = scheme;
        }
        None => warn(tr("terminal.iterm.noAnsi", json!({}), env), warnings),
    }
    // font: "JetBrainsMono-Regular 13"
    let font_re = Regex::new(r"^(.*)\s+([\d.]+)$").unwrap();
    let font = if truthy(&o["Normal Font"]) { to_str(&o["Normal Font"]) } else { String::new() };
    if let Some(c) = font_re.captures(&font) {
        p["fontSize"] = num(js_round(c[2].parse::<f64>().unwrap_or(f64::NAN)).clamp(7., 40.));
        match (env.font)(&c[1]) {
            Some(family) => p["font"] = json!(family),
            None if !c[1].to_ascii_lowercase().starts_with("jetbrainsmono") => {
                warn(tr("terminal.iterm.noFont", json!({"font":&c[1]}), env), warnings)
            }
            None => {}
        }
    }
    if truthy(&o["Use Non-ASCII Font"]) && truthy(&o["Non Ascii Font"]) {
        let text = to_str(&o["Non Ascii Font"]);
        let name_part = Regex::new(r"^(.*)\s+[\d.]+$").unwrap().captures(&text).map(|c| c[1].to_owned());
        match name_part.as_deref().and_then(|n| (env.font)(n)) {
            Some(family) => p["nonAsciiFont"] = json!(family),
            None => warn(tr("terminal.iterm.noNaFont", json!({"font":name_part.unwrap_or_default()}), env), warnings),
        }
    }
    let flag = |key: &str| truthy(&o[key]);
    if present(&o["ASCII Ligatures"]) {
        p["ligatures"] = json!(flag("ASCII Ligatures"));
    }
    if present(&o["Vertical Spacing"]) {
        p["lineSpacing"] = num((js_round(to_num(&o["Vertical Spacing"]) * 100.) / 100.).clamp(0.8, 2.));
    }
    if present(&o["Horizontal Spacing"]) && (to_num(&o["Horizontal Spacing"]) - 1.).abs() > 0.01 {
        warn(tr("terminal.iterm.hspacing", json!({}), env), warnings);
    }
    for (key, field) in [("Use Bold Font", "useBoldFont"), ("Use Bright Bold", "boldBright"), ("Use Italic Font", "useItalic"), ("Blink Allowed", "blinkText")] {
        if present(&o[key]) {
            p[field] = json!(flag(key));
        }
    }
    if present(&o["Minimum Contrast"]) {
        p["minimumContrast"] = num(js_round(clamp01(&o["Minimum Contrast"]) * 100.) / 100.);
    }
    // cursor
    match to_num(&o["Cursor Type"]) {
        t if t == 0. => p["cursorStyle"] = json!("underline"),
        t if t == 1. => p["cursorStyle"] = json!("beam"),
        t if t == 2. => p["cursorStyle"] = json!("block"),
        _ => {}
    }
    if present(&o["Blinking Cursor"]) {
        p["cursorBlink"] = json!(flag("Blinking Cursor"));
    }
    if present(&o["Use Cursor Guide"]) {
        p["cursorGuide"] = json!(flag("Use Cursor Guide"));
    }
    // window
    if present(&o["Transparency"]) {
        p["transparency"] = num(js_round(clamp01(&o["Transparency"]).min(0.9) * 100.) / 100.);
    }
    if present(&o["Blur"]) {
        p["blur"] = json!(flag("Blur"));
    }
    if to_num(&o["Columns"]) > 0. && to_num(&o["Rows"]) > 0. {
        p["columns"] = num(to_num(&o["Columns"]).min(400.));
        p["rows"] = num(to_num(&o["Rows"]).min(200.));
    }
    if present(&o["Show Status Bar"]) {
        p["statusBar"] = json!(flag("Show Status Bar"));
    }
    if truthy(&o["Background Image Location"]) {
        warn(tr("terminal.iterm.bgImage", json!({}), env), warnings);
    }
    // terminal
    p["scrollback"] = if flag("Unlimited Scrollback") {
        json!(1_000_000)
    } else {
        let n = to_num(&o["Scrollback Lines"]);
        if n > 0. { num(n.min(1_000_000.)) } else { env.defaults["scrollback"].clone() }
    };
    if present(&o["Mouse Reporting"]) {
        p["mouseReporting"] = json!(flag("Mouse Reporting"));
    }
    if present(&o["Option Key Sends"]) {
        p["altSendsEscape"] = json!(to_num(&o["Option Key Sends"]) != 0.);
    }
    if flag("Silence Bell") {
        p["bell"] = json!("none");
    }
    if present(&o["Prompt Before Closing 2"]) {
        p["confirmClose"] = json!(to_num(&o["Prompt Before Closing 2"]) != 0.);
    }
    if truthy(&o["Character Encoding"]) && to_num(&o["Character Encoding"]) != 4. {
        warn(tr("terminal.iterm.encoding", json!({}), env), warnings);
    }
    if flag("Ambiguous Double Width") {
        warn(tr("terminal.iterm.ambiguous", json!({}), env), warnings);
    }
    // general
    match o["Custom Directory"].as_str() {
        Some("Yes") if truthy(&o["Working Directory"]) => {
            p["workingDir"] = json!("custom");
            p["customDir"] = json!(mac_path(&to_str(&o["Working Directory"])));
        }
        Some("Recycle" | "Advanced") => p["workingDir"] = json!("reuse"),
        Some("No") => p["workingDir"] = json!("home"),
        _ => {}
    }
    if truthy(&o["Badge Text"]) {
        let badge = to_str(&o["Badge Text"]);
        if badge.contains("\\(") {
            warn(tr("terminal.iterm.badgeVars", json!({}), env), warnings);
        }
        p["badge"] = json!(badge);
    }
    if flag("Use Tab Color") && truthy(&o["Tab Color"]) {
        p["tabColor"] = json!(color(&o["Tab Color"]).unwrap_or_default());
    }
    if truthy(&o["Initial Text"]) {
        p["initialText"] = json!(to_str(&o["Initial Text"]));
    }
    if o["Custom Command"] == "Yes" && truthy(&o["Command"]) {
        // the terminal only opens the user's shell: the command is typed into it when the session opens
        let command = to_str(&o["Command"]);
        let initial = p["initialText"].as_str().unwrap_or("").to_owned();
        p["initialText"] = json!(format!("{command}{}", if initial.is_empty() { String::new() } else { format!("\n{initial}") }));
        warn(tr("terminal.iterm.command", json!({"cmd":command}), env), warnings);
    }
    // automatic profile switching: "user@host", "host", "/path", "&job"
    let bound: Vec<String> = o["Bound Hosts"].as_array().map(|a| a.iter().map(to_str).collect()).unwrap_or_default();
    let rules: Vec<String> = bound.iter().map(|r| r.trim().to_owned()).filter(|r| !r.is_empty() && !r.starts_with('&')).map(|r| mac_path(&r)).collect();
    if bound.iter().any(|r| r.starts_with('&')) {
        warn(tr("terminal.iterm.jobRules", json!({}), env), warnings);
    }
    if !rules.is_empty() {
        p["rules"] = json!(rules.join("\n"));
    }
    // triggers
    let mut skipped = 0;
    let mut triggers = vec![];
    for t in o["Triggers"].as_array().into_iter().flatten() {
        let Some(action) = t["action"].as_str().and_then(trigger_action).filter(|_| truthy(&t["regex"])) else {
            skipped += 1;
            continue;
        };
        let mut param = if t["parameter"].is_null() { String::new() } else { to_str(&t["parameter"]) };
        if t["parameter"].is_object() {
            param.clear();
        }
        triggers.push(json!({"regex":to_str(&t["regex"]),"action":action,"param":param,"enabled":t["disabled"] != true}));
    }
    p["triggers"] = json!(triggers);
    if skipped > 0 {
        warn(tr("terminal.iterm.triggersSkipped", json!({"n":skipped}), env), warnings);
    }
    // the profile's keys
    let keys = key_map(&o["Keyboard Map"], warnings, &name, env);
    p["keyMap"] = json!(keys);
    if truthy(&o["Dynamic Profile Parent Name"]) {
        warn(tr("terminal.iterm.parent", json!({"name":to_str(&o["Dynamic Profile Parent Name"])}), env), warnings);
    }
    p
}

// ---- entry ----

fn is_colors_dict(o: &Value) -> bool {
    o.is_object() && o.get("Ansi 0 Color").is_some() && !(o.get("Name").is_some() && o.get("Guid").is_some())
}
/// Reads an export: profile JSON (or Dynamic Profiles), an XML plist or a binary plist.
pub fn import(input: Input, file_name: &str, env: &mut Env) -> Result<Imported, String> {
    let fail = |f: Fail, env: &Env| match f {
        Fail::Key(key) => (env.tr)(key, json!({})),
        Fail::Raw(message) => message,
    };
    let doc = match input {
        Input::Bytes(bytes) => {
            if !bytes.starts_with(b"bplist00") {
                return Err((env.tr)("terminal.iterm.unknownBinary", json!({})));
            }
            parse_binary(bytes).map_err(|f| fail(f, env))?
        }
        Input::Text(text) => {
            let text = text.trim();
            if text.starts_with('{') || text.starts_with('[') {
                serde_json::from_str(text).map_err(|e| fail(Fail::Raw(e.to_string()), env))?
            } else if text.starts_with('<') {
                Xml::new(text).value(0)
            } else {
                return Err((env.tr)("terminal.iterm.notIterm", json!({})));
            }
        }
    };
    let mut out = Imported::default();
    let base = file_name.rsplit('/').next().unwrap_or("Imported");
    let base = if base.is_empty() { "Imported" } else { base };
    let base = Regex::new(r"(?i)\.(itermcolors|json|plist)$").unwrap().replace(base, "").into_owned();
    if is_colors_dict(&doc) {
        let scheme = scheme_from(&doc, &base).ok_or_else(|| (env.tr)("terminal.iterm.no16", json!({})))?;
        out.schemes.push(scheme);
        return Ok(out);
    }
    let list: Vec<Value> = if let Some(a) = doc.as_array() {
        a.clone()
    } else if let Some(a) = doc["Profiles"].as_array() {
        a.clone()
    } else if let Some(a) = doc["New Bookmarks"].as_array() {
        out.default_guid = doc["Default Bookmark Guid"].as_str().filter(|g| !g.is_empty()).map(str::to_owned);
        if truthy(&doc["GlobalKeyMap"]) {
            let mut warnings = std::mem::take(&mut out.warnings);
            out.key_map = key_map(&doc["GlobalKeyMap"], &mut warnings, "Global shortcuts", env);
            out.warnings = warnings;
        }
        for (name, colors) in doc["Custom Color Presets"].as_object().into_iter().flatten() {
            if let Some(s) = scheme_from(colors, name) {
                out.schemes.push(s);
            }
        }
        a.clone()
    } else if doc.is_object() && (doc.get("Name").is_some() || doc.get("Guid").is_some()) {
        vec![doc.clone()]
    } else {
        vec![]
    };
    if list.is_empty() && out.schemes.is_empty() {
        return Err((env.tr)("terminal.iterm.noProfiles", json!({})));
    }
    let mut warnings = std::mem::take(&mut out.warnings);
    for o in list.iter().filter(|o| o.is_object()) {
        out.profiles.push(convert_profile(o, env, &mut warnings));
    }
    out.warnings = warnings;
    Ok(out)
}

#[cfg(test)]
#[path = "iterm_tests.rs"]
mod tests;
