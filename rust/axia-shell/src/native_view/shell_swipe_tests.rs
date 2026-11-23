use super::*;
fn call(session: &mut Session, command: &str, values: &[Value]) {
    session.call(command,&[vec![json!({})],values.to_vec()].concat()).unwrap();
}
fn gesture(session: &mut Session, x: f64, y: f64) {
    assert!(session.physical_swipe(1,3,0.,0.));
    assert!(session.physical_swipe(2,3,x,y));
    assert!(session.physical_swipe(3,3,0.,0.));
}
#[test]
fn recognition_preserves_threshold_dominant_axis_cancel_and_one_completion() {
    let mut swipe = swipe::Swipe::default();
    assert!(swipe.event(2,3,200.,0.).is_none()); assert!(swipe.event(3,3,0.,0.).is_none());
    swipe.event(1,3,0.,0.); swipe.event(2,3,30.,1.); swipe.event(2,3,29.9,0.);
    assert!(swipe.event(3,3,900.,0.).is_none(),"end does not add motion");
    swipe.event(1,4,0.,0.); swipe.event(2,4,-30.,20.); swipe.event(2,4,-30.,20.);
    let result = swipe.event(3,4,0.,0.).unwrap(); assert_eq!((result.direction,result.fingers),("left",4));
    assert!(swipe.event(3,4,0.,0.).is_none());
    swipe.event(1,3,0.,0.); swipe.event(2,3,-60.,-60.);
    assert_eq!(swipe.event(3,3,0.,0.).unwrap().direction,"up");
    for invalid in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY] {
        swipe.event(1,3,0.,0.); swipe.event(2,3,100.,0.); swipe.event(2,3,invalid,0.);
        assert!(swipe.event(3,3,0.,0.).is_none());
    }
    swipe.event(1,3,0.,0.); swipe.event(2,3,100.,0.); swipe.event(4,3,0.,0.);
    assert!(swipe.event(3,3,0.,0.).is_none());
    swipe.event(1,3,0.,0.); swipe.event(2,3,100.,0.); swipe.event(1,4,0.,0.);
    assert!(swipe.event(3,4,0.,0.).is_none(),"a new begin replaces old accumulation");
}
#[test]
fn horizontal_swipes_revalidate_at_completion_and_cycle_with_one_desktop() {
    let (home,mut session,mut control,root) = input_fixture("swipe-model","macos");
    control.bind_settings(&home.0);
    call(&mut session,"setDesktops",&[json!([10,20]),json!(10)]);
    let reader = session.reader(); let revision = reader.revision();
    gesture(&mut session,-60.,0.);
    assert_eq!(reader.read().active_desktop(),20); assert_eq!(reader.revision(),revision+1);
    gesture(&mut session,-100.,0.); assert_eq!(reader.revision(),revision+1);
    assert!(session.physical_swipe(1,3,0.,0.)); session.physical_swipe(2,3,100.,0.);
    call(&mut session,"setDesktops",&[json!([20,30]),json!(30)]);
    session.physical_swipe(3,3,0.,0.); assert_eq!(reader.read().active_desktop(),20);
    call(&mut session,"setDesktops",&[json!([20]),json!(20)]);
    call(&mut session,"open",&[json!({"app":"one"})]); call(&mut session,"open",&[json!({"app":"two"})]);
    let revision = reader.revision(); gesture(&mut session,-100.,0.);
    assert_eq!(reader.read().focused_window(),Some("one")); assert_eq!(reader.revision(),revision+1);
    gesture(&mut session,100.,0.); assert_eq!(reader.read().focused_window(),Some("two"));
    scene::destroy(root);
}
#[test]
fn vertical_intents_are_once_only_and_old_roots_cannot_deliver_or_resume_them() {
    let (_home,mut session,mut control,root) = input_fixture("swipe-root","macos");
    let revision = session.reader().revision(); gesture(&mut session,0.,-60.);
    session.physical_swipe(3,3,0.,0.); assert_eq!(session.reader().revision(),revision);
    let effects = session.take_service_effects().unwrap();
    let effects: Vec<_> = effects.as_array().unwrap().iter().filter(|e|e["kind"]=="shellGesture").collect();
    assert_eq!(effects.len(),1); assert_eq!(effects[0]["direction"],"up"); assert_eq!(effects[0]["fingers"],3);
    gesture(&mut session,0.,60.);
    session.physical_swipe(1,3,0.,0.); session.physical_swipe(2,3,-100.,0.);
    let next = scene::create(scene::VIEW); scene::set_test_root(next);
    control.request(&json!({"cmd":"shellRoot","node":next}));
    assert!(session.take_service_effects().is_none());
    session.physical_swipe(3,3,0.,0.); assert_eq!(session.reader().revision(),revision);
    scene::destroy(next); assert!(!session.physical_swipe(1,3,0.,0.)); scene::destroy(root);
}
#[test]
fn open_switcher_consumes_system_swipes_without_resuming_partial_motion() {
    let (_home,mut session,mut control,root) = input_fixture("swipe-priority","macos");
    call(&mut session,"setDesktops",&[json!([1,2]),json!(1)]);
    session.physical_swipe(1,3,0.,0.); session.physical_swipe(2,3,-100.,0.);
    control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    let revision = session.reader().revision(); session.physical_swipe(3,3,0.,0.);
    gesture(&mut session,0.,-100.); assert_eq!(session.reader().revision(),revision);
    control.request(&json!({"cmd":"switcherCancel"})); session.physical_swipe(3,3,0.,0.);
    assert_eq!(session.reader().read().active_desktop(),1);
    gesture(&mut session,-100.,0.); assert_eq!(session.reader().read().active_desktop(),2);
    scene::destroy(root);
}
