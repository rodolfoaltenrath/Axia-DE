//! Live notification permission and firewall checks against the host's stores.
use serde_json::{json, Value};
use std::ffi::CString;
extern "C" {
    fn lpk_valid_id(id: *const std::ffi::c_char) -> i32;
}

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

// None means this patch does not write a package permission. A failed attempt
// still refreshes the compatibility UI, as the existing public setter does.
pub(super) fn update(app: &str, source: &Source, patch: &Value) -> Option<bool> {
    let value = patch.get("allow").filter(|_| source.package)?;
    let allow = match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|n| n != 0.),
        Value::String(v) => !v.is_empty(),
        _ => true,
    };
    Some(write_state(app, if allow { 1 } else { 2 }))
}

fn write_state(app: &str, state: i32) -> bool {
    if !(0..=2).contains(&state) {
        return false;
    }
    let Ok(id) = CString::new(app) else {
        return false;
    };
    if unsafe { lpk_valid_id(id.as_ptr()) } == 0 {
        return false;
    }
    let saved = host::write(&id, state) == 0;
    if saved && state != 1 {
        host::revoke(&id);
    }
    saved
}

/// Shared with lord.perms.set: the C adapter borrows its validated JS string.
#[no_mangle]
pub unsafe extern "C" fn axia_notification_permission_set(
    app: *const std::ffi::c_char,
    state: i32,
) -> i32 {
    if app.is_null() {
        return 0;
    }
    std::ffi::CStr::from_ptr(app)
        .to_str()
        .ok()
        .is_some_and(|id| write_state(id, state)) as i32
}

#[cfg(not(test))]
mod host {
    use std::ffi::{c_char, CStr};
    extern "C" {
        fn perm_get(id: *const c_char, permission: i32) -> i32;
        fn restrict_blocks(id: *const c_char, flag: u32) -> i32;
        fn perm_set(id: *const c_char, permission: i32, state: i32) -> i32;
        fn brokersrv_revoke(id: *const c_char, permission: *const c_char);
    }
    pub fn permission(id: &CStr) -> i32 {
        unsafe { perm_get(id.as_ptr(), 2) }
    }
    pub fn blocked(id: &CStr) -> bool {
        unsafe { restrict_blocks(id.as_ptr(), 1 << 10) != 0 }
    }
    pub fn write(id: &CStr, state: i32) -> i32 {
        unsafe { perm_set(id.as_ptr(), 2, state) }
    }
    pub fn revoke(id: &CStr) {
        unsafe {
            brokersrv_revoke(id.as_ptr(), c"notifications".as_ptr());
        }
    }
}
#[cfg(test)]
pub(super) mod host {
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        ffi::CStr,
    };
    thread_local! {
        static PERMISSIONS:RefCell<BTreeMap<String,i32>>=RefCell::new(BTreeMap::new());
        static BLOCKED:RefCell<BTreeSet<String>>=RefCell::new(BTreeSet::new());
        static REVOKED:RefCell<Vec<String>>=const{RefCell::new(Vec::new())};
        static FAIL_WRITES:Cell<bool>=const{Cell::new(false)};
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
    pub fn write(id: &CStr, state: i32) -> i32 {
        if FAIL_WRITES.with(Cell::get) {
            return -1;
        }
        set_permission(id.to_str().unwrap(), state);
        0
    }
    pub fn revoke(id: &CStr) {
        REVOKED.with(|r| r.borrow_mut().push(id.to_str().unwrap().into()));
    }
    pub fn revoked() -> Vec<String> {
        REVOKED.with(|r| r.borrow().clone())
    }
    pub fn fail_writes(on: bool) {
        FAIL_WRITES.with(|v| v.set(on));
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
    #[test]
    fn permission_writes_validate_ids_and_revoke_only_after_a_successful_denial() {
        let package = Source::from(&json!({"package":true}));
        assert_eq!(
            update("notes", &package, &json!({"allow":true})),
            Some(true)
        );
        assert_eq!(permission("notes", &package), "allow");
        assert!(host::revoked().is_empty());
        assert_eq!(
            update("notes", &package, &json!({"allow":false})),
            Some(true)
        );
        assert_eq!(permission("notes", &package), "deny");
        assert_eq!(host::revoked(), ["notes"]);
        for id in ["n", "UPPER", "bad/id", "bad\0id", &"x".repeat(41)] {
            assert_eq!(update(id, &package, &json!({"allow":false})), Some(false));
        }
        assert_eq!(host::revoked(), ["notes"]);
        assert_eq!(
            update("clock", &Source::default(), &json!({"allow":false})),
            None
        );
        assert_eq!(update("notes", &package, &json!({"sound":false})), None);
    }
    #[test]
    fn failed_permission_writes_do_not_send_revocation() {
        let package = Source::from(&json!({"package":true}));
        host::fail_writes(true);
        assert_eq!(
            update("notes", &package, &json!({"allow":false})),
            Some(false)
        );
        assert!(host::revoked().is_empty());
    }
    #[test]
    fn public_permission_entry_preserves_ask_and_rejects_invalid_arguments() {
        let id = CString::new("notes").unwrap();
        unsafe {
            assert_eq!(axia_notification_permission_set(id.as_ptr(), 1), 1);
            assert!(host::revoked().is_empty());
            assert_eq!(axia_notification_permission_set(id.as_ptr(), 0), 1);
            assert_eq!(host::permission(&id), 0);
            assert_eq!(host::revoked(), ["notes"]);
            assert_eq!(axia_notification_permission_set(id.as_ptr(), 3), 0);
            assert_eq!(axia_notification_permission_set(std::ptr::null(), 1), 0);
            let invalid = CString::new(vec![0xff]).unwrap();
            assert_eq!(axia_notification_permission_set(invalid.as_ptr(), 1), 0);
        }
    }
}
