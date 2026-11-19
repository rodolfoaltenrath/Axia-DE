//! A transient desktop indicator, driven by native revisions and one expiry.
use super::NativeApp;
use crate::winmodel::WindowState;
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const VISIBLE: Duration = Duration::from_millis(800);

pub struct DesktopHud {
    windows: Reader<WindowState>,
    changes: Subscription,
    active: Option<i64>,
    until: Option<Instant>,
    last: Value,
}

impl DesktopHud {
    pub fn new(windows: Reader<WindowState>) -> Self {
        let changes = windows.subscribe();
        Self {
            windows,
            changes,
            active: None,
            until: None,
            last: Value::Null,
        }
    }

    fn update(&mut self, now: Instant, requested: bool) {
        self.changes.take();
        let state = self.windows.read();
        let active = state.active_desktop();
        let switched = self.active.is_some_and(|old| old != active);
        self.active = Some(active);
        if state.desktops().len() < 2 || state.overview() {
            self.until = None;
        } else if switched || requested {
            self.until = Some(now + VISIBLE);
        } else if self.until.is_some_and(|until| now >= until) {
            self.until = None;
        }
    }

    fn tree(&self, input: &Value) -> Value {
        if self.until.is_none() {
            return json!({"kind":"view","key":"desktop-hud","style":{"pointerEvents":"none"}});
        }
        let state = self.windows.read();
        let theme = &input["context"]["theme"];
        let display = &input["args"]["display"];
        let width = display["w"].as_f64().unwrap_or(0.);
        let height = display["h"].as_f64().unwrap_or(0.);
        let active = state.active_desktop();
        let position = state
            .desktops()
            .iter()
            .position(|id| *id == active)
            .unwrap_or(0)
            + 1;
        let dots: Vec<_> = state.desktops().iter().map(|id| json!({
            "kind":"view","key":format!("desktop-{id}"),
            "style":{"width":18,"height":6,"borderRadius":3,
                "backgroundColor":if *id == active {&theme["accent"]} else {&theme["separator"]}}
        })).collect();
        json!({"kind":"view","key":"desktop-hud","style":{
            "position":"absolute","left":display["x"],
            "top":display["y"].as_f64().unwrap_or(0.)+(height*0.14).round(),
            "width":width,"alignItems":"center","pointerEvents":"none"},"children":[{
            "kind":"view","key":"indicator","style":{"paddingHorizontal":16,"paddingVertical":10,
                "gap":8,"alignItems":"center","borderRadius":theme["groupRadius"].as_f64().unwrap_or(10.)+4.,
                "backgroundColor":if theme["mode"]=="dark" {"rgba(30,30,34,0.92)"} else {"rgba(245,245,247,0.94)"},
                "border":format!("1px solid {}",theme["separator"].as_str().unwrap_or("#777")),
                "boxShadow":"0 10px 28px rgba(0,0,0,0.35)"},"children":[{
                    "kind":"text","key":"caption","label":{"key":"shell.desktops.name","params":{"n":position}},
                    "style":{"fontSize":13,"fontWeight":600,"color":theme["text"],"whiteSpace":"nowrap"}
                },{"kind":"view","key":"dots","style":{"flexDirection":"row","gap":6},"children":dots}]
        }]})
    }
}

impl NativeApp for DesktopHud {
    fn invalidated(&self) -> bool {
        self.changes.pending()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.until.map(|until| {
            until
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                * 1000.0
                + 0.01
        })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str() {
            Some("show") => {
                self.update(Instant::now(), true);
                json!({})
            }
            Some("sync") => {
                self.update(Instant::now(), false);
                let tree = self.tree(input);
                if self.last == tree {
                    return json!({"same":true});
                }
                self.last = tree.clone();
                json!({"tree":tree})
            }
            _ => json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winmodel::Session;

    #[test]
    fn revisions_show_once_extend_from_the_latest_switch_and_expire_without_polling() {
        let mut session = Session::default();
        session
            .call("setDesktops", &[json!({}), json!([1, 2, 3]), json!(1)])
            .unwrap();
        let mut hud = DesktopHud::new(session.reader());
        let now = Instant::now();
        hud.update(now, false);
        assert!(hud.until.is_none());
        assert!(hud.refresh_ms().is_none());
        session
            .call("setActiveDesktop", &[json!({}), json!(2)])
            .unwrap();
        assert!(hud.invalidated());
        hud.update(now, false);
        assert_eq!(hud.until, Some(now + VISIBLE));
        assert!(!hud.invalidated());
        hud.update(now + Duration::from_millis(500), false);
        assert_eq!(hud.until, Some(now + VISIBLE));
        session
            .call("setActiveDesktop", &[json!({}), json!(3)])
            .unwrap();
        hud.update(now + Duration::from_millis(600), false);
        assert_eq!(hud.until, Some(now + Duration::from_millis(1400)));
        hud.update(now + Duration::from_millis(1400), false);
        assert!(hud.until.is_none());
        assert!(hud.refresh_ms().is_none());
        hud.update(now + Duration::from_millis(1500), true);
        assert!(hud.until.is_some());
        session
            .call("setOverview", &[json!({}), json!(true)])
            .unwrap();
        hud.update(now, false);
        assert!(hud.until.is_none());
        session
            .call("setOverview", &[json!({}), json!(false)])
            .unwrap();
        hud.update(now, false);
        assert!(hud.until.is_none());
        session
            .call("setDesktops", &[json!({}), json!([1]), json!(1)])
            .unwrap();
        hud.update(now, true);
        assert!(hud.until.is_none());
    }
}
