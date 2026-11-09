//! Snap Assist: after a window snaps, the rest of its layout is offered to the other windows. The
//! zones still free follow from the zone just taken; the candidates are drawn as live miniatures
//! inside the first free zone (the Overview's renderer, sdk/overview.jsx, with winmodel's grid).
use super::{zone_rect, SnapRect, Zone};

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
