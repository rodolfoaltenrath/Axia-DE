//! SHA-512 and Ed25519 (RFC 8032) with a 32-byte seed as the private key: the public key, signing
//! and verification. The curve arithmetic is TweetNaCl's (public domain), the same as
//! server/src/pkg/crypto.c, so signatures are identical to the C ones. The field elements are 16
//! limbs of 16 bits in `i64`.
//!
//! Used to sign and verify the release manifests and the `.lpk` packages (docs/SIGNING.md).

// ---- SHA-512 ----

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538,
    0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118, 0xd807aa98a3030242, 0x12835b0145706fbe,
    0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235,
    0xc19bf174cf692694, 0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5, 0x983e5152ee66dfab,
    0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725,
    0x06ca6351e003826f, 0x142929670a0e6e70, 0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218,
    0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8, 0x19a4c116b8d2d0c8, 0x1e376c085141ab53,
    0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3, 0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b, 0xca273eceea26619c,
    0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6,
    0x113f9804bef90dae, 0x1b710b35131c471b, 0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// An incremental SHA-512.
#[derive(Clone)]
pub struct Sha512 {
    h: [u64; 8],
    len: u64,
    buf: [u8; 128],
    n: usize,
}

impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha512 {
    pub fn new() -> Self {
        Self {
            h: [
                0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
                0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
            ],
            len: 0,
            buf: [0; 128],
            n: 0,
        }
    }

    fn block(&mut self, p: &[u8]) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes(p[8 * i..8 * i + 8].try_into().unwrap());
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
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
        for (slot, v) in self.h.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.n > 0 {
            let k = (128 - self.n).min(data.len());
            self.buf[self.n..self.n + k].copy_from_slice(&data[..k]);
            self.n += k;
            data = &data[k..];
            if self.n < 128 {
                return;
            }
            let buf = self.buf;
            self.block(&buf);
            self.n = 0;
        }
        while data.len() >= 128 {
            self.block(&data[..128]);
            data = &data[128..];
        }
        self.buf[..data.len()].copy_from_slice(data);
        self.n = data.len();
    }

    pub fn finish(mut self) -> [u8; 64] {
        let bits = self.len.wrapping_mul(8);
        let pad_len = if self.n < 112 { 112 } else { 240 } - self.n;
        let mut pad = [0u8; 144];
        pad[0] = 0x80;
        pad[pad_len + 8..pad_len + 16].copy_from_slice(&bits.to_be_bytes()); // after a 64-bit zero high half
        let keep = self.len;
        self.update(&pad[..pad_len + 16]);
        self.len = keep;
        let mut out = [0u8; 64];
        for (chunk, v) in out.chunks_mut(8).zip(self.h) {
            chunk.copy_from_slice(&v.to_be_bytes());
        }
        out
    }
}

pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut s = Sha512::new();
    s.update(data);
    s.finish()
}

// ---- Ed25519 ----

type Gf = [i64; 16];

const GF0: Gf = [0; 16];
const GF1: Gf = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const D: Gf = [0x78a3, 0x1359, 0x4dca, 0x75eb, 0xd8ab, 0x4141, 0x0a4d, 0x0070, 0xe898, 0x7779, 0x4079, 0x8cc7, 0xfe73, 0x2b6f, 0x6cee, 0x5203];
const D2: Gf = [0xf159, 0x26b2, 0x9b94, 0xebd6, 0xb156, 0x8283, 0x149a, 0x00e0, 0xd130, 0xeef3, 0x80f2, 0x198e, 0xfce7, 0x56df, 0xd9dc, 0x2406];
const X: Gf = [0xd51a, 0x8f25, 0x2d60, 0xc956, 0xa7b2, 0x9525, 0xc760, 0x692c, 0xdc5c, 0xfdd6, 0xe231, 0xc0a4, 0x53fe, 0xcd6e, 0x36d3, 0x2169];
const Y: Gf = [0x6658, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666];
const I: Gf = [0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7, 0x3dfb, 0x0099, 0x2b4d, 0xdf0b, 0x4fc1, 0x2480, 0x2b83];
/// The group order.
const L: [i64; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

fn car25519(o: &mut Gf) {
    for i in 0..16 {
        o[i] += 1 << 16;
        let c = o[i] >> 16;
        let next = (i + 1) * usize::from(i < 15);
        o[next] += c - 1 + 37 * (c - 1) * i64::from(i == 15);
        o[i] -= c * 65536;
    }
}

fn sel25519(p: &mut Gf, q: &mut Gf, b: i64) {
    let c = !(b - 1);
    for i in 0..16 {
        let t = c & (p[i] ^ q[i]);
        p[i] ^= t;
        q[i] ^= t;
    }
}

fn pack25519(n: &Gf) -> [u8; 32] {
    let mut t = *n;
    let mut m = [0i64; 16];
    car25519(&mut t);
    car25519(&mut t);
    car25519(&mut t);
    for _ in 0..2 {
        m[0] = t[0] - 0xffed;
        for i in 1..15 {
            m[i] = t[i] - 0xffff - ((m[i - 1] >> 16) & 1);
            m[i - 1] &= 0xffff;
        }
        m[15] = t[15] - 0x7fff - ((m[14] >> 16) & 1);
        let b = (m[15] >> 16) & 1;
        m[14] &= 0xffff;
        sel25519(&mut t, &mut m, 1 - b);
    }
    let mut o = [0u8; 32];
    for i in 0..16 {
        o[2 * i] = (t[i] & 0xff) as u8;
        o[2 * i + 1] = (t[i] >> 8) as u8;
    }
    o
}

fn neq25519(a: &Gf, b: &Gf) -> bool {
    pack25519(a) != pack25519(b)
}

fn par25519(a: &Gf) -> u8 {
    pack25519(a)[0] & 1
}

fn unpack25519(n: &[u8]) -> Gf {
    let mut o = [0i64; 16];
    for i in 0..16 {
        o[i] = i64::from(n[2 * i]) + (i64::from(n[2 * i + 1]) << 8);
    }
    o[15] &= 0x7fff;
    o
}

fn fa(a: &Gf, b: &Gf) -> Gf {
    std::array::from_fn(|i| a[i] + b[i])
}
fn fz(a: &Gf, b: &Gf) -> Gf {
    std::array::from_fn(|i| a[i] - b[i])
}
fn fm(a: &Gf, b: &Gf) -> Gf {
    let mut t = [0i64; 31];
    for i in 0..16 {
        for j in 0..16 {
            t[i + j] += a[i] * b[j];
        }
    }
    for i in 0..15 {
        t[i] += 38 * t[i + 16];
    }
    let mut o: Gf = std::array::from_fn(|i| t[i]);
    car25519(&mut o);
    car25519(&mut o);
    o
}
fn fs(a: &Gf) -> Gf {
    fm(a, a)
}

fn pow2523(i: &Gf) -> Gf {
    let mut c = *i;
    for a in (0..=250).rev() {
        c = fs(&c);
        if a != 1 {
            c = fm(&c, i);
        }
    }
    c
}

fn inv25519(i: &Gf) -> Gf {
    let mut c = *i;
    for a in (0..=253).rev() {
        c = fs(&c);
        if a != 2 && a != 4 {
            c = fm(&c, i);
        }
    }
    c
}

type Point = [Gf; 4];

fn padd(p: &mut Point, q: &Point) {
    let a = fm(&fz(&p[1], &p[0]), &fz(&q[1], &q[0]));
    let b = fm(&fa(&p[0], &p[1]), &fa(&q[0], &q[1]));
    let c = fm(&fm(&p[3], &q[3]), &D2);
    let d = fm(&p[2], &q[2]);
    let d = fa(&d, &d);
    let (e, f, g, h) = (fz(&b, &a), fz(&d, &c), fa(&d, &c), fa(&b, &a));
    p[0] = fm(&e, &f);
    p[1] = fm(&h, &g);
    p[2] = fm(&g, &f);
    p[3] = fm(&e, &h);
}

fn cswap(p: &mut Point, q: &mut Point, b: i64) {
    for i in 0..4 {
        sel25519(&mut p[i], &mut q[i], b);
    }
}

fn ppack(p: &Point) -> [u8; 32] {
    let zi = inv25519(&p[2]);
    let (tx, ty) = (fm(&p[0], &zi), fm(&p[1], &zi));
    let mut r = pack25519(&ty);
    r[31] ^= par25519(&tx) << 7;
    r
}

fn scalarmult(q: &mut Point, s: &[u8]) -> Point {
    let mut p: Point = [GF0, GF1, GF1, GF0];
    for i in (0..=255).rev() {
        let b = i64::from((s[i / 8] >> (i & 7)) & 1);
        cswap(&mut p, q, b);
        let pc = p;
        padd(q, &pc);
        let pc = p;
        padd(&mut p, &pc);
        cswap(&mut p, q, b);
    }
    p
}

fn scalarbase(s: &[u8]) -> Point {
    let mut q: Point = [X, Y, GF1, fm(&X, &Y)];
    scalarmult(&mut q, s)
}

fn mod_l(x: &mut [i64; 64]) -> [u8; 32] {
    for i in (32..64).rev() {
        let mut carry = 0i64;
        let mut j = i - 32;
        while j < i - 12 {
            x[j] += carry - 16 * x[i] * L[j - (i - 32)];
            carry = (x[j] + 128) >> 8;
            x[j] -= carry * 256;
            j += 1;
        }
        x[j] += carry;
        x[i] = 0;
    }
    let mut carry = 0i64;
    for j in 0..32 {
        x[j] += carry - (x[31] >> 4) * L[j];
        carry = x[j] >> 8;
        x[j] &= 255;
    }
    for j in 0..32 {
        x[j] -= carry * L[j];
    }
    let mut r = [0u8; 32];
    for i in 0..32 {
        x[i + 1] += x[i] >> 8;
        r[i] = (x[i] & 255) as u8;
    }
    r
}

/// A 64-byte hash reduced modulo the group order.
fn reduce(h: &[u8; 64]) -> [u8; 32] {
    let mut x: [i64; 64] = std::array::from_fn(|i| i64::from(h[i]));
    mod_l(&mut x)
}

/// The point with `-A` for a public key, or `None` if it is not on the curve.
fn unpackneg(p: &[u8; 32]) -> Option<Point> {
    let r2 = GF1;
    let r1 = unpack25519(p);
    let mut num = fs(&r1);
    let mut den = fm(&num, &D);
    num = fz(&num, &r2);
    den = fa(&r2, &den);
    let den2 = fs(&den);
    let den4 = fs(&den2);
    let den6 = fm(&den4, &den2);
    let mut t = fm(&den6, &num);
    t = fm(&t, &den);
    t = pow2523(&t);
    t = fm(&t, &num);
    t = fm(&t, &den);
    t = fm(&t, &den);
    let mut r0 = fm(&t, &den);
    let mut chk = fm(&fs(&r0), &den);
    if neq25519(&chk, &num) {
        r0 = fm(&r0, &I);
    }
    chk = fm(&fs(&r0), &den);
    if neq25519(&chk, &num) {
        return None;
    }
    if par25519(&r0) == p[31] >> 7 {
        r0 = fz(&GF0, &r0);
    }
    let r3 = fm(&r0, &r1);
    Some([r0, r1, r2, r3])
}

/// Does `sig` (R || S) sign `msg` with the public key `public`? A signature with S >= L is rejected.
pub fn verify(public: &[u8; 32], sig: &[u8; 64], msg: &[u8]) -> bool {
    for i in (0..32).rev() {
        let s = i64::from(sig[32 + i]);
        if s < L[i] {
            break;
        }
        if s > L[i] || i == 0 {
            return false;
        }
    }
    let Some(mut q) = unpackneg(public) else { return false };
    let mut hash = Sha512::new();
    hash.update(&sig[..32]);
    hash.update(public);
    hash.update(msg);
    let h = reduce(&hash.finish());
    let mut p = scalarmult(&mut q, &h);
    let q = scalarbase(&sig[32..]);
    padd(&mut p, &q);
    ppack(&p)[..] == sig[..32]
}

fn expand(seed: &[u8; 32]) -> [u8; 64] {
    let mut d = sha512(seed);
    d[0] &= 248;
    d[31] &= 127;
    d[31] |= 64;
    d
}

/// The public key of a 32-byte seed.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    ppack(&scalarbase(&expand(seed)))
}

/// The signature of `msg` with the 32-byte seed (deterministic).
pub fn sign(seed: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    let d = expand(seed);
    let public = ppack(&scalarbase(&d));
    let mut hash = Sha512::new();
    hash.update(&d[32..]);
    hash.update(msg);
    let r = reduce(&hash.finish());
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&ppack(&scalarbase(&r)));
    let mut hash = Sha512::new();
    hash.update(&sig[..32]);
    hash.update(&public);
    hash.update(msg);
    let h = reduce(&hash.finish());
    let mut x = [0i64; 64];
    for i in 0..32 {
        x[i] = i64::from(r[i]);
    }
    for i in 0..32 {
        for j in 0..32 {
            x[i + j] += i64::from(h[i]) * i64::from(d[j]);
        }
    }
    sig[32..].copy_from_slice(&mod_l(&mut x));
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len() / 2).map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    fn arr<const N: usize>(text: &str) -> [u8; N] {
        hex(text).try_into().unwrap()
    }

    #[test]
    fn sha512_matches_the_nist_vectors_in_pieces_and_whole() {
        assert_eq!(
            sha512(b"abc").to_vec(),
            hex("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f")
        );
        assert_eq!(
            sha512(b"").to_vec(),
            hex("cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e")
        );
        let text: Vec<u8> = (0..1000u32).map(|i| (i * 7 + 3) as u8).collect();
        let mut pieces = Sha512::new();
        for chunk in text.chunks(37) {
            pieces.update(chunk);
        }
        assert_eq!(pieces.finish(), sha512(&text));
        // lengths around the padding boundaries
        for n in [111, 112, 113, 127, 128, 129, 239, 240, 241] {
            let data = vec![0x61u8; n];
            let mut halves = Sha512::new();
            halves.update(&data[..n / 2]);
            halves.update(&data[n / 2..]);
            assert_eq!(halves.finish(), sha512(&data), "{n}");
        }
    }

    #[test]
    fn rfc_8032_vectors() {
        let cases = [
            ("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "",
             "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"),
            ("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb", "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c", "72",
             "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"),
        ];
        for (seed, public, msg, sig) in &cases {
            let seed: [u8; 32] = arr(seed);
            let public: [u8; 32] = arr(public);
            let (msg, sig): (Vec<u8>, [u8; 64]) = (hex(msg), arr(sig));
            assert_eq!(public_key(&seed), public);
            assert_eq!(sign(&seed, &msg), sig);
            assert!(verify(&public, &sig, &msg));
            let mut bad = sig;
            bad[40] ^= 1;
            assert!(!verify(&public, &bad, &msg));
            assert!(!verify(&public, &sig, b"other"));
        }
    }

    #[test]
    fn signatures_round_trip_for_many_seeds_and_lengths() {
        for n in 0..24u8 {
            let seed: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(n).wrapping_add(n));
            let msg: Vec<u8> = (0..usize::from(n) * 13).map(|i| (i * 31) as u8).collect();
            let (public, sig) = (public_key(&seed), sign(&seed, &msg));
            assert!(verify(&public, &sig, &msg), "{n}");
            assert_eq!(sig, sign(&seed, &msg));
        }
    }

    #[test]
    fn a_signature_with_s_at_or_above_the_order_is_rejected() {
        let seed = [7u8; 32];
        let (public, mut sig) = (public_key(&seed), sign(&seed, b"msg"));
        sig[32..].copy_from_slice(&L.map(|v| v as u8));
        assert!(!verify(&public, &sig, b"msg"));
        assert!(!verify(&[0xff; 32], &sign(&seed, b"msg"), b"msg"));
    }
}
