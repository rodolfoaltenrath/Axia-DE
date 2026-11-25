//! The Player's widget trees: the library window and the video view.
//!
//! The two bars are hit-tested by arithmetic (`geometry`), so the layout below keeps their boxes
//! at sizes that follow only from the window width, the text direction and the buttons shown.
use super::*;

const SIDEBAR: f64 = 200.;
const BOTTOM: f64 = 72.;
const CENTER: f64 = 380.;
const BAR_LABEL: f64 = 40.;
const VOLUME_BAR: f64 = 84.;
/// Speaker glyph (13 + 6) and the gap after it.
const SPEAKER: f64 = 19. + 6.;
/// A transport button and the gap that follows it.
const BUTTON: f64 = 30.;
const GAP: f64 = 6.;

pub(super) struct Geometry {
    /// Left edge and width of the time bar, in view coordinates (the physical left, also in RTL).
    pub time: (f64, f64),
    pub volume: Option<(f64, f64)>,
}
/// Buttons next to the volume in the video overlay: subtitles, picture in picture, full screen,
/// and the audio track when the file has more than one.
fn video_buttons(app: &App) -> usize {
    3 + usize::from(app.state.audio_streams().len() > 1)
}
pub(super) fn geometry(app: &App) -> Geometry {
    let (w, rtl) = (app.width, app.rtl);
    if app.video_view() {
        let group = SPEAKER + VOLUME_BAR + (BUTTON + GAP) * video_buttons(app) as f64;
        let left = if rtl { 20. + SPEAKER } else { w - 20. - group + SPEAKER };
        Geometry { time: (20. + BAR_LABEL + 8., w - 40. - 2. * (BAR_LABEL + 8.)), volume: Some((left, VOLUME_BAR)) }
    } else if app.narrow() {
        let left = if rtl { 16. } else { 16. + 48. + 16. };
        Geometry { time: (left + BAR_LABEL + 8., w - 96. - 2. * (BAR_LABEL + 8.)), volume: None }
    } else {
        let left = w / 2. - CENTER / 2.;
        let group = SPEAKER + VOLUME_BAR;
        let volume = if rtl { 16. + SPEAKER } else { w - 16. - group + SPEAKER };
        Geometry { time: (left + BAR_LABEL + 8., CENTER - 2. * (BAR_LABEL + 8.)), volume: Some((volume, VOLUME_BAR)) }
    }
}

fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value, size: f64, color: &Value) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"color":color}})
}
fn clipped(mut node: Value) -> Value {
    node["style"]["whiteSpace"] = json!("nowrap");
    node["style"]["overflow"] = json!("hidden");
    node["style"]["textOverflow"] = json!("ellipsis");
    node
}
fn on(mut node: Value, event: &str, cmd: &str, arg: Value) -> Value {
    node["events"][event] = json!([cmd, arg]);
    node
}
fn click(node: Value, cmd: &str, arg: Value) -> Value {
    on(node, "onClick", cmd, arg)
}
fn fill() -> Value {
    json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0})
}

/// The app icon: a screen with film perforations and a play triangle. It is drawn here, not
/// through `axia_native::icons::app_icon`, which asks the registry (this app) first.
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() { size.clamp(1., 512.) } else { 32. };
    let rect = |id: String, x: f64, y: f64, w: f64, h: f64, r: f64, color: &str| {
        v(&id, json!({"position":"absolute","left":s*x,"top":s*y,"width":s*w,"height":s*h,"borderRadius":s*r,"backgroundColor":color}), vec![])
    };
    let mut kids = vec![rect("screen".into(), 0.14, 0.22, 0.72, 0.56, 0.08, "rgba(20,10,30,0.82)")];
    for (row, y) in [0.25, 0.7].into_iter().enumerate() {
        for i in 0..4 {
            kids.push(rect(format!("hole{row}{i}"), 0.2 + i as f64 * 0.17, y, 0.08, 0.05, 0.015, "rgba(255,255,255,0.35)"));
        }
    }
    kids.extend(tri("play", s * 0.42, s * 0.36, s * 0.24, s * 0.28, &json!("#fff"), ((s / 5.).round() as usize).max(6), false));
    v(
        "player-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #ff7a59, #d6246e)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        kids,
    )
}

// ------------------------------------------------------------------ glyphs
// Monochrome transport symbols drawn with boxes: the media symbols of the fonts are color
// emoji. They always point left to right (time runs that way, also in RTL).

/// A triangle made of bars (boxes are not rotated).
fn tri(id: &str, x: f64, y: f64, w: f64, h: f64, color: &Value, steps: usize, left: bool) -> Vec<Value> {
    (0..steps)
        .map(|i| {
            let f = 1. - ((i as f64 + 0.5) / steps as f64 - 0.5).abs() * 2.;
            let bw = (w * f).max(1.);
            v(
                &format!("{id}{i}"),
                json!({"position":"absolute","left":if left{x+w-bw}else{x},"top":y+h*i as f64/steps as f64,"width":bw,"height":h/steps as f64+0.6,"backgroundColor":color}),
                vec![],
            )
        })
        .collect()
}
fn steps_for(size: f64) -> usize {
    (size.round() as usize).max(8)
}
fn play_glyph(size: f64, color: &Value) -> Value {
    v("play", json!({"width":size,"height":size,"direction":"ltr"}), tri("p", size * 0.2, 0., size * 0.72, size, color, steps_for(size), false))
}
fn pause_glyph(size: f64, color: &Value) -> Value {
    let bw = (size * 0.28).round().max(2.);
    let bar = |id: &str| v(id, json!({"width":bw,"height":size,"borderRadius":1,"backgroundColor":color}), vec![]);
    v(
        "pause",
        json!({"width":size,"height":size,"flexDirection":"row","justifyContent":"center","gap":(size*0.2).round(),"direction":"ltr"}),
        vec![bar("a"), bar("b")],
    )
}
fn skip_glyph(size: f64, color: &Value, back: bool) -> Value {
    let (h, w) = (size * 0.7, size * 0.42);
    let y = (size - h) / 2.;
    let steps = steps_for(h);
    let mut kids = vec![];
    let edge = |x: f64| v("edge", json!({"position":"absolute","left":x,"top":y,"width":2,"height":h,"backgroundColor":color}), vec![]);
    if back {
        kids.push(edge(0.));
        kids.extend(tri("a", 2., y, w, h, color, steps, true));
        kids.extend(tri("b", 2. + w, y, w, h, color, steps, true));
    } else {
        kids.extend(tri("a", size - 2. - w * 2., y, w, h, color, steps, false));
        kids.extend(tri("b", size - 2. - w, y, w, h, color, steps, false));
        kids.push(edge(size - 2.));
    }
    v(if back { "skip-back" } else { "skip" }, json!({"width":size,"height":size,"direction":"ltr"}), kids)
}
fn speaker_glyph(size: f64, color: &Value, level: f64) -> Value {
    let (h, y) = (size * 0.75, size * 0.125);
    let mut kids = vec![v("cone", json!({"position":"absolute","left":0,"top":size*0.36,"width":size*0.22,"height":size*0.28,"backgroundColor":color}), vec![])];
    kids.extend(tri("s", 0., y, size * 0.5, h, color, steps_for(h), true));
    for (i, (shown, top, height)) in [(level > 0., 0.36, 0.28), (level > 0.5, 0.22, 0.56), (level > 1., 0.08, 0.84)].into_iter().enumerate() {
        if shown {
            kids.push(v(&format!("wave{i}"), json!({"position":"absolute","left":size*0.62+4.*i as f64,"top":size*top,"width":2,"height":size*height,"borderRadius":1,"backgroundColor":color}), vec![]));
        }
    }
    v("speaker", json!({"width":size+6.,"height":size,"direction":"ltr"}), kids)
}
fn captions_glyph(color: &Value) -> Value {
    v(
        "cc",
        json!({"height":14,"paddingHorizontal":3,"borderRadius":3,"borderWidth":1.5,"borderColor":color,"justifyContent":"center"}),
        vec![json!({"kind":"text","label":"CC","style":{"fontSize":8,"fontWeight":700,"color":color,"lineHeight":10}})],
    )
}
fn repeat_glyph(color: &Value, one: bool) -> Value {
    let mut kids = vec![text(json!("↻"), 16., color)];
    if one {
        kids.push(json!({"kind":"text","label":"1","style":{"position":"absolute","right":-2,"bottom":-2,"fontSize":8,"fontWeight":700,"color":color}}));
    }
    v("repeat", json!({"width":18,"height":18,"alignItems":"center","justifyContent":"center"}), kids)
}
/// A round borderless button of a fixed width around a glyph.
fn icon_button(id: &str, child: Value, cmd: &str, arg: Value, size: f64) -> Value {
    click(
        v(id, json!({"width":size,"height":size,"borderRadius":size/2.,"alignItems":"center","justifyContent":"center"}), vec![child]),
        cmd,
        arg,
    )
}
fn text_button(id: &str, glyph: &str, font: f64, color: &Value, cmd: &str, arg: Value) -> Value {
    icon_button(id, text(json!(glyph), font, color), cmd, arg, BUTTON)
}
/// A bar with a filled part, pressed to start a drag.
fn bar(id: &str, fraction: f64, width: f64, height: f64, color: &Value, track: &Value, cmd: &str) -> Value {
    let width = width.max(0.);
    on(
        v(
            id,
            json!({"width":width,"height":16,"justifyContent":"center","direction":"ltr"}),
            vec![v(
                &format!("{id}-track"),
                json!({"width":width,"height":height,"borderRadius":height/2.,"backgroundColor":track}),
                vec![v(&format!("{id}-fill"), json!({"width":width*fraction.clamp(0.,1.),"height":height,"borderRadius":height/2.,"backgroundColor":color}), vec![])],
            )],
        ),
        "onMouseDown",
        cmd,
        Value::Null,
    )
}

// ------------------------------------------------------------------ transport

fn buttons(app: &App, t: &Value, color: &Value, video: bool) -> Value {
    let mut row = vec![];
    if !video {
        let c = if app.queue.shuffle { &t["accent"] } else { color };
        row.push(text_button("shuffle", "⤮", 15., c, "shuffle", Value::Null));
    }
    row.push(icon_button("prev", skip_glyph(14., color, true), "prev", Value::Null, BUTTON));
    let main = if app.state.playing { pause_glyph(16., color) } else { play_glyph(16., color) };
    row.push(icon_button("toggle", main, "toggle", Value::Null, 36.));
    row.push(icon_button("skip", skip_glyph(14., color, false), "skip", Value::Null, BUTTON));
    if !video {
        let c = if app.queue.repeat != Repeat::Off { &t["accent"] } else { color };
        row.push(icon_button("repeat", repeat_glyph(c, app.queue.repeat == Repeat::One), "repeat", Value::Null, BUTTON));
    }
    v("buttons", json!({"flexDirection":"row","alignItems":"center","justifyContent":"center","gap":GAP,"direction":"ltr"}), row)
}
fn progress(app: &App, t: &Value, dark: bool) -> Value {
    let geo = geometry(app);
    let pos = app.seek_preview.unwrap_or(app.state.pos);
    let dur = app.state.duration;
    let dim = if dark { json!("rgba(255,255,255,0.7)") } else { t["textDim"].clone() };
    let (color, track) = if dark { (json!("#fff"), json!("rgba(255,255,255,0.25)")) } else { (t["accent"].clone(), t["control"].clone()) };
    let mut left = text(json!(format_time(pos)), 11., &dim);
    left["style"]["width"] = json!(BAR_LABEL);
    left["style"]["textAlign"] = json!("end");
    let mut right = text(json!(format_time(dur)), 11., &dim);
    right["style"]["width"] = json!(BAR_LABEL);
    v(
        "progress",
        json!({"flexDirection":"row","alignItems":"center","gap":8,"direction":"ltr"}),
        vec![left, bar("time", if dur > 0. { pos / dur } else { 0. }, geo.time.1, 4., &color, &track, "timeDown"), right],
    )
}
fn volume_control(app: &App, t: &Value, dark: bool, color: &Value, video: bool) -> Value {
    let dim = if dark { json!("#fff") } else { t["textDim"].clone() };
    let track = if dark { json!("rgba(255,255,255,0.25)") } else { t["control"].clone() };
    let volume = app.volume();
    let mut row = vec![
        speaker_glyph(13., &dim, volume),
        bar("volume", volume / VOLUME_MAX, VOLUME_BAR, 4., &dim, &track, "volDown"),
    ];
    if video {
        let subs_on = app.sub_stream.is_some() || app.ext_subs.is_some();
        let cc = captions_glyph(if subs_on { &t["accent"] } else { color });
        row.push(icon_button("subs", cc, "menu", json!("subs"), BUTTON));
        if app.state.audio_streams().len() > 1 {
            row.push(text_button("audio", "♫", 16., color, "menu", json!("audio")));
        }
        let pip = if app.window_mode == "pip" { &t["accent"] } else { color };
        row.push(text_button("pip", "⧉", 16., pip, "pip", Value::Null));
        row.push(text_button("fullscreen", if app.window_mode == "fullscreen" { "⤡" } else { "⤢" }, 16., color, "fullscreen", Value::Null));
    }
    v("volume-control", json!({"flexDirection":"row","alignItems":"center","gap":GAP,"direction":"ltr"}), row)
}
/// The overlay of a video: the progress bar on top, the controls and the volume below.
fn transport(app: &App, t: &Value) -> Value {
    let color = json!("#fff");
    v(
        "transport",
        json!({"gap":6}),
        vec![
            progress(app, t, true),
            v(
                "transport-row",
                json!({"flexDirection":"row","alignItems":"center","direction":"ltr"}),
                vec![
                    v("left-fill", json!({"flex":1}), vec![]),
                    buttons(app, t, &color, true),
                    v("right-fill", json!({"flex":1,"alignItems":"flex-end","direction":if app.rtl{"rtl"}else{"ltr"}}), vec![volume_control(app, t, true, &color, true)]),
                ],
            ),
        ],
    )
}
fn popup(t: &Value, items: Vec<Value>) -> Value {
    on(
        v(
            "popup",
            json!({"position":"absolute","right":12,"bottom":64,"minWidth":200,"padding":6,"borderRadius":10,"gap":2,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 12px 30px rgba(0,0,0,0.45)"}),
            items,
        ),
        "onMouseDown",
        "noop",
        Value::Null,
    )
}
fn popup_item(t: &Value, id: &str, label: Value, checked: bool, cmd: &str, arg: Value) -> Value {
    click(
        v(
            id,
            json!({"paddingHorizontal":8,"height":26,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":6,"backgroundColor":if checked{t["hover"].clone()}else{json!("transparent")}}),
            vec![
                json!({"kind":"text","label":if checked{"✓"}else{""},"style":{"width":12,"fontSize":11,"color":t["text"]}}),
                json!({"kind":"text","label":label,"style":{"fontSize":13,"color":t["text"],"whiteSpace":"nowrap"}}),
            ],
        ),
        cmd,
        arg,
    )
}
fn stream_label(stream: &Value, i: usize) -> Value {
    match track_label(stream) {
        Some(label) => json!(label),
        None => json!({"key":"player.track","params":{"n":i + 1}}),
    }
}
fn menu(app: &App, t: &Value) -> Option<Value> {
    match app.menu {
        Menu::Closed => None,
        Menu::Subs => {
            let mut items = vec![popup_item(t, "off", key("player.subs.off"), app.sub_stream.is_none() && app.ext_subs.is_none(), "subOff", Value::Null)];
            if app.ext_subs.is_some() {
                items.push(popup_item(t, "file", key("player.subs.file"), app.sub_stream.is_none(), "subFile", Value::Null));
            }
            for (i, s) in app.state.subtitle_streams().into_iter().enumerate() {
                let index = s["index"].as_i64().unwrap_or(-1);
                items.push(popup_item(t, &format!("sub{index}"), stream_label(s, i), app.sub_stream == Some(index), "subStream", json!(index)));
            }
            Some(popup(t, items))
        }
        Menu::Audio => {
            let items = app
                .state
                .audio_streams()
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    let index = s["index"].as_i64().unwrap_or(-1);
                    let detail = format!(" ({}, {} ch)", str_value(&s["codec"]), s["channels"].as_i64().unwrap_or(0));
                    let label = json!({"parts":[stream_label(s, i), detail]});
                    popup_item(t, &format!("audio{index}"), label, app.state.info["audio"].as_i64() == Some(index), "audioStream", json!(index))
                })
                .collect();
            Some(popup(t, items))
        }
    }
}

// ------------------------------------------------------------------ video

fn video(app: &App, t: &Value, now: i64) -> Value {
    let small = app.window_mode == "pip";
    let controls = !small && controls_visible(true, app.state.playing, app.menu != Menu::Closed, app.last_move, now);
    let mut kids = vec![json!({"kind":"media","key":"video","media":app.id,"style":fill()})];
    if let Some(error) = &app.state.error {
        let label = if error == "crash" { key("player.crashed") } else { json!({"key":"player.error","params":{"error":{"error":error}}}) };
        kids.push(v("error", json!({"position":"absolute","left":0,"right":0,"top":"45%","alignItems":"center"}), vec![text(label, 13., &json!("#ff9f9f"))]));
    }
    let cues: &[Cue] = if app.sub_stream.is_some() { &app.state.subs } else { app.ext_subs.as_deref().unwrap_or(&[]) };
    let cue = cue_at(cues, app.state.pos);
    if !cue.is_empty() {
        let mut line = text(json!(cue), if small { 12. } else { 20. }, &json!("#fff"));
        line["style"]["textAlign"] = json!("center");
        line["style"]["lineHeight"] = json!(1.3);
        kids.push(v(
            "cue",
            json!({"position":"absolute","left":24,"right":24,"bottom":if controls&&!small{96}else{28},"alignItems":"center"}),
            vec![v("cue-box", json!({"paddingHorizontal":10,"paddingVertical":4,"borderRadius":6,"backgroundColor":"rgba(0,0,0,0.55)"}), vec![line])],
        ));
    }
    let white = json!("#fff");
    if small {
        let main = if app.state.playing { pause_glyph(13., &white) } else { play_glyph(13., &white) };
        kids.push(v(
            "pip-buttons",
            json!({"position":"absolute","right":6,"top":6,"flexDirection":"row","gap":2}),
            vec![icon_button("toggle", main, "toggle", Value::Null, BUTTON), text_button("pip", "⧉", 16., &white, "pip", Value::Null)],
        ));
    } else if controls {
        kids.push(v(
            "overlay",
            json!({"position":"absolute","left":0,"right":0,"bottom":0,"paddingHorizontal":20,"paddingTop":28,"paddingBottom":12,"background":"linear-gradient(180deg, rgba(0,0,0,0), rgba(0,0,0,0.75))"}),
            vec![transport(app, t)],
        ));
        if app.mode_is_window() {
            kids.push(v(
                "back",
                json!({"position":"absolute","left":12,"top":12}),
                vec![text_button("hide", if app.rtl { "›" } else { "‹" }, 20., &white, "hideVideo", Value::Null)],
            ));
        }
    }
    kids.extend(menu(app, t));
    let mut root = v("video-view", json!({"flex":1,"backgroundColor":"#000","direction":if app.rtl{"rtl"}else{"ltr"}}), kids);
    root["events"]["onMouseMove"] = json!(["poke"]);
    root["events"]["onMouseDown"] = json!(["dismiss"]);
    root["dropAction"] = json!(["drop"]);
    root
}

// ---------------------------------------------------------------- library

fn cover(app: &App, t: &Value, path: &str, size: f64, radius: f64, thumb: u32) -> Value {
    let inner = match app.thumb(path, thumb) {
        Some(src) => json!({"kind":"image","src":src,"style":{"width":size,"height":size,"objectFit":"cover"}}),
        None => text(json!("♪"), size * 0.42, &t["textDim"]),
    };
    v(
        &format!("cover:{path}"),
        json!({"width":size,"height":size,"borderRadius":radius,"overflow":"hidden","backgroundColor":t["control"],"alignItems":"center","justifyContent":"center","flexShrink":0}),
        vec![inner],
    )
}
fn side_item(t: &Value, id: &str, label: Value, icon: &str, selected: bool, cmd: &str, arg: Value) -> Value {
    let color = if selected { json!("#fff") } else { t["text"].clone() };
    click(
        v(
            id,
            json!({"flexDirection":"row","alignItems":"center","gap":8,"height":28,"paddingHorizontal":10,"borderRadius":6,"backgroundColor":if selected{t["accent"].clone()}else{json!("transparent")}}),
            vec![text(json!(icon), 13., &color), clipped(text(label, 13., &color))],
        ),
        cmd,
        arg,
    )
}
fn section(t: &Value, id: &str, label: &str) -> Value {
    v(id, json!({"paddingHorizontal":10,"paddingTop":12,"paddingBottom":4}), vec![text(json!({"upper":key(label)}), 11., &t["textDim"])])
}
fn sidebar(app: &App, t: &Value) -> Value {
    let mut kids = vec![section(t, "s-library", "player.library")];
    for (view, icon) in [("songs", "♪"), ("artists", "☺"), ("albums", "◫"), ("videos", "▶")] {
        kids.push(side_item(t, view, key(&format!("player.nav.{view}")), icon, app.view.name() == view, "nav", json!(view)));
    }
    kids.push(section(t, "s-playing", "player.playing"));
    kids.push(side_item(t, "queue", key("player.nav.queue"), "≡", app.view == View::Queue, "nav", json!("queue")));
    if app.queue.current().is_some_and(is_video) {
        kids.push(side_item(t, "video", key("player.nav.video"), "▣", false, "showVideo", Value::Null));
    }
    kids.push(v("s-fill", json!({"flex":1}), vec![]));
    if app.scanning {
        kids.push(text(
            json!({"key":"player.scanning","params":{"done":app.probe_done,"total":app.probe_total}}),
            11.,
            &t["textDim"],
        ));
    }
    v(
        "sidebar",
        json!({"width":SIDEBAR,"flexShrink":0,"backgroundColor":t["sidebarBg"],"paddingHorizontal":10,"paddingTop":4,"paddingBottom":10,"gap":2,"borderEndWidth":1,"borderEndColor":t["separator"]}),
        kids,
    )
}
/// What a row shows for a path: the library's tags, or the file name for a queue entry the
/// library does not know.
fn line_of(app: &App, path: &str) -> (String, String, String, f64) {
    match app.track_of(path) {
        Some(t) => (t.title.clone(), t.artist.clone(), t.album.clone(), t.duration),
        None => (base_name(path).to_owned(), String::new(), String::new(), 0.),
    }
}
fn column(t: &Value, flex: f64, label: Value, size: f64, color: &Value, weight: u32) -> Value {
    let mut node = clipped(text(label, size, color));
    node["style"]["flex"] = json!(flex);
    node["style"]["minWidth"] = json!(0);
    node["style"]["fontWeight"] = json!(weight);
    let _ = t;
    node
}
fn table_header(app: &App, t: &Value) -> Value {
    let (show_artist, show_album) = columns(app);
    let dim = &t["textDim"];
    let mut number = text(json!("#"), 11., dim);
    number["style"]["width"] = json!(22);
    number["style"]["textAlign"] = json!("end");
    number["style"]["fontWeight"] = json!(600);
    let mut kids = vec![number, column(t, 2., key("player.col.title"), 11., dim, 600)];
    if show_artist {
        kids.push(column(t, 1., key("player.col.artist"), 11., dim, 600));
    }
    if show_album {
        kids.push(column(t, 1., key("player.col.album"), 11., dim, 600));
    }
    let mut time = text(key("player.col.time"), 11., dim);
    time["style"]["width"] = json!(56);
    time["style"]["textAlign"] = json!("end");
    time["style"]["fontWeight"] = json!(600);
    kids.push(time);
    v(
        "table-header",
        json!({"height":26,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":10,"borderBottomWidth":1,"borderBottomColor":t["separator"]}),
        kids,
    )
}
fn columns(app: &App) -> (bool, bool) {
    let content = app.width - SIDEBAR;
    (content >= 380., content >= 560.)
}
fn row(app: &App, t: &Value, i: usize, path: &str, number: Option<i64>) -> Value {
    let (title, artist, album, duration) = line_of(app, path);
    let (show_artist, show_album) = columns(app);
    let playing = app.queue.current() == Some(path);
    let color = if playing { &t["accent"] } else { &t["text"] };
    let stripe = if i % 2 == 1 {
        json!(if t["mode"] == "dark" { "rgba(255,255,255,0.025)" } else { "rgba(0,0,0,0.025)" })
    } else {
        json!("transparent")
    };
    let mut num = text(json!(if playing { "♪".to_owned() } else { number.unwrap_or(i as i64 + 1).to_string() }), 11., color);
    num["style"]["width"] = json!(22);
    num["style"]["textAlign"] = json!("end");
    let mut name = column(t, 2., json!(title), 13., color, if playing { 500 } else { 400 });
    name["style"]["fontWeight"] = json!(if playing { 500 } else { 400 });
    let dim = &t["textDim"];
    let mut kids = vec![num, name];
    if show_artist {
        kids.push(column(t, 1., json!(artist), 12., dim, 400));
    }
    if show_album {
        kids.push(column(t, 1., json!(album), 12., dim, 400));
    }
    let mut time = text(json!(if duration > 0. { format_time(duration) } else { String::new() }), 11., dim);
    time["style"]["width"] = json!(56);
    time["style"]["textAlign"] = json!("end");
    time["style"]["direction"] = json!("ltr");
    kids.push(time);
    click(
        v(
            &format!("row{i}:{path}"),
            json!({"height":ROW,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":10,"borderRadius":6,"backgroundColor":if playing{t["hover"].clone()}else{stripe}}),
            kids,
        ),
        "row",
        json!(i),
    )
}
/// Songs and the queue: only the rows in view are built.
fn virtual_list(app: &App, t: &Value, paths: Vec<&str>) -> Value {
    let height = app.list_height();
    let first = ((app.offset / ROW).floor() as usize).saturating_sub(2).min(paths.len());
    let last = (((app.offset + height) / ROW).ceil() as usize + 2).min(paths.len());
    let rows = (first..last).map(|i| row(app, t, i, paths[i], None)).collect();
    let mut children = vec![v("rows", json!({"position":"absolute","left":0,"right":0,"top":first as f64*ROW-app.offset}), rows)];
    let total = paths.len() as f64 * ROW;
    if total > height && height > 0. {
        let thumb = (height * height / total).max(28.).min(height);
        let top = (height - thumb) * app.offset / app.max_offset().max(1.);
        let shade = if t["mode"] == "dark" { "rgba(255,255,255,0.3)" } else { "rgba(0,0,0,0.28)" };
        children.push(v("thumb", json!({"position":"absolute","right":2,"top":top,"width":7,"height":thumb,"borderRadius":4,"backgroundColor":shade}), vec![]));
    }
    let mut list = v("list", json!({"flex":1,"overflow":"hidden"}), children);
    list["events"]["onWheel"] = json!(["scroll"]);
    v("songs", json!({"flex":1,"paddingHorizontal":10}), vec![table_header(app, t), list])
}
fn album_card(app: &App, t: &Value, a: &Album) -> Value {
    click(
        v(
            &format!("album:{}", a.key),
            json!({"width":150,"gap":2}),
            vec![
                cover(app, t, &a.tracks[0].path, 150., 8., 256),
                {
                    let mut name = clipped(text(if a.album.is_empty() { key("player.unknownAlbum") } else { json!(a.album) }, 13., &t["text"]));
                    name["style"]["fontWeight"] = json!(500);
                    name["style"]["marginTop"] = json!(6);
                    name
                },
                clipped(text(if a.artist.is_empty() { key("player.unknownArtist") } else { json!(a.artist) }, 12., &t["textDim"])),
            ],
        ),
        "album",
        json!(a.key),
    )
}
fn grid(id: &str, cards: Vec<Value>) -> Value {
    json!({"kind":"scroll","key":id,"style":{"flex":1,"paddingHorizontal":10},"children":[
        v("grid", json!({"flexDirection":"row","flexWrap":"wrap","gap":20,"paddingHorizontal":10,"paddingBottom":16}), cards)]})
}
fn album_page(app: &App, t: &Value, a: &Album) -> Value {
    let mut parts = vec![];
    if !a.year.is_empty() {
        parts.push(json!(format!("{} · ", a.year)));
    }
    parts.push(json!({"key":"player.songs","params":{"count":a.tracks.len()}}));
    let subtitle = json!({"parts":parts});
    let mut title = text(if a.album.is_empty() { key("player.unknownAlbum") } else { json!(a.album) }, 22., &t["text"]);
    title["style"]["fontWeight"] = json!(600);
    let head = v(
        "album-head",
        json!({"flexDirection":"row","gap":20,"alignItems":"flex-end"}),
        vec![
            cover(app, t, &a.tracks[0].path, 140., 10., 256),
            v(
                "album-text",
                json!({"gap":4,"flex":1,"minWidth":0}),
                vec![
                    title,
                    text(if a.artist.is_empty() { key("player.unknownArtist") } else { json!(a.artist) }, 15., &t["accent"]),
                    text(subtitle, 12., &t["textDim"]),
                ],
            ),
        ],
    );
    let rows = a.tracks.iter().enumerate().map(|(i, x)| row(app, t, i, &x.path, Some(if x.track > 0 { x.track } else { i as i64 + 1 }))).collect();
    json!({"kind":"scroll","key":"album","style":{"flex":1,"paddingHorizontal":10},"children":[
        v("album", json!({"gap":20,"paddingHorizontal":10,"paddingBottom":16}), vec![head, v("album-rows", json!({"marginHorizontal":-10}), vec![table_header(app, t), v("tracks", json!({}), rows)])])]})
}
fn artist_list(app: &App, t: &Value) -> Value {
    let rows = app
        .artists
        .iter()
        .map(|a| {
            click(
                v(
                    &format!("artist:{}", a.name),
                    json!({"height":48,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":10,"borderRadius":8}),
                    vec![
                        cover(app, t, &a.tracks[0].path, 36., 18., 256),
                        {
                            let mut name = clipped(text(if a.name.is_empty() { key("player.unknownArtist") } else { json!(a.name) }, 13., &t["text"]));
                            name["style"]["flex"] = json!(1);
                            name["style"]["minWidth"] = json!(0);
                            name
                        },
                        text(json!({"key":"player.songs","params":{"count":a.tracks.len()}}), 11., &t["textDim"]),
                    ],
                ),
                "artist",
                json!(a.name),
            )
        })
        .collect();
    json!({"kind":"scroll","key":"artists","style":{"flex":1,"paddingHorizontal":10},"children":[v("artist-rows", json!({"paddingBottom":16}), rows)]})
}
fn video_cards(app: &App, t: &Value) -> Value {
    let cards = app
        .videos
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let picture = match app.thumb(&x.path, 320) {
                Some(src) => json!({"kind":"image","src":src,"style":{"width":200,"height":112,"objectFit":"cover"}}),
                None => text(json!("▶"), 26., &json!("#666")),
            };
            click(
                v(
                    &format!("video:{}", x.path),
                    json!({"width":200,"gap":8}),
                    vec![
                        v("poster", json!({"width":200,"height":112,"borderRadius":8,"overflow":"hidden","backgroundColor":"#111","alignItems":"center","justifyContent":"center"}), vec![picture]),
                        clipped(text(json!(x.title), 13., &t["text"])),
                    ],
                ),
                "row",
                json!(i),
            )
        })
        .collect();
    grid("videos", cards)
}
fn empty_state(app: &App, t: &Value) -> Value {
    let videos = app.view == View::Videos;
    v(
        "empty",
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":20}),
        vec![
            text(json!(if videos { "▶" } else { "♪" }), 40., &t["textDim"]),
            {
                let mut title = text(key(if videos { "player.emptyVideosTitle" } else { "player.emptyMusicTitle" }), 15., &t["text"]);
                title["style"]["fontWeight"] = json!(600);
                title
            },
            {
                let mut hint = text(key(if videos { "player.emptyVideos" } else { "player.emptyMusic" }), 12., &t["textDim"]);
                hint["style"]["textAlign"] = json!("center");
                hint["style"]["maxWidth"] = json!(360);
                hint
            },
        ],
    )
}
fn content(app: &App, t: &Value) -> Value {
    let empty = !(if app.view == View::Videos { !app.videos.is_empty() } else { !app.tracks.is_empty() } || app.view == View::Queue);
    if empty {
        return empty_state(app, t);
    }
    match (app.view, &app.sel) {
        (View::Albums, Some(key)) => app.albums.iter().find(|a| a.key == *key).map_or_else(|| v("none", json!({"flex":1}), vec![]), |a| album_page(app, t, a)),
        (View::Albums, None) => grid("albums", app.albums.iter().map(|a| album_card(app, t, a)).collect()),
        (View::Artists, Some(name)) => grid("artist-albums", albums_of_artist(&app.albums, name).into_iter().map(|a| album_card(app, t, a)).collect()),
        (View::Artists, None) => artist_list(app, t),
        (View::Videos, _) => video_cards(app, t),
        (View::Queue, _) => virtual_list(app, t, app.queue.paths.iter().map(String::as_str).collect()),
        (View::Songs, _) => virtual_list(app, t, app.songs.iter().map(|x| x.path.as_str()).collect()),
    }
}
fn header(app: &App, t: &Value) -> Value {
    let mut kids = vec![];
    if app.sel.is_some() && app.view != View::Queue {
        kids.push(text_button("back", if app.rtl { "›" } else { "‹" }, 20., &t["text"], "back", Value::Null));
    }
    let title = match (app.view, &app.sel) {
        (View::Songs, _) => key("player.songsTitle"),
        (View::Artists, Some(name)) => json!(name),
        (View::Artists, None) => key("player.artists"),
        (View::Albums, _) => key("player.albums"),
        (View::Videos, _) => key("player.videos"),
        (View::Queue, _) => key("player.queue"),
    };
    let mut label = clipped(text(title, 22., &t["text"]));
    label["style"]["fontWeight"] = json!(600);
    kids.push(label);
    v("header", json!({"height":52,"flexDirection":"row","alignItems":"center","paddingHorizontal":20,"gap":6}), kids)
}
fn bottom_bar(app: &App, t: &Value, path: &str) -> Value {
    let narrow = app.narrow();
    // the library's tags, then the ones the open file reports, then the file name
    let tags = &app.state.info["tags"];
    let (mut title, mut artist, _, _) = line_of(app, path);
    if app.track_of(path).is_none() && !str_value(&tags["title"]).is_empty() {
        title = str_value(&tags["title"]).to_owned();
    }
    if artist.is_empty() {
        artist = str_value(&tags["artist"]).to_owned();
    }
    let mut art = cover(app, t, path, 48., 6., 256);
    if is_video(path) {
        art = click(v("cover-open", json!({}), vec![art]), "showVideo", Value::Null);
    }
    let mut left = vec![art];
    if !narrow {
        let mut name = clipped(text(json!(title), 13., &t["text"]));
        name["style"]["fontWeight"] = json!(600);
        left.push(v("now", json!({"flex":1,"minWidth":0,"gap":2,"overflow":"hidden"}), vec![name, clipped(text(json!(artist), 12., &t["textDim"]))]));
    }
    let left_style = if narrow { json!({"minWidth":0,"flexDirection":"row","alignItems":"center","gap":12}) } else { json!({"flex":1,"minWidth":0,"flexDirection":"row","alignItems":"center","gap":12}) };
    let center_style = if narrow { json!({"minWidth":0,"gap":2,"flex":1}) } else { json!({"minWidth":0,"gap":2,"width":CENTER}) };
    let mut kids = vec![
        v("now-left", left_style, left),
        v("now-center", center_style, vec![buttons(app, t, &t["text"], false), progress(app, t, false)]),
    ];
    if !narrow {
        kids.push(v("now-right", json!({"flex":1,"minWidth":0,"alignItems":"flex-end"}), vec![volume_control(app, t, false, &t["text"], false)]));
    }
    v(
        "bottom",
        json!({"height":BOTTOM,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":16,"paddingHorizontal":16,"backgroundColor":t["sidebarBg"],"borderTopWidth":1,"borderTopColor":t["separator"]}),
        kids,
    )
}
fn library(app: &App, t: &Value) -> Value {
    let main = v(
        "main",
        json!({"flex":1,"flexDirection":"row"}),
        vec![sidebar(app, t), v("page", json!({"flex":1,"minWidth":0}), vec![header(app, t), content(app, t)])],
    );
    let mut kids = vec![main];
    if let Some(path) = app.queue.current() {
        kids.push(bottom_bar(app, t, path));
    }
    let mut root = v("player", json!({"flex":1,"direction":if app.rtl{"rtl"}else{"ltr"}}), kids);
    root["events"]["onMouseDown"] = json!(["dismiss"]);
    root["dropAction"] = json!(["drop"]);
    root
}
pub(super) fn tree(app: &App, now: i64) -> Value {
    let t = app.theme.clone();
    if app.video_view() {
        video(app, &t, now)
    } else {
        library(app, &t)
    }
}
