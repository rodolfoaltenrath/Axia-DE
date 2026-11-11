//! Editing a one-line text field: the caret, the selection and what each key does to them
//! (sdk/components.jsx `TextInput`, through `lord.textEdit`).
//!
//! The field used to only append and remove at its end: no caret to move, Delete did nothing, and
//! a name could only be fixed by erasing it back to the mistake. Here a field is its value plus
//! a caret and an anchor (the other end of the selection; equal to the caret when nothing is
//! selected), both counted in Unicode scalar values like JS's `[...value]`.
//!
//! Keys: characters replace the selection; Backspace/Delete erase the selection or one character
//! (a word with Ctrl/Alt); arrows move (a word with Ctrl/Alt, Shift extends the selection);
//! Home/End/Up/Down go to an end; Ctrl+A selects all; Ctrl+C/X copy and cut the selection (never
//! from a password field); Ctrl+V asks for the clipboard (`paste`), which comes back as `insert`;
//! Enter submits and Escape cancels.
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    chars: Vec<char>,
    pub caret: usize,
    pub anchor: usize,
}

/// What a key did besides changing the field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// the key was the field's (the caller doesn't pass it on)
    pub handled: bool,
    pub changed: bool,
    pub submit: bool,
    pub cancel: bool,
    /// text for the clipboard
    pub copy: Option<String>,
    /// the clipboard's text is wanted: the caller sends it back as `insert`
    pub paste: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Key<'a> {
    pub key: &'a str,
    pub chr: &'a str,
    /// Ctrl or Cmd
    pub command: bool,
    pub alt: bool,
    pub shift: bool,
    /// the key's letter without modifiers ("v" for Ctrl+V on any layout)
    pub base: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Other
    }
}

impl Field {
    pub fn new(value: &str, caret: usize, anchor: usize) -> Field {
        let chars: Vec<char> = value.chars().collect();
        let n = chars.len();
        Field { chars, caret: caret.min(n), anchor: anchor.min(n) }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    /// The selection as [start, end).
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    fn selected(&self) -> String {
        let (a, b) = self.selection();
        self.chars[a..b].iter().collect()
    }

    fn place(&mut self, at: usize, extend: bool) {
        self.caret = at.min(self.chars.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Replaces the selection with `text` (line breaks become spaces: the field has one line).
    pub fn insert(&mut self, text: &str) -> bool {
        let (a, b) = self.selection();
        let new: Vec<char> = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).filter(|c| !c.is_control()).collect();
        if a == b && new.is_empty() {
            return false;
        }
        let n = new.len();
        self.chars.splice(a..b, new);
        self.place(a + n, false);
        true
    }

    fn erase(&mut self, from: usize, to: usize) -> bool {
        let (a, b) = (from.min(to), from.max(to).min(self.chars.len()));
        if a == b {
            self.place(a, false);
            return false;
        }
        self.chars.drain(a..b);
        self.place(a, false);
        true
    }

    /// Where a word jump from `at` lands: back to the start of the word before, or on past the end
    /// of the word after (spaces and punctuation in between are crossed).
    fn word(&self, at: usize, forward: bool) -> usize {
        let c = &self.chars;
        let mut i = at;
        if forward {
            while i < c.len() && class(c[i]) == Class::Space {
                i += 1;
            }
            let k = c.get(i).map(|&ch| class(ch));
            while i < c.len() && Some(class(c[i])) == k {
                i += 1;
            }
        } else {
            while i > 0 && class(c[i - 1]) == Class::Space {
                i -= 1;
            }
            let k = i.checked_sub(1).map(|j| class(c[j]));
            while i > 0 && Some(class(c[i - 1])) == k {
                i -= 1;
            }
        }
        i
    }

    /// One key. `secure`: a password field (nothing is copied out, words are the whole value).
    pub fn key(&mut self, k: Key, secure: bool) -> Outcome {
        let mut out = Outcome { handled: true, ..Outcome::default() };
        let (a, b) = self.selection();
        let some = a != b;
        let words = (k.command || k.alt) && !secure;
        let base = if k.base.is_empty() { k.key } else { k.base }.to_lowercase();
        if k.command && !k.alt {
            match base.as_str() {
                "a" => {
                    self.anchor = 0;
                    self.caret = self.chars.len();
                    return out;
                }
                "c" | "x" => {
                    if some && !secure {
                        out.copy = Some(self.selected());
                        if base == "x" {
                            out.changed = self.erase(a, b);
                        }
                    }
                    return out;
                }
                "v" => {
                    out.paste = true;
                    return out;
                }
                _ => {}
            }
        }
        match k.key {
            "Enter" => out.submit = true,
            "Escape" => out.cancel = true,
            "Backspace" => {
                out.changed = if some {
                    self.erase(a, b)
                } else if secure && k.command {
                    self.erase(0, self.caret)
                } else if words {
                    self.erase(self.word(self.caret, false), self.caret)
                } else {
                    self.erase(self.caret.saturating_sub(1), self.caret)
                }
            }
            "Delete" => {
                out.changed = if some {
                    self.erase(a, b)
                } else if words {
                    self.erase(self.caret, self.word(self.caret, true))
                } else {
                    self.erase(self.caret, self.caret + 1)
                }
            }
            "ArrowLeft" | "ArrowRight" => {
                let forward = k.key == "ArrowRight";
                let to = if some && !k.shift && !words {
                    if forward { b } else { a }             // a selection collapses to its side
                } else if secure && words {
                    if forward { self.chars.len() } else { 0 }
                } else if words {
                    self.word(self.caret, forward)
                } else if forward {
                    self.caret + 1
                } else {
                    self.caret.saturating_sub(1)
                };
                self.place(to, k.shift);
            }
            "Home" | "ArrowUp" => self.place(0, k.shift),
            "End" | "ArrowDown" => self.place(self.chars.len(), k.shift),
            _ => {
                // a typed character (AltGr's included); a shortcut of the app's is not the field's
                if k.chr.is_empty() || (k.command && !k.alt) {
                    out.handled = false;
                } else {
                    out.changed = self.insert(k.chr);
                }
            }
        }
        out
    }
}

/// The part of a file name a rename selects at first: the name without its extension, so typing
/// replaces only that ("report.final.pdf" → "report.final"; ".bashrc" and folders: all of it).
pub fn stem_len(name: &str, folder: bool) -> usize {
    let n = name.chars().count();
    if folder {
        return n;
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].chars().count(),
        _ => n,
    }
}

fn reply(f: &Field, o: &Outcome) -> Value {
    json!({
        "value": f.value(), "caret": f.caret, "anchor": f.anchor,
        "handled": o.handled, "changed": o.changed, "submit": o.submit, "cancel": o.cancel,
        "copy": o.copy, "paste": o.paste,
    })
}

/// `lord.textEdit(json)`: { value, caret, anchor, secure, key, char, ctrl, meta, alt, shift, base }
/// for a key, or { value, caret, anchor, insert } to replace the selection (a paste). Returns the
/// field and what happened (see `reply`).
pub fn request(v: &Value) -> Value {
    let s = |k: &str| v[k].as_str().unwrap_or("");
    let n = |k: &str| v[k].as_u64().map_or(usize::MAX, |n| n as usize);
    let b = |k: &str| v[k].as_bool().unwrap_or(false);
    let mut f = Field::new(s("value"), n("caret"), n("anchor"));
    if let Some(text) = v["insert"].as_str() {
        let changed = f.insert(text);
        return reply(&f, &Outcome { handled: true, changed, ..Outcome::default() });
    }
    let key = Key { key: s("key"), chr: s("char"), command: b("ctrl") || b("meta"), alt: b("alt"), shift: b("shift"), base: s("base") };
    let o = f.key(key, b("secure"));
    reply(&f, &o)
}

/// C ABI for js.c (`lord.textEdit`): a JSON request, a JSON reply freed with `axia_text_edit_free`.
#[no_mangle]
pub unsafe extern "C" fn axia_text_edit(input: *const std::ffi::c_char) -> *mut std::ffi::c_char {
    let v = if input.is_null() {
        Value::Null
    } else {
        std::ffi::CStr::from_ptr(input).to_str().ok().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null)
    };
    std::ffi::CString::new(request(&v).to_string()).map_or(std::ptr::null_mut(), std::ffi::CString::into_raw)
}

#[no_mangle]
pub unsafe extern "C" fn axia_text_edit_free(reply: *mut std::ffi::c_char) {
    if !reply.is_null() {
        drop(std::ffi::CString::from_raw(reply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> Key<'_> {
        Key { key: k, ..Key::default() }
    }
    fn typed(c: &str) -> Key<'_> {
        Key { key: c, chr: c, ..Key::default() }
    }
    fn cmd(c: &str) -> Key<'_> {
        Key { key: c, base: c, command: true, ..Key::default() }
    }
    fn shift(k: &str) -> Key<'_> {
        Key { key: k, shift: true, ..Key::default() }
    }
    fn word(k: &str) -> Key<'_> {
        Key { key: k, command: true, ..Key::default() }
    }
    fn at(v: &str, caret: usize) -> Field {
        Field::new(v, caret, caret)
    }

    #[test]
    fn typing_in_the_middle_fixes_a_name() {
        // "relatrio.pdf": the caret after "relat", a typed 'ó'
        let mut f = at("relatrio.pdf", 5);
        assert!(f.key(typed("ó"), false).changed);
        assert_eq!((f.value().as_str(), f.caret, f.anchor), ("relatório.pdf", 6, 6));
        // Delete erases the character after the caret; Backspace the one before
        assert!(f.key(key("Delete"), false).changed);
        assert_eq!((f.value().as_str(), f.caret), ("relatóio.pdf", 6));
        assert!(f.key(key("Backspace"), false).changed);
        assert_eq!((f.value().as_str(), f.caret), ("relatio.pdf", 5));
        // at the ends, nothing to erase
        let mut f = at("ab", 0);
        assert!(!f.key(key("Backspace"), false).changed);
        let mut f = at("ab", 2);
        assert!(!f.key(key("Delete"), false).changed);
    }

    #[test]
    fn arrows_move_and_shift_selects() {
        let mut f = at("hello world", 11);
        f.key(key("ArrowLeft"), false);
        assert_eq!(f.caret, 10);
        f.key(key("Home"), false);
        assert_eq!(f.caret, 0);
        f.key(key("ArrowLeft"), false);
        assert_eq!(f.caret, 0, "nowhere to go");
        f.key(shift("ArrowRight"), false);
        f.key(shift("ArrowRight"), false);
        assert_eq!((f.caret, f.anchor, f.selection()), (2, 0, (0, 2)));
        // a selection collapses to the side the arrow points to
        let mut g = f.clone();
        g.key(key("ArrowLeft"), false);
        assert_eq!((g.caret, g.anchor), (0, 0));
        f.key(key("ArrowRight"), false);
        assert_eq!((f.caret, f.anchor), (2, 2));
        f.key(shift("End"), false);
        assert_eq!(f.selection(), (2, 11));
        f.key(key("ArrowUp"), false);
        assert_eq!((f.caret, f.anchor), (0, 0));
        f.key(key("ArrowDown"), false);
        assert_eq!(f.caret, 11);
    }

    #[test]
    fn a_typed_character_replaces_the_selection() {
        let mut f = Field::new("report.final.pdf", 12, 0);
        f.key(typed("x"), false);
        assert_eq!((f.value().as_str(), f.caret), ("x.pdf", 1));
        let mut f = Field::new("abc", 0, 3);
        f.key(key("Backspace"), false);
        assert_eq!(f.value(), "");
        let mut f = Field::new("abc", 1, 2);
        f.key(key("Delete"), false);
        assert_eq!(f.value(), "ac");
    }

    #[test]
    fn words_with_ctrl() {
        let mut f = at("my old-file name.txt", 20);
        f.key(word("ArrowLeft"), false);
        assert_eq!(f.caret, 17, "before 'txt'");
        f.key(word("ArrowLeft"), false);
        assert_eq!(f.caret, 16, "the '.' is a word of its own");
        f.key(word("ArrowLeft"), false);
        assert_eq!(f.caret, 12, "before 'name'");
        f.key(word("Backspace"), false);
        assert_eq!((f.value().as_str(), f.caret), ("my old-name.txt", 7));
        f.key(word("Delete"), false);
        assert_eq!(f.value(), "my old-.txt");
        let mut f = at("a  b", 0);
        f.key(word("ArrowRight"), false);
        assert_eq!(f.caret, 1);
        f.key(word("ArrowRight"), false);
        assert_eq!(f.caret, 4, "spaces crossed, then the word");
    }

    #[test]
    fn clipboard_and_select_all() {
        let mut f = at("hello world", 3);
        f.key(cmd("a"), false);
        assert_eq!(f.selection(), (0, 11));
        let o = f.key(cmd("c"), false);
        assert_eq!((o.copy.as_deref(), o.changed), (Some("hello world"), false));
        let mut f = Field::new("hello world", 5, 11);
        let o = f.key(cmd("x"), false);
        assert_eq!((o.copy.as_deref(), o.changed, f.value().as_str()), (Some(" world"), true, "hello"));
        assert!(f.key(cmd("v"), false).paste);
        assert!(f.insert("\nthere\r"));
        assert_eq!(f.value(), "hello there ");
        // a password never leaves its field, and Ctrl+Backspace erases it back to the start
        let mut p = Field::new("secret", 0, 6);
        assert_eq!(p.key(cmd("c"), true).copy, None);
        assert_eq!(p.key(cmd("x"), true).copy, None);
        assert_eq!(p.value(), "secret");
        let mut p = at("secret", 4);
        p.key(word("Backspace"), true);
        assert_eq!(p.value(), "et");
        // Ctrl+V on another layout: the base letter decides
        let mut f = at("", 0);
        assert!(f.key(Key { key: "м", base: "v", command: true, ..Key::default() }, false).paste);
    }

    #[test]
    fn other_keys() {
        let mut f = at("x", 1);
        assert!(f.key(key("Enter"), false).submit);
        assert!(f.key(key("Escape"), false).cancel);
        // an app's shortcut, or a key without a character, isn't the field's
        assert!(!f.key(Key { key: "s", chr: "s", command: true, base: "s", ..Key::default() }, false).handled);
        assert!(!f.key(key("F5"), false).handled);
        // AltGr+Q gives '@' on many layouts: typed
        assert!(f.key(Key { key: "@", chr: "@", alt: true, command: true, ..Key::default() }, false).changed);
        assert_eq!(f.value(), "x@");
    }

    #[test]
    fn positions_count_characters_like_js() {
        // é and emoji are one each; out-of-range positions are clamped
        let mut f = Field::new("café 🎉", 99, 99);
        assert_eq!(f.caret, 6);
        f.key(key("Backspace"), false);
        assert_eq!(f.value(), "café ");
        f.key(key("Home"), false);
        f.key(key("ArrowRight"), false);
        f.key(key("ArrowRight"), false);
        f.key(key("ArrowRight"), false);
        f.key(shift("ArrowRight"), false);
        assert_eq!(f.selected(), "é");
    }

    #[test]
    fn a_rename_selects_the_name_without_its_extension() {
        assert_eq!(stem_len("report.final.pdf", false), 12);
        assert_eq!(stem_len("foto.JPG", false), 4);
        assert_eq!(stem_len(".bashrc", false), 7);
        assert_eq!(stem_len("Makefile", false), 8);
        assert_eq!(stem_len("café.txt", false), 4);
        assert_eq!(stem_len("my.folder", true), 9);
    }

    #[test]
    fn the_json_request() {
        let r = request(&json!({ "value": "abc", "caret": 1, "anchor": 1, "key": "x", "char": "x" }));
        assert_eq!(r["value"], "axbc");
        assert_eq!((r["caret"].clone(), r["changed"].clone(), r["handled"].clone()), (json!(2), json!(true), json!(true)));
        let r = request(&json!({ "value": "abc", "caret": 0, "anchor": 3, "insert": "z" }));
        assert_eq!((r["value"].clone(), r["caret"].clone()), (json!("z"), json!(1)));
        // without positions the caret is at the end
        let r = request(&json!({ "value": "abc", "key": "Backspace" }));
        assert_eq!(r["value"], "ab");
        let r = request(&json!({ "value": "abc", "key": "c", "base": "c", "ctrl": true, "caret": 0, "anchor": 3 }));
        assert_eq!(r["copy"], "abc");
        assert_eq!(request(&Value::Null)["handled"], json!(false));
        unsafe {
            let q = std::ffi::CString::new(r#"{"value":"a","key":"Enter"}"#).unwrap();
            let p = axia_text_edit(q.as_ptr());
            let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
            axia_text_edit_free(p);
            assert!(s.contains("\"submit\":true"));
            let p = axia_text_edit(std::ptr::null());
            assert!(!p.is_null());
            axia_text_edit_free(p);
        }
    }
}
