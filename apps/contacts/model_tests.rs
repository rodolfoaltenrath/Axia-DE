use super::*;

#[test]
fn names_initials_and_non_bmp_letters() {
    let c = json!({"n":{"given":" Élodie ","family":"张"}});
    assert_eq!(initials(&c), "É张");
    assert_eq!(
        display_name(&json!({"n":{"prefix":"Dr.","given":"Ada","family":"Lovelace"}})),
        "Dr. Ada Lovelace"
    );
    assert_eq!(
        display_name(&json!({"fn":"Published","org":"Company"})),
        "Published"
    );
    assert_eq!(initials(&json!({"fn":"𐐨"})), "𐐀");
    assert_eq!(initials(&blank()), "?");
    assert_eq!(display_name(&json!({"emails":[{"value":"a@b"}]})), "a@b");
}
#[test]
fn search_requires_every_word_across_fields() {
    let c = json!({"fn":"Élodie Zhang","org":"Axia","nick":"Z","emails":[{"value":"hello@EXAMPLE.org"}],"phones":[{"value":"555-0123"}]});
    assert!(matches(&c, "  ÉLODIE\nexample 555"));
    assert!(!matches(&c, "Élodie missing"));
    assert!(matches(&c, " \t"));
}
#[test]
fn groups_and_same_uid_in_another_book_are_distinct() {
    let all = vec![
        json!({"uid":"same","book":"a","fn":"One"}),
        json!({"uid":"same","book":"b","fn":"Two"}),
        json!({"uid":"g","book":"a","fn":"Team","kind":"group","members":["same"]}),
    ];
    assert_eq!(visible(&all, "all", "").len(), 2);
    assert_eq!(visible(&all, "group:a/g", "")[0]["fn"], "One");
    assert_eq!(visible(&all, "book:b", "")[0]["fn"], "Two");
    assert!(is_member(&all[2], &all[0]));
    assert!(!is_member(&all[2], &all[1]));
    assert!(toggle_member(&all[2], &all[1]).is_none());
    assert_eq!(
        toggle_member(&all[2], &all[0]).unwrap()["members"],
        json!([])
    );
    assert_ne!(key(&all[0]), key(&all[1]));
}
#[test]
fn group_toggle_preserves_properties_and_deduplicates_removal() {
    let c = json!({"book":"a","uid":"x"});
    let g = json!({"book":"a","uid":"g","members":["x","y","x"],"extensions":{"z":1}});
    let removed = toggle_member(&g, &c).unwrap();
    assert_eq!(removed["members"], json!(["y"]));
    let added = toggle_member(&removed, &c).unwrap();
    assert_eq!(added["members"], json!(["y", "x"]));
    assert_eq!(added["extensions"], g["extensions"]);
}
#[test]
fn deleted_group_falls_back_to_all_and_unknown_book_stays_empty() {
    let all = vec![json!({"book":"a","uid":"1","fn":"Ada"})];
    assert_eq!(visible(&all, "group:a/deleted", "").len(), 1);
    assert!(visible(&all, "book:deleted", "").is_empty());
}
#[test]
fn writes_choose_selected_then_local_then_first_then_create() {
    let books = vec![
        json!({"id":"remote","kind":"dav"}),
        json!({"id":"local","kind":"local"}),
    ];
    assert_eq!(target_book(&books, Some("remote")), Some("remote"));
    assert_eq!(target_book(&books, Some("gone")), Some("local"));
    assert_eq!(target_book(&books[..1], None), Some("remote"));
    assert_eq!(target_book(&[], None), None);
}
#[test]
fn normalizes_imported_types_without_losing_custom_labels() {
    for (kind, raw, expected) in [
        ("phones", "VOICE;MOBILE", "cell"),
        ("emails", "pref,work", "work"),
        ("urls", "internet", "other"),
        ("addrs", "PREF, vacation", "vacation"),
        ("phones", "VOICE,CELL,WORK", "cell"),
    ] {
        assert_eq!(field_type(kind, raw), expected);
    }
}
#[test]
fn editor_retains_unknown_card_and_row_properties() {
    let draft = json!({"_new":true,"uid":"u","book":"a","n":{"prefix":" Dr. ","given":" Ada ","family":" Lovelace ","custom":"retained"},"dept":"Research","cats":["Friends"],"im":[{"value":"xmpp:a"}],"bday":"--0229","notes":"  keep whitespace\n","extensions":{"custom":"v"},"phones":[{"type":"VOICE,WORK","value":" 123 ","pref":1},{"value":" "}],"addrs":[{"type":"custom","street":" Road ","box":"PO 12","ext":"Suite 3","country":" GB "}]});
    let c = build(&draft).unwrap();
    assert_eq!(c["fn"], "Dr. Ada Lovelace");
    assert!(c.get("_new").is_none());
    assert_eq!(c["phones"].as_array().unwrap().len(), 1);
    assert_eq!(c["phones"][0]["pref"], 1);
    assert_eq!(c["phones"][0]["type"], "VOICE,WORK");
    assert_eq!(c["addrs"][0]["box"], "PO 12");
    assert_eq!(c["addrs"][0]["ext"], "Suite 3");
    assert_eq!(c["addrs"][0]["street"], "Road");
    for field in ["dept", "cats", "im", "bday", "notes", "extensions"] {
        assert_eq!(c[field], draft[field]);
    }
    assert_eq!(c["n"]["custom"], "retained");
}
#[test]
fn editor_requires_identity_but_accepts_phone_only_contacts() {
    assert_eq!(build(&blank()).unwrap_err(), "contacts.needName");
    assert_eq!(
        build(&json!({"phones":[{"value":" 123 "}]})).unwrap()["fn"],
        "123"
    );
    assert_eq!(build(&json!({"org":" Company "})).unwrap()["fn"], "Company");
    assert_eq!(
        build(&json!({"emails":[{"value":" a@b "}]})).unwrap()["fn"],
        "a@b"
    );
}
#[test]
fn sort_and_section_use_family_then_given_and_nfc() {
    let all = vec![
        json!({"fn":"Zoe"}),
        json!({"fn":"E\u{301}va"}),
        json!({"fn":"Éva"}),
        json!({"fn":"123"}),
        json!({"n":{"family":"Abel","given":"Z"}}),
    ];
    let list = visible(&all, "all", "");
    assert_eq!(display_name(list[0]), "123");
    assert_eq!(display_name(list[1]), "Z Abel");
    assert_eq!(display_name(list[2]), "Zoe");
    assert_eq!(display_name(list[3]), "E\u{301}va");
    assert_eq!(letter(list[0]), "#");
    assert_eq!(letter(list[1]), "A");
}
#[test]
fn large_list_viewport_is_bounded_and_selected_row_is_revealed() {
    let all: Vec<_> = (0..10_000)
        .map(|i| json!({"fn":format!("Person {i:05}")}))
        .collect();
    let refs: Vec<_> = all.iter().collect();
    let layout = ListLayout::new(&refs);
    assert_eq!(layout.height, 300024.);
    let range = layout.viewport(150024., 600.);
    assert_eq!(range.len(), 20);
    assert_eq!(layout.rows[range.start].1, Row::Contact(5000));
    assert_eq!(layout.reveal(9999, 0., 600.), 299424.);
    assert_eq!(layout.reveal(0, 300., 600.), 24.);
    assert_eq!(layout.reveal(5, 0., 600.), 0.);
    assert_eq!(layout.viewport(0., 24.), 0..1);
    assert_eq!(layout.viewport(24., 30.), 1..2);
    assert!(layout.viewport(layout.height, 600.).is_empty());
    assert!(ListLayout::new(&[]).viewport(0., 600.).is_empty());
    assert!(layout.viewport(25., 0.).is_empty());
}
