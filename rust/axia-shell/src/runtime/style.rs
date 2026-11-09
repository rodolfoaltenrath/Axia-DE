//! Typed property dispatch: no byte offsets or untyped writes into style memory.
use super::types::{Dim, Style, StyleExt};
use std::ffi::CStr;

enum Property<'a> {
    Dim(&'a mut Dim),
    Float(&'a mut f32),
    Color(&'a mut u32),
    Enum(&'a mut u8, &'static [&'static str]),
    Byte(&'a mut u8),
}
impl Style {
    fn property(&mut self, key: &str) -> Option<Property<'_>> {
        Some(match key {
            "display" => Property::Enum(&mut self.display_none, &["flex", "none"]),
            "position" => Property::Enum(&mut self.absolute, &["relative", "absolute"]),
            "flexDirection" => Property::Enum(&mut self.row, &["column", "row"]),
            "flexWrap" => Property::Enum(&mut self.wrap, &["nowrap", "wrap"]),
            "fontFamily" => Property::Enum(&mut self.mono, &["sans-serif", "monospace", "brand"]),
            "cursor" => Property::Enum(
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
            "justifyContent" => Property::Enum(
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
            "alignItems" => Property::Enum(
                &mut self.align_items,
                &["auto", "stretch", "flex-start", "center", "flex-end"],
            ),
            "alignSelf" => Property::Enum(
                &mut self.align_self,
                &["auto", "stretch", "flex-start", "center", "flex-end"],
            ),
            "overflow" => Property::Enum(&mut self.overflow_hidden, &["visible", "hidden"]),
            "pointerEvents" => {
                Property::Enum(&mut self.pointer_none, &["auto", "none", "box-none"])
            }
            "textAlign" => Property::Enum(
                &mut self.text_align,
                &["left", "center", "right", "start", "end"],
            ),
            "direction" => Property::Enum(&mut self.dir, &["inherit", "ltr", "rtl"]),
            "objectFit" => Property::Enum(&mut self.object_fit, &["fill", "cover", "contain"]),
            "whiteSpace" => Property::Enum(&mut self.nowrap, &["normal", "nowrap"]),
            "textOverflow" => Property::Enum(&mut self.text_overflow, &["clip", "ellipsis"]),
            "flexBasis" => Property::Dim(&mut self.basis),
            "width" => Property::Dim(&mut self.width),
            "height" => Property::Dim(&mut self.height),
            "minWidth" => Property::Dim(&mut self.min_w),
            "minHeight" => Property::Dim(&mut self.min_h),
            "maxWidth" => Property::Dim(&mut self.max_w),
            "maxHeight" => Property::Dim(&mut self.max_h),
            "left" => Property::Dim(&mut self.pos[0]),
            "top" => Property::Dim(&mut self.pos[1]),
            "right" => Property::Dim(&mut self.pos[2]),
            "bottom" => Property::Dim(&mut self.pos[3]),
            "flexGrow" => Property::Float(&mut self.grow),
            "flexShrink" => Property::Float(&mut self.shrink),
            "marginLeft" => Property::Float(&mut self.margin[0]),
            "marginTop" => Property::Float(&mut self.margin[1]),
            "marginRight" => Property::Float(&mut self.margin[2]),
            "marginBottom" => Property::Float(&mut self.margin[3]),
            "paddingLeft" => Property::Float(&mut self.padding[0]),
            "paddingTop" => Property::Float(&mut self.padding[1]),
            "paddingRight" => Property::Float(&mut self.padding[2]),
            "paddingBottom" => Property::Float(&mut self.padding[3]),
            "gap" => Property::Float(&mut self.gap),
            "borderRadius" => Property::Float(&mut self.radius),
            "borderWidth" => Property::Float(&mut self.border_width),
            "opacity" => Property::Float(&mut self.opacity),
            "shadowBlur" => Property::Float(&mut self.shadow_blur),
            "shadowOffsetX" => Property::Float(&mut self.shadow_x),
            "shadowOffsetY" => Property::Float(&mut self.shadow_y),
            "backdropBlur" => Property::Float(&mut self.backdrop_blur),
            "fontSize" => Property::Float(&mut self.font_size),
            "lineHeight" => Property::Float(&mut self.line_height),
            "fontWeight" => Property::Float(&mut self.font_weight),
            "translateX" => Property::Float(&mut self.translate_x),
            "translateY" => Property::Float(&mut self.translate_y),
            "zIndex" => Property::Float(&mut self.z_index),
            "backgroundColor" => Property::Color(&mut self.bg),
            "borderColor" => Property::Color(&mut self.border_color),
            "color" => Property::Color(&mut self.color),
            "shadowColor" => Property::Color(&mut self.shadow_color),
            "numberOfLines" => Property::Byte(&mut self.line_clamp),
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
                    return None;
                }
                let x = self.extension();
                x.r.ok = 0;
                match key {
                    "borderLeftWidth" => {
                        x.edge_set |= 1 << 0;
                        Property::Float(&mut x.border_side[0])
                    }
                    "borderLeftColor" => {
                        x.edge_set |= 1 << 6;
                        Property::Color(&mut x.border_side_color[0])
                    }
                    "borderTopWidth" => {
                        x.edge_set |= 1 << 1;
                        Property::Float(&mut x.border_side[1])
                    }
                    "borderTopColor" => {
                        x.edge_set |= 1 << 7;
                        Property::Color(&mut x.border_side_color[1])
                    }
                    "borderRightWidth" => {
                        x.edge_set |= 1 << 2;
                        Property::Float(&mut x.border_side[2])
                    }
                    "borderRightColor" => {
                        x.edge_set |= 1 << 8;
                        Property::Color(&mut x.border_side_color[2])
                    }
                    "borderBottomWidth" => {
                        x.edge_set |= 1 << 3;
                        Property::Float(&mut x.border_side[3])
                    }
                    "borderBottomColor" => {
                        x.edge_set |= 1 << 9;
                        Property::Color(&mut x.border_side_color[3])
                    }
                    "borderStartWidth" => {
                        x.edge_set |= 1 << 4;
                        Property::Float(&mut x.border_side[4])
                    }
                    "borderStartColor" => {
                        x.edge_set |= 1 << 10;
                        Property::Color(&mut x.border_side_color[4])
                    }
                    "borderEndWidth" => {
                        x.edge_set |= 1 << 5;
                        Property::Float(&mut x.border_side[5])
                    }
                    "borderEndColor" => {
                        x.edge_set |= 1 << 11;
                        Property::Color(&mut x.border_side_color[5])
                    }
                    "borderTopLeftRadius" => {
                        x.edge_set |= 1 << 16;
                        Property::Float(&mut x.radius_corner[0])
                    }
                    "borderTopRightRadius" => {
                        x.edge_set |= 1 << 17;
                        Property::Float(&mut x.radius_corner[1])
                    }
                    "borderBottomRightRadius" => {
                        x.edge_set |= 1 << 18;
                        Property::Float(&mut x.radius_corner[2])
                    }
                    "borderBottomLeftRadius" => {
                        x.edge_set |= 1 << 19;
                        Property::Float(&mut x.radius_corner[3])
                    }
                    "borderTopStartRadius" => {
                        x.edge_set |= 1 << 20;
                        Property::Float(&mut x.radius_corner[4])
                    }
                    "borderTopEndRadius" => {
                        x.edge_set |= 1 << 21;
                        Property::Float(&mut x.radius_corner[5])
                    }
                    "borderBottomStartRadius" => {
                        x.edge_set |= 1 << 22;
                        Property::Float(&mut x.radius_corner[6])
                    }
                    "borderBottomEndRadius" => {
                        x.edge_set |= 1 << 23;
                        Property::Float(&mut x.radius_corner[7])
                    }
                    "marginStart" => {
                        x.edge_set |= 1 << 12;
                        Property::Float(&mut x.margin_inline[0])
                    }
                    "paddingStart" => {
                        x.edge_set |= 1 << 14;
                        Property::Float(&mut x.padding_inline[0])
                    }
                    "marginEnd" => {
                        x.edge_set |= 1 << 13;
                        Property::Float(&mut x.margin_inline[1])
                    }
                    "paddingEnd" => {
                        x.edge_set |= 1 << 15;
                        Property::Float(&mut x.padding_inline[1])
                    }
                    "start" => {
                        x.edge_set |= 0;
                        Property::Dim(&mut x.inset_inline[0])
                    }
                    "end" => {
                        x.edge_set |= 0;
                        Property::Dim(&mut x.inset_inline[1])
                    }
                    "scaleX" => {
                        x.edge_set |= 0;
                        Property::Float(&mut x.scale_x)
                    }
                    "scaleY" => {
                        x.edge_set |= 0;
                        Property::Float(&mut x.scale_y)
                    }
                    "rotate" => {
                        x.edge_set |= 0;
                        Property::Float(&mut x.rotate)
                    }
                    "transformOriginX" => {
                        x.edge_set |= 0;
                        Property::Dim(&mut x.origin_x)
                    }
                    "transformOriginY" => {
                        x.edge_set |= 0;
                        Property::Dim(&mut x.origin_y)
                    }
                    _ => unreachable!(),
                }
            }
        })
    }
    fn extension(&mut self) -> &mut StyleExt {
        if self.ext.is_null() {
            self.ext = Box::into_raw(Box::new(StyleExt::default()));
        }
        // SAFETY: ext is created here and freed only by release(), on the UI thread.
        unsafe { &mut *self.ext }
    }
    pub fn set_number(&mut self, key: &str, value: f64) -> bool {
        match self.property(key) {
            Some(Property::Dim(d)) => {
                *d = Dim {
                    v: value as f32,
                    unit: 1,
                }
            }
            Some(Property::Float(f)) => *f = value as f32,
            Some(Property::Color(c)) => *c = (value as i64) as u32,
            Some(Property::Byte(n)) => *n = value.clamp(0., 255.) as u8,
            _ => return false,
        }
        true
    }
    pub fn set_string(&mut self, key: &str, value: &CStr) -> bool {
        match self.property(key) {
            Some(Property::Enum(n, values)) => {
                let Some(i) = values.iter().position(|v| v.as_bytes() == value.to_bytes()) else {
                    return false;
                };
                *n = i as u8;
            }
            Some(Property::Dim(d)) => {
                if value.to_bytes() == b"auto" {
                    *d = Dim::default();
                } else {
                    let mut end = std::ptr::null_mut();
                    let n = unsafe { libc::strtof(value.as_ptr(), &mut end) };
                    if end == value.as_ptr().cast_mut() {
                        return false;
                    }
                    *d = Dim {
                        v: n,
                        unit: if unsafe { *end } == b'%' as std::ffi::c_char {
                            2
                        } else {
                            1
                        },
                    };
                }
            }
            Some(Property::Float(f)) => {
                *f = unsafe { libc::strtof(value.as_ptr(), std::ptr::null_mut()) }
            }
            Some(Property::Byte(n)) => {
                *n = unsafe { libc::strtol(value.as_ptr(), std::ptr::null_mut(), 10) }.clamp(0, 255)
                    as u8
            }
            _ => return false,
        }
        true
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
