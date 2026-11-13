//! The on-screen keyboard's logic with nothing on screen (what sdk/oskmodel.js was).
//!
//! The layout is not data of our own: rows are lists of Linux key codes, and what each key shows
//! comes from the active XKB layout (`lord.input.keyLabels`, rust/axia-shell/src/keyboard), so the
//! keyboard follows it like a physical one. `labels` = `{"altgr", "keys": {code: [plain, shift,
//! altgr, altgr + shift]}}`. A key is `{code, level, label}` ready for `lord.input.tap(code, level)`;
//! level 1 is Shift, 2 AltGr. Reached through the launcher bridge as `{"op":"osk","cmd":…}`; the
//! JS adapter puts the fields of each object back in the order the public API always had.
use serde_json::{json, Map, Value};

const TAB: i64 = 15;
const ESC: i64 = 1;
const SPACE: i64 = 57;
const UP: i64 = 103;
const LEFT: i64 = 105;
const RIGHT: i64 = 106;
const DOWN: i64 = 108;
const COMMA: i64 = 51;
const DOT: i64 = 52;
const MARGIN: f64 = 12.;

fn range(a: i64, b: i64) -> Vec<i64> {
    (a..=b).collect()
}
fn row1() -> Vec<i64> {
    range(16, 25)
}
fn row2() -> Vec<i64> {
    range(30, 38)
}
fn row3() -> Vec<i64> {
    range(44, 50)
}
fn digits() -> Vec<i64> {
    range(2, 11)
}
// keys beside the letters: they join a row only when the layout puts a letter or an accent on them
// (ñ, ç, Arabic letters); plain punctuation stays on the symbol pages
const EXTRA1: [i64; 2] = [26, 27];
const EXTRA2: [i64; 2] = [39, 40];
const EXTRA3: [i64; 5] = [86, 43, 51, 52, 53];

/// Without XKB (no xkb-data installed): the US layout.
fn fallback_labels() -> Value {
    let mut keys = Map::new();
    let mut put = |codes: &[i64], plain: &str, shifted: &str| {
        let (plain, shifted): (Vec<char>, Vec<char>) = (plain.chars().collect(), shifted.chars().collect());
        for (i, c) in codes.iter().enumerate() {
            let p = plain[i].to_string();
            let s = shifted.get(i).map_or_else(|| p.clone(), char::to_string);
            keys.insert(c.to_string(), json!([p, s, "", ""]));
        }
    };
    put(&row1(), "qwertyuiop", "QWERTYUIOP");
    put(&row2(), "asdfghjkl", "ASDFGHJKL");
    put(&row3(), "zxcvbnm", "ZXCVBNM");
    put(&digits(), "1234567890", "!@#$%^&*()");
    put(&[12, 13, 26, 27, 39, 40, 41, 43, 51, 52, 53], "-=[];'`\\,./", "_+{}:\"~|<>?");
    keys.insert(SPACE.to_string(), json!([" ", " ", "", ""]));
    json!({"altgr": false, "keys": keys})
}

fn is_dead(s: &str) -> bool {
    s.starts_with("dead:")
}
fn shown(s: &str) -> &str {
    if is_dead(s) {
        &s[5..]
    } else {
        s
    }
}
/// A letter or a dead key (JS `/\p{L}/u`; Rust's alphabetic is the closest it has).
fn lettery(s: &str) -> bool {
    !s.is_empty() && (is_dead(s) || s.chars().any(char::is_alphabetic))
}

fn cell<'a>(labels: &'a Value, code: i64, level: usize) -> Option<&'a str> {
    labels["keys"][code.to_string()][level].as_str()
}
fn key_obj(code: i64, level: i64, label: &str) -> Value {
    json!({"code": code, "level": level, "label": label})
}

/// The key at this level, or nothing if the layout has no symbol there.
fn key_at(labels: &Value, code: i64, level: usize) -> Option<Value> {
    let row = labels["keys"][code.to_string()].as_array()?;
    let own = row.get(level).and_then(Value::as_str).filter(|s| !s.is_empty());
    let s = own.or_else(|| if level == 1 { row.first().and_then(Value::as_str).filter(|s| !s.is_empty()) } else { None })?;
    Some(key_obj(code, if own.is_some() { level as i64 } else { 0 }, shown(s)))
}

/// The codes of the layout in the order JS enumerates integer keys (ascending).
fn codes(labels: &Value) -> Vec<i64> {
    let mut v: Vec<i64> = labels["keys"].as_object().map(|m| m.keys().filter_map(|k| k.parse().ok()).collect()).unwrap_or_default();
    v.sort_unstable();
    v
}

/// Where the active layout has this symbol. A symbol that exists only as a dead key is that key and
/// then Space (`after`), as a person types it.
fn key_for(labels: &Value, ch: &str) -> Option<Value> {
    let levels: &[usize] = if labels["altgr"] == true { &[0, 1, 2, 3] } else { &[0, 1] };
    let mut dead = None;
    let dead_form = format!("dead:{ch}");
    for &level in levels {
        for code in codes(labels) {
            let s = cell(labels, code, level);
            if s == Some(ch) {
                return Some(key_obj(code, level as i64, ch));
            }
            if dead.is_none() && s == Some(dead_form.as_str()) {
                dead = Some(json!({"code": code, "level": level, "label": ch, "after": SPACE}));
            }
        }
    }
    dead
}

/// Rows of the letters page. `shift`: 0 off, 1 next key, 2 locked.
fn letter_rows(labels: &Value, shift: i64) -> Value {
    let level = usize::from(shift != 0);
    let row = |base: Vec<i64>, extra: &[i64]| -> Vec<Value> {
        let mut all = base;
        all.extend(extra.iter().copied().filter(|c| lettery(cell(labels, *c, 0).unwrap_or(""))));
        all.into_iter().filter_map(|c| key_at(labels, c, level)).collect()
    };
    json!([row(row1(), &EXTRA1), row(row2(), &EXTRA2), row(row3(), &EXTRA3)])
}

fn typable(labels: &Value, chars: &str) -> Vec<Value> {
    chars.chars().filter_map(|c| key_for(labels, &c.to_string())).collect()
}

/// The digits row: the real digits wherever the layout has them, or else what that row types.
fn digit_row(labels: &Value) -> Vec<Value> {
    let want = "1234567890";
    digits()
        .iter()
        .enumerate()
        .filter_map(|(i, code)| key_for(labels, &want[i..i + 1]).or_else(|| key_at(labels, *code, 0)))
        .collect()
}

/// Page 1: numbers and the usual symbols. Page 2: the rest, then the keys that move the caret. Only
/// what this layout can type is offered.
fn symbol_rows(labels: &Value, page: i64) -> Value {
    if page == 1 {
        return json!([digit_row(labels), typable(labels, "@#$%&-+()/"), typable(labels, "*\"':;!?")]);
    }
    json!([
        typable(labels, "~`|\\^_=[]{}"),
        typable(labels, "<>€£°¿¡§¬"),
        [
            {"code": TAB, "level": 0, "name": "tab"},
            {"code": LEFT, "level": 0, "label": "←"},
            {"code": DOWN, "level": 0, "label": "↓"},
            {"code": UP, "level": 0, "label": "↑"},
            {"code": RIGHT, "level": 0, "label": "→"},
            {"code": ESC, "level": 0, "name": "esc"},
        ]
    ])
}

fn punctuation(labels: &Value) -> Value {
    json!([
        key_for(labels, ",").unwrap_or_else(|| key_obj(COMMA, 0, ",")),
        key_for(labels, ".").unwrap_or_else(|| key_obj(DOT, 0, ".")),
    ])
}

/// JS `Math.round`: halves go up.
fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// A display's keyboard: tablet proportions, the whole width up to a limit, four rows.
fn panel_size(w: f64, h: f64) -> Value {
    let height = round(236_f64.max(336_f64.min(h * 0.4)));
    let width = w.min(760_f64.max(round(w * 0.8))).min(round(height * 3.6));
    let gap = if height < 280. { 6. } else { 8. };
    let pad = gap + 2.;
    json!({"width": width, "height": height, "gap": gap, "pad": pad, "keyH": ((height - pad * 2. - gap * 3.) / 4.).floor()})
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// Where a hint from another process puts the field: the hint `{y, h, fw, fh}` (y in the window and
/// `fw` × `fh` the window's size when it was given). A window that changed size since has moved its
/// contents, and a centered form by a lot, so that y is a place the field has left: null, as if it
/// were never known, until the app says where it is now. `top`: from the window's top to its content.
fn caret_on_screen(hint: &Value, frame: &Value, top: f64) -> Value {
    let (Some(hy), Some(hh)) = (num(&hint["y"]), num(&hint["h"])) else { return Value::Null };
    let (Some(fy), Some(fw), Some(fh)) = (num(&frame["y"]), num(&frame["w"]), num(&frame["h"])) else { return Value::Null };
    if hy < 0. || num(&hint["fw"]) != Some(fw) || num(&hint["fh"]) != Some(fh) {
        return Value::Null;
    }
    json!({"y": fy + top + hy, "h": hh})
}

/// What to do so the field stays in view. `caret`: `{y, h}` on screen or null when only the window
/// is known; `frame`: the window with the field or null; `area`: its work area; `top`: the keyboard's
/// top edge docked at the bottom; `panelBottom`: its bottom edge docked at the top. The reply is
/// `{"dock": "bottom" | "top"}` and, when the window must move, `"frameY"` and `"frameH"` (the
/// window's new y and, if it shrinks, h). It keeps nothing: each call decides from where the field
/// is now. On a low display the two places can overlap: the top is only worth it if it clears the field.
fn plan_avoid(input: &Value) -> Value {
    let caret = input["caret"].is_object().then(|| (input["caret"]["y"].as_f64().unwrap_or(0.), input["caret"]["h"].as_f64().unwrap_or(0.)));
    let frame = input["frame"].is_object().then(|| &input["frame"]);
    let (area_y, top) = (input["area"]["y"].as_f64().unwrap_or(0.), input["top"].as_f64().unwrap_or(0.));
    let panel_bottom = input["panelBottom"].as_f64().unwrap_or(0.);
    let min_height = input["minHeight"].as_f64().unwrap_or(160.);
    let refused = input["refused"] == true;
    let bottom = || json!({"dock": "bottom"});
    let fy = frame.map(|f| f["y"].as_f64().unwrap_or(0.));
    let fh = frame.map(|f| f["h"].as_f64().unwrap_or(0.));
    let need = match (caret, frame) {
        (Some((y, h)), _) => y + h + MARGIN,
        (None, Some(_)) => fy.unwrap_or(0.) + fh.unwrap_or(0.),
        (None, None) => 0.,
    } - top;
    if need <= 0. {
        return bottom();
    }
    let up = || json!({"dock": if caret.is_some_and(|(y, _)| y < panel_bottom) { "bottom" } else { "top" }});
    let (Some(fy), Some(fh)) = (fy, fh) else { return up() }; // nothing to move: the keyboard goes up
    if refused {
        return if caret.is_some() { up() } else { bottom() }; // a window with a larger minimum size kept it
    }
    let room = fy - area_y;
    if need <= room {
        return json!({"dock": "bottom", "frameY": fy - need});
    }
    let height = min_height.max(top - area_y);
    if height >= fh && room == 0. {
        return if caret.is_some() { up() } else { bottom() }; // it can't do more
    }
    json!({"dock": "bottom", "frameY": area_y, "frameH": fh.min(height)})
}

pub(super) fn request(input: &Value) -> Result<Value, &'static str> {
    let labels = &input["labels"];
    Ok(match input["cmd"].as_str().unwrap_or("") {
        "constants" => json!({"topGap": 30}),
        "fallbackLabels" => fallback_labels(),
        "keyFor" => key_for(labels, input["ch"].as_str().unwrap_or("")).unwrap_or(Value::Null),
        "letterRows" => letter_rows(labels, input["shift"].as_i64().unwrap_or(0)),
        "symbolRows" => symbol_rows(labels, input["page"].as_i64().unwrap_or(1)),
        "punctuation" => punctuation(labels),
        "panelSize" => panel_size(input["w"].as_f64().ok_or("invalid box")?, input["h"].as_f64().ok_or("invalid box")?),
        "caretOnScreen" => caret_on_screen(&input["hint"], &input["frame"], input["top"].as_f64().unwrap_or(0.)),
        "planAvoid" => plan_avoid(input),
        _ => return Err("unknown osk command"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_text(rows: &Value) -> Vec<String> {
        rows.as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(|k| k["label"].as_str().unwrap()).collect()).collect()
    }

    #[test]
    fn the_us_fallback_gives_the_letters_in_order_and_shift_uppercases() {
        let us = fallback_labels();
        assert_eq!(labels_text(&letter_rows(&us, 0)), ["qwertyuiop", "asdfghjkl", "zxcvbnm"]);
        assert_eq!(labels_text(&letter_rows(&us, 1)), ["QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"]);
        assert_eq!(letter_rows(&us, 2)[0][0], json!({"code": 16, "level": 1, "label": "Q"}));
        assert_eq!(key_for(&us, "@"), Some(json!({"code": 3, "level": 1, "label": "@"})));
        assert_eq!(key_for(&us, "€"), None);
    }
    #[test]
    fn a_dead_key_is_typed_with_space_and_a_real_symbol_wins() {
        let mut latam = fallback_labels();
        latam["altgr"] = json!(true);
        latam["keys"]["27"] = json!(["+", "*", "dead:~", ""]);
        latam["keys"]["41"] = json!(["|", "°", "", ""]); // the layout has no real tilde
        assert_eq!(key_for(&latam, "~"), Some(json!({"code": 27, "level": 2, "label": "~", "after": SPACE})));
        assert!(key_for(&fallback_labels(), "`").unwrap().get("after").is_none());
    }
    #[test]
    fn the_panel_fits_and_the_keys_are_for_a_finger() {
        for (w, h) in [(1280., 800.), (800., 480.), (1920., 1080.), (3840., 2160.), (1024., 600.)] {
            let s = panel_size(w, h);
            let (width, height, keyh, gap, pad) = (s["width"].as_f64().unwrap(), s["height"].as_f64().unwrap(), s["keyH"].as_f64().unwrap(), s["gap"].as_f64().unwrap(), s["pad"].as_f64().unwrap());
            assert!(width <= w && height <= h * 0.5 && keyh >= 40. && pad * 2. + keyh * 4. + gap * 3. <= height, "{w}x{h}");
        }
    }
    #[test]
    fn the_window_rises_shrinks_or_the_keyboard_goes_up() {
        let area = json!({"x": 0, "y": 30, "w": 1280, "h": 770});
        let frame = json!({"x": 100, "y": 200, "w": 600, "h": 500});
        let plan = |caret: Value, frame: &Value| plan_avoid(&json!({"caret": caret, "frame": frame, "area": area, "top": 480}));
        assert_eq!(plan(json!({"y": 300, "h": 20}), &frame), json!({"dock": "bottom"}));
        assert_eq!(plan(json!({"y": 500, "h": 20}), &frame), json!({"dock": "bottom", "frameY": 148.}));
        assert_eq!(plan(json!({"y": 690, "h": 20}), &frame), json!({"dock": "bottom", "frameY": 30., "frameH": 450.}));
        assert_eq!(plan(Value::Null, &frame), json!({"dock": "bottom", "frameY": 30., "frameH": 450.}));
        assert_eq!(plan(json!({"y": 600, "h": 20}), &Value::Null), json!({"dock": "top"}));
        assert_eq!(plan(Value::Null, &Value::Null), json!({"dock": "bottom"}));
    }
    #[test]
    fn a_stale_hint_is_dropped_and_a_fresh_one_moves_with_the_window() {
        let full = json!({"x": 200, "y": 60, "w": 880, "h": 700});
        let hint = json!({"y": 500, "h": 34, "fw": 880, "fh": 700});
        assert_eq!(caret_on_screen(&hint, &full, 40.), json!({"y": 600., "h": 34.}));
        assert_eq!(caret_on_screen(&hint, &json!({"x": 200, "y": 30, "w": 880, "h": 700}), 40.), json!({"y": 570., "h": 34.}));
        assert_eq!(caret_on_screen(&hint, &json!({"x": 200, "y": 30, "w": 700, "h": 700}), 40.), Value::Null);
        assert_eq!(caret_on_screen(&json!({"y": -1, "h": 34, "fw": 880, "fh": 700}), &full, 0.), Value::Null);
        assert_eq!(caret_on_screen(&hint, &Value::Null, 0.), Value::Null);
    }
}
