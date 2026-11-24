//! Exercises the Rust NetworkManager, brightness (logind) and StatusNotifier contracts on an
//! isolated session bus: a fake NetworkManager, a fake logind and a fake tray application answer the
//! `desktop_bus_contract_*` tests of axia-shell, which talk to them as if the session bus were the
//! system bus. No real network, password or running application is changed.
//!
//!   cargo xtask network-tray-check
//!
//! Build the axia-shell tests first (`cargo test -p axia-shell --lib --no-run` into
//! server/build/rust), then run (the command starts its own `dbus-run-session`). Needs glib/gio
//! at run time (loaded with dlopen, so xtask itself does not link them) and `dbus-daemon`.

use crate::{at, Result};
use std::collections::HashSet;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const NM: &str = "org.freedesktop.NetworkManager";
const BASE: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "/org/freedesktop/NetworkManager/Devices/1";
const PROFILE: &str = "/org/freedesktop/NetworkManager/Settings/1";
const ACTIVE: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
const WATCH: &str = "org.kde.StatusNotifierWatcher";
const AP1: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";

type Ptr = *mut c_void;
type MethodCall = unsafe extern "C" fn(Ptr, *const c_char, *const c_char, *const c_char, *const c_char, Ptr, Ptr, Ptr);
type GetProperty = unsafe extern "C" fn(Ptr, *const c_char, *const c_char, *const c_char, *const c_char, *mut Ptr, Ptr) -> Ptr;

#[repr(C)]
struct VTable {
    method_call: Option<MethodCall>,
    get_property: Option<GetProperty>,
    set_property: Option<Ptr>,
    padding: [Ptr; 8],
}

#[repr(C)]
struct NodeInfo {
    ref_count: c_int,
    path: *mut c_char,
    interfaces: *mut Ptr,
}

macro_rules! library {
    ($($name:ident: $ty:ty),* $(,)?) => {
        struct Lib { $($name: $ty),* }
        impl Lib {
            fn load() -> Result<Self> {
                let open = |name: &str| -> Result<Ptr> {
                    let c = CString::new(name).unwrap();
                    // SAFETY: a plain dlopen of a system library by name.
                    let handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
                    if handle.is_null() { Err(format!("{name} is not installed")) } else { Ok(handle) }
                };
                let (glib, gio) = (open("libglib-2.0.so.0")?, open("libgio-2.0.so.0")?);
                Ok(Self { $($name: {
                    let c = CString::new(stringify!($name)).unwrap();
                    // SAFETY: the symbol is looked up by name and cast to its documented signature.
                    let mut p = unsafe { libc::dlsym(gio, c.as_ptr()) };
                    if p.is_null() { p = unsafe { libc::dlsym(glib, c.as_ptr()) }; }
                    if p.is_null() { return Err(format!("missing symbol {}", stringify!($name))); }
                    unsafe { std::mem::transmute::<Ptr, $ty>(p) }
                }),* })
            }
        }
    };
}

library! {
    g_variant_new_parsed: unsafe extern "C" fn(*const c_char, ...) -> Ptr,
    g_variant_print: unsafe extern "C" fn(Ptr, c_int) -> *mut c_char,
    g_variant_unref: unsafe extern "C" fn(Ptr),
    g_variant_get_type_string: unsafe extern "C" fn(Ptr) -> *const c_char,
    g_variant_get_string: unsafe extern "C" fn(Ptr, *mut usize) -> *const c_char,
    g_variant_get_uint32: unsafe extern "C" fn(Ptr) -> u32,
    g_variant_get_fixed_array: unsafe extern "C" fn(Ptr, *mut usize, usize) -> *const c_void,
    g_variant_get_child_value: unsafe extern "C" fn(Ptr, usize) -> Ptr,
    g_variant_lookup_value: unsafe extern "C" fn(Ptr, *const c_char, Ptr) -> Ptr,
    g_free: unsafe extern "C" fn(Ptr),
    g_error_free: unsafe extern "C" fn(Ptr),
    g_main_context_default: unsafe extern "C" fn() -> Ptr,
    g_main_context_pending: unsafe extern "C" fn(Ptr) -> c_int,
    g_main_context_iteration: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    g_idle_add: unsafe extern "C" fn(unsafe extern "C" fn(Ptr) -> c_int, Ptr) -> u32,
    g_timeout_add: unsafe extern "C" fn(u32, unsafe extern "C" fn(Ptr) -> c_int, Ptr) -> u32,
    g_bus_get_sync: unsafe extern "C" fn(c_int, Ptr, *mut Ptr) -> Ptr,
    g_dbus_connection_call_sync: unsafe extern "C" fn(Ptr, *const c_char, *const c_char, *const c_char, *const c_char, Ptr, Ptr, c_int, c_int, Ptr, *mut Ptr) -> Ptr,
    g_dbus_connection_call: unsafe extern "C" fn(Ptr, *const c_char, *const c_char, *const c_char, *const c_char, Ptr, Ptr, c_int, c_int, Ptr, Ptr, Ptr),
    g_dbus_node_info_new_for_xml: unsafe extern "C" fn(*const c_char, *mut Ptr) -> *mut NodeInfo,
    g_dbus_connection_register_object: unsafe extern "C" fn(Ptr, *const c_char, Ptr, *const VTable, Ptr, Ptr, *mut Ptr) -> u32,
    g_dbus_method_invocation_return_value: unsafe extern "C" fn(Ptr, Ptr),
    g_dbus_method_invocation_return_dbus_error: unsafe extern "C" fn(Ptr, *const c_char, *const c_char),
    g_dbus_connection_new_for_address_sync: unsafe extern "C" fn(*const c_char, c_int, Ptr, Ptr, *mut Ptr) -> Ptr,
    g_dbus_connection_flush_sync: unsafe extern "C" fn(Ptr, Ptr, *mut Ptr) -> c_int,
    g_dbus_connection_close_sync: unsafe extern "C" fn(Ptr, Ptr, *mut Ptr) -> c_int,
}

/// What the fake services saw; the callbacks reach it through `user_data`.
struct State {
    lib: Lib,
    records: HashSet<String>,
    failures: Vec<String>,
    wired: bool,
    registered: bool,
    bus: Ptr,
    app: Ptr,
}

fn cs(text: &str) -> CString {
    CString::new(text).expect("no NUL in a fixed text")
}

fn text_of(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: GLib hands out NUL-terminated strings.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

impl State {
    /// A (floating) variant parsed from the GVariant text format.
    fn parse(&self, text: &str) -> Ptr {
        let c = cs(text);
        // SAFETY: the text is a constant of this file; no arguments are needed.
        let v = unsafe { (self.lib.g_variant_new_parsed)(c.as_ptr()) };
        assert!(!v.is_null(), "bad variant text: {text}");
        v
    }
    fn print(&self, v: Ptr) -> String {
        // SAFETY: `v` is a live variant; the string is freed right after copying.
        unsafe {
            let p = (self.lib.g_variant_print)(v, 0);
            let s = text_of(p);
            (self.lib.g_free)(p as Ptr);
            s
        }
    }
    /// Does the arguments' text equal the text of a variant written in the text format?
    fn is(&self, params: Ptr, expected: &str) -> bool {
        let want = self.parse(expected);
        let same = self.print(params) == self.print(want);
        // SAFETY: floating, never sunk elsewhere.
        unsafe { (self.lib.g_variant_unref)(want) };
        same
    }
    fn lookup(&self, dict: Ptr, key: &str) -> Option<Ptr> {
        let k = cs(key);
        // SAFETY: a live dictionary variant.
        let v = unsafe { (self.lib.g_variant_lookup_value)(dict, k.as_ptr(), std::ptr::null_mut()) };
        (!v.is_null()).then_some(v)
    }
    fn string_in(&self, dict: Ptr, section: &str, key: &str) -> Option<String> {
        let sub = self.lookup(dict, section)?;
        let v = self.lookup(sub, key)?;
        // SAFETY: `v` is a string variant when the key is one of the string fields.
        Some(text_of(unsafe { (self.lib.g_variant_get_string)(v, std::ptr::null_mut()) }))
    }
    fn child(&self, params: Ptr, i: usize) -> Ptr {
        // SAFETY: `params` is a tuple with that child.
        unsafe { (self.lib.g_variant_get_child_value)(params, i) }
    }
    fn reply(&self, inv: Ptr, text: Option<&str>) {
        let v = text.map_or(std::ptr::null_mut(), |t| self.parse(t));
        // SAFETY: the invocation takes the floating reply.
        unsafe { (self.lib.g_dbus_method_invocation_return_value)(inv, v) };
    }
    fn fail(&self, inv: Ptr, name: &str, message: &str) {
        let (n, m) = (cs(name), cs(message));
        // SAFETY: both strings outlive the call.
        unsafe { (self.lib.g_dbus_method_invocation_return_dbus_error)(inv, n.as_ptr(), m.as_ptr()) };
    }
}

fn ssid_text() -> String {
    "[byte 0x43, 0x61, 0x66, 0xc3, 0xa9, 0x3a, 0x20, 0x63, 0x61, 0x73, 0x61]".into() // "Café: casa"
}

/// The properties of one interface of one object, as an `a{sv}` reply text.
fn props(wired: bool, path: &str, interface: &str) -> String {
    let devices = format!("{BASE}/Devices/2");
    let wired_active = format!("{BASE}/ActiveConnection/2");
    let body = if path == BASE && interface == NM {
        format!(
            "'WirelessEnabled': <true>, 'WirelessHardwareEnabled': <true>, 'Connectivity': <uint32 4>, 'Devices': <[objectpath '{DEVICE}', objectpath '{devices}']>, 'PrimaryConnection': <objectpath '{}'>",
            if wired { &wired_active } else { ACTIVE }
        )
    } else if path == devices && interface == format!("{NM}.Device") {
        format!("'DeviceType': <uint32 1>, 'State': <uint32 {}>, 'ActiveConnection': <objectpath '{wired_active}'>", if wired { 100 } else { 20 })
    } else if path == DEVICE && interface == format!("{NM}.Device") {
        format!("'DeviceType': <uint32 2>, 'State': <uint32 100>, 'ActiveConnection': <objectpath '{ACTIVE}'>, 'AvailableConnections': <[objectpath '{PROFILE}']>")
    } else if path == DEVICE && interface == format!("{NM}.Device.Wireless") {
        format!("'AccessPoints': <[objectpath '{BASE}/AccessPoint/1', objectpath '{BASE}/AccessPoint/2', objectpath '{BASE}/AccessPoint/3']>, 'ActiveAccessPoint': <objectpath '{AP1}'>")
    } else if let Some(index) = path.strip_prefix(&format!("{BASE}/AccessPoint/")) {
        let index: u32 = index.parse().unwrap_or(1);
        let strength = [81, 95, 40][(index.clamp(1, 3) - 1) as usize];
        format!(
            "'Ssid': <{}>, 'Flags': <uint32 {}>, 'WpaFlags': <uint32 0>, 'RsnFlags': <uint32 {}>, 'Strength': <byte {strength}>",
            ssid_text(),
            u32::from(index != 3),
            if index != 3 { 0x100 } else { 0 }
        )
    } else if path == ACTIVE {
        "'State': <uint32 2>".into()
    } else {
        String::new()
    };
    if body.is_empty() { "({},)".into() } else { format!("({{{body}}},)") }
}

unsafe fn state<'a>(data: Ptr) -> &'a mut State {
    &mut *(data as *mut State)
}

/// The fake NetworkManager: one handler for every object and interface.
unsafe extern "C" fn nm_call(_: Ptr, _: *const c_char, path: *const c_char, interface: *const c_char, method: *const c_char, params: Ptr, inv: Ptr, data: Ptr) {
    let st = state(data);
    let (path, interface, method) = (text_of(path), text_of(interface), text_of(method));
    let outcome: std::result::Result<(), String> = (|| {
        match method.as_str() {
            "TestWired" | "TestWifiOnly" => {
                st.wired = method == "TestWired";
                st.records.insert(if st.wired { "wired" } else { "unplug" }.into());
                st.reply(inv, None);
            }
            "GetAll" if interface == "org.freedesktop.DBus.Properties" => {
                let asked = st.child(params, 0);
                let name = text_of((st.lib.g_variant_get_string)(asked, std::ptr::null_mut()));
                st.reply(inv, Some(&props(st.wired, &path, &name)));
            }
            "Set" if interface == "org.freedesktop.DBus.Properties" => {
                if !st.is(params, &format!("('{NM}', 'WirelessEnabled', <false>)")) {
                    return Err("Set".into());
                }
                st.records.insert("toggle".into());
                st.reply(inv, None);
            }
            "GetSettings" => st.reply(
                inv,
                Some(&format!(
                    "({{'connection': {{'id': <'Perfil preservado'>, 'type': <'802-11-wireless'>}}, '802-11-wireless': {{'ssid': <{}>}}, '802-11-wireless-security': {{'key-mgmt': <'wpa-psk'>}}, 'ipv4': {{'method': <'manual'>}}}},)",
                    ssid_text()
                )),
            ),
            "Update" => {
                let settings = st.child(params, 0);
                let psk_flags = st.lookup(settings, "802-11-wireless-security").and_then(|s| st.lookup(s, "psk-flags")).map(|v| (st.lib.g_variant_get_uint32)(v));
                let good = st.string_in(settings, "ipv4", "method").as_deref() == Some("manual")
                    && st.string_in(settings, "connection", "id").as_deref() == Some("Perfil preservado")
                    && st.string_in(settings, "802-11-wireless-security", "psk").as_deref() == Some("fixture-password")
                    && psk_flags == Some(0);
                if !good {
                    return Err("Update".into());
                }
                st.records.insert("saved-password-update".into());
                st.reply(inv, None);
            }
            "RequestScan" => {
                if text_of((st.lib.g_variant_get_type_string)(params)) != "(a{sv})" {
                    return Err("RequestScan".into());
                }
                st.records.insert("scan".into());
                st.reply(inv, None);
            }
            "ActivateConnection" => {
                if !st.is(params, &format!("(objectpath '{PROFILE}', objectpath '{DEVICE}', objectpath '{AP1}')")) {
                    return Err("ActivateConnection".into());
                }
                st.records.insert("saved-activation".into());
                st.reply(inv, Some(&format!("(objectpath '{ACTIVE}',)")));
            }
            "AddAndActivateConnection" => {
                let settings = st.child(params, 0);
                let ssid = st.lookup(settings, "802-11-wireless").and_then(|s| st.lookup(s, "ssid")).map(|v| {
                    let mut n = 0usize;
                    let p = (st.lib.g_variant_get_fixed_array)(v, &mut n, 1) as *const u8;
                    String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned()
                });
                let device = text_of((st.lib.g_variant_get_string)(st.child(params, 1), std::ptr::null_mut()));
                let ap = text_of((st.lib.g_variant_get_string)(st.child(params, 2), std::ptr::null_mut()));
                let good = ssid.as_deref() == Some("Nova: rede")
                    && st.string_in(settings, "802-11-wireless-security", "key-mgmt").as_deref() == Some("wpa-psk")
                    && st.string_in(settings, "802-11-wireless-security", "psk").as_deref() == Some("fixture-password")
                    && device == DEVICE
                    && ap == AP1;
                if !good {
                    return Err("AddAndActivateConnection".into());
                }
                st.records.insert("new-activation".into());
                st.reply(inv, Some(&format!("(objectpath '{PROFILE}', objectpath '{ACTIVE}')")));
            }
            "Denied" => {
                st.fail(inv, &format!("{NM}.PermissionDenied"), "fixture denied");
                st.records.insert("denied".into());
            }
            other => return Err(other.to_string()),
        }
        Ok(())
    })();
    if let Err(what) = outcome {
        st.failures.push(format!("{method}: AssertionError ({what})"));
        st.fail(inv, "org.axia.Test.Failed", "AssertionError");
    }
}

/// The fake logind: the brightness service asks it, never a root helper or the real backlight.
unsafe extern "C" fn login_call(_: Ptr, _: *const c_char, _: *const c_char, _: *const c_char, method: *const c_char, params: Ptr, inv: Ptr, data: Ptr) {
    let st = state(data);
    let method = text_of(method);
    let outcome: std::result::Result<(), String> = (|| {
        match method.as_str() {
            "GetSessionByPID" => {
                if (st.lib.g_variant_get_uint32)(st.child(params, 0)) == 0 {
                    return Err("pid".into());
                }
                st.records.insert("brightness-session".into());
                st.reply(inv, Some("(objectpath '/org/freedesktop/login1/session/test',)"));
            }
            "SetBrightness" => {
                let text = st.print(params);
                if !text.starts_with("('backlight', 'intel_backlight', ") {
                    return Err("device".into());
                }
                if text.ends_with(", 999)") {
                    st.records.insert("brightness-denied".into());
                    st.fail(inv, "org.freedesktop.DBus.Error.AccessDenied", "fixture");
                } else if text.ends_with(", 397)") {
                    st.records.insert("brightness-write".into());
                    st.reply(inv, None);
                } else {
                    return Err("value".into());
                }
            }
            other => return Err(other.to_string()),
        }
        Ok(())
    })();
    if let Err(what) = outcome {
        st.failures.push(format!("{method}: AssertionError ({what})"));
        st.fail(inv, "org.axia.Test.Failed", "AssertionError");
    }
}

unsafe extern "C" fn release_item(data: Ptr) -> c_int {
    let st = state(data);
    (st.lib.g_dbus_connection_flush_sync)(st.app, std::ptr::null_mut(), std::ptr::null_mut());
    (st.lib.g_dbus_connection_close_sync)(st.app, std::ptr::null_mut(), std::ptr::null_mut());
    st.records.insert("tray-disconnect".into());
    0
}

/// The fake tray application: a StatusNotifierItem and its dbusmenu.
unsafe extern "C" fn tray_call(_: Ptr, _: *const c_char, _: *const c_char, _: *const c_char, method: *const c_char, params: Ptr, inv: Ptr, data: Ptr) {
    let st = state(data);
    let method = text_of(method);
    let outcome: std::result::Result<(), String> = (|| {
        match method.as_str() {
            "Activate" => {
                if !st.is(params, "(12, 34)") {
                    return Err("Activate".into());
                }
                st.records.insert("tray-activate".into());
                st.reply(inv, None);
            }
            "AboutToShow" => {
                st.records.insert("tray-about-to-show".into());
                st.reply(inv, Some("(false,)"));
            }
            "GetLayout" => {
                if !st.is(params, "(0, 4, @as [])") {
                    return Err("GetLayout".into());
                }
                st.records.insert("tray-menu-layout".into());
                st.reply(inv, Some("(uint32 1, (0, @a{sv} {}, [<(1, {'label': <'_Abrir janela'>}, @av [])>, <(2, {'label': <'_Sair'>}, @av [])>]))"));
            }
            "Event" => {
                let printed = st.print(params);
                if !printed.starts_with("(2, 'clicked', <") || !printed.ends_with(">, uint32 0)") && !printed.ends_with(">, 0)") {
                    return Err(format!("Event {printed}"));
                }
                st.records.insert("tray-menu-event".into());
                st.reply(inv, None);
                (st.lib.g_idle_add)(release_item, data);
            }
            other => return Err(other.to_string()),
        }
        Ok(())
    })();
    if let Err(what) = outcome {
        st.failures.push(format!("{method}: AssertionError ({what})"));
        st.fail(inv, "org.axia.Test.Failed", "AssertionError");
    }
}

unsafe extern "C" fn tray_prop(_: Ptr, _: *const c_char, _: *const c_char, _: *const c_char, name: *const c_char, _: *mut Ptr, data: Ptr) -> Ptr {
    let st = state(data);
    let pixels = (0..4).map(|_| "0xff, 0x5a, 0x64, 0xf0").collect::<Vec<_>>().join(", ");
    let text = match text_of(name).as_str() {
        "Id" => "'discord-test'".to_string(),
        "Title" => "'Discord de teste'".to_string(),
        "Status" => "'Active'".to_string(),
        "Menu" => "objectpath '/Menu'".to_string(),
        "ItemIsMenu" => "false".to_string(),
        "IconName" => "''".to_string(),
        "IconPixmap" => format!("@a(iiay) [(2, 2, [byte {pixels}])]"),
        _ => return std::ptr::null_mut(),
    };
    st.parse(&text)
}

unsafe extern "C" fn register_app(data: Ptr) -> c_int {
    let st = state(data);
    if st.registered {
        return 0;
    }
    let mut err: Ptr = std::ptr::null_mut();
    let args = st.parse(&format!("('{WATCH}',)"));
    let reply = (st.lib.g_dbus_connection_call_sync)(
        st.bus, cs("org.freedesktop.DBus").as_ptr(), cs("/org/freedesktop/DBus").as_ptr(), cs("org.freedesktop.DBus").as_ptr(), cs("NameHasOwner").as_ptr(), args, std::ptr::null_mut(), 0, 1000, std::ptr::null_mut(), &mut err,
    );
    if reply.is_null() {
        (st.lib.g_error_free)(err);
        return 1;
    }
    if st.print(reply).contains("true") {
        let item = st.parse("('/StatusNotifierItem',)");
        (st.lib.g_dbus_connection_call)(
            st.app, cs(WATCH).as_ptr(), cs("/StatusNotifierWatcher").as_ptr(), cs(WATCH).as_ptr(), cs("RegisterStatusNotifierItem").as_ptr(), item, std::ptr::null_mut(), 0, 1000, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
        );
        st.registered = true;
    }
    c_int::from(!st.registered)
}

const NM_XML: &str = r#"<node><interface name="org.freedesktop.DBus.Properties">
<method name="GetAll"><arg type="s" direction="in"/><arg type="a{sv}" direction="out"/></method>
<method name="Set"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/></method></interface>
<interface name="org.freedesktop.NetworkManager">
<method name="ActivateConnection"><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/></method>
<method name="AddAndActivateConnection"><arg type="a{sa{sv}}" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/><arg type="o" direction="out"/></method>
<method name="Denied"/><method name="TestWired"/><method name="TestWifiOnly"/></interface>
<interface name="org.freedesktop.NetworkManager.Device.Wireless"><method name="RequestScan"><arg type="a{sv}" direction="in"/></method></interface>
<interface name="org.freedesktop.NetworkManager.Settings.Connection"><method name="GetSettings"><arg type="a{sa{sv}}" direction="out"/></method><method name="Update"><arg type="a{sa{sv}}" direction="in"/></method></interface></node>"#;
const LOGIN_XML: &str = r#"<node><interface name="org.freedesktop.login1.Manager">
<method name="GetSessionByPID"><arg type="u" direction="in"/><arg type="o" direction="out"/></method>
</interface><interface name="org.freedesktop.login1.Session">
<method name="SetBrightness"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="u" direction="in"/></method>
</interface></node>"#;
const TRAY_XML: &str = r#"<node><interface name="org.kde.StatusNotifierItem">
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<property name="Id" type="s" access="read"/><property name="Title" type="s" access="read"/>
<property name="Status" type="s" access="read"/><property name="Menu" type="o" access="read"/>
<property name="ItemIsMenu" type="b" access="read"/><property name="IconName" type="s" access="read"/>
<property name="IconPixmap" type="a(iiay)" access="read"/></interface>
<interface name="com.canonical.dbusmenu"><method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method></interface></node>"#;

/// The newest `axia_shell-*` test executable under `debug`. Older cargo leaves it in `deps/`; newer
/// cargo (split build directory) keeps it in `build/axia-shell/<hash>/out/`, so both are searched.
fn newest_test_binary(debug: &Path) -> Result<PathBuf> {
    let mut folders = vec![debug.join("deps")];
    for hashed in super::entries(&debug.join("build/axia-shell")) {
        folders.push(hashed.path().join("out"));
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for folder in &folders {
        for entry in super::entries(folder) {
            let (name, path) = (entry.file_name().to_string_lossy().into_owned(), entry.path());
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            let executable = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0;
            if name.starts_with("axia_shell-") && path.extension().is_none() && path.is_file() && executable {
                let modified = meta.modified().map_err(|e| e.to_string())?;
                if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                    best = Some((modified, path));
                }
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| format!("no axia_shell test binary in {}: build it first", debug.display()))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    // always create our own bus, even if invoked from a real desktop
    if args != ["--private"] {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let status = Command::new("dbus-run-session").arg("--").arg(exe).args(["network-tray-check", "--private"]).status().map_err(|e| format!("dbus-run-session: {e}"))?;
        return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
    }
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| "Run this check through dbus-run-session.".to_string())?;
    let mut state = Box::new(State { lib: Lib::load()?, records: HashSet::new(), failures: Vec::new(), wired: false, registered: false, bus: std::ptr::null_mut(), app: std::ptr::null_mut() });
    let data: Ptr = &mut *state as *mut State as Ptr;
    let lib = &state.lib;
    let mut err: Ptr = std::ptr::null_mut();
    // SAFETY: plain GIO calls with valid arguments; the vtables and the state outlive the main loop below.
    unsafe {
        let bus = (lib.g_bus_get_sync)(2, std::ptr::null_mut(), &mut err);
        if bus.is_null() {
            return Err("cannot connect to the session bus".into());
        }
        let ask = |name: &str| {
            let args = state.parse(&format!("('{name}', uint32 4)"));
            let (s, p, i, m) = (cs("org.freedesktop.DBus"), cs("/org/freedesktop/DBus"), cs("org.freedesktop.DBus"), cs("RequestName"));
            (lib.g_dbus_connection_call_sync)(bus, s.as_ptr(), p.as_ptr(), i.as_ptr(), m.as_ptr(), args, std::ptr::null_mut(), 0, 1000, std::ptr::null_mut(), &mut std::ptr::null_mut())
        };
        ask(NM);
        ask("org.freedesktop.login1");
        state.bus = bus;
        let register = |conn: Ptr, xml: &str, index: usize, path: &str, vtable: &'static VTable| {
            let x = cs(xml);
            let info = (lib.g_dbus_node_info_new_for_xml)(x.as_ptr(), &mut std::ptr::null_mut());
            let interface = *(*info).interfaces.add(index);
            (lib.g_dbus_connection_register_object)(conn, cs(path).as_ptr(), interface, vtable, data, std::ptr::null_mut(), &mut std::ptr::null_mut());
        };
        let nm: &'static VTable = Box::leak(Box::new(VTable { method_call: Some(nm_call), get_property: None, set_property: None, padding: [std::ptr::null_mut(); 8] }));
        let login: &'static VTable = Box::leak(Box::new(VTable { method_call: Some(login_call), get_property: None, set_property: None, padding: [std::ptr::null_mut(); 8] }));
        register(bus, LOGIN_XML, 0, "/org/freedesktop/login1", login);
        register(bus, LOGIN_XML, 1, "/org/freedesktop/login1/session/test", login);
        let paths = [BASE.to_string(), DEVICE.to_string(), format!("{BASE}/Devices/2"), PROFILE.to_string(), ACTIVE.to_string(), format!("{BASE}/AccessPoint/1"), format!("{BASE}/AccessPoint/2"), format!("{BASE}/AccessPoint/3")];
        for path in &paths {
            for index in 0..4 {
                register(bus, NM_XML, index, path, nm);
            }
        }
        // a separate connection for the tray app, so closing it tests the cleanup
        let app = (lib.g_dbus_connection_new_for_address_sync)(cs(&address).as_ptr(), 1 | 8, std::ptr::null_mut(), std::ptr::null_mut(), &mut err);
        if app.is_null() {
            return Err("cannot open the tray app connection".into());
        }
        state.app = app;
        let tray: &'static VTable = Box::leak(Box::new(VTable { method_call: Some(tray_call), get_property: Some(tray_prop), set_property: None, padding: [std::ptr::null_mut(); 8] }));
        let menu: &'static VTable = Box::leak(Box::new(VTable { method_call: Some(tray_call), get_property: None, set_property: None, padding: [std::ptr::null_mut(); 8] }));
        register(app, TRAY_XML, 0, "/StatusNotifierItem", tray);
        register(app, TRAY_XML, 1, "/Menu", menu);
        (lib.g_timeout_add)(40, register_app, data);
    }

    let binary = newest_test_binary(&repo.join("server/build/rust/debug"))?;
    let cache = std::env::temp_dir().join(format!("axia-network-tray-check-{}", std::process::id()));
    at(&cache, std::fs::create_dir_all(&cache))?;
    let output_file = cache.join("output.txt");
    let out = at(&output_file, std::fs::File::create(&output_file))?;
    let mut child = Command::new(&binary)
        .args(["desktop_bus_contract", "--ignored", "--nocapture"])
        .env("DBUS_SYSTEM_BUS_ADDRESS", &address)
        .env("AXIA_PRIVATE_BUS_TEST", "1")
        .env("XDG_CACHE_HOME", &cache)
        .stdout(Stdio::from(out.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(out))
        .spawn()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    // SAFETY: GLib main-context iteration on the main thread, where the callbacks above run.
    let context = unsafe { (state.lib.g_main_context_default)() };
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        // SAFETY: as above.
        unsafe {
            while (state.lib.g_main_context_pending)(context) != 0 {
                (state.lib.g_main_context_iteration)(context, 0);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    print!("{}", String::from_utf8_lossy(&std::fs::read(&output_file).unwrap_or_default()));
    let _ = std::fs::remove_dir_all(&cache);
    let expected = [
        "brightness-session", "brightness-write", "brightness-denied", "scan", "toggle", "saved-password-update", "saved-activation", "new-activation", "denied", "wired", "unplug", "tray-activate",
        "tray-about-to-show", "tray-menu-layout", "tray-menu-event", "tray-disconnect",
    ];
    let mut missing: Vec<&str> = expected.iter().copied().filter(|e| !state.records.contains(*e)).collect();
    missing.sort_unstable();
    if status.is_none_or(|s| !s.success()) || !state.failures.is_empty() || !missing.is_empty() {
        return Err(format!("FAILED: exit={:?}, errors={:?}, missing={missing:?}", status.and_then(|s| s.code()), state.failures));
    }
    println!("Network, tray and brightness private-bus checks passed; host settings untouched.");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_follow_the_wired_state_and_the_object() {
        let wifi = props(false, BASE, NM);
        assert!(wifi.starts_with("({'WirelessEnabled': <true>") && wifi.contains(&format!("'PrimaryConnection': <objectpath '{ACTIVE}'>")));
        assert!(props(true, BASE, NM).contains("ActiveConnection/2"));
        assert!(props(true, &format!("{BASE}/Devices/2"), &format!("{NM}.Device")).contains("'State': <uint32 100>"));
        assert!(props(false, &format!("{BASE}/Devices/2"), &format!("{NM}.Device")).contains("'State': <uint32 20>"));
        assert!(props(false, &format!("{BASE}/AccessPoint/3"), "x").contains("'Strength': <byte 40>") && props(false, &format!("{BASE}/AccessPoint/3"), "x").contains("'RsnFlags': <uint32 0>"));
        assert!(props(false, &format!("{BASE}/AccessPoint/2"), "x").contains("<byte 95>") && props(false, &format!("{BASE}/AccessPoint/2"), "x").contains("'Flags': <uint32 1>"));
        assert_eq!(props(false, "/nothing", "x"), "({},)");
        assert_eq!(props(false, ACTIVE, "x"), "({'State': <uint32 2>},)");
    }

    #[test]
    fn the_variant_texts_are_balanced_and_the_ssid_is_the_utf8_of_the_name() {
        let ssid = ssid_text();
        let bytes: Vec<u8> = ssid.trim_start_matches("[byte ").trim_end_matches(']').split(", ").map(|b| u8::from_str_radix(b.trim_start_matches("0x"), 16).unwrap()).collect();
        assert_eq!(String::from_utf8(bytes).unwrap(), "Café: casa");
        for xml in [NM_XML, LOGIN_XML, TRAY_XML] {
            assert_eq!(xml.matches("<interface").count(), xml.matches("</interface>").count());
        }
    }

    #[test]
    fn without_a_built_test_binary_the_check_says_what_to_build() {
        let dir = crate::TestDir::new("net-tray-empty");
        assert!(newest_test_binary(&dir.0).unwrap_err().contains("build it first"));
    }

    #[test]
    fn the_test_binary_is_found_in_either_cargo_layout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::TestDir::new("net-tray-layouts");
        let old = dir.0.join("deps");
        let new = dir.0.join("build/axia-shell/abc123/out");
        for folder in [&old, &new] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let write = |path: &Path, mode: u32| {
            std::fs::write(path, "x").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        write(&new.join("axia_shell-abc123"), 0o755);
        write(&new.join("axia_shell-abc123.d"), 0o755);
        write(&old.join("axia_shell-def456"), 0o644);
        assert_eq!(newest_test_binary(&dir.0).unwrap(), new.join("axia_shell-abc123"));
    }
}
