//! Typed property dispatch: no byte offsets or untyped writes into style memory.
use super::{
    memory,
    types::{Dim, Style, StyleExt},
};
use std::ffi::CStr;

enum Value<'a> {
    Dim(&'a mut Dim),
    Float(&'a mut f32),
    Color(&'a mut u32),
    Enum(&'a mut u8, &'static [&'static str]),
    Byte(&'a mut u8),
}
struct Property<'a> {
    value: Value<'a>,
    extension: Option<(&'a mut u32, u32, &'a mut u8)>,
}
impl Property<'_> {
    fn commit(&mut self) {
        if let Some((flags, bit, valid)) = &mut self.extension {
            **flags |= *bit;
            **valid = 0;
        }
    }
    fn set_number(mut self, value: f64) -> bool {
        match &mut self.value {
            Value::Dim(d) if (value as f32).is_finite() => {
                **d = Dim {
                    v: value as f32,
                    unit: 1,
                }
            }
            Value::Float(f) if (value as f32).is_finite() => **f = value as f32,
            Value::Color(c) => **c = (value as i64) as u32,
            Value::Byte(n) => **n = value.clamp(0., 255.) as u8,
            _ => return false,
        }
        self.commit();
        true
    }
    fn set_string(mut self, value: &CStr) -> bool {
        match &mut self.value {
            Value::Enum(n, values) => {
                let Some(i) = values.iter().position(|v| v.as_bytes() == value.to_bytes()) else {
                    return false;
                };
                **n = i as u8;
            }
            Value::Dim(d) if value.to_bytes() == b"auto" => **d = Dim::default(),
            Value::Dim(d) => {
                let Some((n, percent)) = parse_float(value) else {
                    return false;
                };
                **d = Dim {
                    v: n,
                    unit: if percent { 2 } else { 1 },
                };
            }
            Value::Float(f) => {
                let Some((n, _)) = parse_float(value) else {
                    return false;
                };
                **f = n;
            }
            Value::Byte(n) => {
                let mut end = std::ptr::null_mut();
                let number = unsafe { libc::strtol(value.as_ptr(), &mut end, 10) };
                if end == value.as_ptr().cast_mut() {
                    return false;
                }
                **n = number.clamp(0, 255) as u8;
            }
            _ => return false,
        }
        self.commit();
        true
    }
}
// Keep the existing finite numeric-prefix/px/% grammar; reject missing numbers,
// NaN and overflow before they can reach integer casts or the C renderer.
fn parse_float(value: &CStr) -> Option<(f32, bool)> {
    let mut end = std::ptr::null_mut();
    let number = unsafe { libc::strtof(value.as_ptr(), &mut end) };
    if end == value.as_ptr().cast_mut() || !number.is_finite() {
        return None;
    }
    Some((number, unsafe { *end } == b'%' as std::ffi::c_char))
}
impl Style {
    pub fn set_gradient(&mut self, angle: f64, from: f64, to: f64) -> bool {
        if !(angle as f32).is_finite() || !from.is_finite() || !to.is_finite() {
            return false;
        }
        self.grad_angle = angle as f32;
        self.grad_from = (from as i64) as u32;
        self.grad_to = (to as i64) as u32;
        self.has_gradient = 1;
        true
    }
    fn property(&mut self, key: &str) -> Option<Property<'_>> {
        let value = match key {
            "display" => Value::Enum(&mut self.display_none, &["flex", "none"]),
            "position" => Value::Enum(&mut self.absolute, &["relative", "absolute"]),
            "flexDirection" => Value::Enum(&mut self.row, &["column", "row"]),
            "flexWrap" => Value::Enum(&mut self.wrap, &["nowrap", "wrap"]),
            "fontFamily" => Value::Enum(&mut self.mono, &["sans-serif", "monospace", "brand"]),
            "cursor" => Value::Enum(
                &mut self.cursor,
                &[
                    "auto",
                    "default",
                    "pointer",
                    "text",
                    "move",
                    "grab",
                    "grabbing",
                    "not-allowed",
                    "wait",
                    "progress",
                    "crosshair",
                    "n-resize",
                    "s-resize",
                    "e-resize",
                    "w-resize",
                    "ne-resize",
                    "nw-resize",
                    "se-resize",
                    "sw-resize",
                    "ew-resize",
                    "ns-resize",
                    "nesw-resize",
                    "nwse-resize",
                ],
            ),
            "justifyContent" => Value::Enum(
                &mut self.justify,
                &[
                    "flex-start",
                    "center",
                    "flex-end",
                    "space-between",
                    "space-around",
                    "space-evenly",
                ],
            ),
            "alignItems" => Value::Enum(
                &mut self.align_items,
                &["auto", "stretch", "flex-start", "center", "flex-end"],
            ),
            "alignSelf" => Value::Enum(
                &mut self.align_self,
                &["auto", "stretch", "flex-start", "center", "flex-end"],
            ),
            "overflow" => Value::Enum(&mut self.overflow_hidden, &["visible", "hidden"]),
            "pointerEvents" => Value::Enum(&mut self.pointer_none, &["auto", "none", "box-none"]),
            "textAlign" => Value::Enum(
                &mut self.text_align,
                &["left", "center", "right", "start", "end"],
            ),
            "direction" => Value::Enum(&mut self.dir, &["inherit", "ltr", "rtl"]),
            "objectFit" => Value::Enum(&mut self.object_fit, &["fill", "cover", "contain"]),
            "whiteSpace" => Value::Enum(&mut self.nowrap, &["normal", "nowrap"]),
            "textOverflow" => Value::Enum(&mut self.text_overflow, &["clip", "ellipsis"]),
            "flexBasis" => Value::Dim(&mut self.basis),
            "width" => Value::Dim(&mut self.width),
            "height" => Value::Dim(&mut self.height),
            "minWidth" => Value::Dim(&mut self.min_w),
            "minHeight" => Value::Dim(&mut self.min_h),
            "maxWidth" => Value::Dim(&mut self.max_w),
            "maxHeight" => Value::Dim(&mut self.max_h),
            "left" => Value::Dim(&mut self.pos[0]),
            "top" => Value::Dim(&mut self.pos[1]),
            "right" => Value::Dim(&mut self.pos[2]),
            "bottom" => Value::Dim(&mut self.pos[3]),
            "flexGrow" => Value::Float(&mut self.grow),
            "flexShrink" => Value::Float(&mut self.shrink),
            "marginLeft" => Value::Float(&mut self.margin[0]),
            "marginTop" => Value::Float(&mut self.margin[1]),
            "marginRight" => Value::Float(&mut self.margin[2]),
            "marginBottom" => Value::Float(&mut self.margin[3]),
            "paddingLeft" => Value::Float(&mut self.padding[0]),
            "paddingTop" => Value::Float(&mut self.padding[1]),
            "paddingRight" => Value::Float(&mut self.padding[2]),
            "paddingBottom" => Value::Float(&mut self.padding[3]),
            "gap" => Value::Float(&mut self.gap),
            "borderRadius" => Value::Float(&mut self.radius),
            "borderWidth" => Value::Float(&mut self.border_width),
            "opacity" => Value::Float(&mut self.opacity),
            "shadowBlur" => Value::Float(&mut self.shadow_blur),
            "shadowOffsetX" => Value::Float(&mut self.shadow_x),
            "shadowOffsetY" => Value::Float(&mut self.shadow_y),
            "backdropBlur" => Value::Float(&mut self.backdrop_blur),
            "fontSize" => Value::Float(&mut self.font_size),
            "lineHeight" => Value::Float(&mut self.line_height),
            "fontWeight" => Value::Float(&mut self.font_weight),
            "translateX" => Value::Float(&mut self.translate_x),
            "translateY" => Value::Float(&mut self.translate_y),
            "zIndex" => Value::Float(&mut self.z_index),
            "backgroundColor" => Value::Color(&mut self.bg),
            "borderColor" => Value::Color(&mut self.border_color),
            "color" => Value::Color(&mut self.color),
            "shadowColor" => Value::Color(&mut self.shadow_color),
            "numberOfLines" => Value::Byte(&mut self.line_clamp),

            _ => return self.extended_property(key),
        };
        Some(Property {
            value,
            extension: None,
        })
    }
    fn extended_property(&mut self, key: &str) -> Option<Property<'_>> {
        if !matches!(
            key,
            "borderLeftWidth"
                | "borderLeftColor"
                | "borderTopWidth"
                | "borderTopColor"
                | "borderRightWidth"
                | "borderRightColor"
                | "borderBottomWidth"
                | "borderBottomColor"
                | "borderStartWidth"
                | "borderStartColor"
                | "borderEndWidth"
                | "borderEndColor"
                | "borderTopLeftRadius"
                | "borderTopRightRadius"
                | "borderBottomRightRadius"
                | "borderBottomLeftRadius"
                | "borderTopStartRadius"
                | "borderTopEndRadius"
                | "borderBottomStartRadius"
                | "borderBottomEndRadius"
                | "marginStart"
                | "paddingStart"
                | "marginEnd"
                | "paddingEnd"
                | "start"
                | "end"
                | "scaleX"
                | "scaleY"
                | "rotate"
                | "transformOriginX"
                | "transformOriginY"
        ) {
            return None;
        }
        let x = self.extension()?;
        let (value, bit) = match key {
            "borderLeftWidth" => (Value::Float(&mut x.border_side[0]), 1 << 0),
            "borderLeftColor" => (Value::Color(&mut x.border_side_color[0]), 1 << 6),
            "borderTopWidth" => (Value::Float(&mut x.border_side[1]), 1 << 1),
            "borderTopColor" => (Value::Color(&mut x.border_side_color[1]), 1 << 7),
            "borderRightWidth" => (Value::Float(&mut x.border_side[2]), 1 << 2),
            "borderRightColor" => (Value::Color(&mut x.border_side_color[2]), 1 << 8),
            "borderBottomWidth" => (Value::Float(&mut x.border_side[3]), 1 << 3),
            "borderBottomColor" => (Value::Color(&mut x.border_side_color[3]), 1 << 9),
            "borderStartWidth" => (Value::Float(&mut x.border_side[4]), 1 << 4),
            "borderStartColor" => (Value::Color(&mut x.border_side_color[4]), 1 << 10),
            "borderEndWidth" => (Value::Float(&mut x.border_side[5]), 1 << 5),
            "borderEndColor" => (Value::Color(&mut x.border_side_color[5]), 1 << 11),
            "borderTopLeftRadius" => (Value::Float(&mut x.radius_corner[0]), 1 << 16),
            "borderTopRightRadius" => (Value::Float(&mut x.radius_corner[1]), 1 << 17),
            "borderBottomRightRadius" => (Value::Float(&mut x.radius_corner[2]), 1 << 18),
            "borderBottomLeftRadius" => (Value::Float(&mut x.radius_corner[3]), 1 << 19),
            "borderTopStartRadius" => (Value::Float(&mut x.radius_corner[4]), 1 << 20),
            "borderTopEndRadius" => (Value::Float(&mut x.radius_corner[5]), 1 << 21),
            "borderBottomStartRadius" => (Value::Float(&mut x.radius_corner[6]), 1 << 22),
            "borderBottomEndRadius" => (Value::Float(&mut x.radius_corner[7]), 1 << 23),
            "marginStart" => (Value::Float(&mut x.margin_inline[0]), 1 << 12),
            "paddingStart" => (Value::Float(&mut x.padding_inline[0]), 1 << 14),
            "marginEnd" => (Value::Float(&mut x.margin_inline[1]), 1 << 13),
            "paddingEnd" => (Value::Float(&mut x.padding_inline[1]), 1 << 15),
            "start" => (Value::Dim(&mut x.inset_inline[0]), 0),
            "end" => (Value::Dim(&mut x.inset_inline[1]), 0),
            "scaleX" => (Value::Float(&mut x.scale_x), 0),
            "scaleY" => (Value::Float(&mut x.scale_y), 0),
            "rotate" => (Value::Float(&mut x.rotate), 0),
            "transformOriginX" => (Value::Dim(&mut x.origin_x), 0),
            "transformOriginY" => (Value::Dim(&mut x.origin_y), 0),
            _ => unreachable!(),
        };
        Some(Property {
            value,
            extension: Some((&mut x.edge_set, bit, &mut x.r.ok)),
        })
    }
    fn extension(&mut self) -> Option<&mut StyleExt> {
        if self.ext.is_null() {
            self.ext = Box::into_raw(memory::try_box(StyleExt::default())?);
        }
        // SAFETY: ext is created here and freed only by its owner, on the UI thread.
        Some(unsafe { &mut *self.ext })
    }
    pub fn set_number(&mut self, key: &str, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        let fresh = self.ext.is_null();
        let changed = self.property(key).is_some_and(|p| p.set_number(value));
        if fresh && !changed {
            unsafe {
                self.release();
            }
        }
        changed
    }
    pub fn set_string(&mut self, key: &str, value: &CStr) -> bool {
        let fresh = self.ext.is_null();
        let changed = self.property(key).is_some_and(|p| p.set_string(value));
        if fresh && !changed {
            unsafe {
                self.release();
            }
        }
        changed
    }
    pub fn reset(&mut self, key: &str) -> bool {
        let d = Style::default();
        match key {
            "display" => self.display_none = d.display_none,
            "position" => self.absolute = d.absolute,
            "flexDirection" => self.row = d.row,
            "flexWrap" => self.wrap = d.wrap,
            "fontFamily" => self.mono = d.mono,
            "cursor" => self.cursor = d.cursor,
            "justifyContent" => self.justify = d.justify,
            "alignItems" => self.align_items = d.align_items,
            "alignSelf" => self.align_self = d.align_self,
            "overflow" => self.overflow_hidden = d.overflow_hidden,
            "pointerEvents" => self.pointer_none = d.pointer_none,
            "textAlign" => self.text_align = d.text_align,
            "direction" => self.dir = d.dir,
            "objectFit" => self.object_fit = d.object_fit,
            "whiteSpace" => self.nowrap = d.nowrap,
            "textOverflow" => self.text_overflow = d.text_overflow,
            "flexBasis" => self.basis = d.basis,
            "width" => self.width = d.width,
            "height" => self.height = d.height,
            "minWidth" => self.min_w = d.min_w,
            "minHeight" => self.min_h = d.min_h,
            "maxWidth" => self.max_w = d.max_w,
            "maxHeight" => self.max_h = d.max_h,
            "left" => self.pos[0] = d.pos[0],
            "top" => self.pos[1] = d.pos[1],
            "right" => self.pos[2] = d.pos[2],
            "bottom" => self.pos[3] = d.pos[3],
            "flexGrow" => self.grow = d.grow,
            "flexShrink" => self.shrink = d.shrink,
            "marginLeft" => self.margin[0] = d.margin[0],
            "marginTop" => self.margin[1] = d.margin[1],
            "marginRight" => self.margin[2] = d.margin[2],
            "marginBottom" => self.margin[3] = d.margin[3],
            "paddingLeft" => self.padding[0] = d.padding[0],
            "paddingTop" => self.padding[1] = d.padding[1],
            "paddingRight" => self.padding[2] = d.padding[2],
            "paddingBottom" => self.padding[3] = d.padding[3],
            "gap" => self.gap = d.gap,
            "borderRadius" => self.radius = d.radius,
            "borderWidth" => self.border_width = d.border_width,
            "opacity" => self.opacity = d.opacity,
            "shadowBlur" => self.shadow_blur = d.shadow_blur,
            "shadowOffsetX" => self.shadow_x = d.shadow_x,
            "shadowOffsetY" => self.shadow_y = d.shadow_y,
            "backdropBlur" => self.backdrop_blur = d.backdrop_blur,
            "fontSize" => self.font_size = d.font_size,
            "lineHeight" => self.line_height = d.line_height,
            "fontWeight" => self.font_weight = d.font_weight,
            "translateX" => self.translate_x = d.translate_x,
            "translateY" => self.translate_y = d.translate_y,
            "zIndex" => self.z_index = d.z_index,
            "backgroundColor" => self.bg = d.bg,
            "borderColor" => self.border_color = d.border_color,
            "color" => self.color = d.color,
            "shadowColor" => self.shadow_color = d.shadow_color,
            "numberOfLines" => self.line_clamp = d.line_clamp,
            _ => {
                if !matches!(
                    key,
                    "borderLeftWidth"
                        | "borderLeftColor"
                        | "borderTopWidth"
                        | "borderTopColor"
                        | "borderRightWidth"
                        | "borderRightColor"
                        | "borderBottomWidth"
                        | "borderBottomColor"
                        | "borderStartWidth"
                        | "borderStartColor"
                        | "borderEndWidth"
                        | "borderEndColor"
                        | "borderTopLeftRadius"
                        | "borderTopRightRadius"
                        | "borderBottomRightRadius"
                        | "borderBottomLeftRadius"
                        | "borderTopStartRadius"
                        | "borderTopEndRadius"
                        | "borderBottomStartRadius"
                        | "borderBottomEndRadius"
                        | "marginStart"
                        | "paddingStart"
                        | "marginEnd"
                        | "paddingEnd"
                        | "start"
                        | "end"
                        | "scaleX"
                        | "scaleY"
                        | "rotate"
                        | "transformOriginX"
                        | "transformOriginY"
                ) {
                    return false;
                }
                if self.ext.is_null() {
                    return true;
                }
                let x = unsafe { &mut *self.ext };
                let d = StyleExt::default();
                match key {
                    "borderLeftWidth" => {
                        x.edge_set &= !(1 << 0);
                        x.border_side[0] = d.border_side[0];
                    }
                    "borderLeftColor" => {
                        x.edge_set &= !(1 << 6);
                        x.border_side_color[0] = d.border_side_color[0];
                    }
                    "borderTopWidth" => {
                        x.edge_set &= !(1 << 1);
                        x.border_side[1] = d.border_side[1];
                    }
                    "borderTopColor" => {
                        x.edge_set &= !(1 << 7);
                        x.border_side_color[1] = d.border_side_color[1];
                    }
                    "borderRightWidth" => {
                        x.edge_set &= !(1 << 2);
                        x.border_side[2] = d.border_side[2];
                    }
                    "borderRightColor" => {
                        x.edge_set &= !(1 << 8);
                        x.border_side_color[2] = d.border_side_color[2];
                    }
                    "borderBottomWidth" => {
                        x.edge_set &= !(1 << 3);
                        x.border_side[3] = d.border_side[3];
                    }
                    "borderBottomColor" => {
                        x.edge_set &= !(1 << 9);
                        x.border_side_color[3] = d.border_side_color[3];
                    }
                    "borderStartWidth" => {
                        x.edge_set &= !(1 << 4);
                        x.border_side[4] = d.border_side[4];
                    }
                    "borderStartColor" => {
                        x.edge_set &= !(1 << 10);
                        x.border_side_color[4] = d.border_side_color[4];
                    }
                    "borderEndWidth" => {
                        x.edge_set &= !(1 << 5);
                        x.border_side[5] = d.border_side[5];
                    }
                    "borderEndColor" => {
                        x.edge_set &= !(1 << 11);
                        x.border_side_color[5] = d.border_side_color[5];
                    }
                    "borderTopLeftRadius" => {
                        x.edge_set &= !(1 << 16);
                        x.radius_corner[0] = d.radius_corner[0];
                    }
                    "borderTopRightRadius" => {
                        x.edge_set &= !(1 << 17);
                        x.radius_corner[1] = d.radius_corner[1];
                    }
                    "borderBottomRightRadius" => {
                        x.edge_set &= !(1 << 18);
                        x.radius_corner[2] = d.radius_corner[2];
                    }
                    "borderBottomLeftRadius" => {
                        x.edge_set &= !(1 << 19);
                        x.radius_corner[3] = d.radius_corner[3];
                    }
                    "borderTopStartRadius" => {
                        x.edge_set &= !(1 << 20);
                        x.radius_corner[4] = d.radius_corner[4];
                    }
                    "borderTopEndRadius" => {
                        x.edge_set &= !(1 << 21);
                        x.radius_corner[5] = d.radius_corner[5];
                    }
                    "borderBottomStartRadius" => {
                        x.edge_set &= !(1 << 22);
                        x.radius_corner[6] = d.radius_corner[6];
                    }
                    "borderBottomEndRadius" => {
                        x.edge_set &= !(1 << 23);
                        x.radius_corner[7] = d.radius_corner[7];
                    }
                    "marginStart" => {
                        x.edge_set &= !(1 << 12);
                        x.margin_inline[0] = d.margin_inline[0];
                    }
                    "paddingStart" => {
                        x.edge_set &= !(1 << 14);
                        x.padding_inline[0] = d.padding_inline[0];
                    }
                    "marginEnd" => {
                        x.edge_set &= !(1 << 13);
                        x.margin_inline[1] = d.margin_inline[1];
                    }
                    "paddingEnd" => {
                        x.edge_set &= !(1 << 15);
                        x.padding_inline[1] = d.padding_inline[1];
                    }
                    "start" => {
                        x.edge_set &= !(0);
                        x.inset_inline[0] = d.inset_inline[0];
                    }
                    "end" => {
                        x.edge_set &= !(0);
                        x.inset_inline[1] = d.inset_inline[1];
                    }
                    "scaleX" => {
                        x.edge_set &= !(0);
                        x.scale_x = d.scale_x;
                    }
                    "scaleY" => {
                        x.edge_set &= !(0);
                        x.scale_y = d.scale_y;
                    }
                    "rotate" => {
                        x.edge_set &= !(0);
                        x.rotate = d.rotate;
                    }
                    "transformOriginX" => {
                        x.edge_set &= !(0);
                        x.origin_x = d.origin_x;
                    }
                    "transformOriginY" => {
                        x.edge_set &= !(0);
                        x.origin_y = d.origin_y;
                    }
                    _ => unreachable!(),
                }
                x.r.ok = 0;
            }
        }
        true
    }
    /// Only the owner of a style may call this; snapshots are borrowed copies.
    pub unsafe fn release(&mut self) {
        if !self.ext.is_null() {
            drop(Box::from_raw(self.ext));
            self.ext = std::ptr::null_mut();
        }
    }
    pub fn same_base(&self, b: &Self) -> bool {
        self.display_none == b.display_none
            && self.absolute == b.absolute
            && self.row == b.row
            && self.wrap == b.wrap
            && self.mono == b.mono
            && self.cursor == b.cursor
            && self.justify == b.justify
            && self.align_items == b.align_items
            && self.align_self == b.align_self
            && self.overflow_hidden == b.overflow_hidden
            && self.pointer_none == b.pointer_none
            && self.text_align == b.text_align
            && self.dir == b.dir
            && self.object_fit == b.object_fit
            && self.nowrap == b.nowrap
            && self.text_overflow == b.text_overflow
            && self.basis.same(b.basis)
            && self.width.same(b.width)
            && self.height.same(b.height)
            && self.min_w.same(b.min_w)
            && self.min_h.same(b.min_h)
            && self.max_w.same(b.max_w)
            && self.max_h.same(b.max_h)
            && self.pos[0].same(b.pos[0])
            && self.pos[1].same(b.pos[1])
            && self.pos[2].same(b.pos[2])
            && self.pos[3].same(b.pos[3])
            && self.grow.to_bits() == b.grow.to_bits()
            && self.shrink.to_bits() == b.shrink.to_bits()
            && self.margin[0].to_bits() == b.margin[0].to_bits()
            && self.margin[1].to_bits() == b.margin[1].to_bits()
            && self.margin[2].to_bits() == b.margin[2].to_bits()
            && self.margin[3].to_bits() == b.margin[3].to_bits()
            && self.padding[0].to_bits() == b.padding[0].to_bits()
            && self.padding[1].to_bits() == b.padding[1].to_bits()
            && self.padding[2].to_bits() == b.padding[2].to_bits()
            && self.padding[3].to_bits() == b.padding[3].to_bits()
            && self.gap.to_bits() == b.gap.to_bits()
            && self.radius.to_bits() == b.radius.to_bits()
            && self.border_width.to_bits() == b.border_width.to_bits()
            && self.opacity.to_bits() == b.opacity.to_bits()
            && self.shadow_blur.to_bits() == b.shadow_blur.to_bits()
            && self.shadow_x.to_bits() == b.shadow_x.to_bits()
            && self.shadow_y.to_bits() == b.shadow_y.to_bits()
            && self.backdrop_blur.to_bits() == b.backdrop_blur.to_bits()
            && self.font_size.to_bits() == b.font_size.to_bits()
            && self.line_height.to_bits() == b.line_height.to_bits()
            && self.font_weight.to_bits() == b.font_weight.to_bits()
            && self.translate_x.to_bits() == b.translate_x.to_bits()
            && self.translate_y.to_bits() == b.translate_y.to_bits()
            && self.z_index.to_bits() == b.z_index.to_bits()
            && self.bg == b.bg
            && self.border_color == b.border_color
            && self.color == b.color
            && self.shadow_color == b.shadow_color
            && self.line_clamp == b.line_clamp
            && self.has_gradient == b.has_gradient
            && self.grad_from == b.grad_from
            && self.grad_to == b.grad_to
            && self.grad_angle.to_bits() == b.grad_angle.to_bits()
            && self.ext == b.ext
    }
}
impl StyleExt {
    pub fn same_values(&self, b: &Self, ignore_transform: bool) -> bool {
        self.edge_set == b.edge_set
            && self.border_side.map(f32::to_bits) == b.border_side.map(f32::to_bits)
            && self.border_side_color == b.border_side_color
            && self.margin_inline.map(f32::to_bits) == b.margin_inline.map(f32::to_bits)
            && self.padding_inline.map(f32::to_bits) == b.padding_inline.map(f32::to_bits)
            && self.radius_corner.map(f32::to_bits) == b.radius_corner.map(f32::to_bits)
            && self
                .inset_inline
                .iter()
                .zip(&b.inset_inline)
                .all(|(a, b)| a.same(*b))
            && (ignore_transform
                || (self.scale_x.to_bits() == b.scale_x.to_bits()
                    && self.scale_y.to_bits() == b.scale_y.to_bits()
                    && self.rotate.to_bits() == b.rotate.to_bits()
                    && self.origin_x.same(b.origin_x)
                    && self.origin_y.same(b.origin_y)))
    }
}
