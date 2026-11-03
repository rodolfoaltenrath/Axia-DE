use super::{
    io_error,
    model::{Session, Shared},
    operations as ops, policy, ui,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::path::Path;

#[derive(Default)]
struct App {
    sessions: HashMap<u64, Session>,
    shared: Shared,
    sequence: u64,
    delivery: u64,
}
thread_local! {static APP:RefCell<App>=RefCell::new(App::default());}
impl App {
    fn request(&mut self, args: &Value) -> Value {
        let op = args["op"].as_str().unwrap_or("");
        if op == "icon" {
            return ui::icon(
                args["name"].as_str().unwrap_or("files"),
                args["size"].as_f64().unwrap_or(32.).clamp(8., 256.),
            );
        }
        if op == "fs" {
            return fs_json(args);
        }
        if op == "create" {
            if self.sessions.len() >= 64 {
                return json!({"error":"fs.tooManyCopies"});
            }
            self.sequence += 1;
            let id = self.sequence;
            return match Session::new(id, args, &mut self.shared) {
                Ok(mut session) => {
                    session.poll(args, &self.shared);
                    let reply = self.snapshot(&mut session);
                    self.sessions.insert(id, session);
                    reply
                }
                Err(e) => json!({"error":e}),
            };
        }
        let id = args["id"].as_u64().unwrap_or(0);
        let Some(mut session) = self.sessions.remove(&id) else {
            return json!({"error":"sys.badArgument"});
        };
        if op == "close" {
            session
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Value::Null;
        }
        if op == "event" {
            session.event(args, &mut self.shared);
            for s in self.sessions.values_mut() {
                s.dirty = true;
            }
        }
        session.poll(args, &self.shared);
        let reply = if session.dirty || !session.effects.is_empty() {
            self.snapshot(&mut session)
        } else {
            Value::Null
        };
        self.sessions.insert(id, session);
        reply
    }
    fn snapshot(&mut self, s: &mut Session) -> Value {
        self.delivery += 1;
        let tree = s.render(&self.shared);
        let menus = s.menus(&self.shared);
        let title = s.title_value();
        s.dirty = false;
        json!({"id":s.id,"serial":self.delivery,"tree":tree,"menus":menus,"title":title,"effects":std::mem::take(&mut s.effects),"loading":s.loading,"capture":s.drag.is_some(),"closed":s.closed})
    }
}

fn fs_json(args: &Value) -> Value {
    let p = policy::current();
    let path = Path::new(args["path"].as_str().unwrap_or("."));
    let result = (|| -> super::Result<Value> {
        Ok(match args["method"].as_str().unwrap_or("") {
            "list" => json!(ops::list(&p, path)?),
            "stat" => {
                let real = p.read(path)?;
                let e = ops::entry(&real).map_err(io_error)?;
                let mut value = json!(e);
                value["mode"] = json!(format!("{:04o}", e.mode & 0o7777));
                value
            }
            "free" => {
                let real = p.read(path)?;
                let (total, free) = ops::free(&real).map_err(io_error)?;
                json!({"total":total,"free":free})
            }
            "mkdir" => {
                ops::mkdir(path)?;
                json!(true)
            }
            _ => return Err("sys.badArgument".into()),
        })
    })();
    match result {
        Ok(value) => json!({"value":value}),
        Err(e) => json!({"error":e}),
    }
}

// SAFETY: input is a NUL-terminated UTF-8 JSON string owned by C for this call.
// All responses use a dedicated free function; QuickJS never owns Rust allocations.
#[no_mangle]
pub unsafe extern "C" fn axia_files_request(input: *const c_char) -> *mut c_char {
    let reply = (|| {
        if input.is_null() {
            return json!({"error":"sys.badArgument"});
        }
        let bytes = CStr::from_ptr(input).to_bytes();
        if bytes.len() > 1024 * 1024 {
            return json!({"error":"sys.badArgument"});
        }
        let args: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(_) => return json!({"error":"sys.badArgument"}),
        };
        APP.with(|app| app.borrow_mut().request(&args))
    })();
    CString::new(serde_json::to_string(&reply).unwrap_or_else(|_| "null".into()))
        .unwrap()
        .into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn axia_files_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_handles_and_inputs_do_not_create_sessions() {
        let mut app = App::default();
        assert!(app.request(&json!({"op":"event","id":123}))["error"].is_string());
        assert!(app.sessions.is_empty());
    }
    #[test]
    fn icon_has_vector_geometry_without_emoji() {
        let icon = ui::icon("files", 32.);
        assert_eq!(icon["type"], "view");
        assert!(!icon["children"].as_array().unwrap().is_empty());
    }
}
