use super::*;
use crate::winmodel::Session;
use serde_json::{json, Value};
fn call(s: &mut Session, op: &str, values: &[Value]) {
    let mut args = vec![json!({})];
    args.extend_from_slice(values);
    s.call(op, &args).unwrap();
}
fn fixture() -> (Session, Animation) {
    let mut session = Session::default();
    call(
        &mut session,
        "setDesktops",
        &[json!([10, 20, 30]), json!(10)],
    );
    let mut animation = Animation::new(session.reader());
    animation.initialize();
    animation.configure(false, false);
    (session, animation)
}
fn window(session: &mut Session, name: &str, desktop: i64) -> i32 {
    call(
        session,
        "open",
        &[json!({"app":"notes","instance":name,"desktop":desktop})],
    );
    scene::create(scene::VIEW)
}
fn frames(now: f64) -> Vec<anim::AnimFrame> {
    let mut out = [anim::AnimFrame::default(); 128];
    let n = unsafe { anim::axia_anim_step(now, out.as_mut_ptr(), out.len()) };
    out[..n].to_vec()
}
fn switch(session: &mut Session, animation: &mut Animation, desktop: i64, now: f64) {
    call(session, "setActiveDesktop", &[json!(desktop)]);
    animation.sync(now);
}

#[test]
fn selection_is_native_and_completion_restores_the_base_without_a_js_notice() {
    let (mut session, mut animation) = fixture();
    let target = window(&mut session, "target", 20);
    let other = window(&mut session, "other", 10);
    let hidden = window(&mut session, "hidden", 20);
    call(&mut session, "minimize", &[json!("notes#hidden")]);
    let base = anim::AnimProps {
        opacity: 0.7,
        translate_x: 9.,
        scale_x: 1.2,
        ..Default::default()
    };
    scene::apply_animation(target, base);
    for (name, node) in [("target", target), ("other", other), ("hidden", hidden)] {
        assert!(animation.bind(&format!("notes#{name}"), node, 0.));
    }
    switch(&mut session, &mut animation, 20, 100.);
    assert_eq!(scene::animation_props(target).unwrap().translate_x, 65.);
    let frame = frames(185.);
    assert_eq!(frame.len(), 1);
    assert_eq!(frame[0].node, target);
    assert_eq!(frame[0].done, 0);
    let frame = frames(270.);
    assert_eq!(frame[0].done, 2);
    assert_eq!(frame[0].props, base);
    assert!(frames(280.).is_empty());
    assert_eq!(anim::axia_anim_active(), 0);
    assert!(
        anim::clear_desktop(target, frame[0].id).is_none(),
        "completion already released its base"
    );
    drop(animation);
    for n in [target, other, hidden] {
        scene::destroy(n);
    }
}

#[test]
fn late_frames_use_the_current_transition_and_expired_transitions_do_not_replay() {
    let (mut session, mut animation) = fixture();
    let first = window(&mut session, "late", 20);
    switch(&mut session, &mut animation, 20, 100.);
    assert!(animation.bind("notes#late", first, 185.));
    let p = scene::animation_props(first).unwrap();
    assert!(p.translate_x > 0. && p.translate_x < 56.);
    assert!(p.opacity > 0. && p.opacity < 1.);
    let late = window(&mut session, "expired", 20);
    assert!(animation.bind("notes#expired", late, 300.));
    assert_eq!(scene::animation_props(late).unwrap().translate_x, 0.);
    assert_eq!(animation.running.len(), 1);
    drop(animation);
    scene::destroy(first);
    scene::destroy(late);
}

#[test]
fn reversal_rtl_reduce_motion_and_overview_cancel_only_owned_animations() {
    let (mut session, mut animation) = fixture();
    let a = window(&mut session, "a", 10);
    let b = window(&mut session, "b", 20);
    animation.bind("notes#a", a, 0.);
    animation.bind("notes#b", b, 0.);
    switch(&mut session, &mut animation, 20, 100.);
    switch(&mut session, &mut animation, 10, 120.);
    assert_eq!(scene::animation_props(b).unwrap().translate_x, 0.);
    assert_eq!(scene::animation_props(a).unwrap().translate_x, -56.);
    animation.configure(true, false);
    animation.sync(125.);
    assert_eq!(scene::animation_props(a).unwrap().translate_x, 0.);
    animation.configure(false, true);
    switch(&mut session, &mut animation, 20, 150.);
    assert_eq!(scene::animation_props(b).unwrap().translate_x, -56.);
    call(&mut session, "setOverview", &[json!(true)]);
    animation.sync(155.);
    assert_eq!(scene::animation_props(b).unwrap().translate_x, 0.);
    call(&mut session, "setOverview", &[json!(false)]);
    animation.sync(160.);
    assert!(animation.running.is_empty());
    drop(animation);
    scene::destroy(a);
    scene::destroy(b);
}

#[test]
fn recycled_node_ids_and_unbinding_cannot_redirect_an_old_window_effect() {
    let (mut session, mut animation) = fixture();
    let node = window(&mut session, "old", 20);
    assert!(animation.bind("notes#old", node, 0.));
    let generation = scene::generation(node).unwrap();
    scene::destroy(node);
    let replacement = scene::create(scene::VIEW);
    assert_eq!(replacement, node);
    assert_ne!(scene::generation(replacement), Some(generation));
    switch(&mut session, &mut animation, 20, 100.);
    assert!(animation.running.is_empty());
    assert_eq!(scene::animation_props(replacement).unwrap().translate_x, 0.);
    assert!(!animation.bind("missing", replacement, 100.));
    animation.bind("notes#old", replacement, 110.);
    assert_eq!(animation.running.len(), 1);
    animation.bind("notes#old", 0, 120.);
    assert!(animation.running.is_empty());
    assert_eq!(scene::animation_props(replacement).unwrap().translate_x, 0.);
    drop(animation);
    scene::destroy(replacement);
}

#[test]
fn a_newer_legacy_animation_is_not_cleared_by_a_desktop_cancellation() {
    let (mut session, mut animation) = fixture();
    let node = window(&mut session, "target", 20);
    animation.bind("notes#target", node, 0.);
    switch(&mut session, &mut animation, 20, 100.);
    let p = anim::AnimProps::default();
    let id = unsafe { anim::axia_anim_start(node, 110., 400., 0, &p, &p, &p) };
    animation.configure(true, false);
    animation.sync(120.);
    let frame = frames(130.);
    assert_eq!(frame.len(), 1);
    assert_eq!(frame[0].id, id);
    unsafe {
        let mut base = p;
        anim::axia_anim_clear(node, &mut base);
    }
    drop(animation);
    scene::destroy(node);
}
