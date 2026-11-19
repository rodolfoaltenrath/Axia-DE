use super::{titlebar::TitleBar, Host};
use crate::runtime::native as scene;
use serde_json::{json, Value};

#[test]
fn model_revisions_update_headers_without_facts_and_are_scoped_to_their_owner() {
    let mut a = crate::winmodel::Session::default();
    let mut b = crate::winmodel::Session::default();
    for (session, title) in [(&mut a, "Owner A"), (&mut b, "Owner B")] {
        session
            .call("open", &[json!({}), json!({"app":"probe"})])
            .unwrap();
        session
            .call(
                "patch",
                &[json!({}), json!("probe"), json!({"title":title})],
            )
            .unwrap();
    }
    let node_a = scene::create(scene::VIEW);
    let node_b = scene::create(scene::VIEW);
    let mut host_a = Host::new(
        node_a,
        Box::new(TitleBar::with_windows(a.reader())),
        "",
        None,
    );
    let mut host_b = Host::new(
        node_b,
        Box::new(TitleBar::with_windows(b.reader())),
        "",
        None,
    );
    let facts = json!({"args":{"id":"probe","fallbackTitle":"Probe","height":40,"controls":[],"tokens":{}}});
    host_a.facts(facts.clone());
    host_b.facts(facts);
    assert_ne!(host_a.find_text("Owner A"), 0);
    assert_ne!(host_b.find_text("Owner B"), 0);
    assert!(host_a.deadline().is_none());
    a.call(
        "patch",
        &[json!({}), json!("probe"), json!({"title":"Updated A"})],
    )
    .unwrap();
    assert!(
        host_a.deadline().is_some(),
        "the loop receives an immediate invalidation"
    );
    assert!(
        host_b.deadline().is_none(),
        "another context's model never invalidates this header"
    );
    host_a.tick(0.);
    assert_ne!(host_a.find_text("Updated A"), 0);
    assert_eq!(host_a.find_text("Owner A"), 0);
    assert!(
        host_a.deadline().is_none(),
        "no idle timer remains after consuming the revision"
    );
    drop(a);
    host_a.tick(0.);
    assert_ne!(host_a.find_text("Updated A"), 0);
    drop(host_a);
    drop(host_b);
    scene::destroy(node_a);
    scene::destroy(node_b);
}

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
