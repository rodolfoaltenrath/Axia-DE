//! Bounds-checked MaxMind DB and Natural Earth outlines; no network lookups.
use serde_json::{json, Value};
use std::net::IpAddr;
pub struct Database {
    bytes: Vec<u8>,
    nodes: u32,
    bits: usize,
    version: u64,
    data: usize,
    end: usize,
    v4: u32,
}
fn uint(b: &[u8]) -> Option<u64> {
    if b.len() > 8 {
        return None;
    }
    Some(b.iter().fold(0, |n, b| (n << 8) | *b as u64))
}
fn decode(b: &[u8], offset: &mut usize, depth: u8, pointer: bool) -> Option<Value> {
    if depth > 24 {
        return None;
    }
    let c = *b.get(*offset)?;
    *offset += 1;
    let mut kind = c >> 5;
    let mut n = (c & 31) as usize;
    if kind == 1 {
        if !pointer {
            return None;
        }
        let count = ((c >> 3) & 3) as usize + 1;
        let tail = b.get(*offset..offset.checked_add(count)?)?;
        *offset += count;
        let prefix = if count == 4 { 0 } else { (c & 7) as u64 };
        let base = match count {
            2 => 2048,
            3 => 526336,
            _ => 0,
        };
        let target = ((prefix << (count * 8)) | uint(tail)?).checked_add(base)? as usize;
        return decode(b, &mut target.clone(), depth + 1, false);
    }
    if kind == 0 {
        kind = 7 + *b.get(*offset)?;
        *offset += 1;
    }
    if n >= 29 {
        let count = n - 28;
        let size = uint(b.get(*offset..offset.checked_add(count)?)?)? as usize;
        *offset += count;
        n = match n {
            29 => 29,
            30 => 285,
            _ => 65821,
        } + size;
    }
    match kind {
        7 => {
            if n > 4096 {
                return None;
            }
            let mut map = serde_json::Map::new();
            for _ in 0..n {
                let k = decode(b, offset, depth + 1, true)?.as_str()?.to_owned();
                let v = decode(b, offset, depth + 1, true)?;
                map.insert(k, v);
            }
            Some(Value::Object(map))
        }
        11 => {
            if n > 4096 {
                return None;
            }
            let mut v = Vec::new();
            for _ in 0..n {
                v.push(decode(b, offset, depth + 1, true)?)
            }
            Some(json!(v))
        }
        14 => Some(json!(n != 0)),
        _ => {
            let bytes = b.get(*offset..offset.checked_add(n)?)?;
            *offset += n;
            match kind {
                2 => Some(json!(std::str::from_utf8(bytes).ok()?)),
                5 | 6 | 8 | 9 | 10 => Some(json!(uint(bytes)?)),
                3 if n == 8 => {
                    let v = f64::from_be_bytes(bytes.try_into().ok()?);
                    v.is_finite().then(|| json!(v))
                }
                4 => Some(Value::Null),
                _ => None,
            }
        }
    }
}
impl Database {
    pub fn parse(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() > 64 << 20 {
            return None;
        }
        let mark = b"\xab\xcd\xefMaxMind.com";
        let end = bytes.windows(mark.len()).rposition(|b| b == mark)?;
        let meta = decode(bytes.get(end + mark.len()..)?, &mut 0, 0, true)?;
        let nodes = u32::try_from(meta["node_count"].as_u64()?).ok()?;
        let bits = meta["record_size"].as_u64()? as usize;
        let version = meta["ip_version"].as_u64()?;
        if nodes == 0 || !matches!(bits, 24 | 28 | 32) || !matches!(version, 4 | 6) {
            return None;
        }
        let data = (nodes as usize).checked_mul(bits / 4)?.checked_add(16)?;
        if data > end {
            return None;
        }
        let mut db = Self {
            bytes,
            nodes,
            bits,
            version,
            data,
            end,
            v4: 0,
        };
        if version == 6 {
            for _ in 0..96 {
                if db.v4 >= nodes {
                    break;
                }
                db.v4 = db.record(db.v4, 0)?;
            }
        }
        Some(db)
    }
    fn record(&self, node: u32, bit: usize) -> Option<u32> {
        if node >= self.nodes {
            return None;
        }
        let pos = node as usize * (self.bits / 4);
        let b = self.bytes.get(pos..pos + self.bits / 4)?;
        Some(match self.bits {
            24 => uint(b.get(bit * 3..bit * 3 + 3)?)? as u32,
            28 => {
                if bit == 0 {
                    ((b[3] & 240) as u32) << 20 | uint(&b[..3])? as u32
                } else {
                    ((b[3] & 15) as u32) << 24 | uint(&b[4..7])? as u32
                }
            }
            32 => uint(b.get(bit * 4..bit * 4 + 4)?)? as u32,
            _ => return None,
        })
    }
    pub fn country(&self, addr: IpAddr) -> Option<String> {
        let (bytes, bits, mut node) = match addr {
            IpAddr::V4(a) => (
                a.octets().to_vec(),
                32,
                if self.version == 6 { self.v4 } else { 0 },
            ),
            IpAddr::V6(a) => {
                if self.version == 4 {
                    return None;
                }
                (a.octets().to_vec(), 128, 0)
            }
        };
        for i in 0..bits {
            if node >= self.nodes {
                break;
            }
            node = self.record(node, ((bytes[i / 8] >> (7 - i % 8)) & 1) as usize)?;
        }
        let mut offset = node.checked_sub(self.nodes)?.checked_sub(16)? as usize;
        let obj = decode(self.bytes.get(self.data..self.end)?, &mut offset, 0, true)?;
        let cc = obj["country"]["iso_code"].as_str()?;
        if cc.len() == 2 && cc.bytes().all(|b| b.is_ascii_uppercase()) {
            Some(cc.into())
        } else {
            None
        }
    }
}
pub fn outlines(bytes: &[u8]) -> Option<Vec<Vec<(f64, f64)>>> {
    if bytes.len() > 8 << 20 || bytes.get(..8)? != b"LORDMAP1" {
        return None;
    }
    let u32at = |at| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let n = u32at(8)?;
    if n > 4096 {
        return None;
    }
    let mut pos = 12;
    let mut rings = Vec::new();
    for _ in 0..n {
        let head = bytes.get(pos..pos + 12)?;
        let count = u16::from_le_bytes(head[10..12].try_into().ok()?);
        pos += 12;
        for _ in 0..count {
            let points = u32at(pos)? as usize;
            pos += 4;
            let data = bytes.get(pos..pos.checked_add(points.checked_mul(4)?)?)?;
            pos += data.len();
            let ring = data
                .chunks_exact(4)
                .map(|p| {
                    (
                        i16::from_le_bytes([p[0], p[1]]) as f64 / 100.,
                        i16::from_le_bytes([p[2], p[3]]) as f64 / 100.,
                    )
                })
                .collect();
            rings.push(ring);
        }
    }
    Some(rings)
}
