//! User-owned, immutable wallpaper imports. No privileged helper or distro service.
use std::{
    ffi::{c_char, CStr},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    ptr,
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
fn dimensions(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    let value = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        if data.len() < 33 || data[8..12] != 13u32.to_be_bytes() || &data[12..16] != b"IHDR" {
            return None;
        }
        (
            u32::from_be_bytes(data[16..20].try_into().ok()?),
            u32::from_be_bytes(data[20..24].try_into().ok()?),
            "png",
        )
    } else if data.starts_with(&[0xff, 0xd8]) {
        let mut p = 2;
        let mut found = None;
        while p < data.len() {
            if data[p] != 0xff {
                return None;
            }
            while p < data.len() && data[p] == 0xff {
                p += 1;
            }
            let marker = *data.get(p)?;
            p += 1;
            if marker == 0xda || marker == 0xd9 {
                break;
            }
            if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                continue;
            }
            let n = u16::from_be_bytes(data.get(p..p + 2)?.try_into().ok()?) as usize;
            if n < 2 || p.checked_add(n)? > data.len() {
                return None;
            }
            if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
                if n < 8 {
                    return None;
                }
                found = Some((
                    u16::from_be_bytes([data[p + 5], data[p + 6]]) as u32,
                    u16::from_be_bytes([data[p + 3], data[p + 4]]) as u32,
                    "jpg",
                ));
                break;
            }
            p += n;
        }
        found?
    } else {
        return None;
    };
    let (w, h, _) = value;
    (w > 0 && h > 0 && w <= 8192 && h <= 8192 && u64::from(w) * u64::from(h) <= MAX_PIXELS)
        .then_some(value)
}
fn import(source: &Path, root: &Path) -> Result<PathBuf, &'static str> {
    if !source.is_absolute() || !root.is_absolute() {
        return Err("path");
    }
    // O_NONBLOCK prevents opening a named pipe/device from stalling the compositor.
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(0x800)
        .open(source)
        .map_err(|_| "read")?;
    let m = f.metadata().map_err(|_| "read")?;
    if !m.is_file() || m.len() > MAX_BYTES {
        return Err("size");
    }
    let mut data = Vec::new();
    f.take(MAX_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|_| "read")?;
    if data.len() as u64 > MAX_BYTES {
        return Err("size");
    }
    let (_, _, ext) = dimensions(&data).ok_or("image")?;
    let hash = data.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let dir = root.join("axia/wallpapers");
    fs::create_dir_all(&dir).map_err(|_| "write")?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|_| "write")?;
    let path = dir.join(format!("{hash:016x}-{}.{ext}", data.len()));
    if path.exists() {
        // Hashes name files, never establish equality; check bytes on a deduplicated import.
        let mut prior = Vec::new();
        File::open(&path)
            .map_err(|_| "read")?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut prior)
            .map_err(|_| "read")?;
        return if prior == data {
            Ok(path)
        } else {
            Err("collision")
        };
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "write")?
        .as_nanos();
    let tmp = dir.join(format!(".import-{}-{stamp}", std::process::id()));
    let result = (|| {
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| "write")?;
        out.write_all(&data)
            .and_then(|_| out.sync_all())
            .map_err(|_| "write")?;
        fs::rename(&tmp, &path).map_err(|_| "write")?;
        Ok(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
// All strings are UTF-8 NUL-terminated. Output is caller-owned and remains unchanged on failure.
// home/data are supplied by the session bridge, not by untrusted app arguments.
#[no_mangle]
pub unsafe extern "C" fn axia_wallpaper_import(
    source: *const c_char,
    home: *const c_char,
    data: *const c_char,
    out: *mut c_char,
    len: usize,
) -> bool {
    if source.is_null() || home.is_null() || out.is_null() {
        return false;
    }
    let Ok(source) = CStr::from_ptr(source).to_str() else {
        return false;
    };
    let Ok(home) = CStr::from_ptr(home).to_str() else {
        return false;
    };
    let root = if data.is_null() {
        PathBuf::from(home).join(".local/share")
    } else {
        let Ok(data) = CStr::from_ptr(data).to_str() else {
            return false;
        };
        if Path::new(data).is_absolute() {
            PathBuf::from(data)
        } else {
            PathBuf::from(home).join(".local/share")
        }
    };
    let Ok(path) = import(Path::new(source), &root) else {
        return false;
    };
    let Some(path) = path.to_str() else {
        return false;
    };
    if path.len() + 1 > len {
        return false;
    }
    ptr::copy_nonoverlapping(path.as_ptr(), out.cast(), path.len());
    *out.add(path.len()) = 0;
    true
}

// The existing C image codec encodes PNG; dimensions, resampling and file lifecycle
// belong to Rust. The original never stays decoded just to display a gallery thumbnail.
type EncodePng = unsafe extern "C" fn(*const c_char, i32, i32, *const u8) -> i32;
#[no_mangle]
pub unsafe extern "C" fn axia_wallpaper_preview(
    path: *const c_char,
    rgba: *const u8,
    w: usize,
    h: usize,
    encode: Option<EncodePng>,
    out: *mut c_char,
    len: usize,
) -> bool {
    if path.is_null()
        || rgba.is_null()
        || out.is_null()
        || w == 0
        || h == 0
        || w > 8192
        || h > 8192
        || w * h > MAX_PIXELS as usize
    {
        return false;
    }
    let Some(encode) = encode else {
        return false;
    };
    let Ok(path) = CStr::from_ptr(path).to_str() else {
        return false;
    };
    let final_path = format!("{path}.thumb.png");
    if final_path.len() + 1 > len {
        return false;
    }
    let scale = (320.0 / w as f64).min(180.0 / h as f64).min(1.0);
    let tw = (w as f64 * scale).round().max(1.0) as usize;
    let th = (h as f64 * scale).round().max(1.0) as usize;
    let input = std::slice::from_raw_parts(rgba, w * h * 4);
    let mut pixels = vec![0u8; tw * th * 4];
    for y in 0..th {
        for x in 0..tw {
            let (x0, x1, y0, y1) = (
                x * w / tw,
                ((x + 1) * w / tw).max(x * w / tw + 1),
                y * h / th,
                ((y + 1) * h / th).max(y * h / th + 1),
            );
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    for c in 0..4 {
                        sum[c] += u64::from(input[(sy * w + sx) * 4 + c]);
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u64;
            for c in 0..4 {
                pixels[(y * tw + x) * 4 + c] = (sum[c] / n) as u8;
            }
        }
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp = format!("{final_path}.{}.{}.tmp", std::process::id(), stamp);
    let Ok(cpath) = std::ffi::CString::new(temp.as_str()) else {
        return false;
    };
    let ok = encode(cpath.as_ptr(), tw as i32, th as i32, pixels.as_ptr()) != 0
        && fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).is_ok()
        && fs::rename(&temp, &final_path).is_ok();
    if !ok {
        let _ = fs::remove_file(temp);
        return false;
    }
    ptr::copy_nonoverlapping(final_path.as_ptr(), out.cast(), final_path.len());
    *out.add(final_path.len()) = 0;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        p.extend(w.to_be_bytes());
        p.extend(h.to_be_bytes());
        p.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
        p
    }
    #[test]
    fn validates_dimensions_and_format() {
        assert_eq!(dimensions(&png(1920, 1080)), Some((1920, 1080, "png")));
        assert_eq!(dimensions(&png(8192, 8192)), None);
        assert_eq!(dimensions(&png(0, 12)), None);
        assert_eq!(dimensions(b"not an image"), None);
        for n in 0..33 {
            assert_eq!(dimensions(&png(12, 12)[..n]), None);
        }
        let jpeg = [255, 216, 255, 192, 0, 11, 8, 0, 100, 0, 200, 1, 1, 17, 0];
        assert_eq!(dimensions(&jpeg), Some((200, 100, "jpg")));
        for n in 0..jpeg.len() {
            assert_eq!(dimensions(&jpeg[..n]), None);
        }
    }
    #[test]
    fn import_is_private_deduplicated_and_independent_of_original() {
        let root = std::env::temp_dir().join(format!(
            "axia-wallpaper-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("a picture.png");
        fs::write(&source, png(10, 10)).unwrap();
        let path = import(&source, &root).unwrap();
        assert_eq!(path, import(&source, &root).unwrap());
        fs::remove_file(&source).unwrap();
        assert!(path.is_file());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&source, b"garbage").unwrap();
        assert!(import(&source, &root).is_err());
        assert!(import(Path::new("relative"), &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
