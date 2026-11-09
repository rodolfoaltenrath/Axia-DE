//! Monitor palette. The mode follows the system theme sent by the host; data colors
//! stay the same in both modes so a series keeps its meaning.
use serde_json::Value;
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Content area behind the cards.
    pub bg: &'static str,
    pub sidebar: &'static str,
    pub card: &'static str,
    pub text: &'static str,
    pub dim: &'static str,
    pub line: &'static str,
    /// Selected sidebar entry and inline chips.
    pub selected: &'static str,
    /// Bar tracks and empty chart areas.
    pub track: &'static str,
    pub shadow: &'static str,
    /// RGBA used by rasters (icons, rings) for text-coloured strokes.
    pub ink_dim: [u8; 4],
    pub ring_track: [u8; 4],
}
pub const KERNEL: &str = "#e5484d";
pub const APPS: &str = "#f5b324";
pub const CACHE: &str = "#3b8ff6";
pub const FREE: &str = "#34c759";
pub const OTHER: &str = "#8e8e93";
pub const CPU: &str = "#d55bd6";
pub const DISK: &str = "#2cc9b5";
pub const RX: &str = "#3b8ff6";
pub const TX: &str = "#f5853d";
pub fn rgba(hex: &str) -> [u8; 4] {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    [
        byte(0),
        byte(2),
        byte(4),
        if h.len() >= 8 { byte(6) } else { 255 },
    ]
}
pub fn palette(context: &Value) -> Palette {
    if context["theme"]["mode"] == "light" {
        Palette {
            dark: false,
            bg: "#efeef3",
            sidebar: "#e4e3e9",
            card: "#ffffff",
            text: "#1d1d1f",
            dim: "#6e6e76",
            line: "#e2e1e7",
            selected: "#d3d2da",
            track: "#e8e7ed",
            shadow: "#0000001f",
            ink_dim: [110, 110, 118, 255],
            ring_track: [0, 0, 0, 22],
        }
    } else {
        Palette {
            dark: true,
            bg: "#1b1b1e",
            sidebar: "#232327",
            card: "#2a2a2f",
            text: "#f2f2f5",
            dim: "#9d9da6",
            line: "#38383f",
            selected: "#3a3a42",
            track: "#3a3a41",
            shadow: "#00000066",
            ink_dim: [157, 157, 166, 255],
            ring_track: [255, 255, 255, 26],
        }
    }
}
