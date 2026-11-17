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
    let len = || LenDec {
        choice: 1024,
        choice2: 1024,
        low: [[1024; 8]; 16],
        mid: [[1024; 8]; 16],
        high: [1024; 256],
    };
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
            let prev = if pos > dict_start {
                d[pos - 1] as u32
            } else {
                0
            };
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
            z.state = if z.state < 4 {
                0
            } else if z.state < 10 {
                z.state - 3
            } else {
                z.state - 6
            };
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
        let unpacked =
            (((c & 0x1f) as usize) << 16 | (inp[p] as usize) << 8 | inp[p + 1] as usize) + 1;
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
        let mut rc = Rc {
            inp: &inp[p..p + packed],
            pos: 0,
            range: 0xFFFF_FFFF,
            code: 0,
            bad: false,
        };
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
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
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
            c = if c & 1 != 0 {
                0xC96C_5795_D787_0F42 ^ (c >> 1)
            } else {
                c >> 1
            };
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
    let mut o = Out {
        buf: Vec::new(),
        max,
        dict_start: 0,
    };
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
                let got = if check == 1 {
                    crc32(data) as u64
                } else {
                    crc64(data)
                };
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
