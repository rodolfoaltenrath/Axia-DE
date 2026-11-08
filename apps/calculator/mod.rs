//! Calculator: bounded arithmetic, keyboard/pointer state, display and icon are Rust-owned.
#[cfg(test)]
mod tests;
mod ui;
use serde_json::{json, Value};
pub const KEYS: [[&str; 4]; 5] = [
    ["C", "(", ")", "÷"],
    ["7", "8", "9", "×"],
    ["4", "5", "6", "-"],
    ["1", "2", "3", "+"],
    ["0", ".", "⌫", "="],
];
pub struct App {
    expression: String,
    result: String,
    error: bool,
    hover: Option<String>,
    down: Option<String>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            expression: String::new(),
            result: "0".into(),
            error: false,
            hover: None,
            down: None,
        }
    }
}
impl App {
    fn press(&mut self, key: &str) {
        if !KEYS.iter().flatten().any(|k| *k == key) {
            return;
        }
        match key {
            "C" => {
                self.expression.clear();
                self.result = "0".into();
                self.error = false;
            }
            "⌫" => {
                self.expression.pop();
            }
            "=" => match evaluate(&self.expression) {
                Some(value) => {
                    self.result = number(value);
                    self.expression = self.result.clone();
                    self.error = false;
                }
                None => self.error = true,
            },
            _ => {
                if self.expression.chars().count() < 40 {
                    self.expression.push_str(key);
                }
            }
        }
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let key = input["key"].as_str().unwrap_or("");
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                json!({"tree":ui::tree(self,&input["context"]),"expression":self.expression,"result":self.result,"failed":self.error})
            }
            "icon" => ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "press" => {
                self.press(key);
                json!({})
            }
            "key" => {
                let key = input["event"].as_str().unwrap_or("");
                let ch = input["char"].as_str().unwrap_or("");
                let key = match key {
                    "Enter" => "=",
                    "Backspace" => "⌫",
                    "Escape" => "C",
                    _ => {
                        if KEYS.iter().flatten().any(|k| *k == key)
                            || ["*", "/", ","].contains(&key)
                        {
                            key
                        } else {
                            ch
                        }
                    }
                };
                let key = match key {
                    "*" => "×",
                    "/" => "÷",
                    "," => ".",
                    _ => key,
                };
                self.press(key);
                json!({})
            }
            "hover" | "down" | "up" | "leave" if KEYS.iter().flatten().any(|k| *k == key) => {
                match input["cmd"].as_str().unwrap() {
                    "hover" => self.hover = Some(key.into()),
                    "down" => self.down = Some(key.into()),
                    "up" => {
                        if self.down.as_deref() == Some(key) {
                            self.down = None;
                        }
                    }
                    _ => {
                        if self.hover.as_deref() == Some(key) {
                            self.hover = None;
                        }
                        if self.down.as_deref() == Some(key) {
                            self.down = None;
                        }
                    }
                }
                json!({})
            }
            _ => json!({"error":"app.invalidRequest"}),
        }
    }
}
fn number(value: f64) -> String {
    if value == 0. {
        return "0".into();
    }
    if value.abs() < 1e-6 || value.abs() >= 1e21 {
        format!("{value:e}")
    } else {
        value.to_string()
    }
}
/// Full-input parsing; invalid characters and unmatched parentheses are never discarded.
fn evaluate(source: &str) -> Option<f64> {
    if source.len() > 4096 {
        return None;
    }
    let normalized = source.replace('×', "*").replace('÷', "/").replace(',', ".");
    let mut p = Parser {
        bytes: normalized.as_bytes(),
        at: 0,
        depth: 0,
    };
    let value = p.expression()?;
    p.space();
    if p.at != p.bytes.len() || !value.is_finite() {
        return None;
    }
    let scaled = value * 1e10;
    Some(if scaled.is_finite() && scaled.abs() < 4503599627370496. {
        (scaled + 0.5).floor() / 1e10
    } else {
        value
    })
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}
impl Parser<'_> {
    fn peek(&self) -> u8 {
        self.bytes.get(self.at).copied().unwrap_or(0)
    }
    fn space(&mut self) {
        while self.peek().is_ascii_whitespace() {
            self.at += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.space();
        if self.peek() == c {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expression(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.take(b'+') {
                v += self.term()?;
            } else if self.take(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        loop {
            if self.take(b'*') {
                v *= self.factor()?;
            } else if self.take(b'/') {
                v /= self.factor()?;
            } else {
                return Some(v);
            }
        }
    }
    fn factor(&mut self) -> Option<f64> {
        if self.depth >= 64 {
            return None;
        }
        self.depth += 1;
        self.space();
        let result = if self.take(b'-') {
            self.factor().map(|v| -v)
        } else if self.take(b'+') {
            self.factor()
        } else if self.take(b'(') {
            let v = self.expression()?;
            self.take(b')').then_some(v)
        } else {
            self.numeric()
        };
        self.depth -= 1;
        result
    }
    fn numeric(&mut self) -> Option<f64> {
        let start = self.at;
        let mut digits = 0;
        while self.peek().is_ascii_digit() {
            self.at += 1;
            digits += 1;
        }
        if self.peek() == b'.' {
            self.at += 1;
            while self.peek().is_ascii_digit() {
                self.at += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if matches!(self.peek(), b'e' | b'E') {
            self.at += 1;
            if matches!(self.peek(), b'+' | b'-') {
                self.at += 1;
            }
            let exp = self.at;
            while self.peek().is_ascii_digit() {
                self.at += 1;
            }
            if self.at == exp {
                return None;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse()
            .ok()
    }
}
