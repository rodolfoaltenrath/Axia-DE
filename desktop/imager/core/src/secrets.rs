//! The two secrets of the first-run file, derived on this computer so only the results reach the
//! card: the password's SHA-512 crypt hash ($6$, accepted by lordos-firstrun and by every Linux
//! PAM) and the WPA2 key PBKDF2-HMAC-SHA1(password, ssid, 4096, 32) (IEEE 802.11i).
use crate::{Error, Result};

/// rounds: 200 000 keeps a login on the Raspberry Pi 5 well under a second while making offline
/// guessing 40 times slower than glibc's default of 5 000.
pub const ROUNDS: usize = 200_000;

pub fn hash_password(password: &str) -> Result<String> {
    if password.is_empty() || password.len() > 1024 || password.contains('\0') {
        return Err(Error::new("imager.hashFailed", "length"));
    }
    let params = sha_crypt::Sha512Params::new(ROUNDS).map_err(|_| Error::new("imager.hashFailed", "params"))?;
    sha_crypt::sha512_simple(password, &params).map_err(|_| Error::new("imager.hashFailed", ""))
}

pub fn check_password(password: &str, hash: &str) -> bool { sha_crypt::sha512_check(password, hash).is_ok() }

pub fn wifi_psk(ssid: &str, password: &str) -> Result<String> {
    if ssid.is_empty() || ssid.len() > 32 { return Err(Error::new("imager.pskFailed", "ssid")); }
    if password.len() < 8 || password.len() > 63 || !password.bytes().all(|c| (0x20..=0x7e).contains(&c)) {
        return Err(Error::new("imager.pskFailed", "password"));
    }
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), ssid.as_bytes(), 4096, &mut out);
    Ok(crate::sums::hex(&out))
}
