//! Captions for native views: the port of `t()` (sdk/i18n.js) and `errorText()`
//! (sdk/errors.js). Catalogs come from the user's language packs, then the
//! bundled assets, as `lord.locale.catalog` reads them; English is the fallback.
use serde_json::{Map, Value};
use std::{collections::HashMap, path::PathBuf};
const FALLBACK: &str = "en";
#[derive(Default)]
pub struct I18n {
    pub lang: String,
    /// The decimal separator of the region ("," or ".").
    pub decimal: String,
    /// The digit group separator of the region (",", ".", a space, or none).
    pub group: String,
    roots: Vec<PathBuf>,
    cache: HashMap<(String, String), Option<Map<String, Value>>>,
}
fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !id.contains("..")
}
impl I18n {
    pub fn new(assets: &str, home: Option<&str>) -> Self {
        let mut roots = Vec::new();
        if let Some(home) = home.filter(|h| !h.is_empty()) {
            roots.push(PathBuf::from(home).join(".local/share/axia/locales"));
        }
        if !assets.is_empty() {
            roots.push(PathBuf::from(assets).join("locales"));
        }
        Self {
            lang: FALLBACK.into(),
            decimal: ".".into(),
            group: ",".into(),
            roots,
            cache: HashMap::new(),
        }
    }
    fn catalog(&mut self, lang: &str, ns: &str) -> Option<&Map<String, Value>> {
        let key = (lang.to_owned(), ns.to_owned());
        if !self.cache.contains_key(&key) {
            let found = (valid(lang) && valid(ns))
                .then(|| {
                    self.roots.iter().find_map(|root| {
                        let text = crate::monitor::read(
                            &root.join(lang).join(format!("{ns}.json")),
                            1 << 20,
                        );
                        serde_json::from_str::<Value>(&text)
                            .ok()?
                            .as_object()
                            .cloned()
                    })
                })
                .flatten();
            self.cache.insert(key.clone(), found);
        }
        self.cache.get(&key).and_then(Option::as_ref)
    }
    /// Raw localized service data, using the same roots and cache as captions.
    pub fn entry(&mut self, lang: &str, namespace: &str, key: &str) -> Option<Value> {
        self.catalog(lang, namespace)?.get(key).cloned()
    }
    fn lookup(&mut self, lang: &str, key: &str) -> Option<Value> {
        let (ns, id) = key.split_once('.')?;
        self.catalog(lang, ns)?.get(id).cloned()
    }
    fn plural_rule(&mut self, n: u64) -> &'static str {
        let lang = self.lang.clone();
        let rule = self
            .catalog(&lang, "meta")
            .and_then(|m| m.get("plural"))
            .and_then(Value::as_str)
            .unwrap_or("one-other")
            .to_owned();
        let (m10, m100) = (n % 10, n % 100);
        match rule.as_str() {
            "zero-one-other" => {
                if n <= 1 {
                    "one"
                } else {
                    "other"
                }
            }
            "none" => "other",
            "east-slavic" | "polish" => {
                if (rule == "polish" && n == 1) || (rule != "polish" && m10 == 1 && m100 != 11) {
                    "one"
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    "few"
                } else {
                    "many"
                }
            }
            "czech-slovak" => match n {
                1 => "one",
                2..=4 => "few",
                _ => "other",
            },
            "arabic" => match (n, m100) {
                (0, _) => "zero",
                (1, _) => "one",
                (2, _) => "two",
                (_, 3..=10) => "few",
                (_, 11..) => "many",
                _ => "other",
            },
            _ => {
                if n == 1 {
                    "one"
                } else {
                    "other"
                }
            }
        }
    }
    /// `t(key, vars)`: the user's language, then English, then the key itself.
    pub fn t(&mut self, key: &str, vars: &Map<String, Value>) -> String {
        let lang = self.lang.clone();
        let found = self.lookup(&lang, key).or_else(|| {
            (lang != FALLBACK)
                .then(|| self.lookup(FALLBACK, key))
                .flatten()
        });
        let text = match found {
            None => key.to_owned(),
            Some(Value::Object(forms)) => {
                let n = vars
                    .get("count")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.)
                    .abs() as u64;
                let form = self.plural_rule(n);
                forms
                    .get(form)
                    .or_else(|| forms.get("other"))
                    .or_else(|| forms.values().next())
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_owned()
            }
            Some(Value::String(s)) => s,
            Some(other) => other.to_string(),
        };
        fill(&text, vars)
    }
    fn optional(&mut self, key: &str, vars: &Map<String, Value>) -> Option<String> {
        let lang = self.lang.clone();
        self.lookup(&lang, key)
            .or_else(|| self.lookup(FALLBACK, key))
            .map(|_| self.t(key, vars))
    }
    /// `errorText`: "ns.code key=value…" lines become their translation in `errors`.
    pub fn error(&mut self, line: &str) -> String {
        let Some((code, rest)) = parse_code(line) else {
            return line.to_owned();
        };
        let mut params = Map::new();
        for part in rest.split(' ') {
            if let Some((k, v)) = part.split_once('=').filter(|(k, _)| !k.is_empty()) {
                params.insert(k.into(), Value::from(decode(v)));
            }
        }
        if let Some(e) = params
            .get("errno")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            let t = self.optional(&format!("errors.errno.{e}"), &Map::new());
            params.insert("errno".into(), Value::from(t.unwrap_or(e)));
        }
        if let Some(r) = params
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            params.insert("reason".into(), Value::from(self.error(&r)));
        }
        self.optional(&format!("errors.{code}"), &params)
            .unwrap_or_else(|| line.to_owned())
    }
    /// A number with at most `digits` decimals, no trailing zeros, in the region's separators.
    fn number(&self, value: f64, digits: usize) -> String {
        let fixed = format!("{value:.digits$}");
        let n: f64 = fixed.parse().unwrap_or(value);
        if n.abs() >= 1e15 || (n != 0. && n.abs() < 1e-9) {
            return format!("{value:.7e}");
        }
        let (whole, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
        let (sign, whole) = whole.strip_prefix('-').map_or(("", whole), |w| ("-", w));
        let mut grouped = String::new();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 {
                grouped.push_str(&self.group);
            }
            grouped.push(c);
        }
        let tail = frac.trim_end_matches('0');
        let sign = if sign == "-" && n == 0. { "" } else { sign };
        format!("{sign}{grouped}{}{tail}", if tail.is_empty() { "" } else { self.decimal.as_str() })
    }
    /// `caption(value)` of rustwidgets.js: text, a number, `{key, params}` or `{error}`.
    pub fn caption(&mut self, value: &Value) -> String {
        match value {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Object(o) => {
                if let Some(e) = o.get("error") {
                    return match e {
                        Value::String(s) => self.error(s),
                        other => self.caption(other),
                    };
                }
                // the Spotlight's captions (sdk/search.js searchCaption)
                if let Some(parts) = o.get("parts").and_then(Value::as_array) {
                    return parts.iter().map(|p| self.caption(p)).collect();
                }
                if let Some(upper) = o.get("upper") {
                    return self.caption(upper).to_uppercase();
                }
                if let Some(n) = o.get("number").and_then(Value::as_f64) {
                    return self.number(n, o.get("digits").and_then(Value::as_u64).unwrap_or(0).min(15) as usize);
                }
                if let Some(n) = o.get("size").and_then(Value::as_f64) {
                    return match n {
                        n if n < 1024. => format!("{n} B"),
                        n if n < 1_048_576. => format!("{} KB", self.number(n / 1024., 1)),
                        n if n < 1_073_741_824. => format!("{} MB", self.number(n / 1_048_576., 1)),
                        n => format!("{} GB", self.number(n / 1_073_741_824., 1)),
                    };
                }
                let Some(key) = o.get("key").and_then(Value::as_str) else {
                    return String::new();
                };
                let mut vars = Map::new();
                if let Some(params) = o.get("params").and_then(Value::as_object) {
                    for (k, v) in params {
                        let v = match v {
                            Value::Object(_) => Value::from(self.caption(v)),
                            other => other.clone(),
                        };
                        vars.insert(k.clone(), v);
                    }
                }
                self.t(key, &vars)
            }
            Value::Array(_) => String::new(),
        }
    }
}
fn fill(text: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end)
                if !after[..end].is_empty()
                    && after[..end]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_') =>
            {
                match vars.get(&after[..end]).filter(|v| !v.is_null()) {
                    Some(Value::String(s)) => out.push_str(s),
                    Some(v) => out.push_str(&v.to_string()),
                    None => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
/// `ns.code` at the start of a line (`errors.js` CODE_RE), and the rest.
fn parse_code(line: &str) -> Option<(&str, &str)> {
    let end = line.find(' ').unwrap_or(line.len());
    let code = &line[..end];
    let mut parts = code.split('.');
    let first = parts.next()?;
    let ok = first.starts_with(|c: char| c.is_ascii_lowercase())
        && first.chars().all(|c| c.is_ascii_alphanumeric())
        && code.contains('.')
        && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()));
    ok.then(|| (code, &line[end..]))
}
fn decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(n) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| v.to_owned())
}
