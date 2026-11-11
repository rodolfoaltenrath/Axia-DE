//! Only linked into axia-vault. Rust's allocator must use smem for *every*
//! allocation, not merely copy the final result into protected memory.
use std::alloc::{GlobalAlloc, Layout};
unsafe extern "C" {
    pub(crate) fn sm_alloc(size: usize) -> *mut std::ffi::c_void;
    fn sm_free(ptr: *mut std::ffi::c_void);
}
struct Secrets;
unsafe impl GlobalAlloc for Secrets {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(std::mem::align_of::<usize>());
        let Some(size) = layout
            .size()
            .checked_add(align)
            .and_then(|n| n.checked_add(std::mem::size_of::<usize>()))
        else {
            return std::ptr::null_mut();
        };
        let base = sm_alloc(size).cast::<u8>();
        if base.is_null() {
            return base;
        }
        let start = base.add(std::mem::size_of::<usize>()) as usize;
        let aligned = ((start + align - 1) & !(align - 1)) as *mut u8;
        aligned.cast::<usize>().sub(1).write(base as usize);
        aligned
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _: Layout) {
        let base = ptr.cast::<usize>().sub(1).read() as *mut std::ffi::c_void;
        sm_free(base);
    }
}
#[global_allocator]
static ALLOCATOR: Secrets = Secrets;

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" {
        fn sm_size(ptr: *const std::ffi::c_void) -> usize;
    }
    #[test]
    fn honors_large_alignment_and_exports_arena_buffers() {
        unsafe {
            for align in [1, 16, 64, 4096] {
                let layout = Layout::from_size_align(8193, align).unwrap();
                let p = ALLOCATOR.alloc(layout);
                assert!(!p.is_null());
                assert_eq!(p as usize % align, 0);
                p.write_bytes(0xa5, layout.size());
                ALLOCATOR.dealloc(p, layout);
            }
            let secret = b"vault data";
            let out = crate::export(secret);
            assert!(!out.is_null());
            assert!(sm_size(out.cast()) >= secret.len());
            assert_eq!(std::slice::from_raw_parts(out, secret.len()), secret);
            sm_free(out.cast());
        }
    }
    #[test]
    fn qr_pixels_are_argb_in_the_arena() {
        let png = crate::images::encode(&[12, 34, 56, 128], 1, 1, 4, None).unwrap();
        unsafe {
            let (mut w, mut h) = (0, 0);
            let out = crate::axia_vault_decode_argb(png.as_ptr(), png.len(), &mut w, &mut h);
            assert!(!out.is_null());
            assert_eq!((w, h), (1, 1));
            assert!(sm_size(out.cast()) >= 4);
            assert_eq!(*out, 0x800c2238);
            sm_free(out.cast());
        }
    }
}
