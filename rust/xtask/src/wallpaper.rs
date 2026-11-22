//! The four gradient wallpapers (Aurora, Ocean, Sunset, Graphite) of the "Gradients" category.
//!
//!   cargo xtask gen-wallpaper --palette aurora --output assets/wallpapers/gradients/aurora.png
//!         [--width 1920 --height 1200]
//!
//! Each picture is a base color with a slow vertical/horizontal tint plus a few soft light blobs.
//! `pixel` is the whole definition: the committed PNGs are exactly its output at 1920×1200 (a test
//! decodes them and compares every pixel), so the files can be regenerated at any size.

use crate::Result;
use std::path::Path;

/// A light blob: center (x, y) and radius in 0..1 of the picture, then its color.
type Blob = [f64; 6];

struct Palette {
    name: &'static str,
    base: [f64; 3],
    blobs: &'static [Blob],
}

const PALETTES: [Palette; 4] = [
    Palette {
        name: "aurora",
        base: [0.06, 0.05, 0.20],
        blobs: &[
            [0.18, 0.28, 0.55, 0.55, 0.22, 0.90],
            [0.82, 0.22, 0.45, 0.98, 0.33, 0.58],
            [0.62, 0.88, 0.60, 0.16, 0.45, 0.98],
            [0.05, 0.95, 0.40, 0.10, 0.75, 0.85],
        ],
    },
    Palette {
        name: "ocean",
        base: [0.02, 0.08, 0.16],
        blobs: &[
            [0.25, 0.20, 0.60, 0.05, 0.55, 0.85],
            [0.85, 0.60, 0.55, 0.05, 0.80, 0.70],
            [0.40, 0.95, 0.50, 0.10, 0.30, 0.75],
        ],
    },
    Palette {
        name: "sunset",
        base: [0.14, 0.04, 0.10],
        blobs: &[
            [0.20, 0.80, 0.60, 0.98, 0.45, 0.20],
            [0.75, 0.30, 0.55, 0.95, 0.25, 0.50],
            [0.50, 0.05, 0.45, 0.55, 0.20, 0.80],
        ],
    },
    Palette {
        name: "graphite",
        base: [0.08, 0.08, 0.09],
        blobs: &[[0.30, 0.30, 0.65, 0.35, 0.36, 0.40], [0.80, 0.80, 0.55, 0.22, 0.24, 0.30]],
    },
];

fn palette(name: &str) -> Result<&'static Palette> {
    PALETTES.iter().find(|p| p.name == name).ok_or_else(|| {
        let names: Vec<_> = PALETTES.iter().map(|p| p.name).collect();
        format!("unknown palette {name}; use one of {}", names.join(", "))
    })
}

/// One pixel at (u, v) in 0..1; `aspect` is width / height, so the blobs stay round.
fn pixel(p: &Palette, u: f64, v: f64, aspect: f64) -> [u8; 3] {
    let mut c = [p.base[0] + 0.08 * u, p.base[1] + 0.04 * v, p.base[2] + 0.14 * (1.0 - v) - 0.04];
    for blob in p.blobs {
        let (dx, dy) = ((u - blob[0]) * aspect, v - blob[1]);
        let k = 1.0 - (dx * dx + dy * dy).sqrt() / blob[2];
        if k > 0.0 {
            let k = k * k * 0.55;
            for (channel, light) in c.iter_mut().zip(&blob[3..]) {
                *channel += light * k;
            }
        }
    }
    // int(c * 255), capped at 255 (a negative channel saturates to 0)
    c.map(|v| (v * 255.0).min(255.0) as u8)
}

/// The picture as `width * height * 3` RGB bytes, row by row.
fn render(name: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let p = palette(name)?;
    if width == 0 || height == 0 {
        return Err("the size must not be zero".into());
    }
    let aspect = f64::from(width) / f64::from(height);
    let mut out = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            out.extend(pixel(p, f64::from(x) / f64::from(width), f64::from(y) / f64::from(height), aspect));
        }
    }
    Ok(out)
}

pub fn execute(args: &[String]) -> Result<()> {
    let values = crate::options(args, &["palette", "output", "width", "height"])?;
    let get = |name: &str| values.get(name).ok_or_else(|| format!("gen-wallpaper requires --{name}"));
    let size = |name: &str, default: u32| match values.get(name) {
        Some(text) => text.parse::<u32>().map_err(|_| format!("--{name} must be a whole number")),
        None => Ok(default),
    };
    let (name, output) = (get("palette")?, Path::new(get("output")?));
    let (width, height) = (size("width", 1920)?, size("height", 1200)?);
    let pixels = render(name, width, height)?;
    image::save_buffer(output, &pixels, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|error| format!("{}: {error}", output.display()))?;
    println!("{}: {width}x{height}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
    }

    #[test]
    fn the_committed_pictures_are_exactly_what_the_palettes_draw() {
        for p in &PALETTES {
            let file = repo().join(format!("assets/wallpapers/gradients/{}.png", p.name));
            let picture = image::open(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display())).to_rgb8();
            assert_eq!(picture.dimensions(), (1920, 1200), "{}", p.name);
            let drawn = render(p.name, 1920, 1200).unwrap();
            let at = picture.as_raw().iter().zip(&drawn).position(|(a, b)| a != b);
            assert_eq!(at, None, "{}: first pixel byte that differs", p.name);
        }
    }

    #[test]
    fn the_thumbnails_exist_and_are_small_for_every_palette() {
        for p in &PALETTES {
            let file = repo().join(format!("assets/wallpapers/thumbs/gradients/{}.jpg", p.name));
            let thumb = image::ImageReader::open(&file).unwrap().with_guessed_format().unwrap();
            assert_eq!(thumb.format(), Some(image::ImageFormat::Jpeg), "{}", p.name);
            assert!(std::fs::metadata(&file).unwrap().len() < 40_000, "{}", p.name);
        }
    }

    #[test]
    fn a_picture_keeps_its_size_and_the_palettes_differ() {
        let a = render("aurora", 64, 40).unwrap();
        assert_eq!(a.len(), 64 * 40 * 3);
        assert_ne!(a, render("ocean", 64, 40).unwrap());
        assert_eq!(a, render("aurora", 64, 40).unwrap());
        assert!(render("nope", 8, 8).unwrap_err().contains("aurora, ocean, sunset, graphite"));
        assert!(render("aurora", 0, 8).is_err());
    }

    #[test]
    fn without_light_a_pixel_is_the_base_color_plus_the_slow_tint() {
        let dark = Palette { name: "test", base: [0.08, 0.08, 0.09], blobs: &[] };
        assert_eq!(pixel(&dark, 0.0, 0.0, 1.6), [20, 20, 48]);
        // the far corner: red gains 0.08, green 0.04, blue loses its 0.14 term (values truncated)
        assert_eq!(pixel(&dark, 1.0, 1.0, 1.6), [(0.16 * 255.0) as u8, (0.12 * 255.0) as u8, ((0.09 - 0.04) * 255.0) as u8]);
    }
}
