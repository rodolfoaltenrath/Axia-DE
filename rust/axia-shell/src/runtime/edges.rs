//! Direction-dependent physical edges. Logical overrides win over physical ones.
use super::types::Node;
#[derive(Clone, Copy)]
pub struct Edges {
    pub margin: [f32; 4],
    pub padding: [f32; 4],
    pub border: [f32; 4],
}
impl Edges {
    pub fn of(n: &Node) -> Self {
        if let Some(x) = unsafe { n.s.ext.as_ref() } {
            Self {
                margin: x.r.mg,
                padding: x.r.pd,
                border: x.r.bw,
            }
        } else {
            Self {
                margin: n.s.margin,
                padding: n.s.padding,
                border: [n.s.border_width; 4],
            }
        }
    }
    pub fn horizontal(self) -> f32 {
        self.padding[0] + self.padding[2] + self.border[0] + self.border[2]
    }
    pub fn vertical(self) -> f32 {
        self.padding[1] + self.padding[3] + self.border[1] + self.border[3]
    }
}
pub fn resolve(n: &mut Node) {
    let s = &n.s;
    let Some(x) = (unsafe { s.ext.as_mut() }) else {
        return;
    };
    x.r.mg = s.margin;
    x.r.pd = s.padding;
    x.r.bw = [s.border_width; 4];
    x.r.bc = [s.border_color; 4];
    x.r.rad = [s.radius; 4];
    let (start, end) = if n.rtl != 0 { (2, 0) } else { (0, 2) };
    for (i, side) in [0, 1, 2, 3, start, end].into_iter().enumerate() {
        if x.edge_set & (1 << i) != 0 {
            x.r.bw[side] = x.border_side[i];
        }
        if x.edge_set & (1 << (6 + i)) != 0 {
            x.r.bc[side] = x.border_side_color[i];
        }
    }
    for (i, side) in [start, end].into_iter().enumerate() {
        if x.edge_set & (1 << (12 + i)) != 0 {
            x.r.mg[side] = x.margin_inline[i];
        }
        if x.edge_set & (1 << (14 + i)) != 0 {
            x.r.pd[side] = x.padding_inline[i];
        }
    }
    let corners = if n.rtl != 0 {
        [0, 1, 2, 3, 1, 0, 2, 3]
    } else {
        [0, 1, 2, 3, 0, 1, 3, 2]
    };
    for (i, corner) in corners.into_iter().enumerate() {
        if x.edge_set & (1 << (16 + i)) != 0 {
            x.r.rad[corner] = x.radius_corner[i];
        }
    }
    x.r.plain = u8::from(
        (1..4)
            .all(|i| x.r.bw[i] == x.r.bw[0] && x.r.bc[i] == x.r.bc[0] && x.r.rad[i] == x.r.rad[0]),
    );
    x.r.rtl = n.rtl;
    x.r.ok = 1;
}
#[no_mangle]
pub unsafe extern "C" fn node_resolve_edges(n: *mut Node) {
    if let Some(n) = n.as_mut() {
        resolve(n);
    }
}
