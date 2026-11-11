//! Read-only Markdown, using the same block and word layout as sdk/markdown.jsx.
//! No HTML evaluation. Links become application actions, never executable markup.
use super::ui::{click, text, view};
use regex::Regex;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::OnceLock};
#[derive(Clone, Debug)]
pub enum Block {
    Heading(usize, String),
    Paragraph(String),
    Rule,
    Code(String),
    Quote(Vec<Block>),
    List(Vec<Item>),
    Table {
        header: Vec<String>,
        align: Vec<&'static str>,
        rows: Vec<Vec<String>>,
    },
}
#[derive(Clone, Debug)]
pub struct Item {
    depth: usize,
    num: Option<usize>,
    task: Option<bool>,
    text: String,
}
struct Patterns {
    fence: Regex,
    heading: Regex,
    list: Regex,
    table: Regex,
    inline: Vec<(&'static str, Regex)>,
}
fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        fence: Regex::new(r"^(`{3,}|~{3,})\s*[\w+-]*").unwrap(),
        heading: Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap(),
        list: Regex::new(r"^(\s*)([-*+]|(\d+)[.)])\s+(\[([ xX])\]\s+)?(.*)$").unwrap(),
        table: Regex::new(r"^\s*\|?\s*:?-{2,}").unwrap(),
        inline: vec![
            ("code", r"^`([^`]+)`"),
            ("image", r#"^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#),
            ("link", r#"^\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#),
            ("bold", r"^\*\*(.+?)\*\*"),
            ("bold", r"^__(.+?)__"),
            ("strike", r"^~~(.+?)~~"),
            ("italic", r"^\*([^*\s][^*]*?)\*"),
            ("underscore", r"^_([^_\s][^_]*?)_"),
            ("url", r"^https?://[^\s<>()]+[^\s<>().,;:!?]"),
        ]
        .into_iter()
        .map(|(k, s)| (k, Regex::new(s).unwrap()))
        .collect(),
    })
}
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|s| s.trim().into()).collect()
}
fn table_row(line: &str) -> bool {
    let s = line.trim();
    s.starts_with('|') && s.ends_with('|')
}
fn rule(line: &str) -> bool {
    let s: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3
        && s.chars()
            .next()
            .is_some_and(|first| "-*_".contains(first) && s.chars().all(|c| c == first))
}
pub fn parse(source: &str) -> Vec<Block> {
    parse_at(&source.replace("\r\n", "\n"), 0)
}
fn parse_at(source: &str, depth: usize) -> Vec<Block> {
    if depth >= 32 {
        return vec![Block::Paragraph(source.into())];
    }
    let p = patterns();
    let lines: Vec<_> = source.split('\n').collect();
    let mut i = 0;
    let mut out = vec![];
    while i < lines.len() {
        let s = lines[i];
        if s.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(m) = p.fence.captures(s) {
            let fence = m[1].to_owned();
            let mut body = vec![];
            i += 1;
            while i < lines.len() && !lines[i].starts_with(&fence) {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            out.push(Block::Code(body.join("\n")));
            continue;
        }
        if let Some(m) = p.heading.captures(s) {
            out.push(Block::Heading(m[1].len(), m[2].into()));
            i += 1;
            continue;
        }
        if rule(s) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if s.trim_start().starts_with('>') {
            let mut body = vec![];
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let s = lines[i].trim_start().strip_prefix('>').unwrap();
                body.push(s.strip_prefix(' ').unwrap_or(s));
                i += 1;
            }
            out.push(Block::Quote(parse_at(&body.join("\n"), depth + 1)));
            continue;
        }
        if table_row(s) && i + 1 < lines.len() && p.table.is_match(lines[i + 1]) {
            let header = cells(s);
            let align = cells(lines[i + 1])
                .iter()
                .map(|c| {
                    if c.starts_with(':') && c.ends_with(':') {
                        "center"
                    } else if c.ends_with(':') {
                        "flex-end"
                    } else {
                        "flex-start"
                    }
                })
                .collect();
            i += 2;
            let mut rows = vec![];
            while i < lines.len() && table_row(lines[i]) {
                rows.push(cells(lines[i]));
                i += 1;
            }
            out.push(Block::Table {
                header,
                align,
                rows,
            });
            continue;
        }
        if p.list.is_match(s) {
            let mut items: Vec<Item> = vec![];
            while i < lines.len() {
                if let Some(m) = p.list.captures(lines[i]) {
                    items.push(Item {
                        depth: m[1].replace('\t', "  ").chars().count() / 2,
                        num: m.get(3).and_then(|n| n.as_str().parse().ok()),
                        task: m.get(4).map(|_| &m[5] != " "),
                        text: m[6].into(),
                    });
                    i += 1;
                } else if !lines[i].trim().is_empty()
                    && lines[i].chars().take_while(|c| c.is_whitespace()).count() >= 2
                {
                    if let Some(item) = items.last_mut() {
                        item.text.push(' ');
                        item.text.push_str(lines[i].trim());
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Block::List(items));
            continue;
        }
        let mut body = vec![s.trim()];
        i += 1;
        while i < lines.len()
            && !lines[i].trim().is_empty()
            && !p.heading.is_match(lines[i])
            && !lines[i].starts_with('>')
            && !p.fence.is_match(lines[i])
            && !p.list.is_match(lines[i])
        {
            body.push(lines[i].trim());
            i += 1;
        }
        out.push(Block::Paragraph(body.join(" ")));
    }
    out
}
#[derive(Clone, Debug, Default)]
struct Span {
    text: String,
    link: Option<String>,
    image: Option<String>,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}
fn inline(source: &str, style: &Span, depth: usize) -> Vec<Span> {
    if depth >= 32 {
        return vec![Span {
            text: source.into(),
            ..style.clone()
        }];
    }
    let mut out = vec![];
    let mut buf = String::new();
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        let c = rest.chars().next().unwrap();
        let hit = if "`![*_~h".contains(c) {
            patterns().inline.iter().find_map(|(kind, re)| {
                let m = re.captures(rest)?;
                if *kind == "underscore"
                    && (source[..i]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric())
                        || rest[m[0].len()..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphanumeric()))
                {
                    return None;
                }
                Some((*kind, m))
            })
        } else {
            None
        };
        if let Some((kind, m)) = hit {
            if !buf.is_empty() {
                out.push(Span {
                    text: std::mem::take(&mut buf),
                    ..style.clone()
                });
            }
            let mut next = style.clone();
            match kind {
                "code" => {
                    next.text = m[1].into();
                    next.code = true;
                    out.push(next);
                }
                "image" => out.push(Span {
                    text: m[1].into(),
                    image: Some(m[2].into()),
                    ..Span::default()
                }),
                "url" => {
                    next.text = m[0].into();
                    next.link = Some(m[0].into());
                    out.push(next);
                }
                "link" => {
                    next.link = Some(m[2].into());
                    out.extend(inline(&m[1], &next, depth + 1));
                }
                kind => {
                    match kind {
                        "bold" => next.bold = true,
                        "strike" => next.strike = true,
                        _ => next.italic = true,
                    };
                    out.extend(inline(&m[1], &next, depth + 1));
                }
            }
            i += m[0].len();
            continue;
        }
        i += c.len_utf8();
        if c == '\\' && i < source.len() {
            let c = source[i..].chars().next().unwrap();
            buf.push(c);
            i += c.len_utf8();
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push(Span {
            text: buf,
            ..style.clone()
        });
    }
    out
}
fn inl(source: &str, size: f64, weight: i32, width: f64, t: &Value) -> Value {
    let mut children = vec![];
    let dark = t["mode"] == "dark";
    for span in inline(source, &Span::default(), 0) {
        if let Some(path) = span.image {
            if path
                .split_once(':')
                .is_some_and(|(s, _)| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic()))
            {
                children.push(text(
                    json!(format!(
                        "[{}]",
                        if span.text.is_empty() {
                            &path
                        } else {
                            &span.text
                        }
                    )),
                    json!({"fontSize":size,"color":t["textDim"]}),
                ));
                continue;
            }
            let path = format!("/{}", path.trim_start_matches('/'));
            let max = (width - 8.).max(40.);
            let (w, h) = image::image_dimensions(&path)
                .ok()
                .map(|(w, h)| {
                    let width = max.min(w as f64);
                    (width, (width * h as f64 / (w.max(1) as f64)).round())
                })
                .unwrap_or((max, (max * 0.6).round()));
            children.push(json!({"kind":"image","src":path,"style":{"width":w,"height":h,"borderRadius":6,"marginVertical":4,"objectFit":"contain"}}));
            continue;
        }
        let color = if span.link.is_some() {
            t["accent"].clone()
        } else if span.italic {
            json!(if dark { "#e9e3ff" } else { "#4b3b8f" })
        } else {
            t["text"].clone()
        };
        let font = if span.code { size - 1. } else { size };
        let mut st = json!({"fontSize":font,"fontWeight":if span.bold{700}else{weight},"color":color,"whiteSpace":"nowrap"});
        if span.code {
            st["fontFamily"] = json!("monospace");
            st["backgroundColor"] = json!(if dark {
                "rgba(255,255,255,0.1)"
            } else {
                "rgba(0,0,0,0.07)"
            });
            st["paddingHorizontal"] = json!(3);
            st["borderRadius"] = json!(4);
        }
        if span.strike {
            st["opacity"] = json!(0.55);
        }
        let words: Vec<_> = if span.code {
            vec![span.text.as_str()]
        } else {
            span.text.split_inclusive(char::is_whitespace).collect()
        };
        for word in words.into_iter().filter(|w| !w.is_empty()) {
            let mut node = text(json!(word), st.clone());
            if span.strike {
                let mut parts = vec![node];
                if !word.trim().is_empty() {
                    parts.push(view(json!({"position":"absolute","left":0,"right":if word.ends_with(' '){font*0.28}else{0.},"top":(font*0.68).round(),"height":1,"backgroundColor":color,"opacity":0.55}),vec![]));
                }
                node = view(json!({}), parts);
            }
            if let Some(link) = &span.link {
                node = click(view(json!({}), vec![node]), json!(["link", link]));
            }
            children.push(node);
        }
    }
    view(
        json!({"flexDirection":"row","flexWrap":"wrap","alignItems":"flex-end"}),
        children,
    )
}
pub fn render(blocks: &[Block], width: f64, t: &Value) -> Value {
    let children=blocks.iter().map(|b|match b {
        Block::Heading(level,s)=>{
            let size=[0.,22.,18.,16.,14.,13.,13.][*level];
            let mut children=vec![inl(s,size,700,width,t)];
            if *level==1{children.push(view(json!({"height":1,"marginTop":4,"backgroundColor":t["separator"]}),vec![]));}
            view(json!({"marginTop":if *level<=2{10}else{6},"marginBottom":4}),children)
        }
        Block::Paragraph(s)=>view(json!({"marginVertical":3}),vec![inl(s,14.,400,width,t)]),
        Block::Rule=>view(json!({"height":1,"marginVertical":8,"backgroundColor":t["separator"]}),vec![]),
        Block::Code(s)=>view(json!({"marginVertical":4,"padding":8,"borderRadius":7,"backgroundColor":if t["mode"]=="dark"{"rgba(0,0,0,0.35)"}else{"rgba(0,0,0,0.06)"}}),s.split('\n').map(|line|text(json!(if line.is_empty(){" "}else{line}),json!({"fontFamily":"monospace","fontSize":13,"color":t["text"],"whiteSpace":"nowrap"}))).collect()),
        Block::Quote(bs)=>{let mut content=render(bs,width,t);content["style"]["gap"]=json!(0);view(json!({"flexDirection":"row","marginVertical":4}),vec![view(json!({"width":3,"borderRadius":2,"backgroundColor":t["accent"],"marginRight":10}),vec![]),view(json!({"flex":1,"opacity":0.85}),vec![content])])},
        Block::List(items)=>{
            let mut counters=BTreeMap::new();
            let rows=items.iter().map(|it|{
                counters.retain(|&depth,_|depth<=it.depth);
                let previous:usize=counters.get(&it.depth).copied().unwrap_or(0);let n=if let Some(num)=it.num{if previous>0{previous}else{num.max(1)}}else{0};counters.insert(it.depth,n.saturating_add(1));
                let mark=if let Some(on)=it.task {
                    view(json!({"width":16,"height":16,"borderRadius":4,"marginTop":2,"marginRight":8,"alignItems":"center","justifyContent":"center","backgroundColor":if on{t["accent"].clone()}else{json!("transparent")},"borderWidth":1.5,"borderColor":if on{&t["accent"]}else{&t["textDim"]}}),if on{vec![text(json!("✓"),json!({"fontSize":11,"fontWeight":700,"color":"#fff"}))]}else{vec![]})
                }else{text(json!(if it.num.is_some(){format!("{n}.")}else{["•","◦","▪"][it.depth%3].into()}),json!({"width":18,"fontSize":14,"color":t["textDim"]}))};
                view(json!({"flexDirection":"row","alignItems":"flex-start","marginLeft":it.depth*18}),vec![mark,view(json!({"width":(width-18.*(it.depth+1) as f64).max(0.),"opacity":if it.task==Some(true){0.6}else{1.}}),vec![inl(&it.text,14.,400,width,t)])])
            }).collect();
            view(json!({"marginVertical":3,"gap":2}),rows)
        }
        Block::Table{header,align,rows}=>{
            let rows=std::iter::once(header).chain(rows).enumerate().map(|(r,row)|{
                let cells=header.iter().enumerate().map(|(c,_)|view(json!({"flex":1,"padding":6,"alignItems":align.get(c).copied().unwrap_or("flex-start")}),vec![inl(row.get(c).map(String::as_str).unwrap_or(""),14.,if r==0{700}else{400},width,t)])).collect();
                view(json!({"flexDirection":"row","backgroundColor":if r==0{t["hover"].clone()}else if r%2==1{json!("transparent")}else{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.03)"}else{"rgba(0,0,0,0.02)"})},"borderTop":format!("{}px solid {}",if r>0{1}else{0},t["separator"].as_str().unwrap_or("transparent"))}),cells)
            }).collect();
            view(json!({"marginVertical":6,"borderRadius":6,"border":format!("1px solid {}",t["separator"].as_str().unwrap_or("transparent")),"overflow":"hidden"}),rows)
        }
    }).collect();
    view(json!({"gap":2}), children)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_cover_the_shipped_article_formats() {
        let bs=parse("# Title\n\nText\n\n| A | B |\n| :-- | --: |\n| x | y |\n\n> quote\n> next\n\n1. one\n   continued\n  - [x] done\n\n---\n\n```rust\nlet x = 1;\n```\n");
        assert_eq!(cells("|| B |"), ["", "B"]);
        assert!(matches!(bs[0], Block::Heading(1, _)));
        assert!(matches!(bs[2], Block::Table { .. }));
        assert!(matches!(bs[3], Block::Quote(_)));
        let Block::List(items) = &bs[4] else { panic!() };
        assert_eq!(items[0].text, "one continued");
        assert_eq!(items[1].task, Some(true));
        assert_eq!(items[1].depth, 1);
        assert!(matches!(bs[5], Block::Rule));
        assert!(matches!(bs[6], Block::Code(_)));
    }
    #[test]
    fn nested_inline_styles_links_escapes_and_unicode_survive() {
        let spans = inline(
            "**bold [ação](help:clock)** `code` *italic* ~~gone~~ foo_bar_baz \\*",
            &Span::default(),
            0,
        );
        assert!(spans
            .iter()
            .any(|s| s.bold && s.link.as_deref() == Some("help:clock") && s.text == "ação"));
        assert!(spans.iter().any(|s| s.code && s.text == "code"));
        assert!(spans.iter().any(|s| s.italic));
        assert!(spans.iter().any(|s| s.strike));
        assert!(spans.iter().any(|s| s.text.contains("foo_bar_baz *")));
    }
    #[test]
    fn malformed_and_deeply_nested_markup_terminates() {
        for src in [
            ">".repeat(10000),
            "***[bad](".repeat(1000),
            "# \n###\n```\nunclosed".into(),
        ] {
            assert!(!parse(&src).is_empty());
        }
        assert!(inline(&"__".repeat(500), &Span::default(), 0).len() > 0);
    }
}
