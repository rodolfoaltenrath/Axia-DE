//! Snap Assist: after a window snaps, the rest of its layout is offered to the other windows. The
//! zones still free follow from the zone just taken; the candidates are drawn as live miniatures in
//! a grid inside the first free zone (the Overview's renderer, sdk/overview.jsx).
use super::{zone_rect, SnapRect, Zone};

/// At most this many candidates are shown in one zone.
pub const ASSIST_MAX: usize = 12;
const PAD: f64 = 24.0;
const GAP: f64 = 20.0;
/// The title row above each miniature (sdk/desktopmodel.js OVERVIEW_LABEL).
const LABEL: f64 = 30.0;

/// Where a candidate's miniature goes and its scale.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AssistPlace {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

/// The rest of the layout `zone` belongs to, in the order they are offered.
fn layout_rest(zone: Zone) -> &'static [Zone] {
    use Zone::*;
    match zone {
        Left => &[Right],
        Right => &[Left],
        Top => &[Bottom],
        Bottom => &[Top],
        TopLeft => &[TopRight, BottomLeft, BottomRight],
        TopRight => &[TopLeft, BottomLeft, BottomRight],
        BottomLeft => &[TopLeft, TopRight, BottomRight],
        BottomRight => &[TopLeft, TopRight, BottomLeft],
        LeftThird => &[CenterThird, RightThird],
        CenterThird => &[LeftThird, RightThird],
        RightThird => &[LeftThird, CenterThird],
        LeftTwoThirds => &[RightThird],
        RightTwoThirds => &[LeftThird],
        Max => &[],
    }
}

/// The zones of `zone`'s layout still free, given the zones other windows on that display hold.
/// `layout` is the layout the person picked (the snap layouts flyout), when it has `zone`; otherwise
/// the layout follows from the zone. A zone counts as taken when any held zone overlaps it.
pub fn assist_zones(zone: Zone, layout: &[Zone], occupied: &[Zone]) -> Vec<Zone> {
    // divisible by 2 and 3: the zones of one layout tile it exactly
    const REFERENCE: SnapRect = SnapRect::new(0.0, 0.0, 1200.0, 1200.0);
    let rest: Vec<Zone> = if layout.contains(&zone) {
        let mut rest: Vec<Zone> = Vec::new();
        for z in layout.iter().copied().filter(|z| *z != zone) {
            if !rest.contains(&z) {
                rest.push(z);
            }
        }
        rest
    } else {
        layout_rest(zone).to_vec()
    };
    rest.into_iter()
        .filter(|z| {
            let r = zone_rect(*z, REFERENCE);
            !occupied.iter().any(|o| zone_rect(*o, REFERENCE).overlaps(&r))
        })
        .collect()
}

/// The candidates' miniatures in `cell`, in rows, each scaled to fit with its title above it.
pub fn assist_grid(cell: SnapRect, frames: &[SnapRect]) -> Vec<AssistPlace> {
    let n = frames.len().min(ASSIST_MAX);
    if n == 0 || !cell.is_valid() {
        return Vec::new();
    }
    let cols = ((n as f64 * (cell.w / cell.h.max(1.0))) / 1.6).sqrt().ceil() as usize;
    let c = cols.clamp(1, n);
    let r = (n + c - 1) / c;
    let cw = ((cell.w - 2.0 * PAD - GAP * (c as f64 - 1.0)) / c as f64).max(1.0);
    let ch = ((cell.h - 2.0 * PAD - GAP * (r as f64 - 1.0)) / r as f64).max(LABEL + 1.0);
    frames[..n]
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let (col, row) = (i % c, i / c);
            let in_row = if row == r - 1 { n - row * c } else { c };
            let row_left = cell.x + PAD + ((c - in_row) as f64 * (cw + GAP)) / 2.0;
            let (fw, fh) = (f.w.max(1.0), f.h.max(1.0));
            let scale = (cw / fw).min((ch - LABEL) / fh).clamp(0.05, 0.85);
            let (sw, sh) = (fw * scale, fh * scale);
            AssistPlace {
                x: (row_left + col as f64 * (cw + GAP) + (cw - sw) / 2.0).round(),
                y: (cell.y + PAD + row as f64 * (ch + GAP) + LABEL + (ch - LABEL - sh) / 2.0).round(),
                w: sw,
                h: sh,
                scale,
            }
        })
        .collect()
}
