use super::*;
const SKK: &[u8] = include_bytes!("../../tests/data/ime/SKK-JISYO.L");
const PINYIN: &[u8] = include_bytes!("../../tests/data/ime/pinyin_simp.dict.yaml");
fn instance(engine: &[u8], dictionaries: bool) -> Ime {
    let mut ime = Ime {
        kana: kana::Kana::for_test(dictionaries.then(|| SKK.to_vec())),
        pinyin: pinyin::Pinyin::for_test(dictionaries.then(|| PINYIN.to_vec())),
        ..Ime::default()
    };
    let result = ime.set(engine);
    assert_eq!(
        result,
        if engine != b"hangul" && !dictionaries {
            1
        } else {
            0
        }
    );
    ime.set_context(true);
    ime
}
fn key(byte: u8) -> KeyInfo {
    let mut key = KeyInfo::default();
    match byte {
        b'_' => {
            copy_text(&mut key.key, b" ");
            copy_text(&mut key.chr, b" ");
        }
        b'^' => copy_text(&mut key.key, b"Enter"),
        b'<' => copy_text(&mut key.key, b"Backspace"),
        b'~' => copy_text(&mut key.key, b"Escape"),
        _ => {
            key.key[0] = byte;
            key.chr[0] = byte;
            key.shift = byte.is_ascii_uppercase() as i32;
        }
    }
    key
}
fn press(ime: &mut Ime, key: &KeyInfo, cap: usize) -> (bool, Vec<u8>) {
    let mut output = Output {
        bytes: Vec::new(),
        cap,
    };
    let used = ime.key(key, &mut output);
    assert!(output.bytes.len() < cap || cap == 0);
    assert!((0..=64).contains(&ime.state.ncand));
    assert!(ime.state.sel >= 0 && (ime.state.sel < ime.state.ncand || ime.state.ncand == 0));
    assert!(ime.state.preedit.contains(&0));
    for candidate in &ime.state.cand[..ime.state.ncand as usize] {
        assert!(candidate.contains(&0));
    }
    (used, output.bytes)
}
fn feed(ime: &mut Ime, input: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for &byte in input {
        let key = key(byte);
        let (used, output) = press(ime, &key, 512);
        result.extend_from_slice(&output);
        if !used {
            result.extend_from_slice(text(&key.chr));
        }
    }
    result
}
fn named(name: &[u8], shift: bool) -> KeyInfo {
    let mut key = KeyInfo::default();
    copy_text(&mut key.key, name);
    key.shift = shift as i32;
    key
}

#[test]
fn korean_compounds_and_backspace_match_frozen_legacy_examples() {
    for (input, expected) in [(b"gksrmf^".as_slice(), "한글"), (b"rhkrtk^", "곽사")] {
        let mut ime = instance(b"hangul", false);
        assert_eq!(feed(&mut ime, input), expected.as_bytes());
        assert_eq!(text(&ime.state.preedit), b"");
    }
    let mut ime = instance(b"hangul", false);
    feed(&mut ime, b"gks");
    assert_eq!(text(&ime.state.preedit), "한".as_bytes());
    feed(&mut ime, b"<");
    assert_eq!(text(&ime.state.preedit), "하".as_bytes());
    feed(&mut ime, b"<");
    assert_eq!(text(&ime.state.preedit), "ㅎ".as_bytes());
    feed(&mut ime, b"<");
    assert_eq!(text(&ime.state.preedit), b"");
    assert!(!press(&mut ime, &named(b"Backspace", false), 512).0);
}
#[test]
fn japanese_romaji_and_phrase_segmentation_match_frozen_legacy_examples() {
    for (input, expected) in [
        (b"kon'nichiha^".as_slice(), "こんにちは"),
        (b"kitte^", "きって"),
        (b"watashihanihonheikimasu_^", "私は日本へ行きます"),
    ] {
        let mut ime = instance(b"kana", true);
        assert_eq!(feed(&mut ime, input), expected.as_bytes());
    }
    let mut ime = instance(b"kana", true);
    feed(&mut ime, b"nihongo_");
    assert_eq!(text(&ime.state.preedit), "日本語".as_bytes());
    assert_eq!(ime.state.ncand, 3);
    assert_eq!(text(&ime.state.cand[0]), "日本語".as_bytes());
    assert_eq!(text(&ime.state.cand[1]), "にほんご".as_bytes());
    assert_eq!(text(&ime.state.cand[2]), "ニホンゴ".as_bytes());
    assert_eq!((ime.state.focus_begin, ime.state.focus_end), (0, 9));
    press(&mut ime, &named(b"ArrowDown", false), 512);
    assert_eq!(text(&ime.state.preedit), "にほんご".as_bytes());
    press(&mut ime, &named(b"ArrowLeft", true), 512);
    assert!(ime.state.converting != 0);
    press(&mut ime, &named(b"Escape", false), 512);
    assert_eq!(ime.state.converting, 0);
    assert_eq!(text(&ime.state.preedit), "にほんご".as_bytes());
}
#[test]
fn chinese_exact_abbreviated_prefix_and_paged_selection_match_legacy_examples() {
    for (input, expected) in [
        (b"zg_".as_slice(), "中国"),
        (b"bjdx_", "北京大学"),
        (b"nihaox_^", "你好x"),
        (b"zhong-1", "中"),
    ] {
        let mut ime = instance(b"pinyin", true);
        assert_eq!(feed(&mut ime, input), expected.as_bytes());
    }
    let mut ime = instance(b"pinyin", true);
    feed(&mut ime, b"zhong");
    assert_eq!(ime.state.ncand, 64);
    assert_eq!(text(&ime.state.cand[0]), "中".as_bytes());
    feed(&mut ime, b"=");
    assert_eq!(ime.state.sel, 9);
    feed(&mut ime, b"-");
    assert_eq!(ime.state.sel, 0);
    assert!(!ime.state.cand[..ime.state.ncand as usize]
        .iter()
        .enumerate()
        .any(|(i, a)| ime.state.cand[..i].iter().any(|b| text(a) == text(b))));
}
#[test]
fn focus_shortcuts_and_unknown_engines_do_not_leak_composition() {
    let mut ime = instance(b"hangul", false);
    feed(&mut ime, b"gks");
    assert_eq!(ime.set(b"unknown"), -1);
    assert_eq!(text(&ime.state.preedit), "한".as_bytes());
    let mut shortcut = key(b'c');
    shortcut.ctrl = 1;
    let (used, output) = press(&mut ime, &shortcut, 512);
    assert!(!used);
    assert_eq!(output, "한".as_bytes());
    assert_eq!(text(&ime.state.preedit), b"");
    feed(&mut ime, b"gks");
    ime.set_context(false);
    assert_eq!(text(&ime.state.preedit), b"");
    assert_eq!(press(&mut ime, &key(b'a'), 512), (false, Vec::new()));
    assert_eq!(ime.set(b""), 0);
    ime.set_context(true);
    assert_eq!(press(&mut ime, &key(b'a'), 512), (false, Vec::new()));
    let mut missing = instance(b"kana", false);
    assert_eq!(feed(&mut missing, b"kitte^"), "きって".as_bytes());
    let mut missing = instance(b"pinyin", false);
    assert_eq!(feed(&mut missing, b"nihao_"), b"nihao");
}
#[test]
fn bounded_preedit_segment_counts_and_small_commit_buffers_remain_valid() {
    for engine in [b"hangul".as_slice(), b"kana", b"pinyin"] {
        let mut ime = instance(engine, true);
        for _ in 0..1000 {
            press(&mut ime, &key(b'a'), 2);
        }
        for cap in [0, 1, 2, 3, 4, 8, 16] {
            press(&mut ime, &named(b"Enter", false), cap);
            for &byte in b"nihongo" {
                press(&mut ime, &key(byte), cap);
            }
            press(&mut ime, &key(b'_'), cap);
        }
    }
    let mut ime = instance(b"kana", true);
    for _ in 0..30 {
        feed(&mut ime, b"watashihanihonheikimasu");
    }
    feed(&mut ime, b"_");
    for _ in 0..180 {
        press(&mut ime, &named(b"ArrowRight", true), 512);
    }
    for _ in 0..180 {
        press(&mut ime, &named(b"ArrowLeft", true), 512);
    }
    press(&mut ime, &named(b"Enter", false), 512);
    assert_eq!(ime.state.converting, 0);
}
