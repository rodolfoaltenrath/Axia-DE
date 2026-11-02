//! The uncompressed size of an .xz file from its stream footers and indexes (CRC-checked), like
//! server/src/imager/imgverify.c's img_xz_size: one or more streams, stream padding allowed.
use std::io::{Read, Seek, SeekFrom};

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 { crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 }; }
    }
    !crc
}

fn le32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }

fn varint(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..9 {
        let c = *b.get(*pos)?;
        *pos += 1;
        v |= ((c & 0x7f) as u64) << (7 * i);
        if c & 0x80 == 0 { return if i > 0 && c == 0 { None } else { Some(v) }; }
    }
    None
}

fn read_at<R: Read + Seek>(r: &mut R, off: u64, n: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; n];
    r.seek(SeekFrom::Start(off)).ok()?;
    r.read_exact(&mut b).ok()?;
    Some(b)
}

/// None when the file isn't xz or is damaged.
pub fn uncompressed_size<R: Read + Seek>(r: &mut R, fsize: u64) -> Option<u64> {
    if fsize < 32 || fsize % 4 != 0 { return None; }
    let (mut pos, mut total, mut streams) = (fsize, 0u64, 0);
    while pos > 0 {
        if pos < 12 { return None; }
        let f = read_at(r, pos - 12, 12)?;
        if f[8..12] == [0, 0, 0, 0] { pos -= 4; continue; }          // stream padding
        if &f[10..12] != b"YZ" || crc32(&f[4..10]) != le32(&f[0..4]) { return None; }
        let isize = (le32(&f[4..8]) as u64 + 1) * 4;
        if isize > 1 << 24 || isize + 24 > pos { return None; }
        let ix = read_at(r, pos - 12 - isize, isize as usize)?;
        let end = ix.len() - 4;
        if ix[0] != 0 || crc32(&ix[..end]) != le32(&ix[end..]) { return None; }
        let mut p = 1usize;
        let n = varint(&ix[..end], &mut p)?;
        if n > 1 << 24 { return None; }
        let (mut blocks, mut usum) = (0u64, 0u64);
        for _ in 0..n {
            let un = varint(&ix[..end], &mut p)?;
            let us = varint(&ix[..end], &mut p)?;
            if un == 0 { return None; }
            blocks = blocks.checked_add((un + 3) & !3)?;
            usum = usum.checked_add(us)?;
        }
        while p % 4 != 0 { if ix[p] != 0 { return None; } p += 1; }
        if p != end { return None; }
        let ssize = 12 + blocks + isize + 12;
        if ssize > pos { return None; }
        let h = read_at(r, pos - ssize, 12)?;
        if &h[0..6] != b"\xfd7zXZ\0" || h[6] != f[8] || h[7] != f[9] || crc32(&h[6..8]) != le32(&h[8..12]) { return None; }
        total = total.checked_add(usum)?;
        pos -= ssize;
        streams += 1;
        if streams > 1024 { return None; }
    }
    if streams == 0 { None } else { Some(total) }
}
