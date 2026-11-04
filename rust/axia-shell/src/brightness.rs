//! Backlight capability and asynchronous logind control; no root helper or shell.
use crate::network_bus::{Bus, V};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    ffi::{c_char, CStr, CString},
    fs,
    path::Path,
    sync::{mpsc, Arc, Mutex, OnceLock},
};
#[derive(Clone, Serialize, Debug)]
struct Device {
    name: String,
    percent: u32,
    #[serde(skip)]
    max: u32,
    #[serde(skip)]
    rank: u8,
}
fn devices(root: &Path) -> Vec<Device> {
    let mut list = vec![];
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten().take(32) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                continue;
            }
            let number = |file| {
                fs::read_to_string(e.path().join(file))
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
            };
            let Some(max) = number("max_brightness").filter(|v| *v > 0) else {
                continue;
            };
            let Some(value) = number("brightness") else {
                continue;
            };
            let kind = fs::read_to_string(e.path().join("type")).unwrap_or_default();
            let rank = match kind.trim() {
                "raw" => 0,
                "platform" => 1,
                _ => 2,
            };
            list.push(Device {
                name,
                percent: ((value.min(max) as u64 * 100 + max as u64 / 2) / max as u64) as u32,
                max,
                rank,
            });
        }
    }
    list.sort_by(|a, b| a.rank.cmp(&b.rank).then(a.name.cmp(&b.name)));
    list.truncate(8);
    list
}
fn raw_value(percent: u32, max: u32) -> u32 {
    ((percent.min(100) as u64 * max as u64 + 50) / 100)
        .max(1)
        .min(max as u64) as u32
}
#[derive(Default, Serialize)]
struct State {
    devices: Vec<Device>,
    error: String,
    busy: bool,
}
struct Service {
    state: Arc<Mutex<State>>,
    pending: Arc<Mutex<Option<(String, u32)>>>,
    tx: mpsc::SyncSender<()>,
}
fn write(bus: &mut Bus, name: &str, value: u32) -> Result<(), String> {
    bus.begin_for(1500);
    let session = bus
        .call(
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "GetSessionByPID",
            V::tuple(vec![V::uint32(std::process::id())]),
        )?
        .child(0)
        .string();
    if !session.starts_with("/org/freedesktop/login1/session/") {
        return Err("unavailable".into());
    }
    bus.call(
        &session,
        "org.freedesktop.login1.Session",
        "SetBrightness",
        V::tuple(vec![V::text("backlight"), V::text(name), V::uint32(value)]),
    )?;
    Ok(())
}
fn service() -> &'static Service {
    static SERVICE: OnceLock<Service> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(1);
        let state = Arc::new(Mutex::new(State::default()));
        let out = state.clone();
        let pending = Arc::new(Mutex::new(None::<(String, u32)>));
        let work = pending.clone();
        std::thread::Builder::new()
            .name("axia-brightness".into())
            .spawn(move || {
                let mut bus = None;
                while rx.recv().is_ok() {
                    let command = work.lock().unwrap().take();
                    let mut list = devices(Path::new("/sys/class/backlight"));
                    let mut error = out.lock().unwrap().error.clone();
                    if let Some((name, percent)) = command {
                        error.clear();
                        if let Some(d) = list.iter().find(|d| d.name == name) {
                            if bus.is_none() {
                                bus = Bus::open_kind(1, "org.freedesktop.login1").ok()
                            }
                            let result = match bus.as_mut() {
                                Some(b) => write(b, &name, raw_value(percent, d.max)),
                                None => Err("unavailable".into()),
                            };
                            if let Err(e) = result {
                                error = e;
                                bus = None;
                            }
                            list = devices(Path::new("/sys/class/backlight"));
                        } else {
                            error = "unavailable".into();
                        }
                    }
                    let busy = work.lock().unwrap().is_some();
                    *out.lock().unwrap() = State {
                        devices: list,
                        error,
                        busy,
                    };
                }
            })
            .expect("brightness worker");
        Service { state, pending, tx }
    })
}
fn request(v: Value) -> Value {
    let s = service();
    match v["op"].as_str().unwrap_or("") {
        "set" => {
            let Some(percent) = v["percent"]
                .as_f64()
                .filter(|n| n.is_finite() && (0. ..=100.).contains(n))
            else {
                return json!({"error":"invalid"});
            };
            let name = v["name"].as_str().unwrap_or("");
            if !s
                .state
                .lock()
                .unwrap()
                .devices
                .iter()
                .any(|d| d.name == name)
            {
                return json!({"error":"unavailable"});
            }
            *s.pending.lock().unwrap() = Some((name.into(), percent.round() as u32));
            {
                let mut state = s.state.lock().unwrap();
                state.busy = true;
                state.error.clear();
            }
            let _ = s.tx.try_send(());
        }
        "refresh" => {
            let _ = s.tx.try_send(());
        }
        _ => {}
    }
    json!(*s.state.lock().unwrap())
}
#[no_mangle]
pub unsafe extern "C" fn axia_brightness_request(input: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if input.is_null() {
            return json!({"error":"invalid"});
        }
        let bytes = CStr::from_ptr(input).to_bytes();
        if bytes.len() > 2048 {
            return json!({"error":"invalid"});
        }
        match serde_json::from_slice(bytes) {
            Ok(v) => request(v),
            Err(_) => json!({"error":"invalid"}),
        }
    })
    .unwrap_or(json!({"error":"unavailable"}));
    CString::new(result.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn axia_brightness_free(p: *mut c_char) {
    if !p.is_null() {
        drop(CString::from_raw(p));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires tools/desktop/network-tray-check.py private bus"]
    fn desktop_bus_contract_brightness() {
        assert_eq!(std::env::var("AXIA_PRIVATE_BUS_TEST").as_deref(), Ok("1"));
        let mut bus = Bus::open_kind(1, "org.freedesktop.login1").unwrap();
        write(&mut bus, "intel_backlight", 397).unwrap();
        assert_eq!(
            write(&mut bus, "intel_backlight", 999).err().as_deref(),
            Some("denied")
        );
    }
    #[test]
    fn absent_or_invalid_backlights_are_disabled_and_values_never_black_out() {
        let root = std::env::temp_dir().join(format!("axia-backlight-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(devices(&root).is_empty());
        for (name, max, value, kind) in [
            ("acpi_video0", "100", "60", "firmware"),
            ("intel_backlight", "937", "397", "raw"),
            ("bad", "0", "5", "raw"),
        ] {
            let p = root.join(name);
            fs::create_dir_all(&p).unwrap();
            for (f, v) in [
                ("max_brightness", max),
                ("brightness", value),
                ("type", kind),
            ] {
                fs::write(p.join(f), v).unwrap();
            }
        }
        let list = devices(&root);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "intel_backlight");
        assert_eq!(list[0].percent, 42);
        assert_eq!(raw_value(0, 937), 1);
        assert_eq!(raw_value(100, 937), 937);
        assert_eq!(raw_value(50, u32::MAX), 2147483648);
        fs::remove_dir_all(root).unwrap();
    }
}
