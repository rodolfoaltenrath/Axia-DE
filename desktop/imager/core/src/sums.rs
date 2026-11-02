//! The release signature and SHA256SUMS (docs/SIGNING.md, "Release files").
use crate::{Error, Result};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::BTreeMap;

pub const SUMS_MAX: usize = 65536;
pub const SIG_MAX: usize = 512;

/// Checks `sig` (base64, optional newline) over the exact bytes of `sums` with `key`.
pub fn verify(sums: &[u8], sig: &[u8], key: &[u8; 32]) -> Result<()> {
    if sums.is_empty() || sums.len() > SUMS_MAX || sig.len() > SIG_MAX {
        return Err(Error::new("imager.badSignature", "size"));
    }
    let text = std::str::from_utf8(sig).map_err(|_| Error::new("imager.badSignature", "not text"))?;
    let b = text.trim_end_matches(['\n', '\r']);
    if b.len() != 88 { return Err(Error::new("imager.badSignature", "length")); }
    let raw = base64::engine::general_purpose::STANDARD.decode(b).map_err(|_| Error::new("imager.badSignature", "base64"))?;
    let raw: [u8; 64] = raw.try_into().map_err(|_| Error::new("imager.badSignature", "length"))?;
    let vk = VerifyingKey::from_bytes(key).map_err(|_| Error::new("imager.badSignature", "key"))?;
    vk.verify_strict(sums, &Signature::from_bytes(&raw)).map_err(|_| Error::new("imager.badSignature", ""))
}

/// Every "<64 hex>  <name>" (or " *<name>") line. Malformed text or a name listed twice is an error.
pub fn parse(sums: &[u8]) -> Result<BTreeMap<String, [u8; 32]>> {
    let text = std::str::from_utf8(sums).map_err(|_| Error::new("imager.notListed", "not text"))?;
    let mut out = BTreeMap::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() { continue; }
        let b = line.as_bytes();
        if b.len() < 67 || b[64] != b' ' || (b[65] != b' ' && b[65] != b'*') { return Err(Error::new("imager.notListed", "malformed")); }
        let mut h = [0u8; 32];
        for (i, byte) in h.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&line[2 * i..2 * i + 2], 16).map_err(|_| Error::new("imager.notListed", "hex"))?;
        }
        let name = &line[66..];
        if out.insert(name.to_string(), h).is_some() { return Err(Error::new("imager.notListed", "repeated")); }
    }
    Ok(out)
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
