//! Generates server/src/ui/unidata.h from the Unicode Character Database (UCD) files: range tables for
//!   - bidi class (UAX #9), mirrored pairs and bracket pairs (bidi)
//!   - joining type for Arabic, Syriac, N'Ko, Mongolian… (ArabicShaping): init/medi/fina forms
//!   - script (Scripts.txt): OpenType tag and fallback font
//!   - combining class (canonical mark order) and default-ignorable characters
//!   - Indic categories (syllabic and positional, grouped the way HarfBuzz does), the Khmer and
//!     Myanmar ones, the Universal Shaping Engine ones (Tibetan, Sinhala, Balinese, Javanese…), marks,
//!     canonical decompositions of the complex scripts and composition exclusions
//!   - line-break classes that matter for Chinese, Japanese and Korean (UAX #14)
//!
//!   cargo xtask gen-unicode UCD-FOLDER
//!
//! The folder needs (from https://www.unicode.org/Public/<version>/ucd/):
//!   extracted/DerivedBidiClass.txt  extracted/DerivedJoiningType.txt  BidiMirroring.txt
//!   BidiBrackets.txt  Scripts.txt  PropertyValueAliases.txt  LineBreak.txt
//!   extracted/DerivedCombiningClass.txt  DerivedCoreProperties.txt
//!   IndicSyllabicCategory.txt  IndicPositionalCategory.txt  UnicodeData.txt  CompositionExclusions.txt
//!   ArabicShaping.txt, and from HarfBuzz (src/ms-use/): IndicSyllabicCategory-Additional.txt
//!   IndicPositionalCategory-Additional.txt (Universal Shaping Engine categories)
//! (the extracted/ ones may be in the same folder).

use crate::{at, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// One line of a UCD file: `XXXX..YYYY ; value ; ...`, or an `@missing` default.
#[derive(Clone, Debug)]
struct Entry {
    lo: u32,
    hi: u32,
    v: String,
    miss: bool,
    f: Vec<String>,
}

fn hex_prefix(text: &str) -> u32 {
    let end = text.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(text.len());
    u32::from_str_radix(&text[..end], 16).unwrap_or(0)
}

fn parse(text: &str, missing: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        if missing {
            if let Some(rest) = line.strip_prefix("# @missing: ") {
                let hexes = rest.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(0);
                if let Some(range) = rest[hexes..].strip_prefix("..") {
                    let end = range.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(0);
                    if hexes > 0 && end > 0 {
                        if let Some(value) = range[end..].strip_prefix("; ").and_then(|v| v.split_whitespace().next()) {
                            out.push(Entry { lo: u32::from_str_radix(&rest[..hexes], 16).unwrap_or(0), hi: u32::from_str_radix(&range[..end], 16).unwrap_or(0), v: value.to_string(), miss: true, f: vec![] });
                            continue;
                        }
                    }
                }
            }
        }
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let f: Vec<String> = body.split(';').map(|s| s.trim().to_string()).collect();
        let (a, b) = match f[0].split_once("..") {
            Some((a, b)) => (a, Some(b)),
            None => (f[0].as_str(), None),
        };
        let (lo, hi) = (hex_prefix(a), hex_prefix(b.filter(|b| !b.is_empty()).unwrap_or(a)));
        out.push(Entry { lo, hi, v: f.get(1).cloned().unwrap_or_default(), miss: false, f });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
struct Range {
    lo: u32,
    hi: u32,
    v: i64,
}

/// Assigns values to a sparse range map: first the `@missing` ones (from the most general to the most
/// specific, as they appear in the file), then the explicit ones; the result is the ranges whose
/// value is not the default.
fn ranges(entries: &[Entry], map: &mut dyn FnMut(&str) -> i64, default: i64) -> Vec<Range> {
    let mut val: Vec<Option<i64>> = vec![None; 0x110000];
    for e in entries.iter().filter(|e| e.miss).chain(entries.iter().filter(|e| !e.miss)) {
        let v = map(&e.v);
        for c in e.lo..=e.hi.min(0x10ffff) {
            val[c as usize] = Some(v);
        }
    }
    let mut out = Vec::new();
    let mut cur: Option<Range> = None;
    for c in 0..=0x10ffffu32 {
        let v = val[c as usize].unwrap_or(default);
        match &mut cur {
            Some(r) if r.v == v && r.hi == c.wrapping_sub(1) => r.hi = c,
            _ => {
                if let Some(r) = cur.take() {
                    if r.v != default {
                        out.push(r);
                    }
                }
                cur = Some(Range { lo: c, hi: c, v });
            }
        }
    }
    if let Some(r) = cur {
        if r.v != default {
            out.push(r);
        }
    }
    out
}

/// Ranges of a code point → value map, without the zero values.
fn to_ranges(map: &BTreeMap<u32, i64>) -> Vec<Range> {
    let mut out: Vec<Range> = Vec::new();
    for (&c, &v) in map {
        match out.last_mut() {
            Some(last) if last.hi == c.wrapping_sub(1) && last.v == v => last.hi = c,
            _ => out.push(Range { lo: c, hi: c, v }),
        }
    }
    out.retain(|r| r.v != 0);
    out
}

fn hex(n: u32) -> String {
    format!("0x{n:X}")
}

fn index_of(list: &[&str], name: &str) -> i64 {
    list.iter().position(|n| *n == name).map_or(-1, |i| i as i64)
}

/// `LIST[v]` as JavaScript would print it, `undefined` out of range.
fn name_at<'a>(list: &[&'a str], v: i64) -> &'a str {
    usize::try_from(v).ok().and_then(|i| list.get(i)).copied().unwrap_or("undefined")
}

fn table(name: &str, rows: Vec<String>) -> String {
    format!("static const UniRange {name}[] = {{\n{}\n}};\n\n", rows.join("\n"))
}

fn enum_of(prefix: &str, list: &[&str]) -> String {
    format!("enum {{ {} }};\n", list.iter().map(|c| format!("{prefix}_{c}")).collect::<Vec<_>>().join(", "))
}

fn range_rows(rs: &[Range], prefix: &str, list: &[&str]) -> Vec<String> {
    rs.iter().map(|r| format!("    {{ {}, {}, {prefix}_{} }},", hex(r.lo), hex(r.hi), name_at(list, r.v))).collect()
}

fn number_rows(rs: &[Range]) -> Vec<String> {
    rs.iter().map(|r| format!("    {{ {}, {}, {} }},", hex(r.lo), hex(r.hi), r.v)).collect()
}

const BIDI: [&str; 23] = ["L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S", "WS", "ON", "LRE", "LRO", "RLE", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI"];
const JOIN: [&str; 6] = ["U", "R", "L", "D", "C", "T"];
const LB: [&str; 8] = ["XX", "ID", "CL", "OP", "NS", "EX", "IS", "SA"];
const IC: [&str; 18] = ["X", "C", "V", "N", "H", "ZWNJ", "ZWJ", "M", "SM", "A", "PLACEHOLDER", "DOTTEDCIRCLE", "RS", "Repha", "Ra", "CM", "Symbol", "CS"];
const IP: [&str; 7] = ["END", "PRE_C", "ABOVE_C", "BELOW_C", "POST_C", "AFTER_MAIN", "PRE_M"];
const SEA: [&str; 36] = [
    "X", "C", "V", "N", "H", "ZWNJ", "ZWJ", "M", "SM", "A", "VD", "PLACEHOLDER", "DOTTEDCIRCLE", "RS", "MPst", "Repha", "Ra", "CM", "Symbol", "CS", "SMPst", "VAbv", "VBlw", "VPre", "VPst",
    "Robatic", "Xgroup", "Ygroup", "As", "MH", "MR", "MW", "MY", "PT", "VS", "ML",
];
const USE: [&str; 44] = [
    "O", "B", "N", "GB", "CGJ", "SUB", "H", "HN", "ZWNJ", "WJ", "R", "CS", "IS", "Sk", "G", "J", "SB", "SE", "HVM", "HM", "HR", "RK", "FAbv", "FBlw", "FPst", "MAbv", "MBlw", "MPst", "MPre", "CMAbv",
    "CMBlw", "VAbv", "VBlw", "VPst", "VPre", "VMAbv", "VMBlw", "VMPst", "VMPre", "SMAbv", "SMBlw", "FMAbv", "FMBlw", "FMPst",
];

fn lookup<'a>(pairs: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

const BIDI_LONG: [(&str, &str); 5] = [("Left_To_Right", "L"), ("Right_To_Left", "R"), ("Arabic_Letter", "AL"), ("European_Terminator", "ET"), ("Boundary_Neutral", "BN")];
const JOIN_LONG: [(&str, &str); 6] =
    [("Non_Joining", "U"), ("Right_Joining", "R"), ("Left_Joining", "L"), ("Dual_Joining", "D"), ("Join_Causing", "C"), ("Transparent", "T")];
const OT: [(&str, &str); 11] = [
    ("Zyyy", "DFLT"), ("Zinh", "DFLT"), ("Hira", "kana"), ("Kana", "kana"), ("Hrkt", "kana"), ("Hani", "hani"), ("Hang", "hang"), ("Laoo", "lao "), ("Yiii", "yi  "), ("Nkoo", "nko "), ("Vaii", "vai "),
];
const LB_MAP: [(&str, &str); 14] = [
    ("ID", "ID"), ("H2", "ID"), ("H3", "ID"), ("JL", "ID"), ("JV", "ID"), ("JT", "ID"), ("CL", "CL"), ("CP", "CL"), ("OP", "OP"), ("NS", "NS"), ("CJ", "NS"), ("EX", "EX"), ("IS", "IS"), ("SA", "SA"),
];
const IC_MAP: &[(&str, &str)] = &[
    ("Avagraha", "Symbol"), ("Bindu", "SM"), ("Brahmi_Joining_Number", "PLACEHOLDER"), ("Cantillation_Mark", "A"), ("Consonant", "C"), ("Consonant_Dead", "C"), ("Consonant_Final", "CM"),
    ("Consonant_Head_Letter", "C"), ("Consonant_Initial_Postfixed", "C"), ("Consonant_Killer", "M"), ("Consonant_Medial", "CM"), ("Consonant_Placeholder", "PLACEHOLDER"),
    ("Consonant_Preceding_Repha", "Repha"), ("Consonant_Prefixed", "X"), ("Consonant_Subjoined", "CM"), ("Consonant_Succeeding_Repha", "CM"), ("Consonant_With_Stacker", "CS"),
    ("Gemination_Mark", "SM"), ("Invisible_Stacker", "H"), ("Joiner", "ZWJ"), ("Modifying_Letter", "X"), ("Non_Joiner", "ZWNJ"), ("Nukta", "N"), ("Number", "PLACEHOLDER"),
    ("Number_Joiner", "PLACEHOLDER"), ("Pure_Killer", "M"), ("Register_Shifter", "RS"), ("Syllable_Modifier", "SM"), ("Tone_Letter", "X"), ("Tone_Mark", "N"), ("Virama", "H"),
    ("Visarga", "SM"), ("Vowel", "V"), ("Vowel_Dependent", "M"), ("Vowel_Independent", "V"),
];
const IP_MAP: [(&str, &str); 15] = [
    ("Not_Applicable", "END"), ("Left", "PRE_C"), ("Top", "ABOVE_C"), ("Bottom", "BELOW_C"), ("Right", "POST_C"), ("Bottom_And_Right", "POST_C"), ("Left_And_Right", "POST_C"),
    ("Top_And_Bottom", "BELOW_C"), ("Top_And_Bottom_And_Left", "BELOW_C"), ("Top_And_Bottom_And_Right", "POST_C"), ("Top_And_Left", "ABOVE_C"), ("Top_And_Left_And_Right", "POST_C"),
    ("Top_And_Right", "POST_C"), ("Overstruck", "AFTER_MAIN"), ("Visual_Order_Left", "PRE_M"),
];

fn ic_override() -> HashMap<u32, &'static str> {
    let mut m: HashMap<u32, &'static str> = [
        (0x0930, "Ra"), (0x09B0, "Ra"), (0x09F0, "Ra"), (0x0A30, "Ra"), (0x0AB0, "Ra"), (0x0B30, "Ra"), (0x0BB0, "Ra"), (0x0C30, "Ra"), (0x0CB0, "Ra"), (0x0D30, "Ra"),
        (0x0953, "SM"), (0x0954, "SM"), (0x0A72, "C"), (0x0A73, "C"), (0x1CF5, "C"), (0x1CF6, "C"), (0x0CF1, "CS"), (0x0CF2, "CS"), (0x00A0, "PLACEHOLDER"), (0x00D7, "PLACEHOLDER"),
        (0x2010, "PLACEHOLDER"), (0x2011, "PLACEHOLDER"), (0x2012, "PLACEHOLDER"), (0x2013, "PLACEHOLDER"), (0x2014, "PLACEHOLDER"), (0x2212, "PLACEHOLDER"), (0x25CC, "DOTTEDCIRCLE"),
        (0x200C, "ZWNJ"), (0x200D, "ZWJ"),
    ]
    .into_iter()
    .collect();
    for c in 0x1CE2..=0x1CE8 {
        m.insert(c, "A");
    }
    m.insert(0x1CED, "A");
    m.insert(0x1CF8, "A");
    m.insert(0x1CF9, "A");
    for c in 0xA8F2..=0xA8F7 {
        m.insert(c, "Symbol");
    }
    m
}

fn sea_override() -> HashMap<u32, &'static str> {
    let mut m: HashMap<u32, &'static str> = [
        (0x2015, "PLACEHOLDER"), (0x2022, "PLACEHOLDER"), (0x25FB, "PLACEHOLDER"), (0x25FC, "PLACEHOLDER"), (0x25FD, "PLACEHOLDER"), (0x25FE, "PLACEHOLDER"), (0x25CC, "DOTTEDCIRCLE"),
        (0x00A0, "PLACEHOLDER"), (0x179A, "Ra"), (0x17CC, "Robatic"), (0x17C9, "Robatic"), (0x17CA, "Robatic"), (0x17C6, "Xgroup"), (0x17CB, "Xgroup"), (0x17CD, "Xgroup"), (0x17CE, "Xgroup"),
        (0x17CF, "Xgroup"), (0x17D0, "Xgroup"), (0x17D1, "Xgroup"), (0x17C7, "Ygroup"), (0x17C8, "Ygroup"), (0x17DD, "Ygroup"), (0x17D3, "Ygroup"), (0x17D9, "PLACEHOLDER"), (0x104E, "C"),
        (0x1004, "Ra"), (0x101B, "Ra"), (0x105A, "Ra"), (0x1032, "A"), (0x1036, "A"), (0x103A, "As"), (0x103E, "MH"), (0x1060, "ML"), (0x103C, "MR"), (0x103D, "MW"), (0x1082, "MW"), (0x103B, "MY"),
        (0x105E, "MY"), (0x105F, "MY"), (0x1063, "PT"), (0x1064, "PT"), (0x1069, "PT"), (0x106A, "PT"), (0x106B, "PT"), (0x106C, "PT"), (0x106D, "PT"), (0xAA7B, "PT"), (0x1038, "SM"),
        (0x1087, "SM"), (0x1088, "SM"), (0x1089, "SM"), (0x108A, "SM"), (0x108B, "SM"), (0x108C, "SM"), (0x108D, "SM"), (0x108F, "SM"), (0x109A, "SM"), (0x109B, "SM"), (0x109C, "SM"),
        (0x104A, "PLACEHOLDER"),
    ]
    .into_iter()
    .collect();
    for c in 0xFE00..=0xFE0F {
        m.insert(c, "VS");
    }
    m
}

const SEA_POS: [(&str, &str); 12] = [
    ("Left", "VPre"), ("Top", "VAbv"), ("Bottom", "VBlw"), ("Right", "VPst"), ("Bottom_And_Right", "VPst"), ("Left_And_Right", "VPst"), ("Top_And_Bottom", "VBlw"),
    ("Top_And_Bottom_And_Left", "VBlw"), ("Top_And_Bottom_And_Right", "VPst"), ("Top_And_Left", "VAbv"), ("Top_And_Left_And_Right", "VPst"), ("Top_And_Right", "VPst"),
];

fn sea_block(c: u32) -> Option<&'static str> {
    if (0x1780..=0x17FF).contains(&c) {
        Some("Khmer")
    } else if (0x1000..=0x109F).contains(&c) || (0xA9E0..=0xA9FF).contains(&c) || (0xAA60..=0xAA7F).contains(&c) || (0x116D0..=0x116FF).contains(&c) {
        Some("Myanmar")
    } else if c <= 0xFF || (0x2000..=0x209F).contains(&c) || (0x1CD0..=0x1CFF).contains(&c) || (0xA8E0..=0xA8FF).contains(&c) {
        Some("other")
    } else {
        None
    }
}

/// The USE rule names in the order of the old tool; `O` and `WJ` depend on the others.
const USE_RULES: [&str; 29] = [
    "B", "N", "GB", "CGJ", "F", "FM", "M", "CM", "SUB", "CS", "H", "HVM", "HN", "IS", "G", "HM", "HR", "J", "SB", "SE", "ZWNJ", "O", "RK", "R", "Sk", "SM", "V", "VM", "WJ",
];

struct Props<'a> {
    u: u32,
    i: &'a str,
    d: bool,
    g: &'a str,
    j: &'a str,
}

fn use_rule(name: &str, p: &Props) -> bool {
    let (u, i, d, g, j) = (p.u, p.i, p.d, p.g, p.j);
    match name {
        "B" => {
            ["Number", "Consonant", "Consonant_Head_Letter", "Tone_Letter", "Vowel_Independent"].contains(&i)
                || (["C", "D", "L", "R"].contains(&j) && i != "Joiner")
                || (g == "Lo" && ["Avagraha", "Bindu", "Consonant_Final", "Consonant_Medial", "Consonant_Subjoined", "Vowel", "Vowel_Dependent"].contains(&i))
        }
        "N" => i == "Brahmi_Joining_Number",
        "GB" => i == "Consonant_Placeholder" || [0x2015, 0x2022, 0x25FB, 0x25FC, 0x25FD, 0x25FE].contains(&u),
        "CGJ" => i == "Joiner" || (d && ["Mc", "Me", "Mn"].contains(&g)),
        "F" => (i == "Consonant_Final" && g != "Lo") || i == "Consonant_Succeeding_Repha",
        "FM" => i == "Syllable_Modifier",
        "M" => (i == "Consonant_Medial" && g != "Lo") || i == "Consonant_Initial_Postfixed",
        "CM" => ["Nukta", "Gemination_Mark", "Consonant_Killer"].contains(&i),
        "SUB" => i == "Consonant_Subjoined" && g != "Lo",
        "CS" => i == "Consonant_With_Stacker",
        "H" => i == "Virama" && u != 0x0DCA,
        "HVM" => u == 0x0DCA,
        "HN" => i == "Number_Joiner",
        "IS" => i == "Invisible_Stacker" && u != 0x1A60,
        "G" => i == "Hieroglyph",
        "HM" => i == "Hieroglyph_Modifier",
        "HR" => i == "Hieroglyph_Mirror",
        "J" => i == "Hieroglyph_Joiner",
        "SB" => i == "Hieroglyph_Mark_Begin" || i == "Hieroglyph_Segment_Begin",
        "SE" => i == "Hieroglyph_Mark_End" || i == "Hieroglyph_Segment_End",
        "ZWNJ" => i == "Non_Joiner",
        "RK" => i == "Reordering_Killer",
        "R" => i == "Consonant_Preceding_Repha" || i == "Consonant_Prefixed",
        "Sk" => u == 0x1A60,
        "SM" => i == "Symbol_Modifier",
        "V" => i == "Pure_Killer" || (g != "Lo" && (i == "Vowel" || i == "Vowel_Dependent")),
        "VM" => ["Tone_Mark", "Cantillation_Mark", "Register_Shifter", "Visarga"].contains(&i) || (g != "Lo" && i == "Bindu"),
        "WJ" => {
            (d && ![0x115F, 0x1160, 0x3164, 0xFFA0, 0x1BCA0, 0x1BCA1, 0x1BCA2, 0x1BCA3].contains(&u) && i == "Other" && !use_rule("CGJ", p)) || g == "Cn"
        }
        "O" => {
            (g == "Po" || ["Consonant_Dead", "Joiner", "Modifying_Letter", "Other"].contains(&i))
                && !use_rule("B", p)
                && !use_rule("GB", p)
                && !use_rule("CGJ", p)
                && !use_rule("SM", p)
                && !use_rule("WJ", p)
        }
        _ => false,
    }
}

fn use_positions(cat: &str) -> Option<&'static [(&'static str, &'static [&'static str])]> {
    Some(match cat {
        "F" => &[("Abv", &["Top"]), ("Blw", &["Bottom"]), ("Pst", &["Right"])],
        "M" => &[("Abv", &["Top"]), ("Blw", &["Bottom", "Bottom_And_Left", "Bottom_And_Right"]), ("Pst", &["Right"]), ("Pre", &["Left", "Top_And_Bottom_And_Left"])],
        "CM" => &[("Abv", &["Top"]), ("Blw", &["Bottom", "Overstruck"])],
        "V" => &[
            ("Abv", &["Top", "Top_And_Bottom", "Top_And_Bottom_And_Right", "Top_And_Right"]),
            ("Blw", &["Bottom", "Overstruck", "Bottom_And_Right"]),
            ("Pst", &["Right"]),
            ("Pre", &["Left", "Top_And_Left", "Top_And_Left_And_Right", "Left_And_Right"]),
        ],
        "VM" => &[("Abv", &["Top"]), ("Blw", &["Bottom", "Overstruck"]), ("Pst", &["Right"]), ("Pre", &["Left"])],
        "SM" => &[("Abv", &["Top"]), ("Blw", &["Bottom"])],
        "FM" => &[("Abv", &["Top"]), ("Blw", &["Bottom"]), ("Pst", &["Not_Applicable"])],
        _ => return None,
    })
}

struct Ucd(PathBuf);

impl Ucd {
    fn read(&self, name: &str) -> Result<String> {
        for p in [self.0.join(name), self.0.join("extracted").join(name)] {
            if p.exists() {
                return Ok(String::from_utf8_lossy(&at(&p, fs::read(&p))?).into_owned());
            }
        }
        Err(format!("missing {name}"))
    }
}

fn field<'a>(f: &[&'a str], i: usize) -> &'a str {
    f.get(i).copied().unwrap_or("")
}

fn categories(text: &str) -> HashMap<u32, String> {
    let mut m = HashMap::new();
    for e in parse(text, false) {
        for c in e.lo..=e.hi {
            m.insert(c, e.v.clone());
        }
    }
    m
}

/// The whole header text for a UCD folder, and a one-line summary.
pub fn generate(dir: &Path) -> Result<(String, String)> {
    let ucd = Ucd(dir.to_path_buf());
    let bidi_text = ucd.read("DerivedBidiClass.txt")?;
    // DerivedBidiClass-<version>.txt in the header of the file
    let version = bidi_text
        .find("DerivedBidiClass-")
        .and_then(|i| {
            let rest = &bidi_text[i + "DerivedBidiClass-".len()..];
            let run: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let version = if rest[run.len()..].starts_with("txt") { run.strip_suffix('.').unwrap_or(&run) } else { return None };
            (!version.is_empty()).then(|| version.to_string())
        })
        .unwrap_or_else(|| "?".into());
    let (mut enums, mut h) = (String::new(), String::new());

    // ---- bidi ----
    let bidi = ranges(&parse(&bidi_text, true), &mut |v| index_of(&BIDI, lookup(&BIDI_LONG, v).unwrap_or(v)), 0);
    enums += &enum_of("BIDI", &BIDI);
    h += &table("uni_bidi", range_rows(&bidi, "BIDI", &BIDI));
    let mirror: Vec<(u32, u32)> = parse(&ucd.read("BidiMirroring.txt")?, false).iter().map(|e| (e.lo, hex_prefix(e.f.get(1).map_or("", String::as_str)))).collect();
    h += &format!("static const UniPair uni_mirror[] = {{\n{}\n}};\n\n", mirror.iter().map(|(a, b)| format!("    {{ {}, {} }},", hex(*a), hex(*b))).collect::<Vec<_>>().join("\n"));
    let brackets: Vec<(u32, u32, bool)> =
        parse(&ucd.read("BidiBrackets.txt")?, false).iter().map(|e| (e.lo, hex_prefix(e.f.get(1).map_or("", String::as_str)), e.f.get(2).is_some_and(|v| v == "o"))).collect();
    h += &format!(
        "static const UniBracket uni_brackets[] = {{\n{}\n}};\n\n",
        brackets.iter().map(|(a, b, open)| format!("    {{ {}, {}, {} }},", hex(*a), hex(*b), u8::from(*open))).collect::<Vec<_>>().join("\n")
    );

    // ---- joining ----
    let join = ranges(&parse(&ucd.read("DerivedJoiningType.txt")?, true), &mut |v| index_of(&JOIN, lookup(&JOIN_LONG, v).unwrap_or(v)), 0);
    enums += &enum_of("JOIN", &JOIN);
    h += &table("uni_join", range_rows(&join, "JOIN", &JOIN));

    // ---- scripts: ISO 15924 code and OpenType tag ----
    let (mut alias, mut long_name): (HashMap<String, String>, HashMap<String, String>) = (HashMap::new(), HashMap::new());
    for line in ucd.read("PropertyValueAliases.txt")?.split('\n') {
        // ^sc\s*;\s*(\w+)\s*;\s*(\w+)
        let Some(rest) = line.strip_prefix("sc") else { continue };
        let Some(rest) = rest.trim_start().strip_prefix(';') else { continue };
        let word = |s: &str| s.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>();
        let first = word(rest);
        let Some(after) = rest.trim_start().get(first.len()..).and_then(|r| r.trim_start().strip_prefix(';')) else { continue };
        let second = word(after);
        if !first.is_empty() && !second.is_empty() {
            alias.insert(second.clone(), first.clone());
            long_name.insert(first, second);
        }
    }
    let mut scripts: Vec<String> = vec!["Zyyy".into()];
    let mut sidx = |name: &str| -> i64 {
        let code = if name == "Common" { "Zyyy".to_string() } else { alias.get(name).cloned().unwrap_or_else(|| name.to_string()) };
        match scripts.iter().position(|s| *s == code) {
            Some(i) => i as i64,
            None => {
                scripts.push(code);
                scripts.len() as i64 - 1
            }
        }
    };
    sidx("Inherited"); // 1 = Inherited
    let scripts_text = ucd.read("Scripts.txt")?;
    let script = ranges(&parse(&scripts_text, false), &mut sidx, 0);
    enums += "#define SCRIPT_COMMON 0\n#define SCRIPT_INHERITED 1\n";
    for k in ["Latn", "Grek", "Cyrl", "Arab", "Hebr", "Syrc", "Thaa", "Nkoo", "Mong", "Hani", "Hira", "Kana", "Hang", "Thai", "Laoo", "Khmr", "Mymr"] {
        enums += &format!("#define SCRIPT_{} {}\n", k.to_uppercase(), scripts.iter().position(|s| s == k).map_or(-1, |i| i as i64));
    }
    enums += &format!("#define SCRIPT_COUNT {}\n", scripts.len());
    h += &format!(
        "static const char uni_script_tag[][5] = {{\n{}\n}};\n\n",
        scripts.iter().map(|s| format!("    \"{}\",   /* {s} */", lookup(&OT, s).map_or_else(|| s.to_lowercase(), String::from))).collect::<Vec<_>>().join("\n")
    );
    h += &format!(
        "static const char *const uni_script_name[] = {{\n{}\n}};\n\n",
        scripts.iter().map(|s| format!("    \"{}\",", long_name.get(s).unwrap_or(s).replace('_', " "))).collect::<Vec<_>>().join("\n")
    );
    h += &number_table("uni_script", &script);

    // ---- line breaking ----
    let lb = ranges(&parse(&ucd.read("LineBreak.txt")?, true), &mut |v| index_of(&LB, lookup(&LB_MAP, v).unwrap_or("XX")), 0);
    enums += &enum_of("LB", &LB);
    h += &table("uni_linebreak", range_rows(&lb, "LB", &LB));

    // ---- marks: canonical combining class; invisibles ----
    let ccc = ranges(&parse(&ucd.read("DerivedCombiningClass.txt")?, true), &mut |v| v.trim().parse::<f64>().ok().filter(|n| *n != 0.0).map_or(0, |n| n as i64), 0);
    h += &number_table("uni_ccc", &ccc);
    let core = ucd.read("DerivedCoreProperties.txt")?;
    let ignorable: Vec<Entry> = parse(&core, false).into_iter().filter(|e| e.v == "Default_Ignorable_Code_Point").collect();
    let ign = ranges(&ignorable, &mut |_| 1, 0);
    h += &format!("static const UniRange uni_ignorable[] = {{\n{}\n}};\n\n", ign.iter().map(|r| format!("    {{ {}, {}, 1 }},", hex(r.lo), hex(r.hi))).collect::<Vec<_>>().join("\n"));

    // ---- Indic scripts ----
    let (ic_over, isc_text, ipc_text) = (ic_override(), ucd.read("IndicSyllabicCategory.txt")?, ucd.read("IndicPositionalCategory.txt")?);
    let indic_range = |c: u32| (0x0900..=0x0DFF).contains(&c) || (0x1CD0..=0x1CFF).contains(&c) || (0xA8E0..=0xA8FF).contains(&c) || ic_over.contains_key(&c);
    let mut ic_val: BTreeMap<u32, i64> = BTreeMap::new();
    for e in parse(&isc_text, false) {
        for c in e.lo..=e.hi {
            if indic_range(c) {
                ic_val.insert(c, index_of(&IC, lookup(&IC_MAP, &e.v).unwrap_or("X")));
            }
        }
    }
    for (c, v) in &ic_over {
        ic_val.insert(*c, index_of(&IC, v));
    }
    let mut ip_val: BTreeMap<u32, i64> = BTreeMap::new();
    for e in parse(&ipc_text, false) {
        for c in e.lo..=e.hi {
            if indic_range(c) {
                ip_val.insert(c, index_of(&IP, lookup(&IP_MAP, &e.v).unwrap_or("END")));
            }
        }
    }
    let (ic_r, ip_r) = (to_ranges(&ic_val), to_ranges(&ip_val));
    enums += &enum_of("IC", &IC);
    enums += &enum_of("IP", &IP);
    h += &table("uni_indic_cat", range_rows(&ic_r, "IC", &IC));
    h += &table("uni_indic_pos", range_rows(&ip_r, "IP", &IP));

    // two-part canonical decompositions of the complex scripts, and which ones are not recomposed
    let complex = |c: u32| {
        (0x0900..=0x0DFF).contains(&c) || (0x0F00..=0x109F).contains(&c) || (0x1780..=0x17FF).contains(&c) || (0x1B00..=0x1BFF).contains(&c) || (0xA800..=0xABFF).contains(&c) || (0x11000..=0x11FFF).contains(&c)
    };
    let unicode_data_text = ucd.read("UnicodeData.txt")?;
    let unicode_data: Vec<Vec<&str>> = unicode_data_text.split('\n').filter(|l| !l.is_empty()).map(|l| l.split(';').collect()).collect();
    let mut decomp: Vec<(u32, u32, u32)> = Vec::new();
    for f in &unicode_data {
        let c = hex_prefix(field(f, 0));
        let d = field(f, 5);
        if !complex(c) || d.is_empty() || d.starts_with('<') {
            continue;
        }
        let parts: Vec<u32> = d.split(' ').map(hex_prefix).collect();
        if parts.len() == 2 {
            decomp.push((c, parts[0], parts[1]));
        }
    }
    let mut excl: HashSet<u32> = HashSet::new();
    for line in ucd.read("CompositionExclusions.txt")?.split('\n') {
        let end = line.find(|c: char| !(c.is_ascii_digit() || ('A'..='F').contains(&c))).unwrap_or(line.len());
        if end > 0 {
            excl.insert(u32::from_str_radix(&line[..end], 16).unwrap_or(0));
        }
    }
    let ccc_of: HashMap<u32, i64> = unicode_data.iter().map(|f| (hex_prefix(field(f, 0)), field(f, 3).trim().parse::<f64>().ok().map_or(0, |n| n as i64))).collect();
    for (c, a, _) in &decomp {
        if ccc_of.get(a).copied().unwrap_or(0) != 0 {
            excl.insert(*c);
        }
    }
    h += "typedef struct { uint32_t cp, a, b; uint8_t excluded; } UniDecomp;\n";
    h += &format!(
        "static const UniDecomp uni_decomp[] = {{\n{}\n}};\n\n",
        decomp.iter().map(|(c, a, b)| format!("    {{ {}, {}, {}, {} }},", hex(*c), hex(*a), hex(*b), u8::from(excl.contains(c)))).collect::<Vec<_>>().join("\n")
    );

    // ---- marks (general category Mn, Mc, Me) ----
    let gc_of: HashMap<u32, &str> = unicode_data.iter().map(|f| (hex_prefix(field(f, 0)), field(f, 2))).collect();
    let mark_map: BTreeMap<u32, i64> = gc_of.iter().filter(|(_, gc)| matches!(**gc, "Mn" | "Mc" | "Me")).map(|(c, _)| (*c, 1)).collect();
    let mark_r = to_ranges(&mark_map);
    h += &format!("static const UniRange uni_mark[] = {{\n{}\n}};\n\n", mark_r.iter().map(|r| format!("    {{ {}, {}, 1 }},", hex(r.lo), hex(r.hi))).collect::<Vec<_>>().join("\n"));

    // ---- Khmer and Myanmar ----
    let (isc_all, ipc_all) = (categories(&isc_text), categories(&ipc_text));
    let sea_over = sea_override();
    let mut sea_val: BTreeMap<u32, i64> = BTreeMap::new();
    let candidates: HashSet<u32> = isc_all.keys().chain(sea_over.keys()).copied().collect();
    for c in candidates {
        let block = sea_block(c);
        if block.is_none() && !sea_over.contains_key(&c) {
            continue;
        }
        let ipc = ipc_all.get(&c).map_or("Not_Applicable", String::as_str);
        let mut cat = isc_all.get(&c).and_then(|v| lookup(&IC_MAP, v)).unwrap_or("X");
        if cat == "SM" && ipc == "Not_Applicable" {
            cat = "SMPst";
        }
        if let Some(over) = sea_over.get(&c) {
            cat = over;
        }
        if (cat == "M" || cat == "MPst") && matches!(block, Some("Khmer" | "Myanmar")) {
            cat = lookup(&SEA_POS, ipc).unwrap_or(cat);
        }
        sea_val.insert(c, index_of(&SEA, cat));
    }
    let sea_r = to_ranges(&sea_val);
    enums += &format!("{}#define SEA_NAMES {}\n", enum_of("SEA", &SEA), SEA.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "));
    h += &table("uni_sea", range_rows(&sea_r, "SEA", &SEA));

    // ---- Universal Shaping Engine ----
    let (mut use_isc, mut use_ipc) = (isc_all.clone(), ipc_all.clone());
    let mut ajt: HashMap<u32, String> = HashMap::new();
    let mut udi: HashSet<u32> = HashSet::new();
    let mut in_use: HashSet<u32> = isc_all.keys().chain(ipc_all.keys()).copied().collect();
    for e in parse(&ucd.read("ArabicShaping.txt")?, false) {
        for c in e.lo..=e.hi {
            ajt.insert(c, e.f.get(2).cloned().unwrap_or_default());
            in_use.insert(c);
        }
    }
    for e in parse(&core, false).into_iter().filter(|e| e.v == "Default_Ignorable_Code_Point") {
        for c in e.lo..=e.hi {
            udi.insert(c);
            in_use.insert(c);
        }
    }
    for e in parse(&ucd.read("IndicSyllabicCategory-Additional.txt")?, false) {
        for c in e.lo..=e.hi {
            use_isc.insert(c, if e.v == "Consonant_Final_Modifier" { "Syllable_Modifier".into() } else { e.v.clone() });
            in_use.insert(c);
        }
    }
    for e in parse(&ucd.read("IndicPositionalCategory-Additional.txt")?, false) {
        for c in e.lo..=e.hi {
            use_ipc.insert(c, if e.v == "NA" { "Not_Applicable".into() } else { e.v.clone() });
            in_use.insert(c);
        }
    }
    let script_name = categories(&scripts_text);
    let mut use_val: BTreeMap<u32, i64> = BTreeMap::new();
    let mut conflicts = 0usize;
    for u in in_use {
        let sc = script_name.get(&u).map_or("Unknown", String::as_str);
        if ["Arabic", "Lao", "Samaritan", "Syriac", "Thai"].contains(&sc) {
            continue;
        }
        let g = gc_of.get(&u).copied().unwrap_or("Cn");
        if g == "Cn" {
            continue; // unassigned in this Unicode version
        }
        let mut i = use_isc.get(&u).map_or("Other", String::as_str);
        let mut p = use_ipc.get(&u).map_or("Not_Applicable", String::as_str);
        let (d, j) = (udi.contains(&u), ajt.get(&u).map_or("X", String::as_str));
        if (0x1CE2..=0x1CE8).contains(&u) {
            i = "Cantillation_Mark";
        }
        if (0x0F18..=0x0F19).contains(&u) || (0x0F3E..=0x0F3F).contains(&u) {
            i = "Vowel_Dependent";
        }
        if u == 0x1CED {
            i = "Tone_Mark";
        }
        let props = Props { u, i, d, g, j };
        let hits: Vec<&str> = USE_RULES.iter().copied().filter(|k| use_rule(k, &props)).collect();
        if hits.len() != 1 {
            conflicts += 1;
            if hits.is_empty() {
                continue;
            }
        }
        let mut cat = hits[0].to_string();
        if [0x11302, 0x11303, 0x114C1].contains(&u) {
            p = "Top";
        }
        if let Some(pm) = use_positions(&cat) {
            let suffixes: Vec<&str> = pm.iter().filter(|(_, list)| list.contains(&p)).map(|(k, _)| *k).collect();
            if suffixes.len() != 1 {
                conflicts += 1;
                continue;
            }
            cat += suffixes[0];
        }
        if cat != "O" {
            use_val.insert(u, index_of(&USE, &cat));
        }
    }
    let use_r = to_ranges(&use_val);
    enums += &format!("enum {{ {}, USE_COUNT }};\n#define USE_NAMES {}\n", USE.iter().map(|c| format!("USE_{c}")).collect::<Vec<_>>().join(", "), USE.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "));
    h += &table("uni_use", range_rows(&use_r, "USE", &USE));

    let header = format!(
        "/* unidata.h — generated by cargo xtask gen-unicode from Unicode {version}. Do not edit.\n * Sorted range tables; absent values are the default value (0). The tables\n * are only compiled in unicode.c; everything else uses the functions in unicode.h. */\n#ifndef LORD_UNIDATA_H\n#define LORD_UNIDATA_H\n\n#include <stdint.h>\n\n#define UNIDATA_VERSION \"{version}\"\n\n{enums}\n#ifdef UNIDATA_TABLES\n\ntypedef struct {{ uint32_t lo, hi; uint8_t v; }} UniRange;\ntypedef struct {{ uint32_t a, b; }} UniPair;\ntypedef struct {{ uint32_t a, b; uint8_t open; }} UniBracket;\n\n{h}/* Value of cp in a range table (0 if absent). */\nstatic inline uint8_t uni_lookup(const UniRange *t, int n, uint32_t cp) {{\n    int lo = 0, hi = n - 1;\n    while (lo <= hi) {{\n        int mid = (lo + hi) / 2;\n        if (cp < t[mid].lo) hi = mid - 1;\n        else if (cp > t[mid].hi) lo = mid + 1;\n        else return t[mid].v;\n    }}\n    return 0;\n}}\n#define UNI_N(t) ((int)(sizeof t / sizeof *t))\n\n#endif /* UNIDATA_TABLES */\n#endif\n"
    );
    let summary = format!(
        "Unicode {version}, Indic {}/{}/{}, Khmer/Myanmar {}, USE {} ({conflicts} without a unique category), marks {}, bidi {} ranges, ccc {}, invisibles {}, joining {}, scripts {}/{}, line break {}, mirrors {}, brackets {}",
        ic_r.len(), ip_r.len(), decomp.len(), sea_r.len(), use_r.len(), mark_r.len(), bidi.len(), ccc.len(), ign.len(), join.len(), scripts.len(), script.len(), lb.len(), mirror.len(), brackets.len()
    );
    Ok((header, summary))
}

fn number_table(name: &str, rs: &[Range]) -> String {
    table(name, number_rows(rs))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let Some(dir) = args.first() else {
        eprintln!("usage: cargo xtask gen-unicode <UCD folder>");
        return Ok(ExitCode::from(1));
    };
    let (text, summary) = generate(Path::new(dir))?;
    let out = repo.join("server/src/ui/unidata.h");
    at(&out, fs::write(&out, text))?;
    println!("{}: {summary}", out.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_give_ranges_missing_defaults_and_fields() {
        let text = "# @missing: 0000..10FFFF; Left_To_Right\n0041..005A ; L # letters\n\n0300 ; NSM\n# a comment\n";
        let entries = parse(text, true);
        assert_eq!(entries.len(), 3);
        assert!(entries[0].miss && (entries[0].lo, entries[0].hi, entries[0].v.as_str()) == (0, 0x10FFFF, "Left_To_Right"));
        assert_eq!((entries[1].lo, entries[1].hi, entries[1].v.as_str()), (0x41, 0x5A, "L"));
        assert_eq!((entries[2].lo, entries[2].hi), (0x300, 0x300));
        assert_eq!(parse(text, false).len(), 2);
    }

    #[test]
    fn ranges_apply_the_defaults_first_and_drop_the_default_value() {
        let text = "# @missing: 0000..10FFFF; X\n# @missing: 0600..06FF; AL\n0041..0043 ; L\n0620 ; R\n";
        let rs = ranges(&parse(text, true), &mut |v| index_of(&BIDI, if v == "X" { "L" } else { v }), 0);
        // L (0) is the default value: only AL and R show up, with the letters of 0041..0043 inside L
        assert_eq!(rs, [Range { lo: 0x600, hi: 0x61F, v: 2 }, Range { lo: 0x620, hi: 0x620, v: 1 }, Range { lo: 0x621, hi: 0x6FF, v: 2 }]);
    }

    #[test]
    fn to_ranges_merges_neighbours_and_drops_zero() {
        let map: BTreeMap<u32, i64> = [(1, 5), (2, 5), (3, 6), (5, 6), (6, 0)].into_iter().collect();
        assert_eq!(to_ranges(&map), [Range { lo: 1, hi: 2, v: 5 }, Range { lo: 3, hi: 3, v: 6 }, Range { lo: 5, hi: 5, v: 6 }]);
        assert_eq!((hex(0x1f), name_at(&BIDI, 2), name_at(&BIDI, -1), index_of(&BIDI, "ZZ")), ("0x1F".to_string(), "AL", "undefined", -1));
    }

    #[test]
    fn the_use_rules_pick_one_category() {
        let letter = Props { u: 0x1000, i: "Consonant", d: false, g: "Lo", j: "X" };
        assert!(use_rule("B", &letter) && !use_rule("O", &letter) && !use_rule("V", &letter));
        let vowel = Props { u: 0x102C, i: "Vowel_Dependent", d: false, g: "Mc", j: "X" };
        assert!(use_rule("V", &vowel) && !use_rule("B", &vowel));
        let zwj = Props { u: 0x200D, i: "Joiner", d: true, g: "Cf", j: "C" };
        assert!(use_rule("CGJ", &zwj) && !use_rule("O", &zwj));
        assert_eq!(use_positions("V").unwrap().iter().filter(|(_, l)| l.contains(&"Top")).count(), 1);
        assert!(use_positions("B").is_none());
    }
}
