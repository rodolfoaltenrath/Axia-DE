//! Rust port of `server/src/vault/vcrypto.c` with the C ABI of `vcrypto.h`: the primitives that
//! foreign formats still need — AES-128/192/256 (block and CBC) for CTAP2 PIN protocol 1, KDBX
//! and Secret Service; SHA-1 and HMAC-SHA1/256/512 for TOTP, CTAP2 and KDBX; HKDF-SHA256
//! (RFC 5869). Native vault files use the RustCrypto primitives in `lib.rs`.
//!
//! Nothing here allocates: the vault links this crate with its secrets-arena allocator and
//! every buffer that held key material is wiped before returning. SHA-256/512 are private to
//! this module (`server/src/pkg/crypto.c` keeps the exported `sha256`/`sha512` symbols).
use std::os::raw::{c_int, c_long, c_void};
use std::{ptr, slice};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

unsafe fn sl<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if n == 0 || p.is_null() {
        &[]
    } else {
        slice::from_raw_parts(p, n)
    }
}

// ================= AES =================

const SB: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];
const ISB: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[SB[i] as usize] = i as u8;
        i += 1;
    }
    t
};

fn xt(x: u8) -> u8 {
    (x << 1) ^ ((x >> 7) * 0x1b)
}
fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = xt(a);
        b >>= 1;
    }
    r
}
fn sub_word(t: u32) -> u32 {
    (SB[(t >> 24) as usize] as u32) << 24
        | (SB[((t >> 16) & 255) as usize] as u32) << 16
        | (SB[((t >> 8) & 255) as usize] as u32) << 8
        | SB[(t & 255) as usize] as u32
}

/// Round keys (the same for encryption and decryption). Layout of the C `Aes`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Aes {
    rk: [u32; 60],
    nr: c_int,
}

fn expand(key: &[u8], nk: usize) -> Aes {
    let mut a = Aes { rk: [0; 60], nr: nk as c_int + 6 };
    let total = 4 * (nk + 7);
    for i in 0..nk {
        a.rk[i] = u32::from_be_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
    }
    let mut rcon = 1u8;
    for i in nk..total {
        let mut t = a.rk[i - 1];
        if i % nk == 0 {
            t = sub_word(t.rotate_left(8)) ^ (rcon as u32) << 24;
            rcon = xt(rcon);
        } else if nk > 6 && i % nk == 4 {
            t = sub_word(t);
        }
        a.rk[i] = a.rk[i - nk] ^ t;
    }
    a
}

fn add_rk(s: &mut [u8; 16], rk: &[u32]) {
    for c in 0..4 {
        let w = rk[c].to_be_bytes();
        for f in 0..4 {
            s[4 * c + f] ^= w[f];
        }
    }
}

fn rounds(a: &Aes) -> usize {
    (a.nr as usize).clamp(10, 14)
}

fn enc_block(a: &Aes, input: &[u8; 16]) -> [u8; 16] {
    let nr = rounds(a);
    let mut s = *input;
    let mut t = [0u8; 16];
    add_rk(&mut s, &a.rk);
    for r in 1..=nr {
        for b in s.iter_mut() {
            *b = SB[*b as usize];
        }
        // ShiftRows: row f is shifted by f positions (state in 4-byte columns)
        for c in 0..4 {
            for f in 0..4 {
                t[4 * c + f] = s[4 * ((c + f) & 3) + f];
            }
        }
        if r != nr {
            for c in 0..4 {
                let (a0, a1, a2, a3) = (t[4 * c], t[4 * c + 1], t[4 * c + 2], t[4 * c + 3]);
                s[4 * c] = xt(a0) ^ xt(a1) ^ a1 ^ a2 ^ a3;
                s[4 * c + 1] = a0 ^ xt(a1) ^ xt(a2) ^ a2 ^ a3;
                s[4 * c + 2] = a0 ^ a1 ^ xt(a2) ^ xt(a3) ^ a3;
                s[4 * c + 3] = xt(a0) ^ a0 ^ a1 ^ a2 ^ xt(a3);
            }
        } else {
            s = t;
        }
        add_rk(&mut s, &a.rk[4 * r..]);
    }
    t.zeroize();
    let out = s;
    s.zeroize();
    out
}

fn dec_block(a: &Aes, input: &[u8; 16]) -> [u8; 16] {
    let nr = rounds(a);
    let mut s = *input;
    let mut t = [0u8; 16];
    add_rk(&mut s, &a.rk[4 * nr..]);
    for r in (0..nr).rev() {
        // InvShiftRows + InvSubBytes
        for c in 0..4 {
            for f in 0..4 {
                t[4 * ((c + f) & 3) + f] = ISB[s[4 * c + f] as usize];
            }
        }
        add_rk(&mut t, &a.rk[4 * r..]);
        if r != 0 {
            for c in 0..4 {
                let (a0, a1, a2, a3) = (t[4 * c], t[4 * c + 1], t[4 * c + 2], t[4 * c + 3]);
                s[4 * c] = gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9);
                s[4 * c + 1] = gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13);
                s[4 * c + 2] = gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11);
                s[4 * c + 3] = gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14);
            }
        } else {
            s = t;
        }
    }
    t.zeroize();
    let out = s;
    s.zeroize();
    out
}

unsafe fn rd16(p: *const u8) -> [u8; 16] {
    ptr::read(p as *const [u8; 16])
}
unsafe fn wr16(p: *mut u8, v: [u8; 16]) {
    ptr::write(p as *mut [u8; 16], v)
}

/// 128, 192 or 256 key bits; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn aes_init(a: *mut Aes, key: *const u8, keybits: c_int) -> c_int {
    if a.is_null() || key.is_null() || !matches!(keybits, 128 | 192 | 256) {
        return -1;
    }
    let nk = (keybits / 32) as usize;
    let mut k = [0u8; 32];
    ptr::copy_nonoverlapping(key, k.as_mut_ptr(), nk * 4);
    ptr::write(a, expand(&k[..nk * 4], nk));
    k.zeroize();
    0
}

#[no_mangle]
pub unsafe extern "C" fn aes_wipe(a: *mut Aes) {
    if !a.is_null() {
        for i in 0..60 {
            ptr::write_volatile(ptr::addr_of_mut!((*a).rk[i]), 0);
        }
        ptr::write_volatile(ptr::addr_of_mut!((*a).nr), 0);
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// `input` and `out` may be the same block.
#[no_mangle]
pub unsafe extern "C" fn aes_encrypt(a: *const Aes, input: *const u8, out: *mut u8) {
    if a.is_null() || input.is_null() || out.is_null() {
        return;
    }
    wr16(out, enc_block(&*a, &rd16(input)));
}

#[no_mangle]
pub unsafe extern "C" fn aes_decrypt(a: *const Aes, input: *const u8, out: *mut u8) {
    if a.is_null() || input.is_null() || out.is_null() {
        return;
    }
    wr16(out, dec_block(&*a, &rd16(input)));
}

/// CBC without padding: `len` a multiple of 16 (`input` and `out` may be the same buffer).
/// `iv` is updated to the last ciphertext block.
#[no_mangle]
pub unsafe extern "C" fn aes_cbc_encrypt(
    a: *const Aes,
    iv: *mut u8,
    input: *const u8,
    out: *mut u8,
    len: usize,
) -> c_int {
    if len % 16 != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if a.is_null() || iv.is_null() || input.is_null() || out.is_null() {
        return -1;
    }
    let a = &*a;
    let mut v = rd16(iv);
    let mut b;
    for off in (0..len).step_by(16) {
        b = rd16(input.add(off));
        for i in 0..16 {
            b[i] ^= v[i];
        }
        v = enc_block(a, &b);
        wr16(out.add(off), v);
        b.zeroize();
    }
    wr16(iv, v);
    0
}

#[no_mangle]
pub unsafe extern "C" fn aes_cbc_decrypt(
    a: *const Aes,
    iv: *mut u8,
    input: *const u8,
    out: *mut u8,
    len: usize,
) -> c_int {
    if len % 16 != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if a.is_null() || iv.is_null() || input.is_null() || out.is_null() {
        return -1;
    }
    let a = &*a;
    let mut v = rd16(iv);
    for off in (0..len).step_by(16) {
        let c = rd16(input.add(off));
        let mut p = dec_block(a, &c);
        for i in 0..16 {
            p[i] ^= v[i];
        }
        wr16(out.add(off), p);
        p.zeroize();
        v = c;
    }
    wr16(iv, v);
    0
}

/// Strips PKCS#7 padding from an already decrypted block: the unpadded length, or -1.
#[no_mangle]
pub unsafe extern "C" fn pkcs7_unpad(buf: *const u8, len: usize) -> c_long {
    if len == 0 || len % 16 != 0 || buf.is_null() {
        return -1;
    }
    let b = slice::from_raw_parts(buf, len);
    let n = b[len - 1];
    if n == 0 || n > 16 {
        return -1;
    }
    let mut bad = 0u8;
    for i in 0..n as usize {
        bad |= b[len - 1 - i] ^ n;
    }
    if bad != 0 {
        -1
    } else {
        (len - n as usize) as c_long
    }
}

// ================= SHA-1 / SHA-256 / SHA-512 =================

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// A Merkle–Damgård compression function: its chaining state is the implementing array.
trait Core: Sized + Copy + Zeroize {
    const BLOCK: usize;
    const OUT: usize;
    fn init() -> Self;
    fn compress(&mut self, block: &[u8]);
    fn emit(&self, out: &mut [u8]);
}

impl Core for [u32; 5] {
    const BLOCK: usize = 64;
    const OUT: usize = 20;
    fn init() -> Self {
        [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0]
    }
    fn compress(&mut self, p: &[u8]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = *self;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (h, v) in self.iter_mut().zip([a, b, c, d, e]) {
            *h = h.wrapping_add(v);
        }
        w.zeroize();
    }
    fn emit(&self, out: &mut [u8]) {
        for (i, h) in self.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&h.to_be_bytes());
        }
    }
}

impl Core for [u32; 8] {
    const BLOCK: usize = 64;
    const OUT: usize = 32;
    fn init() -> Self {
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]
    }
    fn compress(&mut self, p: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *self;
        for i in 0..64 {
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in self.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *x = x.wrapping_add(v);
        }
        w.zeroize();
    }
    fn emit(&self, out: &mut [u8]) {
        for (i, h) in self.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&h.to_be_bytes());
        }
    }
}

impl Core for [u64; 8] {
    const BLOCK: usize = 128;
    const OUT: usize = 64;
    fn init() -> Self {
        [
            0x6a09e667f3bcc908,
            0xbb67ae8584caa73b,
            0x3c6ef372fe94f82b,
            0xa54ff53a5f1d36f1,
            0x510e527fade682d1,
            0x9b05688c2b3e6c1f,
            0x1f83d9abfb41bd6b,
            0x5be0cd19137e2179,
        ]
    }
    fn compress(&mut self, p: &[u8]) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut q = [0u8; 8];
            q.copy_from_slice(&p[8 * i..8 * i + 8]);
            w[i] = u64::from_be_bytes(q);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *self;
        for i in 0..80 {
            let t1 = h
                .wrapping_add(e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K512[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in self.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *x = x.wrapping_add(v);
        }
        w.zeroize();
    }
    fn emit(&self, out: &mut [u8]) {
        for (i, h) in self.iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&h.to_be_bytes());
        }
    }
}

/// Buffers input into blocks. `buf` holds at least `C::BLOCK` bytes.
fn absorb<C: Core>(h: &mut C, len: &mut u64, buf: &mut [u8], n: &mut usize, mut d: &[u8]) {
    *len = len.wrapping_add(d.len() as u64);
    while !d.is_empty() {
        let k = (C::BLOCK - *n).min(d.len());
        buf[*n..*n + k].copy_from_slice(&d[..k]);
        *n += k;
        d = &d[k..];
        if *n == C::BLOCK {
            h.compress(&buf[..C::BLOCK]);
            *n = 0;
        }
    }
}

/// Pads, writes the digest to `out` (`C::OUT` bytes) and wipes the state.
fn conclude<C: Core>(h: &mut C, len: &mut u64, buf: &mut [u8], n: &mut usize, out: &mut [u8]) {
    let bits = len.wrapping_mul(8);
    let lenfield = if C::BLOCK == 128 { 16 } else { 8 };
    absorb(h, len, buf, n, &[0x80]);
    while *n != C::BLOCK - lenfield {
        absorb(h, len, buf, n, &[0]);
    }
    if lenfield == 16 {
        absorb(h, len, buf, n, &[0; 8]);
    }
    absorb(h, len, buf, n, &bits.to_be_bytes());
    h.emit(out);
    h.zeroize();
    buf.zeroize();
    len.zeroize();
    n.zeroize();
}

struct Md<C: Core> {
    h: C,
    len: u64,
    buf: [u8; 128],
    n: usize,
}
impl<C: Core> Md<C> {
    fn new() -> Self {
        Md { h: C::init(), len: 0, buf: [0; 128], n: 0 }
    }
    fn update(&mut self, d: &[u8]) {
        absorb(&mut self.h, &mut self.len, &mut self.buf, &mut self.n, d);
    }
    fn finish(mut self, out: &mut [u8]) {
        conclude(&mut self.h, &mut self.len, &mut self.buf, &mut self.n, out);
    }
}

/// Layout of the C `Sha1`.
#[repr(C)]
pub struct Sha1 {
    h: [u32; 5],
    len: u64,
    buf: [u8; 64],
    n: usize,
}

#[no_mangle]
pub unsafe extern "C" fn sha1_init(s: *mut Sha1) {
    if !s.is_null() {
        ptr::write(s, Sha1 { h: <[u32; 5]>::init(), len: 0, buf: [0; 64], n: 0 });
    }
}

#[no_mangle]
pub unsafe extern "C" fn sha1_update(s: *mut Sha1, data: *const c_void, len: usize) {
    if s.is_null() {
        return;
    }
    let s = &mut *s;
    if s.n >= 64 {
        return;
    }
    absorb(&mut s.h, &mut s.len, &mut s.buf, &mut s.n, sl(data as *const u8, len));
}

#[no_mangle]
pub unsafe extern "C" fn sha1_final(s: *mut Sha1, out: *mut u8) {
    if s.is_null() || out.is_null() {
        return;
    }
    let s = &mut *s;
    if s.n >= 64 {
        return;
    }
    let mut d = [0u8; 20];
    conclude(&mut s.h, &mut s.len, &mut s.buf, &mut s.n, &mut d);
    ptr::copy_nonoverlapping(d.as_ptr(), out, 20);
}

#[no_mangle]
pub unsafe extern "C" fn sha1(data: *const c_void, len: usize, out: *mut u8) {
    if out.is_null() {
        return;
    }
    let mut m = Md::<[u32; 5]>::new();
    m.update(sl(data as *const u8, len));
    let mut d = [0u8; 20];
    m.finish(&mut d);
    ptr::copy_nonoverlapping(d.as_ptr(), out, 20);
}

// ================= HMAC / HKDF =================

fn hmac<'a, C: Core>(key: &[u8], parts: impl Iterator<Item = &'a [u8]>, out: &mut [u8]) {
    let (bl, ol) = (C::BLOCK, C::OUT);
    let mut k = [0u8; 128];
    let mut pad = [0u8; 128];
    let mut inner = [0u8; 64];
    if key.len() > bl {
        let mut m = Md::<C>::new();
        m.update(key);
        m.finish(&mut k[..ol]);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    for i in 0..bl {
        pad[i] = k[i] ^ 0x36;
    }
    let mut m = Md::<C>::new();
    m.update(&pad[..bl]);
    for p in parts {
        m.update(p);
    }
    m.finish(&mut inner[..ol]);
    for i in 0..bl {
        pad[i] = k[i] ^ 0x5c;
    }
    let mut m = Md::<C>::new();
    m.update(&pad[..bl]);
    m.update(&inner[..ol]);
    m.finish(&mut out[..ol]);
    k.zeroize();
    pad.zeroize();
    inner.zeroize();
}

#[no_mangle]
pub unsafe extern "C" fn hmac_sha1(key: *const u8, klen: usize, msg: *const u8, len: usize, out: *mut u8) {
    if out.is_null() {
        return;
    }
    hmac::<[u32; 5]>(sl(key, klen), [sl(msg, len)].into_iter(), slice::from_raw_parts_mut(out, 20));
}

#[no_mangle]
pub unsafe extern "C" fn hmac_sha256(key: *const u8, klen: usize, msg: *const u8, len: usize, out: *mut u8) {
    if out.is_null() {
        return;
    }
    hmac::<[u32; 8]>(sl(key, klen), [sl(msg, len)].into_iter(), slice::from_raw_parts_mut(out, 32));
}

/// Multi-part HMAC-SHA256 (KDBX: index || length || block). `parts`/`lens` end with a NULL part.
#[no_mangle]
pub unsafe extern "C" fn hmac_sha256_v(
    key: *const u8,
    klen: usize,
    parts: *const *const u8,
    lens: *const usize,
    out: *mut u8,
) {
    if out.is_null() || parts.is_null() || lens.is_null() {
        return;
    }
    let it = (0..).map_while(|i| {
        let p = *parts.add(i);
        if p.is_null() {
            None
        } else {
            Some(sl(p, *lens.add(i)))
        }
    });
    hmac::<[u32; 8]>(sl(key, klen), it, slice::from_raw_parts_mut(out, 32));
}

#[no_mangle]
pub unsafe extern "C" fn hmac_sha512(key: *const u8, klen: usize, msg: *const u8, len: usize, out: *mut u8) {
    if out.is_null() {
        return;
    }
    hmac::<[u64; 8]>(sl(key, klen), [sl(msg, len)].into_iter(), slice::from_raw_parts_mut(out, 64));
}

/// HKDF-SHA256 (RFC 5869) of up to 255*32 bytes; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn hkdf_sha256(
    salt: *const u8,
    slen: usize,
    ikm: *const u8,
    ilen: usize,
    info: *const u8,
    infolen: usize,
    out: *mut u8,
    outlen: usize,
) -> c_int {
    if outlen > 255 * 32 {
        return -1;
    }
    if outlen == 0 {
        return 0;
    }
    if out.is_null() {
        return -1;
    }
    let out = slice::from_raw_parts_mut(out, outlen);
    let zero = [0u8; 32];
    let salt = if salt.is_null() || slen == 0 { &zero[..] } else { sl(salt, slen) };
    let info = sl(info, infolen);
    let mut prk = [0u8; 32];
    let mut t = [0u8; 32];
    hmac::<[u32; 8]>(salt, [sl(ikm, ilen)].into_iter(), &mut prk);
    let (mut done, mut tlen) = (0usize, 0usize);
    let mut i = 1u8;
    while done < outlen {
        let counter = [i];
        let prev = t;
        hmac::<[u32; 8]>(&prk, [&prev[..tlen], info, &counter[..]].into_iter(), &mut t);
        tlen = 32;
        let k = (outlen - done).min(32);
        out[done..done + k].copy_from_slice(&t[..k]);
        done += k;
        i = i.wrapping_add(1);
    }
    prk.zeroize();
    t.zeroize();
    0
}

// ================= misc =================

extern "C" {
    fn getrandom(buf: *mut c_void, buflen: usize, flags: u32) -> isize;
    fn __errno_location() -> *mut c_int;
}

/// Fills `buf` from the kernel's random generator; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn vc_random(buf: *mut c_void, n: usize) -> c_int {
    if n != 0 && buf.is_null() {
        return -1;
    }
    let mut p = buf as *mut u8;
    let mut left = n;
    while left != 0 {
        let r = getrandom(p as *mut c_void, left, 0);
        if r < 0 {
            if *__errno_location() == 4 {
                continue; // EINTR
            }
            return -1;
        }
        p = p.add(r as usize);
        left -= r as usize;
    }
    0
}

/// 1 if both buffers are equal (constant time).
#[no_mangle]
pub unsafe extern "C" fn vc_equal(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    sl(a as *const u8, n).ct_eq(sl(b as *const u8, n)).unwrap_u8() as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
    fn aes(key: &[u8]) -> Aes {
        let mut a = Aes { rk: [0; 60], nr: 0 };
        assert_eq!(unsafe { aes_init(&mut a, key.as_ptr(), key.len() as c_int * 8) }, 0);
        a
    }
    fn digest<C: Core>(d: &[u8]) -> String {
        let mut m = Md::<C>::new();
        m.update(d);
        let mut o = vec![0u8; C::OUT];
        m.finish(&mut o);
        hex(&o)
    }

    #[test]
    fn aes_fips197() {
        let pt = unhex("00112233445566778899aabbccddeeff");
        for (k, ct) in [
            ("000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a"),
            ("000102030405060708090a0b0c0d0e0f1011121314151617", "dda97ca4864cdfe06eaf70a0ec0d7191"),
            (
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "8ea2b7ca516745bfeafc49904b496089",
            ),
        ] {
            let a = aes(&unhex(k));
            let mut b = [0u8; 16];
            unsafe { aes_encrypt(&a, pt.as_ptr(), b.as_mut_ptr()) };
            assert_eq!(hex(&b), ct);
            // in place
            unsafe { aes_decrypt(&a, b.as_ptr(), b.as_mut_ptr()) };
            assert_eq!(b.to_vec(), pt);
        }
        let mut a = Aes { rk: [0; 60], nr: 0 };
        let k = [0u8; 32];
        assert_eq!(unsafe { aes_init(&mut a, k.as_ptr(), 100) }, -1);
        assert_eq!(unsafe { aes_init(&mut a, std::ptr::null(), 128) }, -1);
    }

    #[test]
    fn aes_cbc_sp800_38a() {
        let a = aes(&unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"));
        let pt = unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e5130c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710");
        let ct = "f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d39f23369a9d9bacfa530e26304231461b2eb05e2c39be9fcda6c19078c6a9d1b";
        let iv0 = unhex("000102030405060708090a0b0c0d0e0f");
        let mut iv = iv0.clone();
        let mut out = vec![0u8; pt.len()];
        unsafe { aes_cbc_encrypt(&a, iv.as_mut_ptr(), pt.as_ptr(), out.as_mut_ptr(), pt.len()) };
        assert_eq!(hex(&out), ct);
        assert_eq!(iv, out[out.len() - 16..]);
        let mut iv = iv0.clone();
        unsafe { aes_cbc_decrypt(&a, iv.as_mut_ptr(), out.as_ptr(), out.as_mut_ptr(), out.len()) };
        assert_eq!(out, pt);
        assert_eq!(unsafe { aes_cbc_encrypt(&a, iv.as_mut_ptr(), pt.as_ptr(), out.as_mut_ptr(), 17) }, -1);
        // SP 800-38A F.2.1 (CBC-AES128), first block
        let a = aes(&unhex("2b7e151628aed2a6abf7158809cf4f3c"));
        let mut iv = iv0.clone();
        let mut o = [0u8; 16];
        unsafe { aes_cbc_encrypt(&a, iv.as_mut_ptr(), pt.as_ptr(), o.as_mut_ptr(), 16) };
        assert_eq!(hex(&o), "7649abac8119b246cee98e9b12e9197d");
    }

    #[test]
    fn pkcs7() {
        let mut p = [0u8; 32];
        p[20..].fill(12);
        assert_eq!(unsafe { pkcs7_unpad(p.as_ptr(), 32) }, 20);
        p[25] = 11;
        assert_eq!(unsafe { pkcs7_unpad(p.as_ptr(), 32) }, -1);
        assert_eq!(unsafe { pkcs7_unpad(p.as_ptr(), 0) }, -1);
        assert_eq!(unsafe { pkcs7_unpad(p.as_ptr(), 17) }, -1);
    }

    #[test]
    fn hashes() {
        assert_eq!(digest::<[u32; 5]>(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            digest::<[u32; 5]>(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            digest::<[u32; 8]>(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest::<[u64; 8]>(b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        let mut s = std::mem::MaybeUninit::<Sha1>::uninit();
        let mut o = [0u8; 20];
        unsafe {
            sha1_init(s.as_mut_ptr());
            let a = [b'a'; 1000];
            for _ in 0..1000 {
                sha1_update(s.as_mut_ptr(), a.as_ptr() as *const c_void, a.len());
            }
            sha1_final(s.as_mut_ptr(), o.as_mut_ptr());
        }
        assert_eq!(hex(&o), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
        assert_eq!(std::mem::size_of::<Sha1>(), 104);
        assert_eq!(std::mem::size_of::<Aes>(), 244);
    }

    #[test]
    fn hmac_vectors() {
        let k = [0x0bu8; 20];
        let mut h = [0u8; 64];
        unsafe {
            hmac_sha1(k.as_ptr(), 20, b"Hi There".as_ptr(), 8, h.as_mut_ptr());
            assert_eq!(hex(&h[..20]), "b617318655057264e28bc0b6fb378c8ef146be00");
            hmac_sha1(b"Jefe".as_ptr(), 4, b"what do ya want for nothing?".as_ptr(), 28, h.as_mut_ptr());
            assert_eq!(hex(&h[..20]), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
            let k80 = [0xaau8; 80];
            let m = b"Test Using Larger Than Block-Size Key - Hash Key First";
            hmac_sha1(k80.as_ptr(), 80, m.as_ptr(), m.len(), h.as_mut_ptr());
            assert_eq!(hex(&h[..20]), "aa4ae5e15272d00e95705637ce8a3b55ed402112");
            hmac_sha256(k.as_ptr(), 20, b"Hi There".as_ptr(), 8, h.as_mut_ptr());
            assert_eq!(hex(&h[..32]), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
            hmac_sha256(b"Jefe".as_ptr(), 4, b"what do ya want for nothing?".as_ptr(), 28, h.as_mut_ptr());
            assert_eq!(hex(&h[..32]), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
            hmac_sha512(k.as_ptr(), 20, b"Hi There".as_ptr(), 8, h.as_mut_ptr());
            assert_eq!(hex(&h), "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854");
            // multi-part equals the one-shot form
            let parts = [b"Hi ".as_ptr(), b"There".as_ptr(), std::ptr::null()];
            let lens = [3usize, 5, 0];
            let mut h2 = [0u8; 32];
            hmac_sha256_v(k.as_ptr(), 20, parts.as_ptr(), lens.as_ptr(), h2.as_mut_ptr());
            hmac_sha256(k.as_ptr(), 20, b"Hi There".as_ptr(), 8, h.as_mut_ptr());
            assert_eq!(h2[..], h[..32]);
        }
    }

    /// HMAC and digests across the block-size edges against Python's hashlib/hmac.
    #[test]
    fn matrix_against_reference() {
        let mut blob = Vec::new();
        for alg in 0..3 {
            for kl in [0usize, 1, 63, 64, 65, 127, 128, 129, 200] {
                let key: Vec<u8> = (0..kl).map(|i| (i * 7 + kl) as u8).collect();
                for ml in [0usize, 1, 55, 56, 63, 64, 111, 112, 127, 128, 1000] {
                    let msg: Vec<u8> = (0..ml).map(|i| (i * 13 + ml + 3) as u8).collect();
                    let mut o = [0u8; 64];
                    let n = match alg {
                        0 => {
                            unsafe { hmac_sha1(key.as_ptr(), kl, msg.as_ptr(), ml, o.as_mut_ptr()) };
                            blob.extend_from_slice(&o[..20]);
                            let mut d = [0u8; 20];
                            unsafe { sha1(msg.as_ptr() as *const c_void, ml, d.as_mut_ptr()) };
                            blob.extend_from_slice(&d);
                            continue;
                        }
                        1 => {
                            unsafe { hmac_sha256(key.as_ptr(), kl, msg.as_ptr(), ml, o.as_mut_ptr()) };
                            let mut m = Md::<[u32; 8]>::new();
                            m.update(&msg);
                            let mut d = [0u8; 32];
                            m.finish(&mut d);
                            blob.extend_from_slice(&o[..32]);
                            blob.extend_from_slice(&d);
                            continue;
                        }
                        _ => 64,
                    };
                    unsafe { hmac_sha512(key.as_ptr(), kl, msg.as_ptr(), ml, o.as_mut_ptr()) };
                    let mut m = Md::<[u64; 8]>::new();
                    m.update(&msg);
                    let mut d = [0u8; 64];
                    m.finish(&mut d);
                    blob.extend_from_slice(&o[..n]);
                    blob.extend_from_slice(&d);
                }
            }
        }
        assert_eq!(digest::<[u32; 8]>(&blob), "502117cf804816452094b5acb6a9574fc6974b291ceb149c2e9bad1d1abd10dc");
    }

    #[test]
    fn hkdf_rfc5869() {
        let ikm = [0x0bu8; 22];
        let salt = unhex("000102030405060708090a0b0c");
        let info = unhex("f0f1f2f3f4f5f6f7f8f9");
        let mut okm = [0u8; 42];
        unsafe {
            assert_eq!(
                hkdf_sha256(salt.as_ptr(), 13, ikm.as_ptr(), 22, info.as_ptr(), 10, okm.as_mut_ptr(), 42),
                0
            );
            assert_eq!(
                hex(&okm),
                "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
            );
            assert_eq!(
                hkdf_sha256(std::ptr::null(), 0, ikm.as_ptr(), 22, std::ptr::null(), 0, okm.as_mut_ptr(), 42),
                0
            );
            assert_eq!(
                hex(&okm),
                "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
            );
            let mut big = vec![0u8; 255 * 32 + 1];
            assert_eq!(
                hkdf_sha256(std::ptr::null(), 0, ikm.as_ptr(), 22, std::ptr::null(), 0, big.as_mut_ptr(), big.len()),
                -1
            );
            // the full 255 blocks (the counter reaches 255)
            assert_eq!(
                hkdf_sha256(std::ptr::null(), 0, ikm.as_ptr(), 22, std::ptr::null(), 0, big.as_mut_ptr(), 255 * 32),
                0
            );
        }
    }

    #[test]
    fn random_and_equal() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        unsafe {
            assert_eq!(vc_random(a.as_mut_ptr() as *mut c_void, 32), 0);
            assert_eq!(vc_random(b.as_mut_ptr() as *mut c_void, 32), 0);
            assert_ne!(a, b);
            assert_eq!(vc_equal(a.as_ptr() as *const c_void, a.as_ptr() as *const c_void, 32), 1);
            assert_eq!(vc_equal(a.as_ptr() as *const c_void, b.as_ptr() as *const c_void, 32), 0);
            assert_eq!(vc_equal(std::ptr::null(), std::ptr::null(), 0), 1);
        }
    }
}
