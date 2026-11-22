//! The fonts of LordBench's shaping workload (server/src/bench/ui.rs, docs/BENCHMARK.md).
//!
//! axia-bench opens no files, so the fonts it shapes with are built into it (Rust `include_bytes!`,
//! from the files in server/src/bench/fonts). The Latin one is Axia's own Inter (assets/fonts); the
//! others are Debian 13's Noto fonts subset to exactly the characters of the paragraphs in
//! server/src/bench/paragraphs.rs, keeping every OpenType layout feature and lookup those characters
//! can reach, so the shaper gives the same glyphs and positions it gives with the full font on that
//! text. It is not the same work: the subsets have smaller coverage tables and fewer lookups, and
//! shape about 12 % faster than the full fonts (VM 116), so the workload is comparable only with
//! itself. Run it by hand when a paragraph changes (it is not part of the build):
//!
//!   cargo xtask gen-lbfonts [--fonts DIR] [--ucd DIR]
//!
//! DIR (default /usr/share/fonts) must hold truetype/noto/NotoSans{Arabic,Devanagari,Thai}-Regular.ttf
//! and opentype/noto/NotoSansCJK-Regular.ttc (face 2, Simplified Chinese); the Unicode data
//! (default /usr/share/unicode/ucd, package unicode-ucd) gives the decompositions. The subsetting
//! itself is done by fontTools' `pyftsubset` (`dnf install fonttools`; `PYFTSUBSET` overrides the
//! program). The results are committed: every build, on every machine, shapes with the same bytes.

use crate::{at, Result};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// (output, source relative to --fonts, face index in a collection)
const FONTS: [(&str, &str, u32); 4] = [
    ("NotoSansArabic-LB.ttf", "truetype/noto/NotoSansArabic-Regular.ttf", 0),
    ("NotoSansDevanagari-LB.ttf", "truetype/noto/NotoSansDevanagari-Regular.ttf", 0),
    ("NotoSansThai-LB.ttf", "truetype/noto/NotoSansThai-Regular.ttf", 0),
    ("NotoSansCJKsc-LB.otf", "opentype/noto/NotoSansCJK-Regular.ttc", 2),
];

/// The five paragraphs of paragraphs.rs: its plain `"…"` literals between `PARAGRAPHS … = [` and `];`.
pub fn paragraphs(source: &str) -> Result<Vec<String>> {
    let body = &source[source.find("PARAGRAPHS").ok_or("no PARAGRAPHS in the source")?..];
    let from = body.find("= [").ok_or("no list after PARAGRAPHS")? + 3;
    let to = from + body[from..].find("];").ok_or("the list is not closed")?;
    let mut out = Vec::new();
    let mut rest = &body[from..to];
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['"', '\\']) else { break };
        if after.as_bytes()[close] == b'"' {
            out.push(after[..close].to_string());
            rest = &after[close + 1..];
        } else {
            // a literal with an escape is not matched by the old pattern; skip its opening quote
            rest = after;
        }
    }
    if out.len() != 5 {
        return Err(format!("gen-lbfonts: expected 5 paragraphs, found {}", out.len()));
    }
    Ok(out)
}

/// Canonical and compatibility decompositions from UnicodeData.txt.
pub struct Decomposer(HashMap<u32, (bool, Vec<u32>)>);

impl Decomposer {
    pub fn new(unicode_data: &str) -> Self {
        let mut map = HashMap::new();
        for line in unicode_data.lines() {
            let f: Vec<&str> = line.split(';').collect();
            let Some(d) = f.get(5).filter(|d| !d.is_empty()) else { continue };
            let compat = d.starts_with('<');
            let parts: Vec<u32> = d.split(' ').filter(|p| !p.starts_with('<')).filter_map(|p| u32::from_str_radix(p, 16).ok()).collect();
            if let Some(c) = u32::from_str_radix(f[0], 16).ok().filter(|_| !parts.is_empty()) {
                map.insert(c, (compat, parts));
            }
        }
        Self(map)
    }

    /// The characters of NFD (or NFKD) of `c`; the order does not matter to a subset.
    pub fn decompose(&self, c: u32, compat: bool, out: &mut Vec<u32>) {
        // Hangul syllables decompose by arithmetic
        if (0xAC00..0xAC00 + 11172).contains(&c) {
            let s = c - 0xAC00;
            out.push(0x1100 + s / 588);
            out.push(0x1161 + (s % 588) / 28);
            if s % 28 != 0 {
                out.push(0x11A7 + s % 28);
            }
            return;
        }
        match self.0.get(&c) {
            Some((is_compat, parts)) if compat || !is_compat => {
                for &p in parts {
                    self.decompose(p, compat, out);
                }
            }
            _ => out.push(c),
        }
    }
}

/// The code points to keep: the text and what the shaper may turn it into (Thai SARA AM is shaped
/// as NIKHAHIT + SARA AA, shape.c).
pub fn characters(text: &str, decomposer: &Decomposer) -> Vec<u32> {
    let mut set: BTreeSet<u32> = text.chars().map(|c| c as u32).collect();
    for compat in [false, true] {
        let mut out = Vec::new();
        for c in text.chars() {
            decomposer.decompose(c as u32, compat, &mut out);
        }
        set.extend(out);
    }
    set.into_iter().collect()
}

/// `maxp.numGlyphs` of an sfnt font file.
pub fn glyph_count(font: &[u8]) -> Option<u16> {
    let tables = u16::from_be_bytes(font.get(4..6)?.try_into().ok()?) as usize;
    for i in 0..tables {
        let entry = font.get(12 + 16 * i..12 + 16 * i + 16)?;
        if &entry[..4] == b"maxp" {
            let offset = u32::from_be_bytes(entry[8..12].try_into().ok()?) as usize;
            return Some(u16::from_be_bytes(font.get(offset + 4..offset + 6)?.try_into().ok()?));
        }
    }
    None
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let (mut fonts, mut ucd) = (PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/share/unicode/ucd"));
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--fonts" => fonts = PathBuf::from(value),
            "--ucd" => ucd = PathBuf::from(value),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let source = repo.join("server/src/bench/paragraphs.rs");
    let text = paragraphs(&at(&source, fs::read_to_string(&source))?)?.concat();
    let data = ucd.join("UnicodeData.txt");
    let decomposer = Decomposer::new(&at(&data, fs::read_to_string(&data))?);
    let unicodes: Vec<String> = characters(&text, &decomposer).iter().map(|c| format!("{c:X}")).collect();
    let out = repo.join("server/src/bench/fonts");
    at(&out, fs::create_dir_all(&out))?;
    let program = std::env::var_os("PYFTSUBSET").unwrap_or_else(|| "pyftsubset".into());
    for (name, rel, index) in FONTS {
        let dst = out.join(name);
        // every feature (the shaper picks what its script needs), the name records kept, hinting dropped
        let status = Command::new(&program)
            .arg(fonts.join(rel))
            .arg(format!("--unicodes={}", unicodes.join(",")))
            .args(["--layout-features=*", "--layout-scripts=*", "--name-IDs=*", "--name-languages=*", "--name-legacy", "--notdef-outline", "--no-hinting"])
            .arg(format!("--font-number={index}"))
            .arg(format!("--output-file={}", dst.display()))
            .status()
            .map_err(|e| format!("{}: {e} (dnf install fonttools)", program.to_string_lossy()))?;
        if !status.success() {
            return Err(format!("pyftsubset failed for {name} ({status})"));
        }
        let bytes = at(&dst, fs::read(&dst))?;
        println!("{name}: {} bytes, {} glyphs", bytes.len(), glyph_count(&bytes).map_or("?".into(), |n| n.to_string()));
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
    }

    #[test]
    fn the_real_source_has_five_paragraphs() {
        let source = fs::read_to_string(repo().join("server/src/bench/paragraphs.rs")).unwrap();
        let list = paragraphs(&source).unwrap();
        assert_eq!(list.len(), 5);
        assert!(list.iter().all(|p| !p.is_empty()));
        assert!(paragraphs("pub const PARAGRAPHS: [&str; 2] = [\"a\", \"b\"];").unwrap_err().contains("expected 5"));
        assert!(paragraphs("nothing").is_err());
    }

    #[test]
    fn decompositions_follow_canonical_and_compatibility_rules() {
        let data = "0958;DEVANAGARI LETTER QA;Lo;0;L;0915 093C;;;;N;;;;;\n0E33;THAI CHARACTER SARA AM;Lo;0;L;<compat> 0E4D 0E32;;;;N;;;;;\n00C5;A WITH RING;Lu;0;L;0041 030A;;;;N;;;;;\n212B;ANGSTROM;Lu;0;L;00C5;;;;N;;;;;\n";
        let d = Decomposer::new(data);
        let set = |text: &str| characters(text, &d);
        // canonical: QA → KA + NUKTA, and SARA AM only by compatibility, so both forms are kept
        assert_eq!(set("\u{958}"), [0x915, 0x93C, 0x958]);
        assert_eq!(set("\u{E33}"), [0xE32, 0xE33, 0xE4D]);
        assert_eq!(set("\u{212B}"), [0x41, 0x30A, 0x212B]);
        // Hangul by arithmetic: GAG = G + A + G
        let mut out = Vec::new();
        d.decompose(0xAC01, false, &mut out);
        assert_eq!(out, [0x1100, 0x1161, 0x11A8]);
    }

    #[test]
    fn glyph_counts_come_from_the_maxp_table() {
        let dir = repo().join("server/src/bench/fonts");
        for name in ["NotoSansArabic-LB.ttf", "NotoSansDevanagari-LB.ttf", "NotoSansThai-LB.ttf", "NotoSansCJKsc-LB.otf"] {
            let font = fs::read(dir.join(name)).unwrap();
            assert!(glyph_count(&font).unwrap() > 10, "{name}");
        }
        assert_eq!(glyph_count(b"short"), None);
    }
}
