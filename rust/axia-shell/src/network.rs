//! Network policy, connection workflow and quick-panel widgets. No network I/O
//! or authentication can block the compositor: one bounded worker owns D-Bus.
use crate::network_bus::Bus;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    ffi::{c_char, CStr, CString},
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Security {
    Open,
    Owe,
    Psk,
    Sae,
    Enterprise,
    Legacy,
}
impl Security {
    pub fn from_flags(flags: u32, security: u32) -> Self {
        if security & 0x100 != 0 {
            Self::Psk
        } else if security & 0x400 != 0 {
            Self::Sae
        } else if security & (0x200 | 0x2000) != 0 {
            Self::Enterprise
        } else if security & (0x800 | 0x1000) != 0 {
            Self::Owe
        } else if flags & 1 != 0 {
            Self::Legacy
        } else {
            Self::Open
        }
    }
    pub fn matches(self, km: &str) -> bool {
        match self {
            Self::Open => km.is_empty(),
            Self::Owe => km == "owe",
            Self::Psk => km == "wpa-psk",
            Self::Sae => km == "sae" || km == "wpa-psk",
            Self::Enterprise => km == "wpa-eap" || km == "wpa-eap-suite-b-192",
            Self::Legacy => km == "none" || km == "ieee8021x",
        }
    }
    pub fn password(self) -> bool {
        matches!(self, Self::Psk | Self::Sae)
    }
    pub fn key_mgmt(self) -> Option<&'static str> {
        match self {
            Self::Open => None,
            Self::Owe => Some("owe"),
            Self::Psk => Some("wpa-psk"),
            Self::Sae => Some("sae"),
            _ => None,
        }
    }
    fn accepts(self, password: &str) -> bool {
        if password.contains('\0') || password.len() > 256 {
            return false;
        }
        match self {
            Self::Psk => {
                (8..=63).contains(&password.len())
                    || (password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
            }
            Self::Sae => !password.is_empty(),
            _ => password.is_empty(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct Network {
    pub id: String,
    pub device: String,
    pub ssid: String,
    #[serde(skip)]
    pub raw: Vec<u8>,
    pub security: Security,
    pub strength: u8,
    pub connected: bool,
    pub saved: Option<String>,
}
impl Default for Security {
    fn default() -> Self {
        Self::Open
    }
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Snapshot {
    pub available: bool,
    pub enabled: bool,
    pub hardware: bool,
    pub connectivity: u32,
    pub kind: String,
    pub wired: bool,
    pub wifi: Option<Network>,
    pub radios: Vec<String>,
    pub networks: Vec<Network>,
    pub busy: bool,
    pub scanning: bool,
    pub message: String,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            available: false,
            enabled: false,
            hardware: false,
            connectivity: 0,
            kind: "offline".into(),
            wired: false,
            wifi: None,
            radios: vec![],
            networks: vec![],
            busy: false,
            scanning: false,
            message: String::new(),
        }
    }
}
impl Snapshot {
    pub fn normalize(&mut self) {
        let mut groups: BTreeMap<(Vec<u8>, Security), Network> = BTreeMap::new();
        for n in self.networks.drain(..) {
            let key = (n.raw.clone(), n.security);
            if let Some(old) = groups.get_mut(&key) {
                let same_radio = old.device == n.device;
                let saved = if same_radio {
                    old.saved.clone().or(n.saved.clone())
                } else {
                    None
                };
                if (n.connected, n.strength) > (old.connected, old.strength) {
                    *old = n;
                }
                if same_radio {
                    old.saved = saved;
                }
            } else {
                groups.insert(key, n);
            }
        }
        self.networks = groups.into_values().collect();
        self.networks.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(b.strength.cmp(&a.strength))
                .then(a.ssid.cmp(&b.ssid))
        });
    }
}
pub fn display_ssid(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
struct Secret(String);
impl Drop for Secret {
    fn drop(&mut self) {
        unsafe {
            for b in self.0.as_bytes_mut() {
                std::ptr::write_volatile(b, 0)
            }
        }
    }
}
enum Command {
    Scan,
    Toggle(bool),
    Connect(Network, Secret),
}
struct Service {
    snapshot: std::sync::Arc<Mutex<Snapshot>>,
    tx: mpsc::SyncSender<Command>,
}
static SERVICE: OnceLock<Service> = OnceLock::new();
static CONNECTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn service() -> &'static Service {
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(4);
        let snapshot = std::sync::Arc::new(Mutex::new(Snapshot::default()));
        let out = snapshot.clone();
        std::thread::Builder::new()
            .name("axia-network".into())
            .spawn(move || worker(rx, out))
            .expect("network worker");
        Service { snapshot, tx }
    })
}
fn worker(rx: mpsc::Receiver<Command>, out: std::sync::Arc<Mutex<Snapshot>>) {
    let mut bus = None;
    let mut state = Snapshot::default();
    let mut dirty = true;
    let mut last = Instant::now() - Duration::from_secs(60);
    let mut scan_end = None;
    let mut activation: Option<(String, Instant)> = None;
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(_) => None,
        };
        if bus.is_none() && (last.elapsed() >= Duration::from_secs(10) || cmd.is_some()) {
            bus = Bus::open().ok();
            last = Instant::now();
            dirty = true;
        }
        if let Some(b) = bus.as_mut() {
            dirty |= b.events();
            if let Some(cmd) = cmd {
                let connecting_command = matches!(&cmd, Command::Connect(..));
                b.begin();
                let result = match cmd {
                    Command::Scan => {
                        scan_end = Some(Instant::now() + Duration::from_secs(5));
                        state.scanning = true;
                        b.scan(&state.radios)
                    }
                    Command::Toggle(on) => b.toggle(on),
                    Command::Connect(n, secret) => {
                        state.busy = true;
                        b.connect(&n, &secret.0).map(|p| {
                            activation = Some((p, Instant::now() + Duration::from_secs(60)));
                        })
                    }
                };
                state.message = result.err().unwrap_or_default();
                if !state.message.is_empty() && connecting_command {
                    state.busy = false;
                    CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                    activation = None;
                }
                dirty = true;
            }
            if last.elapsed() >= Duration::from_secs(30) {
                dirty = true
            }
            if dirty && last.elapsed() >= Duration::from_millis(500) {
                b.begin();
                match b.snapshot() {
                    Ok(mut s) => {
                        s.busy = state.busy;
                        s.message = state.message.clone();
                        s.scanning = state.scanning;
                        state = s;
                    }
                    Err(e) => {
                        state = Snapshot::default();
                        state.message = e;
                    }
                }
                last = Instant::now();
                dirty = false;
            }
            if let Some((path, end)) = &activation {
                b.begin();
                match b.activation(path) {
                    Ok(2) => {
                        state.busy = false;
                        CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                        state.message = "connected".into();
                        activation = None;
                        dirty = true;
                    }
                    Ok(4) => {
                        state.busy = false;
                        CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                        state.message = "passwordFailed".into();
                        activation = None;
                        dirty = true;
                    }
                    Err(_) => {
                        state.busy = false;
                        CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                        state.message = "failed".into();
                        activation = None;
                        dirty = true;
                    }
                    _ if Instant::now() > *end => {
                        state.busy = false;
                        CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                        state.message = "timedOut".into();
                        activation = None;
                    }
                    _ => {}
                }
            }
        } else if cmd.is_some() {
            state.message = "unavailable".into();
            state.busy = false;
            CONNECTING.store(false, std::sync::atomic::Ordering::Release);
        }
        if scan_end.is_some_and(|end| Instant::now() > end) {
            state.scanning = false;
            scan_end = None;
            dirty = true;
        }
        if let Ok(mut o) = out.lock() {
            if *o != state {
                *o = state.clone();
            }
        }
    }
}
#[derive(Default)]
struct Panel {
    compact: bool,
    selected: Option<String>,
    password: String,
    message: String,
    previous: Option<u64>,
}
impl Drop for Panel {
    fn drop(&mut self) {
        let _ = Secret(std::mem::take(&mut self.password));
    }
}
thread_local! {static PANELS:RefCell<(u64,HashMap<u64,Panel>)>=RefCell::new((0,HashMap::new()));}
fn tr(k: &str) -> Value {
    json!({"key":format!("network.quick.{k}")})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"type":"view","style":style,"children":children})
}
fn text(value: Value, color: &str, size: u32) -> Value {
    json!({"type":"text","text":value,"style":{"color":color,"fontSize":size}})
}
fn button(k: &str, action: Value, disabled: bool) -> Value {
    json!({"type":"button","label":tr(k),"action":action,"disabled":disabled})
}
// Vector primitives use the compositor renderer and need no font/icon-theme lookup.
fn glyph(kind: &str, strength: u8, color: &str) -> Value {
    if kind == "ethernet" {
        return view(
            json!({"width":20,"height":16}),
            vec![
                view(
                    json!({"position":"absolute","left":2,"top":1,"width":16,"height":10,"border":format!("1.5px solid {color}"),"borderRadius":2}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":9,"top":11,"width":2,"height":3,"backgroundColor":color}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":5,"top":14,"width":10,"height":1.5,"backgroundColor":color}),
                    vec![],
                ),
            ],
        );
    }
    let level = if kind != "wifi" {
        0
    } else if strength >= 70 {
        3
    } else if strength >= 40 {
        2
    } else {
        1
    };
    let mut children = vec![];
    for (i, size) in [20., 13., 6.].into_iter().enumerate() {
        children.push(view(json!({"position":"absolute","left":(20.-size)/2.,"top":i as f64*4.,"width":size,"height":size*0.32,"overflow":"hidden"}),vec![view(json!({"width":size,"height":size,"borderRadius":size/2.,"border":format!("1.7px solid {}",if level>=3-i{color}else{"rgba(150,150,150,0.35)"})}),vec![])]))
    }
    children.push(view(json!({"position":"absolute","left":9,"top":13,"width":3,"height":3,"borderRadius":2,"backgroundColor":color}),vec![]));
    view(json!({"width":20,"height":17}), children)
}
fn theme<'a>(t: &'a Value, k: &str, fallback: &'a str) -> &'a str {
    t[k].as_str().unwrap_or(fallback)
}
impl Panel {
    fn action(&mut self, a: &Value, s: &Snapshot) -> Result<()> {
        let name = a[0].as_str().unwrap_or("");
        let send = |c| service().tx.try_send(c).map_err(|_| "busy".to_owned());
        match name {
            "scan" => {
                if !s.scanning && !s.busy {
                    send(Command::Scan)?;
                }
            }
            "toggle" => {
                if !s.busy {
                    send(Command::Toggle(!s.enabled))?;
                }
            }
            "select" => {
                if s.busy {
                    return Ok(());
                }
                self.selected = a[1].as_str().map(str::to_owned);
                let _ = Secret(std::mem::take(&mut self.password));
                self.message.clear();
            }
            "password" => {
                let p = a[1].as_str().unwrap_or("");
                if p.len() <= 256 && !p.contains('\0') {
                    let _ = Secret(std::mem::replace(&mut self.password, p.into()));
                }
            }
            "cancel" => {
                self.selected = None;
                let _ = Secret(std::mem::take(&mut self.password));
                self.message.clear();
            }
            "connect" => {
                if s.busy {
                    return Ok(());
                }
                let n = s
                    .networks
                    .iter()
                    .find(|n| Some(&n.id) == self.selected.as_ref())
                    .ok_or("gone")?;
                if matches!(n.security, Security::Enterprise | Security::Legacy)
                    && n.saved.is_none()
                {
                    return Err("advanced".into());
                }
                if !(self.password.is_empty() && n.saved.is_some())
                    && !n.security.accepts(&self.password)
                {
                    return Err("invalidPassword".into());
                }
                if CONNECTING.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    return Err("busy".into());
                }
                if let Err(error) = send(Command::Connect(
                    n.clone(),
                    Secret(std::mem::take(&mut self.password)),
                )) {
                    CONNECTING.store(false, std::sync::atomic::Ordering::Release);
                    return Err(error);
                }
                self.selected = None;
            }
            _ => {}
        }
        Ok(())
    }
    fn tree(&self, s: &Snapshot, t: &Value) -> Value {
        let fg = theme(t, "text", "#fff");
        let dim = theme(t, "textDim", "#aaa");
        let accent = theme(t, "accent", "#08f");
        let bg = theme(t, "hover", "rgba(255,255,255,0.05)");
        let mut content = vec![view(
            json!({"flexDirection":"row","alignItems":"center","gap":8}),
            vec![
                glyph(
                    &s.kind,
                    s.wifi.as_ref().map(|n| n.strength).unwrap_or(0),
                    fg,
                ),
                text(
                    if s.kind == "ethernet" {
                        tr("wired")
                    } else if let Some(n) = &s.wifi {
                        json!(n.ssid)
                    } else {
                        tr("offline")
                    },
                    fg,
                    13,
                ),
            ],
        )];
        if !s.available {
            content.push(text(tr("unavailable"), dim, 12));
            return view(json!({"gap":12}), content);
        }
        let mut header = vec![text(json!("Wi-Fi"), fg, 13)];
        if !s.radios.is_empty() {
            header.push(button(
                if s.enabled { "turnOff" } else { "turnOn" },
                json!(["toggle"]),
                s.busy || !s.hardware,
            ));
            header.push(button(
                if s.scanning { "scanning" } else { "scan" },
                json!(["scan"]),
                s.scanning || !s.enabled || s.busy,
            ));
        }
        content.push(view(json!({"flexDirection":"row","alignItems":"center","justifyContent":"space-between","gap":8}),header));
        let message = if !self.message.is_empty() {
            self.message.as_str()
        } else {
            &s.message
        };
        if !message.is_empty() {
            content.push(text(
                tr(message),
                if message == "connected" { accent } else { dim },
                12,
            ));
        }
        if s.busy {
            content.push(text(tr("connecting"), accent, 12));
        }
        if s.radios.is_empty() {
            content.push(text(tr("noAdapter"), dim, 12));
        } else if !s.hardware {
            content.push(text(tr("hardwareOff"), dim, 12));
        } else if !s.enabled {
            content.push(text(tr("disabled"), dim, 12));
        } else {
            let mut rows = vec![];
            let mut selected_detail = None;
            for (index, n) in s.networks.iter().enumerate() {
                let selected = self.selected.as_deref() == Some(&n.id);
                if self.compact && index >= 6 && !selected {
                    continue;
                }
                let mut name = text(json!(n.ssid), fg, 12);
                name["style"]["flex"] = json!(1);
                name["style"]["minWidth"] = json!(0);
                name["style"]["whiteSpace"] = json!("nowrap");
                name["style"]["textOverflow"] = json!("ellipsis");
                name["style"]["overflow"] = json!("hidden");
                let mut row = view(
                    json!({"flexDirection":"row","alignItems":"center","gap":10,"padding":8,"height":40,"flexShrink":0,"borderRadius":8,"backgroundColor":if selected{theme(t,"selection",bg)}else{"transparent"}}),
                    vec![
                        glyph("wifi", n.strength, if n.connected { accent } else { fg }),
                        name,
                        text(
                            if n.connected {
                                tr("active")
                            } else if n.security == Security::Open {
                                tr("open")
                            } else {
                                tr("secured")
                            },
                            dim,
                            10,
                        ),
                    ],
                );
                row["events"] = json!({"onClick":["select",n.id]});
                rows.push(row);
                if selected && !n.connected {
                    let mut detail = vec![];
                    if matches!(n.security, Security::Enterprise | Security::Legacy)
                        && n.saved.is_none()
                    {
                        detail.push(text(tr("advanced"), dim, 12));
                    } else {
                        if n.security.password() {
                            detail.push(json!({"type":"input","value":self.password,"secure":true,"autoFocus":true,"placeholder":tr(if n.saved.is_some(){"savedPassword"}else{"password"}),"action":["password"],"submit":["connect"],"cancel":["cancel"],"style":{"height":32}}));
                        }
                        if n.security == Security::Open {
                            detail.push(text(tr("openWarning"), dim, 11));
                        }
                        detail.push(view(
                            json!({"flexDirection":"row","gap":8,"justifyContent":"flex-end"}),
                            vec![
                                button("cancel", json!(["cancel"]), false),
                                button("connect", json!(["connect"]), s.busy),
                            ],
                        ));
                    }
                    let detail = view(json!({"padding":10,"gap":8}), detail);
                    if self.compact {
                        selected_detail = Some(detail);
                    } else {
                        rows.push(detail);
                    }
                }
            }
            if rows.is_empty() {
                rows.push(text(
                    tr(if s.scanning { "scanning" } else { "noNetworks" }),
                    dim,
                    12,
                ));
            }
            content.push(json!({"type":"scroll","style":{"height":if self.compact {(s.networks.len().min(4)*40).clamp(44,160)}else{(s.networks.len()*42+if self.selected.is_some(){130}else{0}).clamp(44,240)},"flex":0},"children":rows}));
            if let Some(detail) = selected_detail {
                content.push(detail);
            }
            if self.compact {
                content.push(button("allNetworks", json!(["settings"]), false));
            }
        }
        view(json!({"gap":10}), content)
    }
}
fn request(v: Value) -> Result<Value> {
    let op = v["op"].as_str().unwrap_or("");
    let mut s = service()
        .snapshot
        .lock()
        .map_err(|_| "unavailable")?
        .clone();
    s.busy |= CONNECTING.load(std::sync::atomic::Ordering::Acquire);
    if op == "status" {
        return Ok(json!(s));
    }
    if op == "icon" {
        return Ok(glyph(
            &s.kind,
            s.wifi.as_ref().map(|n| n.strength).unwrap_or(0),
            v["color"].as_str().unwrap_or("#fff"),
        ));
    }
    PANELS.with(|p| {
        let mut p = p.borrow_mut();
        if op == "create" {
            p.0 += 1;
            let id = p.0;
            let mut panel = Panel::default();
            panel.compact = v["compact"].as_bool().unwrap_or(false);
            p.1.insert(id, panel);
            if s.available && s.enabled && !s.busy {
                let _ = service().tx.try_send(Command::Scan);
            }
            return Ok(json!({"id":id}));
        }
        let id = v["id"].as_u64().ok_or("badRequest")?;
        if op == "close" {
            p.1.remove(&id);
            return Ok(Value::Null);
        }
        let panel = p.1.get_mut(&id).ok_or("badRequest")?;
        if op == "action" {
            let mut a = v["action"].clone();
            if a[0] == "password" {
                a = json!(["password", v["value"]]);
            }
            if let Err(e) = panel.action(&a, &s) {
                panel.message = e;
            } else if a[0] != "password" {
                panel.message.clear();
            }
        }
        let tree = panel.tree(&s, &v["theme"]);
        use std::hash::{Hash, Hasher};
        let encoded = Secret(tree.to_string());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        encoded.0.hash(&mut hasher);
        let fingerprint = hasher.finish();
        if Some(fingerprint) == panel.previous {
            return Ok(Value::Null);
        }
        panel.previous = Some(fingerprint);
        Ok(tree)
    })
}
/// C receives owned JSON only. A malformed UI request cannot unwind across FFI.
#[no_mangle]
pub unsafe extern "C" fn axia_network_request(input: *const c_char) -> *mut c_char {
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
pub unsafe extern "C" fn axia_network_free(p: *mut c_char) {
    if !p.is_null() {
        drop(CString::from_raw(p))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssid_names_are_not_mac_addresses() {
        assert_eq!(
            display_ssid("Minha rede: café 📡".as_bytes()),
            "Minha rede: café 📡"
        );
        assert_eq!(display_ssid(&[0xff, b'a', 0]), "�a�");
    }
    #[test]
    fn security_does_not_downgrade_encrypted_aps() {
        assert_eq!(Security::from_flags(1, 0x500), Security::Psk);
        assert_eq!(Security::from_flags(1, 0x400), Security::Sae);
        assert_eq!(Security::from_flags(1, 0x200), Security::Enterprise);
        assert_eq!(Security::from_flags(1, 0x800), Security::Owe);
        assert_eq!(Security::from_flags(1, 0), Security::Legacy);
        assert_eq!(Security::from_flags(0, 0), Security::Open);
    }
    #[test]
    fn validate_wpa_credentials() {
        assert!(!Security::Psk.accepts("1234567"));
        assert!(Security::Psk.accepts("12345678"));
        assert!(Security::Psk.accepts(&"a".repeat(64)));
        assert!(!Security::Psk.accepts(&"g".repeat(64)));
        assert!(!Security::Psk.accepts("hello\0world"));
        assert!(Security::Sae.accepts("pass"));
    }
    #[test]
    fn group_radios_by_raw_ssid_and_security_not_display_name() {
        let mut s = Snapshot::default();
        for (id, strength, connected, security) in [
            ("a", 20, false, Security::Psk),
            ("b", 90, false, Security::Psk),
            ("c", 40, true, Security::Psk),
            ("d", 100, false, Security::Open),
        ] {
            s.networks.push(Network {
                id: id.into(),
                raw: b"Cafe".to_vec(),
                ssid: "Cafe".into(),
                strength,
                connected,
                security,
                ..Network::default()
            });
        }
        s.normalize();
        assert_eq!(s.networks.len(), 2);
        assert_eq!(s.networks[0].id, "c");
        assert_eq!(s.networks[1].id, "d");
    }
    #[test]
    fn panel_and_icon_are_rust_owned() {
        let s = Snapshot {
            available: true,
            enabled: true,
            hardware: true,
            radios: vec!["radio".into()],
            networks: vec![Network {
                id: "ap".into(),
                ssid: "Casa".into(),
                security: Security::Psk,
                ..Network::default()
            }],
            ..Snapshot::default()
        };
        let mut p = Panel::default();
        p.action(&json!(["select", "ap"]), &s).unwrap();
        assert!(p
            .tree(&s, &json!({}))
            .to_string()
            .contains("\"secure\":true"));
        assert!(glyph("ethernet", 0, "#fff").to_string().contains("border"));
        assert!(p.action(&json!(["connect"]), &s).is_err());
    }
}

#[cfg(test)]
mod compact_tests {
    use super::*;
    #[test]
    fn crowded_airwaves_keep_a_small_list_and_password_outside_the_scroll() {
        let mut s = Snapshot {
            available: true,
            enabled: true,
            hardware: true,
            radios: vec!["radio".into()],
            ..Snapshot::default()
        };
        for n in 0..100 {
            s.networks.push(Network {
                id: format!("ap{n}"),
                ssid: format!("Rede {n}"),
                security: Security::Psk,
                ..Network::default()
            });
        }
        let mut p = Panel::default();
        p.compact = true;
        p.selected = Some("ap99".into());
        let tree = p.tree(&s, &json!({}));
        let children = tree["children"].as_array().unwrap();
        let scroll = children.iter().find(|n| n["type"] == "scroll").unwrap();
        assert_eq!(scroll["style"]["flex"], 0);
        assert!(scroll["style"]["height"].as_u64().unwrap() <= 160);
        assert_eq!(scroll["children"].as_array().unwrap().len(), 7); // six best plus selected AP
        assert!(!scroll.to_string().contains("\"secure\":true"));
        assert!(children
            .iter()
            .any(|n| n.to_string().contains("\"secure\":true")));
        assert!(children
            .iter()
            .any(|n| n.to_string().contains("allNetworks")));
    }
}
