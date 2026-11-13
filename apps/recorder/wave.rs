//! The original RGBA waveform, generated in Rust with bounded native dimensions.
pub fn draw(
    peaks: &[f64],
    width: usize,
    height: usize,
    dark: bool,
    played: f64,
    sel: Option<[f64; 2]>,
) -> Vec<u8> {
    let mut out = vec![0; width * height * 4];
    let top = peaks.iter().copied().fold(50., f64::max);
    let color = if dark { [255, 105, 97] } else { [224, 53, 43] };
    for x in 0..width {
        let f = x as f64 / width as f64;
        let v = peaks
            .get((f * peaks.len() as f64).floor() as usize)
            .copied()
            .unwrap_or(0.)
            / top;
        let bh = (v * (height.saturating_sub(2)) as f64).round().max(1.) as usize;
        let y0 = ((height as f64 - bh as f64) / 2.).round().max(0.) as usize;
        let inside = sel.is_none_or(|s| f >= s[0] && f <= s[1]);
        let alpha = if x % 3 == 2 {
            0
        } else if f < played {
            255
        } else if inside {
            170
        } else {
            60
        };
        for y in y0..(y0 + bh).min(height) {
            let i = (y * width + x) * 4;
            out[i..i + 3].copy_from_slice(&color);
            out[i + 3] = alpha;
        }
    }
    out
}
