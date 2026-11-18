//! Filesystem services shared by native applications.
pub mod operations;
pub mod policy;
pub mod trash;
pub type Result<T> = std::result::Result<T, String>;

pub fn io_error(e: std::io::Error) -> String {
    if e.raw_os_error() == Some(libc::ESTALE) {
        return "fs.sourceChanged".into();
    }
    let name = match e.raw_os_error().unwrap_or(libc::EIO) {
        libc::ENOENT => "ENOENT",
        libc::EACCES => "EACCES",
        libc::EPERM => "EPERM",
        libc::EEXIST => "EEXIST",
        libc::ENOSPC => "ENOSPC",
        libc::EROFS => "EROFS",
        libc::ENOTDIR => "ENOTDIR",
        libc::EISDIR => "EISDIR",
        libc::ENOTEMPTY => "ENOTEMPTY",
        libc::ELOOP => "ELOOP",
        libc::ECANCELED => "ECANCELED",
        libc::EXDEV => "EXDEV",
        libc::EINVAL => "EINVAL",
        libc::ENAMETOOLONG => "ENAMETOOLONG",
        _ => "EIO",
    };
    format!("sys.errno errno={name}")
}

pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natural_lowercase(a.to_lowercase().as_bytes(), b.to_lowercase().as_bytes())
}
// Both already lowercase. UTF-8 sorts bytewise in character order, and no byte
// of a multi-byte character is an ASCII digit.
pub fn natural_lowercase(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let run = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
    let (mut a, mut b) = (a, b);
    loop {
        match (a.first(), b.first()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, rest_a) = a.split_at(run(a));
                let (y, rest_b) = b.split_at(run(b));
                let strip = |s: &'_ [u8]| s.iter().take_while(|c| **c == b'0').count();
                let (x, y) = (&x[strip(x)..], &y[strip(y)..]);
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
                (a, b) = (rest_a, rest_b);
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                (a, b) = (&a[1..], &b[1..]);
            }
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
        }
    }
}
