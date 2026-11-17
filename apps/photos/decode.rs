//! Bounded decoding and premultiplied image pyramids. No media helper or UI calls.
use super::{
    io::{self, Entry},
    model,
};
use image::{AnimationDecoder, ImageDecoder, RgbaImage};
use std::{
    io::BufReader,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
pub const CACHE_BUDGET: usize = 256 << 20;
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}
pub struct Frame {
    pub opaque: bool,
    pub levels: Vec<Level>,
    pub delay: u64,
}
pub struct Photo {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
    pub bytes: usize,
}
impl Photo {
    pub fn frame(&self, elapsed: u64) -> usize {
        if self.frames.len() < 2 {
            return 0;
        }
        let total: u64 = self.frames.iter().map(|f| f.delay).sum();
        let mut t = elapsed % total.max(1);
        for (i, f) in self.frames.iter().enumerate() {
            if t < f.delay {
                return i;
            }
            t = t.saturating_sub(f.delay)
        }
        0
    }
    pub fn rgba(&self) -> RgbaImage {
        let l = &self.frames[0].levels[0];
        let mut bytes = Vec::with_capacity(l.pixels.len() * 4);
        for &v in &l.pixels {
            let a = v >> 24;
            for shift in [16, 8, 0] {
                bytes.push(if a == 0 {
                    0
                } else {
                    (((v >> shift & 255) * 255 + a / 2) / a).min(255) as u8
                })
            }
            bytes.push(a as u8)
        }
        RgbaImage::from_raw(self.width, self.height, bytes).unwrap()
    }
}
pub fn pyramid(image: RgbaImage, delay: u64) -> Frame {
    let (w, h) = image.dimensions();
    let mut first = Level {
        width: w,
        height: h,
        pixels: Vec::with_capacity(w as usize * h as usize),
    };
    let mut opaque = true;
    for p in image.pixels() {
        let [r, g, b, a] = p.0.map(u32::from);
        opaque &= a == 255;
        first.pixels.push(
            a << 24
                | ((r * a + 127) / 255) << 16
                | ((g * a + 127) / 255) << 8
                | ((b * a + 127) / 255),
        );
    }
    drop(image);
    let mut levels = vec![first];
    while levels.len() < 16 {
        let s = levels.last().unwrap();
        if s.width == 1 && s.height == 1 {
            break;
        }
        let (w, h) = (s.width.div_ceil(2), s.height.div_ceil(2));
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                let mut n = 0;
                for yy in y * 2..(y * 2 + 2).min(s.height) {
                    for xx in x * 2..(x * 2 + 2).min(s.width) {
                        let p = s.pixels[(yy * s.width + xx) as usize];
                        for (k, sh) in [0, 8, 16, 24].into_iter().enumerate() {
                            sum[k] += p >> sh & 255
                        }
                        n += 1;
                    }
                }
                let p = sum
                    .into_iter()
                    .enumerate()
                    .fold(0, |v, (k, s)| v | ((s + n / 2) / n) << (k * 8));
                pixels.push(p);
            }
        }
        levels.push(Level {
            width: w,
            height: h,
            pixels,
        });
    }
    Frame {
        opaque,
        levels,
        delay: delay.max(20),
    }
}
pub fn load(
    entry: &Entry,
    home: &Path,
    serial: u64,
    current: &AtomicU64,
) -> Result<Arc<Photo>, String> {
    let f = io::read(entry, home)?;
    let mut reader = image::ImageReader::new(BufReader::new(f))
        .with_guessed_format()
        .map_err(|_| "media.imageUnreadable")?;
    let format = reader.format();
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(192 << 20);
    reader.limits(limits.clone());
    let mut frames = vec![];
    let mut bytes = 0usize;
    if format == Some(image::ImageFormat::Gif) {
        let mut d = image::codecs::gif::GifDecoder::new(reader.into_inner())
            .map_err(|_| "media.imageUnreadable")?;
        d.set_limits(limits).map_err(|_| "photos.tooLarge")?;
        for frame in d.into_frames() {
            if current.load(Ordering::Relaxed) != serial {
                return Err("cancelled".into());
            }
            let f = frame.map_err(|_| "media.imageUnreadable")?;
            let (n, d) = f.delay().numer_denom_ms();
            let image = f.into_buffer();
            if image.width() == 0
                || image.height() == 0
                || image.width() as u64 * image.height() as u64 > io::MAX_PIXELS
                || frames.len() >= 1200
            {
                return Err("photos.tooLarge".into());
            }
            let frame = pyramid(image, n as u64 / d.max(1) as u64);
            bytes += frame
                .levels
                .iter()
                .map(|l| l.pixels.len() * 4)
                .sum::<usize>();
            if bytes > 192 << 20 {
                return Err("photos.tooLarge".into());
            }
            frames.push(frame);
        }
    } else {
        let image = match reader.into_decoder() {
            Ok(mut d) => {
                let (w, h) = d.dimensions();
                if w == 0
                    || h == 0
                    || w as u64 * h as u64 > io::MAX_PIXELS
                    || d.total_bytes() > 192 << 20
                {
                    return Err("photos.tooLarge".into());
                }
                let orientation = d
                    .orientation()
                    .unwrap_or(image::metadata::Orientation::NoTransforms);
                let mut image =
                    image::DynamicImage::from_decoder(d).map_err(|_| "media.imageUnreadable")?;
                image.apply_orientation(orientation);
                image.to_rgba8()
            }
            Err(_) => super::formats::decode(entry, home)?,
        };
        if current.load(Ordering::Relaxed) != serial {
            return Err("cancelled".into());
        }
        let frame = pyramid(image, 100);
        bytes = frame.levels.iter().map(|l| l.pixels.len() * 4).sum();
        frames.push(frame);
    }
    let first = frames.first().ok_or("media.imageUnreadable")?;
    let (width, height) = (first.levels[0].width, first.levels[0].height);
    if bytes > CACHE_BUDGET {
        return Err("photos.tooLarge".into());
    }
    Ok(Arc::new(Photo {
        width,
        height,
        frames,
        bytes,
    }))
}
pub fn edit(
    photo: &Photo,
    view: super::model::View,
    crop: Option<[u32; 4]>,
) -> Result<RgbaImage, String> {
    let src = photo.rgba();
    let (w, h) = view.size(photo.width, photo.height);
    let [x, y, cw, ch] = crop.unwrap_or([0, 0, w, h]);
    if cw == 0 || ch == 0 || x >= w || y >= h || cw > w - x || ch > h - y {
        return Err("media.invalidCrop".into());
    }
    let table = model::tone(view.brightness, view.contrast);
    let mut out = RgbaImage::new(cw, ch);
    for yy in 0..ch {
        for xx in 0..cw {
            let (sx, sy) =
                model::source_xy(view.rotation, photo.width, photo.height, x + xx, y + yy);
            let mut p = *src.get_pixel(sx, sy);
            for c in &mut p.0[..3] {
                *c = table[*c as usize]
            }
            out.put_pixel(xx, yy, p);
        }
    }
    Ok(out)
}
// Thumbnails decode a single frame; never build a full-resolution pyramid or
// retain the rest of a GIF just to show one small tile.
pub fn thumbnail(
    entry: &Entry,
    home: &Path,
    serial: u64,
    current: &AtomicU64,
) -> Result<super::render::Raster, String> {
    if current.load(Ordering::Relaxed) != serial {
        return Err("cancelled".into());
    }
    let f = io::read(entry, home)?;
    let mut reader = image::ImageReader::new(BufReader::new(f))
        .with_guessed_format()
        .map_err(|_| "media.imageUnreadable")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(192 << 20);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits.clone());
    let rgba = if reader.format() == Some(image::ImageFormat::Gif) {
        let mut d = image::codecs::gif::GifDecoder::new(reader.into_inner())
            .map_err(|_| "media.imageUnreadable")?;
        d.set_limits(limits).map_err(|_| "photos.tooLarge")?;
        d.into_frames()
            .next()
            .ok_or("media.imageUnreadable")?
            .map_err(|_| "media.imageUnreadable")?
            .into_buffer()
    } else {
        match reader.into_decoder() {
            Ok(mut d) => {
                let (w, h) = d.dimensions();
                if w == 0
                    || h == 0
                    || w as u64 * h as u64 > io::MAX_PIXELS
                    || d.total_bytes() > 192 << 20
                {
                    return Err("photos.tooLarge".into());
                }
                let orientation = d
                    .orientation()
                    .unwrap_or(image::metadata::Orientation::NoTransforms);
                let mut image =
                    image::DynamicImage::from_decoder(d).map_err(|_| "media.imageUnreadable")?;
                image.apply_orientation(orientation);
                image.to_rgba8()
            }
            Err(_) => super::formats::decode(entry, home)?,
        }
    };
    if rgba.width() == 0
        || rgba.height() == 0
        || rgba.width() as u64 * rgba.height() as u64 > io::MAX_PIXELS
        || current.load(Ordering::Relaxed) != serial
    {
        return Err("photos.tooLarge".into());
    }
    let small = image::imageops::thumbnail(&rgba, 192, 152);
    drop(rgba);
    let frame = pyramid(small, 100);
    let width = frame.levels[0].width;
    let height = frame.levels[0].height;
    let bytes = frame.levels.iter().map(|l| l.pixels.len() * 4).sum();
    let photo = Photo {
        width,
        height,
        frames: vec![frame],
        bytes,
    };
    super::render::render(
        &photo,
        super::model::View {
            width: 192,
            height: 152,
            ..Default::default()
        },
        0,
        serial,
        current,
    )
    .ok_or_else(|| "cancelled".into())
}
