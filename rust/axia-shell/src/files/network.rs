//! Fedora GIO/GVfs integration. Credentials travel only through the owning
//! dialog and GMountOperation, never through shell arguments or preferences.
use super::{policy, Result};
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
type Obj = *mut c_void;
#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *mut c_char,
}
#[link(name = "gio-2.0")]
unsafe extern "C" {
    fn g_file_new_for_uri(uri: *const c_char) -> Obj;
    fn g_file_get_path(file: Obj) -> *mut c_char;
    fn g_file_mount_enclosing_volume(
        file: Obj,
        flags: i32,
        op: Obj,
        cancel: Obj,
        callback: unsafe extern "C" fn(Obj, Obj, Obj),
        data: Obj,
    );
    fn g_file_mount_enclosing_volume_finish(file: Obj, result: Obj, error: *mut *mut GError)
        -> i32;
    fn g_mount_operation_new() -> Obj;
    fn g_mount_operation_set_username(op: Obj, value: *const c_char);
    fn g_mount_operation_set_password(op: Obj, value: *const c_char);
    fn g_mount_operation_set_domain(op: Obj, value: *const c_char);
    fn g_mount_operation_set_anonymous(op: Obj, value: i32);
    fn g_mount_operation_set_choice(op: Obj, value: i32);
    fn g_mount_operation_reply(op: Obj, value: i32);
    fn g_cancellable_new() -> Obj;
    fn g_cancellable_cancel(cancel: Obj);
    fn g_io_error_quark() -> u32;
}
#[link(name = "gobject-2.0")]
unsafe extern "C" {
    fn g_object_unref(object: Obj);
    fn g_signal_connect_data(
        instance: Obj,
        name: *const c_char,
        callback: unsafe extern "C" fn(),
        data: Obj,
        destroy: Option<unsafe extern "C" fn(Obj, Obj)>,
        flags: i32,
    ) -> u64;
}
#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_main_context_new() -> Obj;
    fn g_main_context_push_thread_default(context: Obj);
    fn g_main_context_pop_thread_default(context: Obj);
    fn g_main_context_iteration(context: Obj, block: i32) -> i32;
    fn g_main_context_unref(context: Obj);
    fn g_error_free(error: *mut GError);
    fn g_free(data: Obj);
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub uri: String,
    pub scheme: String,
    pub user: String,
}
pub fn parse(text: &str) -> Result<Address> {
    let text = text.trim();
    let uri = if text.contains("://") {
        text.to_owned()
    } else {
        format!("smb://{text}")
    };
    if uri.len() > 4096
        || uri
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "\"'`$;&|<>\\(){}*?!#".contains(c))
    {
        return Err("files.invalidAddress".into());
    }
    let (scheme, rest) = uri.split_once("://").ok_or("files.invalidAddress")?;
    if ![
        "smb", "nfs", "sftp", "ssh", "ftp", "ftps", "dav", "webdav", "davs", "rdp", "vnc", "http",
        "https",
    ]
    .contains(&scheme)
    {
        return Err("files.invalidAddress".into());
    }
    let authority = rest.split('/').next().unwrap_or("");
    let (user, host) = authority.rsplit_once('@').unwrap_or(("", authority));
    if !user.is_empty()
        && (!user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
            || user.starts_with('-'))
    {
        return Err("files.invalidAddress".into());
    }
    let name = if let Some((h, p)) = host.rsplit_once(':') {
        if p.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
            return Err("files.invalidAddress".into());
        }
        h
    } else {
        host
    };
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        || rest.split('/').any(|p| p == "..")
    {
        return Err("files.invalidAddress".into());
    }
    let backend = match scheme {
        "ssh" => "sftp",
        "webdav" => "dav",
        s => s,
    };
    Ok(Address {
        uri: format!("{backend}://{rest}"),
        scheme: scheme.into(),
        user: user.into(),
    })
}
pub struct Prompt {
    pub value: Value,
    pub reply: mpsc::Sender<Value>,
}
struct State {
    done: bool,
    error: Option<String>,
    prompt: Box<dyn Fn(Prompt) -> bool>,
    attempts: u8,
}
unsafe fn string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}
unsafe extern "C" fn password(
    op: Obj,
    _message: *const c_char,
    user: *const c_char,
    domain: *const c_char,
    flags: i32,
    data: Obj,
) {
    let state = &mut *data.cast::<State>();
    state.attempts += 1;
    if state.attempts > 3 {
        g_mount_operation_reply(op, 1);
        return;
    }
    let (tx, rx) = mpsc::channel();
    if !(state.prompt)(Prompt {
        value: json!({"type":"password","user":string(user),"domain":string(domain),"anonymous":flags&16!=0}),
        reply: tx,
    }) {
        g_mount_operation_reply(op, 1);
        return;
    }
    let response = rx.recv_timeout(Duration::from_secs(60)).ok();
    if let Some(v) = response.filter(|v| v["cancel"] != true) {
        let u = CString::new(v["user"].as_str().unwrap_or("")).unwrap_or_default();
        let d = CString::new(v["domain"].as_str().unwrap_or("")).unwrap_or_default();
        let mut secret = v["password"].as_str().unwrap_or("").as_bytes().to_vec();
        secret.retain(|b| *b != 0);
        secret.push(0);
        g_mount_operation_set_username(op, u.as_ptr());
        g_mount_operation_set_domain(op, d.as_ptr());
        g_mount_operation_set_password(op, secret.as_ptr().cast());
        secret.fill(0);
        g_mount_operation_set_anonymous(op, (v["guest"] == true) as i32);
        g_mount_operation_reply(op, 0);
    } else {
        g_mount_operation_reply(op, 1);
    }
}
unsafe extern "C" fn question(
    op: Obj,
    message: *const c_char,
    choices: *const *const c_char,
    data: Obj,
) {
    let state = &mut *data.cast::<State>();
    let mut labels = Vec::new();
    if !choices.is_null() {
        for n in 0..8 {
            let p = *choices.add(n);
            if p.is_null() {
                break;
            }
            labels.push(string(p));
        }
    }
    let (tx, rx) = mpsc::channel();
    let n = labels.len();
    if !(state.prompt)(Prompt {
        value: json!({"type":"question","message":string(message),"choices":labels}),
        reply: tx,
    }) {
        g_mount_operation_reply(op, 1);
        return;
    }
    if let Some(choice) = rx
        .recv_timeout(Duration::from_secs(60))
        .ok()
        .and_then(|v| v["choice"].as_u64())
        .filter(|v| (*v as usize) < n)
    {
        g_mount_operation_set_choice(op, choice as i32);
        g_mount_operation_reply(op, 0);
    } else {
        g_mount_operation_reply(op, 1);
    }
}
unsafe extern "C" fn finished(file: Obj, result: Obj, data: Obj) {
    let state = &mut *data.cast::<State>();
    let mut error = std::ptr::null_mut();
    let ok = g_file_mount_enclosing_volume_finish(file, result, &mut error);
    // G_IO_ERROR_ALREADY_MOUNTED (17) is success for Connect to Server.
    if ok == 0 && !error.is_null() {
        if !((*error).domain == g_io_error_quark() && (*error).code == 17) {
            state.error = Some(
                match (*error).code {
                    14 => "files.networkAuth",
                    19 => "files.networkCancelled",
                    _ => "files.networkFailed",
                }
                .into(),
            );
        }
    }
    if !error.is_null() {
        g_error_free(error);
    }
    state.done = true;
}
pub fn connect(address: Address, prompt: impl Fn(Prompt) -> bool + 'static) -> Result<PathBuf> {
    let uri = CString::new(address.uri).map_err(|_| "files.invalidAddress")?;
    // All objects and callbacks live on this worker's private GLib context.
    // Cancellation is completed before state or context is released.
    unsafe {
        let context = g_main_context_new();
        g_main_context_push_thread_default(context);
        let file = g_file_new_for_uri(uri.as_ptr());
        let op = g_mount_operation_new();
        let cancel = g_cancellable_new();
        let mut state = State {
            done: false,
            error: None,
            prompt: Box::new(prompt),
            attempts: 0,
        };
        let data = (&mut state as *mut State).cast();
        g_signal_connect_data(
            op,
            c"ask-password".as_ptr(),
            std::mem::transmute::<
                unsafe extern "C" fn(Obj, *const c_char, *const c_char, *const c_char, i32, Obj),
                unsafe extern "C" fn(),
            >(password),
            data,
            None,
            0,
        );
        g_signal_connect_data(
            op,
            c"ask-question".as_ptr(),
            std::mem::transmute::<
                unsafe extern "C" fn(Obj, *const c_char, *const *const c_char, Obj),
                unsafe extern "C" fn(),
            >(question),
            data,
            None,
            0,
        );
        g_file_mount_enclosing_volume(file, 0, op, cancel, finished, data);
        let start = Instant::now();
        let mut cancelled = false;
        while !state.done {
            while g_main_context_iteration(context, 0) != 0 {}
            if start.elapsed() > Duration::from_secs(45) && !cancelled {
                g_cancellable_cancel(cancel);
                cancelled = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = if let Some(e) = state.error {
            Err(e)
        } else {
            let path = g_file_get_path(file);
            if path.is_null() {
                Err("files.networkUnavailable".into())
            } else {
                use std::os::unix::ffi::OsStrExt;
                let p = PathBuf::from(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
                g_free(path.cast());
                policy::current().read(&p)
            }
        };
        g_object_unref(cancel);
        g_object_unref(op);
        g_object_unref(file);
        g_main_context_pop_thread_default(context);
        g_main_context_unref(context);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_are_data_and_credentials_are_separate() {
        assert_eq!(parse("nas/Public").unwrap().uri, "smb://nas/Public");
        assert_eq!(
            parse("ssh://ana@host:2222/home/ana").unwrap().uri,
            "sftp://ana@host:2222/home/ana"
        );
        for value in [
            "ssh://-oProxyCommand=x",
            "ssh://a;rm@host",
            "vnc://$(x)",
            "smb://a b",
            "nfs://h/../etc",
            "ssh://host:99999",
            "ssh://-x@host",
            "smb://h/a\\b",
            "smb://user:password@host",
            "",
            "gopher://host",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
    }
}

// Discovery belongs to the existing bounded, sandboxed LAN service. Rust turns
// its published facts into entries; no address supplied by a page is probed.
pub fn entries(state: &Value) -> Vec<super::operations::Entry> {
    let mut entries = Vec::new();
    for host in state["hosts"].as_array().into_iter().flatten().take(256) {
        let Some(addr) = host["addr"].as_str() else {
            continue;
        };
        if addr.parse::<std::net::Ipv4Addr>().is_err() {
            continue;
        }
        let name = host["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .unwrap_or(addr);
        let mut ways = Vec::<(String, String)>::new();
        for share in host["shares"].as_array().into_iter().flatten() {
            if let Some(share) = share["name"].as_str().filter(|n| !n.contains(['/', '\0'])) {
                ways.push((format!("{share} — {name}"), format!("smb://{addr}/{share}")));
            }
        }
        if host["smb"] == true && ways.is_empty() {
            ways.push((name.into(), format!("smb://{addr}/")));
        }
        for export in host["exports"].as_array().into_iter().flatten() {
            if let Some(path) = export["path"].as_str() {
                ways.push((format!("{path} — {name}"), format!("nfs://{addr}{path}")));
            }
        }
        for service in host["services"].as_array().into_iter().flatten() {
            let kind = service["kind"].as_str().unwrap_or("");
            let scheme = match kind {
                "ssh" | "sftp" => "sftp",
                "webdav" => "dav",
                "webdavs" => "davs",
                "ftp" | "ftps" | "http" | "https" | "rdp" => kind,
                "screen" => "vnc",
                _ => continue,
            };
            let port = service["port"]
                .as_u64()
                .filter(|p| *p > 0 && *p <= 65535)
                .map(|p| format!(":{p}"))
                .unwrap_or_default();
            let uri = format!("{scheme}://{addr}{port}");
            if !ways.iter().any(|(_, u)| u == &uri) {
                ways.push((format!("{name} ({scheme})"), uri));
            }
        }
        for (name, uri) in ways.into_iter().take(32) {
            if parse(&uri).is_err() {
                continue;
            }
            entries.push(super::operations::Entry {
                name,
                path: format!("net:{uri}"),
                raw: uri.into(),
                dir: false,
                link: false,
                size: 0,
                mtime: 0,
                mode: 0,
                writable: false,
                original: None,
                stamp: 0,
            });
        }
    }
    entries
}
