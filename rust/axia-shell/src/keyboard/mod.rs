//! Owned XKB keymaps, modifier state, compose state and on-screen key labels.
//! Compatibility entry points match server/src/keyboard.h during migration.
mod ffi;
mod symbols;
use ffi::*;
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    ptr::{self, NonNull},
    sync::{Mutex, MutexGuard, OnceLock},
};

struct Owned {
    pointer: NonNull<c_void>,
    release: unsafe extern "C" fn(Handle),
}
impl Owned {
    fn new(pointer: Handle, release: unsafe extern "C" fn(Handle)) -> Option<Self> {
        NonNull::new(pointer).map(|pointer| Self { pointer, release })
    }
    fn ptr(&self) -> Handle {
        self.pointer.as_ptr()
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.release)(self.ptr()) };
    }
}
// XKB objects are accessed only under KEYBOARD's mutex (or by their local owner).
unsafe impl Send for Owned {}

pub struct Keyboard {
    context: Option<Owned>,
    keymap: Option<Owned>,
    state: Option<Owned>,
    compose_table: Option<Owned>,
    compose: Option<Owned>,
    levels: [Option<Owned>; 4],
    layouts: [u8; 128],
    variants: [u8; 128],
    altgr_down: bool,
}
impl Default for Keyboard {
    fn default() -> Self {
        Self {
            context: None,
            keymap: None,
            state: None,
            compose_table: None,
            compose: None,
            levels: std::array::from_fn(|_| None),
            layouts: [0; 128],
            variants: [0; 128],
            altgr_down: false,
        }
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.labels_end();
        self.compose = None;
        self.compose_table = None;
        self.state = None;
        self.keymap = None;
        self.context = None;
    }
}

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq)]
pub struct KeyInfo {
    pub(crate) key: [u8; 32],
    pub(crate) chr: [u8; 16],
    pub(crate) base: [u8; 8],
    pub(crate) shift: c_int,
    pub(crate) ctrl: c_int,
    pub(crate) alt: c_int,
    pub(crate) altgr: c_int,
    pub(crate) meta: c_int,
    pub(crate) caps: c_int,
    pub(crate) dead: c_int,
}

fn copy_text(out: &mut [u8], text: &[u8]) {
    if out.is_empty() {
        return;
    }
    let count = text.len().min(out.len() - 1);
    out[..count].copy_from_slice(&text[..count]);
    out[count] = 0;
}
fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
fn valid(name: &[u8]) -> bool {
    name.len() <= 120
        && name
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b",_-".contains(b))
}
fn pointer(handle: &Option<Owned>) -> Handle {
    handle.as_ref().map_or(ptr::null_mut(), Owned::ptr)
}

impl Keyboard {
    fn context(&mut self) -> Option<Handle> {
        if self.context.is_none() {
            self.context = Owned::new(unsafe { xkb_context_new(0) }, xkb_context_unref);
        }
        self.context.as_ref().map(Owned::ptr)
    }

    fn setup_compose(&mut self) {
        self.compose = None;
        self.compose_table = None;
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .filter_map(std::env::var_os)
            .find(|value| !value.is_empty());
        use std::os::unix::ffi::OsStrExt;
        let locale = locale
            .and_then(|value| CString::new(value.as_bytes()).ok())
            .unwrap_or_else(|| CString::new("C").unwrap());
        unsafe {
            let mut table =
                xkb_compose_table_new_from_locale(pointer(&self.context), locale.as_ptr(), 0);
            if table.is_null() {
                table = xkb_compose_table_new_from_locale(
                    pointer(&self.context),
                    c"en_US.UTF-8".as_ptr(),
                    0,
                );
            }
            self.compose_table = Owned::new(table, xkb_compose_table_unref);
            if let Some(table) = &self.compose_table {
                self.compose = Owned::new(
                    xkb_compose_state_new(table.ptr(), 0),
                    xkb_compose_state_unref,
                );
            }
        }
    }

    fn set(&mut self, layouts: &CStr, variants: &CStr) -> c_int {
        if layouts.is_empty() || !valid(layouts.to_bytes()) || !valid(variants.to_bytes()) {
            return -1;
        }
        let Some(context) = self.context() else {
            return -1;
        };
        let names = RuleNames {
            rules: c"evdev".as_ptr(),
            model: c"pc105".as_ptr(),
            layout: layouts.as_ptr(),
            variant: variants.as_ptr(),
            options: c"".as_ptr(),
        };
        unsafe {
            let Some(keymap) = Owned::new(
                xkb_keymap_new_from_names(context, &names, 0),
                xkb_keymap_unref,
            ) else {
                return -1;
            };
            let Some(state) = Owned::new(xkb_state_new(keymap.ptr()), xkb_state_unref) else {
                return -1;
            };
            // Install atomically: allocation failure leaves the old keyboard usable.
            self.state = Some(state);
            self.keymap = Some(keymap);
        }
        copy_text(&mut self.layouts, layouts.to_bytes());
        copy_text(&mut self.variants, variants.to_bytes());
        self.setup_compose();
        0
    }

    fn set_string(&mut self, source: &CStr) -> c_int {
        if source.is_empty() {
            return -1;
        }
        let Some(context) = self.context() else {
            return -1;
        };
        unsafe {
            let Some(keymap) = Owned::new(
                xkb_keymap_new_from_string(context, source.as_ptr(), 1, 0),
                xkb_keymap_unref,
            ) else {
                return -1;
            };
            let Some(state) = Owned::new(xkb_state_new(keymap.ptr()), xkb_state_unref) else {
                return -1;
            };
            self.state = Some(state);
            self.keymap = Some(keymap);
        }
        // App mode keeps the compose table prepared before entering the sandbox.
        0
    }

    fn group(&self) -> c_int {
        if self.state.is_none() {
            return 0;
        }
        unsafe {
            for group in 0..xkb_keymap_num_layouts(pointer(&self.keymap)) {
                if xkb_state_layout_index_is_active(pointer(&self.state), group, LAYOUT_EFFECTIVE)
                    > 0
                {
                    return group as c_int;
                }
            }
        }
        0
    }

    fn set_group(&mut self, group: c_int) {
        if self.state.is_none() || group < 0 {
            return;
        }
        unsafe {
            if group as u32 >= xkb_keymap_num_layouts(pointer(&self.keymap)) {
                return;
            }
            let state = pointer(&self.state);
            xkb_state_update_mask(
                state,
                xkb_state_serialize_mods(state, MOD_DEPRESSED),
                xkb_state_serialize_mods(state, MOD_LATCHED),
                xkb_state_serialize_mods(state, MOD_LOCKED),
                0,
                0,
                group as u32,
            );
        }
    }

    fn reset(&mut self) {
        if self.keymap.is_none() || self.state.is_none() {
            return;
        }
        let group = self.group();
        if let Some(state) = Owned::new(
            unsafe { xkb_state_new(pointer(&self.keymap)) },
            xkb_state_unref,
        ) {
            self.state = Some(state);
            self.altgr_down = false;
            self.set_group(group);
        }
    }

    fn labels_end(&mut self) {
        self.levels = std::array::from_fn(|_| None);
    }
    fn labels_begin(&mut self) -> c_int {
        self.labels_end();
        if self.state.is_none() {
            return 0;
        }
        let group = self.group() as u32;
        for level in 0..4 {
            unsafe {
                let Some(state) = Owned::new(xkb_state_new(pointer(&self.keymap)), xkb_state_unref)
                else {
                    self.labels_end();
                    return 0;
                };
                if level & 1 != 0 {
                    xkb_state_update_key(state.ptr(), 42 + 8, 1);
                }
                if level & 2 != 0 {
                    xkb_state_update_key(state.ptr(), 100 + 8, 1);
                }
                let depressed = xkb_state_serialize_mods(state.ptr(), MOD_DEPRESSED);
                xkb_state_update_mask(state.ptr(), depressed, 0, 0, 0, 0, group);
                self.levels[level] = Some(state);
            }
        }
        1
    }

    fn label(&self, code: c_int, level: c_int, out: &mut [u8]) {
        if out.is_empty() {
            return;
        }
        out[0] = 0;
        let Some(state) = usize::try_from(level)
            .ok()
            .and_then(|i| self.levels.get(i))
            .and_then(Option::as_ref)
        else {
            return;
        };
        unsafe {
            let sym = xkb_state_key_get_one_sym(state.ptr(), (code as u32).wrapping_add(8));
            if let Some(mark) = dead_mark(sym) {
                copy_text(out, format!("dead:{mark}").as_bytes());
            } else if sym != symbols::NOSYMBOL
                && xkb_keysym_to_utf8(sym, out.as_mut_ptr().cast(), out.len()) <= 0
            {
                out[0] = 0;
            }
        }
        if out[0] < 0x20 || out[0] == 0x7f {
            out[0] = 0;
        }
    }

    fn has_altgr(&self) -> c_int {
        if self.keymap.is_none() {
            return 0;
        }
        let mut syms = ptr::null();
        unsafe {
            let count = xkb_keymap_key_get_syms_by_level(
                pointer(&self.keymap),
                100 + 8,
                self.group() as u32,
                0,
                &mut syms,
            );
            (count == 1 && *syms == symbols::ISO_LEVEL3_SHIFT) as c_int
        }
    }

    fn key(&mut self, code: c_int, value: c_int) -> KeyInfo {
        let mut key = KeyInfo::default();
        if self.state.is_none() {
            return key;
        }
        let down = value != 0;
        let code_xkb = (code as u32).wrapping_add(8);
        if code == 100 {
            self.altgr_down = down;
        }
        unsafe {
            let state = pointer(&self.state);
            if value != 2 {
                xkb_state_update_key(state, code_xkb, down as c_int);
            }
            let active = |name: &CStr, components| {
                (xkb_state_mod_name_is_active(state, name.as_ptr(), components) > 0) as c_int
            };
            key.shift = active(c"Shift", MOD_EFFECTIVE);
            key.ctrl = active(c"Control", MOD_EFFECTIVE);
            key.alt = active(c"Mod1", MOD_EFFECTIVE);
            key.meta = active(c"Mod4", MOD_EFFECTIVE);
            key.caps = active(c"Lock", MOD_LOCKED);
            key.altgr = self.altgr_down as c_int;
            if !down {
                return key;
            }
            let sym = xkb_state_key_get_one_sym(state, code_xkb);
            let name = named(sym);
            if let Some(name) = name {
                copy_text(&mut key.key, name.as_bytes());
            }
            let cp = xkb_keysym_to_utf32(sym);
            if cp >= 0x20 && cp != 0x7f {
                if let Some(ch) = char::from_u32(cp) {
                    let mut bytes = [0u8; 4];
                    copy_text(&mut key.base, ch.encode_utf8(&mut bytes).as_bytes());
                }
            }
            if key.ctrl != 0 || key.alt != 0 || key.meta != 0 || name.is_some() {
                if name.is_none() {
                    copy_text(&mut key.key, text(&key.base));
                }
                return key;
            }
            if let Some(compose) = &self.compose {
                xkb_compose_state_feed(compose.ptr(), sym);
                match xkb_compose_state_get_status(compose.ptr()) {
                    1 => {
                        key.dead = 1;
                        return key;
                    }
                    2 => {
                        xkb_compose_state_get_utf8(
                            compose.ptr(),
                            key.chr.as_mut_ptr().cast(),
                            key.chr.len(),
                        );
                        xkb_compose_state_reset(compose.ptr());
                        copy_text(&mut key.key, text(&key.chr));
                        return key;
                    }
                    3 => {
                        xkb_compose_state_reset(compose.ptr());
                        return key;
                    }
                    _ => {}
                }
            }
            xkb_state_key_get_utf8(state, code_xkb, key.chr.as_mut_ptr().cast(), key.chr.len());
            if key.chr[0] < 0x20 || key.chr[0] == 0x7f {
                key.chr[0] = 0;
            }
            copy_text(&mut key.key, text(&key.chr));
        }
        key
    }
}

fn dead_mark(sym: u32) -> Option<&'static str> {
    use symbols::*;
    Some(match sym {
        DEAD_ACUTE => "´",
        DEAD_GRAVE => "`",
        DEAD_CIRCUMFLEX => "^",
        DEAD_TILDE => "~",
        DEAD_DIAERESIS => "¨",
        DEAD_CEDILLA => "¸",
        DEAD_ABOVERING => "°",
        DEAD_MACRON => "¯",
        DEAD_CARON => "ˇ",
        DEAD_BREVE => "˘",
        DEAD_ABOVEDOT => "˙",
        DEAD_OGONEK => "˛",
        DEAD_DOUBLEACUTE => "˝",
        _ => return None,
    })
}
fn named(sym: u32) -> Option<&'static str> {
    use symbols::*;
    Some(match sym {
        ESCAPE => "Escape",
        BACKSPACE => "Backspace",
        TAB | ISO_LEFT_TAB => "Tab",
        RETURN | KP_ENTER => "Enter",
        UP | KP_UP => "ArrowUp",
        DOWN | KP_DOWN => "ArrowDown",
        LEFT | KP_LEFT => "ArrowLeft",
        RIGHT | KP_RIGHT => "ArrowRight",
        HOME | KP_HOME => "Home",
        END | KP_END => "End",
        PAGE_UP | KP_PAGE_UP => "PageUp",
        PAGE_DOWN | KP_PAGE_DOWN => "PageDown",
        INSERT | KP_INSERT => "Insert",
        DELETE | KP_DELETE => "Delete",
        SUPER_L | SUPER_R => "Meta",
        F1..=F12 => [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        ][(sym - F1) as usize],
        _ => return None,
    })
}

static KEYBOARD: OnceLock<Mutex<Keyboard>> = OnceLock::new();
fn keyboard() -> MutexGuard<'static, Keyboard> {
    KEYBOARD
        .get_or_init(|| Mutex::new(Keyboard::default()))
        .lock()
        .unwrap()
}

/// # Safety
/// Inputs must be null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn kb_set(layouts: *const c_char, variants: *const c_char) -> c_int {
    if layouts.is_null() || variants.is_null() {
        return -1;
    }
    // Inputs can be the borrowed strings returned by kb_layouts/kb_variants.
    // Own copies before updating the same storage under the keyboard lock.
    let layouts = CStr::from_ptr(layouts).to_owned();
    let variants = CStr::from_ptr(variants).to_owned();
    keyboard().set(&layouts, &variants)
}
/// # Safety
/// `source` must be null or a valid NUL-terminated XKB keymap.
#[no_mangle]
pub unsafe extern "C" fn kb_set_keymap_string(source: *const c_char) -> c_int {
    if source.is_null() {
        return -1;
    }
    keyboard().set_string(CStr::from_ptr(source))
}
#[no_mangle]
pub extern "C" fn kb_reset_state() {
    keyboard().reset();
}
#[no_mangle]
pub extern "C" fn kb_ready() -> c_int {
    keyboard().state.is_some() as c_int
}
/// Borrowed, stable storage; callers must not read concurrently with kb_set.
#[no_mangle]
pub extern "C" fn kb_layouts() -> *const c_char {
    keyboard().layouts.as_ptr().cast()
}
/// Borrowed, stable storage; callers must not read concurrently with kb_set.
#[no_mangle]
pub extern "C" fn kb_variants() -> *const c_char {
    keyboard().variants.as_ptr().cast()
}
#[no_mangle]
pub extern "C" fn kb_group() -> c_int {
    keyboard().group()
}
#[no_mangle]
pub extern "C" fn kb_set_group(group: c_int) {
    keyboard().set_group(group);
}
#[no_mangle]
pub extern "C" fn kb_reload_compose() {
    let mut kb = keyboard();
    if kb.context.is_some() {
        kb.setup_compose();
    }
}
#[no_mangle]
pub extern "C" fn kb_labels_begin() -> c_int {
    keyboard().labels_begin()
}
#[no_mangle]
pub extern "C" fn kb_labels_end() {
    keyboard().labels_end();
}
/// # Safety
/// For nonzero `size`, `out` must point to writable storage of at least `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn kb_label(code: c_int, level: c_int, out: *mut c_char, size: usize) {
    if size == 0 || out.is_null() {
        return;
    }
    keyboard().label(
        code,
        level,
        std::slice::from_raw_parts_mut(out.cast(), size),
    );
}
#[no_mangle]
pub extern "C" fn kb_has_altgr() -> c_int {
    keyboard().has_altgr()
}
/// # Safety
/// `out` must point to writable, aligned KeyInfo storage.
#[no_mangle]
pub unsafe extern "C" fn kb_key(code: c_int, value: c_int, out: *mut KeyInfo) {
    out.write(keyboard().key(code, value));
}
/// Returned XKB serialization must be released by the caller with libc free.
#[no_mangle]
pub extern "C" fn kb_keymap_string() -> *mut c_char {
    let kb = keyboard();
    if kb.keymap.is_none() {
        return ptr::null_mut();
    }
    unsafe { xkb_keymap_get_as_string(pointer(&kb.keymap), 1) }
}
/// # Safety
/// Each output must point to a writable u32, without aliasing another output.
#[no_mangle]
pub unsafe extern "C" fn kb_mods(
    depressed: *mut u32,
    latched: *mut u32,
    locked: *mut u32,
    group: *mut u32,
) {
    let kb = keyboard();
    let values = if kb.state.is_none() {
        [0; 4]
    } else {
        let state = pointer(&kb.state);
        [
            xkb_state_serialize_mods(state, MOD_DEPRESSED),
            xkb_state_serialize_mods(state, MOD_LATCHED),
            xkb_state_serialize_mods(state, MOD_LOCKED),
            xkb_state_serialize_layout(state, LAYOUT_EFFECTIVE),
        ]
    };
    for (out, value) in [depressed, latched, locked, group].into_iter().zip(values) {
        out.write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_modifiers_repeat_labels_and_focus_reset() {
        let mut kb = Keyboard::default();
        assert_eq!(kb.key(30, 1), KeyInfo::default());
        assert_eq!(kb.set(c"us,latam", c","), 0);
        assert_eq!(text(&kb.layouts), b"us,latam");
        assert_eq!(text(&kb.key(30, 1).chr), b"a");
        kb.key(30, 0);
        kb.key(42, 1);
        let shifted = kb.key(30, 1);
        assert_eq!(text(&shifted.chr), b"A");
        assert_eq!(shifted.shift, 1);
        kb.key(30, 0);
        assert_eq!(kb.labels_begin(), 1);
        let mut label = [0u8; 32];
        kb.label(30, 0, &mut label);
        assert_eq!(text(&label), b"a");
        kb.label(30, 1, &mut label);
        assert_eq!(text(&label), b"A");
        kb.labels_end();
        assert_eq!(text(&kb.key(30, 2).chr), b"A");
        kb.set_group(1);
        assert_eq!(kb.group(), 1);
        kb.reset();
        assert_eq!(kb.group(), 1);
        assert_eq!(text(&kb.key(30, 1).chr), b"a");
        kb.key(30, 0);
        kb.key(29, 1);
        let shortcut = kb.key(30, 1);
        assert_eq!(shortcut.ctrl, 1);
        assert_eq!(text(&shortcut.base), b"a");
        assert_eq!(text(&shortcut.key), b"a");
        assert_eq!(text(&shortcut.chr), b"");
        kb.reset();
        kb.key(100, 1);
        assert_eq!(kb.key(30, 1).altgr, 1);
        kb.reset();
        assert_eq!(kb.key(30, 1).altgr, 0);
        kb.set_group(-1);
        kb.set_group(99);
        assert_eq!(kb.group(), 1);
        assert_eq!(kb.set(c"../us", c""), -1);
        assert_eq!(kb.set(c"US", c""), -1);
        assert_eq!(kb.set(c"", c""), -1);
        assert_eq!(kb.group(), 1);
    }

    #[test]
    fn compose_and_serialized_keymap_roundtrip() {
        let mut kb = Keyboard::default();
        assert_eq!(kb.set(c"us", c"intl"), 0);
        assert_eq!(kb.has_altgr(), 1);
        assert_eq!(kb.key(40, 1).dead, 1);
        kb.key(40, 0);
        let composed = kb.key(30, 1);
        assert_eq!(text(&composed.chr), "á".as_bytes());
        kb.key(30, 0);
        assert_eq!(kb.labels_begin(), 1);
        let mut label = [0u8; 32];
        kb.label(40, 0, &mut label);
        assert_eq!(text(&label), "dead:´".as_bytes());
        kb.label(28, 0, &mut label);
        assert_eq!(text(&label), b"");
        let source = unsafe { xkb_keymap_get_as_string(pointer(&kb.keymap), 1) };
        assert!(!source.is_null());
        let mut imported = Keyboard::default();
        assert_eq!(imported.set(c"us", c""), 0);
        assert_eq!(unsafe { imported.set_string(CStr::from_ptr(source)) }, 0);
        unsafe {
            libc::free(source.cast());
        }
        assert_eq!(text(&imported.key(30, 1).chr), b"a");
        assert_eq!(imported.set_string(c""), -1);
        assert_eq!(text(&imported.key(48, 1).chr), b"b");
    }

    #[test]
    fn label_empty_and_tiny_buffers_are_bounded() {
        let mut kb = Keyboard::default();
        assert_eq!(kb.set(c"us", c"intl"), 0);
        assert_eq!(kb.labels_begin(), 1);
        kb.label(40, 0, &mut []);
        let mut tiny = [0xff; 1];
        kb.label(40, 0, &mut tiny);
        assert_eq!(tiny, [0]);
        let mut out = [0xff; 8];
        kb.label(40, -1, &mut out);
        assert_eq!(out[0], 0);
        kb.label(40, 4, &mut out);
        assert_eq!(out[0], 0);
    }
}
