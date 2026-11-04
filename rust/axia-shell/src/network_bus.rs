//! Typed, bounded NetworkManager D-Bus transport. Used only on the network worker.
use super::network::{Network, Result, Security, Snapshot};
use std::collections::BTreeMap;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    time::{Duration, Instant},
};
pub(crate) type Obj = *mut c_void;
#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *mut c_char,
}
#[link(name = "gio-2.0")]
extern "C" {
    fn g_bus_get_sync(kind: i32, cancel: Obj, error: *mut *mut GError) -> Obj;
    fn g_dbus_connection_call_sync(
        bus: Obj,
        dest: *const c_char,
        path: *const c_char,
        iface: *const c_char,
        method: *const c_char,
        args: Obj,
        reply: Obj,
        flags: i32,
        timeout: i32,
        cancel: Obj,
        error: *mut *mut GError,
    ) -> Obj;
    fn g_dbus_error_get_remote_error(error: *const GError) -> *mut c_char;
    fn g_dbus_connection_signal_subscribe(
        bus: Obj,
        sender: *const c_char,
        iface: *const c_char,
        member: *const c_char,
        path: *const c_char,
        arg: *const c_char,
        flags: i32,
        callback: unsafe extern "C" fn(
            Obj,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            Obj,
            Obj,
        ),
        data: Obj,
        destroy: Obj,
    ) -> u32;
    fn g_dbus_connection_signal_unsubscribe(bus: Obj, id: u32);
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_error_free(e: *mut GError);
    fn g_free(v: Obj);
    fn g_variant_type_new(s: *const c_char) -> Obj;
    fn g_variant_type_free(t: Obj);
    fn g_variant_ref_sink(v: Obj) -> Obj;
    fn g_variant_unref(v: Obj);
    fn g_variant_new_tuple(v: *const Obj, n: usize) -> Obj;
    fn g_variant_new_array(t: Obj, v: *const Obj, n: usize) -> Obj;
    fn g_variant_new_dict_entry(k: Obj, v: Obj) -> Obj;
    fn g_variant_new_variant(v: Obj) -> Obj;
    fn g_variant_new_string(v: *const c_char) -> Obj;
    fn g_variant_new_object_path(v: *const c_char) -> Obj;
    fn g_variant_new_boolean(v: i32) -> Obj;
    fn g_variant_new_byte(v: u8) -> Obj;
    fn g_variant_get_child_value(v: Obj, i: usize) -> Obj;
    fn g_variant_n_children(v: Obj) -> usize;
    fn g_variant_is_container(v: Obj) -> i32;
    fn g_variant_get_variant(v: Obj) -> Obj;
    fn g_variant_get_string(v: Obj, n: *mut usize) -> *const c_char;
    fn g_variant_get_uint32(v: Obj) -> u32;
    fn g_variant_get_byte(v: Obj) -> u8;
    fn g_variant_get_boolean(v: Obj) -> i32;
    fn g_variant_is_of_type(v: Obj, t: Obj) -> i32;
    fn g_main_context_new() -> Obj;
    fn g_main_context_push_thread_default(c: Obj);
    fn g_main_context_pop_thread_default(c: Obj);
    fn g_main_context_unref(c: Obj);
    fn g_main_context_iteration(c: Obj, block: i32) -> i32;
}
#[link(name = "gobject-2.0")]
extern "C" {
    fn g_object_unref(v: Obj);
}
fn cs(s: &str) -> CString {
    CString::new(s).expect("validated D-Bus string")
}
pub struct V(pub(crate) Obj);
impl Drop for V {
    fn drop(&mut self) {
        unsafe { g_variant_unref(self.0) }
    }
}
impl V {
    pub(crate) fn owned(p: Obj) -> Self {
        Self(unsafe { g_variant_ref_sink(p) })
    }
    pub fn text(s: &str) -> Self {
        Self::owned(unsafe { g_variant_new_string(cs(s).as_ptr()) })
    }
    pub(crate) fn path(s: &str) -> Self {
        Self::owned(unsafe { g_variant_new_object_path(cs(s).as_ptr()) })
    }
    pub(crate) fn boolean(b: bool) -> Self {
        Self::owned(unsafe { g_variant_new_boolean(b as i32) })
    }
    pub(crate) fn tuple(v: Vec<Self>) -> Self {
        let p: Vec<_> = v.iter().map(|v| v.0).collect();
        Self::owned(unsafe { g_variant_new_tuple(p.as_ptr(), p.len()) })
    }
    pub(crate) fn array(sig: &str, v: Vec<Self>) -> Self {
        unsafe {
            let t = g_variant_type_new(cs(sig).as_ptr());
            let p: Vec<_> = v.iter().map(|v| v.0).collect();
            let a = Self::owned(g_variant_new_array(t, p.as_ptr(), p.len()));
            g_variant_type_free(t);
            a
        }
    }
    pub(crate) fn dict(sig: &str, v: Vec<(&str, Self)>) -> Self {
        Self::array(
            sig,
            v.into_iter()
                .map(|(k, v)| {
                    let key = Self::text(k);
                    let value = if sig == "{sv}" {
                        Self::owned(unsafe { g_variant_new_variant(v.0) })
                    } else {
                        v
                    };
                    Self::owned(unsafe { g_variant_new_dict_entry(key.0, value.0) })
                })
                .collect(),
        )
    }
    pub(crate) fn child(&self, i: usize) -> Self {
        if i >= self.len() {
            return Self::tuple(vec![]);
        }
        Self(unsafe { g_variant_get_child_value(self.0, i) })
    }
    pub(crate) fn is(&self, s: &str) -> bool {
        unsafe {
            let t = g_variant_type_new(cs(s).as_ptr());
            let ok = g_variant_is_of_type(self.0, t) != 0;
            g_variant_type_free(t);
            ok
        }
    }
    pub(crate) fn len(&self) -> usize {
        unsafe {
            if g_variant_is_container(self.0) == 0 {
                0
            } else {
                g_variant_n_children(self.0)
            }
        }
    }
    pub(crate) fn string(&self) -> String {
        if !self.is("s") && !self.is("o") {
            return String::new();
        }
        unsafe {
            CStr::from_ptr(g_variant_get_string(self.0, std::ptr::null_mut()))
                .to_string_lossy()
                .into_owned()
        }
    }
    pub(crate) fn number(&self) -> u32 {
        unsafe {
            if self.is("u") {
                g_variant_get_uint32(self.0)
            } else if self.is("y") {
                g_variant_get_byte(self.0) as u32
            } else {
                0
            }
        }
    }
    pub(crate) fn yes(&self) -> bool {
        self.is("b") && unsafe { g_variant_get_boolean(self.0) != 0 }
    }
    pub(crate) fn bytes(&self) -> Vec<u8> {
        if !self.is("ay") {
            return vec![];
        }
        (0..self.len().min(32))
            .map(|i| self.child(i).number() as u8)
            .collect()
    }
    pub(crate) fn paths(&self) -> Vec<String> {
        if !self.is("ao") {
            return vec![];
        }
        (0..self.len().min(256))
            .map(|i| self.child(i).string())
            .collect()
    }
    pub(crate) fn map(&self) -> BTreeMap<String, Self> {
        if !self.is("a{sv}") && !self.is("a{sa{sv}}") {
            return BTreeMap::new();
        }
        (0..self.len().min(256))
            .map(|i| {
                let e = self.child(i);
                let k = e.child(0).string();
                let v = e.child(1);
                (
                    k,
                    if v.is("v") {
                        Self(unsafe { g_variant_get_variant(v.0) })
                    } else {
                        v
                    },
                )
            })
            .collect()
    }
}
const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const DEV: &str = "org.freedesktop.NetworkManager.Device";
const WIFI: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const PROFILE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
fn strv(m: &BTreeMap<String, V>, k: &str) -> String {
    m.get(k).map(V::string).unwrap_or_default()
}
fn num(m: &BTreeMap<String, V>, k: &str) -> u32 {
    m.get(k).map(V::number).unwrap_or(0)
}
fn yes(m: &BTreeMap<String, V>, k: &str) -> bool {
    m.get(k).is_some_and(V::yes)
}
unsafe extern "C" fn changed(
    _: Obj,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: Obj,
    data: Obj,
) {
    (*(data as *const std::sync::atomic::AtomicBool))
        .store(true, std::sync::atomic::Ordering::Relaxed);
}
pub struct Bus {
    pub(crate) bus: Obj,
    pub(crate) context: Obj,
    dirty: Box<std::sync::atomic::AtomicBool>,
    dest: String,
    subscription: u32,
    deadline: Instant,
}
impl Drop for Bus {
    fn drop(&mut self) {
        unsafe {
            g_dbus_connection_signal_unsubscribe(self.bus, self.subscription);
            g_object_unref(self.bus);
            g_main_context_pop_thread_default(self.context);
            g_main_context_unref(self.context)
        }
    }
}
impl Bus {
    pub fn open() -> Result<Self> {
        Self::open_kind(1, NM)
    }
    pub(crate) fn open_kind(kind: i32, dest: &str) -> Result<Self> {
        unsafe {
            let context = g_main_context_new();
            g_main_context_push_thread_default(context);
            let mut e = std::ptr::null_mut();
            let bus = g_bus_get_sync(kind, std::ptr::null_mut(), &mut e);
            if bus.is_null() {
                if !e.is_null() {
                    g_error_free(e)
                }
                g_main_context_pop_thread_default(context);
                g_main_context_unref(context);
                return Err("unavailable".into());
            }
            let mut dirty = Box::new(std::sync::atomic::AtomicBool::new(true));
            let sender = cs(dest);
            let subscription = g_dbus_connection_signal_subscribe(
                bus,
                if kind == 2 {
                    std::ptr::null()
                } else {
                    sender.as_ptr()
                },
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                changed,
                (&mut *dirty as *mut std::sync::atomic::AtomicBool).cast(),
                std::ptr::null_mut(),
            );
            Ok(Self {
                bus,
                context,
                dirty,
                dest: dest.to_owned(),
                subscription,
                deadline: Instant::now(),
            })
        }
    }
    pub fn events(&mut self) -> bool {
        unsafe {
            for _ in 0..256 {
                if g_main_context_iteration(self.context, 0) == 0 {
                    break;
                }
            }
        }
        self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed)
    }
    pub(crate) fn begin_for(&mut self, millis: u64) {
        self.deadline = Instant::now() + Duration::from_millis(millis);
    }
    pub fn begin(&mut self) {
        self.deadline = Instant::now() + Duration::from_secs(12)
    }
    pub(crate) fn call(&self, path: &str, iface: &str, method: &str, args: V) -> Result<V> {
        self.call_to(&self.dest, path, iface, method, args)
    }
    pub(crate) fn call_to(
        &self,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        args: V,
    ) -> Result<V> {
        let timeout = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(10000) as i32;
        if timeout <= 0 {
            return Err("unavailable".into());
        }
        unsafe {
            let mut e = std::ptr::null_mut();
            let v = g_dbus_connection_call_sync(
                self.bus,
                cs(dest).as_ptr(),
                cs(path).as_ptr(),
                cs(iface).as_ptr(),
                cs(method).as_ptr(),
                args.0,
                std::ptr::null_mut(),
                2,
                timeout,
                std::ptr::null_mut(),
                &mut e,
            );
            if !v.is_null() {
                return Ok(V(v));
            }
            let mut code = "failed";
            if !e.is_null() {
                let remote = g_dbus_error_get_remote_error(e);
                if !remote.is_null() {
                    let name = CStr::from_ptr(remote).to_string_lossy();
                    code = if name.contains("NotAuthorized")
                        || name.contains("PermissionDenied")
                        || name.contains("AccessDenied")
                    {
                        "denied"
                    } else if name.contains("Secrets") {
                        "passwordFailed"
                    } else if name.contains("ServiceUnknown") || name.contains("NameHasNoOwner") {
                        "unavailable"
                    } else {
                        "failed"
                    };
                    g_free(remote.cast())
                }
                g_error_free(e)
            }
            Err(code.into())
        }
    }
    pub(crate) fn all(&self, path: &str, iface: &str) -> Result<BTreeMap<String, V>> {
        Ok(self
            .call(
                path,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                V::tuple(vec![V::text(iface)]),
            )?
            .child(0)
            .map())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let root = self.all(ROOT, NM)?;
        let mut s = Snapshot {
            available: true,
            enabled: yes(&root, "WirelessEnabled"),
            hardware: yes(&root, "WirelessHardwareEnabled"),
            connectivity: num(&root, "Connectivity"),
            ..Snapshot::default()
        };
        let primary = strv(&root, "PrimaryConnection");
        for path in root
            .get("Devices")
            .map(V::paths)
            .unwrap_or_default()
            .into_iter()
            .take(32)
        {
            let Ok(d) = self.all(&path, DEV) else {
                continue;
            };
            let typ = num(&d, "DeviceType");
            let state = num(&d, "State");
            let active = strv(&d, "ActiveConnection");
            if typ == 1 && state == 100 {
                s.wired = true;
                if active == primary {
                    s.kind = "ethernet".into()
                }
            }
            if typ != 2 {
                continue;
            }
            s.radios.push(path.clone());
            let Ok(w) = self.all(&path, WIFI) else {
                continue;
            };
            let active_ap = strv(&w, "ActiveAccessPoint");
            let profiles: Vec<_> = d
                .get("AvailableConnections")
                .map(V::paths)
                .unwrap_or_default()
                .into_iter()
                .take(64)
                .filter_map(|p| {
                    let settings = self
                        .call(&p, PROFILE, "GetSettings", V::tuple(vec![]))
                        .ok()?
                        .child(0)
                        .map();
                    let wifi = settings.get("802-11-wireless")?.map();
                    let ssid = wifi.get("ssid")?.bytes();
                    let km = settings
                        .get("802-11-wireless-security")
                        .map(V::map)
                        .map(|m| strv(&m, "key-mgmt"))
                        .unwrap_or_default();
                    Some((p, ssid, km))
                })
                .collect();
            for ap in w
                .get("AccessPoints")
                .map(V::paths)
                .unwrap_or_default()
                .into_iter()
                .take(128)
            {
                let Ok(a) = self.all(&ap, AP) else { continue };
                let raw = a.get("Ssid").map(V::bytes).unwrap_or_default();
                if raw.is_empty() {
                    continue;
                } // hidden networks have no displayable SSID
                let security = Security::from_flags(
                    num(&a, "Flags"),
                    num(&a, "WpaFlags") | num(&a, "RsnFlags"),
                );
                let connected = state == 100 && active_ap == ap;
                let saved = profiles
                    .iter()
                    .find(|(_, ssid, km)| *ssid == raw && security.matches(km))
                    .map(|(p, _, _)| p.clone());
                let n = Network {
                    id: ap.clone(),
                    device: path.clone(),
                    ssid: super::network::display_ssid(&raw),
                    raw,
                    security,
                    strength: num(&a, "Strength").min(100) as u8,
                    connected,
                    saved,
                };
                if connected {
                    s.wifi = Some(n.clone());
                    if active == primary || s.kind == "offline" {
                        s.kind = "wifi".into()
                    }
                }
                s.networks.push(n);
            }
        }
        if s.kind == "offline" && s.wired {
            s.kind = "ethernet".into()
        }
        s.normalize();
        Ok(s)
    }
    pub fn scan(&self, radios: &[String]) -> Result<()> {
        let mut ok = false;
        for d in radios {
            if self
                .call(
                    d,
                    WIFI,
                    "RequestScan",
                    V::tuple(vec![V::dict("{sv}", vec![])]),
                )
                .is_ok()
            {
                ok = true
            }
        }
        if ok {
            Ok(())
        } else {
            Err("scanFailed".into())
        }
    }
    pub fn toggle(&self, enabled: bool) -> Result<()> {
        self.call(
            ROOT,
            "org.freedesktop.DBus.Properties",
            "Set",
            V::tuple(vec![
                V::text(NM),
                V::text("WirelessEnabled"),
                V::owned(unsafe { g_variant_new_variant(V::boolean(enabled).0) }),
            ]),
        )?;
        Ok(())
    }
    pub fn connect(&self, n: &Network, password: &str) -> Result<String> {
        // Saved profiles retain their complete IP, enterprise and privacy settings.
        if let Some(profile) = &n.saved {
            if !password.is_empty() {
                let settings = self
                    .call(profile, PROFILE, "GetSettings", V::tuple(vec![]))?
                    .child(0);
                let mut settings = settings.map();
                let mut sec = settings
                    .remove("802-11-wireless-security")
                    .ok_or("failed")?
                    .map();
                sec.insert("psk".into(), V::text(password));
                // NM owns and protects this secret; don't depend on an absent GNOME secret agent.
                sec.insert(
                    "psk-flags".into(),
                    V::owned(unsafe {
                        // Secrets are stored and protected by NetworkManager.
                        g_variant_new_uint32(0)
                    }),
                );
                let sec = V::dict(
                    "{sv}",
                    sec.iter()
                        .map(|(k, v)| (k.as_str(), V(unsafe { g_variant_ref(v.0) })))
                        .collect(),
                );
                settings.insert("802-11-wireless-security".into(), sec);
                let settings = V::dict(
                    "{sa{sv}}",
                    settings
                        .iter()
                        .map(|(k, v)| (k.as_str(), V(unsafe { g_variant_ref(v.0) })))
                        .collect(),
                );
                self.call(profile, PROFILE, "Update", V::tuple(vec![settings]))?;
            }
            return Ok(self
                .call(
                    ROOT,
                    NM,
                    "ActivateConnection",
                    V::tuple(vec![V::path(profile), V::path(&n.device), V::path(&n.id)]),
                )?
                .child(0)
                .string());
        }
        let mut groups = vec![
            (
                "connection",
                V::dict(
                    "{sv}",
                    vec![
                        ("id", V::text(&n.ssid)),
                        ("type", V::text("802-11-wireless")),
                        ("autoconnect", V::boolean(true)),
                    ],
                ),
            ),
            (
                "802-11-wireless",
                V::dict(
                    "{sv}",
                    vec![
                        (
                            "ssid",
                            V::array(
                                "y",
                                n.raw
                                    .iter()
                                    .map(|b| V::owned(unsafe { g_variant_new_byte(*b) }))
                                    .collect(),
                            ),
                        ),
                        ("mode", V::text("infrastructure")),
                    ],
                ),
            ),
        ];
        if let Some(km) = n.security.key_mgmt() {
            let mut sec = vec![("key-mgmt", V::text(km))];
            if n.security.password() {
                sec.push(("psk", V::text(password)))
            }
            groups.push(("802-11-wireless-security", V::dict("{sv}", sec)))
        }
        let reply = self.call(
            ROOT,
            NM,
            "AddAndActivateConnection",
            V::tuple(vec![
                V::dict("{sa{sv}}", groups),
                V::path(&n.device),
                V::path(&n.id),
            ]),
        )?;
        Ok(reply.child(1).string())
    }
    pub fn activation(&self, path: &str) -> Result<u32> {
        Ok(num(&self.all(path, ACTIVE)?, "State"))
    }
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_variant_new_uint32(v: u32) -> Obj;
    fn g_variant_ref(v: Obj) -> Obj;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires tools/desktop/network-tray-check.py private bus"]
    fn desktop_bus_contract_network() {
        assert_eq!(std::env::var("AXIA_PRIVATE_BUS_TEST").as_deref(), Ok("1"));
        let mut b = Bus::open().unwrap();
        b.begin();
        let s = b.snapshot().unwrap();
        assert_eq!(s.kind, "wifi");
        assert_eq!(s.networks.len(), 2);
        assert_eq!(s.wifi.as_ref().unwrap().ssid, "Café: casa");
        assert_eq!(s.wifi.as_ref().unwrap().strength, 81);
        b.scan(&s.radios).unwrap();
        b.toggle(false).unwrap();
        let saved = s.networks.iter().find(|n| n.connected).unwrap();
        assert!(saved.saved.is_some());
        assert_eq!(
            b.connect(saved, "").unwrap(),
            "/org/freedesktop/NetworkManager/ActiveConnection/1"
        );
        b.connect(saved, "fixture-password").unwrap();
        let mut fresh = saved.clone();
        fresh.saved = None;
        fresh.raw = "Nova: rede".as_bytes().to_vec();
        fresh.ssid = "Nova: rede".into();
        let path = b.connect(&fresh, "fixture-password").unwrap();
        assert_eq!(b.activation(&path).unwrap(), 2);
        b.call(ROOT, NM, "TestWired", V::tuple(vec![])).unwrap();
        let wired = b.snapshot().unwrap();
        assert!(wired.wired);
        assert_eq!(wired.kind, "ethernet");
        b.call(ROOT, NM, "TestWifiOnly", V::tuple(vec![])).unwrap();
        assert_eq!(b.snapshot().unwrap().kind, "wifi");
        // Test the error mapping without invoking an operation on the real host.
        assert_eq!(
            b.call(ROOT, NM, "Denied", V::tuple(vec![]))
                .err()
                .as_deref(),
            Some("denied")
        );
    }
}
