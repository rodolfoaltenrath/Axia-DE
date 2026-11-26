//! Mail's rules that need no window: names and sizes, what a reply or forward starts with, what
//! goes out (text and the simple HTML made from the markdown), recipients, links, the provider
//! table and the list request.
use crate::native_view::markdown::{self, Block, Span};
use serde_json::{json, Map, Value};

pub const CATEGORY_COLORS: [&str; 6] = ["#ff453a", "#ff9f0a", "#ffd60a", "#30d158", "#0a84ff", "#bf5af2"];
/// A message list row: sender, subject and two lines of preview.
pub const ROW: f64 = 86.;
pub const MAX_ATTACHMENTS: usize = 15;

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// The same rule as axia-maild (`mime_safe_filename`): a name the sender chose becomes a plain
/// base name without path parts, leading dots, control, direction-override or FAT-reserved
/// characters, at most 200 bytes. Never empty.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String = base
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .map(|c| match c {
            '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}' => '_',
            c => c,
        })
        .collect();
    while out.len() > 200 {
        out.pop();
    }
    let out = out.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if out.chars().all(|c| c == '.' || c == '_' || c.is_whitespace()) {
        "attachment".into()
    } else {
        out.into()
    }
}

pub fn format_size(bytes: f64) -> String {
    let n = if bytes.is_finite() { bytes.max(0.) } else { 0. };
    if n < 1024. {
        format!("{n:.0} B")
    } else if n < 1024. * 1024. {
        format!("{:.0} KB", n / 1024.)
    } else {
        format!("{:.1} MB", n / 1_048_576.)
    }
}

/// The extension shown on a document glyph, and the color of its band.
pub fn doc_glyph(name: &str, mime: &str) -> (String, &'static str) {
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("")
        .to_uppercase();
    let both = format!("{mime}{name}");
    let color = if mime.contains("pdf") {
        "#e5483f"
    } else if mime.starts_with("image") {
        "#30a46c"
    } else if both.contains("calendar") || both.contains("ics") {
        "#f2463d"
    } else if mime.contains("zip") || mime.contains("compress") {
        "#8e8e93"
    } else {
        "#2f7cf6"
    };
    (extension, color)
}

// ---------------------------------------------------------------- what goes out

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn safe_url(url: &str) -> &str {
    let lower = url.to_ascii_lowercase();
    if ["http:", "https:", "mailto:"].iter().any(|p| lower.starts_with(p)) {
        url
    } else {
        "#"
    }
}
fn inline_html(source: &str) -> String {
    markdown::spans(source)
        .iter()
        .map(|span: &Span| {
            if span.image.is_some() {
                return escape(&span.text);
            }
            let mut s = escape(&span.text).replace('\n', "<br>");
            if span.code {
                s = format!("<code style=\"font-family:monospace;background:#f2f2f2;padding:0 3px\">{s}</code>");
            }
            if span.bold {
                s = format!("<b>{s}</b>");
            }
            if span.italic {
                s = format!("<i>{s}</i>");
            }
            if span.strike {
                s = format!("<s>{s}</s>");
            }
            if let Some(link) = &span.link {
                s = format!("<a href=\"{}\">{s}</a>", escape(safe_url(link)));
            }
            s
        })
        .collect()
}
fn blocks_html(blocks: &[Block]) -> String {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            Block::Heading(level, text) => {
                let level = (*level).min(3);
                out.push(format!("<h{level}>{}</h{level}>", inline_html(text)));
            }
            Block::Paragraph(text) => out.push(format!("<p style=\"margin:0 0 10px 0\">{}</p>", inline_html(text))),
            Block::Quote(inner) => out.push(format!(
                "<blockquote style=\"margin:0 0 10px 0;padding-left:10px;border-left:3px solid #ccc;color:#555\">{}</blockquote>",
                blocks_html(inner)
            )),
            Block::Code(text) => out.push(format!("<pre style=\"font-family:monospace;background:#f5f5f5;padding:8px\">{}</pre>", escape(text))),
            Block::Rule => out.push("<hr>".into()),
            Block::List(items) => {
                let ordered = items.first().is_some_and(|i| i.num.is_some());
                out.push(if ordered { "<ol>" } else { "<ul>" }.into());
                for item in items {
                    let mark = match item.task {
                        None => "",
                        Some(true) => "☑ ",
                        Some(false) => "☐ ",
                    };
                    out.push(format!("<li style=\"margin-left:{}px\">{mark}{}</li>", item.depth * 16, inline_html(&item.text)));
                }
                out.push(if ordered { "</ol>" } else { "</ul>" }.into());
            }
            Block::Table { header, rows, .. } => {
                out.push("<table border=\"1\" cellpadding=\"4\" style=\"border-collapse:collapse\"><tr>".into());
                out.push(header.iter().map(|c| format!("<th>{}</th>", inline_html(c))).collect());
                out.push("</tr>".into());
                for row in rows {
                    out.push(format!("<tr>{}</tr>", row.iter().map(|c| format!("<td>{}</td>", inline_html(c))).collect::<String>()));
                }
                out.push("</table>".into());
            }
        }
    }
    out.join("\n")
}
/// The text/html part. It is built from the parsed markdown: nothing typed goes through as HTML.
pub fn markdown_html(source: &str) -> String {
    format!(
        "<div style=\"font-family:Inter,Arial,sans-serif;font-size:14px;line-height:1.45\">{}</div>",
        blocks_html(&markdown::parse(source))
    )
}
/// The text/plain part: the markdown stays readable; links become "text <url>".
pub fn markdown_text(source: &str) -> String {
    let link = regex::Regex::new(r"\[([^\]]+)\]\(([^)\s]+)\)").expect("a built-in pattern");
    let bold = regex::Regex::new(r"\*\*(.+?)\*\*").expect("a built-in pattern");
    let linked = link.replace_all(source, |c: &regex::Captures| {
        if c[1] == c[2] {
            c[2].to_owned()
        } else {
            format!("{} <{}>", &c[1], &c[2])
        }
    });
    bold.replace_all(&linked, "*$1*").into_owned()
}
/// The quoted original of a reply.
pub fn quote(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|l| if l.starts_with('>') { format!(">{l}") } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------- addresses

pub fn address(name: &str, email: &str) -> Value {
    json!({"name":name,"email":email})
}
pub fn address_full(a: &Value) -> String {
    match str_of(&a["name"]) {
        "" => str_of(&a["email"]).to_owned(),
        name => format!("{name} <{}>", str_of(&a["email"])),
    }
}
pub fn address_short(a: &Value) -> &str {
    match str_of(&a["name"]) {
        "" => str_of(&a["email"]),
        name => name,
    }
}
pub fn email_ok(email: &str) -> bool {
    let plain = |s: &str| !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || matches!(c, '@' | '<' | '>' | ',' | ';' | '"'));
    match email.split_once('@') {
        Some((local, domain)) => plain(local) && plain(domain) && domain.trim_matches('.').contains('.'),
        None => false,
    }
}
/// What was typed in a recipient field: `Name <address>` or an address.
pub fn parse_recipient(typed: &str) -> Value {
    let t = typed.trim();
    if let Some(inner) = t.strip_suffix('>') {
        if let Some((name, email)) = inner.rsplit_once('<') {
            let name = name.trim();
            let name = name.strip_prefix('"').unwrap_or(name);
            let name = name.strip_suffix('"').unwrap_or(name).trim();
            if !name.contains(['"', '<']) && !email.is_empty() {
                return address(name, email.trim());
            }
        }
    }
    address("", t)
}

// ---------------------------------------------------------------- reply and forward

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reply {
    Sender,
    All,
    Forward,
}
/// Captions a draft starts with, already translated.
pub struct DraftWords {
    pub quote_head: String,
    pub forwarded: String,
    pub from: String,
    pub date: String,
    pub subject: String,
    pub to: String,
    /// The original's date, written out.
    pub when: String,
}
fn list_of(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn has_prefix(subject: &str, prefixes: &[&str]) -> bool {
    let lower = subject.to_lowercase();
    prefixes.iter().any(|p| lower.starts_with(p))
}
/// The draft a reply or a forward opens with. `me` is the account's own address.
pub fn reply_draft(message: &Value, kind: Reply, me: &str, words: &DraftWords) -> Value {
    let acct = message["acct"].clone();
    let original = json!({"acct":acct,"folder":message["folder"],"uid":message["uid"]});
    let subject = str_of(&message["subject"]);
    let text = str_of(&message["text"]);
    if kind == Reply::Forward {
        let from = list_of(&message["from"]).iter().map(address_full).collect::<Vec<_>>().join(", ");
        let to = list_of(&message["to"]).iter().map(|a| str_of(&a["email"]).to_owned()).collect::<Vec<_>>().join(", ");
        let head = [
            format!("---------- {} ----------", words.forwarded),
            format!("{}: {from}", words.from),
            format!("{}: {}", words.date, words.when),
            format!("{}: {subject}", words.subject),
            format!("{}: {to}", words.to),
        ]
        .join("\n");
        let mut original = original;
        original["forward"] = json!(true);
        let subject = if has_prefix(subject, &["fw:", "fwd:", "rv:", "tr:"]) { subject.to_owned() } else { format!("Fwd: {subject}") };
        return json!({"acct":acct,"subject":subject,"body":format!("\n\n{head}\n\n{text}"),"original":original});
    }
    let me = me.to_lowercase();
    let other = |a: &Value| str_of(&a["email"]).to_lowercase() != me;
    let from = match list_of(&message["replyTo"]) {
        list if !list.is_empty() => list,
        _ => list_of(&message["from"]),
    };
    let to: Vec<Value> = from.iter().filter(|a| other(a)).cloned().collect();
    let cc: Vec<Value> = if kind == Reply::All {
        list_of(&message["to"])
            .into_iter()
            .chain(list_of(&message["cc"]))
            .filter(|a| other(a) && !to.iter().any(|t| t["email"] == a["email"]))
            .collect()
    } else {
        Vec::new()
    };
    let references = [str_of(&message["refs"]), str_of(&message["msgid"])].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    let subject = if has_prefix(subject, &["re:"]) { subject.to_owned() } else { format!("Re: {subject}") };
    json!({
        "acct":acct,"to":if to.is_empty() { from } else { to },"cc":cc,"subject":subject,
        "body":format!("\n\n{}\n{}", words.quote_head, quote(text)),
        "inReplyTo":message["msgid"],"references":references.trim(),"original":original,
    })
}
/// The body of a new compose window: what the draft brings, with the account's signature.
pub fn initial_body(body: &str, signature: &str) -> String {
    if signature.is_empty() {
        return body.to_owned();
    }
    let block = format!("-- \n{signature}");
    if body.is_empty() {
        format!("\n\n{block}")
    } else {
        // above the quoted text, after the room to write
        format!("\n\n{block}\n\n{}", body.trim_start_matches('\n'))
    }
}

// ---------------------------------------------------------------- links

/// The host a link leads to, as the confirmation shows it.
pub fn link_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(scheme, rest)| {
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            rest
        } else {
            url
        }
    });
    rest.split(['/', '?', '#']).next().unwrap_or("")
}
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |i: usize| bytes.get(i).and_then(|b| (*b as char).to_digit(16));
        match (bytes[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(a), Some(b)) => {
                out.push((a * 16 + b) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
/// A `mailto:` link as a draft; `None` for any other address.
pub fn mailto_draft(url: &str) -> Option<Value> {
    if !url.get(..7)?.eq_ignore_ascii_case("mailto:") {
        return None;
    }
    let (to, query) = url[7..].split_once('?').unwrap_or((&url[7..], ""));
    let subject = query
        .split('&')
        .find_map(|p| p.split_once('=').filter(|(k, _)| k.eq_ignore_ascii_case("subject")))
        .map(|(_, v)| percent_decode(v))
        .unwrap_or_default();
    Some(json!({"to":[address("", &percent_decode(to))],"subject":subject}))
}

// ---------------------------------------------------------------- accounts

struct Provider {
    domains: &'static [&'static str],
    name: &'static str,
    imap: (&'static str, u16, &'static str),
    smtp: (&'static str, u16, &'static str),
    sent_saved: bool,
    app_password: &'static str,
    oauth: &'static str,
    caldav: &'static str,
    carddav: &'static str,
    dav_note: &'static str,
}
const PROVIDERS: [Provider; 4] = [
    Provider {
        domains: &["gmail.com", "googlemail.com"],
        name: "Gmail",
        imap: ("imap.gmail.com", 993, "ssl"),
        smtp: ("smtp.gmail.com", 465, "ssl"),
        sent_saved: true,
        app_password: "https://myaccount.google.com/apppasswords",
        oauth: "google",
        caldav: "https://apidata.googleusercontent.com/caldav/v2/",
        carddav: "",
        dav_note: "oauth",
    },
    Provider {
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com", "outlook.es", "hotmail.es"],
        name: "Outlook.com",
        imap: ("outlook.office365.com", 993, "ssl"),
        smtp: ("smtp-mail.outlook.com", 587, "starttls"),
        sent_saved: true,
        app_password: "https://account.live.com/proofs/AppPassword",
        oauth: "microsoft",
        caldav: "",
        carddav: "",
        dav_note: "",
    },
    Provider {
        domains: &["yahoo.com", "ymail.com", "yahoo.es", "yahoo.com.ar", "yahoo.com.br", "rocketmail.com"],
        name: "Yahoo",
        imap: ("imap.mail.yahoo.com", 993, "ssl"),
        smtp: ("smtp.mail.yahoo.com", 465, "ssl"),
        sent_saved: false,
        app_password: "https://login.yahoo.com/account/security",
        oauth: "",
        caldav: "",
        carddav: "",
        dav_note: "",
    },
    Provider {
        domains: &["icloud.com", "me.com", "mac.com"],
        name: "iCloud",
        imap: ("imap.mail.me.com", 993, "ssl"),
        smtp: ("smtp.mail.me.com", 587, "starttls"),
        sent_saved: false,
        app_password: "https://appleid.apple.com/account/manage",
        oauth: "",
        caldav: "https://caldav.icloud.com/",
        carddav: "https://contacts.icloud.com/",
        dav_note: "",
    },
];
fn server((host, port, security): (&str, u16, &str)) -> Value {
    json!({"host":host,"port":port,"security":security})
}
/// The servers of an address: the built-in provider table (no lookups), or a guess to edit.
pub fn autoconfig(email: &str) -> Option<Value> {
    let domain = email.split('@').nth(1)?.trim().to_lowercase();
    if domain.is_empty() {
        return None;
    }
    let Some(p) = PROVIDERS.iter().find(|p| p.domains.contains(&domain.as_str())) else {
        return Some(json!({"name":domain,"imap":server((&format!("imap.{domain}"), 993, "ssl")),
            "smtp":server((&format!("smtp.{domain}"), 465, "ssl")),"sentSaved":false,"user":email,"known":false}));
    };
    let mut out = Map::new();
    for (key, value) in [("name", p.name), ("appPassword", p.app_password), ("oauth", p.oauth), ("caldav", p.caldav), ("carddav", p.carddav), ("davNote", p.dav_note)] {
        if !value.is_empty() {
            out.insert(key.into(), json!(value));
        }
    }
    out.extend([
        ("imap".to_owned(), server(p.imap)),
        ("smtp".to_owned(), server(p.smtp)),
        ("sentSaved".to_owned(), json!(p.sent_saved)),
        ("user".to_owned(), json!(email)),
        ("known".to_owned(), json!(true)),
    ]);
    Some(Value::Object(out))
}
/// Changing the security switches a standard port with it.
pub fn port_for(port: u16, security: &str) -> u16 {
    let ssl = security == "ssl";
    match port {
        993 | 143 => {
            if ssl {
                993
            } else {
                143
            }
        }
        465 | 587 => {
            if ssl {
                465
            } else {
                587
            }
        }
        other => other,
    }
}
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out.trim_end_matches('-').to_owned();
    out.truncate(24);
    if out.is_empty() {
        "account".into()
    } else {
        out
    }
}
/// A new account's id, unique among `taken`.
pub fn account_id(base: &str, taken: &[&str]) -> String {
    let mut id = slug(base);
    let mut n = 2;
    while taken.contains(&id.as_str()) {
        let mut stem = slug(base);
        stem.truncate(20);
        id = format!("{stem}-{n}");
        n += 1;
    }
    id
}

// ---------------------------------------------------------------- the list

/// What the message list shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Every inbox, optionally filtered ("unread", "flagged") or one color category (1 to 6).
    Unified { filter: &'static str, label: u8 },
    Folder { account: String, folder: String },
    Search(String),
}
impl Default for Source {
    fn default() -> Self {
        Source::Unified { filter: "all", label: 0 }
    }
}
impl Source {
    /// The helper request for the rows `first..first + count`.
    pub fn request(&self, first: usize, count: usize) -> (&'static str, Value) {
        match self {
            Source::Search(query) => ("search", json!({"q":query,"count":1000})),
            Source::Unified { filter, label } => {
                let mut args = json!({"folder":"*inbox","threaded":true,"offset":first,"count":count,"filter":filter});
                if *label > 0 {
                    args["label"] = json!(label);
                }
                ("list", args)
            }
            Source::Folder { account, folder } => {
                ("list", json!({"acct":account,"folder":folder,"threaded":true,"offset":first,"count":count,"filter":"all"}))
            }
        }
    }
}
/// A list row's identity.
pub fn item_key(item: &Value) -> String {
    format!("{}|{}|{}", str_of(&item["acct"]), str_of(&item["folder"]), item["uid"])
}
pub fn is_unread(item: &Value) -> bool {
    match item["unreadN"].as_f64() {
        Some(n) => n > 0.,
        None => item["seen"] != true,
    }
}
pub fn is_flagged(item: &Value) -> bool {
    item["anyFlag"] == true || item["flagged"] == true
}
/// The messages an action on the selected row applies to: the account, folder and ids.
pub fn targets(selected: &Value) -> Value {
    let own = |x: &&Value| x["acct"] == selected["acct"] && x["folder"] == selected["folder"];
    let uids: Vec<Value> = match selected["thread"].as_array().filter(|t| !t.is_empty()) {
        Some(thread) => thread.iter().filter(own).map(|x| x["uid"].clone()).collect(),
        None => vec![selected["uid"].clone()],
    };
    json!({"acct":selected["acct"],"folder":selected["folder"],"uids":uids})
}
/// Which date form a list row uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ListDate {
    Time,
    Yesterday,
    Weekday,
    Short,
    Numeric,
}
/// `then` and `now` are local calendar times; `age_ms` is how long ago `then` was.
pub fn list_date(then: &libc::tm, now: &libc::tm, yesterday: &libc::tm, age_ms: i64) -> ListDate {
    let day = |t: &libc::tm| (t.tm_year, t.tm_yday);
    if day(then) == day(now) {
        ListDate::Time
    } else if day(then) == day(yesterday) {
        ListDate::Yesterday
    } else if age_ms < 6 * 86_400_000 {
        ListDate::Weekday
    } else if then.tm_year == now.tm_year {
        ListDate::Short
    } else {
        ListDate::Numeric
    }
}

// ---------------------------------------------------------------- attachment files

/// A path the helper gives inside its cache: relative, without `..` or an absolute start.
pub fn inside_cache(relative: &str) -> bool {
    let path = std::path::Path::new(relative);
    !relative.is_empty() && path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
}
/// The user's downloads folder, or the home folder when there is none.
fn downloads(home: &std::path::Path) -> std::path::PathBuf {
    ["Descargas", "Downloads", "Transferências"].iter().map(|d| home.join(d)).find(|d| d.is_dir()).unwrap_or_else(|| home.to_owned())
}
/// `report.pdf`, then `report (2).pdf`…
fn numbered(name: &str, n: u32) -> String {
    if n < 2 {
        return name.to_owned();
    }
    match name.rfind('.') {
        Some(dot) => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        None => format!("{name} ({n})"),
    }
}
/// Mail in the session saves an attachment itself: a copy of the cached file in the downloads
/// folder, under a name that is not taken. Nothing is ever overwritten.
pub fn save_to_downloads(cache: &std::path::Path, relative: &str, name: &str, home: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::io::{Error, ErrorKind};
    if !inside_cache(relative) || !home.is_absolute() {
        return Err(Error::from(ErrorKind::InvalidInput));
    }
    let mut source = std::fs::File::open(cache.join(relative))?;
    let folder = downloads(home);
    let name = safe_name(name);
    for n in 1..100 {
        let target = folder.join(numbered(&name, n));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                if let Err(error) = std::io::copy(&mut source, &mut file) {
                    let _ = std::fs::remove_file(&target);
                    return Err(error);
                }
                return Ok(target);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::from(ErrorKind::AlreadyExists))
}
