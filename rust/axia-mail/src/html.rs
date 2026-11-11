//! Mail HTML without a browser; see server/src/mail/html.h. A lenient
//! tokenizer and tree builder in a single pass, emitting the JSON tree that
//! Mail renders with native views. Nothing is executed or fetched.

use crate::c::*;
use crate::html_entities::HTML_ENTS;
use crate::jbuf::JBuf;
use core::ffi::{CStr, c_char, c_int};

const HTML_MAX_INPUT: usize = 8 << 20;
const HTML_MAX_NODES: c_int = 40000;
const HTML_MAX_DEPTH: usize = 48;
const HTML_MAX_REMOTE: usize = 200;
const HTML_MAX_LINKS: usize = 1000;
const HTML_MAX_DATA: c_int = 64;
const MAX_ATTRS: usize = 32;
const MAX_OPEN: usize = HTML_MAX_DEPTH * 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct HtmlStats {
    pub remote: c_int,
    pub trackers: c_int,
    pub warn: c_int,
    pub cid: c_int,
    pub data: c_int,
    pub nodes: c_int,
    pub truncated: c_int,
}

#[repr(C)]
pub struct HtmlData {
    pub p: *const c_char,
    pub n: usize,
    pub type_: [c_char; 16],
}

// ---- text: entities and UTF-8 ----

/// windows-1252 for numeric references 128-159 (as browsers do)
const C1FIX: [u16; 32] = [
    0x20ac, 0xfffd, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039,
    0x0152, 0xfffd, 0x017d, 0xfffd, 0xfffd, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
    0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0xfffd, 0x017e, 0x0178,
];

fn ent_lookup(name: &[u8]) -> u32 {
    if name.is_empty() || name.len() >= 40 {
        return 0;
    }
    HTML_ENTS
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .map_or(0, |k| HTML_ENTS[k].1)
}

/// Decodes an entity at s[i] == '&': (code point, bytes used); (0, 0) if none.
fn entity(s: &[u8], i: usize) -> (u32, usize) {
    let len = s.len();
    if i + 1 >= len {
        return (0, 0);
    }
    if s[i + 1] == b'#' {
        let mut k = i + 2;
        let mut hex = false;
        if k < len && (s[k] == b'x' || s[k] == b'X') {
            hex = true;
            k += 1;
        }
        let mut cp: u32 = 0;
        let d0 = k;
        while k < len && k - d0 < 8 {
            let c = s[k];
            let v = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' if hex => (c - b'a' + 10) as u32,
                b'A'..=b'F' if hex => (c - b'A' + 10) as u32,
                _ => break,
            };
            cp = cp.wrapping_mul(if hex { 16 } else { 10 }).wrapping_add(v);
            k += 1;
        }
        if k == d0 {
            return (0, 0);
        }
        if k < len && s[k] == b';' {
            k += 1;
        }
        if (0x80..=0x9f).contains(&cp) {
            cp = C1FIX[(cp - 0x80) as usize] as u32;
        }
        if cp == 0 || cp > 0x10ffff || (0xd800..=0xdfff).contains(&cp) {
            cp = 0xfffd;
        }
        return (cp, k - i);
    }
    let mut k = i + 1;
    while k < len && k - i <= 32 && s[k].is_ascii_alphanumeric() {
        k += 1;
    }
    let n = k - i - 1;
    // longest matching name (legacy ones without ";", like &nbsp without a semicolon)
    let mut m = n;
    while m >= 2 {
        let cp = ent_lookup(&s[i + 1..i + 1 + m]);
        if cp != 0 {
            let mut u = 1 + m;
            if m == n && k < len && s[k] == b';' {
                u += 1;
            } else if m != n && m < 4 {
                return (0, 0); // &notit → ¬it only for long names
            }
            return (cp, u);
        }
        m -= 1;
    }
    (0, 0)
}

/// Characters that are never shown (preheader padding, BOM, soft hyphens).
fn invisible(cp: u32) -> bool {
    matches!(
        cp,
        0x034f | 0x200b | 0x200c | 0x200d | 0x2060 | 0xfeff | 0x00ad | 0x200e | 0x200f
    ) || (0x202a..=0x202e).contains(&cp)
        || (0x2066..=0x2069).contains(&cp)
}

fn next_cp(s: &[u8], i: &mut usize) -> u32 {
    let c = s[*i];
    if c < 0x80 {
        *i += 1;
        return c as u32;
    }
    let (k, mut cp) = match c {
        0xc2..=0xdf => (2, (c & 0x1f) as u32),
        0xe0..=0xef => (3, (c & 0x0f) as u32),
        0xf0..=0xf4 => (4, (c & 0x07) as u32),
        _ => (0, 0),
    };
    if k == 0 || *i + k > s.len() {
        *i += 1;
        return 0xfffd;
    }
    for j in 1..k {
        let d = s[*i + j];
        if d & 0xc0 != 0x80 {
            *i += 1;
            return 0xfffd;
        }
        cp = (cp << 6) | (d & 0x3f) as u32;
    }
    if (k == 3 && cp < 0x800)
        || (k == 4 && !(0x10000..=0x10ffff).contains(&cp))
        || (0xd800..=0xdfff).contains(&cp)
    {
        *i += 1;
        return 0xfffd;
    }
    *i += k;
    cp
}

/// Bounded integer from an attribute (clamped; dflt when absent or overflowing).
fn hnum(v: &[u8], lo: i64, hi: i64, dflt: i32) -> i32 {
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let (x, used, over) = strtol(&v[s..]);
    if used == 0 || over {
        return dflt;
    }
    x.clamp(lo, hi) as i32
}

// ---- colors and lengths ----

const NAMED: [(&[u8], u32); 40] = [
    (b"aqua", 0x00ffff),
    (b"black", 0x000000),
    (b"blue", 0x0000ff),
    (b"brown", 0xa52a2a),
    (b"crimson", 0xdc143c),
    (b"cyan", 0x00ffff),
    (b"darkblue", 0x00008b),
    (b"darkgray", 0xa9a9a9),
    (b"darkgreen", 0x006400),
    (b"darkgrey", 0xa9a9a9),
    (b"darkred", 0x8b0000),
    (b"dimgray", 0x696969),
    (b"fuchsia", 0xff00ff),
    (b"gainsboro", 0xdcdcdc),
    (b"gold", 0xffd700),
    (b"gray", 0x808080),
    (b"green", 0x008000),
    (b"grey", 0x808080),
    (b"indigo", 0x4b0082),
    (b"lightblue", 0xadd8e6),
    (b"lightgray", 0xd3d3d3),
    (b"lightgrey", 0xd3d3d3),
    (b"lime", 0x00ff00),
    (b"magenta", 0xff00ff),
    (b"maroon", 0x800000),
    (b"navy", 0x000080),
    (b"olive", 0x808000),
    (b"orange", 0xffa500),
    (b"pink", 0xffc0cb),
    (b"purple", 0x800080),
    (b"red", 0xff0000),
    (b"silver", 0xc0c0c0),
    (b"skyblue", 0x87ceeb),
    (b"steelblue", 0x4682b4),
    (b"teal", 0x008080),
    (b"tomato", 0xff6347),
    (b"violet", 0xee82ee),
    (b"white", 0xffffff),
    (b"whitesmoke", 0xf5f5f5),
    (b"yellow", 0xffff00),
];

/// "#abc", "#aabbcc", "rgb(1,2,3)", "rgba(...)", names → 0xRRGGBBAA.
fn parse_color(v: &[u8]) -> Option<u32> {
    let v = z(v);
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let v = &v[s..];
    if v.first() == Some(&b'#') {
        let h = &v[1..];
        let n = h.iter().take_while(|&&c| is_xdigit(c)).count();
        let x = h[..n.min(16)].iter().fold(0u64, |a, &c| {
            a * 16 + (c as char).to_digit(16).unwrap_or(0) as u64
        });
        return match n {
            3 => {
                let (r, g, b) = ((x >> 8) as u32 & 15, (x >> 4) as u32 & 15, x as u32 & 15);
                Some(((r * 17) << 24) | ((g * 17) << 16) | ((b * 17) << 8) | 0xff)
            }
            6 => Some(((x as u32) << 8) | 0xff),
            8 => Some(x as u32),
            _ => None,
        };
    }
    if starts_ic(v, b"rgb") {
        let mut p = chr(v, b'(')? + 1;
        let mut c = [0f64, 0., 0., 1.];
        let mut k = 0;
        while k < 4 && p < v.len() {
            while p < v.len() && matches!(v[p], b' ' | b',' | b'/') {
                p += 1;
            }
            let (mut x, used) = strtod(&v[p..]);
            if used == 0 {
                break;
            }
            let mut e = p + used;
            if v.get(e) == Some(&b'%') {
                x = if k < 3 { x * 2.55 } else { x / 100.0 };
                e += 1;
            }
            c[k] = x;
            k += 1;
            p = e;
        }
        if k < 3 {
            return None;
        }
        // clamp keeps NaN as NaN, like the C comparisons
        for x in c.iter_mut().take(3) {
            *x = x.clamp(0.0, 255.0);
        }
        let a = c[3].clamp(0.0, 1.0);
        return Some(
            ((c[0] as u32) << 24)
                | ((c[1] as u32) << 16)
                | ((c[2] as u32) << 8)
                | (a * 255.0) as u32,
        );
    }
    let mut low = Vec::new();
    for &c in v {
        if c == b' ' || c == b';' || low.len() + 1 >= 24 {
            break;
        }
        low.push(lower(c));
    }
    if low == b"transparent" {
        return Some(0);
    }
    NAMED
        .iter()
        .find(|(n, _)| *n == low.as_slice())
        .map(|(_, rgb)| (rgb << 8) | 0xff)
}

/// CSS length → px (em/rem relative to base): (px, is_percentage).
fn parse_len(v: &[u8], base: f64) -> Option<(f64, bool)> {
    let v = z(v);
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let v = &v[s..];
    let (mut x, used) = strtod(v);
    if used == 0 {
        return None;
    }
    let e = &v[used..];
    let mut pct = false;
    if e.first() == Some(&b'%') {
        pct = true;
    } else if starts_ic(e, b"pt") {
        x *= 4.0 / 3.0;
    } else if starts_ic(e, b"em") || starts_ic(e, b"rem") {
        x *= base;
    } else if starts_ic(e, b"pc") {
        x *= 16.0;
    } else if starts_ic(e, b"in") {
        x *= 96.0;
    } else if starts_ic(e, b"cm") {
        x *= 37.8;
    } else if starts_ic(e, b"mm") {
        x *= 3.78;
    }
    if x.is_nan() || x > 1e5 || x < -1e5 {
        return None;
    }
    Some((x, pct))
}

// ---- styles ----

#[derive(Clone, Copy, Default)]
struct IStyle {
    b: bool,
    i: bool,
    u: bool,
    st: bool,
    mono: bool,
    pre: bool,
    color: u32,
    bg: u32,
    has_color: bool,
    has_bg: bool,
    fs: f64,   // 0 = the app's default
    link: i32, // -1 = no link
}

#[derive(Clone, Copy, Default)]
struct BStyle {
    align: u8, // 0, 's', 'c', 'e', 'j'
    self_center: bool,
    bg: u32,
    has_bg: bool,
    pad: [f64; 4],
    has_pad: bool,
    mt: f64,
    mb: f64,
    w: f64,
    wpct: bool,
    has_w: bool,
    mw: f64,
    has_mw: bool,
    bw: f64,
    bc: u32,
    has_border: bool,
    bl: f64,
    blc: u32,
    has_bl: bool,
    radius: f64,
    valign: u8, // 't','m','b'
    hidden: bool,
}

fn sw(a: &[u8], b: &[u8]) -> bool {
    starts_ic(a, b)
}

/// Applies a style="" attribute to both styles.
fn apply_css(css: &[u8], is: &mut IStyle, bs: &mut BStyle) {
    let n = css.len();
    let mut i = 0;
    let mut overflow_hidden = false;
    let mut zero_height = false;
    while i < n {
        let a = i;
        let mut par = 0;
        while i < n && (par != 0 || css[i] != b';') {
            if css[i] == b'(' {
                par += 1;
            } else if css[i] == b')' && par != 0 {
                par -= 1;
            }
            i += 1;
        }
        let dl = i - a;
        i += 1;
        if dl >= 512 {
            continue;
        }
        let decl = &css[a..a + dl];
        let Some(colon) = chr(decl, b':') else {
            continue;
        };
        let mut prop = &decl[..colon];
        while let [b' ' | b'\t' | b'\n' | b'\r', rest @ ..] = prop {
            prop = rest;
        }
        while let [rest @ .., b' ' | b'\t'] = prop {
            prop = rest;
        }
        let prop = lower_all(prop);
        let prop = prop.as_slice();
        let mut val = &decl[colon + 1..];
        while let [b' ' | b'\t' | b'\n' | b'\r', rest @ ..] = val {
            val = rest;
        }
        if let Some(k) = find_ic(val, b"!important") {
            val = &val[..k];
        }
        while let [rest @ .., b' ' | b'\t' | b'\r' | b'\n'] = val {
            val = rest;
        }
        // no url(), expression() or @import: the whole declaration is ignored
        if has_ic(val, b"url(") || has_ic(val, b"expression") || has_ic(val, b"javascript") {
            continue;
        }
        let base = if is.fs > 0.0 { is.fs } else { 14.0 };
        let len = |v: &[u8]| parse_len(v, base);
        let hides = (prop == b"display" && eq_ic(val, b"none"))
            || (prop == b"visibility" && eq_ic(val, b"hidden"))
            || (prop == b"mso-hide" && eq_ic(val, b"all"))
            || (prop == b"opacity"
                && strtod(val).0 <= 0.01
                && val.first().is_some_and(|&c| is_digit(c)));
        if hides {
            bs.hidden = true;
        } else if prop == b"overflow" && eq_ic(val, b"hidden") {
            overflow_hidden = true;
        } else if (prop == b"max-height" || prop == b"height")
            && len(val).is_some_and(|(px, _)| px <= 0.0)
        {
            zero_height = true;
        } else if let Some(c) = (prop == b"color").then(|| parse_color(val)).flatten() {
            is.color = c;
            is.has_color = true;
        } else if let Some(c) = (prop == b"background-color" || prop == b"background")
            .then(|| parse_color(val))
            .flatten()
        {
            bs.bg = c;
            bs.has_bg = true;
            is.bg = c;
            is.has_bg = false;
        } else if prop == b"font-weight" {
            is.b = sw(val, b"bold") || sw(val, b"bolder") || hnum(val, 0, 1000, 400) >= 600;
        } else if prop == b"font-style" {
            is.i = sw(val, b"italic") || sw(val, b"oblique");
        } else if prop == b"text-decoration" || prop == b"text-decoration-line" {
            if has_ic(val, b"none") {
                is.u = false;
                is.st = false;
            }
            if has_ic(val, b"underline") {
                is.u = true;
            }
            if has_ic(val, b"line-through") {
                is.st = true;
            }
        } else if prop == b"font-family" {
            is.mono = has_ic(val, b"mono") || has_ic(val, b"courier") || has_ic(val, b"consolas");
        } else if prop == b"font-size" {
            if let Some((px, pct)) = len(val) {
                let v = if pct { base * px / 100.0 } else { px };
                if v <= 0.5 {
                    bs.hidden = true;
                } else {
                    is.fs = v.clamp(8.0, 48.0);
                }
            } else if eq_ic(val, b"small") {
                is.fs = 12.0;
            } else if eq_ic(val, b"x-small") {
                is.fs = 10.0;
            } else if eq_ic(val, b"large") {
                is.fs = 18.0;
            } else if eq_ic(val, b"x-large") {
                is.fs = 24.0;
            } else if eq_ic(val, b"xx-large") {
                is.fs = 32.0;
            } else if eq_ic(val, b"medium") {
                is.fs = 15.0;
            }
        } else if prop == b"font" {
            // shorthand: only weight and size
            if has_ic(val, b"bold") {
                is.b = true;
            }
            if has_ic(val, b"italic") {
                is.i = true;
            }
            let mut p = 0;
            while p < val.len() {
                if is_digit(val[p])
                    && let Some((px, false)) = len(&val[p..])
                    && px >= 6.0
                {
                    is.fs = px.min(48.0);
                    break;
                }
                while p < val.len() && val[p] != b' ' {
                    p += 1;
                }
                while p < val.len() && val[p] == b' ' {
                    p += 1;
                }
            }
        } else if prop == b"text-align" {
            bs.align = if sw(val, b"center") {
                b'c'
            } else if sw(val, b"right") || sw(val, b"end") {
                b'e'
            } else if sw(val, b"justify") {
                b'j'
            } else {
                b's'
            };
        } else if prop == b"white-space" {
            is.pre = sw(val, b"pre");
        } else if prop.starts_with(b"padding") {
            let mut v = [0f64; 4];
            let mut k = 0;
            let mut p = 0;
            while k < 4 && p < val.len() {
                let Some((x, pct)) = len(&val[p..]) else {
                    break;
                };
                v[k] = if pct { 0.0 } else { x };
                k += 1;
                while p < val.len() && val[p] != b' ' {
                    p += 1;
                }
                while p < val.len() && val[p] == b' ' {
                    p += 1;
                }
            }
            if k == 0 {
                continue;
            }
            match prop {
                b"padding" => {
                    let t = v[0];
                    let r = if k > 1 { v[1] } else { t };
                    let b = if k > 2 { v[2] } else { t };
                    let l = if k > 3 { v[3] } else { r };
                    bs.pad = [t, r, b, l];
                }
                b"padding-top" => bs.pad[0] = v[0],
                b"padding-right" => bs.pad[1] = v[0],
                b"padding-bottom" => bs.pad[2] = v[0],
                b"padding-left" => bs.pad[3] = v[0],
                _ => {}
            }
            bs.has_pad = true;
        } else if prop.starts_with(b"margin") {
            if prop == b"margin" {
                let tmp = &val[..val.len().min(127)];
                let tok: Vec<&[u8]> = tmp
                    .split(|&c| c == b' ')
                    .filter(|t| !t.is_empty())
                    .take(4)
                    .collect();
                if tok.is_empty() {
                    continue;
                }
                let k = tok.len();
                let top = tok[0];
                let right = if k > 1 { tok[1] } else { top };
                let bot = if k > 2 { tok[2] } else { top };
                let left = if k > 3 { tok[3] } else { right };
                if eq_ic(right, b"auto") && eq_ic(left, b"auto") {
                    bs.self_center = true;
                }
                if let Some((px, false)) = len(top)
                    && px > 0.0
                {
                    bs.mt = px;
                }
                if let Some((px, false)) = len(bot)
                    && px > 0.0
                {
                    bs.mb = px;
                }
            } else if let Some((px, false)) = (prop == b"margin-top")
                .then(|| len(val))
                .flatten()
                .filter(|&(px, _)| px > 0.0)
            {
                bs.mt = px;
            } else if let Some((px, false)) = (prop == b"margin-bottom")
                .then(|| len(val))
                .flatten()
                .filter(|&(px, _)| px > 0.0)
            {
                bs.mb = px;
            }
        } else if let Some((px, pct)) = (prop == b"width")
            .then(|| len(val))
            .flatten()
            .filter(|&(px, _)| px > 0.0)
        {
            bs.w = px;
            bs.wpct = pct;
            bs.has_w = true;
        } else if let Some((px, _)) = (prop == b"max-width")
            .then(|| len(val))
            .flatten()
            .filter(|&(px, pct)| !pct && px > 0.0)
        {
            bs.mw = px;
            bs.has_mw = true;
        } else if prop == b"border"
            || prop == b"border-left"
            || prop == b"border-top"
            || prop == b"border-bottom"
        {
            let mut w = 0.0;
            let mut col = 0x999999ffu32;
            let none = has_ic(val, b"none");
            let mut p = 0;
            while p < val.len() {
                let c = val[p];
                if is_digit(c) {
                    if let Some((x, _)) = len(&val[p..]) {
                        w = x;
                    }
                } else if (c == b'#' || starts_ic(&val[p..], b"rgb") || is_alpha(c))
                    && let Some(x) = parse_color(&val[p..])
                {
                    col = x;
                }
                while p < val.len() && val[p] != b' ' {
                    if val[p] == b'(' {
                        while p < val.len() && val[p] != b')' {
                            p += 1;
                        }
                    }
                    if p < val.len() {
                        p += 1;
                    }
                }
                while p < val.len() && val[p] == b' ' {
                    p += 1;
                }
            }
            if none || w <= 0.0 {
                continue;
            }
            if w > 8.0 {
                w = 8.0;
            }
            if prop == b"border-left" {
                bs.bl = w;
                bs.blc = col;
                bs.has_bl = true;
            } else if prop == b"border" {
                bs.bw = w;
                bs.bc = col;
                bs.has_border = true;
            }
        } else if let Some((px, _)) = (prop == b"border-radius")
            .then(|| len(val))
            .flatten()
            .filter(|&(_, pct)| !pct)
        {
            bs.radius = px.min(40.0);
        } else if prop == b"vertical-align" {
            bs.valign = if sw(val, b"middle") {
                b'm'
            } else if sw(val, b"bottom") {
                b'b'
            } else {
                b't'
            };
        }
    }
    if overflow_hidden && zero_height {
        bs.hidden = true;
    }
}

// ---- tags ----

#[derive(PartialEq, Eq, Clone, Copy)]
enum Kind {
    Inline,
    Block,
    Drop,
    Raw,
    Ignore,
}

const T_VOID: &[&[u8]] = &[
    b"br",
    b"hr",
    b"img",
    b"meta",
    b"link",
    b"input",
    b"wbr",
    b"col",
    b"area",
    b"base",
    b"source",
    b"track",
    b"param",
    b"embed",
    b"keygen",
    b"basefont",
    b"image",
];
const T_RAW: &[&[u8]] = &[
    b"script",
    b"style",
    b"textarea",
    b"title",
    b"xmp",
    b"noembed",
    b"noframes",
    b"plaintext",
    b"iframe",
];
const T_DROP: &[&[u8]] = &[
    b"head",
    b"object",
    b"applet",
    b"frame",
    b"frameset",
    b"noscript",
    b"template",
    b"svg",
    b"math",
    b"audio",
    b"video",
    b"canvas",
    b"map",
    b"select",
    b"button",
    b"option",
    b"optgroup",
    b"datalist",
    b"dialog",
    b"portal",
    b"picture",
];
const T_BLOCK: &[&[u8]] = &[
    b"div",
    b"p",
    b"h1",
    b"h2",
    b"h3",
    b"h4",
    b"h5",
    b"h6",
    b"blockquote",
    b"pre",
    b"center",
    b"ul",
    b"ol",
    b"li",
    b"dl",
    b"dt",
    b"dd",
    b"address",
    b"article",
    b"section",
    b"header",
    b"footer",
    b"nav",
    b"main",
    b"aside",
    b"figure",
    b"figcaption",
    b"table",
    b"tr",
    b"td",
    b"th",
    b"caption",
    b"body",
    b"html",
    b"form",
    b"fieldset",
    b"legend",
    b"details",
    b"summary",
    b"hgroup",
    b"menu",
    b"listing",
];
const T_IGNORE: &[&[u8]] = &[b"thead", b"tbody", b"tfoot", b"colgroup"];

fn tag_is(t: &[u8], list: &[&[u8]]) -> bool {
    list.contains(&t)
}

fn kind_of(t: &[u8]) -> Kind {
    if tag_is(t, T_RAW) {
        Kind::Raw
    } else if tag_is(t, T_DROP) {
        Kind::Drop
    } else if tag_is(t, T_IGNORE) {
        Kind::Ignore
    } else if tag_is(t, T_BLOCK) {
        Kind::Block
    } else {
        Kind::Inline
    }
}

// ---- attributes ----

struct Attr {
    name: Vec<u8>,
    dec: Vec<u8>, // decoded value, as a C string (up to the first NUL)
}

fn attr<'a>(a: &'a [Attr], name: &[u8]) -> Option<&'a [u8]> {
    a.iter().find(|x| x.name == name).map(|x| x.dec.as_slice())
}

/// attribute value with entities decoded
fn decode_attr(v: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'&' {
            let (cp, used) = entity(v, i);
            if cp != 0 && used != 0 {
                put_cp(&mut b, cp);
                i += used;
                continue;
            }
        }
        b.push(v[i]);
        i += 1;
    }
    z(&b).to_vec()
}

// ---- the builder ----

#[derive(Clone)]
struct Open {
    tag: Vec<u8>,
    is: IStyle,
    frame: i32, // 1 opened a frame, 0 none, -1 skipped (hidden or dropped)
    list_n: i32,
}

#[derive(Clone, Copy)]
struct Frame {
    kids: i32,
    kind: u8,
}

struct Link {
    href: Vec<u8>,
    text: Vec<u8>,
    warn: bool,
}

struct DataRef {
    off: Vec<u8>, // the base64 payload (a copy of the decoded attribute's tail)
}

struct H {
    out: JBuf,
    open: Vec<Open>,
    fr: Vec<Frame>,
    cur: IStyle,
    run: Vec<u8>,
    run_st: IStyle,
    space: bool,
    line_start: bool,
    links: Vec<Link>,
    cur_link: i32,
    remote: Vec<Vec<u8>>,
    st: HtmlStats,
    root_bg: u32,
    has_root_bg: bool,
    want_data: bool,
    data: Vec<(DataRef, [u8; 16])>,
    colors: bool,
}

fn tag16(t: &[u8]) -> Vec<u8> {
    t[..t.len().min(15)].to_vec()
}

impl H {
    fn frame_sep(&mut self) {
        let f = self.fr.last_mut().expect("root frame");
        let had = f.kids != 0;
        f.kids += 1;
        if had {
            self.out.raw(b",");
        }
    }

    fn color_json(&mut self, k: &str, c: u32) {
        if c & 0xff == 0xff {
            self.out.fmt(format_args!(",\"{}\":\"#{:06x}\"", k, c >> 8));
        } else {
            self.out.fmt(format_args!(",\"{}\":\"#{:08x}\"", k, c));
        }
    }

    fn flush_run(&mut self) {
        if self.run.is_empty() {
            return;
        }
        let over = self.st.nodes >= HTML_MAX_NODES;
        self.st.nodes += 1;
        if over {
            self.st.truncated = 1;
            self.run.clear();
            return;
        }
        self.frame_sep();
        self.out.raw(b"{\"t\":\"x\",\"x\":");
        let run = std::mem::take(&mut self.run);
        self.out.strn(&run);
        let s = self.run_st;
        if s.b || s.i || s.u || s.st || s.mono || s.has_color || s.fs > 0.0 || s.has_bg {
            self.out.raw(b",\"s\":{\"_\":0");
            if s.b {
                self.out.raw(b",\"b\":1");
            }
            if s.i {
                self.out.raw(b",\"i\":1");
            }
            if s.u {
                self.out.raw(b",\"u\":1");
            }
            if s.st {
                self.out.raw(b",\"st\":1");
            }
            if s.mono {
                self.out.raw(b",\"mono\":1");
            }
            if s.has_color {
                self.color_json("c", s.color);
                self.colors = true;
            }
            if s.has_bg {
                self.color_json("bg", s.bg);
            }
            if s.fs > 0.0 {
                self.out
                    .fmt(format_args!(",\"fs\":{}", (s.fs + 0.5) as i32));
            }
            self.out.raw(b"}");
        }
        if s.link >= 0 {
            self.out.fmt(format_args!(",\"l\":{}", s.link));
        }
        self.out.raw(b"}");
        self.run = run;
        self.run.clear();
    }

    fn block_break(&mut self) {
        self.flush_run();
        self.space = false;
        self.line_start = true;
    }

    fn emit_simple(&mut self, node: &[u8]) {
        let ok = self.st.nodes < HTML_MAX_NODES;
        self.st.nodes += 1;
        if ok {
            self.frame_sep();
            self.out.raw(node);
        }
    }

    fn add_text(&mut self, s: &[u8]) {
        // s: UTF-8, already decoded
        let mut i = 0;
        while i < s.len() {
            let cp = next_cp(s, &mut i);
            if invisible(cp) {
                continue;
            }
            if !self.cur.pre && matches!(cp, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c) {
                if !self.line_start {
                    self.space = true;
                }
                continue;
            }
            if self.cur.pre && cp == b'\r' as u32 {
                continue;
            }
            if self.cur.pre && cp == b'\n' as u32 {
                self.flush_run();
                self.emit_simple(b"{\"t\":\"br\"}");
                self.line_start = true;
                continue;
            }
            if cp < 0x20 && cp != b'\t' as u32 {
                continue;
            }
            if !self.run.is_empty() && !istyle_eq(&self.run_st, &self.cur) {
                self.flush_run();
            }
            if self.run.is_empty() {
                self.run_st = self.cur;
            }
            if self.space {
                self.run.push(b' ');
                if self.cur_link >= 0 {
                    self.links[self.cur_link as usize].text.push(b' ');
                }
            }
            self.space = false;
            self.line_start = false;
            let start = self.run.len();
            put_cp(&mut self.run, cp);
            if self.cur_link >= 0 {
                let l = &mut self.links[self.cur_link as usize];
                if l.text.len() < 600 {
                    l.text.extend_from_slice(&self.run[start..]);
                }
            }
        }
    }

    fn bstyle_json(&mut self, b: &BStyle) {
        let pre = self.cur.pre;
        let o = &mut self.out;
        o.raw(b",\"s\":{\"_\":0");
        if b.align != 0 {
            o.fmt(format_args!(",\"a\":\"{}\"", b.align as char));
        }
        if b.self_center {
            o.raw(b",\"ac\":1");
        }
        if b.has_bg {
            self.color_json("bg", b.bg);
            self.colors = true;
        }
        let o = &mut self.out;
        if b.has_pad {
            o.fmt(format_args!(
                ",\"p\":[{},{},{},{}]",
                b.pad[0] as i32, b.pad[1] as i32, b.pad[2] as i32, b.pad[3] as i32
            ));
        }
        if b.mt > 0.0 {
            o.fmt(format_args!(",\"mt\":{}", b.mt.min(60.0) as i32));
        }
        if b.mb > 0.0 {
            o.fmt(format_args!(",\"mb\":{}", b.mb.min(60.0) as i32));
        }
        if b.has_w {
            o.fmt(format_args!(
                ",\"{}\":{}",
                if b.wpct { "wp" } else { "w" },
                b.w.min(4000.0) as i32
            ));
        }
        if b.has_mw {
            o.fmt(format_args!(",\"mw\":{}", b.mw.min(4000.0) as i32));
        }
        if b.has_border {
            let w = (b.bw + 0.5) as i32;
            o.fmt(format_args!(",\"bw\":{}", if w != 0 { w } else { 1 }));
            self.color_json("bc", b.bc);
        }
        if b.has_bl {
            let w = (b.bl + 0.5) as i32;
            self.out
                .fmt(format_args!(",\"bl\":{}", if w != 0 { w } else { 1 }));
            self.color_json("blc", b.blc);
        }
        let o = &mut self.out;
        if b.radius > 0.0 {
            o.fmt(format_args!(",\"r\":{}", b.radius as i32));
        }
        if b.valign != 0 {
            o.fmt(format_args!(",\"va\":\"{}\"", b.valign as char));
        }
        if pre {
            o.raw(b",\"pre\":1");
        }
        o.raw(b"}");
    }

    fn open_frame(&mut self, t: &str, b: &BStyle, extra: Option<&str>) -> i32 {
        if self.fr.len() >= HTML_MAX_DEPTH {
            self.block_break();
            return 0;
        }
        self.block_break();
        let over = self.st.nodes >= HTML_MAX_NODES;
        self.st.nodes += 1;
        if over {
            self.st.truncated = 1;
            return 0;
        }
        self.frame_sep();
        self.out.fmt(format_args!("{{\"t\":\"{}\"", t));
        self.bstyle_json(b);
        if let Some(e) = extra {
            self.out.raw(e.as_bytes());
        }
        self.out.raw(b",\"c\":[");
        let kind = match t {
            "tbl" => b'T',
            "tr" => b'R',
            "td" => b'D',
            _ => b'B',
        };
        self.fr.push(Frame { kids: 0, kind });
        1
    }

    fn close_frame(&mut self) {
        self.block_break();
        if self.fr.len() <= 1 {
            return;
        }
        self.out.raw(b"]}");
        self.fr.pop();
    }

    fn end_link(&mut self) {
        if self.cur_link < 0 {
            return;
        }
        let l = &mut self.links[self.cur_link as usize];
        if link_warning(&l.text, &l.href).is_some() {
            l.warn = true;
            self.st.warn += 1;
        }
        self.cur_link = -1;
        self.cur.link = -1;
    }

    /// Closes open elements from the top down to index idx (inclusive).
    fn pop_to(&mut self, idx: usize) {
        while self.open.len() > idx {
            let o = self.open.pop().expect("non-empty");
            if o.tag == b"a" {
                self.end_link();
            }
            if o.frame != 0 {
                self.close_frame();
            } else if kind_of(&o.tag) == Kind::Block {
                self.block_break();
            }
            self.flush_run();
            self.cur = o.is; // the style from before it was opened
            if self.cur_link >= 0 && self.cur.link != self.cur_link {
                self.cur_link = self.cur.link;
            }
        }
    }

    fn find_open(&self, tag: &[u8], stop: Option<&[&[u8]]>) -> Option<usize> {
        for i in (0..self.open.len()).rev() {
            if self.open[i].tag == tag {
                return Some(i);
            }
            if let Some(stop) = stop
                && tag_is(&self.open[i].tag, stop)
            {
                return None;
            }
        }
        None
    }

    fn add_remote(&mut self, url: &[u8]) -> Option<usize> {
        if let Some(k) = self.remote.iter().position(|r| r == url) {
            return Some(k);
        }
        if self.remote.len() >= HTML_MAX_REMOTE {
            return None;
        }
        self.remote.push(url.to_vec());
        Some(self.remote.len() - 1)
    }

    fn do_img(&mut self, a: &[Attr], bs: &BStyle) {
        let Some(src) = attr(a, b"src") else { return };
        if src.is_empty() {
            return;
        }
        let s0 = src
            .iter()
            .position(|&c| !matches!(c, b' ' | b'\n' | b'\t'))
            .unwrap_or(src.len());
        let src = &src[s0..];
        let (mut w, mut hh, mut wpct) = (0.0f64, 0.0f64, false);
        let ws = attr(a, b"width");
        let hs = attr(a, b"height");
        if let Some((px, pct)) = ws.and_then(|v| parse_len(v, 14.0)) {
            w = px;
            wpct = pct;
        }
        if let Some((px, false)) = hs.and_then(|v| parse_len(v, 14.0)) {
            hh = px;
        }
        if bs.has_w {
            w = bs.w;
            wpct = bs.wpct;
        }
        if !wpct
            && ((ws.is_some() && w <= 2.0 && hs.is_some() && hh <= 2.0)
                || (ws.is_some() && w == 0.0)
                || (hs.is_some() && hh == 0.0))
        {
            self.st.trackers += 1;
            return;
        }
        let mut reference: Option<String> = None;
        if starts_ic(src, b"cid:") {
            self.st.cid += 1;
        } else if starts_ic(src, b"http://") || starts_ic(src, b"https://") {
            let Some(k) = self.add_remote(src) else {
                return;
            };
            reference = Some(format!("r:{}", k));
            self.st.remote += 1;
        } else if starts_ic(src, b"data:image/") {
            let Some(semi) = find(src, b";base64,") else {
                return;
            };
            if !self.want_data || self.data.len() >= HTML_MAX_DATA as usize {
                return;
            }
            let ty = &src[11..semi];
            if ty.len() >= 16 {
                return;
            }
            if !matches!(ty, b"png" | b"jpeg" | b"jpg" | b"gif" | b"bmp") {
                return;
            }
            let mut tb = [0u8; 16];
            tb[..ty.len()].copy_from_slice(ty);
            reference = Some(format!("d:{}", self.data.len()));
            self.data.push((
                DataRef {
                    off: src[semi + 8..].to_vec(),
                },
                tb,
            ));
            self.st.data += 1;
        } else {
            return; // relative paths, file:, javascript: …
        }
        self.flush_run();
        let over = self.st.nodes >= HTML_MAX_NODES;
        self.st.nodes += 1;
        if over {
            self.st.truncated = 1;
            return;
        }
        self.frame_sep();
        self.out.raw(b"{\"t\":\"img\",\"src\":");
        match &reference {
            Some(r) => self.out.str(r.as_bytes()),
            None => {
                let mut cid = b"cid:".to_vec();
                cid.extend_from_slice(&src[4..]);
                cid.truncate(199);
                self.out.str(&cid);
            }
        }
        if w > 0.0 {
            self.out.fmt(format_args!(
                ",\"{}\":{}",
                if wpct { "wp" } else { "w" },
                w.min(4000.0) as i32
            ));
        }
        if hh > 0.0 {
            self.out
                .fmt(format_args!(",\"h\":{}", hh.min(4000.0) as i32));
        }
        if let Some(alt) = attr(a, b"alt")
            && !alt.is_empty()
        {
            self.out.raw(b",\"alt\":");
            self.out.str(&alt[..alt.len().min(199)]);
        }
        if self.cur.link >= 0 {
            self.out.fmt(format_args!(",\"l\":{}", self.cur.link));
        }
        if bs.align != 0 {
            self.out
                .fmt(format_args!(",\"a\":\"{}\"", bs.align as char));
        }
        self.out.raw(b"}");
        self.line_start = false;
    }

    fn open_tag(&mut self, tag: &[u8], a: &[Attr], selfclose: bool) {
        let kind = kind_of(tag);
        if kind == Kind::Ignore {
            return;
        }
        let is_void = tag_is(tag, T_VOID);
        // implicit closes
        if kind == Kind::Block
            && let Some(p) = self.find_open(b"p", Some(STOP_CELL))
            && tag != b"td"
            && tag != b"th"
            && tag != b"tr"
        {
            self.pop_to(p);
        }
        if tag == b"li"
            && let Some(k) = self.find_open(b"li", Some(STOP_LIST))
        {
            self.pop_to(k);
        }
        if (tag == b"dt" || tag == b"dd")
            && let Some(k) = self
                .find_open(b"dt", Some(STOP_LIST))
                .or_else(|| self.find_open(b"dd", Some(STOP_LIST)))
        {
            self.pop_to(k);
        }
        if (tag == b"td" || tag == b"th")
            && let Some(k) = self
                .find_open(b"td", Some(STOP_TABLE))
                .or_else(|| self.find_open(b"th", Some(STOP_TABLE)))
        {
            self.pop_to(k);
        }
        if tag == b"tr"
            && let Some(k) = self.find_open(b"tr", Some(STOP_TABLE))
        {
            self.pop_to(k);
        }
        if tag == b"a"
            && let Some(k) = self.find_open(b"a", Some(STOP_CELL))
        {
            self.pop_to(k);
        }
        // stray td inside a table without tr: open a tr
        if (tag == b"td" || tag == b"th")
            && self.fr.last().is_some_and(|f| f.kind == b'T')
            && self.open.len() < MAX_OPEN
        {
            self.open_tag(b"tr", &[], false);
        }

        let saved = self.cur;
        let mut bs = BStyle::default();
        legacy_attrs(a, &mut self.cur, &mut bs, tag);
        let style = attr(a, b"style");
        if let Some(st) = style {
            apply_css(st, &mut self.cur, &mut bs);
        }

        if bs.hidden || kind == Kind::Drop {
            self.cur = saved;
            // skipped up to its close: the main loop does it
            if !is_void && !selfclose && self.open.len() < MAX_OPEN {
                self.open.push(Open {
                    tag: tag16(tag),
                    is: saved,
                    frame: -1,
                    list_n: 0,
                });
            }
            return;
        }

        // per-tag styles
        let cur = &mut self.cur;
        match tag {
            b"b" | b"strong" | b"th" => cur.b = true,
            b"i" | b"em" | b"cite" | b"var" | b"dfn" => cur.i = true,
            b"u" | b"ins" => cur.u = true,
            b"s" | b"strike" | b"del" => cur.st = true,
            b"code" | b"tt" | b"kbd" | b"samp" => cur.mono = true,
            b"pre" | b"listing" => {
                cur.mono = true;
                cur.pre = true;
            }
            b"small" | b"sub" | b"sup" => {
                cur.fs = (if saved.fs > 0.0 { saved.fs } else { 14.0 }) * 0.85
            }
            b"big" => cur.fs = (if saved.fs > 0.0 { saved.fs } else { 14.0 }) * 1.2,
            [b'h', d @ b'1'..=b'6'] => {
                const HS: [f64; 6] = [24.0, 20.0, 17.0, 15.0, 14.0, 13.0];
                cur.b = true;
                if style.is_none_or(|st| !has_ic(st, b"font-size")) {
                    cur.fs = HS[(d - b'1') as usize];
                }
                if bs.mt <= 0.0 {
                    bs.mt = 10.0;
                }
                if bs.mb <= 0.0 {
                    bs.mb = 6.0;
                }
            }
            b"p" => {
                let margin = style.is_some_and(|st| has_ic(st, b"margin"));
                if bs.mt <= 0.0 && !margin {
                    bs.mt = 6.0;
                }
                if bs.mb <= 0.0 && !margin {
                    bs.mb = 6.0;
                }
            }
            b"blockquote" => {
                if !bs.has_bl {
                    bs.bl = 3.0;
                    bs.blc = 0x88888888;
                    bs.has_bl = true;
                }
                if !bs.has_pad {
                    bs.pad[3] = 10.0;
                    bs.has_pad = true;
                }
                if bs.mt <= 0.0 {
                    bs.mt = 4.0;
                }
                if bs.mb <= 0.0 {
                    bs.mb = 4.0;
                }
            }
            b"center" => bs.align = b'c',
            b"ul" | b"ol" | b"menu" | b"dd" if !bs.has_pad => {
                bs.pad[3] = 22.0;
                bs.has_pad = true;
            }
            _ => {}
        }

        if tag == b"body" || tag == b"html" {
            if bs.has_bg && !self.has_root_bg {
                self.root_bg = bs.bg;
                self.has_root_bg = true;
            }
            bs.has_bg = false;
        }

        if tag == b"br" {
            self.flush_run();
            self.emit_simple(b"{\"t\":\"br\"}");
            self.space = false;
            self.line_start = true;
            self.cur = saved;
            return;
        }
        if tag == b"hr" {
            self.block_break();
            self.emit_simple(b"{\"t\":\"hr\"}");
            self.cur = saved;
            return;
        }
        if tag == b"img" || tag == b"image" {
            self.do_img(a, &bs);
            self.cur = saved;
            return;
        }
        if is_void {
            self.cur = saved;
            return;
        }

        if tag == b"a" {
            self.cur.link = -1;
            if let Some(href) = attr(a, b"href")
                && href_ok(href)
                && self.links.len() < HTML_MAX_LINKS
            {
                // canonical form: no blanks or control characters anywhere (as browsers do)
                let href: Vec<u8> = href
                    .iter()
                    .copied()
                    .filter(|&c| c > 0x20 && c != 0x7f)
                    .collect();
                let k = self.links.len() as i32;
                self.links.push(Link {
                    href,
                    text: Vec::new(),
                    warn: false,
                });
                self.cur.link = k;
                self.cur_link = k;
            }
        }

        if self.open.len() >= MAX_OPEN {
            self.cur = saved;
            return;
        }
        self.open.push(Open {
            tag: tag16(tag),
            is: saved,
            frame: 0,
            list_n: 0,
        });
        let oi = self.open.len() - 1;
        if kind == Kind::Block {
            let mut t = "blk";
            let mut extra = String::new();
            if tag == b"table" {
                t = "tbl";
                if let Some(cp) = attr(a, b"cellpadding") {
                    let v = hnum(cp, 0, 30, 0);
                    if v > 0 {
                        extra = format!(",\"cp\":{}", v);
                    }
                }
            } else if tag == b"tr" {
                t = "tr";
            } else if tag == b"td" || tag == b"th" {
                t = "td";
                if let Some(cs) = attr(a, b"colspan") {
                    let v = hnum(cs, 1, 50, 1);
                    if v > 1 {
                        extra = format!(",\"cs\":{}", v);
                    }
                }
            } else if tag == b"li" {
                let lst = self.find_open(b"ol", Some(STOP_LIST));
                let ul = self.find_open(b"ul", Some(STOP_LIST));
                let (li, ui) = (lst.map_or(-1, |x| x as i64), ul.map_or(-1, |x| x as i64));
                if li > ui {
                    let o = &mut self.open[li as usize];
                    o.list_n += 1;
                    extra = format!(",\"mk\":\"{}.\"", o.list_n);
                } else {
                    extra = ",\"mk\":\"•\"".to_string();
                }
            }
            let f = self.open_frame(t, &bs, if extra.is_empty() { None } else { Some(&extra) });
            self.open[oi].frame = f;
        }
    }

    fn close_tag(&mut self, tag: &[u8]) {
        if kind_of(tag) == Kind::Ignore {
            return;
        }
        if tag == b"td" || tag == b"th" || tag == b"tr" {
            if let Some(k) = self.find_open(tag, Some(STOP_TABLE)) {
                self.pop_to(k);
            }
            return;
        }
        if tag == b"br" {
            self.open_tag(b"br", &[], true); // </br> = <br>
            return;
        }
        let stop = if tag == b"table" {
            None
        } else {
            Some(STOP_CELL)
        };
        if let Some(k) = self.find_open(tag, stop) {
            self.pop_to(k);
        }
    }
}

fn istyle_eq(a: &IStyle, b: &IStyle) -> bool {
    a.b == b.b
        && a.i == b.i
        && a.u == b.u
        && a.st == b.st
        && a.mono == b.mono
        && a.has_color == b.has_color
        && (!a.has_color || a.color == b.color)
        && a.fs == b.fs
        && a.link == b.link
        && a.has_bg == b.has_bg
        && (!a.has_bg || a.bg == b.bg)
}

const STOP_CELL: &[&[u8]] = &[b"td", b"th", b"table"];
const STOP_TABLE: &[&[u8]] = &[b"table"];
const STOP_LIST: &[&[u8]] = &[b"ul", b"ol", b"table", b"td", b"th"];

/// Legacy presentational attributes (bgcolor, align, width…)
fn legacy_attrs(a: &[Attr], is: &mut IStyle, bs: &mut BStyle, tag: &[u8]) {
    if let Some(c) = attr(a, b"bgcolor").and_then(parse_color) {
        bs.bg = c;
        bs.has_bg = true;
    }
    if let Some(v) = attr(a, b"align") {
        if tag == b"table" || tag == b"img" {
            if eq_ic(v, b"center") {
                bs.self_center = true;
            }
        } else {
            bs.align = if eq_ic(v, b"center") {
                b'c'
            } else if eq_ic(v, b"right") {
                b'e'
            } else if eq_ic(v, b"justify") {
                b'j'
            } else {
                b's'
            };
        }
    }
    if let Some(v) = attr(a, b"valign") {
        bs.valign = if eq_ic(v, b"middle") {
            b'm'
        } else if eq_ic(v, b"bottom") {
            b'b'
        } else {
            b't'
        };
    }
    if tag != b"img"
        && let Some((px, pct)) = attr(a, b"width")
            .and_then(|v| parse_len(v, 14.0))
            .filter(|&(px, _)| px > 0.0)
    {
        bs.w = px;
        bs.wpct = pct;
        bs.has_w = true;
    }
    if tag == b"table"
        && let Some(v) = attr(a, b"border")
        && hnum(v, 0, 4, 0) > 0
    {
        bs.bw = hnum(v, 0, 4, 0) as f64;
        bs.bc = 0x999999ff;
        bs.has_border = true;
    }
    if tag == b"font" {
        if let Some(c) = attr(a, b"color").and_then(parse_color) {
            is.color = c;
            is.has_color = true;
        }
        if let Some(v) = attr(a, b"size") {
            const FSZ: [i32; 8] = [10, 10, 13, 15, 18, 24, 32, 44];
            let mut k = hnum(v, -10, 10, 3);
            if v.first() == Some(&b'+') {
                k = 3 + hnum(&v[1..], 0, 10, 0);
            } else if v.first() == Some(&b'-') {
                k = 3 - hnum(&v[1..], 0, 10, 0);
            }
            is.fs = FSZ[k.clamp(1, 7) as usize] as f64;
        }
        if let Some(v) = attr(a, b"face") {
            is.mono = has_ic(v, b"mono") || has_ic(v, b"courier");
        }
    }
    if tag == b"body"
        && let Some(c) = attr(a, b"text").and_then(parse_color)
    {
        is.color = c;
        is.has_color = true;
    }
    if attr(a, b"hidden").is_some() {
        bs.hidden = true;
    }
}

// ---- links ----

fn is_ip_host(h: &[u8]) -> bool {
    if h.first() == Some(&b'[') {
        return true;
    }
    let dots = h.iter().filter(|&&c| c == b'.').count();
    let digits = h.iter().all(|&c| c == b'.' || is_digit(c));
    digits && dots == 3
}

/// Domain (lowercase host, without "www.") of an http(s) URL; "" if none.
fn url_host(url: &[u8], cap: usize) -> Vec<u8> {
    if cap == 0 {
        return Vec::new();
    }
    let url = z(url);
    let s = url.iter().position(|&c| c != b' ').unwrap_or(url.len());
    let mut p = &url[s..];
    match find(p, b"://") {
        Some(k) if k < 10 => p = &p[k + 3..],
        _ => {
            if !starts_ic(p, b"www.") {
                return Vec::new();
            }
        }
    }
    let mut end = cspn(p, b"/?#");
    if let Some(at) = rchr(&p[..end], b'@') {
        p = &p[at + 1..];
        end -= at + 1;
    }
    if p.first() != Some(&b'[')
        && let Some(c) = rchr(&p[..end], b':')
    {
        end = c;
    }
    let n = end.min(cap - 1);
    let mut out: Vec<u8> = p[..n].iter().map(|&c| lower(c)).collect();
    while out.last() == Some(&b'.') {
        out.pop();
    }
    if out.len() >= 4 && out.starts_with(b"www.") {
        out.drain(..4);
    }
    out
}

fn host_match(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.len(), b.len());
    if la > lb && a[la - lb - 1] == b'.' && &a[la - lb..] == b {
        return true;
    }
    if lb > la && b[lb - la - 1] == b'.' && &b[lb - la..] == a {
        return true;
    }
    false
}

/// Does the text look like a web address? Returns the host.
fn text_as_url(t: &[u8], cap: usize) -> Option<Vec<u8>> {
    let t = z(t);
    let s = t.iter().position(|&c| c != b' ').unwrap_or(t.len());
    let mut buf = t[s..][..(t.len() - s).min(599)].to_vec();
    while matches!(buf.last(), Some(b' ' | b'.' | b'/')) {
        buf.pop();
    }
    let n = buf.len();
    if n == 0 || chr(&buf, b' ').is_some() {
        return None;
    }
    if starts_ic(&buf, b"http://") || starts_ic(&buf, b"https://") || starts_ic(&buf, b"www.") {
        let h = url_host(&buf, cap);
        return if h.is_empty() { None } else { Some(h) };
    }
    // domain.tld[/...]
    let hl = chr(&buf, b'/').unwrap_or(n);
    if hl >= cap || hl < 4 {
        return None;
    }
    let mut dot = false;
    for &c in &buf[..hl] {
        if c == b'.' {
            dot = true;
        } else if !is_alnum(c) && c != b'-' && c < 0x80 {
            return None;
        }
    }
    if !dot {
        return None;
    }
    // the last label must be letters (a TLD), not "3.5"
    let ld = rchr(&buf, b'.')?;
    if ld >= hl || !buf.get(ld + 1).is_some_and(|&c| is_alpha(c)) {
        return None;
    }
    let mut out: Vec<u8> = buf[..hl].iter().map(|&c| lower(c)).collect();
    if out.starts_with(b"www.") {
        out.drain(..4);
    }
    Some(out)
}

/// Does a link's visible text mislead about its destination?
fn link_warning(text: &[u8], href: &[u8]) -> Option<&'static CStr> {
    let (text, href) = (z(text), z(href));
    if starts_ic(href, b"mailto:") {
        // text shaped like an email address different from the mailto one
        let tt = &text[..text.len().min(299)];
        let s = tt.iter().position(|&c| c != b' ').unwrap_or(tt.len());
        let mut a = &tt[s..];
        while a.last() == Some(&b' ') {
            a = &a[..a.len() - 1];
        }
        if chr(a, b'@').is_some() && chr(a, b' ').is_none() {
            let m = &href[7..];
            let ml = cspn(m, b"?");
            if a.len() != ml || !eq_ic(a, &m[..ml]) {
                return Some(c"deceptive");
            }
        }
        return None;
    }
    let hh = url_host(href, 256);
    if hh.is_empty() {
        return None;
    }
    // user@ in the authority: "https://bank.com@other.net"
    if let Some(k) = find(href, b"://") {
        let p = &href[k + 3..];
        let e = cspn(p, b"/?#");
        if chr(&p[..e], b'@').is_some() {
            return Some(c"userinfo");
        }
    }
    let th = text_as_url(text, 256);
    let looks = th.is_some();
    let th = th.unwrap_or_default();
    if is_ip_host(&hh) && looks {
        return Some(c"ip");
    }
    if looks && !host_match(&th, &hh) {
        return Some(c"deceptive");
    }
    if looks && find(&hh, b"xn--").is_some() && find(&th, b"xn--").is_none() {
        return Some(c"punycode");
    }
    None
}

fn href_ok(v: &[u8]) -> bool {
    // no whitespace or control characters inside the scheme ("java\tscript:")
    let mut sch = Vec::new();
    for &c in v {
        if sch.len() + 1 >= 16 {
            break;
        }
        if c <= 0x20 {
            continue;
        }
        if c == b':' {
            break;
        }
        sch.push(lower(c));
    }
    if chr(v, b':').is_none() {
        return false;
    }
    sch == b"http" || sch == b"https" || sch == b"mailto"
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn html_url_host(url: *const c_char, out: *mut c_char, cap: usize) {
    let o = unsafe { crate::c::out(out, cap) };
    if o.is_empty() {
        return;
    }
    o[0] = 0;
    if url.is_null() {
        return;
    }
    put(o, &url_host(unsafe { cs(url) }, cap));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn html_link_warning(
    text: *const c_char,
    href: *const c_char,
) -> *const c_char {
    if href.is_null() {
        return std::ptr::null();
    }
    link_warning(unsafe { cs(text) }, unsafe { cs(href) }).map_or(std::ptr::null(), |w| w.as_ptr())
}

// ---- tokenizer ----

fn name_char(c: u8) -> bool {
    is_alnum(c) || c == b'-' || c == b'_' || c == b':'
}

fn sanitize(s: &[u8], want_data: bool) -> H {
    let mut h = H {
        out: JBuf::new(),
        open: Vec::new(),
        fr: vec![Frame {
            kids: 0,
            kind: b'r',
        }],
        cur: IStyle {
            link: -1,
            ..IStyle::default()
        },
        run: Vec::new(),
        run_st: IStyle::default(),
        space: false,
        line_start: true,
        links: Vec::new(),
        cur_link: -1,
        remote: Vec::new(),
        st: HtmlStats::default(),
        root_bg: 0,
        has_root_bg: false,
        want_data,
        data: Vec::new(),
        colors: false,
    };
    let s = if s.len() > HTML_MAX_INPUT {
        h.st.truncated = 1;
        &s[..HTML_MAX_INPUT]
    } else {
        s
    };
    let len = s.len();
    h.out.raw(b"{\"v\":1,\"blocks\":[");
    let mut i = 0;
    while i < len {
        // inside something being skipped?
        let skipping = h.open.last().is_some_and(|o| o.frame == -1);
        if s[i] != b'<' {
            let a = i;
            while i < len && s[i] != b'<' {
                i += 1;
            }
            if skipping {
                continue;
            }
            // text: entities → UTF-8
            let run = &s[..i];
            let mut t = Vec::with_capacity(i - a);
            let mut k = a;
            while k < i {
                if s[k] == b'&' {
                    let (cp, used) = entity(run, k);
                    if cp != 0 && used != 0 {
                        put_cp(&mut t, cp);
                        k += used;
                        continue;
                    }
                    t.push(b'&');
                    k += 1;
                    continue;
                }
                let k0 = k;
                while k < i && s[k] != b'&' {
                    k += 1;
                }
                t.extend_from_slice(&s[k0..k]);
            }
            if !t.is_empty() {
                h.add_text(&t);
            }
            continue;
        }
        // comments, doctype, CDATA, instructions
        if i + 3 < len && &s[i..i + 4] == b"<!--" {
            i = s[i + 4..]
                .windows(3)
                .position(|w| w == b"-->")
                .map_or(len, |k| i + 4 + k + 3);
            continue;
        }
        if i + 1 < len && (s[i + 1] == b'!' || s[i + 1] == b'?') {
            i = chr(&s[i..], b'>').map_or(len, |k| i + k + 1);
            continue;
        }
        let closing = i + 1 < len && s[i + 1] == b'/';
        let mut k = i + 1 + closing as usize;
        if k >= len || !is_alpha(s[k]) {
            // stray "<": it is text
            if !skipping {
                h.add_text(b"<");
            }
            i += 1;
            continue;
        }
        let mut tag = Vec::with_capacity(15);
        while k < len && name_char(s[k]) {
            if tag.len() < 15 {
                tag.push(lower(s[k]));
            }
            k += 1;
        }
        // attributes
        let mut at: Vec<Attr> = Vec::new();
        let mut selfclose = false;
        while k < len && s[k] != b'>' {
            while k < len && (is_space(s[k]) || s[k] == b'/') {
                if s[k] == b'/' && k + 1 < len && s[k + 1] == b'>' {
                    selfclose = true;
                }
                k += 1;
            }
            if k >= len || s[k] == b'>' {
                break;
            }
            let an = k;
            while k < len && !is_space(s[k]) && s[k] != b'=' && s[k] != b'>' && s[k] != b'/' {
                k += 1;
            }
            let anl = k - an;
            while k < len && is_space(s[k]) {
                k += 1;
            }
            let mut v: Option<&[u8]> = None;
            if k < len && s[k] == b'=' {
                k += 1;
                while k < len && is_space(s[k]) {
                    k += 1;
                }
                if k < len && (s[k] == b'"' || s[k] == b'\'') {
                    let q = s[k];
                    k += 1;
                    let va = k;
                    while k < len && s[k] != q {
                        k += 1;
                    }
                    v = Some(&s[va..k]);
                    if k < len {
                        k += 1;
                    }
                } else {
                    let va = k;
                    while k < len && !is_space(s[k]) && s[k] != b'>' {
                        k += 1;
                    }
                    v = Some(&s[va..k]);
                }
            }
            if anl == 0 {
                if k < len && s[k] != b'>' {
                    k += 1;
                }
                continue;
            }
            if at.len() < MAX_ATTRS && anl < 24 && !closing && !skipping {
                let name = lower_all(&s[an..an + anl]);
                // no events (on*) or anything unused
                if name.starts_with(b"on") {
                    continue;
                }
                // a name with a NUL ends there, as a C string would
                let name = z(&name).to_vec();
                at.push(Attr {
                    name,
                    dec: v.map_or_else(Vec::new, decode_attr),
                });
            }
        }
        i = if k < len { k + 1 } else { len };
        if skipping {
            // only the close of what is being skipped matters (with same-name nesting)
            let top = h.open.len() - 1;
            if tag == h.open[top].tag {
                if closing {
                    if h.open[top].list_n > 0 {
                        h.open[top].list_n -= 1;
                    } else {
                        let o = h.open.pop().expect("non-empty");
                        h.cur = o.is;
                    }
                } else if !selfclose && !tag_is(&tag, T_VOID) {
                    h.open[top].list_n += 1;
                }
            } else if !closing && h.open[top].tag == b"head" && tag == b"body" {
                h.open.pop(); // unclosed <head>
                h.open_tag(&tag, &at, selfclose);
            }
            continue;
        }
        if closing {
            h.close_tag(&tag);
            continue;
        }
        if tag_is(&tag, T_RAW) {
            // skip up to </tag> without interpreting anything
            let tn = tag.len();
            let mut e = i;
            loop {
                let Some(lt) = chr(&s[e..], b'<') else {
                    e = len;
                    break;
                };
                e += lt;
                if e + 2 + tn <= len
                    && s[e + 1] == b'/'
                    && eq_ic(&s[e + 2..e + 2 + tn], &tag)
                    && (e + 2 + tn == len || !name_char(s[e + 2 + tn]))
                {
                    e = chr(&s[e..], b'>').map_or(len, |g| e + g + 1);
                    break;
                }
                e += 1;
            }
            i = e;
            continue;
        }
        h.open_tag(&tag, &at, selfclose);
    }
    h.pop_to(0);
    h.flush_run();
    while h.fr.len() > 1 {
        h.close_frame();
    }
    h.out.raw(b"]");
    if h.has_root_bg {
        h.color_json("bg", h.root_bg);
    }
    if h.colors || h.has_root_bg {
        h.out.raw(b",\"colors\":1");
    }
    h.out.raw(b",\"remote\":[");
    for r in 0..h.remote.len() {
        if r != 0 {
            h.out.raw(b",");
        }
        let u = std::mem::take(&mut h.remote[r]);
        h.out.str(&u);
    }
    h.out.raw(b"],\"links\":[");
    for l in 0..h.links.len() {
        if l != 0 {
            h.out.raw(b",");
        }
        let Link { href, text, warn } = std::mem::replace(
            &mut h.links[l],
            Link {
                href: Vec::new(),
                text: Vec::new(),
                warn: false,
            },
        );
        h.out.raw(b"{\"h\":");
        h.out.str(&href);
        if warn {
            h.out.raw(b",\"w\":");
            h.out
                .str(link_warning(&text, &href).map_or(&b""[..], |w| w.to_bytes()));
            h.out.raw(b",\"txt\":");
            h.out.str(&text);
        }
        h.out.raw(b"}");
    }
    let st = h.st;
    h.out.fmt(format_args!(
        "],\"stats\":{{\"remote\":{},\"trackers\":{},\"warn\":{},\"cid\":{},\"data\":{},\"nodes\":{},\"truncated\":{}}}}}",
        st.remote, st.trackers, st.warn, st.cid, st.data, st.nodes, st.truncated
    ));
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn html_sanitize(
    html: *const c_char,
    len: usize,
    olen: *mut usize,
    stats: *mut HtmlStats,
    data: *mut HtmlData,
    ndata: *mut c_int,
) -> *mut c_char {
    let mut h = sanitize(unsafe { bytes(html, len) }, !data.is_null());
    if let Some(st) = unsafe { stats.as_mut() } {
        *st = h.st;
    }
    // data: images are handed over as malloc'd copies of their base64 payload
    if !data.is_null() && !ndata.is_null() {
        for (k, (d, ty)) in h.data.iter().enumerate() {
            let slot = unsafe { &mut *data.add(k) };
            let p = malloc_str(&d.off);
            slot.p = p;
            slot.n = if p.is_null() { 0 } else { d.off.len() };
            putc(&mut slot.type_, z(ty));
        }
        unsafe { *ndata = h.data.len() as c_int };
    }
    if h.out.oom != 0 {
        h.out.free();
        set_len(olen, 0);
        return std::ptr::null_mut();
    }
    set_len(olen, h.out.n);
    h.out.p
}

fn to_text(s: &[u8]) -> Vec<u8> {
    let len = s.len();
    let mut b: Vec<u8> = Vec::with_capacity(len / 2);
    let mut space = false;
    let mut skip: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < len {
        if s[i] == b'<' {
            let mut k = i + 1;
            let closing = k < len && s[k] == b'/';
            if closing {
                k += 1;
            }
            let mut tag = Vec::with_capacity(15);
            while k < len && name_char(s[k]) {
                if tag.len() < 15 {
                    tag.push(lower(s[k]));
                }
                k += 1;
            }
            if i + 3 < len && &s[i..i + 4] == b"<!--" {
                let mut e = i + 4;
                while e + 2 < len && !(s[e] == b'-' && s[e + 1] == b'-' && s[e + 2] == b'>') {
                    e += 1;
                }
                i = if e + 3 < len { e + 3 } else { len };
                continue;
            }
            i = chr(&s[k..], b'>').map_or(len, |g| k + g + 1);
            if let Some(st) = &skip {
                if closing && tag == *st {
                    skip = None;
                }
                continue;
            }
            if !closing && (tag_is(&tag, T_RAW) || tag == b"head") {
                skip = Some(tag);
                continue;
            }
            let t = tag.as_slice();
            if matches!(t, b"br" | b"p" | b"div" | b"tr" | b"li" | b"table")
                || (t.first() == Some(&b'h') && t.get(1).is_some_and(|&c| is_digit(c)))
            {
                if b.last().is_some_and(|&c| c != b'\n') {
                    b.push(b'\n');
                }
                space = false;
            } else if t == b"td" || t == b"th" {
                space = true;
            }
            continue;
        }
        if skip.is_some() {
            i += 1;
            continue;
        }
        let cp = if s[i] == b'&' {
            let (cp, used) = entity(s, i);
            if cp != 0 && used != 0 {
                i += used;
                cp
            } else {
                i += 1;
                b'&' as u32
            }
        } else {
            next_cp(s, &mut i)
        };
        if invisible(cp) {
            continue;
        }
        if matches!(cp, 0x20 | 0x09 | 0x0a | 0x0d | 0xa0) {
            space = true;
            continue;
        }
        if cp < 0x20 {
            continue;
        }
        if space && b.last().is_some_and(|&c| c != b'\n') {
            b.push(b' ');
        }
        space = false;
        put_cp(&mut b, cp);
    }
    while matches!(b.last(), Some(b'\n' | b' ')) {
        b.pop();
    }
    b
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn html_to_text(
    html: *const c_char,
    len: usize,
    olen: *mut usize,
) -> *mut c_char {
    let b = to_text(unsafe { bytes(html, len) });
    let p = malloc_str(&b);
    set_len(olen, if p.is_null() { 0 } else { b.len() });
    p
}
