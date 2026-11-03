//! Reusable blur storage, an exact bounded backdrop cache and material policy.
//! glass_render.rs owns all pixel kernels; C supplies the canvas and synchronous pool.
use std::{cell::RefCell, ptr};
const MAX_WORK: usize = 32 * 1024 * 1024;
const CACHE_BYTES: usize = 2 * 1024 * 1024;
const CACHE_SLOTS: usize = 4;

#[repr(C)]
pub struct GlassBuffers {
    pub(crate) pixels: *mut u32,
    pub(crate) source: *mut u32,
    pub(crate) sums: *mut i32,
    pub(crate) columns: *mut i32,
    pub(crate) division: *mut u8,
}
#[derive(Default)]
struct Entry {
    key: (usize, usize, usize),
    input: Vec<u32>,
    output: Vec<u32>,
    valid: bool,
}
#[derive(Default)]
struct Glass {
    pixels: Vec<u32>,
    source: Vec<u32>,
    sums: Vec<i32>,
    columns: Vec<i32>,
    division: Vec<u8>,
    window: usize,
    key: (usize, usize, usize),
    entries: Vec<Entry>,
    pending: Option<usize>,
    next: usize,
}
thread_local! {
    static STATE: RefCell<Glass> = RefCell::new(Glass::default());
    static SCRATCH: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}
fn resize<T: Default + Clone>(v: &mut Vec<T>, n: usize) -> bool {
    if n > v.len() && v.try_reserve_exact(n - v.len()).is_err() {
        return false;
    }
    v.resize(n, T::default());
    true
}
impl Glass {
    fn prepare(&mut self, w: usize, h: usize, rad: usize, columns: usize) -> bool {
        let Some(n) = w.checked_mul(h) else {
            return false;
        };
        let Some(win) = rad.checked_mul(2).and_then(|v| v.checked_add(1)) else {
            return false;
        };
        if w == 0
            || h == 0
            || columns == 0
            || rad == 0
            || rad > 1024
            || n > MAX_WORK / 8
            || columns > 32768
            || w > 32768
        {
            return false;
        }
        self.pending = None;
        if !resize(&mut self.pixels, n)
            || !resize(&mut self.source, n)
            || !resize(&mut self.sums, w * 4)
            || !resize(&mut self.columns, columns * 3)
            || !resize(&mut self.division, 255 * win + 1)
        {
            return false;
        }
        if self.window != win {
            for (i, v) in self.division.iter_mut().enumerate() {
                *v = (i / win) as u8;
            }
            self.window = win;
        }
        self.key = (w, h, rad);
        true
    }
    fn reuse(&mut self) -> bool {
        for e in &self.entries {
            if e.valid && e.key == self.key && e.input == self.pixels {
                self.pixels.copy_from_slice(&e.output);
                return true;
            }
        }
        // Fixed maximum per entry makes the entire cache bounded, even with alternating sizes.
        if self.pixels.len() * 8 > CACHE_BYTES / CACHE_SLOTS {
            return false;
        }
        if self.entries.len() < CACHE_SLOTS {
            self.entries.push(Entry::default());
        }
        let i = self.next % self.entries.len();
        self.next = self.next.wrapping_add(1);
        let e = &mut self.entries[i];
        e.valid = false;
        if !resize(&mut e.input, self.pixels.len()) || !resize(&mut e.output, self.pixels.len()) {
            return false;
        }
        e.key = self.key;
        e.input.copy_from_slice(&self.pixels);
        self.pending = Some(i);
        false
    }
}
// Storage contract: one prepare -> Rust samples pixels -> reuse -> optional kernels + commit.
// Buffers belong to the calling thread, valid until its next prepare. C must join all
// pixel workers before another entrypoint and must never recursively prepare on that thread.
// Scratch is separate TLS storage: valid until that thread's next scratch call.
#[no_mangle]
pub unsafe extern "C" fn axia_glass_prepare(
    w: usize,
    h: usize,
    rad: usize,
    cw: usize,
    out: *mut GlassBuffers,
) -> bool {
    if out.is_null() {
        return false;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.prepare(w, h, rad, cw) {
            return false;
        }
        ptr::write(
            out,
            GlassBuffers {
                pixels: s.pixels.as_mut_ptr(),
                source: s.source.as_mut_ptr(),
                sums: s.sums.as_mut_ptr(),
                columns: s.columns.as_mut_ptr(),
                division: s.division.as_mut_ptr(),
            },
        );
        true
    })
}
#[no_mangle]
pub extern "C" fn axia_glass_reuse() -> bool {
    STATE.with(|s| s.borrow_mut().reuse())
}
#[no_mangle]
pub extern "C" fn axia_glass_commit() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(i) = s.pending.take() {
            let s = &mut *s;
            s.entries[i].output.copy_from_slice(&s.pixels);
            s.entries[i].valid = true;
        }
    });
}
#[no_mangle]
pub extern "C" fn axia_glass_scratch(words: usize) -> *mut u32 {
    if words > MAX_WORK / 4 {
        return ptr::null_mut();
    }
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        if resize(&mut s, words) {
            s.as_mut_ptr()
        } else {
            ptr::null_mut()
        }
    })
}
// RGB stays neutral; translucent tint keeps wallpaper visible while the outline
// separates the surface. Disabling effects yields a fully opaque material.
#[no_mangle]
pub extern "C" fn axia_glass_tint(dark: bool, effects: bool) -> u32 {
    match (dark, effects) {
        (true, true) => 0x602e2e38,
        (false, true) => 0x57ffffff,
        (true, false) => 0xff2a2a30,
        (false, false) => 0xfff2f2f5,
    }
}
#[no_mangle]
pub extern "C" fn axia_glass_outline(dark: bool) -> u32 {
    if dark {
        0x38ffffff
    } else {
        0xa6ffffff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_cache_tracks_content_and_radius() {
        let mut s = Glass::default();
        assert!(s.prepare(2, 2, 1, 8));
        s.pixels.copy_from_slice(&[1, 2, 3, 4]);
        assert!(!s.reuse());
        s.pixels.fill(42);
        let i = s.pending.take().unwrap();
        s.entries[i].output.copy_from_slice(&s.pixels);
        s.entries[i].valid = true;
        assert!(s.prepare(2, 2, 1, 8));
        s.pixels.copy_from_slice(&[1, 2, 3, 4]);
        assert!(s.reuse());
        assert_eq!(s.pixels, [42; 4]);
        s.pixels.copy_from_slice(&[1, 2, 9, 4]);
        assert!(!s.reuse());
        assert!(s.prepare(2, 2, 2, 8));
        s.pixels.copy_from_slice(&[1, 2, 3, 4]);
        assert!(!s.reuse());
    }
    #[test]
    fn warm_storage_is_reused_and_limits_checked() {
        let mut s = Glass::default();
        assert!(s.prepare(128, 128, 3, 512));
        let p = s.pixels.as_ptr();
        assert!(s.prepare(64, 64, 3, 256));
        assert_eq!(p, s.pixels.as_ptr());
        assert!(!s.prepare(usize::MAX, 2, 3, 1));
        assert!(!s.prepare(0, 1, 1, 1));
        assert!(!s.prepare(1, 1, usize::MAX, 1));
        assert!(!s.prepare(8192, 8192, 3, 8192));
    }
    #[test]
    fn cache_budget_and_thread_isolation() {
        let mut s = Glass::default();
        for w in 1..100 {
            assert!(s.prepare(w, 32, 3, w));
            s.pixels.fill(w as u32);
            assert!(!s.reuse());
        }
        assert_eq!(s.entries.len(), CACHE_SLOTS);
        assert!(
            s.entries
                .iter()
                .map(|e| (e.input.capacity() + e.output.capacity()) * 4)
                .sum::<usize>()
                <= CACHE_BYTES
        );
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    assert!(!axia_glass_scratch(8).is_null());
                    axia_glass_commit();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
    #[test]
    fn effects_off_is_opaque() {
        assert_eq!(axia_glass_tint(true, false) >> 24, 255);
        assert_eq!(axia_glass_tint(false, false) >> 24, 255);
    }
}

/// Release the calling renderer's retained storage. No prepared job or worker may
/// still reference its buffers. Used by renderer parity/lifetime checks.
#[no_mangle]
pub extern "C" fn axia_glass_reset() {
    STATE.with(|s| *s.borrow_mut() = Glass::default());
    SCRATCH.with(|s| *s.borrow_mut() = Vec::new());
}
