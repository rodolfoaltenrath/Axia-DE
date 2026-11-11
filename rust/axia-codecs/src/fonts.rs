//! Pure Rust TrueType/OpenType rasterization. The C text cache owns the
//! returned masks, but parsing, glyph metrics and rasterization live here.
use crate::{export, input};
use std::{collections::HashMap, ptr, sync::Mutex};
use ttf_parser::{Face, GlyphId};

pub struct Font {
    raster: fontdue::Font,
    data: Vec<u8>,
    index: u32,
    kern_cache: Mutex<HashMap<u32, i32>>,
}
#[no_mangle]
pub unsafe extern "C" fn axia_font_open(
    data: *const u8,
    len: usize,
    index: u32,
    ascent: *mut i32,
    descent: *mut i32,
    gap: *mut i32,
    upem: *mut i32,
    offset: *mut i32,
) -> *mut Font {
    if [ascent, descent, gap, upem, offset]
        .iter()
        .any(|p| p.is_null())
    {
        return ptr::null_mut();
    }
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    if data.len() > 64 * 1024 * 1024 {
        return ptr::null_mut();
    }
    let Ok(face) = Face::parse(data, index) else {
        return ptr::null_mut();
    };
    *ascent = i32::from(face.tables().hhea.ascender);
    *descent = i32::from(face.tables().hhea.descender);
    *gap = i32::from(face.tables().hhea.line_gap);
    *upem = i32::from(face.units_per_em());
    // otf.c still reads the original mapped font for shaping/color tables.
    *offset = if data.starts_with(b"ttcf") {
        let Some(start) = (index as usize)
            .checked_mul(4)
            .and_then(|n| n.checked_add(12))
        else {
            return ptr::null_mut();
        };
        let Some(bytes) = data.get(start..start + 4) else {
            return ptr::null_mut();
        };
        let value = u32::from_be_bytes(bytes.try_into().unwrap());
        let Ok(value) = i32::try_from(value) else {
            return ptr::null_mut();
        };
        value
    } else {
        0
    };
    if face.tables().glyf.is_none() && face.tables().cff.is_none() {
        return ptr::null_mut();
    }
    let settings = fontdue::FontSettings {
        collection_index: index,
        ..Default::default()
    };
    let Ok(raster) = fontdue::Font::from_bytes(data, settings) else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(Font {
        raster,
        data: data.to_vec(),
        index,
        kern_cache: Mutex::new(HashMap::new()),
    }))
}
#[no_mangle]
pub unsafe extern "C" fn axia_font_close(font: *mut Font) {
    if !font.is_null() {
        drop(Box::from_raw(font));
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_font_glyph(font: *const Font, cp: u32) -> u32 {
    let Some(font) = font.as_ref() else {
        return 0;
    };
    char::from_u32(cp)
        .map(|c| u32::from(font.raster.lookup_glyph_index(c)))
        .unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn axia_font_raster(
    font: *const Font,
    gid: u32,
    size: f32,
    w: *mut i32,
    h: *mut i32,
    x: *mut i32,
    y: *mut i32,
    advance: *mut f32,
) -> *mut u8 {
    if [w, h, x, y].iter().any(|p| p.is_null()) || advance.is_null() {
        return ptr::null_mut();
    }
    *w = 0;
    *h = 0;
    *x = 0;
    *y = 0;
    *advance = 0.0;
    let Some(font) = font.as_ref() else {
        return ptr::null_mut();
    };
    if !size.is_finite() || size <= 0.0 || size > 1024.0 || gid >= font.raster.glyph_count() as u32
    {
        return ptr::null_mut();
    }
    let metrics = font.raster.metrics_indexed(gid as u16, size);
    if metrics.width > 8192
        || metrics.height > 8192
        || metrics.width.saturating_mul(metrics.height) > 16 * 1024 * 1024
    {
        return ptr::null_mut();
    }
    let (m, bitmap) = font.raster.rasterize_indexed(gid as u16, size);
    *w = m.width as i32;
    *h = m.height as i32;
    *x = m.xmin;
    *y = -(m.ymin + *h);
    *advance = m.advance_width;
    if bitmap.is_empty() {
        ptr::null_mut()
    } else {
        export(&bitmap)
    }
}

fn kerning(font: &Font, left: u16, right: u16) -> i32 {
    use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
    let Ok(face) = Face::parse(&font.data, font.index) else {
        return 0;
    };
    let (left, right) = (GlyphId(left), GlyphId(right));
    // Match the previous rasterizer's GPOS pair adjustment, including class
    // pairs used by Inter. Falling back to legacy kern alone loses UI spacing.
    if let Some(gpos) = face.tables().gpos {
        for lookup in gpos.lookups {
            for sub in lookup.subtables.into_iter::<PositioningSubtable>() {
                let PositioningSubtable::Pair(pair) = sub else {
                    continue;
                };
                let adjustment = match pair {
                    PairAdjustment::Format1 { coverage, sets } => coverage
                        .get(left)
                        .and_then(|i| sets.get(i))
                        .and_then(|set| set.get(right)),
                    PairAdjustment::Format2 {
                        coverage,
                        classes,
                        matrix,
                    } => coverage
                        .get(left)
                        .and_then(|_| matrix.get((classes.0.get(left), classes.1.get(right)))),
                };
                if let Some((value, _)) = adjustment {
                    return i32::from(value.x_advance);
                }
            }
        }
        return 0;
    }
    face.tables()
        .kern
        .into_iter()
        .flat_map(|t| t.subtables)
        .filter(|s| s.horizontal && !s.variable && !s.has_cross_stream)
        .find_map(|s| s.glyphs_kerning(left, right))
        .map(i32::from)
        .unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn axia_font_kern(font: *const Font, left: u32, right: u32) -> i32 {
    let Some(font) = font.as_ref() else {
        return 0;
    };
    if left > u16::MAX as u32 || right > u16::MAX as u32 {
        return 0;
    }
    let key = (left << 16) | right;
    let Ok(mut cache) = font.kern_cache.lock() else {
        return 0;
    };
    if let Some(value) = cache.get(&key) {
        return *value;
    }
    let value = kerning(font, left as u16, right as u16);
    if cache.len() < 8192 {
        cache.insert(key, value);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    #[test]
    fn metrics_masks_spaces_and_gpos_kerning() {
        unsafe {
            let (mut asc, mut desc, mut gap, mut upem, mut off) = (0, 0, 0, 0, 0);
            let font = axia_font_open(
                INTER.as_ptr(),
                INTER.len(),
                0,
                &mut asc,
                &mut desc,
                &mut gap,
                &mut upem,
                &mut off,
            );
            assert!(!font.is_null());
            assert!(asc > 0 && desc < 0 && upem > 0);
            let a = axia_font_glyph(font, 'A' as u32);
            let v = axia_font_glyph(font, 'V' as u32);
            assert!(axia_font_kern(font, a, v) < 0);
            for cp in ['A', 'g', 'é', ' '] {
                let (mut w, mut h, mut x, mut y, mut adv) = (0, 0, 0, 0, 0.0);
                let mask = axia_font_raster(
                    font,
                    axia_font_glyph(font, cp as u32),
                    20.0,
                    &mut w,
                    &mut h,
                    &mut x,
                    &mut y,
                    &mut adv,
                );
                assert!(adv > 0.0);
                if cp == ' ' {
                    assert!(mask.is_null());
                } else {
                    assert!(w > 0 && h > 0 && y < 0);
                    assert!(std::slice::from_raw_parts(mask, (w * h) as usize)
                        .iter()
                        .any(|v| *v > 0));
                    libc::free(mask.cast());
                }
            }
            axia_font_close(font);
        }
    }
    #[test]
    fn rejects_invalid_font_and_collection_index() {
        unsafe {
            for (data, index) in [(b"invalid".as_slice(), 0), (INTER, 1)] {
                let (mut a, mut b, mut c, mut d, mut e) = (0, 0, 0, 0, 0);
                assert!(axia_font_open(
                    data.as_ptr(),
                    data.len(),
                    index,
                    &mut a,
                    &mut b,
                    &mut c,
                    &mut d,
                    &mut e
                )
                .is_null());
            }
        }
    }
    #[test]
    fn bundled_outline_formats_render() {
        for data in [
            include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
            include_bytes!("../../../assets/fonts/PlusJakartaSans-Medium.ttf").as_slice(),
            include_bytes!("../../../server/src/bench/fonts/NotoSansCJKsc-LB.otf").as_slice(),
        ] {
            let raster = fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).unwrap();
            let cp = if raster.lookup_glyph_index('A') != 0 {
                'A'
            } else {
                '中'
            };
            let (metrics, mask) = raster.rasterize(cp, 24.0);
            assert!(metrics.advance_width > 0.0 && mask.iter().any(|p| *p > 0));
        }
    }

    #[test]
    fn render_latin_sample() {
        unsafe {
            let (mut a, mut d, mut gap, mut upem, mut off) = (0, 0, 0, 0, 0);
            let font = axia_font_open(
                INTER.as_ptr(),
                INTER.len(),
                0,
                &mut a,
                &mut d,
                &mut gap,
                &mut upem,
                &mut off,
            );
            assert!(!font.is_null());
            let mut canvas =
                image::RgbaImage::from_pixel(1600, 240, image::Rgba([250, 250, 250, 255]));
            for (row, size) in [14.0, 20.0, 32.0, 48.0].into_iter().enumerate() {
                let mut pen = 20.0f32;
                let mut previous = 0;
                let baseline = 32 + row as i32 * 58;
                for cp in "Axia DE · Aplicativos, ações e configurações · AV gy".chars() {
                    let gid = axia_font_glyph(font, cp as u32);
                    assert_ne!(gid, 0);
                    pen += axia_font_kern(font, previous, gid) as f32 * size / upem as f32;
                    let (mut w, mut h, mut x, mut y, mut advance) = (0, 0, 0, 0, 0.0);
                    let mask = axia_font_raster(
                        font,
                        gid,
                        size,
                        &mut w,
                        &mut h,
                        &mut x,
                        &mut y,
                        &mut advance,
                    );
                    if !mask.is_null() {
                        let mask_slice = std::slice::from_raw_parts(mask, (w * h) as usize);
                        for yy in 0..h {
                            for xx in 0..w {
                                let (px, py) = (pen.round() as i32 + x + xx, baseline + y + yy);
                                if px >= 0 && py >= 0 && px < 1600 && py < 240 {
                                    let coverage = u32::from(mask_slice[(yy * w + xx) as usize]);
                                    let value =
                                        ((250 * (255 - coverage) + 25 * coverage) / 255) as u8;
                                    canvas.put_pixel(
                                        px as u32,
                                        py as u32,
                                        image::Rgba([value, value, value, 255]),
                                    );
                                }
                            }
                        }
                        libc::free(mask.cast());
                    }
                    assert!(advance > 0.0);
                    pen += advance;
                    previous = gid;
                }
                assert!(pen > 200.0 && pen < 1600.0);
            }
            if let Some(path) = std::env::var_os("AXIA_CODEC_FONT_SAMPLE") {
                canvas.save(path).unwrap();
            }
            axia_font_close(font);
        }
    }
}
