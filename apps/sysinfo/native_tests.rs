use super::*;

#[test]
fn shared_reports_match_the_legacy_builder_reference_cases() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-app-sysinfo/tests/fixtures/sharing.json"
    ))
    .unwrap();
    for (index, input) in arr(&cases).iter().enumerate() {
        assert_eq!(
            sharing::build(&input["report"], &input["bench"], &input["desktop"]),
            input["expected"],
            "sharing reference {index}"
        );
    }
}

fn fixture() -> App {
    let mut app = App::default();
    app.booted = true;
    app.jobs.bind(1);
    app.epoch = 1;
    app.window = "sysinfo#1".into();
    app.snapshot = json!({"report":{"date":"2026-11-23","software":{"lordos":"0.8.7","os":"Fedora 44","kernel":"6.18"},
        "machine":{"vendor":"ACME","product":"Computer"},"cpu":{"model":"Example CPU","cores":4,"threads":8},
        "memory":{"totalKB":8388608},"session":{"composition":"gpu"},"gpus":[],"storage":[]},
        "markdown":"# Hardware\n","canAdmin":true,"canPower":true});
    app.theme = Rc::new(
        json!({"text":"#eee","textDim":"#aaa","windowBg":"#222","sidebarBg":"#333","separator":"#555","control":"#444","accent":"#0a84ff"}),
    );
    app
}
fn done(app: &mut App, tag: Tag, value: Value, now: i64) {
    app.apply(
        Completed {
            tag,
            result: Ok(value),
        },
        now,
    );
}

#[test]
fn share_preview_and_exports_never_receive_serials_or_peripheral_names() {
    let mut app = fixture();
    app.serials = json!([{"kind":"system","value":"PRIVATE-SERIAL"}]);
    app.local = json!({"devices":[{"name":"PRIVATE-OWNER"}]});
    app.snapshot["report"]["machine"]["serial"] = json!("INJECTED-SERIAL");
    app.snapshot["report"]["extra"] = json!("PRIVATE-EXTRA");
    app.request_at(&json!({"cmd":"share"}), 100);
    assert!(app.share_json.contains("ACME"));
    for secret in [
        "PRIVATE-SERIAL",
        "PRIVATE-OWNER",
        "INJECTED-SERIAL",
        "PRIVATE-EXTRA",
    ] {
        assert!(!app.share_json.contains(secret));
    }
    assert!(
        app.export.is_none(),
        "preview is not consent to write or open a browser"
    );
    let result = app.request_at(&json!({"cmd":"save"}), 100);
    assert_eq!(result["effect"]["args"][1], "sysinfo.save");
    let files = &result["effect"]["args"][2][0];
    assert_eq!(files[0]["text"], app.share_json);
    assert!(text(&files[1]["text"]).contains(text(&files[0]["name"])));
    assert!(!text(&files[1]["text"]).contains("<script"));
    assert!(text(&files[1]["text"]).contains("default-src 'none'"));
}

#[test]
fn stale_privileged_and_refresh_results_do_not_replace_current_data() {
    let mut app = fixture();
    let generation = app.serial_generation;
    app.hide_serials();
    done(
        &mut app,
        Tag::Serials(generation),
        json!(["system\tPRIVATE"]),
        0,
    );
    assert!(app.serials.is_null());
    let before = app.snapshot.clone();
    done(
        &mut app,
        Tag::Snapshot(0),
        json!({"report":{"machine":{"vendor":"STALE"}}}),
        0,
    );
    assert_eq!(app.snapshot, before);
    app.snapshot["canAdmin"] = json!(false);
    app.request_at(&json!({"cmd":"serials"}), 100);
    assert!(!app.serial_busy);
    assert_eq!(app.section, "computer");
    app.request_at(&json!({"cmd":"section","key":"not-a-section"}), 100);
    assert_eq!(app.section, "computer");
}

#[test]
fn export_collision_keeps_the_exact_confirmed_bytes_and_format() {
    let mut app = fixture();
    app.tab = "report".into();
    let first = app.request_at(&json!({"cmd":"save"}), 100);
    assert_eq!(
        first["effect"]["args"][2][0][0]["name"],
        "lordos-report-2026-11-23.md"
    );
    app.format = "json".into();
    app.snapshot["markdown"] = json!("new text");
    app.apply(
        Completed {
            tag: Tag::Export,
            result: Err("sysinfo.exportExists".into()),
        },
        100,
    );
    let export = app.export.as_ref().unwrap();
    assert_eq!(export.attempt, 2);
    assert_eq!(export.value["text"], "# Hardware\n");
    assert_eq!(export.value["format"], "markdown");
}

#[test]
fn probe_times_out_and_quiet_pages_have_no_timer() {
    let mut app = fixture();
    assert_eq!(app.refresh_ms(), None);
    done(&mut app, Tag::ProbeStart, json!(true), 1000);
    assert!(app.refresh_ms().is_some());
    app.schedule(11001);
    assert_eq!(app.probe_until, 0);
    assert_eq!(app.message, key("sysinfo.probe.error.timeout"));
    assert_eq!(app.refresh_ms(), None);
    app.scene = true;
    assert_eq!(
        app.refresh_ms(),
        None,
        "scene movement belongs to the native WM script, not a UI timer"
    );
}

#[test]
fn native_pages_and_benchmark_measurements_preserve_their_models() {
    let mut app = fixture();
    app.saved = json!([{"at":100,"mode":"quick","lordos":"0.8.7","lordbench":1,
        "results":{"cpu.sha256.1":{"value":100,"unit":"MB/s"}},"seconds":20},null,null,null]);
    for tab in ["overview", "hardware", "compat", "checks", "perf", "report"] {
        app.tab = tab.into();
        let tree = ui::tree(&mut app).to_string();
        assert!(tree.contains("sysinfo.tab.overview"));
        if tab == "perf" {
            assert!(tree.contains("cpu_sha256_1"));
            assert!(tree.contains("MB/s"));
            assert!(tree.contains("sysinfo.perf.note.none"));
            assert!(tree.contains("sysinfo.perf.c.temps"));
        }
    }
}

#[test]
fn shared_benchmarks_require_the_same_version_and_html_escapes_hardware() {
    let mut app = fixture();
    app.snapshot["report"]["machine"]["vendor"] = json!("<img src=x>");
    let bench = json!({"lordos":"0.1.0","mode":"full"});
    let report = sharing::build(&app.snapshot["report"], &bench, &Value::Null);
    assert!(report["bench"].is_null());
    let page = sharing::page(&report, "report.json", &mut app.i18n);
    assert!(!page.contains("<img src=x>"));
    assert!(page.contains("&lt;img src=x&gt;"));
}

#[test]
fn failed_benchmark_start_releases_waiting_state() {
    let mut app = fixture();
    app.bench = json!({"state":"waiting"});
    app.apply(
        Completed {
            tag: Tag::Start,
            result: Err("unavailable".into()),
        },
        100,
    );
    assert!(!app.running());
    assert_eq!(app.refresh_ms(), None);
}

#[test]
fn community_reference_machines_are_identified_separately() {
    let mut app = fixture();
    app.tab = "compat".into();
    app.snapshot["community"] = json!({"state":"ok","snapshot":{"format":"lordos-hwsnapshot","version":1,"reports":1,
        "devices":[{"component":"display","key":"machine:ACME|Computer","lordos":"0.8.7","works":1,"limits":0,"no":0,"reports":1,"references":1}]}});
    let tree = ui::tree(&mut app).to_string();
    assert!(tree.contains("sysinfo.community.reference.works"));
    assert!(!tree.contains("sysinfo.community.counts"));
}

#[test]
fn schema_filters_dynamic_property_names_and_applies_limits_after_validation() {
    let mut app = fixture();
    app.snapshot["report"]["software"]["kernelConfig"] =
        json!({"CONFIG_SECCOMP":"y","PRIVATE-OWNER":"y"});
    let report = sharing::build(&app.snapshot["report"], &Value::Null, &Value::Null);
    assert_eq!(
        report["system"]["software"]["kernelConfig"],
        json!({"CONFIG_SECCOMP":"y"})
    );
    let spec = json!({"type":"array","maxItems":2,"items":{"type":"integer","minimum":0}});
    assert_eq!(
        sharing::pick(&json!([-1, 4, 5, 6]), &spec, &spec, 0),
        Some(json!([4, 5]))
    );
}

#[test]
fn exporting_a_fresh_benchmark_uses_the_visible_result() {
    let mut app = fixture();
    app.tab = "perf".into();
    app.saved[0] = json!({"at":1,"mode":"quick","results":{"old":true}});
    let fresh = json!({"at":1795450000,"mode":"full","status":"done","results":{"fresh":true}});
    app.bench = json!({"state":"idle","result":fresh});
    let reply = app.request_at(&json!({"cmd":"save","key":"0"}), 100);
    assert_eq!(
        serde_json::from_str::<Value>(text(&reply["effect"]["args"][2][0][0]["text"])).unwrap(),
        fresh
    );
}

#[test]
fn firmware_dates_validate_the_day_before_localized_display() {
    assert_eq!(firmware_date("02/29/2024"), Some((2024, 2, 29)));
    assert_eq!(firmware_date("2026-11-23"), Some((2026, 11, 23)));
    for invalid in [
        "02/29/2025",
        "2026-13-01",
        "2026-11-00",
        "2026-11-31",
        "unknown",
    ] {
        assert_eq!(firmware_date(invalid), None);
    }
}
