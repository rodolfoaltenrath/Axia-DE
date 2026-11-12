//! Damage history for EGL buffer age. No GL calls or allocation.
use std::ffi::{c_int, c_long};
use std::ptr;

const HISTORY: usize = 5;

/// Transitional ABI shared with canvas.h.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Rect {
    x0: c_int,
    y0: c_int,
    x1: c_int,
    y1: c_int,
}

impl Rect {
    fn nonempty(self) -> bool {
        self.x1 > self.x0 && self.y1 > self.y0
    }
}

/// Transitional ABI shared with gpu/gscissor.h.
#[repr(C)]
#[derive(Default)]
pub struct GpuScissor {
    r: [Rect; HISTORY],
    n: c_int,
}

#[no_mangle]
pub extern "C" fn gsc_area(r: Rect) -> c_long {
    if !r.nonempty() {
        return 0;
    }
    // Widen before subtraction, including for rectangles crossing the origin.
    (i64::from(r.x1) - i64::from(r.x0))
        .saturating_mul(i64::from(r.y1) - i64::from(r.y0))
        .min(c_long::MAX as i64) as c_long
}

#[no_mangle]
pub extern "C" fn gsc_join(a: Rect, b: Rect) -> Rect {
    if !a.nonempty() {
        return if b.nonempty() { b } else { Rect::default() };
    }
    if !b.nonempty() {
        return a;
    }
    Rect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

#[no_mangle]
pub extern "C" fn gsc_clip(r: Rect, width: c_int, height: c_int) -> Rect {
    let clipped = Rect {
        x0: r.x0.max(0),
        y0: r.y0.max(0),
        x1: r.x1.min(width),
        y1: r.y1.min(height),
    };
    if clipped.nonempty() {
        clipped
    } else {
        Rect::default()
    }
}

/// # Safety
/// `g` must point to writable, aligned GpuScissor storage, exclusively accessible.
/// History may be uninitialized; resetting only initializes the count.
#[no_mangle]
pub unsafe extern "C" fn gsc_reset(g: *mut GpuScissor) {
    ptr::addr_of_mut!((*g).n).write(0);
}

/// # Safety
/// `g` must have been zero-initialized or reset, with history populated only by
/// gsc_push. The caller must prevent concurrent mutation during this call.
#[no_mangle]
pub unsafe extern "C" fn gsc_region(
    g: *const GpuScissor,
    age: c_int,
    current: Rect,
    width: c_int,
    height: c_int,
) -> Rect {
    if age <= 0 || age - 1 > (*g).n {
        return Rect {
            x0: 0,
            y0: 0,
            x1: width,
            y1: height,
        };
    }
    let mut region = gsc_clip(current, width, height);
    for i in 0..(age - 1) as usize {
        region = gsc_join(region, gsc_clip((*g).r[i], width, height));
    }
    region
}

/// # Safety
/// `g` must have been zero-initialized or reset and must be exclusively accessible.
#[no_mangle]
pub unsafe extern "C" fn gsc_push(g: *mut GpuScissor, current: Rect) {
    // Raw copying preserves possibly uninitialized, unused history slots.
    let history = ptr::addr_of_mut!((*g).r).cast::<Rect>();
    ptr::copy(history, history.add(1), HISTORY - 1);
    history.write(current);
    (*g).n = ((*g).n + 1).min(HISTORY as c_int);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn history_ages_empty_frames_and_reset() {
        let mut g = GpuScissor::default();
        let a = rect(10, 10, 20, 20);
        let b = rect(50, 5, 60, 8);
        let all = rect(0, 0, 100, 50);
        let empty = Rect::default();
        unsafe {
            assert_eq!(gsc_region(&g, 0, a, 100, 50), all);
            assert_eq!(gsc_region(&g, 2, a, 100, 50), all);
            assert_eq!(gsc_region(&g, 1, a, 100, 50), a);
            gsc_push(&mut g, a);
            assert_eq!(gsc_region(&g, 2, b, 100, 50), rect(10, 5, 60, 20));
            assert_eq!(gsc_region(&g, 3, b, 100, 50), all);
            gsc_push(&mut g, empty);
            assert_eq!(gsc_region(&g, 2, empty, 100, 50), empty);
            assert_eq!(gsc_region(&g, 3, empty, 100, 50), a);
            assert_eq!(
                gsc_region(&g, 1, rect(-5, 40, 200, 90), 100, 50),
                rect(0, 40, 100, 50)
            );
            for _ in 0..HISTORY + 3 {
                gsc_push(&mut g, a);
            }
            assert_eq!(g.n, HISTORY as c_int);
            assert_ne!(gsc_region(&g, HISTORY as c_int + 1, b, 100, 50), all);
            assert_eq!(gsc_region(&g, HISTORY as c_int + 2, b, 100, 50), all);
            gsc_reset(&mut g);
            assert_eq!(gsc_region(&g, 2, a, 100, 50), all);
        }
    }

    #[test]
    fn clipping_and_area_handle_extreme_coordinates() {
        let extreme = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(gsc_clip(extreme, 100, 50), rect(0, 0, 100, 50));
        assert_eq!(gsc_area(extreme), c_long::MAX);
        assert_eq!(gsc_area(rect(10, 0, 5, 8)), 0);
        assert_eq!(gsc_clip(rect(101, 0, 200, 10), 100, 50), Rect::default());
    }

    #[test]
    fn rotating_buffers_match_full_redraws() {
        // A three-buffer swapchain must preserve every unchanged pixel when a
        // buffer is reused. Compare partial redraws with an independent full image.
        let mut g = GpuScissor::default();
        let mut buffers = [[0u32; 128]; 3];
        let mut last_frame = [0usize; 3];
        let mut expected = [0u32; 128];
        for frame in 1..=300 {
            let index = frame % 3;
            let x = frame % 16;
            let y = frame % 8;
            let damage = rect(x as i32, y as i32, x as i32 + 1, y as i32 + 1);
            expected[y * 16 + x] = frame as u32;
            let age = if last_frame[index] == 0 {
                0
            } else {
                (frame - last_frame[index]) as i32
            };
            unsafe {
                let region = gsc_region(&g, age, damage, 16, 8);
                for yy in region.y0..region.y1 {
                    for xx in region.x0..region.x1 {
                        let pixel = (yy * 16 + xx) as usize;
                        buffers[index][pixel] = expected[pixel];
                    }
                }
                assert_eq!(buffers[index], expected, "frame {frame}");
                gsc_push(&mut g, damage);
            }
            last_frame[index] = frame;
        }
    }
}
