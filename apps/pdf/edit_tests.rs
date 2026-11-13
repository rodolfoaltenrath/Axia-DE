use super::*;
fn edit(m: &mut Model, op: &str, fields: Value) -> Value {
    let mut input = fields;
    input["cmd"] = json!("edit");
    input["op"] = json!(op);
    let out = m.request(&input);
    assert!(out.get("error").is_none(), "{input}: {out}");
    out
}
fn open() -> Model {
    let mut m = Model::default();
    edit(&mut m, "open", json!({"pages":4}));
    m
}
fn annotation(page: u32) -> Value {
    json!({"type":"highlight","page":page,"rects":[[1,2,3,4]],"color":"#ffd60a"})
}
fn done(m: &mut Model, ticket: Value, ok: bool) -> Value {
    edit(
        m,
        "complete",
        json!({"ticket":ticket,"ok":ok,"message":"helper failed"}),
    )
}
fn field(kind: &str, value: Value) -> Value {
    json!({"r":[1,2,3,4],"f":{"id":12,"type":kind,"value":value,"ro":false,"items":["a","b"]}})
}

#[test]
fn mutation_queue_serializes_annotation_undo_and_save_in_request_order() {
    let mut m = open();
    let a = edit(&mut m, "annotate", json!({"annotation":annotation(1)}));
    let b = edit(&mut m, "annotate", json!({"annotation":annotation(2)}));
    assert!(b["effect"].is_null());
    assert_eq!(b["state"]["queued"], 1);
    edit(&mut m, "undo", json!({}));
    edit(
        &mut m,
        "save",
        json!({"path":"/home/u/copy.pdf","name":"copy.pdf"}),
    );
    let r = done(&mut m, a["effect"]["ticket"].clone(), true);
    assert_eq!(r["effect"]["kind"], "annot");
    assert_eq!(r["state"]["history"], json!([1]));
    let r = done(&mut m, r["effect"]["ticket"].clone(), true);
    assert_eq!(r["effect"]["kind"], "undo");
    assert_eq!(r["effect"]["page"], 2);
    let r = done(&mut m, r["effect"]["ticket"].clone(), true);
    assert_eq!(r["state"]["history"], json!([1]));
    assert_eq!(r["effect"]["kind"], "save");
    let r = done(&mut m, r["effect"]["ticket"].clone(), true);
    assert_eq!(r["saved"], "copy.pdf");
    assert_eq!(r["state"]["dirty"], true);
    assert_eq!(r["state"]["busy"], false);
}
#[test]
fn failures_preserve_history_and_drain_the_next_queued_request() {
    let mut m = open();
    let a = edit(&mut m, "annotate", json!({"annotation":annotation(0)}));
    edit(&mut m, "undo", json!({}));
    edit(&mut m, "annotate", json!({"annotation":annotation(1)}));
    let failed = done(&mut m, a["effect"]["ticket"].clone(), false);
    assert_eq!(failed["notice"]["key"], "pdf.annotError");
    assert_eq!(failed["state"]["history"], json!([]));
    assert_eq!(failed["effect"]["kind"], "annot");
    assert_eq!(failed["effect"]["page"], 1);
    let ready = done(&mut m, failed["effect"]["ticket"].clone(), true);
    let undo = edit(&mut m, "undo", json!({}));
    let failed = done(&mut m, undo["effect"]["ticket"].clone(), false);
    assert_eq!(ready["state"]["history"], failed["state"]["history"]);
    assert!(failed["notice"].is_null());
    assert_eq!(failed["state"]["dirty"], true);
}
#[test]
fn reset_never_reuses_tickets_or_accepts_old_document_completions() {
    let mut m = open();
    let old = edit(&mut m, "annotate", json!({"annotation":annotation(0)}));
    m.request(&json!({"cmd":"viewer","op":"reset"}));
    assert!(done(&mut m, old["effect"]["ticket"].clone(), true).is_null());
    edit(&mut m, "open", json!({"pages":2}));
    let fresh = edit(&mut m, "annotate", json!({"annotation":annotation(1)}));
    assert_ne!(old["effect"]["ticket"], fresh["effect"]["ticket"]);
    assert!(done(&mut m, old["effect"]["ticket"].clone(), true).is_null());
    assert_eq!(
        done(&mut m, fresh["effect"]["ticket"].clone(), true)["state"]["history"],
        json!([1])
    );
}
#[test]
fn rapid_checkbox_clicks_alternate_even_while_page_facts_are_old() {
    let mut m = open();
    let fm = field("check", json!(false));
    let a = edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}));
    assert_eq!(a["effect"]["payload"]["value"], true);
    edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}));
    let r = done(&mut m, a["effect"]["ticket"].clone(), true);
    assert_eq!(r["effect"]["payload"]["value"], false);
    let r = done(&mut m, r["effect"]["ticket"].clone(), true);
    assert_eq!(r["state"]["dirty"], true);
    assert_eq!(r["state"]["history"], json!([]));
    let a = edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}));
    done(&mut m, a["effect"]["ticket"].clone(), true);
    let b = edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}));
    assert_eq!(b["effect"]["payload"]["value"], false);
}
#[test]
fn readonly_fields_and_invalid_choices_never_reach_the_helper() {
    let mut m = open();
    let mut fm = field("text", json!("old"));
    fm["f"]["ro"] = json!(true);
    assert!(edit(&mut m, "fieldOpen", json!({"page":0,"field":fm})).is_null());
    edit(
        &mut m,
        "fieldOpen",
        json!({"page":0,"field":field("choice",json!(0))}),
    );
    assert!(m.request(&json!({"cmd":"edit","op":"fieldCommit","value":2}))["error"].is_string());
    let out = edit(&mut m, "fieldCommit", json!({"value":1}));
    assert_eq!(out["effect"]["kind"], "field");
    assert_eq!(out["effect"]["payload"]["value"], 1);
    assert!(out["state"]["field"].is_null());
}
#[test]
fn text_and_note_drafts_are_native_and_cancelled_drafts_stay_closed() {
    let mut m = open();
    edit(
        &mut m,
        "fieldOpen",
        json!({"page":0,"field":field("text",json!("old"))}),
    );
    assert_eq!(
        edit(&mut m, "fieldChange", json!({"value":"new"}))["state"]["field"]["value"],
        "new"
    );
    let saved = edit(&mut m, "fieldCommit", json!({}));
    assert_eq!(saved["effect"]["payload"]["value"], "new");
    done(&mut m, saved["effect"]["ticket"].clone(), true);
    edit(&mut m, "noteOpen", json!({"point":{"page":1,"x":5,"y":6}}));
    edit(
        &mut m,
        "noteChange",
        json!({"value":" \u{feff} note \u{feff} "}),
    );
    let saved = edit(&mut m, "noteCommit", json!({}));
    assert_eq!(saved["effect"]["payload"]["text"], "note");
    assert_eq!(saved["effect"]["payload"]["r"], json!([5., 6., 22, 22]));
    edit(&mut m, "noteCancel", json!({}));
    assert!(edit(&mut m, "noteChange", json!({"value":"late"})).is_null());
}
#[test]
fn ink_sampling_emits_deltas_and_finish_uses_original_annotation_settings() {
    let mut m = open();
    let p = json!({"page":0,"x":10,"y":10});
    assert_eq!(
        edit(&mut m, "inkStart", json!({"point":p}))["ink"]["kind"],
        "replace"
    );
    assert!(edit(
        &mut m,
        "inkAppend",
        json!({"point":{"page":0,"x":10.1,"y":10},"scale":1.3333333333333333})
    )
    .is_null());
    let out = edit(
        &mut m,
        "inkAppend",
        json!({"point":{"page":0,"x":12,"y":10},"scale":1.3333333333333333}),
    );
    assert_eq!(out["ink"], json!({"kind":"point","point":[12.,10.]}));
    assert_eq!(
        edit(&mut m, "inkStart", json!({"point":p}))["ink"]["kind"],
        "stroke"
    );
    let out = edit(&mut m, "inkFinish", json!({"color":"#30d158"}));
    assert_eq!(
        out["effect"]["payload"]["strokes"],
        json!([[10., 10., 12., 10.], [10., 10.]])
    );
    assert_eq!(out["effect"]["payload"]["width"], json!(2.2));
    assert!(out["ink"]["value"].is_null());
}
#[test]
fn old_page_info_cannot_overwrite_new_field_values() {
    let mut m = open();
    let stamp = edit(&mut m, "infoStart", json!({"page":0}));
    let a = edit(
        &mut m,
        "fieldOpen",
        json!({"page":0,"field":field("check",json!(false))}),
    );
    done(&mut m, a["effect"]["ticket"].clone(), true);
    assert!(edit(
        &mut m,
        "infoReply",
        json!({"page":0,"stamp":stamp,"info":{"fields":[field("check",json!(false))]}})
    )
    .is_null());
    let fresh = edit(&mut m, "infoStart", json!({"page":0}));
    assert!(!edit(
        &mut m,
        "infoReply",
        json!({"page":0,"stamp":fresh,"info":{"fields":[field("check",json!(true))]}})
    )
    .is_null());
    m.request(&json!({"cmd":"viewer","op":"reset"}));
    edit(&mut m, "open", json!({"pages":4}));
    assert!(edit(
        &mut m,
        "infoReply",
        json!({"page":0,"stamp":fresh,"info":{"fields":[]}})
    )
    .is_null());
}
#[test]
fn highlighting_is_tied_to_the_final_native_selection_ticket() {
    let mut m = open();
    let a = m.request(&json!({"cmd":"viewer","op":"selectionStart"}));
    let b = m.request(&json!({"cmd":"viewer","op":"selectionStart"}));
    let reply = |token| json!({"cmd":"edit","op":"highlightReply","token":token,"page":0,"result":{"text":"x","rects":[[1,2,3,4]]},"color":"#ffd60a"});
    assert!(m.request(&reply(a)).is_null());
    let out = m.request(&reply(b.clone()));
    assert_eq!(out["effect"]["payload"]["type"], "highlight");
    assert_eq!(out["clearSelection"], true);
    assert!(m.request(&reply(b)).is_null());
}

#[test]
fn reopening_a_field_uses_pending_or_committed_values_until_fresh_info_arrives() {
    let mut m = open();
    let fm = field("text", json!("old"));
    edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}));
    let out = edit(&mut m, "fieldCommit", json!({"value":"new"}));
    assert_eq!(
        edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}))["state"]["field"]["value"],
        "new"
    );
    done(&mut m, out["effect"]["ticket"].clone(), true);
    assert_eq!(
        edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}))["state"]["field"]["value"],
        "new"
    );
    let stamp = edit(&mut m, "infoStart", json!({"page":0}));
    edit(
        &mut m,
        "infoReply",
        json!({"page":0,"stamp":stamp,"info":{"fields":[field("text",json!("backend"))]}}),
    );
    assert_eq!(
        edit(&mut m, "fieldOpen", json!({"page":0,"field":fm}))["state"]["field"]["value"],
        "backend"
    );
}

#[test]
fn opening_a_document_atomically_invalidates_searches_and_selections() {
    let mut m = open();
    let old = m.request(&json!({"cmd":"viewer","op":"selectionStart"}));
    let search = m.request(&json!({"cmd":"viewer","op":"findStart","query":"x"}));
    edit(&mut m, "open", json!({"pages":3}));
    assert!(m
        .request(&json!({"cmd":"viewer","op":"findFinish","token":search["token"]}))
        .is_null());
    assert!(m.request(&json!({"cmd":"viewer","op":"selectionReply","token":old,"page":0,"result":{"text":"x","rects":[]}})).is_null());
}

#[test]
fn oversized_highlights_report_the_existing_localized_failure_without_queuing() {
    let mut m = open();
    let token = m.request(&json!({"cmd":"viewer","op":"selectionStart"}));
    let out = m.request(
        &json!({"cmd":"edit","op":"highlightReply","token":token,"page":0,
        "result":{"text":"x","rects":vec![json!([1,2,3,4]);2001]},"color":"#ffd60a"}),
    );
    assert_eq!(out["notice"]["code"], "bad-request");
    assert!(out["effect"].is_null());
    assert_eq!(out["clearSelection"], true);
    assert_eq!(out["state"]["dirty"], false);
}
