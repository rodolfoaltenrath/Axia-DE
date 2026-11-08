use super::*;
fn press(app: &mut App, key: &str) {
    app.request(&json!({"cmd":"press","key":key}));
}
#[test]
fn arithmetic_preserves_precedence_parentheses_unary_decimal_and_rounding() {
    for (input, expected) in [
        ("2+3*4", 14.),
        ("(2+3)×4", 20.),
        ("-3*-2", 6.),
        ("+3", 3.),
        ("1,5÷.5", 3.),
        ("0.1+0.2", 0.3),
        ("1.", 1.),
        ("1e-3+1", 1.001),
    ] {
        assert_eq!(evaluate(input), Some(expected), "{input}");
    }
}
#[test]
fn invalid_input_is_not_silently_stripped_or_partially_evaluated() {
    for input in [
        "", "1x+2", "(1+2", "1+2)", "1..2", "1 2", "2(3)", "1/0", "0/0", "1e309", "1e+",
        "alert(1)", "1%2",
    ] {
        assert_eq!(evaluate(input), None, "{input}");
    }
    assert_eq!(evaluate(&"(".repeat(1000)), None);
    assert_eq!(evaluate(&"1".repeat(4097)), None);
}
#[test]
fn keyboard_clear_backspace_and_errors_remain_rust_owned() {
    let mut app = App::default();
    for ch in ["1", ",", "5", "*", "2"] {
        app.request(&json!({"cmd":"key","event":ch,"char":ch}));
    }
    app.request(&json!({"cmd":"key","event":"Enter"}));
    assert_eq!(app.result, "3");
    app.request(&json!({"cmd":"key","event":"Backspace"}));
    assert!(app.expression.is_empty());
    press(&mut app, "=");
    assert!(app.error);
    app.request(&json!({"cmd":"key","event":"Escape"}));
    assert!(!app.error);
    assert_eq!(app.result, "0");
    app.request(&json!({"cmd":"key","event":"a","char":"a"}));
    assert!(app.expression.is_empty());
    app.request(&json!({"cmd":"key","event":"Unidentified","char":"7"}));
    assert_eq!(app.expression, "7");
}
#[test]
fn input_is_bounded_and_instances_are_independent() {
    let mut a = App::default();
    let b = App::default();
    for _ in 0..100 {
        press(&mut a, "9");
    }
    assert_eq!(a.expression.len(), 40);
    assert!(b.expression.is_empty());
    press(&mut a, "C");
    press(&mut a, "1");
    press(&mut a, "÷");
    press(&mut a, "0");
    press(&mut a, "=");
    assert!(a.error);
    assert!(!b.error);
}
#[test]
fn huge_results_are_finite_reusable_and_do_not_overflow_the_rounding_step() {
    assert_eq!(evaluate("1e200*1e100"), Some(1e300));
    assert_eq!(number(1e30), "1e30");
    assert_eq!(evaluate(&(number(1e30) + "+1")), Some(1e30));
    assert_eq!(number(-0.), "0");
    assert_eq!(evaluate("1e-10"), Some(1e-10));
}
#[test]
fn pointer_state_clears_and_display_tracks_region_and_theme() {
    let mut app = App::default();
    press(&mut app, "1");
    press(&mut app, ".");
    press(&mut app, "5");
    press(&mut app, "=");
    app.request(&json!({"cmd":"hover","key":"7"}));
    app.request(&json!({"cmd":"down","key":"7"}));
    assert_eq!(app.down.as_deref(), Some("7"));
    app.request(&json!({"cmd":"leave","key":"7"}));
    assert_eq!(app.down, None);
    assert_eq!(app.hover, None);
    let tree=app.request(&json!({"cmd":"sync","context":{"decimal":",","theme":{"mode":"dark","nativeStyle":"adwaita","windowBg":"#202020"}}}));
    assert!(tree["tree"].to_string().contains("1,5"));
    assert_eq!(tree["tree"]["style"]["direction"], "ltr");
    assert_eq!(tree["tree"]["style"]["backgroundColor"], "#202020");
}
#[test]
fn an_error_after_a_long_result_is_shown_at_full_size() {
    let mut app = App::default();
    for key in ["1", "2", "3", "4", "5", "6", "7", "8", "9", "×", "9", "9", "9", "="] {
        press(&mut app, key);
    }
    let size = |app: &App| ui::tree(app, &json!({}))["children"][0]["children"][1]["style"]["fontSize"].clone();
    assert_eq!(size(&app), json!(26), "the long result shrinks");
    for key in ["÷", "0", "="] {
        press(&mut app, key);
    }
    assert!(app.error);
    assert_eq!(size(&app), json!(44));
}
