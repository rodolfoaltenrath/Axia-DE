//! The Axia Store's drawings: sidebar icons, the editorial cards' artwork and the
//! fallback app icon colours. Everything is generated: no image assets.
use crate::native_view::paint::{icon, Image, Rgba, Shape};
use serde_json::{json, Value};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Search,
    Discover,
    Create,
    Work,
    Play,
    Develop,
    Categories,
    Updates,
    Installed,
}
impl Glyph {
    pub const NAV: [Self; 8] = [
        Self::Discover,
        Self::Create,
        Self::Work,
        Self::Play,
        Self::Develop,
        Self::Categories,
        Self::Updates,
        Self::Installed,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Discover => "discover",
            Self::Create => "create",
            Self::Work => "work",
            Self::Play => "play",
            Self::Develop => "develop",
            Self::Categories => "categories",
            Self::Updates => "updates",
            Self::Installed => "installed",
        }
    }
}
const S: f64 = 1.6;
fn star() -> Vec<(f64, f64)> {
    (0..10)
        .map(|i| {
            let a = std::f64::consts::PI * (i as f64) / 5. - std::f64::consts::FRAC_PI_2;
            let r = if i % 2 == 0 { 8.2 } else { 3.6 };
            (10. + r * a.cos(), 10.6 + r * a.sin())
        })
        .collect()
}
fn shapes(g: Glyph) -> Vec<Shape> {
    use Shape::*;
    match g {
        Glyph::Search => vec![
            Circle((8.5, 8.5), 5.2, 1.7),
            Line((12.4, 12.4), (16.6, 16.6), 2.1),
        ],
        Glyph::Discover => vec![Polygon(star())],
        Glyph::Create => vec![
            Line((5.6, 14.4), (14.6, 5.4), 3.2),
            Polygon(vec![(2.8, 17.2), (3.9, 13.4), (6.6, 16.1)]),
        ],
        Glyph::Work => vec![Polygon(vec![
            (2.4, 9.6),
            (17.6, 2.8),
            (12.2, 17.4),
            (9.6, 11.2),
        ])],
        Glyph::Play => vec![
            Polygon(vec![
                (10., 2.2),
                (13.2, 6.8),
                (13.2, 13.2),
                (6.8, 13.2),
                (6.8, 6.8),
            ]),
            Polygon(vec![(6.8, 9.6), (4.2, 14.8), (6.8, 13.6)]),
            Polygon(vec![(13.2, 9.6), (15.8, 14.8), (13.2, 13.6)]),
            Dot((10., 15.8), 1.5),
        ],
        Glyph::Develop => vec![
            Line((4.6, 15.6), (11.6, 8.6), 2.4),
            Polygon(vec![
                (9.4, 3.6),
                (14.6, 3.6),
                (17.2, 6.2),
                (13.4, 10.),
                (8.6, 6.6),
            ]),
        ],
        Glyph::Categories => [(3., 3.), (11., 3.), (3., 11.), (11., 11.)]
            .into_iter()
            .map(|(x, y)| FillRect(x, y, 6., 6., 1.6))
            .collect(),
        Glyph::Updates => vec![
            Circle((10., 10.), 7.6, S),
            Line((10., 5.6), (10., 12.6), S),
            Polygon(vec![(6.6, 10.4), (10., 14.), (13.4, 10.4)]),
        ],
        Glyph::Installed => vec![
            Rect(3., 3., 14., 14., 3.4, S),
            Line((6.6, 10.2), (9., 12.6), S),
            Line((9., 12.6), (13.6, 7.6), S),
        ],
    }
}
/// A 20-unit sidebar glyph at `size` pixels.
pub fn glyph(g: Glyph, size: usize, color: Rgba) -> Image {
    icon(&shapes(g), size, color)
}
/// A small deterministic generator, so a card keeps its artwork between frames.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}
pub fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}
const PALETTES: [[&str; 4]; 5] = [
    ["#3d8fd8", "#f2c94c", "#eb5757", "#6fcf97"],
    ["#f0735a", "#3b4b9a", "#fdd6a2", "#ffffff"],
    ["#5b3fd6", "#ff8fb1", "#ffd166", "#1fc8b4"],
    ["#1d9e8b", "#ffe08a", "#ff6b6b", "#2f3a8f"],
    ["#283046", "#ff9f43", "#54a0ff", "#feca57"],
];
fn color(hex: &str) -> Rgba {
    crate::native_view::paint::rgba(hex)
}
/// Editorial artwork: a colour field with bold geometric shapes, stable per `seed`.
pub fn hero(seed: u64, w: usize, h: usize) -> Image {
    let mut rng = Rng(seed | 1);
    let pal = PALETTES[(seed % PALETTES.len() as u64) as usize];
    let mut out = Image::new(w, h);
    out.fill(color(pal[0]));
    let (wf, hf) = (w as f64, h as f64);
    let unit = hf / 6.;
    // A field of small dots behind the shapes.
    let dots = color(&format!("{}55", pal[3]));
    let step = (unit * 0.7).max(6.);
    let (ox, oy) = (rng.next() * wf * 0.5, rng.next() * hf * 0.4);
    for i in 0..6 {
        for j in 0..4 {
            let c = (ox + i as f64 * step, oy + j as f64 * step);
            out.layer(&Shape::Dot(c, step * 0.12), dots);
        }
    }
    for k in 0..7 {
        let c = color(pal[1 + k % 3]);
        let (x, y) = (rng.next() * wf, rng.next() * hf);
        let size = unit * (0.6 + rng.next() * 1.6);
        let shape = match k % 4 {
            0 => Shape::Dot((x, y), size * 0.6),
            1 => Shape::Circle((x, y), size * 0.6, size * 0.22),
            2 => {
                // A rotated rounded square.
                let a = rng.next() * std::f64::consts::PI;
                let r = size * 0.7;
                Shape::Polygon(
                    (0..4)
                        .map(|i| {
                            let t = a + i as f64 * std::f64::consts::FRAC_PI_2;
                            (x + r * t.cos(), y + r * t.sin())
                        })
                        .collect(),
                )
            }
            _ => {
                // A zigzag stroke.
                let a = rng.next() * std::f64::consts::PI;
                let (dx, dy) = (a.cos() * size * 0.5, a.sin() * size * 0.5);
                let mut last = (x, y);
                for n in 0..4 {
                    // Each step goes along the direction and alternates to either side.
                    let side = if n % 2 == 0 { 0.6 } else { -0.6 };
                    let next = (last.0 + dx - dy * side, last.1 + dy + dx * side);
                    out.layer(&Shape::Line(last, next, size * 0.22), c);
                    last = next;
                }
                continue;
            }
        };
        out.layer(&shape, c);
    }
    out
}
/// The fallback icon colours for an app without a PNG, as `LetterIcon` (sdk/apps.js):
/// a hue from the name, light to dark.
pub fn letter_colors(name: &str) -> (String, String) {
    let hue = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32))
        % 360;
    let hsl = |l: f64| {
        let a = 0.55 * l.min(1. - l);
        let f = |n: f64| {
            let k = (n + hue as f64 / 30.) % 12.;
            l - a * (k - 3.).min(9. - k).min(1.).max(-1.)
        };
        format!(
            "#{:02x}{:02x}{:02x}",
            (f(0.) * 255.).round() as u8,
            (f(8.) * 255.).round() as u8,
            (f(4.) * 255.).round() as u8
        )
    };
    (hsl(0.52), hsl(0.36))
}
/// The Axia Store's own icon (Dock, launcher): a blue tile with a shopping bag.
pub fn store_icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(8., 512.)
    } else {
        32.
    };
    json!({"kind":"view","style":{"width":s,"height":s,"borderRadius":s*0.225,
    "background":"linear-gradient(180deg, #4fb4ff, #1463d6)","alignItems":"center","justifyContent":"center"},
    "children":[
        {"kind":"view","style":{"width":s*0.56,"height":s*0.46,"marginTop":s*0.1,"borderRadius":s*0.08,"backgroundColor":"#ffffff"}},
        {"kind":"view","style":{"position":"absolute","top":s*0.2,"left":s*0.34,"width":s*0.32,"height":s*0.26,
            "borderRadius":s*0.16,"borderWidth":(s*0.06).max(2.),"borderColor":"#ffffff"}},
        {"kind":"view","style":{"position":"absolute","top":s*0.5,"left":s*0.36,"width":s*0.28,"height":s*0.07,
            "borderRadius":s*0.035,"backgroundColor":"#1d72e0"}}
    ]})
}
