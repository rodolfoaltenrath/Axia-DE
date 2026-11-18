use super::*;

fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a
}
fn contact() -> Value {
    json!({"book":"a","uid":"1","fn":"Ada Lovelace","n":{"given":"Ada","family":"Lovelace"},"emails":[{"value":"ada@example.org","type":"WORK"}],"phones":[],"urls":[],"addrs":[],"bday":"--0229","notes":"First line\n第二行","photo":false})
}
fn fixture() -> App {
    let mut a = app();
    a.state.books = vec![json!({"id":"a","kind":"local","name":"Personal"})];
    a.state.contacts = vec![contact()];
    a.state.selected = "a/1".into();
    a
}
fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    if v["key"] == key {
        return Some(v);
    }
    v["children"].as_array()?.iter().find_map(|v| find(v, key))
}
fn nodes(v: &Value, kind: &str) -> usize {
    usize::from(v["kind"] == kind)
        + v["children"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| nodes(v, kind))
            .sum::<usize>()
}
#[test]
fn native_card_has_every_kind_and_email_action() {
    let mut a = fixture();
    let c = &mut a.state.contacts[0];
    c["urls"] = json!([{"value":"https://example.org","type":"work"}]);
    c["addrs"] = json!([{"type":"home","street":"Road","city":"London","code":"123","region":"State","country":"GB"}]);
    c["phones"] = json!([{"value":"123","type":"cell"}]);
    let tree = ui::tree(&mut a, 0);
    let s = tree.to_string();
    for expected in [
        "Ada",
        "Lovelace",
        "ada@example.org",
        "https://example.org",
        "Road\\n123 London\\nState\\nGB",
        "First line\\n第二行",
        "February 29",
    ] {
        assert!(s.contains(expected), "missing {expected}: {s}");
    }
    assert_eq!(
        find(&tree, "emails-0").unwrap()["children"][1]["events"]["onClick"],
        json!(["email", 0])
    );
}
#[test]
fn row_actions_round_trip_through_native_host_key_format() {
    let mut a = fixture();
    a.request_at(&json!({"cmd":"edit"}), 0);
    a.request_at(&json!({"cmd":"row","key":{"kind":"emails","index":0,"field":"value"},"value":"new@example.org"}),0);
    assert_eq!(
        a.state.draft.as_ref().unwrap().value["emails"][0]["value"],
        "new@example.org"
    );
    a.request_at(
        &json!({"cmd":"typeMenu","key":{"kind":"emails","index":0}}),
        0,
    );
    assert!(!a.picker.is_null());
    a.request_at(
        &json!({"cmd":"row","key":{"kind":"emails","index":0,"field":"type","value":"home"}}),
        0,
    );
    assert_eq!(
        a.state.draft.as_ref().unwrap().value["emails"][0]["type"],
        "home"
    );
    assert!(a.picker.is_null());
    a.request_at(
        &json!({"cmd":"removeRow","key":{"kind":"emails","index":0}}),
        0,
    );
    assert_eq!(a.state.draft.as_ref().unwrap().value["emails"], json!([]));
}
#[test]
fn native_notes_match_unicode_multiline_input_and_focus_lifetime() {
    let mut a = fixture();
    a.request_at(&json!({"cmd":"edit"}), 0);
    a.request_at(&json!({"cmd":"notesFocus"}), 0);
    assert!(a.text_input());
    a.key_at("x", "🙂", 0, 1);
    a.key_at("Backspace", "", 0, 2);
    a.key_at("Enter", "", 0, 3);
    a.paste_at("pasted", 4);
    assert_eq!(
        a.state.draft.as_ref().unwrap().value["notes"],
        "First line\n第二行\npasted"
    );
    a.request_at(&json!({"cmd":"cancel"}), 5);
    assert!(!a.text_input());
    assert!(a.paste_at("late", 6).is_none());
}
#[test]
fn keyboard_selection_and_shortcuts_respect_modal_and_editor() {
    let mut a = fixture();
    let mut c = contact();
    c["uid"] = json!("2");
    c["n"]["family"] = json!("Zed");
    a.state.contacts.push(c);
    a.key_at("ArrowDown", "", 0, 0);
    assert_eq!(a.state.selected, "a/2");
    a.key_at("ArrowUp", "", 0, 0);
    assert_eq!(a.state.selected, "a/1");
    a.key_at("n", "n", 2, 0);
    assert_eq!(a.state.draft.as_ref().unwrap().value["_new"], true);
    assert!(a.key_at("ArrowDown", "", 0, 0).is_none());
    a.request_at(&json!({"cmd":"groupNew"}), 0);
    a.key_at("Escape", "", 0, 0);
    assert!(a.state.sheet.is_null());
    assert!(a.state.draft.is_some());
}
#[test]
fn list_renders_bounded_rows_reveals_selection_and_captures_scrollbar() {
    let mut a = app();
    a.height = 492.;
    a.state.contacts = (0..10000)
        .map(|i| json!({"book":"a","uid":i.to_string(),"fn":format!("Person {i:05}")}))
        .collect();
    a.state.selected = "a/9999".into();
    a.request_at(&json!({"cmd":"noop"}), 0);
    assert!(a.offset > 299000.);
    let tree = ui::tree(&mut a, 0);
    let list = find(&tree, "contact-list").unwrap();
    assert!(nodes(list, "view") < 30);
    assert!(find(list, "contact-a/9999").is_some());
    a.request_at(&json!({"cmd":"wheel","deltaY":-1000000.}), 0);
    assert_eq!(a.offset, 0.);
    assert_eq!(
        a.request_at(&json!({"cmd":"scrollDrag","y":0}), 0)["capture"],
        true
    );
    a.request_at(&json!({"cmd":"pointerMove","y":1000}), 0);
    assert!(a.offset > 299000.);
    assert_eq!(a.request_at(&json!({"cmd":"dragEnd"}), 0)["capture"], false);
    a.rtl = true;
    let tree = ui::tree(&mut a, 0);
    assert_eq!(find(&tree, "list-thumb").unwrap()["style"]["left"], 2);
}
#[test]
fn birthday_picker_keeps_yearless_value_until_a_date_is_chosen() {
    let mut a = fixture();
    a.request_at(&json!({"cmd":"edit"}), 0);
    a.request_at(&json!({"cmd":"birthdayPicker"}), 0);
    let tree = ui::tree(&mut a, 0);
    assert!(find(&tree, "birthday-calendar").is_some());
    assert_eq!(a.state.draft.as_ref().unwrap().value["bday"], "--0229");
    a.request_at(&json!({"cmd":"month","key":"1"}), 0);
    assert_eq!(date::ymd(a.picker_month), "2000-03-01");
    a.request_at(
        &json!({"cmd":"pickDay","key":date::parse("2000-03-17").unwrap()}),
        0,
    );
    assert!(a.picker.is_null());
    assert_eq!(a.state.draft.as_ref().unwrap().value["bday"], "2000-03-17");
    a.request_at(&json!({"cmd":"birthdayRemove"}), 0);
    assert_eq!(a.state.draft.as_ref().unwrap().value["bday"], "");
}
#[test]
fn membership_menu_excludes_groups_from_other_books() {
    let mut a = fixture();
    a.state.contacts.extend([
        json!({"book":"a","uid":"g","kind":"group","fn":"Team","members":["1"]}),
        json!({"book":"b","uid":"g","kind":"group","fn":"Other book","members":["1"]}),
    ]);
    a.request_at(&json!({"cmd":"menu","key":"membership"}), 0);
    let tree = ui::tree(&mut a, 0);
    let menu = find(&tree, "menu").unwrap().to_string();
    assert!(menu.contains("Team"));
    assert!(!menu.contains("Other book"));
}
