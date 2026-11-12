//! Minimal stable libxkbcommon ABI, checked against the upstream 1.13.1 headers.
//! https://github.com/xkbcommon/libxkbcommon/tree/xkbcommon-1.13.1/include/xkbcommon
use std::ffi::{c_char, c_int, c_void};
pub type Handle = *mut c_void;
pub const MOD_DEPRESSED: u32 = 1;
pub const MOD_LATCHED: u32 = 2;
pub const MOD_LOCKED: u32 = 4;
pub const MOD_EFFECTIVE: u32 = 8;
pub const LAYOUT_EFFECTIVE: u32 = 128;
#[repr(C)]
pub struct RuleNames {
    pub rules: *const c_char,
    pub model: *const c_char,
    pub layout: *const c_char,
    pub variant: *const c_char,
    pub options: *const c_char,
}
#[link(name = "xkbcommon")]
extern "C" {
    pub fn xkb_context_new(flags: c_int) -> Handle;
    pub fn xkb_context_unref(context: Handle);
    pub fn xkb_keymap_new_from_names(
        context: Handle,
        names: *const RuleNames,
        flags: c_int,
    ) -> Handle;
    pub fn xkb_keymap_new_from_string(
        context: Handle,
        text: *const c_char,
        format: c_int,
        flags: c_int,
    ) -> Handle;
    pub fn xkb_keymap_unref(keymap: Handle);
    pub fn xkb_keymap_num_layouts(keymap: Handle) -> u32;
    pub fn xkb_keymap_get_as_string(keymap: Handle, format: c_int) -> *mut c_char;
    pub fn xkb_keymap_key_get_syms_by_level(
        keymap: Handle,
        key: u32,
        layout: u32,
        level: u32,
        syms: *mut *const u32,
    ) -> c_int;
    pub fn xkb_state_new(keymap: Handle) -> Handle;
    pub fn xkb_state_unref(state: Handle);
    pub fn xkb_state_layout_index_is_active(state: Handle, layout: u32, components: u32) -> c_int;
    pub fn xkb_state_serialize_layout(state: Handle, components: u32) -> u32;
    pub fn xkb_state_serialize_mods(state: Handle, components: u32) -> u32;
    pub fn xkb_state_update_mask(
        state: Handle,
        depressed: u32,
        latched: u32,
        locked: u32,
        depressed_layout: u32,
        latched_layout: u32,
        locked_layout: u32,
    ) -> u32;
    pub fn xkb_state_update_key(state: Handle, key: u32, direction: c_int) -> u32;
    pub fn xkb_state_key_get_one_sym(state: Handle, key: u32) -> u32;
    pub fn xkb_state_key_get_utf8(state: Handle, key: u32, out: *mut c_char, size: usize) -> c_int;
    pub fn xkb_state_mod_name_is_active(
        state: Handle,
        name: *const c_char,
        components: u32,
    ) -> c_int;
    pub fn xkb_keysym_to_utf8(sym: u32, out: *mut c_char, size: usize) -> c_int;
    pub fn xkb_keysym_to_utf32(sym: u32) -> u32;
    pub fn xkb_compose_table_new_from_locale(
        context: Handle,
        locale: *const c_char,
        flags: c_int,
    ) -> Handle;
    pub fn xkb_compose_table_unref(table: Handle);
    pub fn xkb_compose_state_new(table: Handle, flags: c_int) -> Handle;
    pub fn xkb_compose_state_unref(state: Handle);
    pub fn xkb_compose_state_feed(state: Handle, sym: u32) -> c_int;
    pub fn xkb_compose_state_get_status(state: Handle) -> c_int;
    pub fn xkb_compose_state_get_utf8(state: Handle, out: *mut c_char, size: usize) -> c_int;
    pub fn xkb_compose_state_reset(state: Handle);
}
