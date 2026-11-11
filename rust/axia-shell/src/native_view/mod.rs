//! Native views: compiled apps whose widget trees Rust mounts straight into the
//! runtime scene, with their own input, widgets, captions and refresh timer.
//! React is not involved: the shell only gives each view a mount node inside the
//! window manager's frame, its facts (settings, theme, services) and runs the
//! effects that belong to the shell (open an app, save settings, firewall calls,
//! clipboard). The compositor delivers pointer events here before JS sees them
//! (see `server/src/ui/js_nativeview.c`).
mod host;
pub mod i18n;
pub mod paint;
pub mod style;
#[cfg(test)]
mod tests;
pub use host::Host;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
};
/// What a native view needs from an app: the request interface of compiled apps
/// and the RGBA pixels of the rasters its trees name.
pub trait NativeApp {
    fn request(&mut self, input: &Value) -> Value;
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])>;
    /// Optional window-level keyboard input (e.g. Calculator). Fields keep
    /// their existing editing path; None leaves the key with the shell.
    fn key(&mut self, _key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        None
    }
}
fn app(id: &str) -> Option<Box<dyn NativeApp>> {
    match id {
        crate::native_apps::monitor::ID => Some(Box::new(crate::monitor::App::default())),
        crate::native_apps::calculator::ID => Some(Box::new(crate::calculator::App::default())),
        crate::native_apps::store::ID => Some(Box::new(crate::store::App::default())),
        _ => None,
    }
}
/// The icon tree a native-view app draws for itself (another app can show it).
pub fn icon_tree(id: &str, size: f64) -> Option<Value> {
    let mut app = app(id)?;
    let tree = app.request(&serde_json::json!({"cmd":"icon","size":size}));
    (!tree.is_null()).then_some(tree)
}
#[derive(Default)]
struct Registry {
    assets: String,
    home: Option<String>,
    next: u32,
    hosts: BTreeMap<u32, Host>,
}
thread_local! { static VIEWS: RefCell<Registry> = RefCell::new(Registry::default()); }
fn with<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    VIEWS.with(|r| f(&mut r.borrow_mut()))
}
fn with_host<T>(handle: u32, f: impl FnOnce(&mut Host) -> T) -> Option<T> {
    // A host is taken out while it runs: an app request cannot reach the registry.
    let mut host = with(|r| r.hosts.remove(&handle))?;
    let out = f(&mut host);
    with(|r| r.hosts.insert(handle, host));
    Some(out)
}
unsafe fn text<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        CStr::from_ptr(p).to_str().unwrap_or("")
    }
}
unsafe fn json(p: *const c_char) -> Value {
    // Facts and results are bounded like native app requests.
    let s = text(p);
    if s.len() > 4 << 20 {
        return Value::Null;
    }
    serde_json::from_str(s).unwrap_or(Value::Null)
}
fn owned(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map_or(std::ptr::null_mut(), CString::into_raw)
}
#[no_mangle]
pub unsafe extern "C" fn axia_view_init(assets: *const c_char, home: *const c_char) {
    let (assets, home) = (text(assets).to_owned(), text(home).to_owned());
    with(|r| {
        r.assets = assets;
        r.home = (!home.is_empty()).then_some(home);
    });
}
/// Mounts app `id` under the JS node `node`; 0 if the app has no native view.
#[no_mangle]
pub unsafe extern "C" fn axia_view_mount(
    node: i32,
    id: *const c_char,
    facts: *const c_char,
) -> u32 {
    let Some(app) = app(text(id)) else {
        return 0;
    };
    if !crate::runtime::native::exists(node) {
        return 0;
    }
    let facts = json(facts);
    let (assets, home) = with(|r| (r.assets.clone(), r.home.clone()));
    let mut host = Host::new(node, app, &assets, home.as_deref());
    host.facts(facts);
    with(|r| {
        r.next = r.next.wrapping_add(1).max(1);
        let handle = r.next;
        r.hosts.insert(handle, host);
        handle
    })
}
#[no_mangle]
pub extern "C" fn axia_view_unmount(handle: u32) {
    let host = with(|r| r.hosts.remove(&handle));
    // Dropped outside the registry: the app and its nodes go now.
    drop(host);
}
#[no_mangle]
pub unsafe extern "C" fn axia_view_facts(handle: u32, facts: *const c_char) {
    let facts = json(facts);
    with_host(handle, |h| h.facts(facts));
}
#[no_mangle]
pub extern "C" fn axia_view_focus(handle: u32, focused: i32) {
    with_host(handle, |h| h.window_focus(focused != 0));
}
/// The effects waiting for the shell, as a JSON array (free with `axia_view_free`).
#[no_mangle]
pub extern "C" fn axia_view_take(handle: u32) -> *mut c_char {
    with_host(handle, |h| h.take())
        .flatten()
        .map_or(std::ptr::null_mut(), |v| owned(v.to_string()))
}
#[no_mangle]
pub unsafe extern "C" fn axia_view_free(p: *mut c_char) {
    if !p.is_null() {
        drop(CString::from_raw(p));
    }
}
/// The result of an app effect run by the shell; the app may answer with another.
#[no_mangle]
pub unsafe extern "C" fn axia_view_result(handle: u32, result: *const c_char) {
    let result = json(result);
    with_host(handle, |h| h.result(result));
}
#[no_mangle]
pub unsafe extern "C" fn axia_view_paste(handle: u32, value: *const c_char) {
    let value = text(value).to_owned();
    with_host(handle, |h| h.paste(&value));
}
/// Text shown by the view (tests and accessibility).
#[no_mangle]
pub extern "C" fn axia_view_text(handle: u32) -> *mut c_char {
    with_host(handle, |h| owned(h.text())).unwrap_or(std::ptr::null_mut())
}
fn handles() -> Vec<u32> {
    with(|r| r.hosts.keys().copied().collect())
}
/// A pointer event on `target`. Views handle it when it belongs to one of them (or
/// a press/drag they started); JS still receives it, on the view's mount node, so
/// window focus and the shell's listeners keep working. Returns JS's target.
#[no_mangle]
pub unsafe extern "C" fn axia_view_pointer(
    kind: *const c_char,
    target: i32,
    x: f32,
    y: f32,
    button: i32,
) -> i32 {
    let kind = text(kind).to_owned();
    let mut out = target;
    for handle in handles() {
        if let Some(Some(js)) = with_host(handle, |h| h.pointer(&kind, target, x, y, button)) {
            out = js;
        }
    }
    out
}
/// Touchpad gestures: a native scroll view takes `scroll`; JS gets the mount node.
#[no_mangle]
pub unsafe extern "C" fn axia_view_gesture(
    kind: *const c_char,
    phase: *const c_char,
    target: i32,
    dx: f64,
    dy: f64,
) -> i32 {
    let (kind, phase) = (text(kind).to_owned(), text(phase).to_owned());
    let mut out = target;
    for handle in handles() {
        if let Some(Some(js)) = with_host(handle, |h| h.gesture(&kind, &phase, target, dx, dy)) {
            out = js;
        }
    }
    out
}
/// A key after the shell's shortcuts: the focused native text field takes it.
#[no_mangle]
pub unsafe extern "C" fn axia_view_key(key: *const c_char, chr: *const c_char, flags: i32) -> i32 {
    let (key, chr) = (text(key).to_owned(), text(chr).to_owned());
    for handle in handles() {
        if with_host(handle, |h| h.key(&key, &chr, flags)) == Some(true) {
            return 1;
        }
    }
    0
}
#[no_mangle]
pub extern "C" fn axia_view_tick(now: f64) {
    for handle in handles() {
        with_host(handle, |h| h.tick(now));
    }
}
/// Milliseconds until a view needs a tick, or -1.
#[no_mangle]
pub extern "C" fn axia_view_timeout(now: f64) -> f64 {
    with(|r| {
        r.hosts
            .values()
            .filter_map(|h| h.deadline())
            .map(|d| (d - now).max(0.))
            .reduce(f64::min)
            .unwrap_or(-1.)
    })
}
/// Effects are waiting for the shell (it calls `__lordNativeView`).
#[no_mangle]
pub extern "C" fn axia_view_pending() -> i32 {
    with(|r| i32::from(r.hosts.values().any(Host::pending)))
}
