//! Draws the safe tree that axia-maild's sanitizer made from a message's HTML
//! (`server/src/mail/html.h`) as native widgets: blocks, layout tables, text runs, images and
//! links. Nothing here interprets HTML or runs anything; images are decoded by axia-maild, and
//! remote ones are asked for only after the user chose to load them.
//!
//! The trees are plain functions of the document, the width and the pictures already received, so
//! the window can keep them until one of those changes.
use serde_json::{json, Value};
use std::collections::HashMap;

/// Beyond this many words a paragraph keeps whole runs (wrapping gets coarser).
const MAX_WORDS: usize = 6000;
const MAX_LINES: usize = 5000;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Picture {
    Loading,
    /// A decoded image of the helper and its size in pixels.
    Ready { handle: i64, width: f64, height: f64 },
    Failed,
}

/// An image the document wants: its source in the document and the widest it can be drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    pub source: String,
    pub max_width: u32,
}

pub struct Colors<'a> {
    pub text: &'a str,
    pub dim: &'a str,
    pub accent: &'a str,
    pub dark: bool,
}

#[derive(Clone)]
struct Context<'a> {
    width: f64,
    font: f64,
    text: String,
    dim: String,
    link: String,
    italic: &'static str,
    links: &'a [Value],
    /// Names the message in the events of its links.
    tag: &'a str,
    remote: bool,
    pictures: &'a HashMap<String, Picture>,
}

fn number(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}
fn positive(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && *n > 0.)
}
fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
/// `#rrggbb` stays; `#rrggbbaa` becomes `rgba()`. Anything else gives the fallback.
fn color(v: &Value, fallback: &str) -> String {
    let hex = str_of(v);
    if !hex.starts_with('#') || !hex[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return fallback.to_owned();
    }
    if hex.len() == 9 {
        let part = |a: usize| u32::from_str_radix(&hex[a..a + 2], 16).unwrap_or(0);
        return format!("rgba({},{},{},{:.2})", part(1), part(3), part(5), f64::from(part(7)) / 255.);
    }
    hex.to_owned()
}

fn view(key: String, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(key: String, label: &str, style: Value) -> Value {
    json!({"kind":"text","key":key,"label":label,"style":style})
}
fn merge(mut style: Value, more: Value) -> Value {
    if let (Some(into), Some(from)) = (style.as_object_mut(), more.as_object()) {
        into.extend(from.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    style
}

impl Context<'_> {
    fn link(&self, node: &Value) -> Option<(usize, &Value)> {
        let i = usize::try_from(node["l"].as_i64()?).ok()?;
        Some((i, self.links.get(i)?))
    }
    fn within(&self, width: f64) -> Self {
        Context { width, ..self.clone() }
    }
}

fn image(node: &Value, key: String, ctx: &Context, wanted: &mut Vec<Wanted>) -> Value {
    let source = str_of(&node["src"]);
    let allowed = !source.starts_with("r:") || ctx.remote;
    let given_width = positive(&node["w"]);
    let given_height = positive(&node["h"]);
    let max_width = given_width.map_or(ctx.width, |w| w.min(ctx.width)).round().max(16.);
    let picture = ctx.pictures.get(source);
    if allowed && picture.is_none() {
        wanted.push(Wanted { source: source.to_owned(), max_width: (max_width * 2.).min(1600.) as u32 });
    }
    let natural = match picture {
        Some(Picture::Ready { width, height, .. }) if *width > 0. && *height > 0. => Some((*width, *height)),
        _ => None,
    };
    // the HTML's width and height, or the picture's own, scaled to fit
    let mut width = match positive(&node["wp"]) {
        Some(percent) => ctx.width * percent / 100.,
        None => given_width.or(natural.map(|(w, _)| w / 2.)).unwrap_or(ctx.width.min(200.)),
    };
    let mut height = given_height
        .or(natural.map(|(w, h)| h * width / w))
        .unwrap_or_else(|| given_width.map_or(60., |w| w * 0.5));
    if let (None, Some(w), Some((nw, nh))) = (given_height, given_width, natural) {
        height = nh * w / nw;
    }
    if width > ctx.width {
        height = height * ctx.width / width;
        width = ctx.width;
    }
    let size = json!({"width":width.max(1.),"height":height.max(1.),"flexShrink":0});
    let mut out = match picture {
        Some(Picture::Ready { handle, .. }) if allowed => {
            json!({"kind":"mailImage","key":key,"image":handle,"style":merge(size, json!({"objectFit":"contain"}))})
        }
        _ => {
            let mut inside = Vec::new();
            if height > 18. {
                let mark = if allowed && picture != Some(&Picture::Failed) { "…" } else { "▨" };
                inside.push(text(format!("{key}:mark"), mark, json!({"fontSize":13,"color":ctx.dim})));
            }
            if height > 36. && !str_of(&node["alt"]).is_empty() {
                inside.push(text(format!("{key}:alt"), str_of(&node["alt"]), json!({"fontSize":11,"color":ctx.dim,"textAlign":"center","paddingHorizontal":4})));
            }
            view(
                key,
                merge(size, json!({"borderRadius":4,"borderWidth":1,"borderColor":ctx.dim,"backgroundColor":"rgba(127,127,127,0.10)",
                    "alignItems":"center","justifyContent":"center","overflow":"hidden"})),
                inside,
            )
        }
    };
    if let Some((i, _)) = ctx.link(node) {
        // a picture is clicked through its box: image nodes take no events
        let key = format!("{}:link", str_of(&out["key"]));
        out = view(key, json!({"flexShrink":0}), vec![out]);
        out["events"] = json!({"onClick":["link", [ctx.tag, i]]});
    }
    out
}

fn run_style(style: &Value, ctx: &Context, link: Option<&Value>) -> Value {
    let size = positive(&style["fs"]).unwrap_or(ctx.font);
    let colored = !str_of(&style["c"]).is_empty();
    let foreground = match link {
        Some(l) if truthy(&l["w"]) => "#e5383b".to_owned(),
        Some(_) => ctx.link.clone(),
        // the engine has no italic face: a tint, like Notes
        None if truthy(&style["i"]) && !colored => ctx.italic.to_owned(),
        None => color(&style["c"], &ctx.text),
    };
    let mut out = json!({"fontSize":size,"color":foreground});
    if truthy(&style["b"]) {
        out["fontWeight"] = json!(700);
    }
    if truthy(&style["mono"]) {
        out["fontFamily"] = json!("monospace");
    }
    if !str_of(&style["bg"]).is_empty() {
        out["backgroundColor"] = json!(color(&style["bg"], "transparent"));
    }
    out
}
/// The sanitizer writes flags as booleans or numbers.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.),
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

fn word(key: String, piece: &str, style: &Value, ctx: &Context, link: Option<(usize, &Value)>) -> Value {
    let looks = run_style(style, ctx, link.map(|(_, l)| l));
    let underline = truthy(&style["u"]) || link.is_some();
    let strike = truthy(&style["st"]);
    let mut out = if !underline && !strike {
        text(key, piece, looks)
    } else {
        let color = looks["color"].clone();
        let size = number(&looks["fontSize"]);
        let mut parts = vec![text(format!("{key}:text"), piece, looks)];
        if underline {
            parts.push(view(format!("{key}:underline"), json!({"height":1,"marginTop":-2,"backgroundColor":color,"opacity":if link.is_some() {0.6} else {1.}}), vec![]));
        }
        if strike {
            parts.push(view(format!("{key}:strike"), json!({"position":"absolute","left":0,"right":0,"top":size * 1.3 / 2.,"height":1,"backgroundColor":color}), vec![]));
        }
        view(key, json!({"flexDirection":"column"}), parts)
    };
    if let Some((i, _)) = link {
        if out["kind"] == "text" {
            let key = format!("{}:link", str_of(&out["key"]));
            out = view(key, json!({}), vec![out]);
        }
        out["events"] = json!({"onClick":["link", [ctx.tag, i]]});
    }
    out
}

/// Inline runs in a wrapping row, one text per word (a text has a single style); a lone plain run
/// stays one text that wraps by itself.
fn paragraph(key: String, items: &[&Value], ctx: &Context, align: &str, wanted: &mut Vec<Wanted>) -> Value {
    let (justify, text_align) = match align {
        "c" => ("center", "center"),
        "e" => ("flex-end", "end"),
        _ => ("flex-start", "start"),
    };
    if let [only] = items {
        let style = &only["s"];
        if only["t"] == "x" && ctx.link(only).is_none() && !truthy(&style["u"]) && !truthy(&style["st"]) {
            let looks = run_style(style, ctx, None);
            let line = number(&looks["fontSize"]) * 1.4;
            return text(key, str_of(&only["x"]), merge(looks, json!({"textAlign":text_align,"lineHeight":line})));
        }
    }
    let mut out = Vec::new();
    let mut words = 0;
    for (i, node) in items.iter().enumerate() {
        match str_of(&node["t"]) {
            "br" => out.push(view(format!("{key}:b{i}"), json!({"width":"100%","height":0}), vec![])),
            "img" => out.push(image(node, format!("{key}:i{i}"), ctx, wanted)),
            _ => {
                let link = ctx.link(node);
                if link.is_some_and(|(_, l)| truthy(&l["w"])) && (i == 0 || items[i - 1]["l"] != node["l"]) {
                    let size = positive(&node["s"]["fs"]).unwrap_or(ctx.font);
                    out.push(text(format!("{key}:w{i}"), "⚠ ", json!({"fontSize":size,"color":"#e5383b"})));
                }
                let source = str_of(&node["x"]);
                let pieces: Vec<&str> = if words > MAX_WORDS { vec![source] } else { source.split_inclusive(' ').collect() };
                words += pieces.len();
                for (k, piece) in pieces.into_iter().enumerate() {
                    out.push(word(format!("{key}:{i}.{k}"), piece, &node["s"], ctx, link));
                }
            }
        }
    }
    view(key, json!({"flexDirection":"row","flexWrap":"wrap","alignItems":"flex-end","justifyContent":justify}), out)
}

fn padding(style: &Value) -> Value {
    match style["p"].as_array() {
        Some(p) if p.len() == 4 => json!({"paddingTop":p[0],"paddingRight":p[1],"paddingBottom":p[2],"paddingLeft":p[3]}),
        _ => json!({}),
    }
}
fn padding_across(style: &Value) -> f64 {
    number(&style["p"][1]) + number(&style["p"][3])
}
fn block_width(style: &Value, available: f64) -> Option<f64> {
    let mut width = positive(&style["wp"]).map(|p| available * p / 100.).or_else(|| positive(&style["w"]));
    if let Some(most) = positive(&style["mw"]) {
        if width.is_none_or(|w| w > most) {
            width = Some(most);
        }
    }
    width.map(|w| w.min(available))
}

fn children(prefix: &str, nodes: &[Value], ctx: &Context, align: &str, depth: usize, wanted: &mut Vec<Wanted>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut inline: Vec<&Value> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let kind = str_of(&node["t"]);
        if kind == "x" || kind == "br" || (kind == "img" && !truthy(&node["a"])) {
            inline.push(node);
            continue;
        }
        if !inline.is_empty() {
            out.push(paragraph(format!("{prefix}p{i}"), &std::mem::take(&mut inline), ctx, align, wanted));
        }
        if depth < MAX_DEPTH {
            out.extend(block(&format!("{prefix}{i}"), node, ctx, align, depth + 1, wanted));
        }
    }
    if !inline.is_empty() {
        out.push(paragraph(format!("{prefix}pend"), &inline, ctx, align, wanted));
    }
    out
}

fn table(key: &str, node: &Value, ctx: &Context, depth: usize, wanted: &mut Vec<Wanted>) -> Value {
    let style = &node["s"];
    let width = block_width(style, ctx.width).unwrap_or(ctx.width);
    let cell_padding = number(&node["cp"]);
    let border = number(&style["bw"]);
    let border_color = color(&style["bc"], &ctx.dim);
    let rows: Vec<Value> = list(&node["c"])
        .iter()
        .filter(|r| r["t"] == "tr" && !list(&r["c"]).is_empty())
        .enumerate()
        .map(|(ri, row)| {
            let cells: Vec<&Value> = list(&row["c"]).iter().filter(|c| c["t"] == "td").collect();
            // fixed widths first, the rest share what is left
            let widths: Vec<Option<f64>> = cells.iter().map(|c| block_width(&c["s"], width)).collect();
            let fixed: f64 = widths.iter().flatten().sum();
            let shares: f64 = cells.iter().zip(&widths).filter(|(_, w)| w.is_none()).map(|(c, _)| positive(&c["cs"]).unwrap_or(1.)).sum();
            let scale = if fixed > width { width / fixed } else { 1. };
            let rest = (width - fixed * scale).max(0.);
            let drawn: Vec<Value> = cells
                .iter()
                .zip(&widths)
                .enumerate()
                .map(|(ci, (cell, own))| {
                    let looks = &cell["s"];
                    let cell_width = match own {
                        Some(w) => w * scale,
                        None if shares > 0. => rest * positive(&cell["cs"]).unwrap_or(1.) / shares,
                        None => 0.,
                    };
                    let padded = looks["p"].is_array();
                    let inner = (cell_width - if padded { padding_across(looks) } else { cell_padding * 2. }).max(10.);
                    let justify = match str_of(&looks["va"]) {
                        "m" => "center",
                        "b" => "flex-end",
                        _ => "flex-start",
                    };
                    let mut out = json!({"width":cell_width,"backgroundColor":color(&looks["bg"], "transparent"),"justifyContent":justify});
                    out = merge(out, if padded {
                        padding(looks)
                    } else {
                        json!({"paddingTop":cell_padding,"paddingBottom":cell_padding,"paddingLeft":cell_padding,"paddingRight":cell_padding})
                    });
                    if border > 0. {
                        out = merge(out, json!({"borderWidth":0.5,"borderColor":border_color}));
                    }
                    let prefix = format!("{key}.{ri}.{ci}.");
                    let kids = children(&prefix, list(&cell["c"]), &ctx.within(inner), str_of(&looks["a"]), depth, wanted);
                    view(format!("{prefix}cell"), out, kids)
                })
                .collect();
            view(format!("{key}.{ri}"), json!({"flexDirection":"row","backgroundColor":color(&row["s"]["bg"], "transparent")}), drawn)
        })
        .collect();
    let radius = number(&style["r"]);
    let mut out = json!({"width":width,"alignSelf":if truthy(&style["ac"]) {"center"} else {"flex-start"},"backgroundColor":color(&style["bg"], "transparent"),
        "borderRadius":radius,"overflow":if radius > 0. {"hidden"} else {"visible"},"marginTop":number(&style["mt"]),"marginBottom":number(&style["mb"])});
    if border > 0. {
        out = merge(out, json!({"borderWidth":border,"borderColor":border_color}));
    }
    view(key.to_owned(), out, rows)
}

fn block(key: &str, node: &Value, ctx: &Context, align: &str, depth: usize, wanted: &mut Vec<Wanted>) -> Vec<Value> {
    match str_of(&node["t"]) {
        "hr" => vec![view(key.to_owned(), json!({"height":1,"backgroundColor":ctx.dim,"opacity":0.4,"marginTop":8,"marginBottom":8}), vec![])],
        "img" => {
            let place = match str_of(&node["a"]) {
                "c" => "center",
                "e" => "flex-end",
                _ => "flex-start",
            };
            vec![view(key.to_owned(), json!({"alignItems":place}), vec![image(node, format!("{key}:image"), ctx, wanted)])]
        }
        "tbl" => vec![table(key, node, ctx, depth, wanted)],
        "tr" | "td" => children(&format!("{key}."), list(&node["c"]), ctx, align, depth, wanted),
        _ => {
            let style = &node["s"];
            let width = block_width(style, ctx.width);
            let bar = number(&style["bl"]);
            let marker = str_of(&node["mk"]);
            let inner = (width.unwrap_or(ctx.width) - padding_across(style) - bar - if marker.is_empty() { 0. } else { 18. }).max(10.);
            let own_align = match str_of(&style["a"]) {
                "" => align,
                a => a,
            };
            let body = children(&format!("{key}."), list(&node["c"]), &ctx.within(inner), own_align, depth, wanted);
            let mut looks = merge(padding(style), json!({"marginTop":number(&style["mt"]),"marginBottom":number(&style["mb"]),
                "backgroundColor":color(&style["bg"], "transparent"),"borderRadius":number(&style["r"])}));
            if let Some(w) = width {
                looks["width"] = json!(w);
            }
            if truthy(&style["ac"]) {
                looks["alignSelf"] = json!("center");
            }
            if positive(&style["bw"]).is_some() {
                looks = merge(looks, json!({"borderWidth":style["bw"],"borderColor":color(&style["bc"], &ctx.dim)}));
            }
            let column = |kids| view(format!("{key}:body"), json!({"flex":1}), kids);
            if bar > 0. {
                let quote = view(format!("{key}:bar"), json!({"width":bar,"backgroundColor":color(&style["blc"], &ctx.dim),"marginRight":10,"borderRadius":1}), vec![]);
                return vec![view(key.to_owned(), merge(looks, json!({"flexDirection":"row"})), vec![quote, column(body)])];
            }
            if !marker.is_empty() {
                let mark = text(format!("{key}:marker"), marker, json!({"fontSize":ctx.font,"color":ctx.text,"width":14,"textAlign":"end"}));
                return vec![view(key.to_owned(), merge(looks, json!({"flexDirection":"row","gap":6})), vec![mark, column(body)])];
            }
            vec![view(key.to_owned(), looks, body)]
        }
    }
}

/// The reading pane's drawing of a sanitized message and the pictures it still needs.
pub fn document(
    key: &str,
    doc: &Value,
    width: f64,
    remote: bool,
    pictures: &HashMap<String, Picture>,
    theme: &Colors,
) -> (Value, Vec<Wanted>) {
    // a sender who chose colors is shown on their own paper, also in dark mode
    let paper = truthy(&doc["colors"]) || truthy(&doc["bg"]) || !theme.dark;
    let ctx = Context {
        width: (width - 28.).max(80.),
        font: 14.,
        text: if paper { "#1f1f1f" } else { theme.text }.to_owned(),
        dim: if paper { "#8a8a8a" } else { theme.dim }.to_owned(),
        link: if paper { "#0a5fd0" } else { theme.accent }.to_owned(),
        italic: if paper { "#4b3b8f" } else { "#e9e3ff" },
        links: list(&doc["links"]),
        tag: key,
        remote,
        pictures,
    };
    let mut wanted = Vec::new();
    let kids = children(&format!("{key}:"), list(&doc["blocks"]), &ctx, "", 0, &mut wanted);
    wanted.dedup_by(|a, b| a.source == b.source);
    let background = if paper { color(&doc["bg"], "#ffffff") } else { "transparent".to_owned() };
    (view(key.to_owned(), json!({"backgroundColor":background,"padding":14,"borderRadius":10,"direction":"ltr"}), kids), wanted)
}

/// Splits a line into plain parts and links: (text, is a link).
pub fn split_links(line: &str) -> Vec<(&str, bool)> {
    static URL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"https?://[^\s<>()]+[^\s<>().,;:!?]|mailto:[^\s<>()]+").expect("a built-in pattern")
    });
    let mut out = Vec::new();
    let mut at = 0;
    for found in URL.find_iter(line) {
        if found.start() > at {
            out.push((&line[at..found.start()], false));
        }
        out.push((found.as_str(), true));
        at = found.end();
    }
    if at < line.len() {
        out.push((&line[at..], false));
    }
    out
}

/// Plain text with clickable links and dimmed quoted lines.
pub fn plain(key: &str, body: &str, width: f64, theme: &Colors) -> Value {
    let lines: Vec<Value> = body
        .replace("\r\n", "\n")
        .split('\n')
        .take(MAX_LINES)
        .enumerate()
        .map(|(i, line)| {
            let color = if line.trim_start().starts_with('>') { theme.dim } else { theme.text };
            let parts = split_links(line);
            if !parts.iter().any(|(_, link)| *link) {
                return text(format!("{key}:{i}"), if line.is_empty() { " " } else { line }, json!({"fontSize":14,"color":color,"minHeight":18}));
            }
            let mut words = Vec::new();
            for (k, (part, link)) in parts.into_iter().enumerate() {
                if link {
                    let mut node = view(format!("{key}:{i}.{k}"), json!({}), vec![text(format!("{key}:{i}.{k}:text"), part, json!({"fontSize":14,"color":theme.accent}))]);
                    node["events"] = json!({"onClick":["url", part]});
                    words.push(node);
                } else {
                    words.extend(part.split_inclusive(' ').enumerate().map(|(j, w)| text(format!("{key}:{i}.{k}.{j}"), w, json!({"fontSize":14,"color":color}))));
                }
            }
            view(format!("{key}:{i}"), json!({"flexDirection":"row","flexWrap":"wrap"}), words)
        })
        .collect();
    view(key.to_owned(), json!({"padding":4,"gap":2,"maxWidth":width}), lines)
}
