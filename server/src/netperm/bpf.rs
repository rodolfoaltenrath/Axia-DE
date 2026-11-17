//! Minimal Linux BPF syscall interface. File descriptors and mappings own their lifetimes.
use crate::shared::*;
use std::{
    collections::HashMap,
    ffi::CString,
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
#[derive(Clone, Debug)]
pub struct Program {
    pub section: &'static str,
    pub name: &'static str,
    pub insns: &'static [u64],
    pub relocs: &'static [(usize, &'static str)],
}
#[cfg(feature = "bpf")]
include!(concat!(env!("OUT_DIR"), "/programs.rs"));
#[cfg(not(feature = "bpf"))]
pub static PROGRAMS: &[Program] = &[];
#[derive(Clone, Copy)]
pub struct Spec {
    pub section: &'static str,
    pub ty: u32,
    pub attach: u32,
    pub btf: Option<&'static str>,
    pub optional: bool,
}
pub const SPECS: &[Spec] = &[
    Spec {
        section: "cgroup/connect4",
        ty: 18,
        attach: 10,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/connect6",
        ty: 18,
        attach: 11,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/sendmsg4",
        ty: 18,
        attach: 14,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/sendmsg6",
        ty: 18,
        attach: 15,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/bind4",
        ty: 18,
        attach: 8,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/bind6",
        ty: 18,
        attach: 9,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/sock_create",
        ty: 9,
        attach: 2,
        btf: None,
        optional: false,
    },
    Spec {
        section: "cgroup/dev",
        ty: 15,
        attach: 6,
        btf: None,
        optional: true,
    },
    Spec {
        section: "cgroup/lsm_socket_create",
        ty: 29,
        attach: 43,
        btf: Some("bpf_lsm_socket_create"),
        optional: true,
    },
    Spec {
        section: "cgroup/skb_ingress",
        ty: 8,
        attach: 0,
        btf: None,
        optional: true,
    },
    Spec {
        section: "cgroup/skb_egress",
        ty: 8,
        attach: 1,
        btf: None,
        optional: true,
    },
];
#[repr(C, align(8))]
struct Attr([u8; 160]);
impl Attr {
    fn new() -> Self {
        Self([0; 160])
    }
    fn u32(&mut self, o: usize, v: u32) {
        self.0[o..o + 4].copy_from_slice(&v.to_ne_bytes())
    }
    fn u64(&mut self, o: usize, v: u64) {
        self.0[o..o + 8].copy_from_slice(&v.to_ne_bytes())
    }
    fn name(&mut self, o: usize, n: &str) {
        for (d, c) in self.0[o..o + 15].iter_mut().zip(n.bytes()) {
            *d = if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' {
                c
            } else {
                b'_'
            }
        }
    }
}
fn syscall(cmd: u32, a: &mut Attr) -> io::Result<i32> {
    let r = unsafe { libc::syscall(libc::SYS_bpf, cmd, &mut a.0 as *mut _, size_of::<Attr>()) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r as i32)
    }
}
fn fd(cmd: u32, a: &mut Attr) -> io::Result<OwnedFd> {
    let fd = syscall(cmd, a)?;
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        Ok(OwnedFd::from_raw_fd(fd))
    }
}
// Kernel writes may only produce these plain integer layouts. Keeping the trait
// sealed prevents a caller from requesting a bool, reference or invalid enum.
mod sealed {
    pub trait Wire {}
}
pub trait Wire: sealed::Wire + Copy + Default {}
macro_rules! wire { ($($t:ty),*) => { $(impl sealed::Wire for $t {} impl Wire for $t {})* } }
wire!(
    u8, u32, u64, NpCfg, NpApp, NpList, NpCount, NpFlow, NpFlowKey, NpSeenKey, NpBoundKey,
    NpLpm4Key, NpLpm6Key
);

#[derive(Default)]
pub struct Maps(pub HashMap<&'static str, OwnedFd>);
impl Maps {
    pub fn fd(&self, n: &str) -> i32 {
        self.0.get(n).map(AsRawFd::as_raw_fd).unwrap_or(-1)
    }
    pub fn create() -> io::Result<Self> {
        let defs = [
            ("np_cfg", 2, 4, size_of::<NpCfg>(), 1, 0),
            ("np_apps", 1, 8, size_of::<NpApp>(), NP_MAX_APPS, 0),
            ("np_union", 1, 8, size_of::<NpApp>(), 64, 0),
            ("np_lpm4", 11, 12, size_of::<NpList>(), NP_MAX_LPM, 1),
            ("np_lpm6", 11, 24, size_of::<NpList>(), NP_MAX_LPM, 1),
            (
                "np_seen",
                9,
                size_of::<NpSeenKey>() as u32,
                8,
                NP_MAX_SEEN,
                0,
            ),
            ("np_counts", 1, 8, size_of::<NpCount>(), NP_MAX_APPS, 0),
            ("np_events", 27, 0, 0, NP_RINGBUF, 0),
            ("np_snd", 1, 4, 4, 1024, 0),
            (
                "np_flows",
                9,
                size_of::<NpFlowKey>() as u32,
                size_of::<NpFlow>(),
                NP_MAX_FLOWS,
                0,
            ),
            (
                "np_bound",
                9,
                size_of::<NpBoundKey>() as u32,
                1,
                NP_MAX_BOUND,
                0,
            ),
            ("np_blk4", 11, 12, 1, NP_MAX_BLK, 1),
            ("np_blk6", 11, 24, 1, NP_MAX_BLK, 1),
            ("np_sysc", 1, 8, size_of::<NpCount>(), NP_MAX_SYS, 0),
        ];
        let mut m = Self::default();
        for (n, ty, k, v, max, flags) in defs {
            let mut a = Attr::new();
            a.u32(0, ty);
            a.u32(4, k);
            a.u32(8, v as u32);
            a.u32(12, max);
            a.u32(16, flags);
            a.name(28, n);
            let f = match fd(0, &mut a) {
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {
                    a.0[28..44].fill(0);
                    fd(0, &mut a)?
                }
                r => r?,
            };
            m.0.insert(n, f);
        }
        Ok(m)
    }
    pub fn put<K: Wire, V: Wire>(&self, n: &str, k: &K, v: &V, flags: u64) -> io::Result<()> {
        self.layout(n, size_of::<K>(), size_of::<V>())?;
        let mut a = Attr::new();
        a.u32(0, self.fd(n) as u32);
        a.u64(8, k as *const K as u64);
        a.u64(16, v as *const V as u64);
        a.u64(24, flags);
        syscall(2, &mut a).map(|_| ())
    }
    pub fn get<K: Wire, V: Wire>(&self, n: &str, k: &K) -> Option<V> {
        self.layout(n, size_of::<K>(), size_of::<V>()).ok()?;
        let mut v = V::default();
        let mut a = Attr::new();
        a.u32(0, self.fd(n) as u32);
        a.u64(8, k as *const K as u64);
        a.u64(16, &mut v as *mut V as u64);
        syscall(1, &mut a).ok().map(|_| v)
    }
    pub fn delete<K: Wire>(&self, n: &str, k: &K) {
        if self.layout(n, size_of::<K>(), 0).is_err() {
            return;
        }
        let mut a = Attr::new();
        a.u32(0, self.fd(n) as u32);
        a.u64(8, k as *const K as u64);
        let _ = syscall(3, &mut a);
    }
    // All callers are internal, but validate widths before the kernel can write a value.
    fn layout(&self, n: &str, k: usize, v: usize) -> io::Result<()> {
        let expected = match n {
            "np_cfg" => (4, 16),
            "np_apps" | "np_union" => (8, size_of::<NpApp>()),
            "np_lpm4" => (12, size_of::<NpList>()),
            "np_lpm6" => (24, size_of::<NpList>()),
            "np_seen" => (size_of::<NpSeenKey>(), 8),
            "np_counts" | "np_sysc" => (8, 40),
            "np_snd" => (4, 4),
            "np_bound" => (16, 1),
            "np_flows" => (size_of::<NpFlowKey>(), 24),
            "np_blk4" => (12, 1),
            "np_blk6" => (24, 1),
            _ => return Err(io::Error::from_raw_os_error(libc::EINVAL)),
        };
        if k != expected.0 || v != 0 && v != expected.1 {
            Err(io::Error::from_raw_os_error(libc::EINVAL))
        } else {
            Ok(())
        }
    }
    pub fn flows(&self) -> io::Result<Vec<(NpFlowKey, NpFlow)>> {
        let max = NP_MAX_FLOWS as usize;
        let mut keys = vec![NpFlowKey::default(); max];
        let mut vals = vec![NpFlow::default(); max];
        let mut got = 0;
        let mut cursor = 0u64;
        while got < max {
            let mut a = Attr::new();
            a.u64(
                0,
                if got == 0 {
                    0
                } else {
                    &cursor as *const u64 as u64
                },
            );
            a.u64(8, &mut cursor as *mut u64 as u64);
            a.u64(16, keys[got..].as_mut_ptr() as u64);
            a.u64(24, vals[got..].as_mut_ptr() as u64);
            a.u32(32, (max - got) as u32);
            a.u32(36, self.fd("np_flows") as u32);
            let r = syscall(24, &mut a);
            let n = u32::from_ne_bytes(a.0[32..36].try_into().unwrap()) as usize;
            match r {
                Ok(_) => {
                    if n == 0 {
                        break;
                    }
                    got += n.min(max - got)
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                    got += n.min(max - got);
                    break;
                }
                Err(e)
                    if got == 0
                        && matches!(
                            e.raw_os_error(),
                            Some(libc::EINVAL | libc::EOPNOTSUPP | 524)
                        ) =>
                {
                    let mut cur: Option<NpFlowKey> = None;
                    for _ in 0..max * 2 {
                        let mut next = NpFlowKey::default();
                        let mut a = Attr::new();
                        a.u32(0, self.fd("np_flows") as u32);
                        a.u64(8, cur.as_ref().map_or(0, |k| k as *const _ as u64));
                        a.u64(16, &mut next as *mut _ as u64);
                        if syscall(4, &mut a).is_err() {
                            break;
                        }
                        cur = Some(next);
                        if let Some(v) = self.get("np_flows", &next) {
                            keys[got] = next;
                            vals[got] = v;
                            got += 1;
                            if got == max {
                                break;
                            }
                        }
                    }
                    break;
                }
                Err(e) => {
                    if got == 0 {
                        return Err(e);
                    } else {
                        break;
                    }
                }
            }
        }
        keys.truncate(got);
        vals.truncate(got);
        Ok(keys.into_iter().zip(vals).collect())
    }
}
pub fn relocate(p: &Program, m: &Maps) -> io::Result<Vec<u64>> {
    if p.insns.is_empty() || p.insns.len() > 1_000_000 {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }
    let mut ins = p.insns.to_vec();
    for &(k, n) in p.relocs {
        let fd = m.fd(n);
        if k + 1 >= ins.len() || ins[k] & 255 != 0x18 || fd < 0 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        ins[k] = (ins[k] & 0x0000000000000fff) | 0x1000 | ((fd as u64) << 32);
        ins[k + 1] = 0;
    }
    Ok(ins)
}
#[derive(Debug)]
pub struct LoadError {
    pub errno: i32,
    pub detail: String,
}
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.detail.fmt(f)
    }
}
impl std::error::Error for LoadError {}
impl From<io::Error> for LoadError {
    fn from(e: io::Error) -> Self {
        Self {
            errno: e.raw_os_error().unwrap_or(libc::EIO),
            detail: e.to_string(),
        }
    }
}
pub fn load(spec: &Spec, p: &Program, m: &Maps) -> Result<OwnedFd, LoadError> {
    let ins = relocate(p, m)?;
    let btf = if let Some(n) = spec.btf {
        btf_file("/sys/kernel/btf/vmlinux", n)
    } else {
        0
    };
    if spec.btf.is_some() && btf == 0 {
        return Err(LoadError {
            errno: libc::ENOENT,
            detail: "LSM hook absent from kernel BTF".into(),
        });
    }
    let license = CString::new(if spec.ty == 29 {
        "Dual MIT/GPL"
    } else {
        "LordOS"
    })
    .unwrap();
    let mut a = Attr::new();
    a.u32(0, spec.ty);
    a.u32(4, ins.len() as u32);
    a.u64(8, ins.as_ptr() as u64);
    a.u64(16, license.as_ptr() as u64);
    a.name(48, p.name);
    a.u32(68, spec.attach);
    a.u32(108, btf);
    match fd(5, &mut a) {
        Ok(f) => Ok(f),
        Err(e) => {
            let mut log = vec![0u8; 1 << 20];
            a.u32(24, 1);
            a.u32(28, log.len() as u32);
            a.u64(32, log.as_mut_ptr() as u64);
            if let Ok(f) = fd(5, &mut a) {
                return Ok(f);
            }
            let end = log.iter().position(|&x| x == 0).unwrap_or(log.len());
            Err(LoadError {
                errno: e.raw_os_error().unwrap_or(libc::EIO),
                detail: format!(
                    "{e}: {}",
                    String::from_utf8_lossy(&log[end.saturating_sub(4000)..end])
                ),
            })
        }
    }
}
pub fn link(prog: i32, target: i32, attach: u32) -> io::Result<OwnedFd> {
    let mut a = Attr::new();
    a.u32(0, prog as u32);
    a.u32(4, target as u32);
    a.u32(8, attach);
    fd(28, &mut a)
}
pub fn btf_file(path: &str, name: &str) -> u32 {
    let Ok(f) = std::fs::File::open(path) else {
        return 0;
    };
    let mut bytes = vec![];
    if f.take((64 << 20) + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 64 << 20 {
        return 0;
    }
    btf(&bytes, name).unwrap_or(0)
}
pub fn btf(b: &[u8], name: &str) -> Option<u32> {
    let r = |off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
    };
    if b.get(..4)? != [0x9f, 0xeb, 1, 0] {
        return None;
    }
    let h = r(4)? as usize;
    if h < 24 {
        return None;
    }
    let t = b.get(
        h.checked_add(r(8)? as usize)?
            ..h.checked_add(r(8)? as usize)?
                .checked_add(r(12)? as usize)?,
    )?;
    let s = b.get(
        h.checked_add(r(16)? as usize)?
            ..h.checked_add(r(16)? as usize)?
                .checked_add(r(20)? as usize)?,
    )?;
    let mut pos = 0;
    let mut id = 1;
    while pos + 12 <= t.len() {
        let no = u32::from_le_bytes(t[pos..pos + 4].try_into().ok()?) as usize;
        let info = u32::from_le_bytes(t[pos + 4..pos + 8].try_into().ok()?);
        let kind = info >> 24 & 31;
        let n = (info & 65535) as usize;
        let extra = match kind {
            1 | 14 | 17 => 4,
            3 => 12,
            4 | 5 | 15 | 19 => 12 * n,
            6 | 13 => 8 * n,
            2 | 7..=12 | 16 | 18 => 0,
            _ => return None,
        };
        if pos + 12 + extra > t.len() {
            return None;
        }
        if kind == 12
            && s.get(no..no + name.len() + 1)
                .is_some_and(|s| s[..name.len()] == *name.as_bytes() && s[name.len()] == 0)
        {
            return Some(id);
        }
        pos += 12 + extra;
        id += 1;
    }
    None
}
pub fn cgroup_id(dir: i32, name: &str) -> u64 {
    let Ok(name) = CString::new(name) else {
        return 0;
    };
    unsafe {
        #[repr(C, align(8))]
        struct Handle {
            len: u32,
            ty: i32,
            id: u64,
        }
        let mut h = Handle {
            len: 8,
            ty: 0,
            id: 0,
        };
        let mut mount = 0;
        if libc::syscall(
            libc::SYS_name_to_handle_at,
            dir,
            name.as_ptr(),
            &mut h as *mut _,
            &mut mount as *mut i32,
            0,
        ) == 0
            && h.len == 8
        {
            return h.id;
        }
        let mut st = std::mem::zeroed::<libc::stat>();
        if libc::fstatat(dir, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) == 0
            && st.st_mode & libc::S_IFMT == libc::S_IFDIR
        {
            st.st_ino
        } else {
            0
        }
    }
}
pub struct Ring {
    cons: *mut u8,
    prod: *mut u8,
    size: usize,
    page: usize,
}
impl Ring {
    pub fn open(fd: i32, size: usize) -> io::Result<Self> {
        if !size.is_power_of_two() {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let c = libc::mmap(
                ptr::null_mut(),
                page,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            if c == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            let p = libc::mmap(
                ptr::null_mut(),
                page + 2 * size,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                page as i64,
            );
            if p == libc::MAP_FAILED {
                libc::munmap(c, page);
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                cons: c.cast(),
                prod: p.cast(),
                size,
                page,
            })
        }
    }
    pub fn drain(&mut self) -> Vec<NpEvent> {
        let mut events = vec![];
        unsafe {
            let cp = &*self.cons.cast::<AtomicU64>();
            let pp = &*self.prod.cast::<AtomicU64>();
            let mut c = cp.load(Ordering::Acquire);
            loop {
                let p = pp.load(Ordering::Acquire);
                if c >= p {
                    break;
                }
                let h = self.prod.add(self.page + (c as usize & (self.size - 1)));
                let n = (&*h.cast::<AtomicU32>()).load(Ordering::Acquire);
                if n & 0x80000000 != 0 {
                    break;
                }
                let body = (n & 0x3fffffff) as usize;
                if body + 8 > self.size {
                    break;
                }
                if n & 0x40000000 == 0 && body >= size_of::<NpEvent>() {
                    events.push(ptr::read_unaligned(h.add(8).cast()))
                }
                c += ((body + 15) & !7) as u64;
                cp.store(c, Ordering::Release)
            }
        }
        events
    }
}
impl Drop for Ring {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.cons.cast(), self.page);
            libc::munmap(self.prod.cast(), self.page + 2 * self.size);
        }
    }
}
