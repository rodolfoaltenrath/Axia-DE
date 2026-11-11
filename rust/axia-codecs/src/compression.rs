use std::io::Read;

/// Incremental, bounded decoding: a dishonest ISIZE never controls allocation.
/// GzDecoder verifies the header, CRC32 and uncompressed length. Reject tails
/// and concatenated members so callers cannot silently ignore hidden payloads.
pub(crate) fn decode(data: &[u8], max: usize, gzip: bool) -> Option<Vec<u8>> {
    fn read(mut decoder: impl Read, max: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            // Probe one byte beyond the limit, without reserving an oversized buffer.
            let available = max.saturating_sub(out.len()).saturating_add(1).min(chunk.len());
            let n = decoder.read(&mut chunk[..available]).ok()?;
            if n == 0 { return Some(out); }
            if n > max - out.len() { return None; }
            out.try_reserve_exact(n).ok()?;
            out.extend_from_slice(&chunk[..n]);
        }
    }
    if gzip {
        let mut decoder = flate2::bufread::GzDecoder::new(data);
        let out = read(&mut decoder, max)?;
        decoder.into_inner().is_empty().then_some(out)
    } else {
        let mut state = flate2::Decompress::new(false);
        let mut scratch = [0; 8192];
        let mut out = Vec::new();
        loop {
            let before_in = state.total_in();
            let before_out = state.total_out();
            let status = state.decompress(&data[before_in as usize..], &mut scratch, flate2::FlushDecompress::None).ok()?;
            let n = (state.total_out() - before_out) as usize;
            if n > max - out.len() { return None; }
            out.try_reserve_exact(n).ok()?;
            out.extend_from_slice(&scratch[..n]);
            if status == flate2::Status::StreamEnd { return (state.total_in() as usize == data.len()).then_some(out); }
            if before_in == state.total_in() && n == 0 { return None; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn compressed(data: &[u8], gzip: bool) -> Vec<u8> {
        if gzip {
            let mut w = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(data).unwrap(); w.finish().unwrap()
        } else {
            let mut w = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(data).unwrap(); w.finish().unwrap()
        }
    }
    #[test]
    fn bounded_roundtrip_including_empty() {
        for gzip in [false, true] { for data in [vec![], vec![42; 200_000]] {
            let packed = compressed(&data, gzip);
            assert!(decode(&packed, data.len(), gzip).as_deref() == Some(data.as_slice()), "gzip={gzip}, len={}", data.len());
            if !data.is_empty() { assert!(decode(&packed, data.len()-1, gzip).is_none()); }
        }}
    }
    #[test]
    fn rejects_truncation_and_trailing_payload() {
        for gzip in [false, true] {
            let packed = compressed(b"secret contents", gzip);
            for end in 0..packed.len() { assert!(decode(&packed[..end], 4096, gzip).is_none(), "gzip={gzip}, end={end}"); }
            let mut tailed = packed; tailed.push(0);
            assert!(decode(&tailed, 4096, gzip).is_none());
        }
    }
    #[test]
    fn rejects_bad_gzip_checksum_and_length() {
        let packed = compressed(b"sensitive record", true);
        for offset in [4, 8] {
            let mut bad = packed.clone(); let n = bad.len(); bad[n-offset] ^= 1;
            assert!(decode(&bad, 4096, true).is_none());
        }
    }
}
