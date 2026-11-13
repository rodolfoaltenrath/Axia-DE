//! Borrowed renderer views. Only this layout crosses the C boundary; ownership
//! and tree edits live in `Scene`. C may update layout/paint fields on the UI
//! thread, but must never free these records or edit their links.
use std::ffi::{c_char, c_void};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Dim {
    pub v: f32,
    pub unit: u8,
}
impl Dim {
    pub fn same(self, other: Self) -> bool {
        self.unit == other.unit && self.v.to_bits() == other.v.to_bits()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub display_none: u8,
    pub absolute: u8,
    pub row: u8,
    pub justify: u8,
    pub align_items: u8,
    pub align_self: u8,
    pub overflow_hidden: u8,
    pub pointer_none: u8,
    pub text_align: u8,
    pub object_fit: u8,
    pub nowrap: u8,
    pub dir: u8,
    pub wrap: u8,
    pub mono: u8,
    pub cursor: u8,
    pub grow: f32,
    pub shrink: f32,
    pub basis: Dim,
    pub width: Dim,
    pub height: Dim,
    pub min_w: Dim,
    pub min_h: Dim,
    pub max_w: Dim,
    pub max_h: Dim,
    pub pos: [Dim; 4],
    pub margin: [f32; 4],
    pub padding: [f32; 4],
    pub gap: f32,
    pub bg: u32,
    pub border_color: u32,
    pub color: u32,
    pub shadow_color: u32,
    pub has_gradient: u8,
    pub text_overflow: u8,
    pub line_clamp: u8,
    pub grad_from: u32,
    pub grad_to: u32,
    pub grad_angle: f32,
    pub radius: f32,
    pub border_width: f32,
    pub opacity: f32,
    pub shadow_blur: f32,
    pub shadow_x: f32,
    pub shadow_y: f32,
    pub backdrop_blur: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub font_weight: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub z_index: f32,
    pub ext: *mut StyleExt,
}
impl Default for Style {
    fn default() -> Self {
        // All-zero is valid for this C record (only numbers and raw pointers).
        let mut s: Self = unsafe { std::mem::zeroed() };
        s.align_items = 1;
        s.object_fit = 1;
        s.opacity = 1.;
        s.color = 0xff000000;
        s.font_size = 14.;
        s.font_weight = 400.;
        s.grad_angle = 180.;
        s.text_align = 3;
        s
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ResolvedEdges {
    pub mg: [f32; 4],
    pub pd: [f32; 4],
    pub bw: [f32; 4],
    pub bc: [u32; 4],
    pub rad: [f32; 4],
    pub ok: u8,
    pub rtl: u8,
    pub plain: u8,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StyleExt {
    pub edge_set: u32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotate: f32,
    pub border_side: [f32; 6],
    pub border_side_color: [u32; 6],
    pub margin_inline: [f32; 2],
    pub padding_inline: [f32; 2],
    pub inset_inline: [Dim; 2],
    pub radius_corner: [f32; 8],
    pub origin_x: Dim,
    pub origin_y: Dim,
    pub r: ResolvedEdges,
}
impl Default for StyleExt {
    fn default() -> Self {
        Self {
            edge_set: 0,
            scale_x: 1.,
            scale_y: 1.,
            rotate: 0.,
            border_side: [0.; 6],
            border_side_color: [0; 6],
            margin_inline: [0.; 2],
            padding_inline: [0.; 2],
            inset_inline: [Dim::default(); 2],
            radius_corner: [0.; 8],
            origin_x: Dim { v: 50., unit: 2 },
            origin_y: Dim { v: 50., unit: 2 },
            r: ResolvedEdges::default(),
        }
    }
}
impl StyleExt {
    /// C snapshots initialize only the bytes before `r` (STYLE_EXT_VALUES).
    /// Never form a reference to the entire source record: its cache is uninitialized.
    ///
    /// # Safety
    /// `source` must hold initialized style values through the start of `r`.
    /// Padding in that prefix may be uninitialized, just as with a C memcpy.
    pub unsafe fn copy_values(source: *const Self) -> Self {
        let mut snapshot = Self::default();
        std::ptr::copy_nonoverlapping(
            source.cast::<std::mem::MaybeUninit<u8>>(),
            std::ptr::addr_of_mut!(snapshot).cast::<std::mem::MaybeUninit<u8>>(),
            std::mem::offset_of!(Self, r),
        );
        snapshot
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}
#[repr(C)]
#[derive(Default)]
pub struct NodeTf {
    pub own: Affine,
    pub world: Affine,
    pub inv: Affine,
    pub sig: u64,
    pub live: u8,
    pub live_known: u8,
}
#[repr(C)]
pub struct Node {
    pub id: i32,
    pub kind: i32,
    pub file_drop: u32,
    pub s: Style,
    pub text: *mut c_char,
    pub img: *mut c_void,
    pub term: i32,
    pub protect: u8,
    pub rtl: u8,
    pub own_tf: u8,
    pub has_tf: u8,
    pub layout_ok: u8,
    pub layout_rtl: u8,
    pub parent: *mut Node,
    pub first: *mut Node,
    pub last: *mut Node,
    pub next: *mut Node,
    pub prev: *mut Node,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub ax: f32,
    pub ay: f32,
    pub measure_valid: i32,
    pub measure_maxw: f32,
    pub measure_w: f32,
    pub measure_h: f32,
    pub paint_dirty: u8,
    pub gen: u32,
    pub prev_box: Rect,
    pub tf: *mut NodeTf,
}
impl Node {
    pub fn new(id: i32, kind: i32) -> Self {
        let mut n: Self = unsafe { std::mem::zeroed() };
        n.id = id;
        n.kind = kind;
        n.s = Style::default();
        n
    }
}
#[repr(C)]
#[derive(Default)]
pub struct DamageList {
    pub r: [Rect; 16],
    pub n: i32,
}
#[repr(C)]
pub struct Ui {
    pub nodes: *mut *mut Node,
    pub cap: i32,
    pub root: *mut Node,
    pub dirty: i32,
    pub width: f32,
    pub height: f32,
    pub theme_dir: [c_char; 1024],
    pub layout_ms: f64,
    pub paint_ms: f64,
    pub damage: Rect,
    pub dlist: DamageList,
    pub dlast: Rect,
    pub damage_px: f64,
    pub quit_requested: i32,
    pub rtl: u8,
    pub theme_request: [c_char; 1024],
    pub theme_failed: [c_char; 1024],
}
impl Ui {
    pub const fn empty() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
