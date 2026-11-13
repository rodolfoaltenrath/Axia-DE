//! Our own QR codes, encode and decode (ISO/IEC 18004): the C ABI of server/src/vault/qr.h, which
//! this replaced (the QR of a Wi-Fi network to join from a phone, the recovery key, a 2FA code
//! read from the image of an otpauth:// QR).
//!
//! Encode: the most compact mode (numeric, alphanumeric or byte) and the smallest version that
//! fits with the requested correction level; Reed-Solomon over GF(256) (polynomial 0x11d),
//! interleaved blocks, function patterns, format and version information (BCH) and the 8 masks
//! with the standard's penalty.
//!
//! Decode: luminance → binarization (global Otsu and Bradley local threshold, normal and inverted)
//! → finder patterns by the 1:1:3:1:1 ratio in rows, confirmed in columns → the three best (right
//! triangle) → side from the timing pattern and the version information → grid (affine, or
//! perspective with the alignment pattern) → format, mask, codewords, Reed-Solomon
//! (Berlekamp-Massey + Chien + Forney) → segments.
use std::ffi::c_int;
use std::sync::OnceLock;

// ================= tables =================

const ECC_PER_BLOCK: [[i8; 41]; 4] = [
    [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];
const NUM_BLOCKS: [[i8; 41]; 4] = [
    [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
    [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];
/// L M Q H in the format information.
const FORMAT_ECC_BITS: [i32; 4] = [1, 0, 3, 2];
const ALNUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

fn raw_modules(ver: i32) -> i32 {
    let mut r = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let na = ver / 7 + 2;
        r -= (25 * na - 10) * na - 55;
        if ver >= 7 {
            r -= 36;
        }
    }
    r
}
fn ecc_per_block(ecc: i32, ver: i32) -> i32 {
    i32::from(ECC_PER_BLOCK[ecc as usize][ver as usize])
}
fn num_blocks(ecc: i32, ver: i32) -> i32 {
    i32::from(NUM_BLOCKS[ecc as usize][ver as usize])
}
fn data_codewords(ver: i32, ecc: i32) -> i32 {
    raw_modules(ver) / 8 - ecc_per_block(ecc, ver) * num_blocks(ecc, ver)
}

fn align_pos(ver: i32) -> Vec<i32> {
    if ver == 1 {
        return Vec::new();
    }
    let na = ver / 7 + 2;
    let size = ver * 4 + 17;
    let step = if ver == 32 { 26 } else { (ver * 4 + na * 2 + 1) / (na * 2 - 2) * 2 };
    let mut pos = vec![0; na as usize];
    pos[0] = 6;
    let mut p = size - 7;
    for i in 0..na - 1 {
        pos[(na - 1 - i) as usize] = p;
        p -= step;
    }
    pos
}

fn cc_bits(mode: i32, ver: i32) -> i32 {
    let g = if ver <= 9 { 0 } else if ver <= 26 { 1 } else { 2 };
    const T: [[i32; 3]; 3] = [[10, 12, 14], [9, 11, 13], [8, 16, 16]];
    T[if mode == 1 { 0 } else if mode == 2 { 1 } else { 2 }][g]
}

// ================= GF(256) and Reed-Solomon =================

struct Gf {
    exp: [u8; 512],
    log: [u8; 256],
}
fn gf() -> &'static Gf {
    static GF: OnceLock<Gf> = OnceLock::new();
    GF.get_or_init(|| {
        let mut g = Gf { exp: [0; 512], log: [0; 256] };
        let mut x: i32 = 1;
        for i in 0..255 {
            g.exp[i] = x as u8;
            g.log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= 0x11d;
            }
        }
        for i in 255..512 {
            g.exp[i] = g.exp[i - 255];
        }
        g
    })
}
fn gmul(a: u8, b: u8) -> u8 {
    if a != 0 && b != 0 {
        let g = gf();
        g.exp[usize::from(g.log[a as usize]) + usize::from(g.log[b as usize])]
    } else {
        0
    }
}
fn gdiv(a: u8, b: u8) -> u8 {
    if a != 0 {
        let g = gf();
        g.exp[(usize::from(g.log[a as usize]) + 255 - usize::from(g.log[b as usize])) % 255]
    } else {
        0
    }
}

/// The generator of degree d: (x - a^0)(x - a^1)…; coefficients without the leading one.
fn rs_gen(d: usize) -> Vec<u8> {
    let mut g = vec![0u8; d];
    g[d - 1] = 1;
    let mut root: u8 = 1;
    for _ in 0..d {
        for j in 0..d {
            g[j] = gmul(g[j], root);
            if j + 1 < d {
                g[j] ^= g[j + 1];
            }
        }
        root = gmul(root, 2);
    }
    g
}
fn rs_rem(data: &[u8], g: &[u8]) -> Vec<u8> {
    let d = g.len();
    let mut out = vec![0u8; d];
    for &byte in data {
        let f = byte ^ out[0];
        out.copy_within(1.., 0);
        out[d - 1] = 0;
        for j in 0..d {
            out[j] ^= gmul(g[j], f);
        }
    }
    out
}

/// Corrects block `c` (n bytes with `ne` correction bytes). True = ok (or no errors).
fn rs_correct(c: &mut [u8], ne: usize) -> bool {
    let n = c.len();
    let g = gf();
    let mut s = [0u8; 64];
    let mut any = 0u8;
    for j in 0..ne {
        let a = g.exp[j];
        let mut v = 0u8;
        for &k in c.iter() {
            v = gmul(v, a) ^ k;
        }
        s[j] = v;
        any |= v;
    }
    if any == 0 {
        return true;
    }
    // Berlekamp-Massey
    let (mut l, mut b) = ([0u8; 64], [0u8; 64]);
    l[0] = 1;
    b[0] = 1;
    let (mut ld, mut m) = (0usize, 1usize);
    let mut bb: u8 = 1;
    for r in 0..ne {
        let mut d = s[r];
        for i in 1..=ld {
            d ^= gmul(l[i], s[r - i]);
        }
        if d == 0 {
            m += 1;
            continue;
        }
        let t = l;
        let coef = gdiv(d, bb);
        for i in 0..64usize.saturating_sub(m) {
            l[i + m] ^= gmul(coef, b[i]);
        }
        if 2 * ld <= r {
            ld = r + 1 - ld;
            b = t;
            bb = d;
            m = 1;
        } else {
            m += 1;
        }
    }
    if ld > ne / 2 {
        return false;
    }
    // Omega = S * Lambda mod x^ne
    let mut o = [0u8; 64];
    for i in 0..ne {
        let mut v = 0u8;
        for j in 0..=i.min(ld) {
            v ^= gmul(l[j], s[i - j]);
        }
        o[i] = v;
    }
    // Chien: positions p = 0..n-1 (power), array index k = n-1-p
    let mut found = 0;
    for p in 0..n {
        let xinv = g.exp[(255 - p) % 255];
        let (mut v, mut xp) = (0u8, 1u8);
        for i in 0..=ld {
            v ^= gmul(l[i], xp);
            xp = gmul(xp, xinv);
        }
        if v != 0 {
            continue;
        }
        // Forney (first root 0): e = X * Omega(X^-1) / Lambda'(X^-1)
        let (mut om, mut dl) = (0u8, 0u8);
        xp = 1;
        for &oi in o.iter().take(ne) {
            om ^= gmul(oi, xp);
            xp = gmul(xp, xinv);
        }
        xp = 1;
        let mut i = 1;
        while i <= ld {
            dl ^= gmul(l[i], xp);
            xp = gmul(xp, gmul(xinv, xinv));
            i += 2;
        }
        if dl == 0 {
            return false;
        }
        let e = gmul(g.exp[p % 255], gdiv(om, dl));
        c[n - 1 - p] ^= e;
        found += 1;
    }
    if found != ld {
        return false;
    }
    for j in 0..ne {
        let a = g.exp[j];
        let mut v = 0u8;
        for &k in c.iter() {
            v = gmul(v, a) ^ k;
        }
        if v != 0 {
            return false;
        }
    }
    true
}

// ================= building the matrix =================

struct Grid {
    size: i32,
    m: Vec<u8>,
    fnc: Vec<u8>,
}
impl Grid {
    fn at(&self, x: i32, y: i32) -> u8 {
        self.m[(y * self.size + x) as usize]
    }
    fn is_fn(&self, x: i32, y: i32) -> bool {
        self.fnc[(y * self.size + x) as usize] != 0
    }
    fn setf(&mut self, x: i32, y: i32, dark: bool) {
        let i = (y * self.size + x) as usize;
        self.m[i] = u8::from(dark);
        self.fnc[i] = 1;
    }
}

fn draw_finder(g: &mut Grid, cx: i32, cy: i32) {
    for dy in -4..=4i32 {
        for dx in -4..=4i32 {
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x >= g.size || y >= g.size {
                continue;
            }
            let d = dx.abs().max(dy.abs());
            g.setf(x, y, d != 2 && d != 4);
        }
    }
}
fn draw_align(g: &mut Grid, cx: i32, cy: i32) {
    for dy in -2..=2i32 {
        for dx in -2..=2i32 {
            let d = dx.abs().max(dy.abs());
            g.setf(cx + dx, cy + dy, d != 1);
        }
    }
}

fn format_bits(ecc: i32, mask: i32) -> i32 {
    let data = FORMAT_ECC_BITS[ecc as usize] << 3 | mask;
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    (data << 10 | rem) ^ 0x5412
}
fn version_bits(ver: i32) -> i32 {
    let mut rem = ver;
    for _ in 0..12 {
        rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
    }
    ver << 12 | rem
}

fn draw_format(g: &mut Grid, bits: i32) {
    let s = g.size;
    for i in 0..=5 {
        g.setf(8, i, bits >> i & 1 != 0);
    }
    g.setf(8, 7, bits >> 6 & 1 != 0);
    g.setf(8, 8, bits >> 7 & 1 != 0);
    g.setf(7, 8, bits >> 8 & 1 != 0);
    for i in 9..15 {
        g.setf(14 - i, 8, bits >> i & 1 != 0);
    }
    for i in 0..8 {
        g.setf(s - 1 - i, 8, bits >> i & 1 != 0);
    }
    for i in 8..15 {
        g.setf(8, s - 15 + i, bits >> i & 1 != 0);
    }
    g.setf(8, s - 8, true);
}

fn draw_functions(g: &mut Grid, ver: i32) {
    let s = g.size;
    for i in 0..s {
        g.setf(6, i, i % 2 == 0);
        g.setf(i, 6, i % 2 == 0);
    }
    draw_finder(g, 3, 3);
    draw_finder(g, s - 4, 3);
    draw_finder(g, 3, s - 4);
    let pos = align_pos(ver);
    let na = pos.len();
    for i in 0..na {
        for j in 0..na {
            if (i == 0 && j == 0) || (i == 0 && j == na - 1) || (i == na - 1 && j == 0) {
                continue;
            }
            draw_align(g, pos[i], pos[j]);
        }
    }
    draw_format(g, 0); // reserve; written later
    if ver >= 7 {
        let bits = version_bits(ver);
        for i in 0..18 {
            let bit = bits >> i & 1 != 0;
            let (a, b) = (s - 11 + i % 3, i / 3);
            g.setf(a, b, bit);
            g.setf(b, a, bit);
        }
    }
}

fn mask_bit(mask: i32, x: i32, y: i32) -> bool {
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3) % 2 == 0,
        _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
    }
}
fn apply_mask(g: &mut Grid, mask: i32) {
    for y in 0..g.size {
        for x in 0..g.size {
            if !g.is_fn(x, y) && mask_bit(mask, x, y) {
                g.m[(y * g.size + x) as usize] ^= 1;
            }
        }
    }
}

/// The data modules in the zigzag order of the standard.
fn zigzag(g: &Grid) -> Vec<(i32, i32)> {
    let s = g.size;
    let mut out = Vec::new();
    let mut right = s - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for v in 0..s {
            for j in 0..2 {
                let x = right - j;
                let up = ((right + 1) & 2) == 0;
                let y = if up { s - 1 - v } else { v };
                if !g.is_fn(x, y) {
                    out.push((x, y));
                }
            }
        }
        right -= 2;
    }
    out
}

// Penalty from the standard (N1=3, N2=3, N3=40, N4=10).
fn run_finder_count(h: &[i32; 7]) -> i64 {
    let n = h[1];
    let core = n > 0 && h[2] == n && h[3] == n * 3 && h[4] == n && h[5] == n;
    i64::from(core && h[0] >= n * 4 && h[6] >= n) + i64::from(core && h[6] >= n * 4 && h[0] >= n)
}
fn run_push(h: &mut [i32; 7], mut len: i32, s: i32) {
    if h[0] == 0 {
        len += s; // blank area on the left
    }
    h.copy_within(0..6, 1);
    h[0] = len;
}
fn penalty(g: &Grid) -> i64 {
    let s = g.size;
    let mut r: i64 = 0;
    for pass in 0..2 {
        for a in 0..s {
            let (mut color, mut run, mut h) = (0u8, 0i32, [0i32; 7]);
            for b in 0..s {
                let c = if pass == 1 { g.at(a, b) } else { g.at(b, a) };
                if c == color {
                    run += 1;
                    if run == 5 {
                        r += 3;
                    } else if run > 5 {
                        r += 1;
                    }
                } else {
                    run_push(&mut h, run, s);
                    if color == 0 {
                        r += run_finder_count(&h) * 40;
                    }
                    color = c;
                    run = 1;
                }
            }
            // end of the line
            if color != 0 {
                run_push(&mut h, run, s);
                run = 0;
            }
            run += s;
            run_push(&mut h, run, s);
            r += run_finder_count(&h) * 40;
        }
    }
    for y in 0..s - 1 {
        for x in 0..s - 1 {
            let c = g.at(x, y);
            if c == g.at(x + 1, y) && c == g.at(x, y + 1) && c == g.at(x + 1, y + 1) {
                r += 3;
            }
        }
    }
    let dark: i64 = g.m.iter().map(|b| i64::from(*b)).sum();
    let total = i64::from(s) * i64::from(s);
    let k = ((dark * 20 - total * 10).abs() + total - 1) / total - 1;
    r + k * 10
}

/// Data bits: mode + length + content.
struct Bits {
    b: Vec<u8>,
    n: usize,
    cap: usize,
}
impl Bits {
    fn put(&mut self, v: u32, nb: i32) {
        for i in (0..nb).rev() {
            if self.n < self.cap * 8 && (v >> i) & 1 != 0 {
                self.b[self.n >> 3] |= 0x80 >> (self.n & 7);
            }
            self.n += 1;
        }
    }
}

fn pick_mode(d: &[u8]) -> i32 {
    let num = d.iter().all(|c| c.is_ascii_digit());
    let al = d.iter().all(|c| *c != 0 && ALNUM.contains(c));
    if num {
        1
    } else if al {
        2
    } else {
        4
    }
}
fn seg_bits(mode: i32, len: usize, ver: i32) -> i64 {
    let mut b = 4 + i64::from(cc_bits(mode, ver));
    if mode == 1 {
        b += (len / 3) as i64 * 10 + if len % 3 == 2 { 7 } else if len % 3 == 1 { 4 } else { 0 };
    } else if mode == 2 {
        b += (len / 2) as i64 * 11 + (len % 2) as i64 * 6;
    } else {
        b += len as i64 * 8;
    }
    if len >= 1usize << cc_bits(mode, ver) {
        return 1 << 30;
    }
    b
}

/// The modules, the side, the version and the mask used.
struct Encoded {
    modules: Vec<u8>,
    size: i32,
    ver: i32,
    mask: i32,
}

fn encode(data: &[u8], ecc: i32, mask: i32, minver: i32) -> Option<Encoded> {
    let mode = pick_mode(data);
    let mut ver = if minver < 1 { 1 } else { minver };
    while ver <= 40 && seg_bits(mode, data.len(), ver) > i64::from(data_codewords(ver, ecc)) * 8 {
        ver += 1;
    }
    if ver > 40 {
        return None;
    }
    let ndc = data_codewords(ver, ecc) as usize;
    let cap = ndc * 8;
    let mut bb = Bits { b: vec![0; ndc], n: 0, cap: ndc };
    bb.put(mode as u32, 4);
    bb.put(data.len() as u32, cc_bits(mode, ver));
    if mode == 1 {
        let mut i = 0;
        while i < data.len() {
            let k = (data.len() - i).min(3);
            let mut v = 0u32;
            for j in 0..k {
                v = v * 10 + u32::from(data[i + j] - b'0');
            }
            bb.put(v, if k == 3 { 10 } else if k == 2 { 7 } else { 4 });
            i += k;
        }
    } else if mode == 2 {
        let index = |c: u8| ALNUM.iter().position(|a| *a == c).unwrap_or(0) as u32;
        let mut i = 0;
        while i < data.len() {
            let a = index(data[i]);
            if i + 1 < data.len() {
                bb.put(a * 45 + index(data[i + 1]), 11);
            } else {
                bb.put(a, 6);
            }
            i += 2;
        }
    } else {
        for &c in data {
            bb.put(u32::from(c), 8);
        }
    }
    let term = (cap - bb.n.min(cap)).min(4) as i32;
    bb.put(0, term);
    bb.put(0, ((8 - bb.n % 8) % 8) as i32);
    let mut pad = 0xecu32;
    while bb.n < cap {
        bb.put(pad, 8);
        pad ^= 0xec ^ 0x11;
    }

    // blocks and correction, interleaved
    let nb = num_blocks(ecc, ver) as usize;
    let ne = ecc_per_block(ecc, ver) as usize;
    let raw = (raw_modules(ver) / 8) as usize;
    let (nshort, shortlen) = (nb - raw % nb, raw / nb);
    let gen = rs_gen(ne);
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(nb);
    let mut k = 0;
    for i in 0..nb {
        let dl = shortlen - ne + usize::from(i >= nshort);
        let mut block = bb.b[k..k + dl].to_vec();
        block.extend(rs_rem(&bb.b[k..k + dl], &gen));
        blocks.push(block);
        k += dl;
    }
    let blen: Vec<usize> = blocks.iter().map(|b| b.len() - ne).collect();
    let mut all = Vec::with_capacity(raw);
    for i in 0..shortlen + 1 - ne {
        for (b, block) in blocks.iter().enumerate() {
            if i < blen[b] {
                all.push(block[i]);
            }
        }
    }
    for i in 0..ne {
        for (b, block) in blocks.iter().enumerate() {
            all.push(block[blen[b] + i]);
        }
    }

    let s = ver * 4 + 17;
    let mut g = Grid { size: s, m: vec![0; (s * s) as usize], fnc: vec![0; (s * s) as usize] };
    draw_functions(&mut g, ver);
    let nbits = all.len() * 8;
    for (i, (x, y)) in zigzag(&g).into_iter().enumerate() {
        if i < nbits {
            g.m[(y * s + x) as usize] = all[i >> 3] >> (7 - (i & 7)) & 1;
        }
    }
    let mut best = mask;
    if best < 0 {
        let mut bp: i64 = -1;
        for k in 0..8 {
            apply_mask(&mut g, k);
            draw_format(&mut g, format_bits(ecc, k));
            let p = penalty(&g);
            if bp < 0 || p < bp {
                bp = p;
                best = k;
            }
            apply_mask(&mut g, k);
        }
    }
    apply_mask(&mut g, best);
    draw_format(&mut g, format_bits(ecc, best));
    Some(Encoded { modules: g.m, size: s, ver, mask: best })
}

// ================= decoding: from the grid to the bytes =================

/// The format (ecc, mask) with the smallest Hamming distance (at most 3).
fn decode_format(bits: i32) -> Option<(i32, i32, i32)> {
    let (mut best, mut ecc, mut mask) = (99, 0, 0);
    for e in 0..4 {
        for m in 0..8 {
            let d = ((bits ^ format_bits(e, m)) as u32).count_ones() as i32;
            if d < best {
                best = d;
                ecc = e;
                mask = m;
            }
        }
    }
    (best <= 3).then_some((best, ecc, mask))
}
fn decode_version(bits: i32) -> i32 {
    let (mut best, mut v) = (99, -1);
    for k in 7..=40 {
        let d = ((bits ^ version_bits(k)) as u32).count_ones() as i32;
        if d < best {
            best = d;
            v = k;
        }
    }
    if best <= 3 {
        v
    } else {
        -1
    }
}

/// Reads the format information from the two copies in grid `g` (already sampled).
fn read_format(g: &Grid) -> Option<(i32, i32)> {
    let s = g.size;
    let (mut a, mut b) = (0i32, 0i32);
    for i in 0..=5 {
        a |= i32::from(g.at(8, i)) << i;
    }
    a |= i32::from(g.at(8, 7)) << 6 | i32::from(g.at(8, 8)) << 7 | i32::from(g.at(7, 8)) << 8;
    for i in 9..15 {
        a |= i32::from(g.at(14 - i, 8)) << i;
    }
    for i in 0..8 {
        b |= i32::from(g.at(s - 1 - i, 8)) << i;
    }
    for i in 8..15 {
        b |= i32::from(g.at(8, s - 15 + i)) << i;
    }
    let (d1, d2) = (decode_format(a), decode_format(b));
    match (d1, d2) {
        (None, None) => None,
        (Some((_, e, m)), None) => Some((e, m)),
        (None, Some((_, e, m))) => Some((e, m)),
        (Some((x1, e1, m1)), Some((x2, e2, m2))) => Some(if x1 <= x2 { (e1, m1) } else { (e2, m2) }),
    }
}

fn read_version_bits(g: &Grid) -> i32 {
    let s = g.size;
    let (mut a, mut b) = (0i32, 0i32);
    for i in 0..18 {
        let (x, y) = (s - 11 + i % 3, i / 3);
        a |= i32::from(g.at(x, y)) << i;
        b |= i32::from(g.at(y, x)) << i;
    }
    let (va, vb) = (decode_version(a), decode_version(b));
    if va > 0 {
        va
    } else {
        vb
    }
}

struct BitReader<'a> {
    b: &'a [u8],
    n: i32,
    p: i32,
}
impl BitReader<'_> {
    fn read(&mut self, k: i32) -> i32 {
        if self.p + k > self.n {
            self.p = self.n + 1;
            return -1;
        }
        let mut v = 0;
        for _ in 0..k {
            v = v << 1 | i32::from(self.b[(self.p >> 3) as usize] >> (7 - (self.p & 7)) & 1);
            self.p += 1;
        }
        v
    }
}

/// Segments → bytes (with the trailing 0; the length, or None when it is not readable).
fn parse_segments(d: &[u8], ver: i32, cap: usize) -> Option<(Vec<u8>, usize)> {
    let mut r = BitReader { b: d, n: d.len() as i32 * 8, p: 0 };
    let mut out: Vec<u8> = Vec::new();
    let mut o = 0usize;
    let mut put = |out: &mut Vec<u8>, c: u8| {
        if o < cap {
            out.push(c);
        }
        o += 1;
    };
    loop {
        if r.n - r.p < 4 {
            break;
        }
        let mode = r.read(4);
        if mode == 0 {
            break;
        }
        if mode == 7 {
            // ECI: skipped
            let f = r.read(8);
            if f < 0 {
                return None;
            }
            if f & 0xc0 == 0x80 {
                r.read(8);
            } else if f & 0xe0 == 0xc0 {
                r.read(16);
            } else if f & 0x80 != 0 {
                return None;
            }
            continue;
        }
        if mode == 3 {
            r.read(16); // structured append
            continue;
        }
        if mode == 5 {
            continue; // FNC1 first position
        }
        if mode == 9 {
            r.read(8);
            continue;
        }
        let g = if ver <= 9 { 0 } else if ver <= 26 { 1 } else { 2 };
        if mode == 1 {
            let mut n = r.read([10, 12, 14][g]);
            if n < 0 {
                return None;
            }
            while n >= 3 {
                let v = r.read(10);
                if !(0..=999).contains(&v) {
                    return None;
                }
                put(&mut out, b'0' + (v / 100) as u8);
                put(&mut out, b'0' + (v / 10 % 10) as u8);
                put(&mut out, b'0' + (v % 10) as u8);
                n -= 3;
            }
            if n == 2 {
                let v = r.read(7);
                if !(0..=99).contains(&v) {
                    return None;
                }
                put(&mut out, b'0' + (v / 10) as u8);
                put(&mut out, b'0' + (v % 10) as u8);
            } else if n == 1 {
                let v = r.read(4);
                if !(0..=9).contains(&v) {
                    return None;
                }
                put(&mut out, b'0' + v as u8);
            }
        } else if mode == 2 {
            let mut n = r.read([9, 11, 13][g]);
            if n < 0 {
                return None;
            }
            while n >= 2 {
                let v = r.read(11);
                if v < 0 || v >= 45 * 45 {
                    return None;
                }
                put(&mut out, ALNUM[(v / 45) as usize]);
                put(&mut out, ALNUM[(v % 45) as usize]);
                n -= 2;
            }
            if n == 1 {
                let v = r.read(6);
                if v < 0 || v >= 45 {
                    return None;
                }
                put(&mut out, ALNUM[v as usize]);
            }
        } else if mode == 4 {
            let n = r.read([8, 16, 16][g]);
            if n < 0 {
                return None;
            }
            for _ in 0..n {
                let v = r.read(8);
                if v < 0 {
                    return None;
                }
                put(&mut out, v as u8);
            }
        } else if mode == 8 {
            // kanji → Shift JIS
            let n = r.read([8, 10, 12][g]);
            if n < 0 {
                return None;
            }
            for _ in 0..n {
                let v = r.read(13);
                if v < 0 {
                    return None;
                }
                let mut c = (v / 0xc0) << 8 | (v % 0xc0);
                c += if c < 0x1f00 { 0x8140 } else { 0xc140 };
                put(&mut out, (c >> 8) as u8);
                put(&mut out, (c & 255) as u8);
            }
        } else {
            return None;
        }
        if r.p > r.n {
            return None;
        }
    }
    if o >= cap {
        return None;
    }
    out.push(0);
    Some((out, o))
}

/// A sampled grid (the modules of version `ver`) → bytes.
fn decode_grid(mods: Vec<u8>, ver: i32, cap: usize) -> Option<(Vec<u8>, usize)> {
    let s = ver * 4 + 17;
    let mut g = Grid { size: s, m: mods, fnc: vec![0; (s * s) as usize] };
    let (ecc, mask) = read_format(&g)?;
    let save = g.m.clone();
    draw_functions(&mut g, ver); // only to mark fn; the modules are restored
    g.m = save;
    apply_mask(&mut g, mask);
    let raw = (raw_modules(ver) / 8) as usize;
    let mut all = vec![0u8; raw + 8];
    for (i, (x, y)) in zigzag(&g).into_iter().enumerate() {
        if i < raw * 8 && g.at(x, y) != 0 {
            all[i >> 3] |= 0x80 >> (i & 7);
        }
    }
    let nb = num_blocks(ecc, ver) as usize;
    let ne = ecc_per_block(ecc, ver) as usize;
    let (nshort, shortlen) = (nb - raw % nb, raw / nb);
    let blen: Vec<usize> = (0..nb).map(|b| shortlen - ne + usize::from(b >= nshort)).collect();
    let mut blocks: Vec<Vec<u8>> = blen.iter().map(|l| vec![0u8; l + ne]).collect();
    let mut k = 0;
    for i in 0..shortlen + 1 - ne {
        for b in 0..nb {
            if i < blen[b] {
                blocks[b][i] = all[k];
                k += 1;
            }
        }
    }
    for i in 0..ne {
        for b in 0..nb {
            blocks[b][blen[b] + i] = all[k];
            k += 1;
        }
    }
    let mut data = Vec::new();
    for b in 0..nb {
        if !rs_correct(&mut blocks[b], ne) {
            return None;
        }
        data.extend_from_slice(&blocks[b][..blen[b]]);
    }
    parse_segments(&data, ver, cap)
}

// ================= decoding: from the image to the grid =================

#[derive(Clone, Copy, Default)]
struct Cand {
    x: f64,
    y: f64,
    ms: f64,
    count: i32,
}
struct Img<'a> {
    bin: &'a [u8],
    w: i32,
    h: i32,
    c: Vec<Cand>,
}
impl Img<'_> {
    fn px(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.bin[(y * self.w + x) as usize] != 0
    }
    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }
}

fn ratio_ok(st: &[i32; 5]) -> bool {
    let mut total = 0;
    for &v in st {
        if v == 0 {
            return false;
        }
        total += v;
    }
    if total < 7 {
        return false;
    }
    let (m, v) = (f64::from(total) / 7.0, f64::from(total) / 7.0 * 0.5);
    let f = |x: i32| f64::from(x);
    (m - f(st[0])).abs() < v && (m - f(st[1])).abs() < v && (3.0 * m - f(st[2])).abs() < 3.0 * v && (m - f(st[3])).abs() < v && (m - f(st[4])).abs() < v
}

/// Counts the runs in one direction from (fx, fy), which must be dark: the center and the module size.
fn cross(im: &Img, fx: f64, fy: f64, dx: i32, dy: i32, maxc: i32) -> Option<(f64, f64)> {
    let (x0, y0) = (fx as i32, fy as i32);
    if x0 < 0 || y0 < 0 || x0 >= im.w || y0 >= im.h || !im.px(x0, y0) {
        return None;
    }
    let mut st = [0i32; 5];
    let (mut x, mut y) = (x0, y0);
    // backwards: center (dark), white, dark
    while im.inside(x, y) && im.px(x, y) {
        st[2] += 1;
        x -= dx;
        y -= dy;
    }
    while im.inside(x, y) && !im.px(x, y) && st[1] <= maxc {
        st[1] += 1;
        x -= dx;
        y -= dy;
    }
    while im.inside(x, y) && im.px(x, y) && st[0] <= maxc {
        st[0] += 1;
        x -= dx;
        y -= dy;
    }
    x = x0 + dx;
    y = y0 + dy;
    while im.inside(x, y) && im.px(x, y) {
        st[2] += 1;
        x += dx;
        y += dy;
    }
    while im.inside(x, y) && !im.px(x, y) && st[3] <= maxc {
        st[3] += 1;
        x += dx;
        y += dy;
    }
    while im.inside(x, y) && im.px(x, y) && st[4] <= maxc {
        st[4] += 1;
        x += dx;
        y += dy;
    }
    let ex = if dx != 0 { x } else { y }; // end (exclusive) on the coordinate that moves
    if !ratio_ok(&st) {
        return None;
    }
    let center = f64::from(ex - st[4] - st[3]) - f64::from(st[2]) / 2.0;
    let ms = f64::from(st[0] + st[1] + st[2] + st[3] + st[4]) / 7.0;
    Some((center, ms))
}

fn add_cand(im: &mut Img, x: f64, y: f64, ms: f64) {
    for c in im.c.iter_mut() {
        if (c.x - x).abs() <= c.ms * 2.0 && (c.y - y).abs() <= c.ms * 2.0 && (c.ms - ms).abs() <= c.ms * 0.5 + 1.0 {
            let n = f64::from(c.count);
            c.x = (c.x * n + x) / (n + 1.0);
            c.y = (c.y * n + y) / (n + 1.0);
            c.ms = (c.ms * n + ms) / (n + 1.0);
            c.count += 1;
            return;
        }
    }
    if im.c.len() < 64 {
        im.c.push(Cand { x, y, ms, count: 1 });
    }
}

fn found(im: &mut Img, st: &[i32; 5], y: i32, xend: i32) {
    let cx = f64::from(xend - st[4] - st[3]) - f64::from(st[2]) / 2.0;
    let total = st[0] + st[1] + st[2] + st[3] + st[4];
    let Some((cy, msv)) = cross(im, cx, f64::from(y), 0, 1, st[2]) else { return };
    let Some((cx2, msh)) = cross(im, cx, cy, 1, 0, st[2]) else { return };
    let ms = (msv + msh) / 2.0;
    if msv * 7.0 > f64::from(total) * 2.0 || msv * 7.0 * 2.0 < f64::from(total) {
        return;
    }
    // diagonal (discards stripes)
    if cross(im, cx2, cy, 1, 1, st[2] * 2).is_none() {
        return;
    }
    add_cand(im, cx2, cy, ms);
}

fn find_finders(im: &mut Img) {
    im.c.clear();
    for y in 0..im.h {
        let (mut st, mut cur) = ([0i32; 5], 0usize);
        for x in 0..im.w {
            if im.px(x, y) {
                if cur & 1 == 1 {
                    cur += 1;
                }
                st[cur] += 1;
            } else {
                if cur == 0 && st[0] == 0 {
                    continue;
                }
                if cur & 1 == 0 {
                    if cur == 4 {
                        if ratio_ok(&st) {
                            found(im, &st, y, x);
                            st = [0; 5];
                            cur = 0;
                            continue;
                        }
                        st = [st[2], st[3], st[4], 1, 0];
                        cur = 3;
                        continue;
                    }
                    cur += 1;
                    st[cur] += 1;
                } else {
                    st[cur] += 1;
                }
            }
        }
        if cur == 4 && ratio_ok(&st) {
            found(im, &st, y, im.w);
        }
    }
}

fn dist(a: &Cand, b: &Cand) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The homography from (u, v) in modules to (x, y) in the image, from 4 points (an 8x8 system).
fn hom_solve(u: &[f64; 4], v: &[f64; 4], x: &[f64; 4], y: &[f64; 4]) -> Option<[f64; 9]> {
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        m[2 * i] = [u[i], v[i], 1.0, 0.0, 0.0, 0.0, -u[i] * x[i], -v[i] * x[i], x[i]];
        m[2 * i + 1] = [0.0, 0.0, 0.0, u[i], v[i], 1.0, -u[i] * y[i], -v[i] * y[i], y[i]];
    }
    for c in 0..8 {
        let mut p = c;
        for r in c + 1..8 {
            if m[r][c].abs() > m[p][c].abs() {
                p = r;
            }
        }
        if m[p][c].abs() < 1e-12 {
            return None;
        }
        if p != c {
            m.swap(c, p);
        }
        for r in 0..8 {
            if r == c {
                continue;
            }
            let f = m[r][c] / m[c][c];
            for k in c..9 {
                m[r][k] -= f * m[c][k];
            }
        }
    }
    let mut h = [0f64; 9];
    for i in 0..8 {
        h[i] = m[i][8] / m[i][i];
    }
    h[8] = 1.0;
    Some(h)
}
fn hom_map(h: &[f64; 9], u: f64, v: f64) -> (f64, f64) {
    let w = h[6] * u + h[7] * v + h[8];
    ((h[0] * u + h[1] * v + h[2]) / w, (h[3] * u + h[4] * v + h[5]) / w)
}

fn sample(im: &Img, x: f64, y: f64) -> u8 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    if !im.inside(ix, iy) || !x.is_finite() || !y.is_finite() {
        return 0;
    }
    u8::from(im.px(ix, iy))
}

/// The alignment pattern near (ex, ey): a dark module surrounded by a light ring and a dark one.
fn find_align(im: &Img, ex: f64, ey: f64, ms: f64) -> Option<(f64, f64)> {
    let r = (ms * 4.0 + 2.0) as i32;
    let mut best = None;
    let mut bd = 1e18;
    let (ey_i, ex_i) = (ey as i32, ex as i32);
    for y in ey_i - r..=ey_i + r {
        if y < 0 || y >= im.h {
            continue;
        }
        for x in ex_i - r..=ex_i + r {
            if x < 0 || x >= im.w || !im.px(x, y) {
                continue;
            }
            // horizontal and vertical runs: dark (~1), light (~1) on each side, dark outside
            let mut ok = true;
            let mut c = [0f64; 2];
            for d in 0..2 {
                let (dx, dy) = (i32::from(d == 0), i32::from(d == 1));
                let (mut n0, mut l1, mut r1) = (0i32, 0i32, 0i32);
                let lim = (ms * 2.5) as i32 + 1;
                let (mut xx, mut yy) = (x, y);
                while xx >= 0 && yy >= 0 && im.px(xx, yy) && n0 < lim {
                    n0 += 1;
                    xx -= dx;
                    yy -= dy;
                }
                let left = if d == 1 { yy } else { xx };
                while xx >= 0 && yy >= 0 && !im.px(xx, yy) && l1 < lim {
                    l1 += 1;
                    xx -= dx;
                    yy -= dy;
                }
                if xx < 0 || yy < 0 || !im.px(xx, yy) {
                    ok = false;
                    break;
                }
                xx = x + dx;
                yy = y + dy;
                while xx < im.w && yy < im.h && im.px(xx, yy) && n0 < lim * 2 {
                    n0 += 1;
                    xx += dx;
                    yy += dy;
                }
                let right = if d == 1 { yy } else { xx };
                while xx < im.w && yy < im.h && !im.px(xx, yy) && r1 < lim {
                    r1 += 1;
                    xx += dx;
                    yy += dy;
                }
                if xx >= im.w || yy >= im.h || !im.px(xx, yy) {
                    ok = false;
                    break;
                }
                if (f64::from(n0) - ms).abs() > ms * 0.7 + 1.0 || (f64::from(l1) - ms).abs() > ms * 0.7 + 1.0 || (f64::from(r1) - ms).abs() > ms * 0.7 + 1.0 {
                    ok = false;
                }
                c[d as usize] = f64::from(left + 1 + right) / 2.0;
            }
            if !ok {
                continue;
            }
            let dd = (c[0] - ex) * (c[0] - ex) + (c[1] - ey) * (c[1] - ey);
            if dd < bd {
                bd = dd;
                best = Some((c[0], c[1]));
            }
        }
    }
    best
}

/// Counts the runs of the timing pattern between two points (in pixels).
fn timing_runs(im: &Img, x0: f64, y0: f64, x1: f64, y1: f64, ms: f64) -> i32 {
    let len = (x1 - x0).hypot(y1 - y0);
    let steps = (len / (ms / 4.0)) as i32 + 1;
    let (mut runs, mut prev) = (0, -1i32);
    for i in 0..=steps {
        let t = f64::from(i) / f64::from(steps);
        let v = i32::from(sample(im, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
        if v != prev {
            runs += 1;
            prev = v;
        }
    }
    runs
}

/// What a grid attempt gave: the bytes, a version to try instead, or nothing.
enum Grid2 {
    Done(Vec<u8>, usize),
    Other(i32),
    No,
}

fn try_grid(im: &Img, b: &Cand, a: &Cand, c: &Cand, ver: i32, use_align: bool, cap: usize) -> Grid2 {
    let s = ver * 4 + 17;
    let ms = (a.ms + b.ms + c.ms) / 3.0;
    let sf = f64::from(s);
    let mut u = [3.5, sf - 3.5, 3.5, 0.0];
    let mut v = [3.5, 3.5, sf - 3.5, 0.0];
    let mut x = [b.x, a.x, c.x, 0.0];
    let mut y = [b.y, a.y, c.y, 0.0];
    // the fourth point: the affine corner (or the alignment pattern)
    let k = (sf - 6.5 - 3.5) / (sf - 7.0);
    u[3] = sf - 6.5;
    v[3] = sf - 6.5;
    x[3] = b.x + (a.x - b.x) * k + (c.x - b.x) * k;
    y[3] = b.y + (a.y - b.y) * k + (c.y - b.y) * k;
    if ver >= 2 && use_align {
        let Some((ax, ay)) = find_align(im, x[3], y[3], ms) else { return Grid2::No };
        x[3] = ax;
        y[3] = ay;
    }
    let Some(h) = hom_solve(&u, &v, &x, &y) else { return Grid2::No };
    let mut mods = vec![0u8; (s * s) as usize];
    for r in 0..s {
        for col in 0..s {
            let (px, py) = hom_map(&h, f64::from(col) + 0.5, f64::from(r) + 0.5);
            mods[(r * s + col) as usize] = sample(im, px, py);
        }
    }
    if ver >= 7 {
        let g = Grid { size: s, m: mods.clone(), fnc: Vec::new() };
        let vv = read_version_bits(&g);
        if vv > 0 && vv != ver {
            return Grid2::Other(vv); // a different version: try that one
        }
    }
    match decode_grid(mods, ver, cap) {
        Some((bytes, n)) => Grid2::Done(bytes, n),
        None => Grid2::No,
    }
}

fn try_triplet(im: &Img, b: Cand, a: Cand, c: Cand, cap: usize) -> Option<(Vec<u8>, usize)> {
    let ms = (a.ms + b.ms + c.ms) / 3.0;
    let (dab, dbc) = (dist(&b, &a), dist(&b, &c));
    let est = ((dab + dbc) / 2.0 / ms).round() as i32 + 7;
    // timing pattern: row 6 (from B to A) and column 6 (from B to C)
    let (ux, uy, vx, vy) = ((c.x - b.x) / dbc, (c.y - b.y) / dbc, (a.x - b.x) / dab, (a.y - b.y) / dab);
    let t1 = timing_runs(im, b.x + ux * 3.0 * ms, b.y + uy * 3.0 * ms, a.x + ux * 3.0 * ms, a.y + uy * 3.0 * ms, ms) + 12;
    let t2 = timing_runs(im, b.x + vx * 3.0 * ms, b.y + vy * 3.0 * ms, c.x + vx * 3.0 * ms, c.y + vy * 3.0 * ms, ms) + 12;
    let mut tried: Vec<i32> = Vec::new();
    for d in [t1, t2, est, est + 1, est - 1, est + 2, est - 2, est + 4] {
        if d % 4 != 1 {
            continue;
        }
        let mut ver = (d - 17) / 4;
        if !(1..=40).contains(&ver) {
            continue;
        }
        for _pass in 0..3 {
            if tried.contains(&ver) || tried.len() >= 16 {
                break;
            }
            tried.push(ver);
            let mut r = try_grid(im, &b, &a, &c, ver, true, cap);
            if matches!(r, Grid2::No) && ver >= 2 {
                r = try_grid(im, &b, &a, &c, ver, false, cap);
            }
            match r {
                Grid2::Done(bytes, n) => return Some((bytes, n)),
                Grid2::Other(v) => {
                    ver = v; // the version information said otherwise
                    continue;
                }
                Grid2::No => break,
            }
        }
    }
    None
}

fn decode_bin(im: &mut Img, cap: usize) -> Option<(Vec<u8>, usize)> {
    find_finders(im);
    if im.c.len() < 3 {
        return None;
    }
    // the most confirmed first
    let nc = im.c.len();
    for i in 0..nc {
        for j in i + 1..nc {
            if im.c[j].count > im.c[i].count {
                im.c.swap(i, j);
            }
        }
    }
    let n = nc.min(12);
    struct Tri {
        score: f64,
        a: usize,
        b: usize,
        c: usize,
    }
    let mut tri: Vec<Tri> = Vec::new();
    'outer: for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let p = [im.c[i], im.c[j], im.c[k]];
                let mn = p[0].ms.min(p[1].ms.min(p[2].ms));
                let mx = p[0].ms.max(p[1].ms.max(p[2].ms));
                if mx > mn * 1.5 {
                    continue;
                }
                let (d01, d02, d12) = (dist(&p[0], &p[1]), dist(&p[0], &p[2]), dist(&p[1], &p[2]));
                let (b, hyp, l1, l2);
                if d12 >= d01 && d12 >= d02 {
                    (b, hyp, l1, l2) = (0, d12, d01, d02);
                } else if d02 >= d01 {
                    (b, hyp, l1, l2) = (1, d02, d01, d12);
                } else {
                    (b, hyp, l1, l2) = (2, d01, d02, d12);
                }
                let ms = (mn + mx) / 2.0;
                if l1 < ms * 12.0 || l2 < ms * 12.0 {
                    continue;
                }
                let legs = (l1 - l2).abs() / l1.max(l2);
                let right = (hyp * hyp - l1 * l1 - l2 * l2).abs() / (hyp * hyp);
                if legs > 0.25 || right > 0.25 {
                    continue;
                }
                let idx = [i, j, k];
                let (a, c) = (idx[(b + 1) % 3], idx[(b + 2) % 3]);
                tri.push(Tri { score: legs + right + (mx - mn) / mx, a: idx[b], b: a, c });
                if tri.len() == 220 {
                    break 'outer;
                }
            }
        }
    }
    let nt = tri.len();
    for i in 0..nt {
        for j in i + 1..nt {
            if tri[j].score < tri[i].score {
                tri.swap(i, j);
            }
        }
    }
    for t in tri.iter().take(6) {
        let (b, mut a, mut c) = (im.c[t.a], im.c[t.b], im.c[t.c]);
        // orientation: with y pointing down, (A-B) x (C-B) > 0
        let cr = (a.x - b.x) * (c.y - b.y) - (a.y - b.y) * (c.x - b.x);
        if cr < 0.0 {
            std::mem::swap(&mut a, &mut c);
        }
        if let Some(r) = try_triplet(im, b, a, c, cap) {
            return Some(r);
        }
    }
    None
}

fn reserve<T: Clone>(n: usize, fill: T) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).ok()?;
    v.resize(n, fill);
    Some(v)
}

fn decode(argb: &[u32], w: i32, h: i32, cap: usize) -> Option<(Vec<u8>, usize)> {
    let n = w as usize * h as usize;
    let mut lum = reserve(n, 0u8)?;
    let mut bin = reserve(n, 0u8)?;
    let mut integ = reserve((w as usize + 1) * (h as usize + 1), 0u32)?;
    let mut hist = [0i64; 256];
    for i in 0..n {
        let p = argb[i];
        let (a, r, g, b) = (p >> 24, p >> 16 & 255, p >> 8 & 255, p & 255);
        let mut l = (r * 77 + g * 150 + b * 29) >> 8;
        l = (l * a + 255 * (255 - a)) / 255; // over a white background
        lum[i] = l as u8;
        hist[l as usize] += 1;
    }
    // Otsu
    let (mut sum, mut sumb, mut wb, mut best) = (0f64, 0f64, 0f64, -1f64);
    let mut th = 128;
    for (i, c) in hist.iter().enumerate() {
        sum += i as f64 * *c as f64;
    }
    for (i, c) in hist.iter().enumerate() {
        wb += *c as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = n as f64 - wb;
        if wf <= 0.0 {
            break;
        }
        sumb += i as f64 * *c as f64;
        let (mb, mf) = (sumb / wb, (sum - sumb) / wf);
        let v = wb * wf * (mb - mf) * (mb - mf);
        if v > best {
            best = v;
            th = i as i32;
        }
    }
    // the integral image for the local threshold
    let stride = w as usize + 1;
    for y in 0..h as usize {
        let mut row: u32 = 0;
        for x in 0..w as usize {
            row = row.wrapping_add(u32::from(lum[y * w as usize + x]));
            integ[(y + 1) * stride + x + 1] = integ[y * stride + x + 1].wrapping_add(row);
        }
    }
    let win = ((w.max(h)) / 8).max(16);
    for attempt in 0..4 {
        let invert = attempt & 1 == 1;
        let local = attempt >= 2;
        for y in 0..h {
            let y0 = (y - win / 2).max(0);
            let y1 = (y + win / 2).min(h - 1);
            for x in 0..w {
                let i = (y * w + x) as usize;
                let l = i64::from(lum[i]);
                let mut dark;
                if !local {
                    dark = i32::from(lum[i]) <= th;
                } else {
                    let x0 = (x - win / 2).max(0);
                    let x1 = (x + win / 2).min(w - 1);
                    let at = |yy: i32, xx: i32| integ[yy as usize * stride + xx as usize];
                    let s = at(y1 + 1, x1 + 1).wrapping_sub(at(y0, x1 + 1)).wrapping_sub(at(y1 + 1, x0)).wrapping_add(at(y0, x0));
                    let cnt = i64::from(x1 - x0 + 1) * i64::from(y1 - y0 + 1);
                    dark = if invert { l * cnt * 100 > i64::from(s) * 115 } else { l * cnt * 100 < i64::from(s) * 85 };
                }
                if !local && invert {
                    dark = !dark;
                }
                bin[i] = u8::from(dark);
            }
        }
        let mut im = Img { bin: &bin, w, h, c: Vec::new() };
        if let Some(r) = decode_bin(&mut im, cap) {
            return Some(r);
        }
    }
    None
}

// ================= the C ABI =================

/// Encodes `len` bytes as a QR with the requested correction level, with a fixed mask (0-7; -1 =
/// the best) and a minimum version; `*mask_out` is the one used. `modules` is QR_MAX_SIZE² bytes
/// (1 = dark), row by row with side `*size`. Returns the version (1-40) or -1 if it does not fit.
///
/// # Safety
/// `data` covers `len` bytes; `modules` covers 177² bytes; `size` is writable; `mask_out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qr_encode_ex(data: *const u8, len: usize, ecc: c_int, mask: c_int, minver: c_int, modules: *mut u8, size: *mut c_int, mask_out: *mut c_int) -> c_int {
    if !(0..=3).contains(&ecc) || modules.is_null() || size.is_null() || (len != 0 && data.is_null()) || mask > 7 {
        return -1;
    }
    let bytes: &[u8] = if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(data, len) } };
    let Some(e) = encode(bytes, ecc, mask, minver) else { return -1 };
    unsafe {
        std::ptr::copy_nonoverlapping(e.modules.as_ptr(), modules, e.modules.len());
        *size = e.size;
        if !mask_out.is_null() {
            *mask_out = e.mask;
        }
    }
    e.ver
}

/// Encodes with the best mask and from version 1.
///
/// # Safety
/// As `qr_encode_ex`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qr_encode(data: *const u8, len: usize, ecc: c_int, modules: *mut u8, size: *mut c_int) -> c_int {
    unsafe { qr_encode_ex(data, len, ecc, -1, 1, modules, size, std::ptr::null_mut()) }
}

/// Reads a QR from a 0xAARRGGBB image (w*h). `out` receives the bytes (with a trailing 0 if it
/// fits). Returns the length or -1 if none was found or it could not be read.
///
/// # Safety
/// `argb` covers `w * h` pixels and `out` covers `outcap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qr_decode(argb: *const u32, w: c_int, h: c_int, out: *mut u8, outcap: usize) -> c_int {
    if argb.is_null() || w < 21 || h < 21 || w > 16384 || h > 16384 || out.is_null() || outcap == 0 {
        return -1;
    }
    let pixels = unsafe { std::slice::from_raw_parts(argb, w as usize * h as usize) };
    let Some((bytes, n)) = decode(pixels, w, h, outcap) else { return -1 };
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len().min(outcap)) };
    n as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(data: &[u8], ecc: i32) -> (i32, i32, Vec<u8>) {
        let e = encode(data, ecc, -1, 1).unwrap();
        (e.ver, e.size, e.modules)
    }
    /// The matrix as an image: `scale` pixels per module and a quiet zone of 4 modules.
    fn draw(modules: &[u8], size: i32, scale: i32) -> (Vec<u32>, i32) {
        let side = (size + 8) * scale;
        let mut img = vec![0xffff_ffffu32; (side * side) as usize];
        for y in 0..size {
            for x in 0..size {
                if modules[(y * size + x) as usize] != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img[(((y + 4) * scale + dy) * side + (x + 4) * scale + dx) as usize] = 0xff00_0000;
                        }
                    }
                }
            }
        }
        (img, side)
    }

    #[test]
    fn the_smallest_codes_have_the_known_sizes_and_modes() {
        assert_eq!((make(b"01234567", 1).0, make(b"01234567", 1).1), (1, 21));
        assert_eq!(make(b"HELLO WORLD", 1).0, 1);
        assert_eq!(make(b"hello", 0).0, 1);
        assert_eq!(make(&[b'x'; 200], 0).0, 9);
        assert!(encode(&vec![b'x'; 3000], 3, -1, 1).is_none(), "it does not fit in version 40 at level H");
        assert_eq!(encode(b"abc", 0, 3, 7).unwrap().ver, 7, "the minimum version is honored");
        assert_eq!(encode(b"abc", 0, 3, 1).unwrap().mask, 3, "and so is a fixed mask");
    }
    #[test]
    fn what_it_encodes_it_decodes_at_every_correction_level_and_scale() {
        for (data, ecc, scale) in [(&b"otpauth://totp/Axia:ada?secret=JBSWY3DPEHPK3PXP&issuer=Axia"[..], 1, 4), (&b"12345678901234567890"[..], 2, 3), (&b"HTTPS://EXAMPLE.COM/A-B"[..], 3, 5), (&b"caf\xc3\xa9 \xe2\x82\xac"[..], 0, 6)] {
            let (_, size, modules) = make(data, ecc);
            let (img, side) = draw(&modules, size, scale);
            let (bytes, n) = decode(&img, side, side, 256).unwrap_or_else(|| panic!("{:?}", String::from_utf8_lossy(data)));
            assert_eq!(&bytes[..n], data);
            assert_eq!(bytes[n], 0, "with the trailing NUL");
        }
    }
    #[test]
    fn a_damaged_code_is_corrected_up_to_its_capacity() {
        let data = b"recovery key: correct horse battery staple";
        let (ver, size, mut modules) = make(data, 3);
        // only data modules (the function patterns are what the finder needs to see)
        let mut g = Grid { size, m: vec![0; (size * size) as usize], fnc: vec![0; (size * size) as usize] };
        draw_functions(&mut g, ver);
        let positions = zigzag(&g);
        for (x, y) in positions.iter().step_by(11).take(16) {
            modules[(y * size + x) as usize] ^= 1;
        }
        let (img, side) = draw(&modules, size, 4);
        let (bytes, n) = decode(&img, side, side, 256).expect("within the capacity of level H");
        assert_eq!(&bytes[..n], data);
        for (x, y) in positions.iter().step_by(2) {
            modules[(y * size + x) as usize] ^= 1;
        }
        let (img, side) = draw(&modules, size, 4);
        let wrecked = decode(&img, side, side, 256);
        assert!(wrecked.is_none(), "beyond the capacity it refuses");
    }
    #[test]
    fn an_inverted_code_and_a_blank_image_are_handled() {
        let (_, size, modules) = make(b"inverted", 1);
        let (mut img, side) = draw(&modules, size, 4);
        for p in img.iter_mut() {
            *p = if *p == 0xff00_0000 { 0xffff_ffff } else { 0xff00_0000 };
        }
        let (bytes, n) = decode(&img, side, side, 64).expect("light on dark");
        assert_eq!(&bytes[..n], b"inverted");
        assert!(decode(&vec![0xffff_ffff; 100 * 100], 100, 100, 64).is_none());
        assert!(unsafe { qr_decode(std::ptr::null(), 100, 100, std::ptr::null_mut(), 0) } == -1);
    }
    #[test]
    fn reed_solomon_corrects_and_refuses() {
        let g = rs_gen(10);
        let data: Vec<u8> = (1..=20).collect();
        let mut block = data.clone();
        block.extend(rs_rem(&data, &g));
        let clean = block.clone();
        for i in [0usize, 5, 11, 29, 17] {
            block[i] ^= 0x5a;
        }
        assert!(rs_correct(&mut block, 10));
        assert_eq!(block, clean);
        for i in 0..8 {
            block[i] ^= 0x33;
        }
        assert!(!rs_correct(&mut block, 10), "more errors than it can correct");
    }
}
