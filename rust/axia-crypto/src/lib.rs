//! RustCrypto primitives with a narrow C ABI. No operation allocates: Argon2
//! borrows caller-owned protected work memory, all other state stays on stack.
//! Pointer contracts: inputs cover their lengths; output/work is exclusive and
//! disjoint from keys/nonces/AD. Message input/output may be equal or disjoint.
use chacha20::cipher::{inout::InOutBuf, KeyIvInit, StreamCipher, StreamCipherSeek};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305};
use std::{ptr, slice};
use zeroize::{Zeroize, Zeroizing};

unsafe fn bytes<'a>(p: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if p.is_null() || len > isize::MAX as usize {
        return None;
    }
    Some(slice::from_raw_parts(p, len))
}
fn overlap(a: *const u8, an: usize, b: *const u8, bn: usize) -> bool {
    an != 0
        && bn != 0
        && (a as usize) < (b as usize).saturating_add(bn)
        && (b as usize) < (a as usize).saturating_add(an)
}
unsafe fn inout<'a>(input: *const u8, out: *mut u8, len: usize) -> Option<InOutBuf<'a, 'a, u8>> {
    if len == 0 {
        let p = ptr::NonNull::<u8>::dangling().as_ptr();
        return Some(InOutBuf::from_raw(p, p, 0));
    }
    if input.is_null()
        || out.is_null()
        || len > isize::MAX as usize
        || (input != out && overlap(input, len, out, len))
    {
        return None;
    }
    // RustCrypto requires initialized output even for disjoint buffers. C's
    // malloc-backed output may be uninitialized. In-place input is already valid.
    if input != out {
        ptr::write_bytes(out, 0, len);
    }
    Some(InOutBuf::from_raw(input, out, len))
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_wipe(p: *mut u8, len: usize) {
    if !p.is_null() && len <= isize::MAX as usize {
        // Initialize before constructing a Rust slice (C may pass unused scratch).
        // Volatile writes plus a compiler fence make this an observable erasure.
        for i in 0..len {
            ptr::write_volatile(p.add(i), 0);
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_verify16(a: *const u8, b: *const u8) -> i32 {
    use subtle::ConstantTimeEq;
    let (Some(a), Some(b)) = (bytes(a, 16), bytes(b, 16)) else {
        return -1;
    };
    if bool::from(a.ct_eq(b)) {
        0
    } else {
        -1
    }
}

/// Hash output size is part of BLAKE2's parameter block, never a truncation of
/// BLAKE2b-512. Two message slices avoid concatenating secret KDF inputs on heap.
fn blake2b(key: &[u8], a: &[u8], b: &[u8], out: &mut [u8]) -> Result<(), ()> {
    use blake2::{
        digest::{
            block_api::{Buffer, UpdateCore, VariableOutputCore},
            Output,
        },
        Blake2bVarCore,
    };
    if key.len() > 64 || out.is_empty() || out.len() > 64 {
        return Err(());
    }
    let mut core = Blake2bVarCore::new_with_params(&[], &[], key.len(), out.len());
    let mut buffer = Buffer::<Blake2bVarCore>::default();
    if !key.is_empty() {
        let mut padded = Zeroizing::new([0u8; 128]);
        padded[..key.len()].copy_from_slice(key);
        buffer.digest_blocks(&*padded, |blocks| core.update_blocks(blocks));
    }
    buffer.digest_blocks(a, |blocks| core.update_blocks(blocks));
    buffer.digest_blocks(b, |blocks| core.update_blocks(blocks));
    let mut full = Output::<Blake2bVarCore>::default();
    core.finalize_variable_core(&mut buffer, &mut full);
    out.copy_from_slice(&full[..out.len()]);
    full.as_mut_slice().zeroize();
    buffer.zeroize();
    Ok(())
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_blake2b(
    out: *mut u8,
    outlen: usize,
    key: *const u8,
    keylen: usize,
    a: *const u8,
    alen: usize,
    b: *const u8,
    blen: usize,
) -> i32 {
    if outlen == 0 {
        return 0;
    }
    if out.is_null() || outlen > 64 {
        return -1;
    }
    let (Some(key), Some(a), Some(b)) = (bytes(key, keylen), bytes(a, alen), bytes(b, blen)) else {
        return -1;
    };
    let mut result = Zeroizing::new([0u8; 64]);
    if blake2b(key, a, b, &mut result[..outlen]).is_err() {
        return -1;
    }
    ptr::copy_nonoverlapping(result.as_ptr(), out, outlen);
    0
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_seal(
    out: *mut u8,
    tag: *mut u8,
    key: *const u8,
    nonce: *const u8,
    ad: *const u8,
    adlen: usize,
    input: *const u8,
    len: usize,
) -> i32 {
    if tag.is_null() {
        return -1;
    }
    let (Some(key), Some(nonce), Some(ad)) = (bytes(key, 32), bytes(nonce, 24), bytes(ad, adlen))
    else {
        return -1;
    };
    let cipher = XChaCha20Poly1305::new(key.try_into().unwrap());
    let Some(buffer) = inout(input, out, len) else {
        return -1;
    };
    let Ok(mac) = cipher.encrypt_inout_detached(nonce.try_into().unwrap(), ad, buffer) else {
        axia_crypto_wipe(out, len);
        return -1;
    };
    ptr::copy_nonoverlapping(mac.as_ptr(), tag, 16);
    0
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_open(
    out: *mut u8,
    tag: *const u8,
    key: *const u8,
    nonce: *const u8,
    ad: *const u8,
    adlen: usize,
    input: *const u8,
    len: usize,
) -> i32 {
    let (Some(key), Some(nonce), Some(ad), Some(tag)) = (
        bytes(key, 32),
        bytes(nonce, 24),
        bytes(ad, adlen),
        bytes(tag, 16),
    ) else {
        return -1;
    };
    let cipher = XChaCha20Poly1305::new(key.try_into().unwrap());
    let Some(buffer) = inout(input, out, len) else {
        return -1;
    };
    if cipher
        .decrypt_inout_detached(
            nonce.try_into().unwrap(),
            ad,
            buffer,
            tag.try_into().unwrap(),
        )
        .is_err()
    {
        axia_crypto_wipe(out, len);
        return -1;
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_chacha20(
    out: *mut u8,
    input: *const u8,
    len: usize,
    key: *const u8,
    nonce: *const u8,
    counter: u32,
) -> i32 {
    let (Some(key), Some(nonce)) = (bytes(key, 32), bytes(nonce, 12)) else {
        return -1;
    };
    let mut cipher = chacha20::ChaCha20::new(key.try_into().unwrap(), nonce.try_into().unwrap());
    if cipher.try_seek(u64::from(counter) * 64).is_err() {
        return -1;
    }
    let buffer = if input.is_null() && len > 0 {
        if out.is_null() || len > isize::MAX as usize {
            return -1;
        }
        ptr::write_bytes(out, 0, len);
        inout(out, out, len)
    } else {
        inout(input, out, len)
    };
    let Some(buffer) = buffer else {
        return -1;
    };
    if cipher.try_apply_keystream_inout(buffer).is_err() {
        axia_crypto_wipe(out, len);
        return -1;
    }
    0
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArgonConfig {
    pub algorithm: u32,
    pub memory_kib: u32,
    pub passes: u32,
    pub lanes: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArgonInputs {
    pub password: *const u8,
    pub salt: *const u8,
    pub password_len: u32,
    pub salt_len: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArgonExtras {
    pub secret: *const u8,
    pub ad: *const u8,
    pub secret_len: u32,
    pub ad_len: u32,
}
#[no_mangle]
pub extern "C" fn axia_crypto_argon_work_size(blocks: u32) -> usize {
    (blocks as usize)
        .checked_mul(1024)
        .and_then(|n| n.checked_add(63))
        .filter(|n| *n <= isize::MAX as usize)
        .unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn axia_crypto_argon2(
    out: *mut u8,
    outlen: usize,
    work: *mut u8,
    worklen: usize,
    config: ArgonConfig,
    input: ArgonInputs,
    extras: ArgonExtras,
) -> i32 {
    use argon2::{Algorithm, Argon2, Block, Params, Version};
    if out.is_null()
        || !(4..=64).contains(&outlen)
        || work.is_null()
        || worklen > isize::MAX as usize
        || overlap(out, outlen, work, worklen)
    {
        return -1;
    }
    let algorithm = match config.algorithm {
        0 => Algorithm::Argon2d,
        1 => Algorithm::Argon2i,
        2 => Algorithm::Argon2id,
        _ => return -1,
    };
    let Ok(params) = Params::new(config.memory_kib, config.passes, config.lanes, Some(outlen))
    else {
        return -1;
    };
    let (Some(password), Some(salt), Some(secret), Some(ad)) = (
        bytes(input.password, input.password_len as usize),
        bytes(input.salt, input.salt_len as usize),
        bytes(extras.secret, extras.secret_len as usize),
        bytes(extras.ad, extras.ad_len as usize),
    ) else {
        return -1;
    };
    let Ok(ctx) = Argon2::new_with_secret(secret, algorithm, Version::V0x13, params.clone()) else {
        return -1;
    };
    let offset = work.align_offset(std::mem::align_of::<Block>());
    let count = params.block_count();
    let Some(required) = count
        .checked_mul(std::mem::size_of::<Block>())
        .and_then(|n| n.checked_add(offset))
    else {
        return -1;
    };
    if required > worklen {
        return -1;
    }
    let aligned = work.add(offset).cast::<Block>();
    for i in 0..count {
        ptr::write(aligned.add(i), Block::new());
    }
    let memory = slice::from_raw_parts_mut(aligned, count);
    let mut result = Zeroizing::new([0u8; 64]);
    let status = ctx.hash_password_into_with_memory_and_ad(
        password,
        salt,
        &mut result[..outlen],
        &mut *memory,
        ad,
    );
    for block in memory {
        block.zeroize();
    }
    if status.is_err() {
        return -1;
    }
    ptr::copy_nonoverlapping(result.as_ptr(), out, outlen);
    0
}

// AES, SHA-1 and HMAC of the foreign formats (C ABI of server/src/vault/vcrypto.h).
pub mod vcrypto;
mod totp;
#[cfg(test)]
mod tests;
