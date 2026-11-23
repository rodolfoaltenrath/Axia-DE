//! Physical shell commands. No JS key event or JSON request crosses this path.
use axia_state::{Reader, Subscription};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) enum Claim { Pass, Consumed, Active, Open { hold: i32, back: bool } }
pub(super) struct Input {
    preferences: Reader<Value>,
    changes: Subscription,
    linux: bool,
    taken: BTreeSet<i32>,
    held_modifiers: i32,
}
impl Input {
    pub(super) fn new(home: &str) -> Self {
        let home = crate::settings::profile(home);
        let preferences = crate::settings::with(&home, |settings|settings.reader());
        let linux = preferences.read()["shortcutProfile"] == "linux";
        let changes = preferences.subscribe();
        Self { preferences, changes, linux, taken: BTreeSet::new(), held_modifiers: 0 }
    }
    pub(super) fn pending(&self) -> bool { self.changes.pending() }
    pub(super) fn sync(&mut self) -> bool {
        self.changes.take();
        let linux = self.preferences.read()["shortcutProfile"] == "linux";
        let changed = self.linux != linux;
        self.linux = linux;
        changed
    }
    pub(super) fn sync_grabs(&self) {
        crate::keygrab::sync_native(self.linux, self.preferences.read()["desktopArrows"] != false);
    }
    pub(super) fn modifier(code: i32) -> bool { Self::modifier_mask(code) != 0 }
    fn modifier_mask(code: i32) -> i32 {
        match code { 42 | 54 => 1, 29 | 97 => 2, 56 => 4, 100 => 4 | 8, 125 | 126 => 16, _ => 0 }
    }
    pub(super) fn claim(&mut self, active: bool, code: i32, value: i32, key: &str, flags: i32, enabled: bool) -> Claim {
        let mask = Self::modifier_mask(code);
        let owned_key = self.taken.contains(&code);
        let owned_modifier = self.held_modifiers & mask != 0;
        if value == 0 {
            self.taken.remove(&code);
            self.held_modifiers &= flags | !mask;
        } else if value == 1 {
            // A fresh press after a lost release is a new gesture, never a repeat.
            self.taken.remove(&code);
        }
        if active {
            if value != 0 { self.taken.insert(code); }
            self.held_modifiers |= flags & 31;
            return Claim::Active;
        }
        if (owned_key && value != 1) || owned_modifier {
            if value != 0 { self.taken.insert(code); }
            return Claim::Consumed;
        }
        let mods = flags & 31;
        let hold = match mods & !1 {
            16 => Some(8),
            4 if self.linux => Some(2),
            _ => None,
        };
        if enabled && code == 15 && value == 1 {
            if let Some(hold) = hold {
                self.taken.insert(code);
                self.held_modifiers |= mods;
                return Claim::Open { hold, back: mods & 1 != 0 };
            }
        }
        Claim::Pass
    }
}
