//! Fedora volume operations over the system UDisks2 bus. No root helper/shell.
use super::Result;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
type Obj = *mut c_void;
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
        reply_type: Obj,
        flags: i32,
        timeout: i32,
        cancel: Obj,
        error: *mut *mut GError,
    ) -> Obj;
    fn g_dbus_error_get_remote_error(error: *const GError) -> *mut c_char;
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_error_free(error: *mut GError);
    fn g_free(data: Obj);
    fn g_variant_type_new(signature: *const c_char) -> Obj;
    fn g_variant_type_free(t: Obj);
    fn g_variant_new_array(t: Obj, children: *const Obj, len: usize) -> Obj;
    fn g_variant_new_tuple(children: *const Obj, len: usize) -> Obj;
    fn g_variant_new_string(value: *const c_char) -> Obj;
    fn g_variant_ref_sink(value: Obj) -> Obj;
    fn g_variant_unref(value: Obj);
    fn g_variant_get_child_value(value: Obj, index: usize) -> Obj;
    fn g_variant_get_variant(value: Obj) -> Obj;
    fn g_variant_is_of_type(value: Obj, t: Obj) -> i32;
    fn g_variant_get_string(value: Obj, len: *mut usize) -> *const c_char;
    fn g_variant_get_boolean(value: Obj) -> i32;
}
#[link(name = "gobject-2.0")]
extern "C" {
    fn g_object_unref(object: Obj);
}
fn cs(s: &str) -> CString {
    CString::new(s).expect("fixed UDisks protocol string")
}
fn device_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn block_path(device: &str) -> Result<String> {
    if !device_ok(device) {
        return Err("sys.badArgument".into());
    }
    let mut path = "/org/freedesktop/UDisks2/block_devices/".to_owned();
    for b in device.bytes() {
        if b.is_ascii_alphanumeric() {
            path.push(b as char);
        } else {
            path.push_str(&format!("_{b:02x}"));
        }
    }
    Ok(path)
}
fn error_code(name: &str) -> String {
    if name.contains("NotAuthorized") || name.contains("AccessDenied") {
        "files.deviceDenied"
    } else if name.ends_with("DeviceBusy") {
        "files.deviceBusy"
    } else if name.ends_with("NotSupported") {
        "files.deviceUnsupported"
    } else if name.ends_with("ServiceUnknown")
        || name.ends_with("UnknownObject")
        || name.ends_with("NameHasNoOwner")
    {
        "files.deviceUnavailable"
    } else {
        "files.deviceFailed"
    }
    .into()
}
unsafe fn take_error(error: *mut GError) -> String {
    if error.is_null() {
        return "files.deviceUnavailable".into();
    }
    let name = unsafe { g_dbus_error_get_remote_error(error) };
    let result = if name.is_null() {
        "files.deviceUnavailable".into()
    } else {
        error_code(&unsafe { CStr::from_ptr(name) }.to_string_lossy())
    };
    unsafe {
        if !name.is_null() {
            g_free(name.cast());
        }
        g_error_free(error);
    }
    result
}
struct Variant(Obj);
impl Drop for Variant {
    fn drop(&mut self) {
        unsafe { g_variant_unref(self.0) }
    }
}
impl Variant {
    fn options() -> Self {
        unsafe {
            let t = g_variant_type_new(cs("{sv}").as_ptr());
            let dict = g_variant_new_array(t, std::ptr::null(), 0);
            g_variant_type_free(t);
            Self(g_variant_ref_sink(g_variant_new_tuple(&dict, 1)))
        }
    }
    fn strings(a: &str, b: &str) -> Self {
        unsafe {
            let v = [
                g_variant_new_string(cs(a).as_ptr()),
                g_variant_new_string(cs(b).as_ptr()),
            ];
            Self(g_variant_ref_sink(g_variant_new_tuple(v.as_ptr(), 2)))
        }
    }
    fn child(&self) -> Self {
        Self(unsafe { g_variant_get_child_value(self.0, 0) })
    }
    fn is(&self, sig: &str) -> bool {
        unsafe {
            let t = g_variant_type_new(cs(sig).as_ptr());
            let ok = g_variant_is_of_type(self.0, t) != 0;
            g_variant_type_free(t);
            ok
        }
    }
    fn string(&self) -> Result<String> {
        if !self.is("s") && !self.is("o") {
            return Err("files.deviceFailed".into());
        }
        Ok(
            unsafe { CStr::from_ptr(g_variant_get_string(self.0, std::ptr::null_mut())) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}
struct Bus {
    object: Obj,
    deadline: Instant,
}
impl Drop for Bus {
    fn drop(&mut self) {
        unsafe { g_object_unref(self.object) }
    }
}
impl Bus {
    fn open() -> Result<Self> {
        unsafe {
            let mut error = std::ptr::null_mut();
            let object = g_bus_get_sync(1, std::ptr::null_mut(), &mut error);
            if object.is_null() {
                return Err(take_error(error));
            }
            Ok(Self {
                object,
                deadline: Instant::now() + Duration::from_secs(60),
            })
        }
    }
    fn call(&self, path: &str, iface: &str, method: &str, args: Variant) -> Result<Variant> {
        let left = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        if left == 0 {
            return Err("files.deviceFailed".into());
        }
        unsafe {
            let mut error = std::ptr::null_mut();
            let result = g_dbus_connection_call_sync(
                self.object,
                cs("org.freedesktop.UDisks2").as_ptr(),
                cs(path).as_ptr(),
                cs(iface).as_ptr(),
                cs(method).as_ptr(),
                args.0,
                std::ptr::null_mut(),
                2, // G_DBUS_CALL_FLAGS_ALLOW_INTERACTIVE_AUTHORIZATION
                left.min(30000) as i32,
                std::ptr::null_mut(),
                &mut error,
            );
            if result.is_null() {
                Err(take_error(error))
            } else {
                Ok(Variant(result))
            }
        }
    }
    fn property(&self, path: &str, iface: &str, name: &str) -> Result<Variant> {
        let reply = self.call(
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Variant::strings(iface, name),
        )?;
        if !reply.is("(v)") {
            return Err("files.deviceFailed".into());
        }
        let wrapped = reply.child();
        Ok(Variant(unsafe { g_variant_get_variant(wrapped.0) }))
    }
}
#[derive(Clone, Debug)]
pub enum Plan {
    Mount(String),
    Release {
        disk: String,
        parts: Vec<String>,
        removable: bool,
    },
}
trait Backend {
    fn mount(&mut self, device: &str) -> Result<String>;
    fn unmount(&mut self, device: &str) -> Result<()>;
    fn remove(&mut self, disk: &str) -> Result<()>;
}
impl Backend for Bus {
    fn mount(&mut self, device: &str) -> Result<String> {
        let v = self.call(
            &block_path(device)?,
            "org.freedesktop.UDisks2.Filesystem",
            "Mount",
            Variant::options(),
        )?;
        if !v.is("(s)") {
            return Err("files.deviceFailed".into());
        }
        v.child().string()
    }
    fn unmount(&mut self, device: &str) -> Result<()> {
        self.call(
            &block_path(device)?,
            "org.freedesktop.UDisks2.Filesystem",
            "Unmount",
            Variant::options(),
        )?;
        Ok(())
    }
    fn remove(&mut self, disk: &str) -> Result<()> {
        let drive = self
            .property(&block_path(disk)?, "org.freedesktop.UDisks2.Block", "Drive")?
            .string()?;
        if !drive.starts_with("/org/freedesktop/UDisks2/drives/") {
            return Err("files.deviceUnavailable".into());
        }
        let power = self.property(&drive, "org.freedesktop.UDisks2.Drive", "CanPowerOff")?;
        if power.is("b") && unsafe { g_variant_get_boolean(power.0) } != 0 {
            self.call(
                &drive,
                "org.freedesktop.UDisks2.Drive",
                "PowerOff",
                Variant::options(),
            )?;
        } else {
            let eject = self.property(&drive, "org.freedesktop.UDisks2.Drive", "Ejectable")?;
            if eject.is("b") && unsafe { g_variant_get_boolean(eject.0) } != 0 {
                self.call(
                    &drive,
                    "org.freedesktop.UDisks2.Drive",
                    "Eject",
                    Variant::options(),
                )?;
            }
            // Some USB bridges cannot power off: a clean unmount is still safe.
        }
        Ok(())
    }
}
fn execute(plan: &Plan, backend: &mut impl Backend) -> Result<Option<String>> {
    match plan {
        Plan::Mount(device) => backend.mount(device).map(Some),
        Plan::Release {
            disk,
            parts,
            removable,
        } => {
            for device in parts {
                backend.unmount(device)?;
            }
            if *removable {
                backend.remove(disk)?;
            }
            Ok(None)
        }
    }
}
#[derive(Default)]
struct Jobs {
    sequence: u64,
    items: HashMap<u64, (Instant, Option<Result<Option<String>>>)>,
}
fn state() -> &'static Mutex<Jobs> {
    static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(Jobs::default()))
}
pub fn start(plan: Plan) -> Result<u64> {
    match &plan {
        Plan::Mount(d) => {
            block_path(d)?;
        }
        Plan::Release { disk, parts, .. } => {
            block_path(disk)?;
            if parts.len() > 128 {
                return Err("sys.badArgument".into());
            }
            for p in parts {
                block_path(p)?;
            }
        }
    }
    let mut jobs = state().lock().unwrap_or_else(|e| e.into_inner());
    jobs.items
        .retain(|_, (at, result)| result.is_none() || at.elapsed() < Duration::from_secs(90));
    if jobs.items.len() >= 8 || jobs.items.values().filter(|(_, v)| v.is_none()).count() >= 2 {
        return Err("fs.tooManyCopies".into());
    }
    jobs.sequence += 1;
    let id = jobs.sequence;
    jobs.items.insert(id, (Instant::now(), None));
    drop(jobs);
    let thread = std::thread::Builder::new()
        .name("axia-volume".into())
        .spawn(move || {
            let result = Bus::open().and_then(|mut bus| execute(&plan, &mut bus));
            if let Some((_, value)) = state()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .items
                .get_mut(&id)
            {
                *value = Some(result);
            }
        });
    if thread.is_err() {
        state()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .remove(&id);
        return Err("files.deviceFailed".into());
    }
    Ok(id)
}
pub fn poll(id: u64) -> Value {
    let mut jobs = state().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, result)) = jobs.items.get_mut(&id) {
        if let Some(result) = result.take() {
            jobs.items.remove(&id);
            return match result {
                Ok(path) => json!({"done":true,"result":path}),
                Err(error) => json!({"done":true,"failure":error}),
            };
        }
        return json!({"done":false});
    }
    json!({"done":true,"failure":"files.deviceUnavailable"})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        busy: bool,
    }
    impl Backend for Fake {
        fn mount(&mut self, d: &str) -> Result<String> {
            self.calls.push(format!("mount:{d}"));
            Ok("/run/media/test/USB".into())
        }
        fn unmount(&mut self, d: &str) -> Result<()> {
            self.calls.push(format!("unmount:{d}"));
            if self.busy {
                Err("files.deviceBusy".into())
            } else {
                Ok(())
            }
        }
        fn remove(&mut self, d: &str) -> Result<()> {
            self.calls.push(format!("remove:{d}"));
            Ok(())
        }
    }
    #[test]
    fn removal_unmounts_every_partition_and_never_forces_busy_files() {
        let plan = Plan::Release {
            disk: "sdb".into(),
            parts: vec!["sdb1".into(), "sdb2".into()],
            removable: true,
        };
        let mut f = Fake::default();
        execute(&plan, &mut f).unwrap();
        assert_eq!(f.calls, ["unmount:sdb1", "unmount:sdb2", "remove:sdb"]);
        let mut f = Fake {
            busy: true,
            ..Default::default()
        };
        assert_eq!(execute(&plan, &mut f).unwrap_err(), "files.deviceBusy");
        assert_eq!(f.calls, ["unmount:sdb1"]);
    }
    #[test]
    fn protocol_paths_and_error_categories_are_fixed() {
        assert_eq!(
            block_path("dm-0").unwrap(),
            "/org/freedesktop/UDisks2/block_devices/dm_2d0"
        );
        assert!(block_path("../sda").is_err());
        assert_eq!(
            error_code("org.freedesktop.UDisks2.Error.NotAuthorizedDismissed"),
            "files.deviceDenied"
        );
        assert_eq!(
            error_code("org.freedesktop.UDisks2.Error.DeviceBusy"),
            "files.deviceBusy"
        );
    }
    #[test]
    fn an_internal_unmount_never_powers_off_the_drive() {
        let mut f = Fake::default();
        execute(
            &Plan::Release {
                disk: "nvme1n1".into(),
                parts: vec!["nvme1n1p1".into()],
                removable: false,
            },
            &mut f,
        )
        .unwrap();
        assert_eq!(f.calls, ["unmount:nvme1n1p1"]);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires a real system bus; only reads the specified device's Drive property"]
    fn read_only_system_bus_probe() {
        let device = std::env::var("AXIA_UDISKS_PROBE")
            .expect("set AXIA_UDISKS_PROBE to a kernel device name");
        let bus = Bus::open().unwrap();
        let drive = bus
            .property(
                &block_path(&device).unwrap(),
                "org.freedesktop.UDisks2.Block",
                "Drive",
            )
            .unwrap()
            .string()
            .unwrap();
        assert!(drive.starts_with("/org/freedesktop/UDisks2/drives/"));
        println!(
            "UDisks2 system bus: typed read-only property passed; no mount/unmount operation sent"
        );
    }
}
