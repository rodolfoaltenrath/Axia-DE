use super::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {static ALLOCATIONS:Cell<Option<usize>>=const{Cell::new(None)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| {
            if let Some(count) = n.get() {
                n.set(Some(count + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        System.dealloc(p, layout)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
#[test]
fn primitives_need_no_heap_and_wipe_scratch() {
    unsafe {
        let mut work = vec![0; axia_crypto_argon_work_size(32)];
        let mut hash = [0; 32];
        let cfg = ArgonConfig {
            algorithm: 2,
            memory_kib: 32,
            passes: 1,
            lanes: 1,
        };
        let inputs = ArgonInputs {
            password: b"password".as_ptr(),
            salt: b"1234567890123456".as_ptr(),
            password_len: 8,
            salt_len: 16,
        };
        let extra = ArgonExtras {
            secret: ptr::null(),
            ad: ptr::null(),
            secret_len: 0,
            ad_len: 0,
        };
        let key = [42u8; 32];
        let nonce = [7u8; 24];
        let plain = [3u8; 64];
        let mut encrypted = [0u8; 64];
        let mut decoded = [0u8; 64];
        let mut tag = [0; 16];
        ALLOCATIONS.with(|n| n.set(Some(0)));
        let argon = axia_crypto_argon2(
            hash.as_mut_ptr(),
            32,
            work.as_mut_ptr(),
            work.len(),
            cfg,
            inputs,
            extra,
        );
        let blake = axia_crypto_blake2b(
            hash.as_mut_ptr(),
            32,
            key.as_ptr(),
            32,
            plain.as_ptr(),
            64,
            ptr::null(),
            0,
        );
        let seal = axia_crypto_seal(
            encrypted.as_mut_ptr(),
            tag.as_mut_ptr(),
            key.as_ptr(),
            nonce.as_ptr(),
            ptr::null(),
            0,
            plain.as_ptr(),
            64,
        );
        let open = axia_crypto_open(
            decoded.as_mut_ptr(),
            tag.as_ptr(),
            key.as_ptr(),
            nonce.as_ptr(),
            ptr::null(),
            0,
            encrypted.as_ptr(),
            64,
        );
        let chacha = axia_crypto_chacha20(
            encrypted.as_mut_ptr(),
            ptr::null(),
            64,
            key.as_ptr(),
            nonce.as_ptr(),
            0,
        );
        let compare = axia_crypto_verify16(tag.as_ptr(), tag.as_ptr());
        let allocations = ALLOCATIONS.with(|n| n.replace(None).unwrap());
        assert_eq!(
            (argon, blake, seal, open, chacha, compare),
            (0, 0, 0, 0, 0, 0)
        );
        assert_eq!(allocations, 0);
        assert_eq!(decoded, plain);
        axia_crypto_wipe(hash.as_mut_ptr(), 32);
        assert_eq!(hash, [0; 32]);
    }
}
