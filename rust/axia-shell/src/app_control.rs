//! Session commands borrow native groups and mutate their context's window owner.
use crate::winmodel::{AppCommand, Session, WindowReader};
use serde_json::{json, Value};
use std::rc::Rc;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod desktop_tests;
mod quit;
pub(crate) mod switcher_source;
#[cfg(test)]
mod quit_tests;
#[cfg(test)]
mod window_menu_tests;

#[derive(Default)]
pub struct Controller {
    app: crate::forcequit::App,
    windows: Option<Session>,
    quit: quit::GracefulQuit,
    preferences: Option<axia_state::Reader<Value>>,
}
impl Controller {
    pub fn new(windows: Session) -> Self {
        let app = crate::forcequit::App::with_window_metadata(Rc::new(WindowReader(windows.reader())), windows.catalog_store());
        windows.notification_service().borrow_mut().bind_catalog(app.metadata_reader());
        windows.restore_service().borrow_mut().bind_catalog(app.metadata_reader());
        windows.memory_service().borrow_mut().bind_catalog(app.metadata_reader());
        Self {
            app,
            windows: Some(windows),
            quit: Default::default(),
            preferences: None,
        }
    }
    pub(crate) fn bind_settings(&mut self, home: &str) {
        let home = crate::settings::profile(home);
        self.preferences = Some(crate::settings::with(&home, |s| s.reader()));
        if let Some(windows) = &self.windows {
            windows.desktop_service().borrow_mut().bind_settings(&home);
            windows.memory_service().borrow_mut().bind_settings(&home);
        }
    }
    pub(crate) fn take_services(&self) -> Option<Value> {
        self.windows.as_ref().and_then(Session::take_service_effects)
    }
    pub fn request(&mut self, input: &Value) -> Value {
        if matches!(input["cmd"].as_str(),Some("shellRoot" | "shellLayer" | "shellKeyboard" | "switcherStatus" | "desktopHudStatus")) {
            let Some(windows) = &mut self.windows else { return json!({"error":"app.invalidRequest"}); };
            match input["cmd"].as_str().unwrap() {
                "switcherStatus" => return windows.switcher_status(),
                "desktopHudStatus" => return windows.shell_scene().borrow().hud_status(),
                "shellKeyboard" => return json!({"busy":windows.switcher_reader().read().is_some()}),
                "shellRoot" => {
                    let Some(node) = input["node"].as_i64().and_then(|id|i32::try_from(id).ok()).filter(|_|windows.resident()) else {
                        return json!({"error":"app.invalidRequest"});
                    };
                    let scene = windows.shell_scene();
                    return json!({"bound":scene.borrow_mut().bind_root(windows,node)});
                },
                "shellLayer" => {
                    let Some(z) = input["z"].as_i64().and_then(|z|i32::try_from(z).ok()).filter(|_|windows.resident()) else {
                        return json!({"error":"app.invalidRequest"});
                    };
                    return json!({"node":windows.shell_scene().borrow_mut().layer(z).unwrap_or(0)});
                },
                _ => unreachable!(),
            }
        }
        if input["cmd"] == "switcherPrepare" {
            let Some(windows) = &mut self.windows else {
                return json!({"error":"app.invalidRequest"});
            };
            let (fullscreen,surface) = switcher_source::current();
            let hold = match input.get("hold") {
                None => 0,
                Some(value) => match value.as_i64().and_then(|value|i32::try_from(value).ok()).filter(|value|*value>=0) {
                    Some(value) => value,
                    None => return json!({"error":"app.invalidRequest"}),
                },
            };
            let back = match input.get("back") { None => false, Some(Value::Bool(value)) => *value, _ => return json!({"error":"app.invalidRequest"}) };
            // Settle the previous profile before creating a new opening. A later
            // scene tick must not cancel it for a preference that already changed.
            if windows.resident() { windows.shell_scene().borrow_mut().sync_input(windows); }
            return windows.prepare_switcher(fullscreen,surface,hold,back)
                .unwrap_or_else(|_|json!({"error":"app.invalidRequest"}));
        }
        if input["cmd"] == "switcherKey" {
            let nonce = match input.get("nonce") {
                Some(value) => value.as_u64(),
                None => self.windows.as_ref().and_then(|windows|windows.switcher_reader().read().as_ref().map(|opening|opening.nonce)),
            };
            let (Some(windows), Some(key), Some(mods), Some(down), Some(shift)) = (
                &mut self.windows, input["key"].as_str(),
                input["mods"].as_i64().and_then(|value|i32::try_from(value).ok()).filter(|value|*value>=0),
                input["down"].as_bool(), input["shift"].as_bool(),
            ) else { return json!({"error":"app.invalidRequest"}); };
            let Some(nonce) = nonce else {
                return if input.get("nonce").is_some() { json!({"error":"app.invalidRequest"}) } else { json!({"consumed":false}) };
            };
            let reply = windows.switcher_key(nonce,key,mods,down,shift);
            if reply["action"].is_array() { windows.switcher_action(nonce,reply["action"].clone()); }
            return reply;
        }
        if input["cmd"] == "switcherCancel" {
            let Some(windows) = &mut self.windows else {
                return json!({"error":"app.invalidRequest"});
            };
            let nonce = match input.get("nonce") {
                Some(value) => match value.as_u64() { Some(nonce)=>nonce,None=>return json!({"error":"app.invalidRequest"}) },
                None => match windows.switcher_reader().read().as_ref() { Some(opening)=>opening.nonce,None=>return json!({"result":false}) },
            };
            return windows.cancel_switcher(nonce)
                .unwrap_or_else(|_|json!({"error":"app.invalidRequest"}));
        }
        if input["cmd"] == "clockCommand" {
            let Some(windows) = &self.windows else { return json!({"error":"app.invalidRequest"}); };
            // Historical/reference times belong to pure calculations only.
            // Session mutations and scheduling always use the native clock.
            let wall = if matches!(input["action"].as_str(), Some("nextFire" | "timerLeft" | "swElapsed")) {
                input["now"].as_i64().unwrap_or_else(axia_clock::now)
            } else { axia_clock::now() };
            return windows.notification_service().borrow_mut().clock_command(input, wall, std::time::Instant::now());
        }
        if input["cmd"] == "notifications" {
            let Some(windows) = &self.windows else { return json!({"error":"app.invalidRequest"}); };
            return windows.notification_service().borrow_mut().command(input, std::time::Instant::now());
        }
        if input["cmd"] == "desktopSession" {
            let Some(windows)=&mut self.windows else { return json!({"error":"app.invalidRequest"}); };
            let service=windows.desktop_service();
            let reply = service.borrow_mut().command(windows,input);
            if windows.resident() && reply["result"]["boundary"]==true { windows.shell_scene().borrow_mut().show_desktop_hud(windows); }
            return reply;
        }
        if input["cmd"] == "sessionRestore" {
            let Some(windows)=&mut self.windows else { return json!({"error":"app.invalidRequest"}); };
            let service=windows.restore_service();
            return service.borrow_mut().command(windows,&Value::Null,input,std::time::Instant::now());
        }
        if input["cmd"] == "memoryStart" {
            let Some(windows) = &self.windows else { return json!({"error":"app.invalidRequest"}); };
            if !input["enabled"].is_boolean() { return json!({"error":"app.invalidRequest"}); }
            windows.memory_service().borrow_mut().enable(input["enabled"] == true);
            return json!({"ok":true});
        }
        if input["cmd"] == "memoryFrame" {
            return self
                .windows
                .as_ref()
                .map(|windows| {
                    let catalog = self.app.metadata_reader();
                    let metadata = catalog.read();
                    if let Some(reader) = &self.preferences {
                        crate::window_memory::restore_profile(&windows.reader(), &metadata, input, &reader.read())
                    } else { crate::window_memory::restore(&windows.reader(), &metadata, input) }
                })
                .unwrap_or_else(|| json!({"error":"app.invalidRequest"}));
        }
        if input["cmd"] == "desktopCommand" {
            let command = match input["action"].as_str() {
                Some("step") => match input["step"].as_i64() {
                    Some(step) => AppCommand::StepDesktop(step),
                    None => return json!({"error":"app.invalidRequest"}),
                },
                Some("add") => AppCommand::AddDesktopAndShow,
                Some("closeActive") => AppCommand::CloseActiveDesktop,
                Some("swipe") => match (input["direction"].as_str(), input["rtl"].as_bool()) {
                    (Some(dir @ ("left" | "right")), Some(rtl)) => AppCommand::SwipeDesktop { left: dir == "left", rtl },
                    _ => return json!({"error":"app.invalidRequest"}),
                },
                _ => return json!({"error":"app.invalidRequest"}),
            };
            let Some(windows) = &mut self.windows else { return json!({"error":"app.invalidRequest"}); };
            let reply = windows.app_command(command,&[]).unwrap_or_else(|_|json!({"error":"app.invalidRequest"}));
            if windows.resident() && reply["result"]["boundary"]==true { windows.shell_scene().borrow_mut().show_desktop_hud(windows); }
            return reply;
        }
        match input["cmd"].as_str() {
            Some("groupInfo" | "group" | "quitApp" | "quitAppNext") => {
                let Some(windows) = &self.windows else {
                    return json!({"error":"app.invalidRequest"});
                };
                if matches!(input["cmd"].as_str(), Some("quitApp" | "quitAppNext")) {
                    return self.quit.request(input, windows, &self.app);
                }
                let groups = self.app.command_groups();
                let reader = windows.reader();
                let state = reader.read();
                let Some(group) = selected_group(&groups, &state, input) else {
                    return json!({"group":null});
                };
                let mut result = json!({"key":group.key,"name":group.name});
                if input["cmd"] == "group" {
                    result["windows"] = json!(group
                        .windows
                        .iter()
                        .map(|w| w.id.as_str())
                        .collect::<Vec<_>>());
                }
                return json!({"group":result});
            }
            _ => {}
        }
        if input["cmd"] != "windowCommand" {
            return self.app.request(input);
        }
        let command = match input["action"].as_str() {
            Some("cycle") => AppCommand::Cycle(input["back"] == true),
            Some("hide") => AppCommand::Hide,
            Some("hideOthers") => AppCommand::HideOthers,
            Some("showAll") => AppCommand::ShowAll,
            Some("desktop") => AppCommand::ToggleDesktop,
            Some("bringAll") => AppCommand::BringAll,
            Some("arrange") => AppCommand::Arrange(match input["layout"].as_str() {
                Some("left-right") => "left-right",
                Some("right-left") => "right-left",
                Some("top-bottom") => "top-bottom",
                Some("bottom-top") => "bottom-top",
                Some("quarters") => "quarters",
                _ => return json!({"error":"app.invalidRequest"}),
            }),
            _ => return json!({"error":"app.invalidRequest"}),
        };
        let Some(windows) = &mut self.windows else {
            return json!({"error":"app.invalidRequest"});
        };
        window_command(windows, &self.app, command, input["id"].as_str(), &input["facts"])
    }
}

pub(crate) fn window_command(
    windows: &mut Session, app: &crate::forcequit::App, command: AppCommand,
    target: Option<&str>, facts: &Value,
) -> Value {
    let reader = windows.reader();
    let state = reader.read();
    if matches!(command, AppCommand::WindowAction(_)) {
        let id = target.or_else(|| state.focused_window()).map(str::to_owned);
        drop(state);
        return id.as_deref().map_or_else(|| json!({"result":null,"state":null}), |id|
            windows.app_command(command,&[id]).unwrap_or_else(|_|json!({"error":"app.invalidRequest"})));
    }
    let id = if matches!(command, AppCommand::Cycle(_)) {
        state.focused_window()
    } else {
        target.or_else(|| state.focused_window())
    };
    let groups = app.command_groups();
    let selected = groups
        .iter()
        .find(|group| group.windows.iter().any(|w| Some(w.id.as_str()) == id));
    if matches!(
        command,
        AppCommand::Cycle(_) | AppCommand::Hide | AppCommand::HideOthers
    ) && selected.is_none()
    {
        return json!({"result":null,"state":null});
    }
    let ids: Vec<_> = groups
        .iter()
        .filter(|group| match command {
            AppCommand::Cycle(_) | AppCommand::Hide => {
                selected.is_some_and(|selected| selected.key == group.key)
            }
            AppCommand::HideOthers => {
                selected.is_some_and(|selected| selected.key != group.key)
            }
            _ => true,
        })
        .flat_map(|group| group.windows.iter().map(|w| w.id.as_str()))
        .collect();
    if matches!(command, AppCommand::BringAll | AppCommand::Arrange(_)) {
        let mut stack: Vec<_> = state
            .windows()
            .filter(|w| {
                ids.contains(&w.id())
                    && !w.minimized()
                    && (w.pip() || w.desktop() == state.active_desktop())
            })
            .map(|w| w.id().to_owned())
            .collect();
        if let Some(anchor) = target
            .filter(|id| stack.iter().any(|w| w == id))
        {
            stack.retain(|id| id != anchor);
            if matches!(command, AppCommand::Arrange(_)) {
                stack.reverse();
                stack.insert(0, anchor.to_owned());
            } else {
                stack.push(anchor.to_owned());
            }
        } else if matches!(command, AppCommand::Arrange(_)) {
            return json!({"result":null,"state":null});
        }
        drop(state);
        return windows
            .app_command_with_facts(
                command,
                &stack.iter().map(String::as_str).collect::<Vec<_>>(),
                facts,
            )
            .unwrap_or_else(|_| json!({"error":"app.invalidRequest"}));
    }
    drop(state);
    windows
        .app_command(command, &ids)
        .unwrap_or_else(|_| json!({"error":"app.invalidRequest"}))
}

fn selected_group<'a>(
    groups: &'a [crate::forcequit::Group],
    state: &crate::winmodel::WindowState,
    input: &Value,
) -> Option<&'a crate::forcequit::Group> {
    if let Some(key) = input["key"].as_str() {
        return groups.iter().find(|group| group.key == key);
    }
    let id = input["id"].as_str().or_else(|| state.focused_window())?;
    groups
        .iter()
        .find(|group| group.windows.iter().any(|w| w.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(session: &mut Session, op: &str, args: Vec<Value>) {
        session.call(op, &[vec![json!({})], args].concat()).unwrap();
    }
    fn controller(session: &Session) -> Controller {
        let mut controller = Controller::new(session.clone());
        assert_eq!(controller.request(&json!({"cmd":"metadata","registry":[
            {"id":"notes"},{"id":"calculator"},{"id":"panel","builtin":true},
            {"id":"linux","builtin":true},{"id":"forcequit"}],
            "catalog":[{"id":"desktop:term","kind":"linux","desktopId":"org.term","wmClass":"Terminal"}]}))["ok"], true);
        controller
    }
    fn command(controller: &mut Controller, action: &str, id: &str, back: bool) -> Value {
        controller.request(
            &json!({"cmd":"windowCommand","action":action,"id":id,"back":back,
            "windows":[{"id":"forged","app":"notes"}]}),
        )
    }
    #[test]
    fn cycles_grouped_windows_in_one_commit_with_the_context_owner() {
        let mut session = Session::default();
        for id in ["a", "b", "c"] {
            call(
                &mut session,
                "open",
                vec![json!({"app":"notes","instance":id})],
            );
        }
        call(&mut session, "setDesktops", vec![json!([1, 2]), json!(1)]);
        call(
            &mut session,
            "open",
            vec![json!({"app":"notes","instance":"away","desktop":2})],
        );
        call(
            &mut session,
            "open",
            vec![json!({"app":"notes","instance":"hidden"})],
        );
        call(&mut session, "minimize", vec![json!("notes#hidden")]);
        let mut control = controller(&session);
        let reader = session.reader();
        let revision = reader.revision();
        command(&mut control, "cycle", "stale", false);
        assert_eq!(reader.read().focused_window(), Some("notes#a"));
        assert_eq!(reader.revision(), revision + 1);
        command(&mut control, "cycle", "", true);
        assert_eq!(reader.read().focused_window(), Some("notes#c"));
        assert_eq!(reader.revision(), revision + 2);
        assert!(reader.read().window("notes#hidden").unwrap().minimized());
        let other = Session::default();
        assert_eq!(other.reader().read().windows().count(), 0);
        drop(session);
        command(&mut control, "hide", "notes#c", false);
        assert!(reader.read().window("notes#away").unwrap().minimized());
    }
    #[test]
    fn linux_classes_share_a_group_and_unrelated_unknown_apps_do_not() {
        let mut session = Session::default();
        for (id, app_id) in [
            (1, "org.term"),
            (2, "Terminal"),
            (3, "unknown"),
            (4, "Terminal"),
        ] {
            call(
                &mut session,
                "open",
                vec![json!({"app":"linux","instance":id,"args":{"appId":app_id}})],
            );
        }
        let mut control = controller(&session);
        command(&mut control, "cycle", "", false);
        assert_eq!(session.reader().read().focused_window(), Some("linux#1"));
        command(&mut control, "hide", "linux#2", false);
        let reader = session.reader();
        let state = reader.read();
        assert!([1, 2, 4]
            .iter()
            .all(|id| state.window(&format!("linux#{id}")).unwrap().minimized()));
        assert!(!state.window("linux#3").unwrap().minimized());
    }
    #[test]
    fn hide_others_respects_desktops_and_panels_and_show_all_restores_minimized() {
        let mut session = Session::default();
        call(&mut session, "setDesktops", vec![json!([1, 2]), json!(1)]);
        for input in [
            json!({"app":"notes"}),
            json!({"app":"calculator"}),
            json!({"app":"panel"}),
            json!({"app":"notes","instance":"away","desktop":2}),
            json!({"app":"forcequit"}),
        ] {
            call(&mut session, "open", vec![input]);
        }
        let mut control = controller(&session);
        command(&mut control, "hideOthers", "notes", false);
        let reader = session.reader();
        {
            let state = reader.read();
            assert!(state.window("calculator").unwrap().minimized());
            assert!(state.window("forcequit").unwrap().minimized());
            assert!(!state.window("notes").unwrap().minimized());
            assert!(!state.window("panel").unwrap().minimized());
            assert!(!state.window("notes#away").unwrap().minimized());
        }
        command(&mut control, "showAll", "", false);
        assert!(reader.read().windows().all(|w| !w.minimized()));
        command(&mut control, "hide", "notes", false);
        assert!(reader.read().window("notes#away").unwrap().minimized());
    }
    #[test]
    fn show_desktop_remembers_only_its_own_windows_and_ignores_closed_or_moved_members() {
        let mut session = Session::default();
        call(&mut session, "setDesktops", vec![json!([1, 2]), json!(1)]);
        for input in [
            json!({"app":"notes"}),
            json!({"app":"calculator"}),
            json!({"app":"panel"}),
            json!({"app":"notes","instance":"away","desktop":2}),
        ] {
            call(&mut session, "open", vec![input]);
        }
        call(&mut session, "minimize", vec![json!("calculator")]);
        let mut control = controller(&session);
        command(&mut control, "desktop", "", false);
        let reader = session.reader();
        assert!(reader.read().window("notes").unwrap().minimized());
        assert!(!reader.read().window("panel").unwrap().minimized());
        call(&mut session, "setActiveDesktop", vec![json!(2)]);
        command(&mut control, "desktop", "", false);
        command(&mut control, "desktop", "", false);
        assert!(!reader.read().window("notes#away").unwrap().minimized());
        call(&mut session, "setActiveDesktop", vec![json!(1)]);
        command(&mut control, "desktop", "", false);
        assert!(!reader.read().window("notes").unwrap().minimized());
        assert!(reader.read().window("calculator").unwrap().minimized());
        command(&mut control, "desktop", "", false);
        call(
            &mut session,
            "moveToDesktop",
            vec![json!("notes"), json!(2)],
        );
        command(&mut control, "desktop", "", false);
        assert!(reader.read().window("notes").unwrap().minimized());
        call(&mut session, "close", vec![json!("notes")]);
        command(&mut control, "desktop", "", false);
        let revision = reader.revision();
        command(&mut control, "hide", "missing", false);
        command(&mut control, "cycle", "", false);
        assert_eq!(reader.revision(), revision);
        assert_eq!(
            Controller::default().request(&json!({"cmd":"windowCommand","action":"showAll"}))
                ["error"],
            "app.invalidRequest"
        );
        assert!(session.call("appControl", &[]).is_err());
    }
}
