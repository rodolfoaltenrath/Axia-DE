//! Mail/contact image decoding and thumbnail generation, inside axia-maild.
use crate::{export, images, input};
use std::{ptr, slice};
fn thumbnail(data: &[u8], maxw: u32, maxh: u32) -> Option<(u32, u32, Vec<u32>)> {
    if data.len() > 64 * 1024 * 1024 || maxw == 0 || maxh == 0 {
        return None;
    }
    let image = images::decode(data, 16384, 40_000_000)?;
    let (w, h) = image.dimensions();
    let scale = 1.0f64
        .min(f64::from(maxw) / f64::from(w))
        .min(f64::from(maxh) / f64::from(h));
    let nw = (f64::from(w) * scale + 0.5).max(1.0) as u32;
    let nh = (f64::from(h) * scale + 0.5).max(1.0) as u32;
    let mut out = Vec::new();
    out.try_reserve_exact((nw as usize) * (nh as usize)).ok()?;
    for y in 0..nh {
        let y0 = (f64::from(y) / scale) as u32;
        let y1 = ((f64::from(y + 1) / scale) as u32).max(y0 + 1).min(h);
        for x in 0..nw {
            let x0 = (f64::from(x) / scale) as u32;
            let x1 = ((f64::from(x + 1) / scale) as u32).max(x0 + 1).min(w);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for yy in (y0..y1).step_by(((y1 - y0) / 4).max(1) as usize) {
                for xx in (x0..x1).step_by(((x1 - x0) / 4).max(1) as usize) {
                    let p = image.get_pixel(xx, yy);
                    let alpha = u64::from(p[3]);
                    r += u64::from(p[0]) * alpha;
                    g += u64::from(p[1]) * alpha;
                    b += u64::from(p[2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            out.push(
                ((a / n) as u32) << 24
                    | ((r / n / 255) as u32) << 16
                    | ((g / n / 255) as u32) << 8
                    | (b / n / 255) as u32,
            );
        }
    }
    Some((nw, nh, out))
}
#[no_mangle]
pub unsafe extern "C" fn img_decode(
    data: *const u8,
    len: usize,
    maxw: i32,
    maxh: i32,
    ow: *mut i32,
    oh: *mut i32,
) -> *mut u32 {
    if ow.is_null() || oh.is_null() {
        return ptr::null_mut();
    }
    *ow = 0;
    *oh = 0;
    if maxw <= 0 || maxh <= 0 {
        return ptr::null_mut();
    }
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    let Some((w, h, px)) = thumbnail(data, maxw as u32, maxh as u32) else {
        return ptr::null_mut();
    };
    let out = export(slice::from_raw_parts(
        px.as_ptr().cast::<u8>(),
        px.len() * 4,
    ))
    .cast::<u32>();
    if !out.is_null() {
        *ow = w as i32;
        *oh = h as i32;
    }
    out
}
#[no_mangle]
pub unsafe extern "C" fn img_thumb_jpeg(
    data: *const u8,
    len: usize,
    max: i32,
    olen: *mut usize,
) -> *mut u8 {
    if olen.is_null() {
        return ptr::null_mut();
    }
    *olen = 0;
    if max <= 0 {
        return ptr::null_mut();
    }
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    let Some((w, h, px)) = thumbnail(data, max as u32, max as u32) else {
        return ptr::null_mut();
    };
    let rgb: Vec<u8> = px
        .into_iter()
        .flat_map(|p| {
            let white = 255 - (p >> 24);
            [16, 8, 0].map(|s| (((p >> s) & 255) + white).min(255) as u8)
        })
        .collect();
    let Some(encoded) = images::encode(&rgb, w, h, 3, Some(88)) else {
        return ptr::null_mut();
    };
    let out = export(&encoded);
    if !out.is_null() {
        *olen = encoded.len();
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumbnail_preserves_aspect_and_premultiplies() {
        let rgba = [255, 0, 0, 128].repeat(8);
        let png = images::encode(&rgba, 4, 2, 4, None).unwrap();
        let (w, h, px) = thumbnail(&png, 2, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        assert!(px.iter().all(|p| *p == 0x80800000));
        assert!(thumbnail(&png, 0, 2).is_none());
    }
}
