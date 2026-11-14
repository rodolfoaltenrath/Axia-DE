//! Rust services embedded in the existing C compositor.
//! Window-system objects and the React UI retain their existing interfaces.
mod anim;
mod clock;
#[path = "../../../apps/help/mod.rs"]
mod help;
#[path = "../../../apps/clock/mod.rs"]
mod clock_app;
#[path = "../../../apps/characters/mod.rs"]
mod characters;
#[path = "../../../apps/about/mod.rs"]
mod about;
#[path = "../../../apps/files/mod.rs"]
mod files;
#[path = "../../../apps/forcequit/mod.rs"]
mod forcequit;
#[path = "../../../apps/calculator/mod.rs"]
mod calculator;
#[path = "../../../apps/appabout/mod.rs"]
mod appabout;
#[path = "../../../apps/settings/model.rs"]
mod settings_model;
#[path = "../../../apps/terminal/model.rs"]
mod terminal_model;
#[path = "../../../apps/passwords/model.rs"]
mod passwords_model;
#[path = "../../../apps/sysinfo/model.rs"]
mod sysinfo_model;
mod native_apps_host;
#[path = "../../../apps/notes/mod.rs"]
mod notes_app;
mod native_view;
#[path = "../../../apps/monitor/mod.rs"]
mod monitor;
#[path = "../../../apps/store/mod.rs"]
mod store;
#[path = "../../../apps/logs/mod.rs"]
mod logs_app;
#[path = "../../../apps/fonts/mod.rs"]
mod fonts_app;
#[path = "../../../apps/scanner/mod.rs"]
mod scanner_app;
#[path = "../../../apps/recorder/mod.rs"]
mod recorder_app;
#[path = "../../../apps/backup/mod.rs"]
mod backup_app;
#[path = "../../../apps/camera/mod.rs"]
mod camera_app;
#[path = "../../native_apps.rs"]
mod native_apps;
mod flip_pacing;
mod frame_pipeline;
mod parallel;
mod gpu_scissor;
mod cursor;
mod stall;
mod css_style;
mod keymap;
mod keyboard;
mod onscreen_keyboard;
mod evdev;
mod key_latency;
mod input_method;
mod touch;
mod keygrab;
mod wayland;
mod x11;
mod window_policy;
mod canvas_shadow;
mod canvas_ops;
mod glass_pacing;
mod text_edit;
mod clipboard;
mod runtime;
mod applications;
mod launcher;
mod search;
mod glass;
mod glass_render;
mod surface_damage;
mod wallpaper;
mod power;
mod brightness;
mod network;
mod network_bus;
mod tray;
mod dock_preview;
mod dock_layout;
mod appearance;
mod external_style;
mod native_style;
mod backdrop;
mod snap;
mod winmodel;
use std::ffi::{c_char, CStr};

const LIMIT: usize = 512;

#[derive(Default)]
pub struct AxiaSwitcher {
    recent: Vec<String>,
    items: Vec<(String, usize)>,
    selected: usize,
}

impl AxiaSwitcher {
    fn focus(&mut self, key: &str) {
        self.recent.retain(|k| k != key);
        self.recent.insert(0, key.to_owned());
        self.recent.truncate(LIMIT);
    }

    fn begin(&mut self, keys: &[String], focused: &str, backwards: bool) {
        self.items.clear();
        for (source, key) in keys.iter().take(LIMIT).enumerate() {
            if !self.items.iter().any(|(k, _)| k == key) {
                self.items.push((key.clone(), source));
            }
        }
        self.recent.retain(|k| keys.contains(k));
        let recent = &self.recent;
        self.items.sort_by_key(|(key, source)| {
            recent
                .iter()
                .position(|k| k == key)
                .unwrap_or(recent.len() + source)
        });
        self.selected = if backwards {
            self.items.len().saturating_sub(1)
        } else if self.items.len() > 1 && self.items[0].0 == focused {
            1
        } else {
            0
        };
    }

    fn step(&mut self, direction: i32) {
        let n = self.items.len();
        if n != 0 {
            self.selected = if direction < 0 {
                (self.selected + n - 1) % n
            } else {
                (self.selected + 1) % n
            };
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(index) = self.items.iter().position(|(k, _)| k == key) {
            self.items.remove(index);
            if index < self.selected {
                self.selected -= 1;
            }
            if self.selected >= self.items.len() {
                self.selected = 0;
            }
        }
        self.recent.retain(|k| k != key);
    }

    fn index(&self) -> i32 {
        if self.items.is_empty() {
            -1
        } else {
            self.selected as i32
        }
    }
}

// SAFETY CONTRACT for every C entrypoint:
// - state is null or a live pointer from axia_switcher_new, accessed on the UI thread;
// - text is null or a valid NUL-terminated UTF-8 string for this call;
// - keys points to count valid string pointers. C bounds count and string sizes.
// C/QuickJS owns the handle and frees it exactly once in the object's finalizer.
unsafe fn text<'a>(value: *const c_char) -> &'a str {
    if value.is_null() {
        ""
    } else {
        CStr::from_ptr(value).to_str().unwrap_or("")
    }
}

#[no_mangle]
pub extern "C" fn axia_switcher_new() -> *mut AxiaSwitcher {
    Box::into_raw(Box::new(AxiaSwitcher::default()))
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_free(state: *mut AxiaSwitcher) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_focus(state: *mut AxiaSwitcher, key: *const c_char) {
    if let Some(s) = state.as_mut() {
        s.focus(text(key));
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_begin(
    state: *mut AxiaSwitcher,
    keys: *const *const c_char,
    count: usize,
    focused: *const c_char,
    backwards: bool,
) {
    if count > LIMIT || (count != 0 && keys.is_null()) {
        return;
    }
    if let Some(s) = state.as_mut() {
        let owned: Vec<String> = (0..count).map(|i| text(*keys.add(i)).to_owned()).collect();
        s.begin(&owned, text(focused), backwards);
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_step(state: *mut AxiaSwitcher, direction: i32) {
    if let Some(s) = state.as_mut() {
        s.step(direction);
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_select(state: *mut AxiaSwitcher, index: usize) {
    if let Some(s) = state.as_mut() {
        if index < s.items.len() {
            s.selected = index;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_remove(state: *mut AxiaSwitcher, key: *const c_char) {
    if let Some(s) = state.as_mut() {
        s.remove(text(key));
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_index(state: *const AxiaSwitcher) -> i32 {
    state.as_ref().map_or(-1, AxiaSwitcher::index)
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_len(state: *const AxiaSwitcher) -> usize {
    state.as_ref().map_or(0, |s| s.items.len())
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_source(state: *const AxiaSwitcher, index: usize) -> i32 {
    state
        .as_ref()
        .and_then(|s| s.items.get(index))
        .map_or(-1, |(_, source)| *source as i32)
}

#[no_mangle]
pub unsafe extern "C" fn axia_switcher_finish(state: *mut AxiaSwitcher, commit: bool) -> i32 {
    if let Some(s) = state.as_mut() {
        let index = if commit { s.index() } else { -1 };
        s.items.clear();
        s.selected = 0;
        index
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn focus_history_orders_and_opens_previous_app() {
        let mut s = AxiaSwitcher::default();
        s.focus("files");
        s.focus("terminal");
        s.focus("browser");
        s.begin(&keys(&["files", "browser", "terminal"]), "browser", false);
        assert_eq!(
            s.items,
            vec![
                ("browser".into(), 1),
                ("terminal".into(), 2),
                ("files".into(), 0)
            ]
        );
        assert_eq!(s.index(), 1);
    }

    #[test]
    fn backwards_and_wrapping() {
        let mut s = AxiaSwitcher::default();
        s.begin(&keys(&["a", "b", "c"]), "a", true);
        assert_eq!(s.index(), 2);
        s.step(1);
        assert_eq!(s.index(), 0);
        s.step(-1);
        assert_eq!(s.index(), 2);
    }

    #[test]
    fn closing_selected_app_keeps_a_valid_choice() {
        let mut s = AxiaSwitcher::default();
        s.begin(&keys(&["a", "b", "c"]), "a", false);
        s.remove("b");
        assert_eq!(s.items[s.selected].0, "c");
        s.remove("a");
        assert_eq!(s.index(), 0);
        s.remove("c");
        assert_eq!(s.index(), -1);
        s.step(-1);
    }

    #[test]
    fn duplicates_and_empty_lists() {
        let mut s = AxiaSwitcher::default();
        s.begin(&keys(&["a", "a"]), "a", false);
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.index(), 0);
        s.begin(&[], "", true);
        assert_eq!(s.index(), -1);
    }

    #[test]
    fn candidate_order_is_stable_until_commit() {
        let mut s = AxiaSwitcher::default();
        s.begin(&keys(&["a", "b"]), "a", false);
        s.focus("b");
        assert_eq!(s.items[0].0, "a");
    }

    #[test]
    fn history_is_bounded_and_pruned() {
        let mut s = AxiaSwitcher::default();
        for i in 0..1000 {
            s.focus(&i.to_string());
        }
        assert_eq!(s.recent.len(), LIMIT);
        s.begin(&keys(&["999"]), "999", false);
        assert_eq!(s.recent, vec!["999"]);
    }
}

mod scheduler;
