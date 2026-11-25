use super::*;
use serde_json::{json, Value};
use std::cmp::Ordering;

fn found(path: &str, size: u64, mtime: f64) -> Found {
    Found { path: path.into(), name: base_name(path).into(), size, mtime }
}
fn track(path: &str, title: &str, artist: &str, album: &str, album_artist: &str, n: i64) -> Track {
    Track {
        path: path.into(),
        size: 1,
        mtime: 1.,
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        album_artist: album_artist.into(),
        track: n,
        year: "2001".into(),
        genre: String::new(),
        duration: 1000.,
        cover: false,
    }
}

#[test]
fn extensions_follow_the_original_regular_expressions() {
    assert!(is_audio("/m/a.MP3") && is_audio("a.flac") && is_audio("/x/y.mka"));
    assert!(is_video("/v/a.MKV") && is_video("clip.ts") && is_video("a.3gp"));
    assert!(!is_audio("/m/a.mp3.txt") && !is_audio("mp3") && !is_audio("a.") && !is_audio(""));
    assert!(!is_video("/v/a.mp4/") && !is_video("a.mp4x"));
    assert!(is_media("a.ogg") && is_media("a.webm") && !is_media("a.pdf"));
}

#[test]
fn names_stems_and_the_subtitle_file() {
    assert_eq!(base_name("/a/b/c.mp3"), "c.mp3");
    assert_eq!(base_name("plain"), "plain");
    assert_eq!(stem("/a/b/c.d.mp3"), "c.d");
    assert_eq!(stem("/a/b/.hidden"), ".hidden");
    assert_eq!(stem("/a/b/noext"), "noext");
    assert_eq!(srt_path("/v/movie.mkv"), "/v/movie.srt");
    assert_eq!(srt_path("/v.d/movie"), "/v.d/movie");
    assert_eq!(srt_path("/v/a.b.mp4"), "/v/a.b.srt");
}

#[test]
fn time_is_not_padded_below_one_hour_and_is_padded_above() {
    assert_eq!(format_time(0.), "0:00");
    assert_eq!(format_time(65_000.), "1:05");
    assert_eq!(format_time(599_999.), "9:59");
    assert_eq!(format_time(3_725_000.), "1:02:05");
    assert_eq!(format_time(f64::NAN), "0:00");
    assert_eq!(format_time(-5.), "0:00");
}

#[test]
fn a_track_comes_from_its_tags_or_its_file_name() {
    let info = json!({"duration":215000.5,"tags":{"title":"Song","artist":"Ann","album":"Rec","album_artist":"Various",
        "track":"3/12","date":"2019-05-01","genre":"Pop"},
        "streams":[{"type":"audio"},{"type":"video","cover":true}]});
    let t = track_from(&found("/m/x.mp3", 10, 5.), &info);
    assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str()), ("Song", "Ann", "Rec"));
    assert_eq!((t.album_artist.as_str(), t.track, t.year.as_str(), t.genre.as_str()), ("Various", 3, "2019", "Pop"));
    assert!(t.cover);
    assert_eq!(t.duration, 215000.5);
    let bare = track_from(&found("/m/Some Name.flac", 1, 1.), &Value::Null);
    assert_eq!(bare.title, "Some Name");
    assert_eq!((bare.artist.as_str(), bare.album.as_str(), bare.track, bare.duration), ("", "", 0, 0.));
    assert!(!bare.cover);
    // the artist stands in for a missing album artist and the other way round
    let only_artist = track_from(&found("/a.mp3", 1, 1.), &json!({"tags":{"artist":"Zed"}}));
    assert_eq!((only_artist.artist.as_str(), only_artist.album_artist.as_str()), ("Zed", "Zed"));
    let only_album_artist = track_from(&found("/a.mp3", 1, 1.), &json!({"tags":{"album_artist":"Band"}}));
    assert_eq!((only_album_artist.artist.as_str(), only_album_artist.album_artist.as_str()), ("Band", "Band"));
    // empty, zero and unparseable values count as missing, a numeric tag is read as its digits
    let odd = track_from(&found("/o.mp3", 1, 1.), &json!({"tags":{"title":"","track":"x","date":"","album":0}}));
    assert_eq!((odd.title.as_str(), odd.track, odd.year.as_str(), odd.album.as_str()), ("o", 0, "", ""));
    assert_eq!(parse_int("  7abc"), Some(7));
    assert_eq!(parse_int("-2/9"), Some(-2));
    assert_eq!(parse_int(""), None);
}

#[test]
fn only_a_file_whose_size_and_date_are_unchanged_skips_the_probe() {
    let a = found("/m/a.mp3", 100, 1000.4);
    let b = found("/m/b.mp3", 200, 2000.);
    let ta = track_from(&a, &json!({"tags":{"title":"A"}}));
    let cache = cache_json(&[ta.clone()]);
    assert_eq!(cache["/m/a.mp3"]["k"], "100:1000");
    // the file kept its size and date, the other one is new
    let (known, todo) = partition(&[a.clone(), b.clone()], &cache);
    assert_eq!(known, vec![ta.clone()]);
    assert_eq!(todo, vec![b]);
    // the file changed: it is read again
    let (known, todo) = partition(&[found("/m/a.mp3", 101, 1000.4)], &cache);
    assert!(known.is_empty() && todo.len() == 1);
    let (known, todo) = partition(&[found("/m/a.mp3", 100, 1001.)], &cache);
    assert!(known.is_empty() && todo.len() == 1);
    // a damaged entry never panics
    let (known, todo) = partition(&[a], &json!({"/m/a.mp3":{"k":"100:1000","t":{"title":5}}}));
    assert!(known.is_empty() && todo.len() == 1);
    assert!(cache_dirty(1, 5, 5) && cache_dirty(0, 5, 4) && !cache_dirty(0, 5, 5));
}

#[test]
fn a_cache_written_by_the_javascript_player_is_still_read() {
    let old = json!({"/m/a.mp3":{"k":"100:1000","t":{"path":"/m/a.mp3","size":100,"mtime":1000,"title":"A","artist":"X",
        "album":"Y","albumArtist":"Z","track":2,"year":"1999","genre":"","duration":1234,"cover":true}}});
    let (known, todo) = partition(&[found("/m/a.mp3", 100, 1000.)], &old);
    assert!(todo.is_empty());
    assert_eq!(known[0].album_artist, "Z");
    assert_eq!((known[0].track, known[0].duration, known[0].cover), (2, 1234., true));
    assert_eq!(known[0].to_json()["albumArtist"], "Z");
}

#[test]
fn albums_and_artists_are_grouped_and_sorted() {
    let tracks = vec![
        track("/1", "Beta", "B", "Two", "B", 2),
        track("/2", "Alpha", "B", "Two", "B", 1),
        track("/3", "Gamma", "B", "Two", "B", 1),
        track("/4", "One", "a", "First", "a", 0),
        track("/5", "Solo", "Guest", "Mix", "a", 0),
    ];
    let albums = albums_of(&tracks);
    // artists compare without caring for case
    assert_eq!(albums.iter().map(|a| a.album.as_str()).collect::<Vec<_>>(), ["First", "Mix", "Two"]);
    let two = &albums[2];
    assert_eq!(two.key, "B\u{0}Two");
    // by number, then by title
    assert_eq!(two.tracks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Alpha", "Gamma", "Beta"]);
    let artists = artists_of(&tracks);
    assert_eq!(artists.iter().map(|a| (a.name.as_str(), a.tracks.len())).collect::<Vec<_>>(), [("a", 2), ("B", 3)]);
    // the page of a guest artist still lists the album they appear on
    let listed = albums_of_artist(&albums, "Guest");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].album, "Mix");
    assert_eq!(albums_of_artist(&albums, "a").len(), 2);
    assert_eq!(songs_sorted(&tracks).iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta", "Gamma", "One", "Solo"]);
}

#[test]
fn collation_ignores_case_and_accents_but_stays_stable() {
    assert_eq!(collate("abc", "ABD"), Ordering::Less);
    assert_eq!(collate("Émile", "emma"), Ordering::Less);
    assert_eq!(collate("a", "A"), Ordering::Greater);
    assert_eq!(collate("x", "x"), Ordering::Equal);
}

#[test]
fn subtitles_are_parsed_like_the_javascript_version() {
    let srt = "1\r\n00:00:01,000 --> 00:00:03,500\r\n<i>Hello</i>\r\n{\\an8}there\r\n\r\n2\r\n01:02:03.5 --> 01:02:04.25\r\nSecond\r\n\r\n\r\n3\r\nno arrow\r\nhere\r\n\r\n4\r\n00:00:05,000 --> 00:00:06,000\r\n<b></b>\r\n";
    let cues = parse_srt(srt);
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0], Cue { start: 1000., end: 3500., text: "Hello\nthere".into() });
    assert_eq!(cues[1].start, 3_723_500.);
    assert_eq!(cues[1].end, 3_724_250.);
    assert_eq!(cues[1].text, "Second");
    // the cue of block 4 is empty once its markup is gone: it is dropped
    assert!(parse_srt("").is_empty());
    // a timing with no text line of its own and an index before it
    let one = parse_srt("7\n00:00:00,5 --> 00:00:01,0\nA < B > C\nx < y\n");
    // `<[^>]+>` takes "< B >" out, a lone "<" stays
    assert_eq!(one[0].text, "A  C\nx < y");
    assert_eq!((one[0].start, one[0].end), (500., 1000.));
    // a bad time becomes zero
    assert_eq!(parse_srt("x --> y\ntext\n")[0].start, 0.);
}

#[test]
fn the_cue_shown_is_every_cue_that_covers_the_position() {
    let cues = vec![
        Cue { start: 0., end: 1000., text: "a".into() },
        Cue { start: 500., end: 1500., text: "b".into() },
        Cue { start: 1500., end: 2000., text: "c".into() },
    ];
    assert_eq!(cue_at(&cues, 700.), "a\nb");
    assert_eq!(cue_at(&cues, 1000.), "b");
    assert_eq!(cue_at(&cues, 1500.), "c");
    assert_eq!(cue_at(&cues, 2000.), "");
}

fn queue(len: usize, index: i64, shuffle: bool, repeat: Repeat) -> Queue {
    Queue { paths: (0..len).map(|i| format!("/q/{i}.mp3")).collect(), index, shuffle, repeat }
}

#[test]
fn next_follows_the_queue_repeat_and_shuffle_rules() {
    assert_eq!(queue(3, 0, false, Repeat::Off).next(true, |_| 0), Advance::Go(1));
    // the end of the queue: it stops by itself, a click goes back to the first
    assert_eq!(queue(3, 2, false, Repeat::Off).next(true, |_| 0), Advance::Stay);
    assert_eq!(queue(3, 2, false, Repeat::Off).next(false, |_| 0), Advance::Go(0));
    assert_eq!(queue(3, 2, false, Repeat::All).next(true, |_| 0), Advance::Go(0));
    // repeat one only repeats on its own, a click moves on
    assert_eq!(queue(3, 1, false, Repeat::One).next(true, |_| 0), Advance::Restart);
    assert_eq!(queue(3, 1, false, Repeat::One).next(false, |_| 0), Advance::Go(2));
    assert_eq!(queue(0, -1, false, Repeat::All).next(true, |_| 0), Advance::Stay);
    assert_eq!(queue(3, -1, false, Repeat::Off).next(false, |_| 0), Advance::Go(0));
    // shuffle picks anywhere, even the same track, and never past the end
    assert_eq!(queue(5, 0, true, Repeat::Off).next(true, |n| { assert_eq!(n, 5); 3 }), Advance::Go(3));
    assert_eq!(queue(5, 0, true, Repeat::Off).next(true, |n| n + 10), Advance::Go(4));
}

#[test]
fn previous_restarts_after_three_seconds_and_never_goes_below_the_first() {
    let q = queue(3, 2, false, Repeat::Off);
    assert_eq!(q.prev(3001.), Advance::Restart);
    assert_eq!(q.prev(3000.), Advance::Go(1));
    assert_eq!(queue(3, 0, false, Repeat::Off).prev(0.), Advance::Go(0));
    assert_eq!(queue(3, -1, false, Repeat::Off).prev(0.), Advance::Go(0));
}

#[test]
fn playing_a_list_sets_the_queue_and_the_current_track() {
    let mut q = Queue::default();
    assert_eq!((q.index, q.repeat, q.shuffle), (-1, Repeat::Off, false));
    assert!(q.current().is_none());
    q.play_list(vec!["/a.mp3".into(), "/b.mp3".into()], 1);
    assert_eq!(q.current(), Some("/b.mp3"));
    q.play_list(vec!["/a.mp3".into()], 4);
    assert_eq!((q.index, q.current()), (-1, None));
    assert_eq!(Repeat::Off.cycle().cycle().cycle(), Repeat::Off);
    assert_eq!(Repeat::parse("one"), Repeat::One);
    assert_eq!(Repeat::parse("junk"), Repeat::Off);
    assert_eq!(Repeat::All.name(), "all");
}

#[test]
fn the_player_state_follows_the_helper_events() {
    let mut s = PlayerState::default();
    assert_eq!((s.state.as_str(), s.playing, s.pos), ("loading", false, 0.));
    s.reduce("ready", &json!({"duration":90000,"video":0,"streams":[
        {"type":"video","width":1920,"height":1080},{"type":"audio","index":1,"channels":2},
        {"type":"subtitle","index":2,"text":true,"lang":"por"},{"type":"subtitle","index":3,"text":false}]}));
    assert_eq!(s.state, "ready");
    assert!(s.has_video());
    assert_eq!(s.audio_streams().len(), 1);
    assert_eq!(s.subtitle_streams().len(), 1);
    assert!((s.video_ratio() - 0.5625).abs() < 1e-9);
    s.reduce("info", &json!({"duration":120000,"video":-1}));
    assert_eq!(s.duration, 120000.);
    assert!(!s.has_video());
    s.reduce("pos", &json!({"pos":4000,"duration":0,"playing":true}));
    assert_eq!((s.pos, s.duration, s.playing), (4000., 120000., true));
    s.reduce("state", &json!({"state":"paused"}));
    assert!(!s.playing);
    s.reduce("state", &json!({"state":"ended"}));
    assert_eq!(s.state, "ended");
    s.reduce("error", &json!({"message":"media.bad"}));
    assert_eq!(s.error.as_deref(), Some("media.bad"));
    s.reduce("closed", &json!({"crashed":true}));
    assert!(s.closed);
    assert_eq!(s.error.as_deref(), Some("crash"));
    s.reduce("done", &json!({}));
    assert_eq!(s.state, "done");
    s.reduce("level", &json!({"level":1}));
    assert_eq!(s.state, "done");
    // no picture size known: 9 over 16
    assert!((PlayerState::default().video_ratio() - 0.5625).abs() < 1e-9);
}

#[test]
fn live_subtitles_forget_what_is_over_a_minute_old_and_keep_two_hundred() {
    let mut s = PlayerState::default();
    s.reduce("sub", &json!({"start":0,"end":1000,"text":"old"}));
    s.reduce("sub", &json!({"start":90000,"end":91000,"text":"new"}));
    assert_eq!(s.subs.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["new"]);
    let mut s = PlayerState::default();
    for i in 0..300 {
        s.reduce("sub", &json!({"start":i,"end":i + 10,"text":i.to_string()}));
    }
    assert_eq!(s.subs.len(), 201);
    assert_eq!(s.subs.last().unwrap().text, "299");
    assert_eq!(s.subs[0].text, "99");
}

#[test]
fn track_labels_prefer_title_and_language() {
    assert_eq!(track_label(&json!({"title":"Director","lang":"eng"})).as_deref(), Some("Director · eng"));
    assert_eq!(track_label(&json!({"lang":"por"})).as_deref(), Some("por"));
    assert_eq!(track_label(&json!({"lang":"und"})), None);
    assert_eq!(track_label(&json!({})), None);
}

#[test]
fn volume_seek_and_bar_arithmetic() {
    assert_eq!(volume_from_fraction(0.5), 0.75);
    assert_eq!(volume_from_fraction(1.), 1.5);
    assert_eq!(volume_from_fraction(2.), 1.5);
    assert_eq!(volume_from_fraction(-1.), 0.);
    assert!((volume_step(1., true) - 1.1).abs() < 1e-9);
    assert_eq!(volume_step(1.45, true), 1.5);
    assert_eq!(volume_step(0.05, false), 0.);
    assert_eq!(seek_by(10_000., true, false), 15_000.);
    assert_eq!(seek_by(10_000., true, true), 40_000.);
    assert_eq!(seek_by(3_000., false, false), 0.);
    assert_eq!(seek_by(50_000., false, true), 20_000.);
    assert_eq!(seek_from_fraction(0.25, 1001.), 250.);
    assert_eq!(seek_from_fraction(3., 1000.), 1000.);
    assert_eq!(bar_fraction(150., 100., 200.), 0.25);
    assert_eq!(bar_fraction(0., 100., 200.), 0.);
    assert_eq!(bar_fraction(900., 100., 200.), 1.);
    assert_eq!(bar_fraction(150., 100., 0.), 1.);
    assert_eq!(bar_fraction(f64::NAN, 0., 10.), 0.);
}

#[test]
fn the_video_controls_hide_only_while_a_video_plays_and_the_pointer_rests() {
    assert!(controls_visible(true, true, false, 1000, 3800));
    assert!(!controls_visible(true, true, false, 1000, 3801));
    assert!(controls_visible(true, false, false, 0, 99_999));
    assert!(controls_visible(true, true, true, 0, 99_999));
    assert!(controls_visible(false, true, false, 0, 99_999));
}

#[test]
fn the_picture_in_picture_sits_at_the_bottom_right() {
    let f = pip_frame(1920., 1080., 0.5625);
    assert_eq!((f["x"].as_f64(), f["y"].as_f64(), f["w"].as_f64(), f["h"].as_f64()), (Some(1496.), Some(759.), Some(400.), Some(225.)));
}

#[test]
fn now_playing_prefers_the_library_then_the_file_tags_then_the_name() {
    let mut s = PlayerState::default();
    s.reduce("info", &json!({"tags":{"title":"Tagged","artist":"TA","album":"TAL"}}));
    let t = track("/m/f.mp3", "Library", "", "LAL", "", 1);
    let n = now_playing("player#2", Some(&t), "/m/f.mp3", &s, Some("/c.png"));
    assert_eq!((n["title"].as_str(), n["artist"].as_str(), n["album"].as_str()), (Some("Library"), Some("TA"), Some("LAL")));
    assert_eq!((n["app"].as_str(), n["window"].as_str(), n["cover"].as_str()), (Some("player"), Some("player#2"), Some("/c.png")));
    let bare = now_playing("player", None, "/m/f.mp3", &PlayerState::default(), None);
    assert_eq!((bare["title"].as_str(), bare["artist"].as_str()), (Some("f.mp3"), Some("")));
    assert!(bare["cover"].is_null());
}

#[test]
fn videos_are_listed_by_file_name() {
    let v = video_from(&found("/v/My Film.mkv", 5, 6.));
    assert_eq!((v.title.as_str(), v.size, v.mtime), ("My Film", 5, 6.));
}
