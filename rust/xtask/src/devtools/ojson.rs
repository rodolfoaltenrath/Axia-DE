//! JSON that keeps the order of an object's keys, so a tool can read a catalog and write it back
//! as Node's `JSON.stringify(value, null, 2)` would. (serde_json sorts keys without an extra crate.)

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number as written.
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    p.space();
    let value = p.value(0)?;
    p.space();
    if p.i != p.b.len() {
        return Err(format!("unexpected data at byte {}", p.i));
    }
    Ok(value)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn value(&mut self, depth: u32) -> Result<Json, String> {
        if depth > 200 {
            return Err("nested too deep".into());
        }
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut out: Vec<(String, Json)> = Vec::new();
                self.space();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Object(out));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.space();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(format!("expected ':' at byte {}", self.i));
                    }
                    self.i += 1;
                    self.space();
                    let value = self.value(depth + 1)?;
                    // a repeated key keeps its first place and takes the last value, as in JavaScript
                    match out.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => out.push((key, value)),
                    }
                    self.space();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Object(out));
                        }
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                self.space();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Array(out));
                }
                loop {
                    self.space();
                    out.push(self.value(depth + 1)?);
                    self.space();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Array(out));
                        }
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
                    }
                }
            }
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                while matches!(self.b.get(self.i), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
                    self.i += 1;
                }
                Ok(Json::Number(String::from_utf8_lossy(&self.b[start..self.i]).into_owned()))
            }
            _ => Err(format!("unexpected character at byte {}", self.i)),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') {
            return Err(format!("expected a string at byte {}", self.i));
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else { return Err("unterminated string".into()) };
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "invalid UTF-8 in a string".to_string()),
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { return Err("unterminated string".into()) };
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut unit = self.hex4()?;
                            if (0xd800..0xdc00).contains(&unit) && self.b[self.i..].starts_with(b"\\u") {
                                let save = self.i;
                                self.i += 2;
                                let low = self.hex4()?;
                                if (0xdc00..0xe000).contains(&low) {
                                    unit = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                                } else {
                                    self.i = save;
                                }
                            }
                            let ch = char::from_u32(unit).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                }
                _ => out.push(c),
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.b.get(self.i..self.i + 4).ok_or("short \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|_| "bad \\u escape")?;
        self.i += 4;
        u32::from_str_radix(text, 16).map_err(|_| "bad \\u escape".into())
    }
}

/// `JSON.stringify(value, null, 2)`
pub fn pretty(value: &Json) -> String {
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

fn quote(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(value: &Json, depth: usize, out: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => out.push_str(n),
        Json::Str(s) => quote(s, out),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                write(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        Json::Object(fields) if fields.is_empty() => out.push_str("{}"),
        Json::Object(fields) => {
            out.push_str("{\n");
            for (i, (key, item)) in fields.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                quote(key, out);
                out.push_str(": ");
                write(item, depth + 1, out);
                out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
    }
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn fields(&self) -> &[(String, Json)] {
        match self {
            Json::Object(fields) => fields,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keep_their_order_and_the_text_matches_json_stringify() {
        let text = "{\"z\":1,\"a\":{\"one\":\"x\",\"zero\":\"é\\n\"},\"list\":[],\"o\":{},\"t\":[true,null,\"q\\\"\"]}";
        let value = parse(text).unwrap();
        let keys: Vec<_> = value.fields().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["z", "a", "list", "o", "t"]);
        assert_eq!(
            pretty(&value),
            "{\n  \"z\": 1,\n  \"a\": {\n    \"one\": \"x\",\n    \"zero\": \"é\\n\"\n  },\n  \"list\": [],\n  \"o\": {},\n  \"t\": [\n    true,\n    null,\n    \"q\\\"\"\n  ]\n}"
        );
        assert_eq!(parse(&pretty(&value)).unwrap(), value);
    }

    #[test]
    fn a_repeated_key_keeps_its_place_and_takes_the_last_value() {
        let value = parse("{\"a\":1,\"b\":2,\"a\":3}").unwrap();
        assert_eq!(pretty(&value), "{\n  \"a\": 3,\n  \"b\": 2\n}");
    }

    #[test]
    fn escapes_surrogates_and_errors() {
        assert_eq!(parse("\"\\ud83d\\ude00 \\u00e9\"").unwrap(), Json::Str("😀 é".into()));
        assert_eq!(pretty(&Json::Str("\u{1}\u{7f}".into())), "\"\\u0001\u{7f}\"");
        assert!(parse("{\"a\":}").is_err());
        assert!(parse("[1,2] x").is_err());
        assert!(parse("\"open").is_err());
    }
}
