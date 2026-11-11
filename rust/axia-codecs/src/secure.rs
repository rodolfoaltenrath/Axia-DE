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
        let Some(size) = layout.size().checked_add(align).and_then(|n| n.checked_add(std::mem::size_of::<usize>())) else { return std::ptr::null_mut(); };
        let base = sm_alloc(size).cast::<u8>();
        if base.is_null() { return base; }
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
