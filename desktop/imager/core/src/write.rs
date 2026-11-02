//! Image → device, verified. The same steps as the native imager-write action
//! (server/src/imager/imager_admin.c), in the same order:
//!
//! 1. official images: the whole file's SHA-256 must be the signed one before anything is written;
//! 2. the file is read once more while writing (.img.xz decompressed on the fly) and hashed again:
//!    if it changed in between, the device is wiped and the write fails;
//! 3. the device is read back and its SHA-256 compared with what was written;
//! 4. on any failure or a cancel after the first write, the first and last MiB are zeroed, so a
//!    half-written card never looks bootable.
//!
//! Writes and reads are done in 4 MiB pieces, each padded to the sector size at the end, so raw
//! devices that only take whole sectors (`macos` /dev/rdiskN, `windows` PhysicalDriveN) work.
use crate::{sums::hex, Error, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub const CHUNK: usize = 4 << 20;
pub const SECTOR: usize = 512;
const MIB: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase { Check, Write, Verify }

#[derive(Debug, Clone, serde::Serialize)]
pub struct Progress { pub phase: Phase, pub done: u64, pub total: u64 }

#[derive(Debug, Clone, serde::Serialize)]
pub struct Outcome { pub sha256: String, pub size: u64, pub ms: u64, pub mbps: f64 }

#[derive(Debug, Clone)]
pub struct Image {
    pub path: std::path::PathBuf,
    pub xz: bool,
    pub file_size: u64,
    pub raw_size: u64,
}

/// What the image will be once written (the .xz index gives the uncompressed size).
pub fn inspect(path: &Path) -> Result<Image> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let kind = crate::image_kind(name);
    if kind == 0 || !crate::valid_name(name) { return Err(Error::new("imager.badImageName", name)); }
    let mut f = File::open(path).map_err(|e| Error::new("disk.imageOpenFailed", e.to_string()))?;
    let meta = f.metadata().map_err(|e| Error::new("disk.imageOpenFailed", e.to_string()))?;
    if !meta.is_file() || meta.len() == 0 { return Err(Error::new("disk.imageOpenFailed", "not a regular file")); }
    let raw = if kind == 2 { crate::xzinfo::uncompressed_size(&mut f, meta.len()).ok_or_else(|| Error::new("imager.xzDamaged", ""))? } else { meta.len() };
    Ok(Image { path: path.to_path_buf(), xz: kind == 2, file_size: meta.len(), raw_size: raw })
}

pub fn sha256_file(path: &Path, cancel: &AtomicBool, mut cb: impl FnMut(Progress)) -> Result<[u8; 32]> {
    let mut f = File::open(path).map_err(|e| Error::new("disk.imageOpenFailed", e.to_string()))?;
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(Error::new("disk.cancelled", "")); }
        let n = f.read(&mut buf).map_err(|e| Error::new("disk.imageReadFailed", e.to_string()))?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        done += n as u64;
        cb(Progress { phase: Phase::Check, done, total });
    }
    Ok(h.finalize().into())
}

/// Hashes everything read through it (the compressed bytes, for the second check).
struct Hashing<R> { inner: R, h: Sha256, n: u64 }
impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.h.update(&buf[..k]);
        self.n += k as u64;
        Ok(k)
    }
}

fn fill(r: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut o = 0;
    while o < buf.len() {
        match r.read(&mut buf[o..]) {
            Ok(0) => break,
            Ok(k) => o += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(o)
}

fn put<D: Write + Seek>(dev: &mut D, off: u64, data: &[u8]) -> std::io::Result<()> {
    dev.seek(SeekFrom::Start(off))?;
    dev.write_all(data)
}

/// Zeroes the first and last MiB of the device (partition tables, GPT backup, boot sectors).
pub fn wipe_ends<D: Write + Seek>(dev: &mut D, dev_size: u64) -> std::io::Result<()> {
    let z = vec![0u8; MIB as usize];
    put(dev, 0, &z[..MIB.min(dev_size) as usize])?;
    if dev_size > 2 * MIB { put(dev, dev_size - MIB, &z)?; }
    dev.flush()
}

pub trait Device: Read + Write + Seek {
    fn sync(&mut self) -> std::io::Result<()>;
}
impl Device for File {
    /// Raw disk devices are unbuffered: some refuse a full sync (`macos` /dev/rdiskN answers
    /// ENOTTY to F_FULLFSYNC), which is not an error there.
    fn sync(&mut self) -> std::io::Result<()> {
        match self.sync_all() {
            Err(e) if matches!(e.raw_os_error(), Some(25) | Some(45) | Some(102) | Some(22)) => Ok(()),   // ENOTTY, ENOTSUP, EOPNOTSUPP, EINVAL
            r => r,
        }
    }
}

/// Writes `img` to `dev` (`dev_size` bytes) and reads it back. `listed`: the signed SHA-256 of the
/// image file (official images), checked again over the bytes actually read while writing.
pub fn write_image<D: Device>(img: &Image, dev: &mut D, dev_size: u64, listed: Option<[u8; 32]>, cancel: &AtomicBool,
                              mut cb: impl FnMut(Progress)) -> Result<Outcome> {
    if img.raw_size > dev_size {
        return Err(Error::new("disk.imageTooLarge", format!("{} > {}", img.raw_size, dev_size)));
    }
    let t0 = std::time::Instant::now();
    let r = (|| -> Result<[u8; 32]> {
        let f = File::open(&img.path).map_err(|e| Error::new("disk.imageOpenFailed", e.to_string()))?;
        let mut hashing = Hashing { inner: f, h: Sha256::new(), n: 0 };
        let mut raw = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut off = 0u64;
        {
            let mut src: Box<dyn Read + '_> = if img.xz { Box::new(xz2::read::XzDecoder::new_multi_decoder(&mut hashing)) } else { Box::new(&mut hashing) };
            loop {
                if cancel.load(Ordering::Relaxed) { return Err(Error::new("imager.cancelledWiped", "")); }
                let n = fill(&mut src, &mut buf).map_err(|e| Error::new(if img.xz { "imager.xzDamaged" } else { "disk.imageReadFailed" }, e.to_string()))?;
                if n == 0 { break; }
                if off + n as u64 > img.raw_size { return Err(Error::new(if img.xz { "imager.xzDamaged" } else { "imager.imageChanged" }, "longer than its size")); }
                raw.update(&buf[..n]);
                let padded = n.div_ceil(SECTOR) * SECTOR;
                buf[n..padded].fill(0);
                put(dev, off, &buf[..padded]).map_err(|e| Error::new("disk.writeFailed", e.to_string()))?;
                off += n as u64;
                cb(Progress { phase: Phase::Write, done: off, total: img.raw_size });
            }
        }
        if off != img.raw_size { return Err(Error::new(if img.xz { "imager.xzDamaged" } else { "imager.imageChanged" }, "shorter than its size")); }
        if hashing.n != img.file_size { return Err(Error::new("imager.imageChanged", "file size changed")); }
        if let Some(want) = listed {
            let got: [u8; 32] = hashing.h.finalize().into();
            if got != want { return Err(Error::new("imager.imageChanged", "the file changed after it was checked")); }
        }
        dev.flush().map_err(|e| Error::new("disk.writeFailed", e.to_string()))?;
        dev.sync().map_err(|e| Error::new("disk.writeFailed", e.to_string()))?;
        Ok(raw.finalize().into())
    })();
    let written = match r {
        Ok(h) => h,
        Err(e) => { let _ = wipe_ends(dev, dev_size); let _ = dev.sync(); return Err(e); }
    };
    let ms = t0.elapsed().as_millis() as u64;
    // read back
    let v = (|| -> Result<[u8; 32]> {
        let mut h = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut off = 0u64;
        while off < img.raw_size {
            if cancel.load(Ordering::Relaxed) { return Err(Error::new("imager.cancelledWiped", "")); }
            let want = (img.raw_size - off).min(CHUNK as u64) as usize;
            let rd = want.div_ceil(SECTOR) * SECTOR;
            dev.seek(SeekFrom::Start(off)).map_err(|e| Error::new("disk.verifyReadFailed", e.to_string()))?;
            dev.read_exact(&mut buf[..rd]).map_err(|e| Error::new("disk.verifyReadFailed", e.to_string()))?;
            h.update(&buf[..want]);
            off += want as u64;
            cb(Progress { phase: Phase::Verify, done: off, total: img.raw_size });
        }
        Ok(h.finalize().into())
    })();
    match v {
        Ok(back) if back == written => {
            let secs = (ms as f64 / 1000.0).max(0.001);
            Ok(Outcome { sha256: hex(&written), size: img.raw_size, ms, mbps: img.raw_size as f64 / secs / 1e6 })
        }
        other => {
            let _ = wipe_ends(dev, dev_size);
            let _ = dev.sync();
            Err(match other { Err(e) => e, _ => Error::new("disk.verifyMismatch", "") })
        }
    }
}
