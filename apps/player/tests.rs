use super::*;

fn app() -> App {
    let mut a = App::default();
    a.resources(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"), Some("/home/test"));
    a.jobs.bind(7);
    a.booted = true;
    a.width = 960.;
    a.height = 600.;
    a
}
fn done(a: &mut App, tag: Tag, result: Result<Value, &str>) {
    a.apply(Completed { tag, result: result.map_err(str::to_owned) }, 1000);
}
/// The answer of the open job of the current epoch.
fn opened(a: &mut App, result: Result<Value, &str>) {
    let epoch = a.epoch;
    done(a, Tag::Open(epoch), result);
}
/// Run every queued job to the point of waiting for its answer: (method, args) in order.
/// Commands go through `dispatch`, which leaves the jobs queued for this to read.
fn jobs(a: &mut App) -> Vec<(String, Value)> {
    let mut out = vec![];
    let mut token = 1;
    while let Some(effect) = a.jobs.pump(0) {
        out.push((effect["args"][1].as_str().unwrap_or("").to_owned(), effect["args"][2].clone()));
        a.jobs.complete(&json!(token), 0);
        token += 1;
    }
    out
}
fn send(a: &mut App, input: Value) {
    assert!(a.dispatch(&input, 0).is_none());
}
fn queue(a: &mut App, paths: &[&str], index: usize) {
    a.play_list(paths.iter().map(|p| p.to_string()).collect(), index);
}
fn key(a: &mut App, key: &str, flags: i32, facts: KeyFacts) -> bool {
    a.handle_key(key, key, flags, &facts)
}
/// An app with a track already open as session 5.
fn playing(paths: &[&str]) -> App {
    let mut a = app();
    queue(&mut a, paths, 0);
    a.id = 5;
    a.open_for = a.current();
    a
}
fn track(path: &str, title: &str) -> Track {
    Track {
        path: path.into(),
        size: 1,
        mtime: 1.,
        title: title.into(),
        artist: "Ann".into(),
        album: "Rec".into(),
        album_artist: "Ann".into(),
        track: 1,
        year: "2001".into(),
        genre: String::new(),
        duration: 61000.,
        cover: false,
    }
}
fn media(a: &mut App, kind: &str, data: Value) {
    a.event(&json!({"kind":"media","id":5,"type":kind,"data":data}));
}
fn dark() -> std::rc::Rc<Value> {
    std::rc::Rc::new(json!({"mode":"dark","text":"#fff","textDim":"#aaa","accent":"#08f","control":"#333","hover":"#444","separator":"#222","sidebarBg":"#111","windowBg":"#000"}))
}

#[test]
fn a_track_opens_once_with_its_music_flag_and_volume() {
    let mut a = app();
    queue(&mut a, &["/m/a.mp3", "/v/b.mkv"], 0);
    a.volume = Some(0.5);
    a.open_current();
    a.open_current();
    let sent = jobs(&mut a);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "media.play");
    assert_eq!(sent[0].1[0], "player");
    assert_eq!(sent[0].1[1], "/m/a.mp3");
    assert_eq!(sent[0].1[2]["music"], true);
    assert_eq!(sent[0].1[2]["volume"], 0.5);
    opened(&mut a, Ok(json!(5)));
    assert_eq!((a.id, a.opening), (5, false));
    // a video is not music, and a new track closes the old session first
    a.queue.index = 1;
    a.open_current();
    let sent = jobs(&mut a);
    assert_eq!(sent[0], ("media.close".to_owned(), json!([5])));
    assert_eq!(sent[1].0, "media.play");
    assert_eq!(sent[1].1[2]["music"], false);
    assert_eq!(a.id, 0);
}

#[test]
fn a_session_opened_for_a_replaced_track_is_closed_when_it_arrives() {
    let mut a = app();
    queue(&mut a, &["/m/a.mp3", "/m/b.mp3"], 0);
    a.open_current();
    let stale = a.epoch;
    a.queue.index = 1;
    a.open_current();
    jobs(&mut a);
    done(&mut a, Tag::Open(stale), Ok(json!(9)));
    assert_eq!(a.id, 0);
    assert!(a.opening, "the newer open is still pending");
    assert_eq!(jobs(&mut a), vec![("media.close".to_owned(), json!([9]))]);
    opened(&mut a, Ok(json!(10)));
    assert_eq!(a.id, 10);
    // a refused open (permission, helper) leaves a message, not a session
    queue(&mut a, &["/m/c.mp3"], 0);
    a.open_current();
    opened(&mut a, Ok(json!(0)));
    assert_eq!(a.id, 0);
    assert_eq!(a.state.error.as_deref(), Some("media.noHelper"));
}

#[test]
fn events_that_arrive_before_the_open_result_are_replayed() {
    let mut a = app();
    queue(&mut a, &["/m/a.mp3"], 0);
    a.open_current();
    a.event(&json!({"kind":"media","id":5,"type":"pos","data":{"pos":1500,"duration":9000,"playing":true}}));
    assert_eq!(a.state.pos, 0.);
    opened(&mut a, Ok(json!(5)));
    assert_eq!((a.state.pos, a.state.duration, a.state.playing), (1500., 9000., true));
    // another session's events are ignored
    a.event(&json!({"kind":"media","id":6,"type":"pos","data":{"pos":99,"duration":1,"playing":false}}));
    assert_eq!(a.state.pos, 1500.);
}

#[test]
fn a_finished_track_goes_on_by_the_queue_rules() {
    let mut a = playing(&["/m/a.mp3", "/m/b.mp3"]);
    media(&mut a, "state", json!({"state":"ended"}));
    assert_eq!(a.queue.index, 1);
    // it counts once, and the last track stops without repeat
    media(&mut a, "state", json!({"state":"ended"}));
    media(&mut a, "state", json!({"state":"playing"}));
    media(&mut a, "state", json!({"state":"ended"}));
    assert_eq!(a.queue.index, 1);
    // repeat all wraps
    a.queue.repeat = Repeat::All;
    media(&mut a, "state", json!({"state":"playing"}));
    media(&mut a, "state", json!({"state":"ended"}));
    assert_eq!(a.queue.index, 0);
}

#[test]
fn repeat_one_and_a_single_track_start_the_open_session_over() {
    let mut a = playing(&["/m/a.mp3"]);
    a.queue.repeat = Repeat::One;
    media(&mut a, "state", json!({"state":"ended"}));
    let sent = jobs(&mut a);
    assert_eq!(sent[0], ("media.control".to_owned(), json!([5, "seek", 0])));
    assert_eq!(sent[1], ("media.control".to_owned(), json!([5, "play", null])));
    assert_eq!(a.queue.index, 0);
    // a click on next in a one-track queue does the same instead of doing nothing
    a.queue.repeat = Repeat::Off;
    send(&mut a, json!({"cmd":"skip"}));
    assert_eq!(jobs(&mut a).len(), 2);
}

#[test]
fn previous_restarts_a_track_that_has_been_playing_for_a_while() {
    let mut a = playing(&["/m/a.mp3", "/m/b.mp3"]);
    a.queue.index = 1;
    a.state.pos = 5000.;
    send(&mut a, json!({"cmd":"prev"}));
    assert_eq!(a.queue.index, 1);
    assert_eq!(jobs(&mut a)[0].1, json!([5, "seek", 0]));
    a.state.pos = 500.;
    send(&mut a, json!({"cmd":"prev"}));
    assert_eq!(a.queue.index, 0);
}

#[test]
fn the_library_fills_as_files_are_probed() {
    let mut a = app();
    a.home.clear();
    a.scanning = true;
    a.probing = 2;
    a.probe_total = 2;
    let file = |p: &str| Found { path: p.into(), name: base_name(p).into(), size: 1, mtime: 1. };
    done(&mut a, Tag::Probe(file("/m/a.mp3")), Ok(json!({"duration":1000,"tags":{"title":"A","album":"X"}})));
    assert!(a.scanning && a.tracks.len() == 1 && a.probing == 1);
    // a probe that failed still lists the file, by its name
    done(&mut a, Tag::Probe(file("/m/b.mp3")), Err("media.noHelper"));
    assert!(!a.scanning);
    assert_eq!(a.tracks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["A", "b"]);
    assert_eq!((a.albums.len(), a.songs.len()), (2, 2));
    assert_eq!(a.read, 2);
}

#[test]
fn a_click_on_a_row_plays_the_list_the_page_shows() {
    let mut a = app();
    a.tracks = vec![track("/m/b.mp3", "Bravo"), track("/m/a.mp3", "Alpha"), track("/v/c.mp3", "Charlie")];
    a.rebuild(0);
    send(&mut a, json!({"cmd":"row","key":1}));
    assert_eq!(a.queue.paths, ["/m/a.mp3", "/m/b.mp3", "/v/c.mp3"]);
    assert_eq!(a.queue.current(), Some("/m/b.mp3"));
    // an album page lists its own tracks, a click on a card opens it
    let album = a.albums[0].key.clone();
    send(&mut a, json!({"cmd":"album","key":album}));
    assert_eq!((a.view, a.sel.as_deref()), (View::Albums, Some(album.as_str())));
    send(&mut a, json!({"cmd":"row","key":0}));
    assert_eq!(a.queue.paths.len(), 3);
    // navigation clears the selection, except for the queue
    send(&mut a, json!({"cmd":"nav","key":"queue"}));
    assert!(a.sel.is_some());
    send(&mut a, json!({"cmd":"nav","key":"songs"}));
    assert!(a.sel.is_none());
    // a row out of range does nothing
    send(&mut a, json!({"cmd":"row","key":99}));
    assert_eq!(a.queue.paths.len(), 3);
}

#[test]
fn the_time_bar_follows_the_pointer_and_seeks_on_release() {
    let mut a = playing(&["/m/a.mp3"]);
    a.state.duration = 100_000.;
    let (left, width) = ui::geometry(&a).time;
    assert_eq!((left, width), (960. / 2. - 190. + 48., 284.));
    send(&mut a, json!({"cmd":"timeDown","x":left + width / 2.}));
    assert!(a.drag.is_some());
    assert_eq!(a.seek_preview, Some(50_000.));
    send(&mut a, json!({"cmd":"pointerMove","x":left + width * 2.}));
    assert_eq!(a.seek_preview, Some(100_000.));
    send(&mut a, json!({"cmd":"pointerMove","x":left + width * 0.25}));
    assert!(jobs(&mut a).is_empty(), "nothing is sent while dragging");
    send(&mut a, json!({"cmd":"dragEnd"}));
    assert!(a.drag.is_none());
    assert_eq!(jobs(&mut a)[0], ("media.control".to_owned(), json!([5, "seek", 25000.0])));
    assert!(a.seek_preview.is_none());
}

#[test]
fn the_volume_bar_sets_steps_of_a_hundredth_up_to_one_and_a_half() {
    let mut a = playing(&["/m/a.mp3"]);
    let (left, width) = ui::geometry(&a).volume.unwrap();
    send(&mut a, json!({"cmd":"volDown","x":left + width / 2.}));
    assert_eq!(a.volume, Some(0.75));
    send(&mut a, json!({"cmd":"pointerMove","x":left + width * 5.}));
    assert_eq!(a.volume, Some(1.5));
    send(&mut a, json!({"cmd":"dragEnd"}));
    let sent = jobs(&mut a);
    assert_eq!(sent[0].1, json!([5, "volume", 0.75]));
    assert_eq!(sent.last().unwrap().1, json!([5, "volume", 1.5]));
    // narrow windows have no volume bar
    a.width = 600.;
    assert!(ui::geometry(&a).volume.is_none());
}

#[test]
fn the_bars_sit_where_the_layout_puts_them_in_both_directions() {
    let mut a = playing(&["/v/a.mkv"]);
    a.show_video = true;
    a.state.reduce("ready", &json!({"video":0,"streams":[{"type":"audio","index":1},{"type":"audio","index":2}]}));
    let ltr = ui::geometry(&a);
    assert_eq!(ltr.time, (68., 960. - 136.));
    // subtitles, picture in picture, full screen and a second audio track: four buttons
    assert_eq!(ltr.volume.unwrap().0, 960. - 20. - (25. + 84. + 36. * 4.) + 25.);
    a.rtl = true;
    assert_eq!(ui::geometry(&a).volume.unwrap().0, 45.);
    a.show_video = false;
    a.width = 600.;
    assert_eq!(ui::geometry(&a).time, (16. + 48., 600. - 192.));
    a.rtl = false;
    assert_eq!(ui::geometry(&a).time, (80. + 48., 600. - 192.));
}

#[test]
fn keys_play_seek_and_change_the_volume() {
    let mut a = playing(&["/m/a.mp3"]);
    a.state.pos = 10_000.;
    assert!(key(&mut a, "ArrowRight", 0, KeyFacts::default()));
    assert!(key(&mut a, "ArrowLeft", 0, KeyFacts { shift: true, ..Default::default() }));
    assert!(key(&mut a, " ", 0, KeyFacts::default()));
    assert!(key(&mut a, "ArrowDown", 0, KeyFacts::default()));
    assert!(!key(&mut a, "x", 0, KeyFacts::default()));
    assert!(!key(&mut a, "ArrowRight", 2, KeyFacts::default()), "Ctrl combinations stay with the shell");
    let sent = jobs(&mut a);
    assert_eq!(sent[0].1, json!([5, "seek", 15000.0]));
    assert_eq!(sent[1].1, json!([5, "seek", 0.0]));
    assert_eq!(sent[2].1, json!([5, "play", null]));
    assert_eq!(sent[3].1[1], "volume");
    // F is full screen only when there is a picture
    assert!(!key(&mut a, "f", 0, KeyFacts::default()));
    a.state.reduce("ready", &json!({"video":0}));
    assert!(key(&mut a, "f", 0, KeyFacts { base: "f", ..Default::default() }));
    assert_eq!(a.effects[0]["kind"], "fullscreen");
    assert_eq!(a.effects[0]["on"], true);
}

#[test]
fn picture_in_picture_asks_for_the_screen_then_places_the_window() {
    let mut a = playing(&["/v/a.mkv"]);
    a.window = "player#2".into();
    a.state.reduce("ready", &json!({"video":0,"streams":[{"type":"video","width":1600,"height":900}]}));
    send(&mut a, json!({"cmd":"pip"}));
    assert_eq!(jobs(&mut a)[0], ("windows.screen".to_owned(), json!([])));
    done(&mut a, Tag::Screen, Ok(json!({"width":1920,"height":1080})));
    let sent = jobs(&mut a);
    assert_eq!(sent[0].0, "windows.pip");
    assert_eq!(sent[0].1[0], "player#2");
    assert_eq!(sent[0].1[1], json!({"x":1496.0,"y":759.0,"w":400.0,"h":225.0}));
    // in picture in picture the same button gives the window back
    a.window_mode = "pip".into();
    send(&mut a, json!({"cmd":"pip"}));
    assert_eq!(jobs(&mut a)[0].1, json!(["player#2", null]));
}

#[test]
fn full_screen_remembers_the_session_for_the_remount_it_causes() {
    let mut a = playing(&["/m/a.mp3", "/m/b.mp3"]);
    a.window = "player".into();
    a.args_seen = json!({"path":null,"nonce":null});
    a.queue.index = 1;
    a.queue.shuffle = true;
    a.queue.repeat = Repeat::All;
    a.view = View::Queue;
    a.volume = Some(0.8);
    a.state.pos = 42_000.;
    a.state.playing = true;
    send(&mut a, json!({"cmd":"fullscreen"}));
    let saved = a.effects.iter().find(|e| e["kind"] == "windowState").unwrap()["state"].clone();
    assert!(a.effects.iter().any(|e| e["kind"] == "fullscreen"));
    let mut b = app();
    b.restore_state(&saved, &json!({}));
    assert_eq!(b.queue.paths, ["/m/a.mp3", "/m/b.mp3"]);
    assert_eq!((b.queue.index, b.queue.shuffle, b.queue.repeat), (1, true, Repeat::All));
    assert_eq!((b.view, b.volume), (View::Queue, Some(0.8)));
    assert_eq!(b.restore, Some((42_000., true)));
    // the position comes back once the file is ready
    b.id = 5;
    media(&mut b, "ready", json!({"duration":90000}));
    assert_eq!(jobs(&mut b)[0].1, json!([5, "seek", 42000.0]));
    // a state saved for another file is not applied
    let mut c = app();
    c.restore_state(&saved, &json!({"path":"/other.mp3","nonce":1}));
    assert!(c.queue.paths.is_empty());
}

#[test]
fn a_long_queue_is_not_stored_whole() {
    let mut a = app();
    a.window = "player".into();
    let paths: Vec<String> = (0..2000).map(|i| format!("/m/{i}.mp3")).collect();
    a.play_list(paths, 1500);
    a.checkpoint(true);
    let state = &a.effects[0]["state"];
    assert_eq!(state["queue"], json!(["/m/1500.mp3"]));
    assert_eq!(state["index"], 0);
}

#[test]
fn the_control_center_entry_follows_the_track_and_is_withdrawn_with_it() {
    let mut a = playing(&["/m/a.mp3"]);
    a.window = "player".into();
    a.tracks = vec![track("/m/a.mp3", "Alpha")];
    a.state.playing = true;
    a.state.pos = 12_500.;
    a.publish_now_playing();
    let sent = jobs(&mut a);
    assert_eq!(sent[0].0, "media.nowPlaying");
    assert_eq!(sent[0].1[0]["title"], "Alpha");
    assert_eq!(sent[0].1[0]["pos"], 12000.0);
    // within the same second nothing is sent again
    a.state.pos = 12_900.;
    a.publish_now_playing();
    assert!(jobs(&mut a).is_empty());
    // the buttons of the entry come back as events
    a.event(&json!({"kind":"nowPlaying","action":"toggle"}));
    assert_eq!(jobs(&mut a)[0].1, json!([5, "pause", null]));
    a.queue.play_list(vec![], 0);
    a.open_current();
    a.publish_now_playing();
    let sent = jobs(&mut a);
    assert_eq!(sent.last().unwrap(), &("media.nowPlaying".to_owned(), json!([null])));
}

#[test]
fn dropped_files_replace_the_queue_and_only_media_counts() {
    let mut a = app();
    send(&mut a, json!({"cmd":"drop","paths":["/x/readme.txt","/v/film.mkv","/m/song.flac"]}));
    assert_eq!(a.queue.paths, ["/v/film.mkv", "/m/song.flac"]);
    assert!(a.show_video);
    send(&mut a, json!({"cmd":"drop","paths":["/x/readme.txt"]}));
    assert_eq!(a.queue.paths.len(), 2);
}

#[test]
fn subtitle_choices_follow_the_original_menu() {
    let mut a = playing(&["/v/a.mkv"]);
    a.ext_subs = Some(vec![Cue { start: 0., end: 1000., text: "file".into() }]);
    send(&mut a, json!({"cmd":"subStream","key":3}));
    assert_eq!(a.sub_stream, Some(3));
    assert_eq!(jobs(&mut a)[0].1, json!([5, "subTrack", 3]));
    send(&mut a, json!({"cmd":"subFile"}));
    assert!(a.sub_stream.is_none() && a.ext_subs.is_some());
    assert_eq!(jobs(&mut a)[0].1, json!([5, "subTrack", -1]));
    send(&mut a, json!({"cmd":"subOff"}));
    assert!(a.ext_subs.is_none());
    send(&mut a, json!({"cmd":"menu","key":"audio"}));
    assert_eq!(a.menu, Menu::Audio);
    send(&mut a, json!({"cmd":"menu","key":"audio"}));
    assert_eq!(a.menu, Menu::Closed);
    a.menu = Menu::Subs;
    send(&mut a, json!({"cmd":"dismiss"}));
    assert_eq!(a.menu, Menu::Closed);
}

#[test]
fn the_trees_show_the_library_the_video_and_the_empty_states() {
    let mut a = app();
    a.theme = dark();
    let text = ui::tree(&a, 0).to_string();
    assert!(text.contains("player.emptyMusicTitle") && text.contains("player.nav.songs"));
    assert!(!text.contains("\"key\":\"bottom\""), "no bar without a track");
    a.view = View::Videos;
    assert!(ui::tree(&a, 0).to_string().contains("player.emptyVideosTitle"));
    a.tracks = vec![track("/m/a.mp3", "Alpha")];
    a.rebuild(0);
    a.view = View::Songs;
    queue(&mut a, &["/m/a.mp3"], 0);
    let text = ui::tree(&a, 0).to_string();
    assert!(text.contains("Alpha") && text.contains("\"key\":\"bottom\"") && text.contains("dropAction"));
    // a video fills the window with the session's picture
    queue(&mut a, &["/v/a.mkv"], 0);
    a.id = 5;
    let text = ui::tree(&a, 0).to_string();
    assert!(text.contains("\"kind\":\"media\"") && text.contains("\"media\":5"));
    // the controls hide while the pointer rests, and an error stays on screen
    a.state.playing = true;
    a.last_move = 0;
    assert!(!ui::tree(&a, 10_000).to_string().contains("\"key\":\"overlay\""));
    assert!(ui::tree(&a, 1_000).to_string().contains("\"key\":\"overlay\""));
    a.state.error = Some("crash".into());
    assert!(ui::tree(&a, 10_000).to_string().contains("player.crashed"));
}

#[test]
fn a_big_library_builds_only_the_rows_in_view() {
    let mut a = app();
    a.theme = dark();
    a.tracks = (0..5000).map(|i| track(&format!("/m/{i:04}.mp3"), &format!("Song {i:04}"))).collect();
    a.rebuild(0);
    let rows = |a: &App| ui::tree(a, 0).to_string().matches("\"key\":\"row").count();
    let first = rows(&a);
    assert!(first > 10 && first < 40, "{first}");
    send(&mut a, json!({"cmd":"scroll","deltaY":3000.0}));
    assert_eq!(a.offset, 3000.);
    assert!(rows(&a) < 40);
    send(&mut a, json!({"cmd":"scroll","deltaY":1.0e9}));
    assert_eq!(a.offset, a.max_offset());
}

#[test]
fn open_with_starts_the_file_once_per_request() {
    let mut a = app();
    a.home.clear();
    a.theme = dark();
    let sync = |a: &mut App, nonce: i64| {
        a.request(&json!({"cmd":"sync","lang":"en","context":{"windowMode":"window","width":960,"height":600},
            "args":{"__nativeOwner":7,"__nativeWindow":"player","path":"/v/film.mkv","nonce":nonce}}))
    };
    sync(&mut a, 1);
    assert_eq!(a.queue.current(), Some("/v/film.mkv"));
    assert!(a.show_video);
    a.queue.play_list(vec!["/m/other.mp3".into()], 0);
    sync(&mut a, 1);
    assert_eq!(a.queue.current(), Some("/m/other.mp3"));
    sync(&mut a, 2);
    assert_eq!(a.queue.current(), Some("/v/film.mkv"));
}

#[test]
fn the_manifest_names_only_services_the_runtime_provides() {
    let m = crate::manifest::manifest();
    assert_eq!(m["id"], crate::ID);
    for service in m["asyncServices"].as_array().unwrap() {
        let service = service.as_str().unwrap();
        assert!(service.starts_with("media.") || service.starts_with("windows."), "{service}");
    }
}
