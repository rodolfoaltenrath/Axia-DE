//! Bounded request ownership and byte-oriented X11/ICCCM/EWMH parsing.
pub const OUTPUT_LIMIT: usize = 64 << 20;

#[derive(Default)]
pub struct Output {
    pub bytes: Vec<u8>,
    start: usize,
    pub failed: bool,
}
impl Output {
    pub fn room(&mut self, more: usize) -> bool {
        if self.failed {
            return false;
        }
        let Some(needed) = self.bytes.len().checked_add(more) else {
            self.failed = true;
            return false;
        };
        if needed <= self.bytes.capacity() {
            return true;
        }
        let mut cap = self.bytes.capacity().max(4096);
        while cap < needed && cap <= OUTPUT_LIMIT {
            cap *= 2;
        }
        if cap > OUTPUT_LIMIT
            || self
                .bytes
                .try_reserve_exact(cap - self.bytes.len())
                .is_err()
        {
            self.failed = true;
            return false;
        }
        true
    }
    pub fn append(&mut self, data: &[u8]) {
        if self.room(data.len()) {
            self.bytes.extend_from_slice(data);
        }
    }
    pub fn pad(&mut self, n: usize) {
        if self.room(n) {
            self.bytes.resize(self.bytes.len() + n, 0);
        }
    }
    pub fn bytes(&mut self, data: &[u8]) {
        let Some(more) = data.len().checked_add(3) else {
            self.failed = true;
            return;
        };
        if !self.room(more) {
            return;
        }
        self.bytes.extend_from_slice(data);
        self.pad((4 - data.len() % 4) % 4);
    }
    pub fn begin(&mut self, opcode: u8, data: u8) {
        self.start = self.bytes.len();
        self.append(&[opcode]);
        self.append(&[data]);
        self.append(&[0; 2]);
    }
    pub fn end(&mut self) {
        if self.failed {
            return;
        }
        // A complete begin must precede end, with no drop in between.
        if self.bytes.len() < self.start + 4 {
            self.failed = true;
            return;
        }
        let units = (self.bytes.len() - self.start).div_ceil(4) as u16;
        self.bytes[self.start + 2..self.start + 4].copy_from_slice(&units.to_le_bytes());
    }
    pub fn drop_prefix(&mut self, n: usize) {
        let n = n.min(self.bytes.len());
        self.bytes.copy_within(n.., 0);
        self.bytes.truncate(self.bytes.len() - n);
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub id_base: u32,
    pub id_mask: u32,
    pub root: u32,
    pub root_visual: u32,
    pub max_req_bytes: u32,
    pub width: u16,
    pub height: u16,
    pub root_depth: u8,
    pub min_keycode: u8,
    pub max_keycode: u8,
}
pub fn get16(p: &[u8]) -> u16 {
    u16::from_le_bytes(p[..2].try_into().unwrap())
}
pub fn get32(p: &[u8]) -> u32 {
    u32::from_le_bytes(p[..4].try_into().unwrap())
}
pub fn setup_request() -> [u8; 12] {
    [b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}

pub struct ParsedSetup {
    pub status: i32,
    pub used: Option<usize>,
    pub setup: Option<Setup>,
}
pub fn parse_setup(p: &[u8]) -> ParsedSetup {
    let mut result = ParsedSetup {
        status: 0,
        used: None,
        setup: None,
    };
    if p.len() < 8 {
        return result;
    }
    let total = 8 + 4 * get16(&p[6..]) as usize;
    if p.len() < total {
        return result;
    }
    result.used = Some(total);
    result.status = -1;
    if p[0] != 1 || total < 40 {
        return result;
    }
    let d = &p[8..total];
    let mut s = Setup {
        id_base: get32(&d[4..]),
        id_mask: get32(&d[8..]),
        max_req_bytes: get16(&d[18..]) as u32 * 4,
        min_keycode: d[26],
        max_keycode: d[27],
        ..Setup::default()
    };
    let off = 32 + (get16(&d[16..]) as usize).div_ceil(4) * 4 + d[21] as usize * 8;
    if d[20] == 0 || off + 40 > d.len() || s.id_mask == 0 {
        result.setup = Some(s);
        return result;
    }
    let screen = &d[off..];
    s.root = get32(screen);
    s.width = get16(&screen[20..]);
    s.height = get16(&screen[22..]);
    s.root_visual = get32(&screen[32..]);
    s.root_depth = screen[38];
    result.status = if s.root != 0 { 1 } else { -1 };
    result.setup = Some(s);
    result
}
pub fn packet_len(p: &[u8]) -> usize {
    if p.len() < 32 {
        return 0;
    }
    let additional = if p[0] == 1 || p[0] & 0x7f == 35 {
        get32(&p[4..]) as usize * 4
    } else {
        0
    };
    let n = 32 + additional;
    if p.len() >= n {
        n
    } else {
        0
    }
}
pub fn seq_widen(last_sent: u32, low: u16) -> u32 {
    let full = (last_sent & !0xffff) | low as u32;
    if full > last_sent {
        full.wrapping_sub(0x10000)
    } else {
        full
    }
}

fn ascii_copy(input: &[u8], out: &mut [u8]) {
    if out.is_empty() {
        return;
    }
    let mut o = 0;
    for &c in input {
        if c == 0 || o + 1 >= out.len() {
            break;
        }
        if (0x20..0x7f).contains(&c) {
            out[o] = c;
            o += 1;
        }
    }
    out[o] = 0;
}
pub fn wm_class(input: &[u8], inst: &mut [u8], class: &mut [u8]) {
    let a = input.iter().position(|&b| b == 0).unwrap_or(input.len());
    ascii_copy(&input[..a], inst);
    ascii_copy(input.get(a + 1..).unwrap_or_default(), class);
}
fn put_utf8(out: &mut [u8], o: &mut usize, cp: u32) {
    if cp < 0x20 || (0x7f..0xa0).contains(&cp) {
        return;
    }
    let Some(c) = char::from_u32(cp) else {
        return;
    };
    let mut bytes = [0; 4];
    let value = c.encode_utf8(&mut bytes).as_bytes();
    if *o + value.len() < out.len() {
        out[*o..*o + value.len()].copy_from_slice(value);
        *o += value.len();
    }
}
pub fn title(input: &[u8], latin1: bool, out: &mut [u8]) {
    if out.is_empty() {
        return;
    }
    let (mut i, mut o) = (0, 0);
    while i < input.len() && o + 1 < out.len() {
        let cp = input[i] as u32;
        if cp == 0 {
            break;
        }
        if latin1 || cp < 0x80 {
            i += 1;
            put_utf8(out, &mut o, cp);
            continue;
        }
        let k = match cp {
            0xf0..=0xf4 => 4,
            0xe0..=0xef => 3,
            0xc2..=0xdf => 2,
            _ => 0,
        };
        let mut c = cp
            & match k {
                2 => 0x1f,
                3 => 0x0f,
                _ => 0x07,
            };
        let mut ok = k != 0 && i + k <= input.len();
        for j in 1..k {
            if !ok {
                break;
            }
            let b = input[i + j];
            if b & 0xc0 != 0x80 {
                ok = false;
            } else {
                c = c << 6 | (b & 0x3f) as u32;
            }
        }
        if ok && ((k == 3 && c < 0x800) || (k == 4 && !(0x10000..=0x10ffff).contains(&c))) {
            ok = false;
        }
        if !ok {
            i += 1;
            put_utf8(out, &mut o, 0xfffd);
        } else {
            i += k;
            put_utf8(out, &mut o, c);
        }
    }
    out[o] = 0;
}

pub fn motif_decorated(v: &[u32]) -> i32 {
    if v.len() < 3 || v[0] & 2 == 0 {
        -1
    } else {
        i32::from(v[2] != 0)
    }
}
#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeHints {
    pub min_w: i32,
    pub min_h: i32,
    pub max_w: i32,
    pub max_h: i32,
}
pub fn size_hints(v: &[u32]) -> SizeHints {
    let mut h = SizeHints::default();
    if v.len() < 9 {
        return h;
    }
    if v[0] & 16 != 0 {
        h.min_w = v[5] as i32;
        h.min_h = v[6] as i32;
    }
    if v[0] & 32 != 0 {
        h.max_w = v[7] as i32;
        h.max_h = v[8] as i32;
    }
    if !(0..=16384).contains(&h.min_w) {
        h.min_w = 0;
    }
    if !(0..=16384).contains(&h.min_h) {
        h.min_h = 0;
    }
    if !(0..=32767).contains(&h.max_w) {
        h.max_w = 0;
    }
    if !(0..=32767).contains(&h.max_h) {
        h.max_h = 0;
    }
    h
}
pub fn hints_input(v: &[u32]) -> i32 {
    i32::from(v.len() < 2 || v[0] & 1 == 0 || v[1] != 0)
}
pub fn moveresize_edges(direction: u32) -> i32 {
    [5, 1, 9, 8, 10, 2, 6, 4, -1]
        .get(direction as usize)
        .copied()
        .unwrap_or(0)
}
pub fn text_rank(name: &[u8]) -> i32 {
    match name {
        b"UTF8_STRING" => 5,
        b"text/plain;charset=utf-8" => 4,
        b"STRING" => 3,
        b"text/plain" => 2,
        b"TEXT" => 1,
        _ => 0,
    }
}
