use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

pub fn decode(data: &[u8], max_side: u32, max_pixels: u64) -> Option<image::RgbaImage> {
    if data.is_empty() || data.len() > 128 * 1024 * 1024 || max_side == 0 {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format()?,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Bmp
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(max_pixels.saturating_mul(16).min(512 * 1024 * 1024));
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > max_pixels {
        return None;
    }
    Some(DynamicImage::from_decoder(decoder).ok()?.into_rgba8())
}

pub(crate) fn encode(
    data: &[u8],
    w: u32,
    h: u32,
    channels: u8,
    jpeg_quality: Option<u8>,
) -> Option<Vec<u8>> {
    let color = match channels {
        3 => image::ExtendedColorType::Rgb8,
        4 => image::ExtendedColorType::Rgba8,
        _ => return None,
    };
    if w == 0
        || h == 0
        || u64::from(w) * u64::from(h) > 120_000_000
        || data.len()
            != (w as usize)
                .checked_mul(h as usize)?
                .checked_mul(channels as usize)?
    {
        return None;
    }
    let mut out = Vec::new();
    if let Some(q) = jpeg_quality {
        // JPEG has no alpha; flatten RGBA over white before encoding.
        let rgb;
        let data = if channels == 4 {
            rgb = data
                .chunks_exact(4)
                .flat_map(|p| {
                    let a = u32::from(p[3]);
                    [0, 1, 2].map(|c| ((u32::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8)
                })
                .collect::<Vec<_>>();
            &rgb
        } else {
            data
        };
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q.clamp(1, 100))
            .encode(data, w, h, image::ExtendedColorType::Rgb8)
            .ok()?;
    } else {
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(data, w, h, color)
            .ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_png_roundtrip_and_limits() {
        let rgba = [255, 0, 0, 128, 0, 255, 0, 255];
        let png = encode(&rgba, 2, 1, 4, None).unwrap();
        assert_eq!(decode(&png, 2, 2).unwrap().as_raw(), &rgba);
        assert!(decode(&png, 1, 2).is_none());
        assert!(decode(&png, 2, 1).is_none());
        assert!(decode(&png[..png.len() / 2], 2, 2).is_none());
    }
    #[test]
    fn jpeg_flattens_transparency_over_white() {
        let jpeg = encode(&[0, 0, 0, 0], 1, 1, 4, Some(95)).unwrap();
        let decoded = decode(&jpeg, 1, 1).unwrap();
        assert!(decoded.get_pixel(0, 0).0[..3].iter().all(|v| *v >= 250));
    }
    #[test]
    fn rejects_garbage() {
        for len in 0..256 {
            assert!(decode(&vec![255; len], 8192, 40_000_000).is_none());
        }
    }
}
