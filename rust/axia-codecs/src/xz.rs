//! In-memory decompression of `.xz` (LZMA2, our own code) and `.gz`: the C ABI of
//! server/src/pkg/xz.h, which this replaced. Used by the Logs journal reader, the package
//! indexes and the benchmark corpus.
//!
//! xz: a container with one or more streams, blocks with a single LZMA2 filter (what Debian's
//! repositories and journald use; no BCJ or delta). Each block's check (CRC32 or CRC64) is
//! verified. The LZMA decoder follows the LZMA specification (Igor Pavlov) and the structure of
//! xz-embedded (public domain). Output is capped at the caller's `max`; every length read from
//! the file is checked against what is left of the input.
struct Rc<'a> {
    inp: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
    bad: bool,
}

impl<'a> Rc<'a> {
    fn byte(&mut self) -> u8 {
        match self.inp.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                b
            }
            None => {
                self.bad = true;
                0
            }
        }
    }
    fn norm(&mut self) {
        if self.range < (1 << 24) {
            self.range <<= 8;
            self.code = self.code << 8 | self.byte() as u32;
        }
    }
    fn bit(&mut self, p: &mut u16) -> u32 {
        self.norm();
        let bound = (self.range >> 11) * *p as u32;
        if self.code < bound {
            self.range = bound;
            *p += (2048 - *p) >> 5;
            0
        } else {
            self.range -= bound;
            self.code -= bound;
            *p -= *p >> 5;
            1
        }
    }
    fn tree(&mut self, p: &mut [u16], limit: u32) -> u32 {
        let mut s = 1u32;
        loop {
            s = s << 1 | self.bit(&mut p[s as usize]);
            if s >= limit {
                break;
            }
        }
        s - limit
    }
    /// Reverse tree; `p[off + s]` with `s` from 1 (`off` may be -1, as in `special`).
    fn tree_rev(&mut self, p: &mut [u16], off: isize, bits: u32) -> u32 {
        let (mut s, mut r) = (1u32, 0u32);
        for i in 0..bits {
            let b = self.bit(&mut p[(off + s as isize) as usize]);
            s = s << 1 | b;
            r |= b << i;
        }
        r
    }
    fn direct(&mut self, bits: u32) -> u32 {
        let mut r = 0u32;
        for _ in 0..bits {
            self.norm();
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            let t = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & t);
            r = (r << 1).wrapping_add(t.wrapping_add(1));
        }
        r
    }
}

struct LenDec {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenDec {
    fn decode(&mut self, rc: &mut Rc, ps: usize) -> u32 {
        if rc.bit(&mut self.choice) == 0 {
            return rc.tree(&mut self.low[ps], 8);
        }
        if rc.bit(&mut self.choice2) == 0 {
            return 8 + rc.tree(&mut self.mid[ps], 8);
        }
        16 + rc.tree(&mut self.high, 256)
    }
}

struct Lzma {
    lc: u32,
    lp: u32,
    pb: u32,
    state: usize,
    rep: [u32; 4],
    is_match: [[u16; 16]; 12],
    is_rep: [u16; 12],
    is_rep0: [u16; 12],
    is_rep1: [u16; 12],
    is_rep2: [u16; 12],
    is_rep0_long: [[u16; 16]; 12],
    slot: [[u16; 64]; 4],
    special: [u16; 114],
    align: [u16; 16],
    len: LenDec,
    replen: LenDec,
    lit: [[u16; 0x300]; 16],
}

fn new_lzma() -> Box<Lzma> {
    let len = || LenDec { choice: 1024, choice2: 1024, low: [[1024; 8]; 16], mid: [[1024; 8]; 16], high: [1024; 256] };
    Box::new(Lzma {
        lc: 0,
        lp: 0,
        pb: 0,
        state: 0,
        rep: [0; 4],
        is_match: [[1024; 16]; 12],
        is_rep: [1024; 12],
        is_rep0: [1024; 12],
        is_rep1: [1024; 12],
        is_rep2: [1024; 12],
        is_rep0_long: [[1024; 16]; 12],
        slot: [[1024; 64]; 4],
        special: [1024; 114],
        align: [1024; 16],
        len: len(),
        replen: len(),
        lit: [[1024; 0x300]; 16],
    })
}

impl Lzma {
    fn reset(&mut self) {
        let (lc, lp, pb) = (self.lc, self.lp, self.pb);
        *self = *new_lzma();
        self.lc = lc;
        self.lp = lp;
        self.pb = pb;
    }
}

struct Out {
    buf: Vec<u8>,
    max: usize,
    dict_start: usize,
}

impl Out {
    /// Makes room for `more` bytes (zeroed) within the cap.
    fn grow(&mut self, more: usize) -> Option<()> {
        let want = self.buf.len().checked_add(more)?;
        if want > self.max {
            return None;
        }
        self.buf.resize(want, 0);
        Some(())
    }
}

/// One LZMA chunk of LZMA2: decodes exactly `unpacked` bytes.
fn lzma_chunk(z: &mut Lzma, rc: &mut Rc, o: &mut Out, unpacked: usize) -> Option<()> {
    let start = o.buf.len();
    o.grow(unpacked)?;
    let end = o.buf.len();
    let dict_start = o.dict_start;
    let d = &mut o.buf[..];
    let mut pos = start;
    let pbmask = (1u32 << z.pb) - 1;
    let lpmask = (1u32 << z.lp) - 1;
    while pos < end && !rc.bad {
        let ps = (pos as u32 & pbmask) as usize;
        if rc.bit(&mut z.is_match[z.state][ps]) == 0 {
            let prev = if pos > dict_start { d[pos - 1] as u32 } else { 0 };
            let li = (((pos as u32 & lpmask) << z.lc) + (prev >> (8 - z.lc))) as usize;
            let p = &mut z.lit[li];
            let sym;
            if z.state < 7 {
                sym = rc.tree(p, 0x100);
            } else {
                if pos - dict_start <= z.rep[0] as usize {
                    return None;
                }
                let mut mb = (d[pos - z.rep[0] as usize - 1] as u32) << 1;
                let mut off = 0x100u32;
                let mut s = 1u32;
                loop {
                    let mbit = mb & off;
                    mb <<= 1;
                    if rc.bit(&mut p[(off + mbit + s) as usize]) != 0 {
                        s = s << 1 | 1;
                        off = mbit;
                    } else {
                        s <<= 1;
                        off ^= mbit;
                    }
                    if s >= 0x100 {
                        break;
                    }
                }
                sym = s;
            }
            d[pos] = sym as u8;
            pos += 1;
            z.state = if z.state < 4 { 0 } else if z.state < 10 { z.state - 3 } else { z.state - 6 };
            continue;
        }
        let mut len;
        if rc.bit(&mut z.is_rep[z.state]) == 0 {
            z.rep[3] = z.rep[2];
            z.rep[2] = z.rep[1];
            z.rep[1] = z.rep[0];
            len = z.len.decode(rc, ps);
            z.state = if z.state < 7 { 7 } else { 10 };
            let ls = len.min(3) as usize;
            let slot = rc.tree(&mut z.slot[ls], 64);
            if slot < 4 {
                z.rep[0] = slot;
            } else {
                let bits = (slot >> 1) - 1;
                let mut dist = (2 | (slot & 1)) << bits;
                if slot < 14 {
                    dist += rc.tree_rev(&mut z.special, dist as isize - slot as isize - 1, bits);
                } else {
                    dist = dist.wrapping_add(rc.direct(bits - 4) << 4);
                    dist = dist.wrapping_add(rc.tree_rev(&mut z.align, 0, 4));
                }
                z.rep[0] = dist;
                if dist == 0xFFFF_FFFF {
                    return None; // end marker: not allowed in LZMA2
                }
            }
        } else {
            if rc.bit(&mut z.is_rep0[z.state]) == 0 {
                if rc.bit(&mut z.is_rep0_long[z.state][ps]) == 0 {
                    z.state = if z.state < 7 { 9 } else { 11 };
                    if pos - dict_start <= z.rep[0] as usize {
                        return None;
                    }
                    d[pos] = d[pos - z.rep[0] as usize - 1];
                    pos += 1;
                    continue;
                }
            } else {
                let dist;
                if rc.bit(&mut z.is_rep1[z.state]) == 0 {
                    dist = z.rep[1];
                } else {
                    if rc.bit(&mut z.is_rep2[z.state]) == 0 {
                        dist = z.rep[2];
                    } else {
                        dist = z.rep[3];
                        z.rep[3] = z.rep[2];
                    }
                    z.rep[2] = z.rep[1];
                }
                z.rep[1] = z.rep[0];
                z.rep[0] = dist;
            }
            len = z.replen.decode(rc, ps);
            z.state = if z.state < 7 { 8 } else { 11 };
        }
        len += 2;
        if pos - dict_start <= z.rep[0] as usize || len as usize > end - pos {
            return None;
        }
        let src = pos - z.rep[0] as usize - 1;
        for i in 0..len as usize {
            d[pos + i] = d[src + i];
        }
        pos += len as usize;
    }
    if rc.bad {
        return None;
    }
    o.buf.truncate(pos);
    Some(())
}

fn lzma2(inp: &[u8], mut p: usize, o: &mut Out, z: &mut Lzma) -> Option<usize> {
    let (mut need_dict, mut need_props) = (true, true);
    loop {
        let c = *inp.get(p)?;
        p += 1;
        if c == 0 {
            break;
        }
        if c == 1 || c == 2 {
            if inp.len() - p < 2 {
                return None;
            }
            let n = ((inp[p] as usize) << 8 | inp[p + 1] as usize) + 1;
            p += 2;
            if c == 1 {
                o.dict_start = o.buf.len();
                need_dict = false;
            } else if need_dict {
                return None;
            }
            if inp.len() - p < n {
                return None;
            }
            if o.buf.len().checked_add(n)? > o.max {
                return None;
            }
            o.buf.extend_from_slice(&inp[p..p + n]);
            p += n;
            continue;
        }
        if c < 0x80 || inp.len() - p < 4 {
            return None;
        }
        let unpacked = (((c & 0x1f) as usize) << 16 | (inp[p] as usize) << 8 | inp[p + 1] as usize) + 1;
        let packed = ((inp[p + 2] as usize) << 8 | inp[p + 3] as usize) + 1;
        p += 4;
        let mode = (c >> 5) & 3;
        if mode == 3 {
            o.dict_start = o.buf.len();
            need_dict = false;
        } else if need_dict {
            return None;
        }
        if mode >= 2 {
            let mut pr = *inp.get(p)? as u32;
            p += 1;
            if pr >= 225 {
                return None;
            }
            z.lc = pr % 9;
            pr /= 9;
            z.lp = pr % 5;
            z.pb = pr / 5;
            if z.lc + z.lp > 4 {
                return None;
            }
            need_props = false;
        } else if need_props {
            return None;
        }
        if mode >= 1 {
            z.reset();
        }
        if inp.len() - p < packed || packed < 5 {
            return None;
        }
        let mut rc = Rc { inp: &inp[p..p + packed], pos: 0, range: 0xFFFF_FFFF, code: 0, bad: false };
        if rc.byte() != 0 {
            return None;
        }
        for _ in 0..4 {
            rc.code = rc.code << 8 | rc.byte() as u32;
        }
        lzma_chunk(z, &mut rc, o, unpacked)?;
        p += packed;
    }
    Some(p)
}

fn varint(inp: &[u8], p: &mut usize, end: usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..9 {
        if *p >= end {
            return None;
        }
        let b = inp[*p];
        *p += 1;
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

const fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
const fn crc64_table() -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u64;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xC96C_5795_D787_0F42 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC32: [u32; 256] = crc32_table();
static CRC64: [u64; 256] = crc64_table();

fn crc32(p: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in p {
        c = CRC32[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}
fn crc64(p: &[u8]) -> u64 {
    let mut c = !0u64;
    for &b in p {
        c = CRC64[((c ^ b as u64) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}
fn le32(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

/// Decodes every stream of `inp`; the output may not exceed `max` bytes.
pub(crate) fn decode(inp: &[u8], max: usize) -> Option<Vec<u8>> {
    const MAGIC: [u8; 6] = [0xFD, b'7', b'z', b'X', b'Z', 0];
    let mut o = Out { buf: Vec::new(), max, dict_start: 0 };
    let mut z = new_lzma();
    let end = inp.len();
    let (mut p, mut streams) = (0usize, 0);
    while p < end {
        if streams > 0 && inp[p] == 0 {
            // padding between streams
            if end - p < 4 || inp[p + 1] != 0 || inp[p + 2] != 0 || inp[p + 3] != 0 {
                return None;
            }
            p += 4;
            continue;
        }
        if end - p < 12 || inp[p..p + 6] != MAGIC || inp[p + 6] != 0 {
            return None;
        }
        let check = (inp[p + 7] & 0x0f) as usize;
        if crc32(&inp[p + 6..p + 8]) != le32(&inp[p + 8..]) {
            return None;
        }
        let checklen = match check {
            0 => 0,
            1 => 4,
            4 => 8,
            10 => 32,
            c => 4usize << ((c - 1) / 3),
        };
        p += 12;
        loop {
            // blocks
            if p >= end {
                return None;
            }
            if inp[p] == 0 {
                break; // index
            }
            let bstart = p;
            let hsize = (inp[p] as usize + 1) * 4;
            if end - p < hsize {
                return None;
            }
            let hend = p + hsize - 4;
            if crc32(&inp[p..hend]) != le32(&inp[hend..]) {
                return None;
            }
            let mut h = p + 1;
            let flags = inp[h];
            h += 1;
            if flags & 0x3c != 0 {
                return None;
            }
            if flags & 0x40 != 0 {
                varint(inp, &mut h, hend)?;
            }
            if flags & 0x80 != 0 {
                varint(inp, &mut h, hend)?;
            }
            if flags & 3 != 0 {
                return None; // a single filter
            }
            let id = varint(inp, &mut h, hend)?;
            let psize = varint(inp, &mut h, hend)?;
            if id != 0x21 || psize != 1 || h >= hend {
                return None;
            }
            p += hsize;
            let ostart = o.buf.len();
            p = lzma2(inp, p, &mut o, &mut z)?;
            while (p - bstart) & 3 != 0 {
                if p >= end || inp[p] != 0 {
                    return None;
                }
                p += 1;
            }
            if end - p < checklen {
                return None;
            }
            if check == 1 || check == 4 {
                // CRC32 or CRC64 (little-endian)
                let mut want = 0u64;
                for i in 0..checklen {
                    want |= (inp[p + i] as u64) << (8 * i);
                }
                let data = &o.buf[ostart..];
                let got = if check == 1 { crc32(data) as u64 } else { crc64(data) };
                if want != got {
                    return None;
                }
            }
            p += checklen;
        }
        // index: 0, count, (sizes)…, padding, CRC32; then the 12-byte footer
        p += 1;
        let istart = p - 1;
        let nrec = varint(inp, &mut p, end)?;
        for _ in 0..nrec {
            varint(inp, &mut p, end)?;
            varint(inp, &mut p, end)?;
        }
        while (p - istart) & 3 != 0 {
            if p >= end {
                return None;
            }
            p += 1;
        }
        if end - p < 4 + 12 {
            return None;
        }
        if crc32(&inp[istart..p]) != le32(&inp[p..]) {
            return None;
        }
        p += 4;
        // footer: CRC32, index size, flags (same as the header's), "YZ"
        if inp[p + 10] != b'Y'
            || inp[p + 11] != b'Z'
            || inp[p + 8] != 0
            || (inp[p + 9] & 0x0f) as usize != check
            || crc32(&inp[p + 4..p + 10]) != le32(&inp[p..])
        {
            return None;
        }
        p += 12;
        streams += 1;
    }
    if streams == 0 {
        None
    } else {
        Some(o.buf)
    }
}

unsafe fn finish(decoded: Option<Vec<u8>>, out: *mut *mut u8, outlen: *mut usize) -> i32 {
    let Some(v) = decoded else { return -1 };
    let p = crate::export(&v);
    if p.is_null() {
        return -1;
    }
    *out = p;
    *outlen = v.len();
    0
}

/// 0 ok; `*out` is malloc'd (free()). `max`: maximum accepted output size.
#[no_mangle]
pub unsafe extern "C" fn xz_decode(inp: *const u8, len: usize, out: *mut *mut u8, outlen: *mut usize, max: usize) -> i32 {
    if out.is_null() || outlen.is_null() {
        return -1;
    }
    let Some(data) = crate::input(inp, len) else { return -1 };
    finish(decode(data, max), out, outlen)
}

/// 0 ok; `*out` is malloc'd (free()). The bounded Rust gunzip.
#[no_mangle]
pub unsafe extern "C" fn gz_decode(inp: *const u8, len: usize, out: *mut *mut u8, outlen: *mut usize, max: usize) -> i32 {
    if out.is_null() || outlen.is_null() {
        return -1;
    }
    let Some(data) = crate::input(inp, len) else { return -1 };
    finish(crate::compression::decode(data, max, true), out, outlen)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT64: &str = "fd377a585a000004e6d6b4460200210116000000742fe5a3e01f3f06e65d00351bcb11f07c08a8d244cdf41c71569cbe9ca882dfb8184dd06a74ab8c116731bbe57e17e96d51397c3e2fae63a28d455fae076b303e4d710fda4734a5518399e09e07befaa35fd0c662aae7f10567fab51bb1b3453b4c7e5f90a03011d98f65bd58764cc61712fbccb012c306f84b54105ded10ebe20ad917cc034fc7a3bd1f60870ed8b7d67fcf28558c04c76f8ac3802fe235d532078e7dd05298c68051976866caf293cbce80e50c51fa73322e52896158abc43519bdff553f82f6d5252fda189a902301f3810561a37e7a4dbc2fe18a5d6882ac98ba5551b052a2736669e06ab193758fa3871a7de0a1f549856c3e6610c4b49f9ae22a11c1d847c49d874bafb21facb9589b0b9a97039e093a26925072e7c1e175e0dc98f3ed9c73a7a4e7b2dac55eea1123c0bf6cc8caf3ea3adf3d5a658d5dc3728b920570167412756df8e9b3e203df9989ab51b0885a94998599a8caea97df690571ab0d6fc8ed81d2919914edd0f807e0e5d4ff36772a67940ac066c3d99255142588e93988d1ddfc15afbf548504d37e0dcf1fe16f4ab1cd985ad60d29f121de850d2868827125b948d2c93ccc4766e63c3a7e7a291d7694074eceb51a3c7510c188e46de1b677bcfb1b6233834370c60939f03c6dc8aee26475db9fc5b11dce6d6c6045e77dc88fc24993ae6c4235aa516a9cf377ccfa7d862d4a5b88ccce96aae4fc10bac82aafd7a66d59360822ffa7dfcf1ebe79d81a98f70468d41d6c05b4aab736b1ff42685c1952c55ea4d48356fb444ad1f6dbba087d450b8c4a04fe02e92e8d957d10eb97896e861f604fe34bd91512820c3050e4d3315a7dcd2b96adb6c82ac2700662fab3019369ea2eaf6a40dd193fc15d83e8fe09a84f356add8b54661fb0fcda5040afc8a108383a9173763487a4e346eb377f37928520d4d36adb0d92949574866db9c517e780dfdd8eb606cb69ff7dfb70207e5a540d21121536e3d6eb9f8b0dc599284a2b786ef432860ff36fe62d7d67dff2d5039eeab228393e7f81e8eb9109dced501d90ce8120520cacae05e4e84bcddadb495ee7f70e76ded31c24bf2eecaa5a0b120d77c3470d02f09b6b8b4cf6c354dfa5c1ec9d310e6342c056965913ff923c72fd997bbaa35c338aebb18474a2c25c93e95f08d700c77f60f8f281c50f693d4c61b9fbbd06d075c1becfd0ff3c051581365acad0c5d5d1ae3b8f3b429177ae356084a21edf8196b2003079076facb4d7cf6cfba5e174c31dbe2b77545be19c17e3caeeaa4ae1d5e03d95aad91fafcc93dab4a1079c941552bb83856967aacae7f1de101e962a7bdb0c17ed3c22d0a6672ead3f1e401a2ee410cf62d2dbdeede4be623dc96869e89358b6e49968e947dc1d90fc14d3ba0ec877ddc9070aa6d33c3e8f4266e91886b5d15a8759816c132301c9f1059cc8ea80fd8c0f6f26449e868406d5f9eddb18f66b943aa4d32660671958e0e6e8cdbbc8c29423a3d85c77c5331705122cbfcb8a859435a5db257b0338df8e92688f5db11961dba546d8bf3a01c4b505a903c4ce5711fd72ebe7c92c7ff6f026ab86d9a6dc311e5ef9fd3b74e57f09aebadfa11b43e59aa195e6c94d42001e12747363c381c2e53fd90d3e470c7eb0a22eb32b631de43f369778db3f262ac46d55d9f05491b100dc533353e6290e8258abcfb7a51321004436171893a400342526494b5d36ba98fed3c68ae660ec68262ac62fa0f34e27a9878f233146e81351f53fe805d472f63e81af78c19198f5c0be85880bf55d527e7647ac55102e92b79b6b54a6e2a4f38971f54072545fa075cb50908898d2d858edb3437765e9125c6ccf8e8c4325a47aa45661b3f455737cfc7ccf66bd81904ee4181ee81f447853a9420a4ffc57d92d1992a0d25c5dd528743f55b4095466bb534092a62b2579b1618bc3576c42558cd1fca2b8b33ef6385c8c181a0bee26b2d445a31838a803578f334456f30c54b6b9178e90406495e54c02fe0861693b6f8fdbb56a0fc2e94458a36aae2dbd930711f4d27cd442f949f54869742c57646d440f300f80311fd0318de29f66d87e7efdc466a725aacf777e816ed6332e59ce48f0f2338ed2b0d154b1dd48144a73129f4f0f6b74aeffc49262e1e8925cf57de82ccb6454032e6912bd8e24db73ff018138add24fc1e98116b3d15c4c05c95b2edb270015994175b5c0cca54971976b56e6a7bd4a33b600fda084e29b4c3ef6aea4c9e011fa59a54c370357a23978643b5a30a1ea074ce9a98611cbb2ad2448c402b927cb4aa31fc5fc003a4f293a9b39e5d6bd4d254af592efa0d6b5e7314f571ab6cb0dc82c51180f7300abdaae16aa67cd7857dd8c33224874807ad677b547d372732685ae4baadbb8826c5e49c4e04497a6f555cb745f08cacb3fe71f393593ce3bee266f3c141910048b0c223f9221bd243730ea390a483265d91ba8351b5b27e60a479f1d3a82b958b8b51eb721eee0a40000005845919178d3770b0001820ec03e0000f3f9a577b1c467fb020000000004595a";
    const TEXT32: &str = "fd377a585a0000016922de360200210116000000742fe5a3e0138704985d003a1a08ce766d6708cab8b54955349718be2d73b06ecdae702af6f9e45537615d4b1ed8ede847e46aa4a7cf48d731d655479f5bac8a1689ce84c3d9f140f3eb69c39e24c2539c1aa61cf3f85a9f708a74da35bc2101eaaad1f4926ea37dd6f13e27d65bc83489bd98386afbc824be764d1bf36af738f62840f2f9ee1b6d893126985152f1f65acf46fde38f4f25663e3497e787f4b18d04f9b9473f1ab0728733686ad6dda59320592e182ce0550502d3e288ed23fe42749d20bf7fcfade572059f9d6fa3cb579bb023be8a3b6619a801ee39dc1471cf2c1fefa81b823d757e74f999a39573df92b465d108743f12bf56c346bc1569218c530d772e3d4ecccbaaee66068aa1782780de88d6bd56d071799be1550eab1b6997fa9f37dec861819a1bab1b65170a8bb90872c4e156ab62ee10c6ef5c22b7fec53a1fbfb785b225a81d1848e1e179708fdd5d3308ab3c6f32192083887d398b7f9be0fae76b952f559db6714bcecda1a2d5e8f626b22089f1706e52224a67790596be49a3c0f4cc885a99cbc17bccccf47c92b0550478b2c51589f039552449098d835bb2520565fb46c56abaca6b3fb924f1584ba49bfcc6d293f202e23f724de176f31f03d2f9299d28ff2c7d440a8c063270eb154c3b3e04d65513d3cae7da1cedfdf9e8879f181e8e4b6705b5b06b14af108897757be084040ee994463d1d78a738a3ff7d76df310f70afe3dc952ff4836b745d288dbdc8b4107b99cd835eeb446d74599bf770d34253716890041095efe39927b70f6f405ce819b38345ccddae1a6783e38d584e24cf95d89df6855804afc9af4b7ab53e03908d1d99d36454ca789f708bb156207e0e46f26799bbd33bd4581ea182d0de164da26776f41ce6e7b96e2afc72c2933fafa93fd10c57416766925020ddb98924d92b6099f23d29aa5063b189b6f91a9ec7420c41421110f4f28a1bc9b9afce3f9115b785767599eac70c0f06ea1ec64c335dfcab702824deca5c62c7a3933111b0160216b65b8cfd1c6e7ec2045670f977b8055a1b3a35188af8dad4ad5a8f5165d52c8c125ecab3c08b8ce336a44c29d836a94ae99676915731d0cde89a06f16d1d4d84bb9eb5e69df6cdf13c27ab13a4cb919b08eaff15fedf930e14eee9d15927c37bfc6330362fb5fb9fd84200756754f1755128ad767c78a217093cdeb20e6632619bdad7fc3502cd27b4d92c5644a1fcd727c1e7f5197aa514135f5d211b9a6c01c1eac443893eedc1d9436ed27b59e117c9b0ee5e45de2c81df1f9b0c86307e73c518200aa5c051468c927766ab48159938b2652435c060813b1117bd7d5665c3eacbada18ab1c346c869394796b5375bfd56143b952131418abab2cd6175e077f2f839c9bec35bc6a8d888a9fc8b54fc2282f9b877da082d721ab1afc85c83e1bad2accb9c71e464de2d9ae708c8838692e73eb9b7b5b96cd30a74c95f13bfceb7bf2013d24f1804581dfa2a919ba8e29c735dcfee9ad7d5b3a231b4634bfb80476637e67819a831b0d86d9b0cb845103a6c60825b829048bcdf0ddd16ddee4cd5e4c9371794484e67c4fb6d84e33c826af053fe3a15d8cc68938afe778380e62203680907e6c45c2415b8daae61ec3904b921edcf73b48ffb58bf9170758374089900036e49f50001b00988270000686b60c13e300d8b020000000001595a";
    const NONE: &str = "fd377a585a000000ff12d9410200210116000000742fe5a3e0001d00095d00309888aafc068980000000000000011d1e54e4e52506729e7a010000000000595a";
    const SHA: &str = "fd377a585a00000ae1fb0ca10200210116000000742fe5a301000d73686132353620636865636b65640000005ff3f99b80310ab09a678b8a082a12daff1cf4f157cd882c4e9ee004673476a000013e0e5183fb86189b4b9a01000000000a595a";
    const RND: &str = "fd377a585a000004e6d6b4460200210116000000742fe5a30105db1be55d996f245c6cda922e55086d62ef64af42981b14db6a191af9420afb28bb59b4b3333401a7c4845dd04bf806b963fbf6b06ab8e9bf7a1c5db270d28c17e74873383da8ce228cdf18a1b1998e4147412c4dac04aed2face909c0e4b0d3683e622eeb6cc8bcec4e9c3a286e907f89b37531a5d006315ea30b3b51b737e868f35c1d3a0a137a96ba45ed4cce970df5fde6a187fad08894943c5ff994bdf050b3350e8fa25d4b4830ee935829ac9f79334714511099d2c1905c87887d42fb4f7e1ce2ec45961f00b2864c7a8fa123e373a68a2131622ff5978bb21e50c709453403da3fe3ede5b03f3cf883e0a4bb64bf14f3085d29703099a9efbb3f5a1a31f4e9c48a8d24bf76b6d297944cb745dce5726ed673efb36296c7104f18dc2e35a0ceb1ec217a8c24397749bba3b8d34c26eeddbb95b509ab9ef343d9fd5d352067b2a234c0bf5bd8b72afeca05c953c2634a3f87b27952db921e7a7bdced4f12299595945af32e943fcda6df52c8e73faaa4a45aca3caf029038a404a76c5c1ae6878beaf045e446a37f51fbbac77da3ed1e1c34ed0efe408961c0a48cea6c0aae6875389087bcf02210000f1dd5072f2a7687060ac040758d89f03b6d776ef16148519d3bc888b0abbfb1297bd1939162ddf4de239095a18cb122c948f374ff2f3740e8d2185768206e653ad4dd231aa64465bd475fede486d7586e2f8e8de3e815333b73572926a00c0c4338e7b58ad4a9d983661e291e8bfc80fa010899ef9bf228951fa4fddc2aa8b66297e14af21e1e40608feb775f8c20bc8ced81a8d25aee10e5b6e1c588a0546378f1f9c3705271aa34a4a7c8878743eb26c519bacc14c90c4d492d904c20ff138646f15ee591d4170fb4631cb68d661cb79790cfccd9b2fa9be6785df69ca8c6aaa6f7ed51dc4586e1df3d63fc7e97315f7516d7b4999bebe18eb22ea813417cb6020d7ed511a5f9baf4f6be297ab768ee5dabd2a35473c04e21faf26094e925d86802034f52056f8b15cf0b5d71e69374312fe0a91a6ab791c042c910119fd1e1c9059ac09d73d86231865b987404c1111fb2f195db40a1ec698992d6993570f225182538c3d14430584c9eca7121f1a4f935059987259f4e0ddf6f841bda23198c19b2a8054db3157a11e503795e253fddb61c844e198f96ad143f38998dd827ee2a332e41a914e196d63e337c795bd1bd4626760ffc72f6375801f41220803d3b29c69b890389b4ae998a6a7a93856a87c5337eccee694cdcaac7a685d23b5993285d0fcb7cf19ec16bd9a863cca20a6d43436e84cf529a7cec90500472e96cf635e68b08d56c6fef2d2bdd67e4d1914dd0465547af3eff182d6c108e129a87543272fd413cda3807fd7109671c042f295c5c53059e2e4abe7d3ac802c14e98bd3e027684a34b172bbcc94c613239c40fe76547ce7c109d5fbc0c86826ee5a1114894e6b308beed4a7b04766657625fe1617143725905f6fc09d13c0d0b9fc070422a5dfc7d25ca644852b7668b9de2cdea80363144fe479346a8e882b74cd6da5aa2dc30d08f9e8904a3ef61aecc8ca572922ffb2fe9af258273ac0351fce42f523e417032fc6998cff81e67c0fe1d77f9a720b019d7fdc2dd31567f0ba0088f58ccbdbaa46c4bb39a4f5458f212b5557fbf187ff2c9436b170207f5a45613ea6e5e30f004df14d953998155124a443e04ba073adaada00e5fd5c0774c0f364062e2ab306444e4fa2be6b55c4174da1188c80cf0c194f3aca7ac7ff3f2bb4fa1667a2c7bd2bdcc15e336f05e6fa276f01bd8f9b1a78f4e45ee76367b985a500d790494a230299a3ca97a225b8cd30ad99decfd6d8c7cafea3442e3fe3d0c77647a9203e3ac987f5e7ec06b53b90695be1ef3e824706cf8897912987be0b185c68b128a2007fa4b7b33199b56d43d379daf0dd9e653505813bce553f483699b138aa5f767626f1e82f665c254fe66c0682e1ad2a34546aeb90fab06683514b77b893c6ba9dbd6f8a5c8b5004e2783604dac2ac26b263ffc594173bfe6d5c2cade96c5e6e73441c9c38a0735424fb2f72e392db92e061c50f4824f70607573e53c9352592f9bbf91ec5a5c614176e585000a898b62d56930fc80001f40bdc0b00000357515ab1c467fb020000000004595a";
    const EMPTY: &str = "fd377a585a000004e6d6b446000000001cdf44211fb6f37d010000000004595a";

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    fn lcg(seed: u64) -> impl FnMut() -> u64 {
        let mut x = seed;
        move || {
            x = (x * 1103515245 + 12345) & 0x7fffffff;
            x >> 8
        }
    }
    fn text(n: usize, seed: u64) -> Vec<u8> {
        const W: [&str; 18] = [
            "alpha", "beta", "gamma", "delta", "packet", "journal", "systemd", "unit", "kernel", "boot", "error",
            "warning", "started", "stopped", "the", "of", "and", "to",
        ];
        let mut g = lcg(seed);
        let (mut words, mut l) = (Vec::new(), 0);
        while l < n {
            let mut w = W[(g() % 18) as usize].to_string();
            if g() % 11 == 0 {
                w += &(g() % 1000).to_string();
            }
            l += w.len() + 1;
            words.push(w);
        }
        words.join(" ").into_bytes()[..n].to_vec()
    }
    fn rnd(n: usize, seed: u64) -> Vec<u8> {
        let mut g = lcg(seed);
        (0..n).map(|_| (g() & 255) as u8).collect()
    }

    #[test]
    fn checks_and_chunk_kinds() {
        assert_eq!(decode(&unhex(TEXT64), 1 << 20).unwrap(), text(8000, 7)); // matches, reps, CRC64
        assert_eq!(decode(&unhex(TEXT32), 1 << 20).unwrap(), text(5000, 9)); // CRC32
        assert_eq!(decode(&unhex(NONE), 100).unwrap(), b"abc".repeat(10)); // no check
        assert_eq!(decode(&unhex(SHA), 100).unwrap(), b"sha256 checked"); // SHA-256 (not verified)
        assert_eq!(decode(&unhex(RND), 2000).unwrap(), rnd(1500, 3)); // uncompressed LZMA2 chunk
        assert_eq!(decode(&unhex(EMPTY), 10).unwrap(), b"");
    }

    #[test]
    fn limits_and_corruption() {
        let t = unhex(TEXT64);
        assert!(decode(&t, 7999).is_none(), "output over max");
        assert_eq!(decode(&t, 8000).unwrap().len(), 8000);
        assert!(decode(&[], 10).is_none());
        assert!(decode(&t[..t.len() - 1], 1 << 20).is_none());
        for cut in [5, 12, 20, 100, t.len() / 2] {
            assert!(decode(&t[..cut], 1 << 20).is_none(), "cut at {cut}");
        }
        // every single flipped byte is rejected or decodes without panicking
        for i in 0..t.len() {
            let mut b = t.clone();
            b[i] ^= 0x55;
            let _ = decode(&b, 1 << 20);
        }
        // the checks catch damage inside the data
        let mut b = t.clone();
        b[t.len() / 2] ^= 1;
        assert!(decode(&b, 1 << 20).is_none());
    }

    #[test]
    fn concatenated_streams_with_padding() {
        let mut b = unhex(NONE);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&unhex(SHA));
        assert_eq!(decode(&b, 1000).unwrap(), [b"abc".repeat(10), b"sha256 checked".to_vec()].concat());
        b.extend_from_slice(&[0, 0, 1, 0]);
        assert!(decode(&b, 1000).is_none());
    }

    #[test]
    fn c_abi() {
        let t = unhex(NONE);
        let (mut out, mut n) = (std::ptr::null_mut(), 0usize);
        assert_eq!(unsafe { xz_decode(t.as_ptr(), t.len(), &mut out, &mut n, 100) }, 0);
        assert_eq!(unsafe { std::slice::from_raw_parts(out, n) }, b"abc".repeat(10));
        unsafe { libc::free(out.cast()) };
        assert_eq!(unsafe { xz_decode(t.as_ptr(), t.len(), &mut out, &mut n, 5) }, -1);
        let e = unhex(EMPTY);
        assert_eq!(unsafe { xz_decode(e.as_ptr(), e.len(), &mut out, &mut n, 5) }, 0);
        assert_eq!(n, 0);
        assert!(!out.is_null());
        unsafe { libc::free(out.cast()) };
    }
}
