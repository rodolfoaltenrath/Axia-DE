//! Bounded GPU-underlay cache for the existing glass renderer. The compositor
//! supplies downsampled intersection pixels on the UI thread; workers only read.
use std::{cell::RefCell, ptr};
pub(crate) type Reader = unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, *mut u32) -> u64;
const MAX_PIXELS: usize = 262144;
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Default)]
struct Entry {
    key: (u64, [i32; 7]),
    pixels: Vec<u32>,
}
#[derive(Default)]
struct Cache {
    reader: Option<Reader>,
    entries: Vec<Entry>,
    next: usize,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

#[no_mangle]
pub extern "C" fn axia_glass_set_backdrop(reader: Option<Reader>) {
    CACHE.with(|c| {
        *c.borrow_mut() = Cache {
            reader,
            ..Cache::default()
        }
    });
}

// Returned storage remains live until the next read on this UI thread. The
// caller joins all glass workers before starting another blur/read.
pub(crate) fn read(area: [i32; 7]) -> *const u32 {
    let [x0, y0, x1, y1, step, w, h] = area;
    if step < 1 || w < 1 || h < 1 {
        return ptr::null();
    }
    let Some(n) = (w as usize)
        .checked_mul(h as usize)
        .filter(|&n| n <= MAX_PIXELS)
    else {
        return ptr::null();
    };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let Some(reader) = c.reader else {
            return ptr::null();
        };
        let key = unsafe { reader(x0, y0, x1, y1, step, w, h, ptr::null_mut()) };
        if key == 0 {
            return ptr::null();
        }
        let key = (key, area);
        if let Some(e) = c.entries.iter().find(|e| e.key == key) {
            return e.pixels.as_ptr();
        }
        // Video changes the generation, not the tile's allocation. Refill the
        // existing region instead of caching four obsolete frames of it.
        if let Some(i) = c.entries.iter().position(|e| e.key.1 == area) {
            let mut entry = c.entries.remove(i);
            if unsafe { reader(x0, y0, x1, y1, step, w, h, entry.pixels.as_mut_ptr()) } != key.0 {
                return ptr::null();
            }
            entry.key = key;
            c.entries.push(entry);
            return c.entries.last().unwrap().pixels.as_ptr();
        }
        let bytes = c.entries.iter().map(|e| e.pixels.len() * 4).sum::<usize>();
        if c.entries.len() == 4 || bytes + n * 4 > MAX_BYTES {
            let i = c.next % c.entries.len().max(1);
            if !c.entries.is_empty() {
                c.entries.remove(i);
            }
            c.next = c.next.wrapping_add(1);
        }
        // A larger replacement can require more than one eviction.
        while c.entries.iter().map(|e| e.pixels.len() * 4).sum::<usize>() + n * 4 > MAX_BYTES {
            c.entries.remove(0);
        }
        let mut pixels = Vec::new();
        if pixels.try_reserve_exact(n).is_err() {
            return ptr::null();
        }
        pixels.resize(n, 0);
        if unsafe { reader(x0, y0, x1, y1, step, w, h, pixels.as_mut_ptr()) } != key.0 {
            return ptr::null();
        }
        c.entries.push(Entry { key, pixels });
        c.entries.last().unwrap().pixels.as_ptr()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    thread_local! { static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
    thread_local! { static GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
    unsafe extern "C" fn reader(
        x: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        w: i32,
        h: i32,
        out: *mut u32,
    ) -> u64 {
        if !out.is_null() {
            READS.with(|n| n.set(n.get() + 1));
            std::slice::from_raw_parts_mut(out, (w * h) as usize).fill(0xffaabbcc);
        }
        x as u64 + 1 + GENERATION.with(|g| g.get()) * 1000
    }
    #[test]
    fn unchanged_tiles_are_reused_and_generations_invalidate() {
        axia_glass_set_backdrop(Some(reader));
        READS.with(|n| n.set(0));
        assert!(!read([0, 0, 8, 8, 4, 2, 2]).is_null());
        assert!(!read([0, 0, 8, 8, 4, 2, 2]).is_null());
        assert_eq!(READS.with(|n| n.get()), 1);
        assert!(!read([1, 0, 9, 8, 4, 2, 2]).is_null());
        assert_eq!(READS.with(|n| n.get()), 2);
        axia_glass_set_backdrop(None);
        assert!(read([0, 0, 8, 8, 4, 2, 2]).is_null());
    }
    #[test]
    fn memory_is_bounded_across_many_windows() {
        axia_glass_set_backdrop(Some(reader));
        for i in 0..16 {
            assert!(!read([i, 0, 1024, 1024, 2, 512, 512]).is_null());
        }
        CACHE.with(|c| {
            let c = c.borrow();
            assert!(c.entries.len() <= 4);
            assert!(c.entries.iter().map(|e| e.pixels.len() * 4).sum::<usize>() <= MAX_BYTES);
        });
        assert!(read([0, 0, 1024, 1024, 1, 1024, 1024]).is_null());
        axia_glass_set_backdrop(None);
    }
    #[test]
    fn changing_video_reuses_the_same_allocation() {
        axia_glass_set_backdrop(Some(reader));
        let first = read([0, 0, 64, 64, 4, 16, 16]);
        assert!(!first.is_null());
        for generation in 1..50 {
            GENERATION.with(|g| g.set(generation));
            assert_eq!(read([0, 0, 64, 64, 4, 16, 16]), first);
        }
        CACHE.with(|c| assert_eq!(c.borrow().entries.len(), 1));
        axia_glass_set_backdrop(None);
    }
}
