//! Korean, Japanese and Chinese composition with bounded buffers and owned dictionaries.
//! Compatibility ABI: server/src/ime.h. No composition is logged or persisted.
mod dictionary;
mod hangul;
mod kana;
mod pinyin;
mod tables;
use crate::keyboard::KeyInfo;
use std::{
    ffi::{c_char, c_int, CStr},
    sync::{Mutex, MutexGuard, OnceLock},
};

const MAX_CANDIDATES: usize = 64;
const PAGE: usize = 9;
#[repr(C)]
pub struct ImeState {
    preedit: [u8; 512],
    ncand: c_int,
    sel: c_int,
    cand: [[u8; 96]; MAX_CANDIDATES],
    converting: c_int,
    focus_begin: c_int,
    focus_end: c_int,
}
impl Default for ImeState {
    fn default() -> Self {
        Self {
            preedit: [0; 512],
            ncand: 0,
            sel: 0,
            cand: [[0; 96]; MAX_CANDIDATES],
            converting: 0,
            focus_begin: 0,
            focus_end: 0,
        }
    }
}
fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len())]
}
fn copy_text(out: &mut [u8], bytes: &[u8]) {
    if out.is_empty() {
        return;
    }
    let n = bytes.len().min(out.len() - 1);
    out[..n].copy_from_slice(&bytes[..n]);
    out[n] = 0;
}
fn append(out: &mut Vec<u8>, cap: usize, bytes: &[u8]) {
    if out.len() + bytes.len() < cap {
        out.extend_from_slice(bytes);
    }
}
fn append_cp(out: &mut Vec<u8>, cap: usize, cp: u32) {
    if let Some(cp) = char::from_u32(cp) {
        append(out, cap, cp.encode_utf8(&mut [0u8; 4]).as_bytes());
    }
}
fn drop_last(bytes: &mut Vec<u8>) {
    while let Some(byte) = bytes.pop() {
        if byte & 0xc0 != 0x80 {
            break;
        }
    }
}
fn character(key: &KeyInfo) -> u8 {
    if key.chr[0] != 0 && key.chr[1] == 0 {
        key.chr[0]
    } else {
        0
    }
}
fn add_candidate(list: &mut Vec<Vec<u8>>, max: usize, value: &[u8]) {
    if value.is_empty() || list.len() >= max || list.iter().any(|entry| entry == value) {
        return;
    }
    list.push(value[..value.len().min(95)].to_vec());
}
struct Output {
    bytes: Vec<u8>,
    cap: usize,
}
impl Output {
    fn append(&mut self, bytes: &[u8]) {
        append(&mut self.bytes, self.cap, bytes);
    }
}
struct Ui<'a> {
    state: &'a mut ImeState,
    changed: &'a mut bool,
}
impl Ui<'_> {
    fn preedit(&mut self, bytes: &[u8]) {
        copy_text(&mut self.state.preedit, bytes);
        *self.changed = true;
    }
    fn clear(&mut self) {
        if self.state.ncand != 0 || self.state.converting != 0 {
            *self.changed = true;
        }
        self.state.ncand = 0;
        self.state.sel = 0;
        self.state.converting = 0;
    }
    fn add(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || self.state.ncand as usize >= MAX_CANDIDATES {
            return false;
        }
        if self.state.cand[..self.state.ncand as usize]
            .iter()
            .any(|entry| text(entry) == bytes)
        {
            return false;
        }
        copy_text(&mut self.state.cand[self.state.ncand as usize], bytes);
        self.state.ncand += 1;
        true
    }
    fn digit(&self, key: &KeyInfo) -> Option<usize> {
        let c = character(key);
        if !(b'1'..=b'9').contains(&c) {
            return None;
        }
        let index = self.state.sel as usize / PAGE * PAGE + (c - b'1') as usize;
        (index < self.state.ncand as usize).then_some(index)
    }
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Engine {
    #[default]
    Off,
    Hangul,
    Kana,
    Pinyin,
}
#[derive(Default)]
struct Ime {
    engine: Engine,
    context: bool,
    changed: bool,
    state: ImeState,
    dict_path: Vec<u8>,
    hangul: hangul::Hangul,
    kana: kana::Kana,
    pinyin: pinyin::Pinyin,
}
impl Ime {
    fn set_context(&mut self, on: bool) {
        if self.context && !on {
            self.reset();
        }
        self.context = on;
    }
    fn reset(&mut self) {
        self.hangul.reset();
        self.kana.reset_text();
        self.pinyin.reset_text();
        if self.state.preedit[0] != 0 || self.state.ncand != 0 {
            self.changed = true;
        }
        self.state.preedit[0] = 0;
        Ui {
            state: &mut self.state,
            changed: &mut self.changed,
        }
        .clear();
    }
    fn set(&mut self, name: &[u8]) -> c_int {
        let engine = match name {
            b"" => Engine::Off,
            b"hangul" => Engine::Hangul,
            b"kana" => Engine::Kana,
            b"pinyin" => Engine::Pinyin,
            _ => return -1,
        };
        self.reset();
        self.engine = engine;
        self.changed = true;
        self.dict_path.clear();
        let result = match engine {
            Engine::Kana => {
                let loaded = self.kana.load();
                if loaded {
                    self.dict_path.extend_from_slice(b"skkdic");
                }
                if loaded {
                    0
                } else {
                    1
                }
            }
            Engine::Pinyin => {
                let (mapped, nonempty) = self.pinyin.load();
                if mapped {
                    self.dict_path.extend_from_slice(b"pinyin_simp");
                }
                if mapped && nonempty {
                    0
                } else {
                    1
                }
            }
            _ => 0,
        };
        self.dict_path.push(0);
        result
    }
    fn key(&mut self, key: &KeyInfo, out: &mut Output) -> bool {
        if self.engine == Engine::Off || !self.context {
            return false;
        }
        let mut ui = Ui {
            state: &mut self.state,
            changed: &mut self.changed,
        };
        if key.ctrl != 0 || key.alt != 0 || key.meta != 0 {
            match self.engine {
                Engine::Hangul if !self.hangul.empty() => self.hangul.commit(out, &mut ui),
                Engine::Kana if !self.kana.empty() => self.kana.commit(out, &mut ui),
                Engine::Pinyin if !self.pinyin.empty() => self.pinyin.commit_raw(out, &mut ui),
                _ => {}
            }
            return false;
        }
        match self.engine {
            Engine::Hangul => self.hangul.key(key, out, &mut ui),
            Engine::Kana => self.kana.key(key, out, &mut ui),
            Engine::Pinyin => self.pinyin.key(key, out, &mut ui),
            Engine::Off => false,
        }
    }
}
static IME: OnceLock<Mutex<Ime>> = OnceLock::new();
fn ime() -> MutexGuard<'static, Ime> {
    IME.get_or_init(|| {
        Mutex::new(Ime {
            dict_path: vec![0],
            ..Ime::default()
        })
    })
    .lock()
    .unwrap()
}
/// # Safety
/// `name` must be null or a valid NUL-terminated engine name.
#[no_mangle]
pub unsafe extern "C" fn ime_set(name: *const c_char) -> c_int {
    let name = if name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(name).to_bytes()
    };
    let name = name.to_vec();
    ime().set(&name)
}
#[no_mangle]
pub extern "C" fn ime_current() -> *const c_char {
    match ime().engine {
        Engine::Off => c"",
        Engine::Hangul => c"hangul",
        Engine::Kana => c"kana",
        Engine::Pinyin => c"pinyin",
    }
    .as_ptr()
}
#[no_mangle]
pub extern "C" fn ime_context(on: c_int) {
    ime().set_context(on != 0);
}
#[no_mangle]
pub extern "C" fn ime_reset() {
    ime().reset();
}
/// # Safety
/// `key` must point to a readable KeyInfo; `out` holds `size` writable bytes (may
/// be null for zero size). Callers must serialize all borrowed-state reads.
#[no_mangle]
pub unsafe extern "C" fn ime_key(key: *const KeyInfo, out: *mut c_char, size: usize) -> c_int {
    let mut output = Output {
        bytes: Vec::new(),
        cap: size,
    };
    let used = ime().key(&*key, &mut output);
    if size != 0 {
        copy_text(
            std::slice::from_raw_parts_mut(out.cast(), size),
            &output.bytes,
        );
    }
    used as c_int
}
/// Borrowed stable storage: valid until the next mutation, no concurrent reads.
#[no_mangle]
pub extern "C" fn ime_state() -> *const ImeState {
    &ime().state
}
#[no_mangle]
pub extern "C" fn ime_changed() -> c_int {
    std::mem::take(&mut ime().changed) as c_int
}
/// Borrowed until the next ime_set; callers must prevent concurrent mutation.
#[no_mangle]
pub extern "C" fn ime_dict_path() -> *const c_char {
    ime().dict_path.as_ptr().cast()
}

#[cfg(test)]
mod tests;
