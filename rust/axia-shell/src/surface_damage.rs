//! Which part of a window an app's commit changed. The glass over a window is
//! recomputed only when that part reaches it, so a blinking cursor in the middle
//! of a window costs no repaint of the bar or the Dock.
//!
//! The answer must never be too small: stale glass is worse than a wasted frame.
//! Anything uncertain answers "the whole window", which is what happens without
//! this module.

/// Damage gathered from `wl_surface.damage` (surface units) and
/// `wl_surface.damage_buffer` (buffer pixels). All zeros means none.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct AxiaSurfaceDamage {
    surface: [i32; 4],
    buffer: [i32; 4],
}

/// The commit changed no pixels.
const NOTHING: i32 = 0;
/// The commit changed the box written to `out`, in the window's coordinates.
const AREA: i32 = 1;
/// Treat the whole window as changed.
const WHOLE: i32 = 2;

fn extend(area: &mut [i32; 4], x: i32, y: i32, width: i32, height: i32) {
    if width <= 0 || height <= 0 {
        return;
    }
    // Apps may send INT32_MAX to mean "everything".
    let (x1, y1) = (x.saturating_add(width), y.saturating_add(height));
    if area[2] <= area[0] || area[3] <= area[1] {
        *area = [x, y, x1, y1];
    } else {
        *area = [
            area[0].min(x),
            area[1].min(y),
            area[2].max(x1),
            area[3].max(y1),
        ];
    }
}

fn is_set(area: &[i32; 4]) -> bool {
    area[2] > area[0] && area[3] > area[1]
}

/// # Safety
/// `damage` is null or points to a valid `AxiaSurfaceDamage`.
#[no_mangle]
pub unsafe extern "C" fn axia_surface_damage_add(
    damage: *mut AxiaSurfaceDamage,
    in_buffer: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if let Some(damage) = damage.as_mut() {
        let area = if in_buffer {
            &mut damage.buffer
        } else {
            &mut damage.surface
        };
        extend(area, x, y, width, height);
    }
}

/// The geometry of a surface as it is after the commit.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AxiaSurfaceShape {
    /// Where the surface's (0, 0) is, in the window's coordinates.
    pub x: f32,
    pub y: f32,
    /// Its size in surface units.
    pub width: i32,
    pub height: i32,
    /// Its buffer's size in pixels.
    pub buffer_width: i32,
    pub buffer_height: i32,
    /// `wp_viewport` crops the buffer, so buffer pixels don't map linearly.
    pub cropped: bool,
    /// A new buffer came with this commit.
    pub attached: bool,
    /// It has subsurfaces, whose order and position a commit can apply.
    pub has_children: bool,
}

fn changed_box(damage: &AxiaSurfaceDamage, shape: &AxiaSurfaceShape) -> (i32, [f32; 4]) {
    let nothing = (NOTHING, [0.0; 4]);
    let whole = (WHOLE, [0.0; 4]);
    if !shape.attached {
        return if shape.has_children { whole } else { nothing };
    }
    if shape.width <= 0 || shape.height <= 0 {
        return whole;
    }
    let mut area: Option<[f32; 4]> = None;
    let mut join = |b: [f32; 4]| {
        area = Some(match area {
            None => b,
            Some(a) => [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ],
        });
    };
    if is_set(&damage.surface) {
        let d = damage.surface;
        join([d[0] as f32, d[1] as f32, d[2] as f32, d[3] as f32]);
    }
    if is_set(&damage.buffer) {
        if shape.cropped || shape.buffer_width <= 0 || shape.buffer_height <= 0 {
            return whole;
        }
        // Scale and viewport destination, as one ratio per axis.
        let kx = shape.width as f32 / shape.buffer_width as f32;
        let ky = shape.height as f32 / shape.buffer_height as f32;
        let d = damage.buffer;
        join([
            d[0] as f32 * kx,
            d[1] as f32 * ky,
            d[2] as f32 * kx,
            d[3] as f32 * ky,
        ]);
    }
    // A buffer without any damage is unusual enough not to be trusted.
    let Some(a) = area else {
        return whole;
    };
    // One pixel more: the GPU's bilinear sampling reaches the neighbours.
    let x0 = (a[0].floor() - 1.0).max(0.0);
    let y0 = (a[1].floor() - 1.0).max(0.0);
    let x1 = (a[2].ceil() + 1.0).min(shape.width as f32);
    let y1 = (a[3].ceil() + 1.0).min(shape.height as f32);
    if x1 <= x0 || y1 <= y0 {
        return nothing;
    }
    (
        AREA,
        [x0 + shape.x, y0 + shape.y, x1 + shape.x, y1 + shape.y],
    )
}

/// Returns 0 (no pixels changed), 1 (`out` holds x0, y0, x1, y1) or 2 (whole window).
///
/// # Safety
/// Null pointers answer 2. Otherwise `damage` and `shape` point to valid values
/// and `out` has room for four floats.
#[no_mangle]
pub unsafe extern "C" fn axia_surface_damage_box(
    damage: *const AxiaSurfaceDamage,
    shape: *const AxiaSurfaceShape,
    out: *mut f32,
) -> i32 {
    if damage.is_null() || shape.is_null() || out.is_null() {
        return WHOLE;
    }
    let (kind, area) = changed_box(&*damage, &*shape);
    if kind == AREA {
        std::ptr::copy_nonoverlapping(area.as_ptr(), out, 4);
    }
    kind
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> AxiaSurfaceShape {
        AxiaSurfaceShape {
            x: 100.0,
            y: 50.0,
            width: 800,
            height: 600,
            buffer_width: 800,
            buffer_height: 600,
            cropped: false,
            attached: true,
            has_children: false,
        }
    }

    fn surface(x: i32, y: i32, w: i32, h: i32) -> AxiaSurfaceDamage {
        let mut d = AxiaSurfaceDamage::default();
        extend(&mut d.surface, x, y, w, h);
        d
    }

    fn buffer(x: i32, y: i32, w: i32, h: i32) -> AxiaSurfaceDamage {
        let mut d = AxiaSurfaceDamage::default();
        extend(&mut d.buffer, x, y, w, h);
        d
    }

    #[test]
    fn surface_damage_lands_in_window_coordinates_with_a_margin() {
        let (kind, a) = changed_box(&surface(10, 20, 2, 16), &shape());
        assert_eq!(kind, AREA);
        assert_eq!(a, [109.0, 69.0, 113.0, 87.0]);
    }

    #[test]
    fn several_rectangles_become_the_box_around_them() {
        let mut d = surface(10, 10, 5, 5);
        extend(&mut d.surface, 100, 200, 10, 10);
        let (_, a) = changed_box(&d, &shape());
        assert_eq!(a, [109.0, 59.0, 211.0, 261.0]);
    }

    #[test]
    fn buffer_damage_is_scaled_to_surface_units() {
        let mut hidpi = shape();
        hidpi.buffer_width = 1600;
        hidpi.buffer_height = 1200;
        let (kind, a) = changed_box(&buffer(20, 40, 4, 32), &hidpi);
        assert_eq!(kind, AREA);
        assert_eq!(a, [109.0, 69.0, 113.0, 87.0]);
    }

    #[test]
    fn fractional_scale_rounds_outwards() {
        let mut fractional = shape();
        fractional.buffer_width = 1200; // 1.5 buffer pixels per surface unit
        fractional.buffer_height = 900;
        let (_, a) = changed_box(&buffer(1, 1, 1, 1), &fractional);
        assert!(a[0] <= 100.0 + 2.0 / 3.0 && a[1] <= 50.0 + 2.0 / 3.0);
        assert!(a[2] >= 100.0 + 4.0 / 3.0 && a[3] >= 50.0 + 4.0 / 3.0);
    }

    #[test]
    fn both_kinds_of_damage_are_joined() {
        let mut d = surface(0, 0, 10, 10);
        extend(&mut d.buffer, 100, 100, 10, 10);
        let (_, a) = changed_box(&d, &shape());
        assert_eq!(a, [100.0, 50.0, 211.0, 161.0]);
    }

    #[test]
    fn huge_damage_saturates_and_is_clipped_to_the_surface() {
        let (kind, a) = changed_box(&surface(0, 0, i32::MAX, i32::MAX), &shape());
        assert_eq!(kind, AREA);
        assert_eq!(a, [100.0, 50.0, 900.0, 650.0]);
    }

    #[test]
    fn damage_outside_the_surface_changes_nothing() {
        let (kind, _) = changed_box(&surface(5000, 5000, 10, 10), &shape());
        assert_eq!(kind, NOTHING);
    }

    #[test]
    fn empty_rectangles_are_ignored() {
        let mut d = AxiaSurfaceDamage::default();
        extend(&mut d.surface, 10, 10, 0, 10);
        extend(&mut d.surface, 10, 10, 10, -1);
        assert_eq!(d, AxiaSurfaceDamage::default());
    }

    #[test]
    fn a_commit_without_a_buffer_changes_no_pixels() {
        let mut s = shape();
        s.attached = false;
        assert_eq!(changed_box(&surface(0, 0, 10, 10), &s).0, NOTHING);
    }

    #[test]
    fn uncertain_cases_answer_the_whole_window() {
        // A new buffer that declares no damage.
        assert_eq!(
            changed_box(&AxiaSurfaceDamage::default(), &shape()).0,
            WHOLE
        );
        // A commit on a parent can reorder or move its subsurfaces.
        let mut parent = shape();
        parent.attached = false;
        parent.has_children = true;
        assert_eq!(changed_box(&AxiaSurfaceDamage::default(), &parent).0, WHOLE);
        // Buffer pixels of a cropped viewport don't map to the surface linearly.
        let mut cropped = shape();
        cropped.cropped = true;
        assert_eq!(changed_box(&buffer(0, 0, 10, 10), &cropped).0, WHOLE);
        // No size yet.
        let mut empty = shape();
        empty.width = 0;
        assert_eq!(changed_box(&surface(0, 0, 10, 10), &empty).0, WHOLE);
    }

    #[test]
    fn surface_damage_stays_exact_on_a_cropped_viewport() {
        let mut cropped = shape();
        cropped.cropped = true;
        assert_eq!(changed_box(&surface(0, 0, 10, 10), &cropped).0, AREA);
    }

    #[test]
    fn ffi_writes_the_box_only_for_an_area_and_tolerates_null() {
        let mut out = [9.0f32; 4];
        unsafe {
            let d = surface(10, 20, 2, 16);
            assert_eq!(
                axia_surface_damage_box(&d, &shape(), out.as_mut_ptr()),
                AREA
            );
            assert_eq!(out, [109.0, 69.0, 113.0, 87.0]);
            let mut untouched = [9.0f32; 4];
            let none = AxiaSurfaceDamage::default();
            assert_eq!(
                axia_surface_damage_box(&none, &shape(), untouched.as_mut_ptr()),
                WHOLE
            );
            assert_eq!(untouched, [9.0; 4]);
            assert_eq!(
                axia_surface_damage_box(std::ptr::null(), &shape(), out.as_mut_ptr()),
                WHOLE
            );
            axia_surface_damage_add(std::ptr::null_mut(), false, 0, 0, 1, 1);
        }
    }
}
