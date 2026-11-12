//! Primitives of the software canvas moving from server/src/canvas.c, one at a time, each with the
//! same pixels as the C it replaces.
//!
//! `canvas_clear` filled its clip one 32-bit store at a time: at -O2 for the x86-64 baseline GCC
//! neither vectorized that loop nor turned it into memset, and it runs for every damaged area and for
//! every app window's hole in the UI layer (4.8 % of the main thread in a desktop profile). Here a row
//! is one `fill`, or a `memset` when the value is 0 (the holes).

/// Fills [x0, x1) × [y0, y1) of a canvas with `value`.
///
/// # Safety
/// Every pixel `px + y * stride + x` inside the rectangle is addressable (a canvas.c canvas and its
/// clip: `px` may start before its buffer, as the drag layer's does).
#[no_mangle]
pub unsafe extern "C" fn axia_canvas_fill(px: *mut u32, stride: i32, x0: i32, y0: i32, x1: i32, y1: i32, value: u32) {
    if px.is_null() || stride <= 0 || x0 >= x1 || y0 >= y1 {
        return;
    }
    let n = (x1 - x0) as usize;
    for y in y0..y1 {
        let row = px.offset(y as isize * stride as isize + x0 as isize);
        if value == 0 {
            std::ptr::write_bytes(row, 0, n);
        } else {
            std::slice::from_raw_parts_mut(row, n).fill(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_exactly_the_rectangle() {
        for value in [0u32, 0xff12_3456, 0x0101_0101] {
            let (w, h) = (37, 23);
            let mut px: Vec<u32> = (0..w * h).map(|i| (i as u32).wrapping_mul(2_654_435_761)).collect();
            let before = px.clone();
            let (x0, y0, x1, y1) = (3, 4, 30, 20);
            unsafe { axia_canvas_fill(px.as_mut_ptr(), w, x0, y0, x1, y1, value) };
            for y in 0..h {
                for x in 0..w {
                    let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                    let i = (y * w + x) as usize;
                    assert_eq!(px[i], if inside { value } else { before[i] }, "({x}, {y})");
                }
            }
        }
        let mut one = [7u32];
        unsafe { axia_canvas_fill(one.as_mut_ptr(), 1, 0, 0, 0, 1, 0) };
        assert_eq!(one, [7], "an empty rectangle");
    }
}
