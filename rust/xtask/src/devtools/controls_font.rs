//! Axia's tiny original COLR font: the three window-control discs of the VS Code product-icon theme.
//!
//!   cargo xtask gen-controls-font
//!
//! The generated font (assets/vscode/axia-window-controls/controls.ttf) is committed; neither the
//! RPM build nor the session needs this command. VS Code's documented product-icon API consumes it
//! without injected CSS. A color glyph is one disc layer; a monochrome outline of each control
//! stays underneath as a fallback for a renderer without color layers.
//!
//! The font is written table by table (head, hhea, maxp, OS/2, hmtx, cmap, loca, glyf, name, post,
//! COLR, CPAL), with a stable timestamp: a regeneration must not change the font just for time.

use crate::{at, Result};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const UPEM: i16 = 1024;
const ASCENT: i16 = 832;
const DESCENT: i16 = -192;
/// Seconds between 1904-01-01 and the stable "created" and "modified" time of the font.
const STAMP: u64 = 3_800_000_000;

/// A point of a contour and whether it is on the curve.
type Point = (i16, i16, bool);

struct Glyph {
    contours: Vec<Vec<Point>>,
}

fn polygon(points: &[(i16, i16)]) -> Vec<Point> {
    points.iter().map(|&(x, y)| (x, y, true)).collect()
}

/// The glyphs in order: `.notdef`, `disc`, `close`, `minimize`, `maximize`, `restore`.
fn glyphs() -> Vec<(&'static str, Glyph)> {
    // a 13 px circle in VS Code's normal 16 px icon cell, as quadratic curves (no image, effect or
    // animation): the contour of four on-curve points with an off-curve corner between each pair
    let (cx, cy, r) = (512i16, 320i16, 416i16);
    let disc = vec![
        (cx + r, cy, true),
        (cx + r, cy + r, false),
        (cx, cy + r, true),
        (cx - r, cy + r, false),
        (cx - r, cy, true),
        (cx - r, cy - r, false),
        (cx, cy - r, true),
        (cx + r, cy - r, false),
    ];
    let close = vec![polygon(&[(240, 544), (288, 592), (784, 96), (736, 48)]), polygon(&[(240, 96), (288, 48), (784, 544), (736, 592)])];
    let minimize = vec![polygon(&[(224, 288), (800, 288), (800, 352), (224, 352)])];
    let square = |x0: i16, y0: i16, x1: i16, y1: i16| polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    let frame = || vec![square(224, 32, 800, 96), square(224, 544, 800, 608), square(224, 96, 288, 544), square(736, 96, 800, 544)];
    vec![
        (".notdef", Glyph { contours: vec![] }),
        ("disc", Glyph { contours: vec![disc] }),
        ("close", Glyph { contours: close }),
        ("minimize", Glyph { contours: minimize }),
        ("maximize", Glyph { contours: frame() }),
        ("restore", Glyph { contours: frame() }),
    ]
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn pad4(mut data: Vec<u8>) -> Vec<u8> {
    while data.len() % 4 != 0 {
        data.push(0);
    }
    data
}

/// One simple TrueType glyph: header, contour ends, no instructions, flags and 16-bit deltas.
fn glyf_entry(g: &Glyph) -> (Vec<u8>, [i16; 4]) {
    let points: Vec<&Point> = g.contours.iter().flatten().collect();
    if points.is_empty() {
        return (Vec::new(), [0; 4]);
    }
    let bounds = [
        points.iter().map(|p| p.0).min().unwrap(),
        points.iter().map(|p| p.1).min().unwrap(),
        points.iter().map(|p| p.0).max().unwrap(),
        points.iter().map(|p| p.1).max().unwrap(),
    ];
    let mut out = Vec::new();
    out.extend(be16(g.contours.len() as u16));
    for b in bounds {
        out.extend(b.to_be_bytes());
    }
    let mut end = 0u16;
    for c in &g.contours {
        end += c.len() as u16;
        out.extend(be16(end - 1));
    }
    out.extend(be16(0)); // no instructions
    out.extend(points.iter().map(|p| u8::from(p.2)));
    for axis in 0..2 {
        let mut last = 0i16;
        for p in &points {
            let v = if axis == 0 { p.0 } else { p.1 };
            out.extend((v - last).to_be_bytes());
            last = v;
        }
    }
    (pad4(out), bounds)
}

fn name_table(records: &[(u16, &str)]) -> Vec<u8> {
    let mut strings = Vec::new();
    let mut entries = Vec::new();
    for (id, text) in records {
        let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        entries.push((*id, strings.len(), utf16.len()));
        strings.extend(utf16);
    }
    let mut out = Vec::new();
    out.extend(be16(0));
    out.extend(be16(entries.len() as u16));
    out.extend(be16(6 + 12 * entries.len() as u16));
    for (id, offset, len) in entries {
        for v in [3, 1, 0x409, id, len as u16, offset as u16] {
            out.extend(be16(v));
        }
    }
    out.extend(strings);
    out
}

/// A format 4 cmap with one segment per code point (the four private-use controls) and the end mark.
fn cmap_table(map: &[(u16, u16)]) -> Vec<u8> {
    let seg = map.len() + 1;
    let mut sub = Vec::new();
    for v in [4u16, 0, 0, (seg * 2) as u16] {
        sub.extend(be16(v));
    }
    let (mut power, mut shift) = (1usize, 0u16);
    while power * 2 <= seg {
        power *= 2;
        shift += 1;
    }
    for v in [(power * 2) as u16, shift, ((seg - power) * 2) as u16] {
        sub.extend(be16(v));
    }
    for (code, _) in map {
        sub.extend(be16(*code));
    }
    sub.extend(be16(0xFFFF));
    sub.extend(be16(0));
    for (code, _) in map {
        sub.extend(be16(*code));
    }
    sub.extend(be16(0xFFFF));
    for (code, glyph) in map {
        sub.extend(be16(glyph.wrapping_sub(*code)));
    }
    sub.extend(be16(1));
    for _ in 0..seg {
        sub.extend(be16(0));
    }
    let length = sub.len() as u16;
    sub[2..4].copy_from_slice(&be16(length));
    let mut out = Vec::new();
    // two encoding records (Unicode BMP, and Windows Unicode) point at the same subtable
    for v in [0u16, 2] {
        out.extend(be16(v));
    }
    for (platform, encoding) in [(0u16, 3u16), (3, 1)] {
        out.extend(be16(platform));
        out.extend(be16(encoding));
        out.extend(20u32.to_be_bytes()); // after the header and the two records
    }
    out.extend(sub);
    out
}

fn post_table(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(0x0002_0000u32.to_be_bytes());
    out.extend([0u8; 28]); // italicAngle, underline, isFixedPitch, memory hints
    out.extend(be16(names.len() as u16));
    let mut custom = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if *name == ".notdef" {
            out.extend(be16(0));
        } else {
            out.extend(be16(258 + custom.len() as u16));
            custom.push((i, *name));
        }
    }
    for (_, name) in custom {
        out.push(name.len() as u8);
        out.extend(name.as_bytes());
    }
    out
}

/// COLR version 0: each control is one layer, the disc, in the palette entry of its color.
fn colr_table(bases: &[(u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [0u16, bases.len() as u16] {
        out.extend(be16(v));
    }
    out.extend(14u32.to_be_bytes());
    out.extend((14 + 6 * bases.len() as u32).to_be_bytes());
    out.extend(be16(bases.len() as u16));
    for (i, (glyph, _)) in bases.iter().enumerate() {
        out.extend(be16(*glyph));
        out.extend(be16(i as u16));
        out.extend(be16(1));
    }
    for (_, palette_index) in bases {
        out.extend(be16(1)); // the disc glyph
        out.extend(be16(*palette_index));
    }
    out
}

/// CPAL version 0, one palette of BGRA colors.
fn cpal_table(colors: &[[u8; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [0u16, colors.len() as u16, 1, colors.len() as u16] {
        out.extend(be16(v));
    }
    out.extend(14u32.to_be_bytes());
    out.extend(be16(0));
    for [r, g, b] in colors {
        out.extend([*b, *g, *r, 255]);
    }
    out
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut word = [0u8; 4];
        word[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// The font file.
pub fn build() -> Vec<u8> {
    let glyph_list = glyphs();
    let names: Vec<&str> = glyph_list.iter().map(|(n, _)| *n).collect();
    let count = glyph_list.len() as u16;
    let (mut glyf, mut loca) = (Vec::new(), Vec::new());
    let (mut bounds_all, mut max_points, mut max_contours) = ([i16::MAX, i16::MAX, i16::MIN, i16::MIN], 0u16, 0u16);
    for (_, g) in &glyph_list {
        loca.extend((glyf.len() as u32).to_be_bytes());
        let (bytes, b) = glyf_entry(g);
        if !bytes.is_empty() {
            bounds_all = [bounds_all[0].min(b[0]), bounds_all[1].min(b[1]), bounds_all[2].max(b[2]), bounds_all[3].max(b[3])];
        }
        max_points = max_points.max(g.contours.iter().map(Vec::len).sum::<usize>() as u16);
        max_contours = max_contours.max(g.contours.len() as u16);
        glyf.extend(bytes);
    }
    loca.extend((glyf.len() as u32).to_be_bytes());

    let mut head = Vec::new();
    head.extend(0x0001_0000u32.to_be_bytes());
    head.extend(0x0001_0000u32.to_be_bytes()); // fontRevision 1.0
    head.extend(0u32.to_be_bytes()); // checkSumAdjustment, filled in below
    head.extend(0x5F0F_3CF5u32.to_be_bytes());
    head.extend(be16(0));
    head.extend(be16(UPEM as u16));
    head.extend(STAMP.to_be_bytes());
    head.extend(STAMP.to_be_bytes());
    for b in bounds_all {
        head.extend(b.to_be_bytes());
    }
    for v in [0u16, 8, 2, 1, 0] {
        head.extend(be16(v)); // macStyle, lowestRecPPEM, fontDirectionHint, indexToLocFormat (long), glyphDataFormat
    }

    let mut hhea = Vec::new();
    hhea.extend(0x0001_0000u32.to_be_bytes());
    for v in [ASCENT, DESCENT, 0] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(be16(UPEM as u16)); // advanceWidthMax
    hhea.extend(0i16.to_be_bytes()); // minLeftSideBearing
    hhea.extend(0i16.to_be_bytes()); // minRightSideBearing
    hhea.extend((bounds_all[2]).to_be_bytes()); // xMaxExtent
    for v in [1i16, 0, 0, 0, 0, 0, 0, 0] {
        hhea.extend(v.to_be_bytes()); // caretSlopeRise, run, offset, 4 reserved, metricDataFormat
    }
    hhea.extend(be16(count));

    let mut maxp = Vec::new();
    maxp.extend(0x0001_0000u32.to_be_bytes());
    for v in [count, max_points, max_contours, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
        maxp.extend(be16(v));
    }

    // OS/2 version 4, 96 bytes
    let mut os2 = Vec::new();
    for v in [4u16, UPEM as u16, 400, 5, 0] {
        os2.extend(be16(v)); // version, xAvgCharWidth, weight class, width class, fsType
    }
    for _ in 0..8 {
        os2.extend(0i16.to_be_bytes()); // subscript and superscript sizes and offsets
    }
    for _ in 0..3 {
        os2.extend(0i16.to_be_bytes()); // strikeout size and position, family class
    }
    os2.extend([0u8; 10]); // panose
    for range in [0u32, 0x1000_0000, 0, 0] {
        os2.extend(range.to_be_bytes()); // ulUnicodeRange1..4: the Private Use Area is bit 60
    }
    os2.extend(*b"NONE");
    for v in [0x0040u16, 0xE001, 0xE004] {
        os2.extend(be16(v)); // fsSelection (REGULAR), first and last character
    }
    for v in [ASCENT, DESCENT, 0] {
        os2.extend(v.to_be_bytes()); // typographic ascender, descender and line gap
    }
    os2.extend(be16(ASCENT as u16));
    os2.extend(be16(DESCENT.unsigned_abs()));
    os2.extend(0u64.to_be_bytes()); // ulCodePageRange1..2
    for _ in 0..2 {
        os2.extend(0i16.to_be_bytes()); // sxHeight, sCapHeight
    }
    for v in [0u16, 0x20, 0] {
        os2.extend(be16(v)); // usDefaultChar, usBreakChar, usMaxContext
    }
    assert_eq!(os2.len(), 96);

    let mut hmtx = Vec::new();
    for _ in 0..count {
        hmtx.extend(be16(UPEM as u16));
        hmtx.extend(0i16.to_be_bytes());
    }
    let cmap = cmap_table(&[(0xE001, 2), (0xE002, 3), (0xE003, 4), (0xE004, 5)]);
    let name = name_table(&[
        (0, "Original Axia DE artwork; MIT license."),
        (1, "Axia Window Controls"),
        (2, "Regular"),
        (3, "AxiaWindowControls-Regular-1"),
        (4, "Axia Window Controls"),
        (6, "AxiaWindowControls-Regular"),
    ]);
    let colr = colr_table(&[(2, 0), (3, 1), (4, 2), (5, 2)]);
    let cpal = cpal_table(&[[255, 95, 87], [254, 188, 46], [40, 200, 64]]);

    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"COLR", colr),
        (b"CPAL", cpal),
        (b"OS/2", os2),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"name", name),
        (b"post", post_table(&names)),
    ];
    tables.sort_by_key(|(tag, _)| **tag);
    let n = tables.len() as u16;
    let mut out = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    let (mut power, mut shift) = (1u16, 0u16);
    while power * 2 <= n {
        power *= 2;
        shift += 1;
    }
    for v in [n, power * 16, shift, n * 16 - power * 16] {
        out.extend(be16(v));
    }
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    let mut head_at = 0;
    for (tag, data) in &tables {
        out.extend(*tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        if *tag == b"head" {
            head_at = offset;
        }
        let padded = pad4(data.clone());
        offset += padded.len();
        body.extend(padded);
    }
    out.extend(body);
    // the whole-file checksum goes into head.checkSumAdjustment
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    out
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() {
        return Err("gen-controls-font takes no arguments".into());
    }
    let out = repo.join("assets/vscode/axia-window-controls/controls.ttf");
    if let Some(parent) = out.parent() {
        at(parent, fs::create_dir_all(parent))?;
    }
    at(&out, fs::write(&out, build()))?;
    println!("{}", out.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttf_parser::{Face, GlyphId, OutlineBuilder};

    #[derive(Default, Debug, PartialEq)]
    struct Path(Vec<String>);

    impl OutlineBuilder for Path {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("M{x},{y}"));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("L{x},{y}"));
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.0.push(format!("Q{x1},{y1},{x},{y}"));
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.0.push(format!("C{x1},{y1},{x2},{y2},{x},{y}"));
        }
        fn close(&mut self) {
            self.0.push("Z".into());
        }
    }

    fn committed() -> Vec<u8> {
        fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/vscode/axia-window-controls/controls.ttf")).unwrap()
    }

    fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
        let n = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
        for i in 0..n {
            let e = &font[12 + 16 * i..28 + 16 * i];
            if &e[..4] == tag {
                let (o, l) = (u32::from_be_bytes(e[8..12].try_into().unwrap()) as usize, u32::from_be_bytes(e[12..16].try_into().unwrap()) as usize);
                return &font[o..o + l];
            }
        }
        panic!("no {}", String::from_utf8_lossy(tag));
    }

    #[test]
    fn the_font_parses_and_has_the_same_shapes_metrics_and_map_as_the_committed_one() {
        let (mine, theirs) = (build(), committed());
        let (a, b) = (Face::parse(&mine, 0).unwrap(), Face::parse(&theirs, 0).unwrap());
        assert_eq!((a.number_of_glyphs(), a.units_per_em(), a.ascender(), a.descender()), (b.number_of_glyphs(), b.units_per_em(), b.ascender(), b.descender()));
        assert_eq!(a.number_of_glyphs(), 6);
        for glyph in 0..6 {
            let id = GlyphId(glyph);
            let (mut pa, mut pb) = (Path::default(), Path::default());
            let (ba, bb) = (a.outline_glyph(id, &mut pa), b.outline_glyph(id, &mut pb));
            assert_eq!((ba.map(|r| (r.x_min, r.y_min, r.x_max, r.y_max)), pa), (bb.map(|r| (r.x_min, r.y_min, r.x_max, r.y_max)), pb), "glyph {glyph}");
            assert_eq!(a.glyph_hor_advance(id), b.glyph_hor_advance(id));
        }
        for code in 0xE001..=0xE004u32 {
            let ch = char::from_u32(code).unwrap();
            assert_eq!(a.glyph_index(ch), b.glyph_index(ch));
            assert!(a.glyph_index(ch).is_some());
        }
        assert_eq!(a.glyph_index('A'), None);
    }

    #[test]
    fn the_color_layers_and_the_palette_are_the_committed_bytes() {
        let (mine, theirs) = (build(), committed());
        assert_eq!(table(&mine, b"COLR"), table(&theirs, b"COLR"));
        assert_eq!(table(&mine, b"CPAL"), table(&theirs, b"CPAL"));
        assert_eq!(table(&mine, b"head")[20..28], table(&theirs, b"head")[20..28]); // the stable timestamp
    }

    #[test]
    fn the_font_is_deterministic_and_its_checksums_hold() {
        let font = build();
        assert_eq!(font, build());
        assert_eq!(checksum(&font), 0xB1B0_AFBA);
        assert_eq!(table(&font, b"OS/2").len(), 96);
    }
}
