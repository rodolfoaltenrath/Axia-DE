//! Bounded calculator and unit conversion; preserves the existing search grammar.
use serde_json::{json, Value};

pub fn calc(input: &str) -> Option<f64> {
    calc_depth(input, 0)
}
fn calc_depth(input: &str, depth: usize) -> Option<f64> {
    if input.len() > 4096 || depth > 64 {
        return None;
    }
    let s = super::trimmed(input)
        .to_lowercase()
        .replace('×', "*")
        .replace('÷', "/");
    let s = s
        .split(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if let Some((a, b)) = s.split_once('%') {
        let b = b.trim();
        if let Some(b) = ["de ", "of ", "do ", "da ", "del "]
            .iter()
            .find_map(|p| b.strip_prefix(p))
        {
            let val = |v: &str| {
                v.trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .ok()
                    .or_else(|| calc_depth(v, depth + 1))
            };
            let v = val(a)? / 100. * val(b)?;
            return v.is_finite().then_some(v);
        }
    }
    let s = if !s.contains('.') {
        s.replace(',', ".")
    } else {
        s
    };
    if !s.chars().any(|c| c.is_ascii_digit()) && !s.contains("pi") {
        return None;
    }
    if !s
        .trim_start_matches(['-', '+'])
        .contains(['+', '-', '*', '/', '^', '%', '(', ')'])
        && ![
            "sqrt",
            "raiz",
            "sin",
            "sen",
            "cos",
            "tan",
            "log",
            "ln",
            "abs",
            "round",
            "redondear",
            "floor",
            "ceil",
            "exp",
        ]
        .iter()
        .any(|f| s.contains(f))
    {
        return None;
    }
    let mut p = Parser {
        s: s.as_bytes(),
        at: 0,
        depth: 0,
    };
    let v = p.expr()?;
    p.space();
    (p.at == p.s.len() && v.is_finite()).then_some(v)
}
struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    depth: usize,
}
impl Parser<'_> {
    fn peek(&self) -> u8 {
        self.s.get(self.at).copied().unwrap_or(0)
    }
    fn space(&mut self) {
        while self.peek() == b' ' {
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
    fn expr(&mut self) -> Option<f64> {
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
            self.space();
            if self.peek() == b'*' && self.s.get(self.at + 1) != Some(&b'*') {
                self.at += 1;
                v *= self.factor()?;
            } else if self.take(b'/') {
                v /= self.factor()?;
            } else {
                return Some(v);
            }
        }
    }
    fn factor(&mut self) -> Option<f64> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let mut v = self.primary()?;
        while self.take(b'%') {
            v /= 100.;
        }
        self.space();
        if self.take(b'^') {
            v = v.powf(self.factor()?);
        } else if self.peek() == b'*' && self.s.get(self.at + 1) == Some(&b'*') {
            self.at += 2;
            v = v.powf(self.factor()?);
        }
        self.depth -= 1;
        Some(v)
    }
    fn primary(&mut self) -> Option<f64> {
        if self.depth >= 64 {
            return None;
        }
        self.depth += 1;
        let result = self.primary_inner();
        self.depth -= 1;
        result
    }
    fn primary_inner(&mut self) -> Option<f64> {
        self.space();
        if self.take(b'(') {
            let v = self.expr()?;
            return self.take(b')').then_some(v);
        }
        if self.take(b'-') {
            return Some(-self.factor()?);
        }
        if self.take(b'+') {
            return self.factor();
        }
        let start = self.at;
        if self.peek().is_ascii_digit() || self.peek() == b'.' {
            while self.peek().is_ascii_digit() {
                self.at += 1;
            }
            if self.peek() == b'.' {
                self.at += 1;
                while self.peek().is_ascii_digit() {
                    self.at += 1;
                }
            }
            if self.peek() == b'e' {
                self.at += 1;
                if self.peek() == b'-' || self.peek() == b'+' {
                    self.at += 1;
                }
                while self.peek().is_ascii_digit() {
                    self.at += 1;
                }
            }
            return std::str::from_utf8(&self.s[start..self.at])
                .ok()?
                .parse()
                .ok();
        }
        while self.peek().is_ascii_lowercase() {
            self.at += 1;
        }
        let id = std::str::from_utf8(&self.s[start..self.at]).ok()?;
        match id {
            "pi" => Some(std::f64::consts::PI),
            "e" => Some(std::f64::consts::E),
            "sqrt" | "raiz" | "sin" | "sen" | "cos" | "tan" | "log" | "ln" | "abs" | "round"
            | "redondear" | "floor" | "ceil" | "exp" => {
                let x = self.primary()?;
                Some(match id {
                    "sqrt" | "raiz" => x.sqrt(),
                    "sin" | "sen" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "log" => x.log10(),
                    "ln" => x.ln(),
                    "abs" => x.abs(),
                    "round" | "redondear" => (x + 0.5).floor(),
                    "floor" => x.floor(),
                    "ceil" => x.ceil(),
                    _ => x.exp(),
                })
            }
            _ => None,
        }
    }
}
fn unit(s: &str) -> Option<(u8, f64)> {
    Some(match s {
        "mm" => (0, 0.001),
        "cm" => (0, 0.01),
        "m" => (0, 1.),
        "km" => (0, 1000.),
        "in" | "pulgada" | "pulgadas" => (0, 0.0254),
        "ft" | "pie" | "pies" => (0, 0.3048),
        "yd" => (0, 0.9144),
        "mi" | "milla" | "millas" => (0, 1609.344),
        "mg" => (1, 1e-6),
        "g" => (1, 0.001),
        "kg" => (1, 1.),
        "t" => (1, 1000.),
        "lb" | "libra" | "libras" => (1, 0.45359237),
        "oz" => (1, 0.028349523125),
        "b" => (2, 1.),
        "kb" => (2, 1e3),
        "mb" => (2, 1e6),
        "gb" => (2, 1e9),
        "tb" => (2, 1e12),
        "kib" => (2, 1024.),
        "mib" => (2, 1048576.),
        "gib" => (2, 1073741824.),
        "tib" => (2, 1099511627776.),
        "ms" => (3, 0.001),
        "s" | "seg" => (3, 1.),
        "min" => (3, 60.),
        "h" | "hora" | "horas" => (3, 3600.),
        "d" | "dia" | "dias" => (3, 86400.),
        "semana" => (3, 604800.),
        "ml" => (4, 0.001),
        "l" | "litro" | "litros" => (4, 1.),
        "gal" | "galon" => (4, 3.785411784),
        "kmh" | "km/h" => (5, 1. / 3.6),
        "mph" => (5, 0.44704),
        "m/s" | "ms_" => (5, 1.),
        "nudo" | "kn" => (5, 0.514444),
        _ => return None,
    })
}
pub fn convert(input: &str) -> Option<Value> {
    if input.len() > 4096 {
        return None;
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let s = super::fold(input);
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"^\s*(-?[\d.,]+)\s*([a-z°/µ_]+)\s*(?:a|to|en|in|para|→|->|=)\s*([a-z°/µ_]+)\s*$",
        )
        .unwrap()
    });
    let m = re.captures(&s)?;
    let v = m[1].replace(',', ".").parse::<f64>().ok()?;
    if !v.is_finite() {
        return None;
    }
    let temp = |s: &str| match s {
        "c" | "°c" | "celsius" => Some("c"),
        "f" | "°f" | "fahrenheit" => Some("f"),
        "k" | "kelvin" => Some("k"),
        _ => None,
    };
    let (a, b) = (&m[2], &m[3]);
    if let (Some(a), Some(b)) = (temp(a), temp(b)) {
        let k = match a {
            "c" => v + 273.15,
            "f" => (v - 32.) * 5. / 9. + 273.15,
            _ => v,
        };
        let value = match b {
            "c" => k - 273.15,
            "f" => (k - 273.15) * 9. / 5. + 32.,
            _ => k,
        };
        let label = |s| match s {
            "c" => "°C",
            "f" => "°F",
            _ => "K",
        };
        return value
            .is_finite()
            .then(|| json!({"value":value,"from":label(a),"to":label(b),"input":v}));
    }
    let (ga, fa) = unit(a)?;
    let (gb, fb) = unit(b)?;
    let value = v * fa / fb;
    (ga == gb && value.is_finite()).then(|| json!({"value":value,"from":a,"to":b,"input":v}))
}
