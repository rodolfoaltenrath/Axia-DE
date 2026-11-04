//! Native StatusNotifier/AppIndicator host and compact background-app tray.
//! Registration, pixmaps, menus, actions and presentation are owned by Rust.
use crate::network_bus::{Bus, Obj, V};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
const WATCH: &str = "org.kde.StatusNotifierWatcher";
const WPATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const XML: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher"><method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method><method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method><property name="RegisteredStatusNotifierItems" type="as" access="read"/><property name="IsStatusNotifierHostRegistered" type="b" access="read"/><property name="ProtocolVersion" type="i" access="read"/><signal name="StatusNotifierItemRegistered"><arg type="s"/></signal><signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal><signal name="StatusNotifierHostRegistered"/></interface></node>"#;
#[repr(C)]
struct Vtable {
    method: Option<
        unsafe extern "C" fn(
            Obj,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            Obj,
            Obj,
            Obj,
        ),
    >,
    property: Option<
        unsafe extern "C" fn(
            Obj,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            Obj,
            Obj,
        ) -> Obj,
    >,
    set: Obj,
    padding: [Obj; 8],
}
// GLib reads this immutable vtable, never its null padding.
unsafe impl Sync for Vtable {}
#[link(name = "gio-2.0")]
extern "C" {
    fn g_dbus_node_info_new_for_xml(xml: *const c_char, error: Obj) -> Obj;
    fn g_dbus_node_info_lookup_interface(info: Obj, name: *const c_char) -> Obj;
    fn g_dbus_node_info_unref(info: Obj);
    fn g_dbus_connection_register_object(
        bus: Obj,
        path: *const c_char,
        info: Obj,
        vtable: *const Vtable,
        data: Obj,
        destroy: Obj,
        error: Obj,
    ) -> u32;
    fn g_dbus_connection_unregister_object(bus: Obj, id: u32) -> i32;
    fn g_dbus_method_invocation_return_value(invocation: Obj, value: Obj);
    fn g_dbus_method_invocation_return_dbus_error(
        invocation: Obj,
        name: *const c_char,
        message: *const c_char,
    );
    fn g_dbus_connection_emit_signal(
        bus: Obj,
        dest: *const c_char,
        path: *const c_char,
        iface: *const c_char,
        signal: *const c_char,
        args: Obj,
        error: Obj,
    ) -> i32;
    fn g_dbus_connection_get_unique_name(bus: Obj) -> *const c_char;
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_variant_ref(v: Obj) -> Obj;
    fn g_variant_ref_sink(v: Obj) -> Obj;
    fn g_variant_get_variant(v: Obj) -> Obj;
    fn g_variant_new_int32(v: i32) -> Obj;
    fn g_variant_get_int32(v: Obj) -> i32;
    fn g_variant_new_uint32(v: u32) -> Obj;
    fn g_variant_new_variant(v: Obj) -> Obj;
    fn g_variant_get_fixed_array(v: Obj, n: *mut usize, size: usize) -> *const u8;
}
fn cs(s: &str) -> CString {
    CString::new(s).expect("validated tray string")
}
fn integer(n: i32) -> V {
    V::owned(unsafe { g_variant_new_int32(n) })
}
fn uint(n: u32) -> V {
    V::owned(unsafe { g_variant_new_uint32(n) })
}
fn i32v(v: &V) -> i32 {
    if v.is("i") {
        unsafe { g_variant_get_int32(v.0) }
    } else {
        0
    }
}
fn strv(m: &BTreeMap<String, V>, key: &str) -> String {
    m.get(key).map(V::string).unwrap_or_default()
}
fn owner(bus: &Bus, name: &str) -> Result<String> {
    Ok(bus
        .call_to(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            V::tuple(vec![V::text(name)]),
        )?
        .child(0)
        .string())
}
fn emit(bus: Obj, signal: &str, id: Option<&str>) {
    let args = V::tuple(id.map(|id| vec![V::text(id)]).unwrap_or_default());
    unsafe {
        g_dbus_connection_emit_signal(
            bus,
            std::ptr::null(),
            cs(WPATH).as_ptr(),
            cs(WATCH).as_ptr(),
            cs(signal).as_ptr(),
            args.0,
            std::ptr::null_mut(),
        );
    }
}
#[derive(Default)]
struct WatchState {
    items: BTreeMap<String, (String, String)>,
    pending: Vec<(String, String)>,
}
fn valid_path(path: &str) -> bool {
    path == "/"
        || (path.starts_with('/')
            && path.len() <= 256
            && path[1..].split('/').all(|part| {
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            }))
}
fn valid_bus_name(name: &str) -> bool {
    if name.len() > 255 {
        return false;
    }
    let unique = name.starts_with(':');
    let parts: Vec<_> = name.strip_prefix(':').unwrap_or(name).split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && (unique || !p.as_bytes()[0].is_ascii_digit())
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
}
fn registration(sender: &str, service: &str) -> Option<(String, String, String)> {
    if !sender.starts_with(':') || service.len() > 512 {
        return None;
    }
    let (name, path) = if service.starts_with('/') {
        (sender, service)
    } else if let Some((name, _)) = service.split_once('/') {
        (name, &service[name.len()..])
    } else {
        (service, "/StatusNotifierItem")
    };
    if !valid_bus_name(name) || !valid_path(path) {
        return None;
    }
    Some((format!("{name}{path}"), name.into(), path.into()))
}
unsafe extern "C" fn method(
    _: Obj,
    sender: *const c_char,
    _: *const c_char,
    _: *const c_char,
    name: *const c_char,
    args: Obj,
    inv: Obj,
    data: Obj,
) {
    let sender = CStr::from_ptr(sender).to_string_lossy();
    let name = CStr::from_ptr(name).to_string_lossy();
    let args = V(g_variant_ref(args));
    let service = args.child(0).string();
    let state = &*(data as *const Mutex<WatchState>);
    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    if name == "RegisterStatusNotifierItem"
        && registration(&sender, &service).is_some()
        && s.pending.len() < 128
        && s.items.len() < 128
    {
        s.pending.push((sender.into_owned(), service));
        g_dbus_method_invocation_return_value(inv, std::ptr::null_mut());
    } else if name == "RegisterStatusNotifierHost" {
        g_dbus_method_invocation_return_value(inv, std::ptr::null_mut());
    } else {
        g_dbus_method_invocation_return_dbus_error(
            inv,
            cs("org.freedesktop.DBus.Error.InvalidArgs").as_ptr(),
            cs("Invalid tray registration").as_ptr(),
        );
    }
}
unsafe extern "C" fn property(
    _: Obj,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    name: *const c_char,
    _: Obj,
    data: Obj,
) -> Obj {
    let name = CStr::from_ptr(name).to_string_lossy();
    match name.as_ref() {
        "RegisteredStatusNotifierItems" => {
            let state = &*(data as *const Mutex<WatchState>);
            let s = state.lock().unwrap_or_else(|e| e.into_inner());
            let v = V::array("s", s.items.keys().map(|k| V::text(k)).collect());
            g_variant_ref(v.0)
        }
        "IsStatusNotifierHostRegistered" => {
            let v = V::boolean(true);
            g_variant_ref(v.0)
        }
        "ProtocolVersion" => g_variant_ref_sink(g_variant_new_int32(0)),
        _ => std::ptr::null_mut(),
    }
}
static VTABLE: Vtable = Vtable {
    method: Some(method),
    property: Some(property),
    set: std::ptr::null_mut(),
    padding: [std::ptr::null_mut(); 8],
};
struct Host {
    bus: Bus,
    watch: Box<Mutex<WatchState>>,
    own: bool,
    object: u32,
}
impl Drop for Host {
    fn drop(&mut self) {
        if self.object != 0 {
            unsafe {
                g_dbus_connection_unregister_object(self.bus.bus, self.object);
            }
        }
        if self.own {
            self.bus.begin_for(500);
            let _ = self.bus.call_to(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ReleaseName",
                V::tuple(vec![V::text(WATCH)]),
            );
        }
    }
}
impl Host {
    fn open() -> Result<Self> {
        let mut bus = Bus::open_kind(2, WATCH)?;
        bus.begin();
        let reply = bus.call_to(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            V::tuple(vec![V::text(WATCH), uint(4)]),
        )?;
        let own = reply.child(0).number() == 1;
        let mut watch = Box::new(Mutex::new(WatchState::default()));
        let mut object = 0;
        if own {
            unsafe {
                let info = g_dbus_node_info_new_for_xml(cs(XML).as_ptr(), std::ptr::null_mut());
                if info.is_null() {
                    return Err("unavailable".into());
                }
                let iface = g_dbus_node_info_lookup_interface(info, cs(WATCH).as_ptr());
                let id = g_dbus_connection_register_object(
                    bus.bus,
                    cs(WPATH).as_ptr(),
                    iface,
                    &VTABLE,
                    (&mut *watch as *mut Mutex<WatchState>).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                g_dbus_node_info_unref(info);
                object = id;
                if id == 0 {
                    return Err("unavailable".into());
                }
            }
            emit(bus.bus, "StatusNotifierHostRegistered", None);
        } else {
            let unique = unsafe { CStr::from_ptr(g_dbus_connection_get_unique_name(bus.bus)) }
                .to_string_lossy();
            bus.call(
                WPATH,
                WATCH,
                "RegisterStatusNotifierHost",
                V::tuple(vec![V::text(&unique)]),
            )?;
        }
        Ok(Self {
            bus,
            watch,
            own,
            object,
        })
    }
    fn registrations(&mut self) -> Vec<(String, String, String)> {
        self.bus.events();
        self.bus.begin();
        if !self.own {
            if let Ok(m) = self.bus.all(WPATH, WATCH) {
                if let Some(items) = m.get("RegisteredStatusNotifierItems") {
                    if items.is("as") {
                        return (0..items.len().min(128))
                            .filter_map(|i| registration(":external", &items.child(i).string()))
                            .collect();
                    }
                }
            }
            return vec![];
        }
        let pending = std::mem::take(&mut self.watch.lock().unwrap().pending);
        for (sender, service) in pending {
            if let Some((id, name, path)) = registration(&sender, &service) {
                if owner(&self.bus, &name).ok().as_deref() == Some(&sender) {
                    let new = self
                        .watch
                        .lock()
                        .unwrap()
                        .items
                        .insert(id.clone(), (name, path))
                        .is_none();
                    if new {
                        emit(self.bus.bus, "StatusNotifierItemRegistered", Some(&id));
                    }
                }
            }
        }
        let entries = self.watch.lock().unwrap().items.clone();
        let mut live = vec![];
        for (id, (name, path)) in entries {
            if owner(&self.bus, &name).is_ok() {
                live.push((id, name, path));
            } else {
                self.watch.lock().unwrap().items.remove(&id);
                emit(self.bus.bus, "StatusNotifierItemUnregistered", Some(&id));
            }
        }
        live
    }
    fn item(&self, id: String, name: String, path: String) -> Result<Item> {
        let m = self.bus.all_to_item(&name, &path)?;
        let status = strv(&m, "Status");
        let attention = status == "NeedsAttention";
        let title = strv(&m, "Title");
        let title = if title.is_empty() {
            strv(&m, "Id")
        } else {
            title
        };
        let icon = icon(&id, &m, attention);
        Ok(Item {
            id,
            name,
            path,
            title: if title.is_empty() {
                "App".into()
            } else {
                title.chars().take(160).collect()
            },
            menu: strv(&m, "Menu"),
            only_menu: m.get("ItemIsMenu").is_some_and(V::yes),
            attention,
            icon,
            passive: status == "Passive",
        })
    }
}
impl Bus {
    fn all_to_item(&self, name: &str, path: &str) -> Result<BTreeMap<String, V>> {
        Ok(self
            .call_to(
                name,
                path,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                V::tuple(vec![V::text(ITEM)]),
            )?
            .child(0)
            .map())
    }
}
#[derive(Clone, PartialEq)]
struct Item {
    id: String,
    name: String,
    path: String,
    title: String,
    menu: String,
    only_menu: bool,
    attention: bool,
    icon: Option<String>,
    passive: bool,
}
fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let dir = base.join("axia/tray");
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::create_dir_all(&dir).ok()?;
    let m = std::fs::symlink_metadata(&dir).ok()?;
    if !m.is_dir() || m.file_type().is_symlink() || m.uid() != unsafe { libc::getuid() } {
        return None;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(dir)
}
fn hash(data: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}
fn pixmap(v: &V) -> Option<(u32, u32, Vec<u8>)> {
    if !v.is("a(iiay)") {
        return None;
    }
    let mut choices = vec![];
    for i in 0..v.len().min(16) {
        let p = v.child(i);
        let (w, h) = (i32v(&p.child(0)), i32v(&p.child(1)));
        if !(1..=512).contains(&w) || !(1..=512).contains(&h) {
            continue;
        }
        let bytes = p.child(2);
        let mut len = 0;
        let ptr = unsafe { g_variant_get_fixed_array(bytes.0, &mut len, 1) };
        if len != w as usize * h as usize * 4 || ptr.is_null() {
            continue;
        }
        let data = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        choices.push((w as u32, h as u32, data));
    }
    choices.sort_by_key(|(w, h, _)| {
        if (*w).min(*h) >= 24 {
            *w as i64 * *h as i64
        } else {
            1_000_000 - (*w as i64 * *h as i64)
        }
    });
    let (w, h, argb) = choices.into_iter().next()?;
    Some((
        w,
        h,
        argb.chunks_exact(4)
            .flat_map(|p| [p[1], p[2], p[3], p[0]])
            .collect(),
    ))
}
fn cached_icon(
    id: &str,
    key: u64,
    image: impl FnOnce() -> Option<image::DynamicImage>,
) -> Option<String> {
    let dir = cache_dir()?;
    let file = dir.join(format!("{:016x}-{key:016x}.png", hash(id)));
    if !file.is_file() {
        image()?.thumbnail(32, 32).save(&file).ok()?;
        let prefix = format!("{:016x}-", hash(id));
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path() != file && entry.file_name().to_string_lossy().starts_with(&prefix)
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    Some(file.to_string_lossy().into_owned())
}
fn file_icon(id: &str, path: &Path) -> Option<String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let path = path.canonicalize().ok()?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&path)
        .ok()?;
    let m = file.metadata().ok()?;
    if !m.is_file() || m.len() > 4 * 1024 * 1024 {
        return None;
    }
    let key = hash((&path, m.len(), m.mtime(), m.mtime_nsec()));
    cached_icon(id, key, || {
        let mut reader = image::ImageReader::new(std::io::BufReader::new(file))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(16 * 1024 * 1024);
        limits.max_image_width = Some(2048);
        limits.max_image_height = Some(2048);
        reader.limits(limits);
        reader.decode().ok()
    })
}
fn icon(id: &str, m: &BTreeMap<String, V>, attention: bool) -> Option<String> {
    let prefix = if attention { "AttentionIcon" } else { "Icon" };
    if let Some((w, h, rgba)) = m.get(&format!("{prefix}Pixmap")).and_then(pixmap) {
        let key = hash((&rgba, w, h));
        return cached_icon(id, key, || {
            Some(image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(
                w, h, rgba,
            )?))
        });
    }
    let name = strv(m, &format!("{prefix}Name"));
    if name.is_empty() {
        return if attention { icon(id, m, false) } else { None };
    }
    // IconThemePath is app-provided (Electron/AppIndicator often use private dirs).
    let private = strv(m, "IconThemePath");
    let mut candidates = vec![];
    if Path::new(&name).is_absolute() {
        candidates.push(PathBuf::from(&name))
    } else if name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && !name.contains("..")
    {
        for base in [
            private,
            "/usr/share/pixmaps".into(),
            "/usr/share/icons/hicolor/48x48/apps".into(),
            "/usr/share/icons/hicolor/32x32/apps".into(),
            "/usr/share/icons/hicolor/24x24/status".into(),
            "/usr/share/icons/hicolor/scalable/apps".into(),
        ] {
            if base.is_empty() {
                continue;
            }
            candidates.push(PathBuf::from(&base).join(&name));
            for ext in ["png", "jpg", "webp"] {
                candidates.push(PathBuf::from(&base).join(format!("{name}.{ext}")))
            }
        }
    }
    candidates
        .into_iter()
        .find_map(|p| file_icon(id, &p))
        .or_else(|| if attention { icon(id, m, false) } else { None })
}
fn menu_label(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else if !c.is_control() {
            out.push(c)
        }
    }
    out.chars().take(160).collect()
}
fn menu_nodes(layout: &V, key: &str, depth: usize, budget: &mut usize) -> Vec<Value> {
    if depth > 4 || !layout.is("(ia{sv}av)") {
        return vec![];
    }
    let children = layout.child(2);
    let mut result = vec![];
    for i in 0..children.len().min(128) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let child = children.child(i);
        if !child.is("v") {
            continue;
        }
        let child = V(unsafe { g_variant_get_variant(child.0) });
        if !child.is("(ia{sv}av)") {
            continue;
        }
        let id = i32v(&child.child(0));
        let m = child.child(1).map();
        if m.get("visible").is_some_and(|v| !v.yes()) {
            continue;
        }
        if strv(&m, "type") == "separator" {
            if !result.is_empty() && result.last() != Some(&json!("-")) {
                result.push(json!("-"))
            }
            continue;
        }
        let label = menu_label(&strv(&m, "label"));
        if label.is_empty() {
            continue;
        }
        let mut node = json!({"label":label,"action":["menuEvent",key,id],"disabled":m.get("enabled").is_some_and(|v|!v.yes())});
        if m.get("toggle-state").is_some_and(|v| i32v(v) > 0) {
            node["checked"] = json!(true)
        }
        let submenu = menu_nodes(&child, key, depth + 1, budget);
        if !submenu.is_empty() {
            node["submenu"] = json!(submenu);
            node.as_object_mut().unwrap().remove("action");
        }
        result.push(node)
    }
    while result.last() == Some(&json!("-")) {
        result.pop();
    }
    result
}
#[derive(Clone, Default, PartialEq)]
struct State {
    items: Vec<Item>,
    message: String,
    menu: Option<(String, Vec<Value>)>,
}
enum Command {
    Activate(String, i32, i32),
    Menu(String, i32, i32),
    Event(String, i32),
}
struct Service {
    out: Arc<Mutex<State>>,
    tx: mpsc::SyncSender<Command>,
}
static SERVICE: OnceLock<Service> = OnceLock::new();
fn service() -> &'static Service {
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(8);
        let out = Arc::new(Mutex::new(State::default()));
        let worker_out = out.clone();
        std::thread::Builder::new()
            .name("axia-tray".into())
            .spawn(move || worker(rx, worker_out))
            .expect("tray worker");
        Service { out, tx }
    })
}
fn worker(rx: mpsc::Receiver<Command>, out: Arc<Mutex<State>>) {
    let mut host = None;
    let mut state = State::default();
    let mut last = Instant::now() - Duration::from_secs(30);
    let mut retry = last;
    let mut dirty = true;
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(_) => None,
        };
        if host.is_none() && retry.elapsed() > Duration::from_secs(5) {
            host = Host::open().ok();
            retry = Instant::now();
        }
        if let Some(h) = host.as_mut() {
            dirty |= h.bus.events() || !h.watch.lock().unwrap().pending.is_empty();
            if (dirty && last.elapsed() > Duration::from_millis(500))
                || last.elapsed() > Duration::from_secs(30)
            {
                h.bus.begin_for(2000);
                let registrations = h.registrations();
                let mut items = vec![];
                for (id, name, path) in registrations.into_iter().take(64) {
                    h.bus.begin_for(1500);
                    if let Ok(item) = h.item(id, name, path) {
                        items.push(item)
                    }
                }
                items.sort_by(|a, b| a.title.cmp(&b.title));
                state.items = items;
                last = Instant::now();
                dirty = false;
            }
            if let Some(cmd) = cmd {
                let key = match &cmd {
                    Command::Activate(k, ..) | Command::Menu(k, ..) | Command::Event(k, ..) => k,
                };
                let item = state.items.iter().find(|i| &i.id == key);
                if let Some(i) = item {
                    h.bus.begin();
                    let r = match cmd {
                        Command::Activate(_, x, y) if !i.only_menu => h
                            .bus
                            .call_to(
                                &i.name,
                                &i.path,
                                ITEM,
                                "Activate",
                                V::tuple(vec![integer(x), integer(y)]),
                            )
                            .map(|_| ()),
                        Command::Menu(_, x, y) | Command::Activate(_, x, y) => {
                            if valid_path(&i.menu) && i.menu != "/" {
                                let _ = h.bus.call_to(
                                    &i.name,
                                    &i.menu,
                                    MENU,
                                    "AboutToShow",
                                    V::tuple(vec![integer(0)]),
                                );
                                h.bus
                                    .call_to(
                                        &i.name,
                                        &i.menu,
                                        MENU,
                                        "GetLayout",
                                        V::tuple(vec![
                                            integer(0),
                                            integer(4),
                                            V::array("s", vec![]),
                                        ]),
                                    )
                                    .map(|v| {
                                        state.menu = Some((
                                            i.id.clone(),
                                            menu_nodes(&v.child(1), &i.id, 0, &mut 128),
                                        ))
                                    })
                            } else {
                                h.bus
                                    .call_to(
                                        &i.name,
                                        &i.path,
                                        ITEM,
                                        "ContextMenu",
                                        V::tuple(vec![integer(x), integer(y)]),
                                    )
                                    .map(|_| ())
                            }
                        }
                        Command::Event(_, id) => {
                            state.menu = None;
                            h.bus
                                .call_to(
                                    &i.name,
                                    &i.menu,
                                    MENU,
                                    "Event",
                                    V::tuple(vec![
                                        integer(id),
                                        V::text("clicked"),
                                        V::owned(unsafe { g_variant_new_variant(integer(0).0) }),
                                        uint(0),
                                    ]),
                                )
                                .map(|_| ())
                        }
                    };
                    state.message = if r.is_ok() {
                        String::new()
                    } else {
                        "failed".into()
                    };
                }
            }
        }
        if let Ok(mut o) = out.lock() {
            if *o != state {
                *o = state.clone();
            }
        }
    }
}
fn tr(k: &str) -> Value {
    json!({"key":format!("shell.tray.{k}")})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"type":"view","style":style,"children":children})
}
fn label(text: Value, color: &str, size: u32) -> Value {
    json!({"type":"text","text":text,"style":{"color":color,"fontSize":size}})
}
fn action_view(style: Value, children: Vec<Value>, action: Value) -> Value {
    let mut v = view(style, children);
    v["events"] = json!({"onClick":action});
    v
}
#[derive(Default)]
struct Panel {
    open: bool,
    menu_key: Option<String>,
    x: i32,
    y: i32,
    previous: String,
}
thread_local! {static PANELS:RefCell<(u64,HashMap<u64,Panel>)>=RefCell::new((0,HashMap::new()));}
// Icon widgets are described only while the tray is open. A single app takes
// 56 × 56 pixels; larger collections wrap at five columns and scroll after four rows.
fn popup(panel: &Panel, state: &State, theme: &Value) -> Value {
    const CELL: usize = 40;
    const GAP: usize = 4;
    let count = state.items.len();
    let columns = count.clamp(1, 5);
    let rows = count.div_ceil(columns).clamp(1, 4);
    let fg = theme["text"].as_str().unwrap_or("#fff");
    let dim = theme["textDim"].as_str().unwrap_or("#aaa");
    let mut content = vec![];
    let grid_width = 16 + columns * (CELL + GAP) - GAP;
    let width = if count == 0 || !state.message.is_empty() {
        grid_width.max(184)
    } else {
        grid_width
    };
    if count == 0 {
        content.push(label(tr("empty"), dim, 12));
    } else {
        let mut icons = vec![];
        for item in &state.items {
            let icon = if let Some(path) = &item.icon {
                json!({"type":"image","src":path,"style":{"width":24,"height":24,"objectFit":"contain"}})
            } else {
                view(
                    json!({"width":24,"height":24,"borderRadius":6,"alignItems":"center","justifyContent":"center","backgroundColor":theme["hover"]}),
                    vec![label(
                        json!(item.title.chars().next().unwrap_or('A').to_string()),
                        fg,
                        14,
                    )],
                )
            };
            let mut tile = action_view(
                json!({"width":CELL,"height":CELL,"flexShrink":0,"alignItems":"center","justifyContent":"center","borderRadius":8,
                "backgroundColor":if item.attention {theme["hover"].clone()} else {json!("transparent")}}),
                vec![icon],
                json!(["activate", item.id]),
            );
            // Reuse the renderer's delayed full-name bubble without adding
            // another tooltip implementation or a timer for inactive icons.
            tile["type"] = json!("fileTile");
            tile["tooltip"] = json!(item.title);
            tile["events"]["onContextMenu"] = json!(["menu", item.id]);
            icons.push(tile);
        }
        content.push(json!({"type":"scroll","style":{"height":rows*(CELL+GAP)-GAP,"flex":0},"children":[
            view(json!({"width":columns*(CELL+GAP)-GAP,"flexDirection":"row","flexWrap":"wrap","gap":GAP}),icons)
        ]}));
    }
    if !state.message.is_empty() {
        content.push(label(tr("failed"), dim, 11));
    }
    if let Some(key) = &panel.menu_key {
        if let Some((menu_key, items)) = &state.menu {
            if key == menu_key {
                content.push(json!({"type":"contextMenu","absolute":true,"x":panel.x,"y":panel.y,"items":items}));
            }
        }
    }
    view(
        json!({"position":"absolute","right":0,"top":28,"width":width,"padding":8,"gap":6,"borderRadius":12,
        "backgroundColor":theme["panelBg"],"backdropFilter":format!("blur({}px)",theme["blur"].as_f64().unwrap_or(24.)),
        "border":format!("1px solid {}",theme["windowBorder"].as_str().unwrap_or("#555")),"boxShadow":"0 8px 24px rgba(0,0,0,0.3)"}),
        content,
    )
}
fn request(v: Value) -> Result<Value> {
    let s = service().out.lock().map_err(|_| "unavailable")?.clone();
    let op = v["op"].as_str().unwrap_or("");
    if op == "status" {
        return Ok(
            json!({"count":s.items.len(),"apps":s.items.iter().map(|i|&i.title).collect::<Vec<_>>()}),
        );
    }
    PANELS.with(|p|{let mut p=p.borrow_mut();if op=="create"{p.0+=1;let id=p.0;p.1.insert(id,Panel::default());return Ok(json!({"id":id}))}let id=v["id"].as_u64().ok_or("badRequest")?;if op=="close"{p.1.remove(&id);return Ok(Value::Null)}let panel=p.1.get_mut(&id).ok_or("badRequest")?;
 let a=&v["action"];let key=a[1].as_str().unwrap_or("");match a[0].as_str().unwrap_or(""){
 "toggle"=>{panel.open=!panel.open;panel.menu_key=None},"dismiss"=>{panel.open=false;panel.menu_key=None},"closeContext"=>panel.menu_key=None,
 "activate"|"menu"=>{let x=v["screenX"].as_i64().unwrap_or(0)as i32;let y=v["screenY"].as_i64().unwrap_or(0)as i32;let c=if a[0]=="activate" && !s.items.iter().any(|i|i.id==key&&i.only_menu){Command::Activate(key.into(),x,y)}else{panel.menu_key=Some(key.into());panel.x=v["screenX"].as_i64().unwrap_or(0)as i32;panel.y=v["screenY"].as_i64().unwrap_or(0)as i32;Command::Menu(key.into(),x,y)};let _=service().tx.try_send(c);},
 "menuEvent"=>{panel.menu_key=None;let _=service().tx.try_send(Command::Event(key.into(),a[2].as_i64().unwrap_or(0)as i32));},_=>{}}
 let t=&v["theme"];let fg=t["text"].as_str().unwrap_or("#fff");let color=v["color"].as_str().unwrap_or(fg);
 let mut dots=vec![];for row in 0..2{for col in 0..3{dots.push(view(json!({"position":"absolute","left":col*5,"top":row*5,"width":3,"height":3,"borderRadius":1,"backgroundColor":color}),vec![]));}}
 let button=action_view(json!({"height":22,"paddingHorizontal":8,"justifyContent":"center","alignItems":"center","flexDirection":"row","gap":5}),vec![view(json!({"width":13,"height":8}),dots),label(json!(s.items.len().to_string()),color,10)],json!(["toggle"]));
 let popup=if panel.open{popup(panel,&s,t)}else{Value::Null};
 let result=json!({"button":button,"panel":popup,"open":panel.open,"context":panel.menu_key.is_some()});let encoded=result.to_string();if encoded==panel.previous{return Ok(Value::Null)}panel.previous=encoded;Ok(result)
 })
}
#[no_mangle]
pub unsafe extern "C" fn axia_tray_request(input: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if input.is_null() {
            return json!({"error":"badRequest"});
        }
        let bytes = CStr::from_ptr(input).to_bytes();
        if bytes.len() > 16384 {
            return json!({"error":"badRequest"});
        }
        match serde_json::from_slice(bytes)
            .map_err(|_| "badRequest".to_owned())
            .and_then(request)
        {
            Ok(v) => v,
            Err(e) => json!({"error":e}),
        }
    })
    .unwrap_or_else(|_| json!({"error":"failed"}));
    CString::new(result.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn axia_tray_free(p: *mut c_char) {
    if !p.is_null() {
        drop(CString::from_raw(p))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_registration_paths() {
        assert_eq!(
            registration(":1.5", "/org/ayatana/NotificationItem/discord1")
                .unwrap()
                .1,
            ":1.5"
        );
        assert!(registration(":1.5", "../../bad").is_none());
        assert!(registration(":1.5", "/bad path").is_none());
        assert_eq!(
            registration(":1.5", "org.test.App").unwrap().2,
            "/StatusNotifierItem"
        );
    }
    #[test]
    fn menu_mnemonics_and_limits() {
        assert_eq!(menu_label("_Open __window"), "Open _window");
        assert_eq!(menu_label(&"a".repeat(400)).len(), 160);
    }
}

#[cfg(test)]
mod contract {
    use super::*;
    #[test]
    #[ignore = "requires tools/desktop/network-tray-check.py private bus"]
    fn desktop_bus_contract_tray() {
        assert_eq!(std::env::var("AXIA_PRIVATE_BUS_TEST").as_deref(), Ok("1"));
        let mut h = Host::open().unwrap();
        assert!(h.own);
        let end = Instant::now() + Duration::from_secs(6);
        let entries = loop {
            let entries = h.registrations();
            if !entries.is_empty() {
                break entries;
            }
            assert!(Instant::now() < end, "fixture did not register");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(entries.len(), 1);
        let (id, name, path) = entries.into_iter().next().unwrap();
        h.bus.begin();
        let item = h.item(id, name, path).unwrap();
        assert_eq!(item.title, "Discord de teste");
        assert!(item.icon.as_ref().is_some_and(|p| Path::new(p).is_file()));
        h.bus
            .call_to(
                &item.name,
                &item.path,
                ITEM,
                "Activate",
                V::tuple(vec![integer(12), integer(34)]),
            )
            .unwrap();
        h.bus
            .call_to(
                &item.name,
                &item.menu,
                MENU,
                "AboutToShow",
                V::tuple(vec![integer(0)]),
            )
            .unwrap();
        let layout = h
            .bus
            .call_to(
                &item.name,
                &item.menu,
                MENU,
                "GetLayout",
                V::tuple(vec![integer(0), integer(4), V::array("s", vec![])]),
            )
            .unwrap();
        let menu = menu_nodes(&layout.child(1), &item.id, 0, &mut 128);
        assert_eq!(menu.len(), 2);
        assert_eq!(menu[0]["label"], "Abrir janela");
        assert_eq!(menu[1]["label"], "Sair");
        h.bus
            .call_to(
                &item.name,
                &item.menu,
                MENU,
                "Event",
                V::tuple(vec![
                    integer(2),
                    V::text("clicked"),
                    V::owned(unsafe { g_variant_new_variant(integer(0).0) }),
                    uint(0),
                ]),
            )
            .unwrap();
        // App termination must remove its stale tray registration.
        let end = Instant::now() + Duration::from_secs(4);
        loop {
            if h.registrations().is_empty() {
                break;
            }
            assert!(Instant::now() < end, "dead item remained registered");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod edge_cases {
    use super::*;
    #[test]
    fn invalid_app_paths_cannot_reach_glib_calls() {
        for bad in [
            "a",
            "1app.example",
            "org..app",
            ":bad",
            "org.app/a//b",
            "org.app/a/",
            "org.app/a-b",
        ] {
            assert!(registration(":1.5", bad).is_none(), "accepted {bad}");
        }
        for good in [
            "org.app.Valid",
            ":1.5",
            "/org/ayatana/App1",
            "org.example.App/StatusNotifierItem",
        ] {
            assert!(registration(":1.5", good).is_some(), "rejected {good}");
        }
    }
    #[test]
    fn malformed_variants_are_empty_not_glib_assertions() {
        assert_eq!(V::text("not a tuple").child(1).len(), 0);
        assert!(menu_nodes(&V::tuple(vec![]), "app", 0, &mut 128).is_empty());
    }
}
