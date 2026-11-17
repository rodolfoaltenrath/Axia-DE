use super::*;
fn complete(s: &mut State, tag: Tag, result: Result<Value, String>) {
    s.apply(Completed { tag, result }, 1000);
}
fn card(book: &str, uid: &str, name: &str) -> Value {
    json!({"book":book,"uid":uid,"fn":name,"n":{"given":name},"kind":"individual"})
}
fn next_job(s: &mut State) -> (&'static str, Value) {
    let effect = s.jobs.pump(0).expect("queued job");
    let method = effect["args"][1].as_str().unwrap();
    let method = match method {
        "pim.request" => "pim.request",
        "pim.fileRelease" => "pim.fileRelease",
        _ => panic!("unexpected {effect}"),
    };
    let args = effect["args"][2].clone();
    s.jobs.complete(&json!({"error":"test-dispatched"}), 0);
    (method, args)
}
fn ready() -> State {
    let mut s = State::default();
    s.jobs.bind(1);
    s.books = vec![
        json!({"id":"a","kind":"local"}),
        json!({"id":"b","kind":"dav"}),
    ];
    s.contacts = vec![card("a", "1", "Ada"), card("b", "1", "Grace")];
    s
}
#[test]
fn late_contact_photo_is_released_and_current_photo_reloads_on_revision() {
    let mut s = ready();
    s.booted = true;
    s.contacts[0]["photo"] = json!(true);
    s.selected = "a/1".into();
    s.update_photo();
    let epoch = s.epoch;
    complete(
        &mut s,
        Tag::Image(epoch, "a/1".into()),
        Ok(json!({"image":101})),
    );
    assert_eq!(s.photo_handle, 101);
    s.selected = "b/1".into();
    s.update_photo();
    assert_eq!(s.photo_handle, 0);
    complete(
        &mut s,
        Tag::Image(epoch, "a/1".into()),
        Ok(json!({"image":102})),
    );
    assert_eq!(s.photo_handle, 0);
    let next = s.jobs.pump(0).unwrap();
    assert_eq!(next["args"][1], "pim.imageRelease");
    assert_eq!(next["args"][2], json!([101]));
}
#[test]
fn clipboard_reply_is_scoped_to_the_editor_and_focus() {
    let mut s = ready();
    s.edit(card("a", "1", "Ada"));
    let seq = s.draft.as_ref().unwrap().sequence;
    s.notes_focus = true;
    s.edit(card("b", "1", "Grace"));
    s.notes_focus = true;
    complete(&mut s, Tag::Paste(seq), Ok(json!({"text":"old secret"})));
    assert!(s.draft.as_ref().unwrap().value["notes"].is_null());
    let seq = s.draft.as_ref().unwrap().sequence;
    s.notes_focus = false;
    complete(&mut s, Tag::Paste(seq), Ok(json!({"text":"late"})));
    assert!(s.draft.as_ref().unwrap().value["notes"].is_null());
}
#[test]
fn old_reload_cannot_overwrite_newer_data_or_error_state() {
    let mut s = ready();
    s.reload();
    let old = s.epoch;
    s.reload();
    let current = s.epoch;
    complete(
        &mut s,
        Tag::Contacts(current),
        Ok(json!({"contacts":[card("b","new","New")]})),
    );
    complete(
        &mut s,
        Tag::Contacts(old),
        Ok(json!({"contacts":[card("a","old","Old")]})),
    );
    complete(&mut s, Tag::Books(old), Err("offline".into()));
    assert_eq!(s.contacts[0]["uid"], "new");
    assert!(!s.unavailable);
}
#[test]
fn failed_refresh_retains_usable_cached_data_but_initial_failure_is_unavailable() {
    let mut s = ready();
    s.epoch = 1;
    complete(&mut s, Tag::Books(1), Err("unavailable".into()));
    assert!(s.unavailable);
    let books = s.books.clone();
    complete(&mut s, Tag::Books(1), Ok(json!({"books":books})));
    assert!(!s.unavailable);
    complete(&mut s, Tag::Books(1), Err("offline".into()));
    assert!(!s.unavailable);
    complete(&mut s, Tag::Contacts(1), Err("offline".into()));
    assert!(s.unavailable);
    let contacts = s.contacts.clone();
    complete(&mut s, Tag::Contacts(1), Ok(json!({"contacts":contacts})));
    assert!(!s.unavailable);
}
#[test]
fn late_photo_pick_is_released_instead_of_reaching_another_editor() {
    let mut s = ready();
    s.edit(card("a", "1", "Ada"));
    let seq = s.draft.as_ref().unwrap().sequence;
    s.edit(card("b", "1", "Grace"));
    complete(&mut s, Tag::PickPhoto(seq), Ok(json!({"file":99})));
    assert_eq!(s.draft.as_ref().unwrap().photo, "keep");
    assert_eq!(next_job(&mut s), ("pim.fileRelease", json!([99])));
}
#[test]
fn photo_replacement_and_cancel_release_each_owned_ticket() {
    let mut s = ready();
    s.edit(card("a", "1", "Ada"));
    let seq = s.draft.as_ref().unwrap().sequence;
    complete(&mut s, Tag::PickPhoto(seq), Ok(json!({"file":10})));
    complete(&mut s, Tag::PickPhoto(seq), Ok(json!({"file":11})));
    s.cancel_draft();
    assert_eq!(next_job(&mut s), ("pim.fileRelease", json!([10])));
    assert_eq!(next_job(&mut s), ("pim.fileRelease", json!([11])));
}
#[test]
fn failed_save_retains_editor_and_photo_for_retry() {
    let mut s = ready();
    s.edit(card("a", "1", "Ada"));
    let d = s.draft.as_mut().unwrap();
    d.photo = json!(12);
    d.busy = true;
    let w = Write {
        sequence: d.sequence,
        contact: d.value.clone(),
        photo: d.photo.clone(),
        book: "a".into(),
        group: None,
    };
    complete(&mut s, Tag::Save(w), Err("offline".into()));
    let d = s.draft.as_ref().unwrap();
    assert!(!d.busy);
    assert_eq!(d.photo, 12);
    assert_eq!(d.error, "contacts.saveFailed");
    s.save();
    assert!(s.draft.as_ref().unwrap().busy);
    let (_, args) = next_job(&mut s);
    assert_eq!(args[0], "contact-save");
    assert_eq!(args[2], json!([12]));
    assert_eq!(args[1]["photo"], "new");
}
#[test]
fn empty_book_save_creates_book_before_card_and_uses_creation_result() {
    let mut s = State::default();
    s.jobs.bind(1);
    s.default_book = "Personal".into();
    let mut c = blank();
    c["_new"] = json!(true);
    c["n"]["given"] = json!("Ada");
    s.edit(c);
    s.save();
    let (_, args) = next_job(&mut s);
    assert_eq!(args[0], "book-create");
    assert_eq!(args[1]["name"], "Personal");
    let d = s.draft.as_ref().unwrap();
    let w = Write {
        sequence: d.sequence,
        contact: build(&d.value).unwrap(),
        photo: d.photo.clone(),
        book: String::new(),
        group: None,
    };
    complete(
        &mut s,
        Tag::CreateBook(Pending::Write(w)),
        Ok(json!({"uid":"new-book"})),
    );
    let (_, args) = next_job(&mut s);
    assert_eq!(args[0], "contact-save");
    assert_eq!(args[1]["book"], "new-book");
    assert!(args[1]["contact"].get("_new").is_none());
}
#[test]
fn late_save_does_not_close_new_editor_or_change_selection() {
    let mut s = ready();
    s.edit(card("a", "1", "Ada"));
    let d = s.draft.as_ref().unwrap();
    let w = Write {
        sequence: d.sequence,
        contact: d.value.clone(),
        photo: json!("keep"),
        book: "a".into(),
        group: None,
    };
    s.edit(card("b", "1", "Grace"));
    s.selected = "b/1".into();
    complete(&mut s, Tag::Save(w), Ok(json!({"uid":"1"})));
    assert_eq!(s.draft.as_ref().unwrap().value["fn"], "Grace");
    assert_eq!(s.selected, "b/1");
}
#[test]
fn successful_contact_waits_for_reload_but_never_sticks_on_deleted_contact() {
    let mut s = ready();
    s.edit(card("a", "2", "New"));
    let d = s.draft.as_ref().unwrap();
    let w = Write {
        sequence: d.sequence,
        contact: d.value.clone(),
        photo: json!("keep"),
        book: "a".into(),
        group: None,
    };
    complete(&mut s, Tag::Save(w), Ok(json!({"uid":"2"})));
    assert_eq!(s.selected, "a/2");
    s.reconcile();
    assert_eq!(s.selected, "a/2");
    let epoch = s.epoch;
    complete(
        &mut s,
        Tag::Contacts(epoch),
        Ok(json!({"contacts":[card("a","1","Ada")]})),
    );
    assert_eq!(s.selected, "a/1");
    assert!(!s.waiting_selection);
}
#[test]
fn failed_group_join_retry_updates_saved_uid_and_retries_group() {
    let mut s = ready();
    let group = json!({"book":"a","uid":"g","kind":"group","fn":"Team","members":[]});
    s.contacts.push(group.clone());
    s.source = "group:a/g".into();
    let mut c = blank();
    c["_new"] = json!(true);
    c["n"]["given"] = json!("New");
    s.edit(c);
    let d = s.draft.as_ref().unwrap();
    let w = Write {
        sequence: d.sequence,
        contact: build(&d.value).unwrap(),
        photo: json!("keep"),
        book: "a".into(),
        group: Some(group),
    };
    complete(&mut s, Tag::Save(w.clone()), Ok(json!({"uid":"new"})));
    let (_, args) = next_job(&mut s);
    assert_eq!(args[1]["contact"]["members"], json!(["new"]));
    complete(&mut s, Tag::Join(w, "new".into()), Err("offline".into()));
    s.save();
    let (_, args) = next_job(&mut s);
    assert_eq!(args[1]["contact"]["uid"], "new");
    assert_eq!(args[1]["book"], "a");
    assert!(s.draft.as_ref().unwrap().join_group.is_some());
}
#[test]
fn import_error_closes_file_and_does_not_report_success() {
    let mut s = ready();
    complete(&mut s, Tag::Import(json!(45)), Err("invalid-vcard".into()));
    assert_eq!(s.sheet["notice"], "contacts.importFailed");
    assert_eq!(next_job(&mut s), ("pim.fileRelease", json!([45])));
}
#[test]
fn unrelated_events_do_not_cause_reload_and_poll_failures_are_throttled() {
    let mut s = ready();
    s.booted = true;
    s.events_pending = true;
    complete(&mut s, Tag::Events, Ok(json!([{"ev":"new","acct":"mail"}])));
    assert!(s.reload_at.is_none());
    assert_eq!(s.events_at, 1200);
    complete(&mut s, Tag::Events, Err("offline".into()));
    assert!(!s.events_pending);
    assert_eq!(s.events_at, 1200);
    complete(&mut s, Tag::Events, Ok(json!([{"ev":"pim-changed"}])));
    assert_eq!(s.reload_at, Some(1050));
}
#[test]
fn group_completion_cannot_close_a_newer_sheet() {
    let mut s = ready();
    s.set_sheet(json!({"group":null,"name":"Team"}));
    let seq = s.sheet_sequence;
    let group = GroupWrite {
        value: json!({"book":"a","fn":"Team","kind":"group"}),
        sheet_sequence: seq,
        new: true,
    };
    s.set_sheet(json!({"group":null,"name":"Another"}));
    complete(&mut s, Tag::Group(group), Ok(json!({"uid":"g"})));
    assert_eq!(s.sheet["name"], "Another");
    assert_eq!(s.source, "all");
}
#[test]
fn late_delete_does_not_discard_another_contact_editor() {
    let mut s = ready();
    s.set_sheet(json!({"confirm":s.contacts[0]}));
    let seq = s.sheet_sequence;
    s.edit(card("b", "1", "Grace"));
    s.set_sheet(json!({"notice":"Other"}));
    complete(&mut s, Tag::Delete("a/1".into(), seq), Ok(json!({})));
    assert_eq!(s.draft.as_ref().unwrap().value["fn"], "Grace");
    assert_eq!(s.sheet["notice"], "Other");
}
#[test]
fn editor_commands_reject_invalid_rows_and_keep_custom_vcard_fields() {
    let mut s = State::default();
    let mut c = card("a", "1", "Ada");
    c["phones"] = json!([{"value":"123","type":"VOICE,WORK","extra":true}]);
    s.edit(c);
    s.request(
        &json!({"cmd":"row","key":"phones","index":0,"field":"value","value":"456"}),
        0,
    );
    s.request(
        &json!({"cmd":"row","key":"phones","index":100,"field":"value","value":"wrong"}),
        0,
    );
    s.request(
        &json!({"cmd":"row","key":"phones","index":0,"field":"extra","value":false}),
        0,
    );
    s.request(&json!({"cmd":"field","key":"uid","value":"wrong"}), 0);
    let c = &s.draft.as_ref().unwrap().value;
    assert_eq!(c["phones"][0]["value"], "456");
    assert_eq!(c["phones"][0]["extra"], true);
    assert_eq!(c["uid"], "1");
}
#[test]
fn protocol_mount_restores_window_state_and_polls_without_reload_loop() {
    let mut s = State::default();
    let mut token = 0u64;
    let mut pending = vec![];
    let mut calls = vec![];
    let args = json!({"__nativeOwner":7,"__nativeWindow":"contacts#2","__nativeState":{"src":"book:b","cur":"b/1"}});
    for tick in 0..60 {
        let now = tick * 100;
        let mut output = s.request(&json!({"cmd":"sync","args":args}), now);
        for round in 0..32 {
            let effect = &output["effect"];
            if effect.is_null() {
                break;
            }
            assert!(round < 31, "service scheduling must yield");
            let result = if effect["kind"] == "nativeCall" {
                assert_eq!(effect["args"][0], 7);
                let method = string(&effect["args"][1]);
                let a = &effect["args"][2];
                calls.push((method.to_owned(), a.clone()));
                let value = match method {
                    "pim.watch" => Value::Null,
                    "pim.events" => json!([]),
                    "windows.state" => Value::Null,
                    "pim.request" => match string(&a[0]) {
                        "book-list" => {
                            json!({"books":[{"id":"a","kind":"local"},{"id":"b","kind":"dav"}]})
                        }
                        "contact-list" => {
                            json!({"contacts":[card("a","1","Ada"),card("b","1","Grace")]})
                        }
                        op => panic!("unexpected PIM request {op}"),
                    },
                    method => panic!("unexpected service {method}"),
                };
                token += 1;
                pending.push(json!({"token":token,"value":value}));
                json!(token)
            } else {
                json!({"value":std::mem::take(&mut pending)})
            };
            output = s.request(&json!({"cmd":"next","result":result}), now);
        }
    }
    assert_eq!(s.source, "book:b");
    assert_eq!(s.selected, "b/1");
    assert_eq!(s.selected_contact().unwrap()["fn"], "Grace");
    assert_eq!(calls.iter().filter(|(m, _)| m == "pim.watch").count(), 1);
    assert_eq!(calls.iter().filter(|(m, _)| m == "pim.request").count(), 2);
    let polls = calls.iter().filter(|(m, _)| m == "pim.events").count();
    assert!((2..=30).contains(&polls));
    assert!(!s.unavailable);
}
