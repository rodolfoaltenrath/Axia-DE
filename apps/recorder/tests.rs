use super::*;
#[path = "native_tests.rs"]
mod native;
#[test]
fn rgba_waveforms_match_original_jsx_pixels() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/recorder_wave.json"
    ))
    .unwrap();
    for c in cases.as_array().unwrap() {
        let peaks: Vec<f64> = serde_json::from_value(c["peaks"].clone()).unwrap();
        let sel: Option<[f64; 2]> = serde_json::from_value(c["sel"].clone()).unwrap();
        let pixels = wave::draw(
            &peaks,
            c["width"].as_u64().unwrap() as usize,
            c["height"].as_u64().unwrap() as usize,
            c["dark"].as_bool().unwrap(),
            c["played"].as_f64().unwrap(),
            sel,
        );
        let hash = pixels
            .iter()
            .fold(2166136261u32, |h, p| (h ^ *p as u32).wrapping_mul(16777619));
        assert_eq!(hash as u64, c["hash"].as_u64().unwrap(), "{c}");
    }
}
fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a.jobs.bind(1);
    a.booted = true;
    a.music = "/home/test/Music".into();
    a.folder = "/home/test/Music/Recordings".into();
    a
}
fn complete(a: &mut App, tag: Tag, result: Result<Value, &str>) {
    a.apply(
        Completed {
            tag,
            result: result.map_err(str::to_owned),
        },
        1000,
    );
}
#[test]
fn late_waveforms_and_players_cannot_replace_selection() {
    let mut a = app();
    a.select(Some("/a.wav".into()));
    let g = a.selection_gen;
    a.select(Some("/b.wav".into()));
    complete(
        &mut a,
        Tag::Wave(g),
        Ok(json!({"duration":5000,"peaks":[1,2]})),
    );
    assert!(a.wave.is_none());
    complete(&mut a, Tag::Play(g), Ok(json!(10)));
    assert_eq!(a.play, 0);
}
#[test]
fn finished_recording_selects_output_and_closes_microphone() {
    let mut a = app();
    a.rec = 7;
    a.paused = true;
    a.event(
        &json!({"kind":"media","id":7,"type":"done","data":{"ok":true,"path":"/a.wav"}}),
        1000,
    );
    assert_eq!(a.rec, 0);
    assert!(!a.paused);
    assert_eq!(a.selection.as_deref(), Some("/a.wav"));
    assert!(a.jobs.busy());
}
#[test]
fn levels_are_bounded_and_record_permission_denial_recovers() {
    let mut a = app();
    a.rec = 7;
    for i in 0..1000 {
        a.event(
            &json!({"kind":"media","id":7,"type":"level","data":{"ms":i*100,"level":500}}),
            1000,
        );
    }
    assert_eq!(a.levels.len(), 181);
    assert_eq!(a.rec_ms, 99900.);
    a.rec = 0;
    a.record_start = true;
    complete(&mut a, Tag::Record, Ok(json!(0)));
    assert!(!a.record_start);
    assert_eq!(a.message["key"], "recorder.denied");
}
#[test]
fn media_events_arriving_before_start_result_are_replayed() {
    let mut a = app();
    a.record_start = true;
    a.event(
        &json!({"kind":"media","id":8,"type":"level","data":{"ms":400,"level":200}}),
        1000,
    );
    assert_eq!(a.deferred.len(), 1);
    complete(&mut a, Tag::Record, Ok(json!(8)));
    assert_eq!(a.rec_ms, 400.);
    assert_eq!(a.levels, vec![0.2]);
    assert!(a.deferred.is_empty());
}
#[test]
fn trim_capture_clamps_and_tracks_edge_after_crossing() {
    let mut a = app();
    a.width = 800.;
    a.trim = Some([0.2, 0.8]);
    let left = a.wave_left();
    let w = a.wave_width() as f64;
    assert_eq!(
        a.request_at(&json!({"cmd":"waveDown","x":left+w*0.2}), 1000)["capture"],
        true
    );
    a.request_at(&json!({"cmd":"pointerMove","x":left+w*0.9}), 1000);
    assert_eq!(a.trim, Some([0.8, 0.9]));
    assert_eq!(a.drag, Some(1));
    a.request_at(&json!({"cmd":"pointerMove","x":10000}), 1000);
    assert_eq!(a.trim, Some([0.8, 1.]));
    a.request_at(&json!({"cmd":"dragEnd"}), 1000);
    assert!(a.drag.is_none());
    a.rtl = true;
    assert_eq!(a.wave_left(), 20.);
}
#[test]
fn trim_pauses_playback_once_at_end_including_background() {
    let mut a = app();
    a.play = 7;
    a.trim = Some([0.1, 0.8]);
    a.wave = Some(Wave {
        duration: 1000.,
        peaks: vec![],
    });
    assert_eq!(a.background_refresh_ms(), Some(100.));
    a.event(
        &json!({"kind":"media","id":7,"type":"pos","data":{"pos":850,"playing":true}}),
        1000,
    );
    assert!(a.trim_paused);
    assert!(!a.playing);
    let effect = a.jobs.pump(1000).unwrap();
    assert_eq!(effect["args"][1], "media.control");
    assert_eq!(effect["args"][2], json!([7, "pause", null]));
}
#[test]
fn export_snapshots_path_format_and_range() {
    let mut a = app();
    a.selection = Some("/a.wav".into());
    a.wave = Some(Wave {
        duration: 1200.,
        peaks: vec![],
    });
    a.trim = Some([0.1, 0.9]);
    a.format = "mp3".into();
    a.export(true, 1000);
    let t = a.target.as_ref().unwrap();
    assert_eq!(t.ext, "wav");
    match &t.operation {
        Operation::Export {
            input,
            format,
            range,
            select,
        } => {
            assert_eq!(input, "/a.wav");
            assert_eq!(format, "wav");
            assert_eq!(*range, Some([120., 1080.]));
            assert!(*select);
        }
        _ => panic!(),
    };
    assert!(a.exporting);
    let generation = a.op_gen;
    complete(&mut a, Tag::Export(generation), Err("failed"));
    assert!(!a.exporting);
    assert_eq!(a.message["key"], "recorder.exportError");
}
#[test]
fn waveform_has_transparent_gaps_and_selection_alpha() {
    let p = wave::draw(&[100., 100.], 6, 10, false, 0., Some([0.3, 0.7]));
    assert_eq!(&p[(5 * 6) * 4..(5 * 6) * 4 + 4], &[224, 53, 43, 60]);
    assert_eq!(p[(5 * 6 + 2) * 4 + 3], 0);
    assert_eq!(p[(5 * 6 + 3) * 4 + 3], 170);
    assert_eq!(duration(3_661_999.), "1:01:01");
}
