use super::*;

fn typed(m: &mut Keymap, keys: &[(i32, i32)]) -> String {
    keys.iter().map(|(c, v)| m.translate(*c, *v).chr).collect()
}
const DOWN: i32 = 1;
const UP: i32 = 0;

#[test]
fn latam_letters_shift_caps_and_altgr() {
    let mut m = Keymap::default();
    assert_eq!(typed(&mut m, &[(30, DOWN), (30, UP)]), "a");
    assert_eq!(typed(&mut m, &[(42, DOWN), (30, DOWN), (42, UP)]), "A");
    // Caps Lock and Shift cancel each other
    assert_eq!(typed(&mut m, &[(58, DOWN), (58, UP), (42, DOWN), (30, DOWN), (42, UP), (30, DOWN), (58, DOWN)]), "aA");
    assert_eq!(typed(&mut m, &[(100, DOWN), (16, DOWN), (100, UP)]), "@", "AltGr+Q");
    assert_eq!(typed(&mut m, &[(39, DOWN)]), "ñ");
    let k = m.translate(28, DOWN);
    assert_eq!((k.key.as_str(), k.chr.as_str()), ("Enter", ""));
}

#[test]
fn dead_keys_compose_or_type_themselves() {
    let mut m = Keymap::default();
    assert_eq!(typed(&mut m, &[(26, DOWN), (18, DOWN)]), "é");
    assert_eq!(typed(&mut m, &[(42, DOWN), (26, DOWN), (42, UP), (22, DOWN)]), "ü", "the dead key waits through Shift");
    assert_eq!(typed(&mut m, &[(26, DOWN), (57, DOWN)]), "´", "a space types the accent");
    assert_eq!(typed(&mut m, &[(26, DOWN), (26, DOWN)]), "´", "twice: the accent once");
    assert_eq!(typed(&mut m, &[(26, DOWN), (34, DOWN)]), "´g");
}

#[test]
fn shortcuts_get_a_base_character_but_no_text() {
    let mut m = Keymap::default();
    m.translate(29, DOWN);
    let k = m.translate(46, DOWN);
    assert_eq!((k.chr.as_str(), k.base.as_str(), k.flags), ("", "c", 2));
    m.translate(29, UP);
    m.set_layout(Layout::Us);
    m.translate(42, DOWN);
    let k = m.translate(3, DOWN);
    assert_eq!((k.chr.as_str(), k.key.as_str(), k.flags), ("@", "@", 1));
}

/// The former runtime/keymap.js on random key sequences: AXIA_KEYMAP_CORPUS is a file of
/// {"layout": …, "keys": [[code, value], …], "events": [{key, char, base, flags}, …]} lines.
#[test]
#[ignore = "needs a corpus made with the former runtime/keymap.js"]
fn matches_the_former_js_keymap_on_a_corpus() {
    let path = std::env::var("AXIA_KEYMAP_CORPUS").expect("AXIA_KEYMAP_CORPUS");
    let (mut events, mut wrong) = (0, Vec::new());
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut m = Keymap::default();
        m.set_layout(if case["layout"] == "us" { Layout::Us } else { Layout::Latam });
        for (key, want) in case["keys"].as_array().unwrap().iter().zip(case["events"].as_array().unwrap()) {
            let got = m.translate(key[0].as_i64().unwrap() as i32, key[1].as_i64().unwrap() as i32);
            events += 1;
            let want = (want["key"].as_str().unwrap(), want["char"].as_str().unwrap(), want["base"].as_str().unwrap(), want["flags"].as_i64().unwrap() as i32);
            // the JS event had no Caps Lock field: its effect shows in the characters
            if (got.key.as_str(), got.chr.as_str(), got.base.as_str(), got.flags & 31) != want {
                wrong.push(format!("{key} in {}: rust {got:?} js {want:?}", case["keys"]));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{} sequences differ:\n{}", wrong.len(), wrong.iter().take(15).cloned().collect::<Vec<_>>().join("\n"));
    println!("keymap: {events} key events translated exactly as the former JS did");
}
