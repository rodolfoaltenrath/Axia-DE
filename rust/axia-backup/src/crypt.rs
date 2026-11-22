//! Optional encryption of backups, with RustCrypto (`axia-crypto`; docs/SYSTEM.md, "Encryption").
//!
//! Each file in the backup is:
//!   "LBK1" | base nonce (24 bytes) | 64 KiB chunks encrypted with XChaCha20-Poly1305 (+16 MAC)
//! Each chunk's nonce is the base nonce with the chunk number mixed into its last 8 bytes, and the
//! associated data is that number plus whether it is the last chunk: chunks cannot be reordered,
//! removed or truncated without failing verification. An empty file is a single empty final chunk.
//! The key comes from the passphrase with Argon2id (64 MiB, 3 passes) and a per-destination salt.
//! File names and symlink targets are NOT encrypted.
use axia_crypto as ac;
use std::os::fd::RawFd;
use std::path::Path;

pub const CHUNK: usize = 65536;
pub const HDR: usize = 28;
pub const MAC: usize = 16;
pub const KEY: usize = 32;
pub const SALT: usize = 16;
/// In the machine folder: the salt and the key check.
pub const FILE: &str = "encryption.json";

const KDF_BLOCKS: u32 = 65536; // 64 MiB
const KDF_PASSES: u32 = 3;
const MAGIC: [u8; 4] = *b"LBK1";

pub type Key = [u8; KEY];

pub fn wipe(bytes: &mut [u8]) {
    unsafe { ac::axia_crypto_wipe(bytes.as_mut_ptr(), bytes.len()) }
}

/// The size of the encrypted file for `plain` bytes of content.
pub fn enc_size(plain: u64) -> u64 {
    let chunks = if plain > 0 { plain.div_ceil(CHUNK as u64) } else { 1 };
    HDR as u64 + plain + chunks * MAC as u64
}

/// The content size of an encrypted file of `enc` bytes; None if no encrypted file can have it.
pub fn plain_size(enc: u64) -> Option<u64> {
    let (hdr, mac, chunk) = (HDR as u64, MAC as u64, CHUNK as u64);
    if enc < hdr + mac {
        return None;
    }
    let body = enc - hdr;
    let (full, rest) = (body / (chunk + mac), body % (chunk + mac));
    let plain = if rest == 0 {
        full * chunk // the last chunk is full
    } else if rest >= mac {
        full * chunk + (rest - mac)
    } else {
        return None;
    };
    (enc_size(plain) == enc).then_some(plain)
}

fn chunk_nonce(base: &[u8; 24], i: u64) -> [u8; 24] {
    let mut out = *base;
    for k in 0..8 {
        out[16 + k] ^= (i >> (8 * k)) as u8;
    }
    out
}

fn chunk_ad(i: u64, last: bool) -> [u8; 9] {
    let mut ad = [0u8; 9];
    for k in 0..8 {
        ad[k] = (i >> (8 * k)) as u8;
    }
    ad[8] = last as u8;
    ad
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Reads until `buf` is full or the end of the file; -1 on error.
fn read_full(fd: RawFd, buf: &mut [u8]) -> isize {
    let mut done = 0;
    while done < buf.len() {
        let k = unsafe { libc::read(fd, buf[done..].as_mut_ptr().cast(), buf.len() - done) };
        if k < 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return -1;
        }
        if k == 0 {
            break;
        }
        done += k as usize;
    }
    done as isize
}

/// 0 ok, -1 I/O, -2 out of space.
fn write_full(fd: RawFd, buf: &[u8]) -> i32 {
    let mut done = 0;
    while done < buf.len() {
        let k = unsafe { libc::write(fd, buf[done..].as_ptr().cast(), buf.len() - done) };
        if k < 0 && errno() == libc::EINTR {
            continue;
        }
        if k <= 0 {
            let e = errno();
            return if e == libc::ENOSPC || e == libc::EDQUOT { -2 } else { -1 };
        }
        done += k as usize;
    }
    0
}

pub fn random(buf: &mut [u8]) -> bool {
    let mut done = 0;
    while done < buf.len() {
        let k = unsafe { libc::getrandom(buf[done..].as_mut_ptr().cast(), buf.len() - done, 0) };
        if k < 0 && errno() == libc::EINTR {
            continue;
        }
        if k <= 0 {
            return false;
        }
        done += k as usize;
    }
    true
}

/// Encrypts `input` into `output`. `tick(bytes)` runs after each chunk: true stops.
/// 0 ok, -1 I/O, -2 out of space, 1 interrupted.
pub fn encrypt_fd(input: RawFd, output: RawFd, key: &Key, tick: &mut dyn FnMut(usize) -> bool) -> i32 {
    let mut hdr = [0u8; HDR];
    hdr[..4].copy_from_slice(&MAGIC);
    if !random(&mut hdr[4..]) {
        return -1;
    }
    let r = write_full(output, &hdr);
    if r != 0 {
        return r;
    }
    let base: [u8; 24] = hdr[4..].try_into().unwrap();
    let (mut a, mut b) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut c = vec![0u8; CHUNK + MAC];
    // read one chunk ahead to know which one is the last
    let mut n = read_full(input, &mut a);
    let mut i = 0u64;
    let mut r = 0;
    while n >= 0 {
        let len = n as usize;
        let next = if len == CHUNK { read_full(input, &mut b) } else { 0 };
        if next < 0 {
            r = -1;
            break;
        }
        let last = next == 0;
        let (nonce, ad) = (chunk_nonce(&base, i), chunk_ad(i, last));
        let sealed = unsafe {
            ac::axia_crypto_seal(c.as_mut_ptr(), c.as_mut_ptr().add(len), key.as_ptr(), nonce.as_ptr(), ad.as_ptr(), ad.len(), a.as_ptr(), len)
        };
        if sealed != 0 {
            r = -1;
            break;
        }
        r = write_full(output, &c[..len + MAC]);
        if r != 0 {
            break;
        }
        if tick(len) {
            r = 1;
            break;
        }
        if last {
            break;
        }
        std::mem::swap(&mut a, &mut b);
        n = next;
        i += 1;
    }
    if n < 0 {
        r = -1;
    }
    wipe(&mut a);
    wipe(&mut b);
    r
}

/// The inverse. As `encrypt_fd`, and -3 if a chunk fails verification (a tampered file, a
/// truncated one or the wrong key).
pub fn decrypt_fd(input: RawFd, output: RawFd, key: &Key, tick: &mut dyn FnMut(usize) -> bool) -> i32 {
    let mut hdr = [0u8; HDR];
    if read_full(input, &mut hdr) != HDR as isize || hdr[..4] != MAGIC {
        return -3;
    }
    let base: [u8; 24] = hdr[4..].try_into().unwrap();
    let (mut a, mut b) = (vec![0u8; CHUNK + MAC], vec![0u8; CHUNK + MAC]);
    let mut p = vec![0u8; CHUNK];
    let mut r: i32;
    let mut n = read_full(input, &mut a);
    let mut i = 0u64;
    loop {
        if n < MAC as isize {
            r = if n < 0 { -1 } else { -3 }; // truncated
            break;
        }
        let len = n as usize - MAC;
        let next = if n as usize == CHUNK + MAC { read_full(input, &mut b) } else { 0 };
        if next < 0 {
            r = -1;
            break;
        }
        let last = next == 0;
        let (nonce, ad) = (chunk_nonce(&base, i), chunk_ad(i, last));
        let opened = unsafe {
            ac::axia_crypto_open(p.as_mut_ptr(), a.as_ptr().add(len), key.as_ptr(), nonce.as_ptr(), ad.as_ptr(), ad.len(), a.as_ptr(), len)
        };
        if opened != 0 {
            r = -3;
            break;
        }
        r = write_full(output, &p[..len]);
        if r != 0 {
            break;
        }
        if tick(len) {
            r = 1;
            break;
        }
        if last {
            break;
        }
        std::mem::swap(&mut a, &mut b);
        n = next;
        i += 1;
    }
    wipe(&mut p);
    r
}

/// The key from the passphrase and the destination's salt (Argon2id; slow: about 0.3 s).
pub fn derive(pass: &[u8], salt: &[u8; SALT], key: &mut Key) -> bool {
    if pass.len() > u32::MAX as usize {
        return false;
    }
    let mut work = vec![0u8; ac::axia_crypto_argon_work_size(KDF_BLOCKS)];
    if work.is_empty() {
        return false;
    }
    let config = ac::ArgonConfig { algorithm: 2, memory_kib: KDF_BLOCKS, passes: KDF_PASSES, lanes: 1 };
    let inputs = ac::ArgonInputs { password: pass.as_ptr(), salt: salt.as_ptr(), password_len: pass.len() as u32, salt_len: SALT as u32 };
    let extras = ac::ArgonExtras { secret: std::ptr::null(), ad: std::ptr::null(), secret_len: 0, ad_len: 0 };
    let rc = unsafe { ac::axia_crypto_argon2(key.as_mut_ptr(), KEY, work.as_mut_ptr(), work.len(), config, inputs, extras) };
    wipe(&mut work);
    rc == 0
}

/// A value that shows the key is right without revealing it.
pub fn check(key: &Key) -> [u8; 16] {
    const LABEL: &[u8] = b"LordOS-Backups key check v1";
    let mut out = [0u8; 16];
    unsafe { ac::axia_crypto_blake2b(out.as_mut_ptr(), 16, key.as_ptr(), KEY, LABEL.as_ptr(), LABEL.len(), std::ptr::null(), 0) };
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &[u8], out: &mut [u8]) -> bool {
    if text.len() < out.len() * 2 {
        return false;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        let pair = &text[2 * i..2 * i + 2];
        let digit = |c: u8| (c as char).to_digit(16);
        match (digit(pair[0]), digit(pair[1])) {
            (Some(h), Some(l)) => *byte = (h * 16 + l) as u8,
            _ => return false,
        }
    }
    true
}

/// `<machine>/encryption.json`: the new salt and key check.
pub fn info_write(machine: &Path, salt: &[u8; SALT], check: &[u8; 16]) -> bool {
    use std::io::Write;
    let path = machine.join(FILE);
    let mut tmp = path.clone().into_os_string();
    tmp.push(".new");
    let tmp = std::path::PathBuf::from(tmp);
    let open = || std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp);
    let file = open().or_else(|_| {
        let _ = std::fs::remove_file(&tmp);
        open()
    });
    let Ok(mut file) = file else { return false };
    let text = format!(
        "{{\"v\":1,\"aead\":\"xchacha20-poly1305\",\"chunk\":{CHUNK},\"kdf\":\"argon2id\",\"blocks\":{KDF_BLOCKS},\"passes\":{KDF_PASSES},\"salt\":\"{}\",\"check\":\"{}\"}}\n",
        hex(salt),
        hex(check)
    );
    let ok = file.write_all(text.as_bytes()).is_ok() && file.sync_all().is_ok();
    drop(file);
    if !ok || std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// The salt and the key check, or None if the destination is not encrypted.
pub fn info_read(machine: &Path) -> Option<([u8; SALT], [u8; 16])> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(machine.join(FILE)).ok()?;
    let mut buf = [0u8; 511];
    let n = file.read(&mut buf).ok().filter(|n| *n > 0)?;
    let text = &buf[..n];
    let find = |needle: &[u8]| text.windows(needle.len()).position(|w| w == needle);
    let (s, c) = (find(b"\"salt\":\"")?, find(b"\"check\":\"")?);
    find(b"\"v\":1")?;
    let (mut salt, mut check) = ([0u8; SALT], [0u8; 16]);
    (unhex(&text[s + 8..], &mut salt) && unhex(&text[c + 9..], &mut check)).then_some((salt, check))
}

/// Derives the key and checks it against encryption.json: Ok(key), Err(-1) wrong passphrase,
/// Err(-2) the destination is not encrypted.
pub fn unlock(machine: &Path, pass: &[u8]) -> Result<Key, i32> {
    let (salt, expected) = info_read(machine).ok_or(-2)?;
    let mut key = [0u8; KEY];
    if !derive(pass, &salt, &mut key) {
        return Err(-1);
    }
    let got = check(&key);
    if unsafe { ac::axia_crypto_verify16(got.as_ptr(), expected.as_ptr()) } != 0 {
        wipe(&mut key);
        return Err(-1);
    }
    Ok(key)
}

/// Sets up encryption for a new destination: Ok(key), Err(-1) on failure.
pub fn setup(machine: &Path, pass: &[u8]) -> Result<Key, i32> {
    let mut salt = [0u8; SALT];
    let mut key = [0u8; KEY];
    if !random(&mut salt) || !derive(pass, &salt, &mut key) {
        return Err(-1);
    }
    if !info_write(machine, &salt, &check(&key)) {
        wipe(&mut key);
        return Err(-1);
    }
    Ok(key)
}
