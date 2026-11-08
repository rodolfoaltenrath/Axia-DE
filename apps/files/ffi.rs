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

// What a window's host already has, for delta() and for skipping empty replies.
struct Sent {
    tree: Value,
    state: Value,
}
#[derive(Default)]
struct App {
    sessions: HashMap<u64, Session>,
    sent: HashMap<u64, Sent>,
    shared: Shared,
    sequence: u64,
    delivery: u64,
}
thread_local! {static APP:RefCell<App>=RefCell::new(App::default());}
impl App {
    fn request(&mut self, args: &Value) -> Value {
        let op = args["op"].as_str().unwrap_or("");
        if op == "externalPrepare" || op == "externalUris" {
            return match if op == "externalPrepare" {
                super::external::prepare(args)
            } else {
                super::external::uris(args)
            } {
                Ok(value) => value,
                Err(error) => json!({"error":error}),
            };
        }
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
        if op == "volumeStart" {
            let device = args["device"].as_str().unwrap_or("");
            let volume = self
                .shared
                .volumes
                .iter()
                .find(|v| v.id == device || v.disk == device);
            let Some(volume) = volume else {
                return json!({"error":"files.deviceUnavailable"});
            };
            let plan = if args["action"] == "mount" {
                if !volume.mountable {
                    return json!({"error":"files.deviceUnsupported"});
                }
                super::udisks::Plan::Mount(volume.id.clone())
            } else if args["action"] == "release" {
                let parts = self
                    .shared
                    .volumes
                    .iter()
                    .filter(|v| {
                        v.path.is_some()
                            && if volume.removable {
                                v.disk == volume.disk
                            } else {
                                v.id == volume.id
                            }
                    })
                    .map(|v| v.id.clone())
                    .collect();
                super::udisks::Plan::Release {
                    disk: volume.disk.clone(),
                    parts,
                    removable: volume.removable,
                }
            } else {
                return json!({"error":"sys.badArgument"});
            };
            return match super::udisks::start(plan) {
                Ok(job) => json!({"job":job}),
                Err(error) => json!({"error":error}),
            };
        }
        if op == "volumePoll" {
            return super::udisks::poll(args["job"].as_u64().unwrap_or(0));
        }
        let id = args["id"].as_u64().unwrap_or(0);
        let Some(mut session) = self.sessions.remove(&id) else {
            return json!({"error":"sys.badArgument"});
        };
        if op == "close" {
            session.flush_zoom(&self.shared);
            self.sent.remove(&id);
            session
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Value::Null;
        }
        if op == "event" {
            session.event(args, &mut self.shared);
            // These only touch their own window; the others need not rebuild.
            let local = [
                "wheel",
                "columnWheel",
                "scroll",
                "pointerMove",
                "noop",
                "select",
                "clear",
                "query",
            ]
            .contains(&args["action"][0].as_str().unwrap_or(""));
            if !local {
                for s in self.sessions.values_mut() {
                    s.dirty = true;
                }
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
        let tree = s.render(&self.shared);
        let state = json!({"menus":s.menus(&self.shared),"title":s.title_value(),"loading":s.visibly_loading(),"capture":s.drag.is_some(),"closed":s.closed,
            "appearance":s.theme,"chrome":crate::native_style::frame(&s.theme)});
        s.dirty = false;
        let sent = self.sent.get(&s.id);
        if s.effects.is_empty() && sent.is_some_and(|p| p.tree == tree && p.state == state) {
            return Value::Null;
        }
        self.delivery += 1;
        let mut reply = state.clone();
        reply["id"] = json!(s.id);
        reply["serial"] = json!(self.delivery);
        reply["effects"] = json!(std::mem::take(&mut s.effects));
        reply["tree"] = delta(sent.map(|p| &p.tree), &tree);
        // Absent means unchanged; the host keeps the array it already has.
        for key in ["menus", "appearance", "chrome"] {
            if sent.is_some_and(|p| p.state[key] == state[key]) {
                reply.as_object_mut().unwrap().remove(key);
            }
        }
        self.sent.insert(s.id, Sent { tree, state });
        reply
    }
}

// The widget tree with every unchanged subtree replaced by {"$":1}. A child is
// compared with the previous child of the same key, or at the same position.
// Parsing the full description in QuickJS costs more than building it.
fn delta(prev: Option<&Value>, next: &Value) -> Value {
    if prev == Some(next) {
        return match next.get("key") {
            Some(key) => json!({"$":1,"key":key}),
            None => json!({"$":1}),
        };
    }
    let Some(children) = next["children"].as_array().filter(|c| !c.is_empty()) else {
        return next.clone();
    };
    let before: &[Value] = prev
        .and_then(|p| p["children"].as_array())
        .map_or(&[], Vec::as_slice);
    let keyed: HashMap<&str, &Value> = before
        .iter()
        .filter_map(|c| Some((c.get("key")?.as_str()?, c)))
        .collect();
    let mut out = serde_json::Map::new();
    for (name, value) in next.as_object().into_iter().flatten() {
        if name != "children" {
            out.insert(name.clone(), value.clone());
        }
    }
    let children = children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let old = match child.get("key").and_then(Value::as_str) {
                Some(key) if !keyed.is_empty() => keyed.get(key).copied(),
                _ => before.get(i),
            };
            delta(old, child)
        })
        .collect();
    out.insert("children".into(), Value::Array(children));
    Value::Object(out)
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
    // The host's half of the protocol: sdk/rustfiles.jsx expand().
    fn expand(prev: Option<&Value>, node: &Value) -> Value {
        if node.get("$").is_some() {
            return prev.unwrap().clone();
        }
        let mut out = node.clone();
        if let Some(children) = node["children"].as_array() {
            let before = prev
                .and_then(|p| p["children"].as_array())
                .map_or(&[][..], Vec::as_slice);
            let keyed: HashMap<_, _> = before
                .iter()
                .filter_map(|c| Some((c.get("key")?.as_str()?, c)))
                .collect();
            out["children"] = children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    let old = match child.get("key").and_then(Value::as_str) {
                        Some(key) if !keyed.is_empty() => keyed.get(key).copied(),
                        _ => before.get(i),
                    };
                    expand(old, child)
                })
                .collect();
        }
        out
    }
    #[test]
    fn delta_rebuilds_the_same_tree_across_moved_added_and_removed_children() {
        let a = |key: &str, text: &str| json!({"key":key,"type":"text","text":text});
        let first = json!({"type":"view","style":{"top":0},"children":[
            a("x","1"),a("y","2"),a("z","3"),{"type":"text","text":"plain"},
            {"type":"view","children":[{"type":"text","text":"deep"}]}]});
        for second in [
            first.clone(),
            json!({"type":"view","style":{"top":-9},"children":[a("y","2"),a("z","3"),a("w","4")]}),
            json!({"type":"view","children":[a("z","changed"),a("x","1"),{"type":"text","text":"new"}]}),
            json!({"type":"view","children":[{"type":"text","text":"plain"},{"type":"view","children":[]}]}),
            json!({"type":"view"}),
        ] {
            let wire = delta(Some(&first), &second);
            assert_eq!(expand(Some(&first), &wire), second);
        }
        assert_eq!(delta(Some(&first), &first), json!({"$":1}));
        assert_eq!(delta(None, &first), first);
    }
    #[test]
    fn replies_carry_only_changes_and_nothing_when_nothing_changed() {
        let home = std::env::temp_dir().join(format!("axia-delta-{}", std::process::id()));
        let dir = home.join("many");
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..200 {
            std::fs::write(dir.join(format!("item-{n:03}.txt")), "").unwrap();
        }
        let mut app = App::default();
        let args = json!({"op":"create","mode":"browser","args":{"path":dir},"width":900,"height":540,"preferences":{},"apps":[]});
        let first = app.request(&args);
        assert_eq!(first["appearance"]["nativeStyle"], "adwaita");
        let id = first["id"].as_u64().unwrap();
        let mut host = first["tree"].clone();
        for _ in 0..500 {
            let r = app.request(&json!({"op":"poll","id":id,"width":900,"height":540}));
            if !r.is_null() {
                host = expand(Some(&host), &r["tree"]);
            }
            if r["loading"] == false && host.to_string().contains("item-000") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let full = host.to_string().len();
        assert!(host.to_string().contains("item-000"));
        let wheel = json!({"op":"event","id":id,"action":["wheel"],"deltaY":10});
        let reply = app.request(&wheel);
        assert!(
            reply.get("appearance").is_none() && reply.get("chrome").is_none(),
            "scrolling does not resend unchanged styling or update the shell frame"
        );
        assert!(
            reply["tree"].to_string().len() < full / 4,
            "a small scroll does not resend the file cells"
        );
        assert_eq!(
            expand(Some(&host), &reply["tree"]),
            app.sent[&id].tree,
            "the host ends up with the tree Rust described"
        );
        // Selecting the same file twice: the second reply has nothing to say.
        let select = json!({"op":"event","id":id,"action":["select",0]});
        assert!(!app.request(&select).is_null());
        assert!(app.request(&select).is_null());
        assert!(app.request(&json!({"op":"poll","id":id})).is_null());
        let legacy=app.request(&json!({"op":"poll","id":id,"theme":{"filesAppearance":"axia","mode":"dark","windowBg":"#123456"}}));
        assert!(
            legacy.get("chrome").is_some() && legacy["chrome"].is_null(),
            "restoring the old style explicitly clears the custom frame"
        );
        assert_eq!(legacy["appearance"]["windowBg"], "#123456");
        app.request(&json!({"op":"close","id":id}));
        assert!(app.sent.is_empty());
        std::fs::remove_dir_all(home).unwrap();
    }
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
