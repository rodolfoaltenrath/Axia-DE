//! The keyboard's fallback translation, used only when axia-server could not load an XKB keymap
//! (formerly runtime/keymap.js). Normally libxkbcommon translates every layout, compose and dead key
//! in Rust (rust/axia-shell/src/keyboard); without it, these two tables ('latam' and 'us') still give the shell
//! a working keyboard. The compositor then hands the shell the same event either way: key name,
//! character, base character and modifier flags (`__lordKey(code, value, key, chr, base, flags)`).
use std::cell::RefCell;
use std::ffi::{c_char, CStr};

pub(super) fn letter(code: i32) -> Option<char> {
    Some(match code {
        16 => 'q', 17 => 'w', 18 => 'e', 19 => 'r', 20 => 't', 21 => 'y', 22 => 'u', 23 => 'i', 24 => 'o', 25 => 'p',
        30 => 'a', 31 => 's', 32 => 'd', 33 => 'f', 34 => 'g', 35 => 'h', 36 => 'j', 37 => 'k', 38 => 'l',
        44 => 'z', 45 => 'x', 46 => 'c', 47 => 'v', 48 => 'b', 49 => 'n', 50 => 'm',
        _ => return None,
    })
}

fn keypad(code: i32) -> Option<&'static str> {
    Some(match code {
        79 => "1", 80 => "2", 81 => "3", 75 => "4", 76 => "5", 77 => "6", 71 => "7", 72 => "8", 73 => "9", 82 => "0",
        83 => ".", 78 => "+", 74 => "-", 55 => "*", 98 => "/",
        _ => return None,
    })
}

pub fn name(code: i32) -> &'static str {
    match code {
        1 => "Escape", 14 => "Backspace", 15 => "Tab", 28 | 96 => "Enter", 57 => " ", 103 => "ArrowUp",
        105 => "ArrowLeft", 106 => "ArrowRight", 108 => "ArrowDown", 102 => "Home", 107 => "End", 111 => "Delete",
        125 | 126 => "Meta", 104 => "PageUp", 109 => "PageDown", 110 => "Insert", 59 => "F1", 60 => "F2", 61 => "F3",
        62 => "F4", 63 => "F5", 64 => "F6", 65 => "F7", 66 => "F8", 67 => "F9", 68 => "F10", 87 => "F11", 88 => "F12",
        _ => "",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Layout {
    Us,
    #[default]
    Latam,
}

/// code → [normal, shift, altgr]; "dead:x" is a dead key, "" nothing.
fn table(layout: Layout, code: i32) -> Option<[&'static str; 3]> {
    Some(match (layout, code) {
        (Layout::Us, 2) => ["1", "!", ""], (Layout::Us, 3) => ["2", "@", ""], (Layout::Us, 4) => ["3", "#", ""],
        (Layout::Us, 5) => ["4", "$", ""], (Layout::Us, 6) => ["5", "%", ""], (Layout::Us, 7) => ["6", "^", ""],
        (Layout::Us, 8) => ["7", "&", ""], (Layout::Us, 9) => ["8", "*", ""], (Layout::Us, 10) => ["9", "(", ""],
        (Layout::Us, 11) => ["0", ")", ""], (Layout::Us, 12) => ["-", "_", ""], (Layout::Us, 13) => ["=", "+", ""],
        (Layout::Us, 26) => ["[", "{", ""], (Layout::Us, 27) => ["]", "}", ""], (Layout::Us, 39) => [";", ":", ""],
        (Layout::Us, 40) => ["'", "\"", ""], (Layout::Us, 41) => ["`", "~", ""], (Layout::Us, 43) => ["\\", "|", ""],
        (Layout::Us, 51) => [",", "<", ""], (Layout::Us, 52) => [".", ">", ""], (Layout::Us, 53) => ["/", "?", ""],
        (Layout::Us, 86) => ["<", ">", ""],
        (Layout::Latam, 2) => ["1", "!", ""], (Layout::Latam, 3) => ["2", "\"", ""], (Layout::Latam, 4) => ["3", "#", ""],
        (Layout::Latam, 5) => ["4", "$", ""], (Layout::Latam, 6) => ["5", "%", ""], (Layout::Latam, 7) => ["6", "&", ""],
        (Layout::Latam, 8) => ["7", "/", ""], (Layout::Latam, 9) => ["8", "(", ""], (Layout::Latam, 10) => ["9", ")", ""],
        (Layout::Latam, 11) => ["0", "=", ""], (Layout::Latam, 12) => ["'", "?", "\\"], (Layout::Latam, 13) => ["¿", "¡", ""],
        (Layout::Latam, 26) => ["dead:´", "dead:¨", ""], (Layout::Latam, 27) => ["+", "*", "~"],
        (Layout::Latam, 39) => ["ñ", "Ñ", ""], (Layout::Latam, 40) => ["{", "[", "^"], (Layout::Latam, 41) => ["|", "°", "¬"],
        (Layout::Latam, 43) => ["}", "]", "`"], (Layout::Latam, 51) => [",", ";", ""], (Layout::Latam, 52) => [".", ":", ""],
        (Layout::Latam, 53) => ["-", "_", ""], (Layout::Latam, 86) => ["<", ">", ""], (Layout::Latam, 16) => ["", "", "@"],
        _ => return None,
    })
}

fn compose(dead: &str, ch: &str) -> Option<&'static str> {
    Some(match (dead, ch) {
        ("´", "a") => "á", ("´", "e") => "é", ("´", "i") => "í", ("´", "o") => "ó", ("´", "u") => "ú",
        ("´", "A") => "Á", ("´", "E") => "É", ("´", "I") => "Í", ("´", "O") => "Ó", ("´", "U") => "Ú",
        ("¨", "a") => "ä", ("¨", "e") => "ë", ("¨", "i") => "ï", ("¨", "o") => "ö", ("¨", "u") => "ü",
        ("¨", "A") => "Ä", ("¨", "E") => "Ë", ("¨", "I") => "Ï", ("¨", "O") => "Ö", ("¨", "U") => "Ü",
        _ => return None,
    })
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct Mods {
    shift: bool,
    ctrl: bool,
    alt: bool,
    altgr: bool,
    meta: bool,
    caps: bool,
}

/// What the shell receives for one key event.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Key {
    pub key: String,
    pub chr: String,
    pub base: String,
    /// 1 Shift, 2 Ctrl, 4 Alt, 8 AltGr, 16 Super, 32 Caps Lock (as the XKB path sends them)
    pub flags: i32,
}

#[derive(Default)]
pub struct Keymap {
    layout: Layout,
    mods: Mods,
    dead: Option<String>,
}

impl Keymap {
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
    }

    /// The character the key gives with Shift, AltGr and Caps Lock but not Ctrl/Alt (shortcuts).
    fn base(&self, code: i32) -> String {
        let t = table(self.layout, code);
        if self.mods.altgr {
            if let Some(c) = t.map(|t| t[2]).filter(|c| !c.is_empty()) {
                return c.into();
            }
        }
        if let Some(l) = letter(code).filter(|_| t.map_or(true, |t| t[0].is_empty())) {
            return if self.mods.shift != self.mods.caps { l.to_ascii_uppercase().into() } else { l.into() };
        }
        if let Some(t) = t {
            let c = t[self.mods.shift as usize];
            return c.strip_prefix("dead:").unwrap_or(c).into();
        }
        if let Some(k) = keypad(code) {
            return k.into();
        }
        if code == 57 { " ".into() } else { String::new() }
    }

    pub fn translate(&mut self, code: i32, value: i32) -> Key {
        let down = value != 0;
        let m = &mut self.mods;
        match code {
            42 | 54 => m.shift = down,
            29 | 97 => m.ctrl = down,
            56 => m.alt = down,
            100 => m.altgr = down,
            125 | 126 => m.meta = down,
            58 if value == 1 => m.caps = !m.caps,
            _ => {}
        }
        let m = self.mods;
        let flags = m.shift as i32 | (m.ctrl as i32) << 1 | (m.alt as i32) << 2 | (m.altgr as i32) << 3 | (m.meta as i32) << 4 | (m.caps as i32) << 5;
        let mut ev = Key { key: name(code).into(), chr: String::new(), base: if down { self.base(code) } else { String::new() }, flags };
        if !down || m.ctrl || m.alt || m.meta {
            return ev;
        }
        let t = table(self.layout, code);
        let ch: String = if m.altgr {
            t.map_or("", |t| t[2]).into()
        } else if let Some(l) = letter(code).filter(|_| t.map_or(true, |t| t[0].is_empty())) {
            if m.shift != m.caps { l.to_ascii_uppercase().into() } else { l.into() }
        } else if let Some(t) = t {
            t[m.shift as usize].into()
        } else if let Some(k) = keypad(code) {
            k.into()
        } else if code == 57 {
            " ".into()
        } else {
            String::new()
        };
        if let Some(d) = ch.strip_prefix("dead:") {
            match self.dead.take() {
                Some(previous) => ev.chr = previous,     // a second dead key types the first one
                None => self.dead = Some(d.into()),
            }
            return ev;
        }
        ev.chr = match (self.dead.take(), ch.is_empty()) {
            (Some(dead), false) => compose(&dead, &ch).map_or_else(|| if ch == " " { dead.clone() } else { format!("{dead}{ch}") }, String::from),
            (dead, true) => {
                self.dead = dead;                         // a key with no character (Shift) keeps it waiting
                String::new()
            }
            (None, false) => ch,
        };
        if !ev.chr.is_empty() && ev.key.is_empty() {
            ev.key = ev.chr.clone();
        }
        ev
    }
}

thread_local! { static KEYMAP: RefCell<Keymap> = RefCell::new(Keymap::default()); }

fn copy(out: *mut c_char, n: usize, s: &str) {
    if out.is_null() || n == 0 {
        return;
    }
    let bytes = s.as_bytes();
    let len = bytes.len().min(n - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast(), len);
        *out.add(len) = 0;
    }
}

/// One key without XKB (server/src/ui/js.c js_key): fills the three strings (each `n` bytes) and
/// returns the flags.
#[no_mangle]
pub unsafe extern "C" fn axia_keymap_translate(code: i32, value: i32, key: *mut c_char, chr: *mut c_char, base: *mut c_char, n: usize) -> i32 {
    let k = KEYMAP.with(|m| m.borrow_mut().translate(code, value));
    copy(key, n, &k.key);
    copy(chr, n, &k.chr);
    copy(base, n, &k.base);
    k.flags
}

/// The key's name when XKB gives none (Escape, Enter, the arrows, F1…): `out`, `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn axia_keymap_name(code: i32, out: *mut c_char, n: usize) {
    copy(out, n, name(code));
}

/// lord.keyboard.fallbackLayout(name): the table used without XKB ("us"; anything else is "latam").
#[no_mangle]
pub unsafe extern "C" fn axia_keymap_set_layout(name: *const c_char) {
    let layout = if !name.is_null() && CStr::from_ptr(name).to_bytes() == b"us" { Layout::Us } else { Layout::Latam };
    KEYMAP.with(|m| m.borrow_mut().set_layout(layout));
}

#[cfg(test)]
mod tests;
