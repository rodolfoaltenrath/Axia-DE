use super::{titlebar::TitleBar, Host};
use crate::runtime::native as scene;
use serde_json::{json, Value};

#[test]
fn double_click_on_a_control_never_bubbles_to_header_maximize() {
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(TitleBar::default()), "", None);
    host.facts(json!({"args":{"id":"probe","title":"Probe","focused":true,"height":40,
        "tokens":{"titleBg":"#222","text":"#fff","textDim":"#888","lightOff":"#555"},
        "controls":[{"key":"min","glyph":"−"}],
        "chrome":{"header":{"height":40},"controls":{},"button":{"width":28,"height":28},"glyph":{},"title":{}}}}));
    let glyph = host.find_text("−");
    for _ in 0..2 {
        host.pointer("down", glyph, 10., 10., 0);
        host.pointer("up", glyph, 10., 10., 0);
    }
    let effects = host.take().unwrap();
    let effects = effects.as_array().unwrap();
    assert_eq!(
        effects
            .iter()
            .filter(|e| e["kind"] == "winAct" && e["key"] == "min")
            .count(),
        2
    );
    assert!(
        !effects
            .iter()
            .any(|e| e["kind"] == "winMax" || e["kind"] == "winDrag"),
        "{effects:?}"
    );
    drop(host);
    scene::destroy(mount);
}

#[test]
fn double_click_on_the_header_emits_one_maximize_and_keeps_drag_coordinates() {
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(TitleBar::default()), "", None);
    host.facts(json!({"args":{"id":"probe","title":"Probe","focused":true,"height":40,"controls":[],"tokens":{}}}));
    let title = host.find_text("Probe");
    for _ in 0..2 {
        host.pointer("down", title, 80., 20., 0);
        host.pointer("up", title, 80., 20., 0);
    }
    let effects: Value = host.take().unwrap();
    let effects = effects.as_array().unwrap();
    assert_eq!(effects.iter().filter(|e| e["kind"] == "winMax").count(), 1);
    assert_eq!(
        effects
            .iter()
            .filter(|e| e["kind"] == "winDrag" && e["x"] == 80. && e["y"] == 20.)
            .count(),
        2
    );
    drop(host);
    scene::destroy(mount);
}
