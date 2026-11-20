//! Live notification permission and firewall checks against the host's stores.
use serde_json::{json, Value};
use std::ffi::CString;

#[derive(Default, Clone)]
pub(super) struct Source {
    package: bool,
    isolated: bool,
}
impl Source {
    pub fn from(input: &Value) -> Self {
        Self {
            package: input["package"] == true,
            isolated: input["isolated"] == true,
        }
    }
    pub fn facts(&self) -> Value {
        json!({"package":self.package,"isolated":self.isolated})
    }
}
pub(super) fn permission(app: &str, source: &Source) -> Value {
    if !source.package {
        return Value::Null;
    }
    let state = CString::new(app)
        .ok()
        .map(|id| host::permission(&id))
        .unwrap_or(0);
    json!(match state {
        1 => "allow",
        2 => "deny",
        _ => "ask",
    })
}
pub(super) fn blocked(app: &str, source: &Source) -> bool {
    if app.is_empty() || app == "settings" || app.starts_with("linux:") {
        return false;
    }
    let scope = if source.package || source.isolated {
        format!("lpk:{app}")
    } else {
        "lordos".into()
    };
    CString::new(scope)
        .ok()
        .is_some_and(|id| host::blocked(&id))
}
pub(super) fn allowed(app: &str, source: &Source) -> bool {
    !blocked(app, source) && permission(app, source) != "deny"
}

#[cfg(not(test))]
mod host {
    use std::ffi::{c_char, CStr};
    extern "C" {
        fn perm_get(id: *const c_char, permission: i32) -> i32;
        fn restrict_blocks(id: *const c_char, flag: u32) -> i32;
    }
    pub fn permission(id: &CStr) -> i32 {
        unsafe { perm_get(id.as_ptr(), 2) }
    }
    pub fn blocked(id: &CStr) -> bool {
        unsafe { restrict_blocks(id.as_ptr(), 1 << 10) != 0 }
    }
}
#[cfg(test)]
pub(super) mod host {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        ffi::CStr,
    };
    thread_local! {
        static PERMISSIONS:RefCell<BTreeMap<String,i32>>=RefCell::new(BTreeMap::new());
        static BLOCKED:RefCell<BTreeSet<String>>=RefCell::new(BTreeSet::new());
    }
    pub fn permission(id: &CStr) -> i32 {
        PERMISSIONS.with(|p| p.borrow().get(id.to_str().unwrap()).copied().unwrap_or(0))
    }
    pub fn blocked(id: &CStr) -> bool {
        BLOCKED.with(|p| p.borrow().contains(id.to_str().unwrap()))
    }
    pub fn set_permission(id: &str, state: i32) {
        PERMISSIONS.with(|p| {
            p.borrow_mut().insert(id.into(), state);
        });
    }
    pub fn set_blocked(id: &str, on: bool) {
        BLOCKED.with(|p| {
            if on {
                p.borrow_mut().insert(id.into());
            } else {
                p.borrow_mut().remove(id);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packages_use_live_permissions_and_other_apps_keep_their_own_preferences() {
        let package = Source::from(&json!({"package":true}));
        host::set_permission("notes", 2);
        assert_eq!(permission("notes", &package), "deny");
        assert_eq!(permission("notes", &Source::default()), Value::Null);
        host::set_permission("notes", 1);
        assert_eq!(permission("notes", &package), "allow");
        host::set_permission("notes", 0);
        assert_eq!(permission("notes", &package), "ask");
    }
    #[test]
    fn firewall_scope_comes_from_app_identity_and_keeps_system_exemptions() {
        let builtin = Source::default();
        let package = Source::from(&json!({"package":true}));
        let isolated = Source::from(&json!({"isolated":true}));
        host::set_blocked("lordos", true);
        assert!(blocked("clock", &builtin));
        assert!(!blocked("notes", &package));
        for exempt in ["", "settings", "linux:notes"] {
            assert!(!blocked(exempt, &builtin));
        }
        host::set_blocked("lpk:notes", true);
        assert!(blocked("notes", &package));
        assert!(blocked("notes", &isolated));
        assert!(!blocked("other", &package));
    }
}
