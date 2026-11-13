//! The wire formats of Files → Network, as pure functions over byte buffers: what
//! axia-lan sends and how it reads the answers (ONC RPC, SMB2, NTLMSSP, DCE/RPC).
//! Everything read here comes from the network and is untrusted. C ABI of
//! server/src/lan/lanparse.h; server/tests/lan_test.c drives it.
use std::ffi::{c_char, CStr};
use std::slice;

const SMB_HDR: usize = 64;

#[repr(C)]
pub struct LanShare {
    name: [u8; 128],
    comment: [u8; 128],
}

#[repr(C)]
pub struct LanExport {
    path: [u8; 256],
    open: i32,
}

#[repr(C)]
pub struct SmbMsg {
    status: u32,
    cmd: u16,
    session: u64,
    tree: u32,
    body: *const u8,
    blen: usize,
}

fn put16(p: &mut [u8], v: u32) {
    p[0] = v as u8;
    p[1] = (v >> 8) as u8;
}
fn put32(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_le_bytes());
}
fn put64(p: &mut [u8], v: u64) {
    p[..8].copy_from_slice(&v.to_le_bytes());
}
fn get16(p: &[u8]) -> u32 {
    p[0] as u32 | (p[1] as u32) << 8
}
fn get32(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}
fn get64(p: &[u8]) -> u64 {
    u64::from_le_bytes(p[..8].try_into().unwrap())
}
fn be32(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_be_bytes());
}
fn rd32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

/// One UTF-8 sequence at the start of `p`: its length, or 0 if it is invalid.
fn utf8_len(p: &[u8]) -> usize {
    let l = match p[0] {
        c if c < 0x80 => 1,
        c if c >> 5 == 6 => 2,
        c if c >> 4 == 14 => 3,
        c if c >> 3 == 30 => 4,
        _ => 0,
    };
    for i in 1..l {
        if p.get(i).is_none_or(|&c| c & 0xc0 != 0x80) {
            return 0;
        }
    }
    l
}

/// lanaddr.c's lan_clean_text: leading/trailing spaces, control characters and
/// invalid UTF-8 removed, cut to fit `cap` with its NUL.
pub(crate) fn clean_text(inp: &[u8], cap: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut p = inp.iter().position(|&c| c != b' ').unwrap_or(inp.len());
    while p < inp.len() {
        let l = utf8_len(&inp[p..]);
        if l == 0 || (l == 1 && (inp[p] < 0x20 || inp[p] == 0x7f)) {
            p += 1;
            continue;
        }
        if out.len() + l + 1 > cap {
            break;
        }
        out.extend_from_slice(&inp[p..p + l]);
        p += l;
    }
    while out.last() == Some(&b' ') {
        out.pop();
    }
    out
}

/// Writes `s` and its NUL into `out` (which has room for it).
fn put_cstr(out: &mut [u8], s: &[u8]) {
    out[..s.len()].copy_from_slice(s);
    out[s.len()] = 0;
}

/// Like snprintf(out, "%s", s): cut at the byte.
pub(crate) fn copy_cstr(out: &mut [u8], s: &[u8]) {
    if out.is_empty() {
        return;
    }
    let n = s.len().min(out.len() - 1);
    put_cstr(out, &s[..n]);
}

/// UTF-16LE (n code units) → UTF-8, cleaned for the interface (empty on nothing left).
fn utf16_text(p: &[u8], units: usize, cap: usize) -> Vec<u8> {
    let mut tmp = Vec::with_capacity(512);
    let mut i = 0;
    while i < units && tmp.len() + 5 < 512 {
        let mut c = get16(&p[i * 2..]);
        if c == 0 {
            break;
        }
        if (0xd800..0xdc00).contains(&c) && i + 1 < units {
            let d = get16(&p[(i + 1) * 2..]);
            if (0xdc00..0xe000).contains(&d) {
                c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
                i += 1;
            } else {
                c = 0xfffd;
            }
        } else if (0xd800..0xe000).contains(&c) {
            c = 0xfffd;
        }
        if c < 0x80 {
            tmp.push(c as u8);
        } else if c < 0x800 {
            tmp.extend_from_slice(&[(0xc0 | c >> 6) as u8, (0x80 | (c & 63)) as u8]);
        } else if c < 0x10000 {
            tmp.extend_from_slice(&[(0xe0 | c >> 12) as u8, (0x80 | ((c >> 6) & 63)) as u8, (0x80 | (c & 63)) as u8]);
        } else {
            tmp.extend_from_slice(&[
                (0xf0 | c >> 18) as u8,
                (0x80 | ((c >> 12) & 63)) as u8,
                (0x80 | ((c >> 6) & 63)) as u8,
                (0x80 | (c & 63)) as u8,
            ]);
        }
        i += 1;
    }
    clean_text(&tmp, cap)
}

/// ASCII → UTF-16LE. Returns bytes written.
fn to_utf16(out: &mut [u8], s: &[u8]) -> usize {
    for (i, &c) in s.iter().enumerate() {
        out[i * 2] = c;
        out[i * 2 + 1] = 0;
    }
    s.len() * 2
}

// ---- ONC RPC ----

const PMAP_PROG: u32 = 100000;
const MOUNT_PROG: u32 = 100005;

fn rpc_head(o: &mut [u8], xid: u32, prog: u32, vers: u32, proc_: u32) -> usize {
    be32(&mut o[4..], xid);
    be32(&mut o[8..], 0); // CALL
    be32(&mut o[12..], 2); // RPC version
    be32(&mut o[16..], prog);
    be32(&mut o[20..], vers);
    be32(&mut o[24..], proc_);
    o[28..44].fill(0); // cred and verifier: AUTH_NULL
    44
}

pub fn portmap_call(out: &mut [u8], xid: u32) -> usize {
    if out.len() < 60 {
        return 0;
    }
    let mut n = rpc_head(out, xid, PMAP_PROG, 2, 3);
    be32(&mut out[n..], MOUNT_PROG);
    be32(&mut out[n + 4..], 3);
    be32(&mut out[n + 8..], 6); // TCP
    be32(&mut out[n + 12..], 0);
    n += 16;
    be32(out, 0x8000_0000 | (n - 4) as u32);
    n
}

pub fn export_call(out: &mut [u8], xid: u32) -> usize {
    if out.len() < 44 {
        return 0;
    }
    let n = rpc_head(out, xid, MOUNT_PROG, 3, 5);
    be32(out, 0x8000_0000 | (n - 4) as u32);
    n
}

/// The results' offset in an accepted, successful reply.
fn rpc_results(b: &[u8], xid: u32) -> Option<usize> {
    let len = b.len();
    if len < 24 || rd32(b) != xid || rd32(&b[4..]) != 1 || rd32(&b[8..]) != 0 {
        return None;
    }
    let vl = rd32(&b[16..]) as usize;
    if vl > 400 || vl & 3 != 0 || 20 + vl + 4 > len {
        return None;
    }
    let o = 20 + vl;
    (rd32(&b[o..]) == 0).then_some(o + 4)
}

pub fn portmap_reply(b: &[u8], xid: u32) -> i32 {
    match rpc_results(b, xid) {
        Some(o) if o + 4 <= b.len() => {
            let p = rd32(&b[o..]);
            if p > 65535 { -1 } else { p as i32 }
        }
        _ => -1,
    }
}

const fn pad4(l: usize) -> usize {
    l.div_ceil(4) * 4
}

/// Exports as (path, open); the path is already cleaned. None if invalid.
pub fn export_reply(b: &[u8], xid: u32, max: usize) -> Option<Vec<([u8; 256], usize, bool)>> {
    let len = b.len();
    let mut o = rpc_results(b, xid)?;
    let mut out = Vec::new();
    loop {
        if o + 4 > len {
            return None;
        }
        let more = rd32(&b[o..]);
        o += 4;
        if more == 0 {
            return Some(out);
        }
        if more != 1 || o + 4 > len {
            return None;
        }
        let l = rd32(&b[o..]) as usize;
        o += 4;
        if l > 1024 || o + pad4(l) > len {
            return None;
        }
        let path = &b[o..o + l];
        o += pad4(l);
        let (mut open, mut groups) = (false, 0);
        loop {
            if o + 4 > len {
                return None;
            }
            let g = rd32(&b[o..]);
            o += 4;
            if g == 0 {
                break;
            }
            groups += 1;
            if g != 1 || o + 4 > len || groups > 4096 {
                return None;
            }
            let gl = rd32(&b[o..]) as usize;
            o += 4;
            if gl > 1024 || o + pad4(gl) > len {
                return None;
            }
            if gl == 1 && b[o] == b'*' {
                open = true;
            }
            o += pad4(gl);
        }
        if groups == 0 {
            open = true; // no list: exported to everyone
        }
        if out.len() < max && path.first() == Some(&b'/') && !path.contains(&0) {
            let t = clean_text(path, 256);
            if !t.is_empty() {
                let mut p = [0u8; 256];
                put_cstr(&mut p, &t);
                out.push((p, t.len(), open));
            }
        }
    }
}

// ---- SMB2 ----

const C_NEGOTIATE: u16 = 0;
const C_SESSION: u16 = 1;
const C_TREE: u16 = 3;
const C_CREATE: u16 = 5;
const C_IOCTL: u16 = 11;

fn smb_head(p: &mut [u8], cmd: u16, msgid: u64, session: u64, tree: u32) {
    p[..SMB_HDR].fill(0);
    p[..4].copy_from_slice(b"\xfeSMB");
    put16(&mut p[4..], SMB_HDR as u32);
    put16(&mut p[12..], cmd as u32);
    put16(&mut p[14..], 16); // credits requested
    put64(&mut p[24..], msgid);
    put32(&mut p[36..], tree);
    put64(&mut p[40..], session);
}

fn transport(out: &mut [u8], body: usize) {
    out[0] = 0;
    out[1] = (body >> 16) as u8;
    out[2] = (body >> 8) as u8;
    out[3] = body as u8;
}

pub fn negotiate(out: &mut [u8], msgid: u64) -> usize {
    const DIALECTS: [u32; 4] = [0x0202, 0x0210, 0x0300, 0x0302];
    let n = 4 + SMB_HDR + 36 + 8;
    if out.len() < n {
        return 0;
    }
    smb_head(&mut out[4..], C_NEGOTIATE, msgid, 0, 0);
    let b = &mut out[4 + SMB_HDR..];
    b[..36].fill(0);
    put16(b, 36);
    put16(&mut b[2..], 4);
    put16(&mut b[4..], 1); // signing enabled, not required
    b[12..28].copy_from_slice(b"LordOS-Network\0\x01");
    for (i, d) in DIALECTS.iter().enumerate() {
        put16(&mut b[36 + i * 2..], *d);
    }
    transport(out, n - 4);
    n
}

/// Listing after signing in signs its messages, and these two dialects sign with HMAC-SHA256 alone.
pub fn negotiate_signed(out: &mut [u8], msgid: u64) -> usize {
    let mut n = negotiate(out, msgid);
    if n == 0 {
        return 0;
    }
    put16(&mut out[4 + SMB_HDR + 2..], 2);
    n -= 4;
    transport(out, n - 4);
    n
}

pub fn session_setup(out: &mut [u8], msgid: u64, session: u64, blob: &[u8]) -> usize {
    let n = 4 + SMB_HDR + 24 + blob.len();
    if out.len() < n {
        return 0;
    }
    smb_head(&mut out[4..], C_SESSION, msgid, session, 0);
    let b = &mut out[4 + SMB_HDR..];
    b[..24].fill(0);
    put16(b, 25);
    b[3] = 1; // signing enabled
    put16(&mut b[12..], (SMB_HDR + 24) as u32);
    put16(&mut b[14..], blob.len() as u32);
    b[24..24 + blob.len()].copy_from_slice(blob);
    transport(out, n - 4);
    n
}

pub fn tree_connect(out: &mut [u8], msgid: u64, session: u64, ip: &[u8]) -> usize {
    let mut unc = b"\\\\".to_vec();
    unc.extend_from_slice(ip);
    unc.extend_from_slice(b"\\IPC$");
    if unc.len() >= 32 {
        return 0;
    }
    let pl = unc.len() * 2;
    let n = 4 + SMB_HDR + 8 + pl;
    if out.len() < n {
        return 0;
    }
    smb_head(&mut out[4..], C_TREE, msgid, session, 0);
    let b = &mut out[4 + SMB_HDR..];
    b[..8].fill(0);
    put16(b, 9);
    put16(&mut b[4..], (SMB_HDR + 8) as u32);
    put16(&mut b[6..], pl as u32);
    to_utf16(&mut b[8..], &unc);
    transport(out, n - 4);
    n
}

pub fn create_pipe(out: &mut [u8], msgid: u64, session: u64, tree: u32) -> usize {
    let nl = 12; // "srvsvc"
    let n = 4 + SMB_HDR + 56 + nl;
    if out.len() < n {
        return 0;
    }
    smb_head(&mut out[4..], C_CREATE, msgid, session, tree);
    let b = &mut out[4 + SMB_HDR..];
    b[..56].fill(0);
    put16(b, 57);
    put32(&mut b[4..], 2); // impersonation
    put32(&mut b[24..], 0x0012_019f); // read, write, attributes, synchronize
    put32(&mut b[32..], 7); // share everything
    put32(&mut b[36..], 1); // open if it exists
    put32(&mut b[40..], 0x40); // not a folder
    put16(&mut b[44..], (SMB_HDR + 56) as u32);
    put16(&mut b[46..], nl as u32);
    to_utf16(&mut b[56..], b"srvsvc");
    transport(out, n - 4);
    n
}

pub fn pipe_ioctl(out: &mut [u8], msgid: u64, session: u64, tree: u32, fid: &[u8; 16], inp: &[u8]) -> usize {
    let n = 4 + SMB_HDR + 56 + inp.len();
    if out.len() < n {
        return 0;
    }
    smb_head(&mut out[4..], C_IOCTL, msgid, session, tree);
    let b = &mut out[4 + SMB_HDR..];
    b[..56].fill(0);
    put16(b, 57);
    put32(&mut b[4..], 0x0011_c017); // FSCTL_PIPE_TRANSCEIVE
    b[8..24].copy_from_slice(fid);
    put32(&mut b[24..], (SMB_HDR + 56) as u32);
    put32(&mut b[28..], inp.len() as u32);
    put32(&mut b[44..], 60000); // the most the answer may have
    put32(&mut b[48..], 1); // it is an FSCTL
    b[56..56 + inp.len()].copy_from_slice(inp);
    transport(out, n - 4);
    n
}

// ---- NTLMSSP ----

/// UNICODE, REQUEST_TARGET, NTLM, ANONYMOUS, EXTENDED_SESSIONSECURITY, TARGET_INFO, 128, 56
const NTLM_FLAGS: u32 = 0xa088_0a05;

pub fn ntlm_neg(o: &mut [u8]) -> usize {
    if o.len() < 32 {
        return 0;
    }
    o[..32].fill(0);
    o[..8].copy_from_slice(b"NTLMSSP\0");
    put32(&mut o[8..], 1);
    put32(&mut o[12..], NTLM_FLAGS);
    32
}

pub fn ntlm_anon(o: &mut [u8]) -> usize {
    if o.len() < 65 {
        return 0;
    }
    o[..65].fill(0);
    o[..8].copy_from_slice(b"NTLMSSP\0");
    put32(&mut o[8..], 3);
    // one zero byte of LM response, every other field empty, all pointing past the header
    put16(&mut o[12..], 1);
    put16(&mut o[14..], 1);
    put32(&mut o[16..], 64);
    for f in (20..=52).step_by(8) {
        put32(&mut o[f + 4..], 65);
    }
    put32(&mut o[60..], NTLM_FLAGS);
    65
}

/// The server's NetBIOS (or DNS) computer name and its NetBIOS domain, and whether
/// the challenge was valid. Like the C, the names read before an invalid entry stay.
pub fn challenge_names(b: &[u8], nn: usize, dn: usize) -> (Vec<u8>, Vec<u8>, bool) {
    let (mut name, mut domain) = (Vec::new(), Vec::new());
    let ok = names_into(b, nn, dn, &mut name, &mut domain).is_some();
    (name, domain, ok)
}

fn names_into(b: &[u8], nn: usize, dn: usize, name: &mut Vec<u8>, domain: &mut Vec<u8>) -> Option<()> {
    let len = b.len();
    if len < 48 || &b[..8] != b"NTLMSSP\0" || get32(&b[8..]) != 2 {
        return None;
    }
    let (il, io) = (get16(&b[40..]) as usize, get32(&b[44..]) as usize);
    if il == 0 {
        return Some(());
    }
    if io > len || il > len - io {
        return None;
    }
    let p = &b[io..io + il];
    let mut have_dns = false;
    let mut o = 0;
    while o + 4 <= il {
        let (id, l) = (get16(&p[o..]), get16(&p[o + 2..]) as usize);
        if id == 0 {
            break;
        }
        if o + 4 + l > il {
            return None;
        }
        let v = &p[o + 4..o + 4 + l];
        if id == 1 && name.is_empty() {
            *name = utf16_text(v, l / 2, nn);
        } else if id == 2 {
            *domain = utf16_text(v, l / 2, dn);
        } else if id == 3 && !have_dns {
            let t = utf16_text(v, l / 2, 256);
            if !t.is_empty() && name.is_empty() {
                *name = t[..t.len().min(nn.saturating_sub(1))].to_vec();
                have_dns = true;
            }
        }
        o += 4 + l;
    }
    Some(())
}

// ---- DCE/RPC ----

fn dce_head(o: &mut [u8], ty: u8, frag: usize, call: u32) {
    o[..8].copy_from_slice(&[5, 0, ty, 3, 0x10, 0, 0, 0]);
    put16(&mut o[8..], frag as u32);
    put16(&mut o[10..], 0);
    put32(&mut o[12..], call);
}

pub fn bind(o: &mut [u8]) -> usize {
    const SRVSVC: [u8; 20] =
        [0xc8, 0x4f, 0x32, 0x4b, 0x70, 0x16, 0xd3, 0x01, 0x12, 0x78, 0x5a, 0x47, 0xbf, 0x6e, 0xe1, 0x88, 3, 0, 0, 0];
    const NDR: [u8; 20] =
        [0x04, 0x5d, 0x88, 0x8a, 0xeb, 0x1c, 0xc9, 0x11, 0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60, 2, 0, 0, 0];
    let n = 16 + 8 + 4 + 44;
    if o.len() < n {
        return 0;
    }
    o[..n].fill(0);
    dce_head(o, 11, n, 1);
    put16(&mut o[16..], 60000); // the answer comes whole, in one fragment
    put16(&mut o[18..], 60000);
    o[24] = 1; // one context
    o[30] = 1; // one transfer syntax
    o[32..52].copy_from_slice(&SRVSVC);
    o[52..72].copy_from_slice(&NDR);
    n
}

pub fn bind_ack(b: &[u8]) -> i32 {
    let len = b.len();
    if len < 28 || b[0] != 5 || b[2] != 12 {
        return -1;
    }
    let o = (26 + get16(&b[24..]) as usize + 3) & !3;
    if o + 8 > len {
        return -1;
    }
    if b[o] >= 1 && get16(&b[o + 4..]) == 0 { 0 } else { -1 }
}

/// NDR conformant-varying UTF-16 string, with its padding.
fn ndr_wstr(o: &mut [u8], s: &[u8]) -> usize {
    let l = s.len() as u32 + 1;
    put32(o, l);
    put32(&mut o[4..], 0);
    put32(&mut o[8..], l);
    let mut w = to_utf16(&mut o[12..], s);
    put16(&mut o[12 + w..], 0);
    w += 2;
    let mut t = 12 + w;
    while t & 3 != 0 {
        o[t] = 0;
        t += 1;
    }
    t
}

pub fn share_enum(o: &mut [u8], ip: &[u8]) -> usize {
    let mut srv = b"\\\\".to_vec();
    srv.extend_from_slice(ip);
    if srv.len() >= 24 || o.len() < 24 + 4 + 12 + 2 * srv.len() + 8 + 40 {
        return 0;
    }
    let s = &mut o[24..];
    let mut n = 0;
    put32(s, 0x20000); // server name
    n += 4;
    n += ndr_wstr(&mut s[n..], &srv);
    for v in [1u32, 1, 0x20004, 0, 0, 0xffff_ffff, 0x20008, 0] {
        // level, union discriminant, the level-1 container, entries read, buffer: none,
        // preferred maximum, resume handle
        put32(&mut s[n..], v);
        n += 4;
    }
    let all = 24 + n;
    dce_head(o, 0, all, 2);
    put32(&mut o[16..], n as u32);
    put16(&mut o[20..], 0); // context 0
    put16(&mut o[22..], 15); // NetrShareEnum
    all
}

/// One NDR string at *o into text; moves *o past it and its padding.
fn ndr_read(b: &[u8], o: &mut usize, cap: usize) -> Option<Vec<u8>> {
    let len = b.len();
    let mut p = *o;
    if p + 12 > len {
        return None;
    }
    let (off, act) = (get32(&b[p + 4..]), get32(&b[p + 8..]) as usize);
    p += 12;
    if off > 4096 || act > 4096 || (len - p) / 2 < act {
        return None;
    }
    let t = utf16_text(&b[p..], act, cap);
    p = (p + act * 2 + 3) & !3;
    *o = p;
    Some(t)
}

/// Disk shares only, without the administrative ones (type bit 31, or a name ending in $).
pub fn share_reply(b: &[u8], max: usize) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    let len = b.len();
    if len < 24 + 24 || b[0] != 5 || b[2] != 2 {
        return None;
    }
    let mut o = 24;
    if get32(&b[o..]) != 1 {
        return None; // level
    }
    o += 8; // level, discriminant
    if get32(&b[o..]) == 0 {
        return None; // the container
    }
    let (count, bufp) = (get32(&b[o + 4..]) as usize, get32(&b[o + 8..]));
    o += 12;
    let mut out = Vec::new();
    if count == 0 || bufp == 0 {
        return Some(out);
    }
    if count > 4096 || o + 4 > len {
        return None;
    }
    let max_count = get32(&b[o..]) as usize;
    o += 4;
    if max_count < count || (len - o) / 12 < count {
        return None;
    }
    let ents = o;
    o += count * 12;
    for i in 0..count {
        let e = &b[ents + i * 12..];
        let name = if get32(e) != 0 { ndr_read(b, &mut o, 128)? } else { Vec::new() };
        let comment = if get32(&e[8..]) != 0 { ndr_read(b, &mut o, 128)? } else { Vec::new() };
        let ty = get32(&e[4..]);
        if ty & 0x0fff_ffff != 0 || ty & 0x8000_0000 != 0 || name.is_empty() || name.last() == Some(&b'$') || out.len() >= max {
            continue;
        }
        out.push((name, comment));
    }
    Some(out)
}

// ---- C ABI ----

pub(crate) unsafe fn buf<'a>(p: *mut u8, cap: usize) -> &'a mut [u8] {
    if p.is_null() || cap == 0 { &mut [] } else { slice::from_raw_parts_mut(p, cap) }
}
unsafe fn bytes<'a>(p: *const u8, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 { &[] } else { slice::from_raw_parts(p, len) }
}
pub(crate) unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() { b"" } else { CStr::from_ptr(p).to_bytes() }
}
unsafe fn body<'a>(m: *const SmbMsg) -> Option<(&'a SmbMsg, &'a [u8])> {
    let m = m.as_ref()?;
    Some((m, bytes(m.body, m.blen)))
}

#[no_mangle]
pub unsafe extern "C" fn rpc_portmap_call(out: *mut u8, cap: usize, xid: u32) -> usize {
    portmap_call(buf(out, cap), xid)
}
#[no_mangle]
pub unsafe extern "C" fn rpc_export_call(out: *mut u8, cap: usize, xid: u32) -> usize {
    export_call(buf(out, cap), xid)
}
#[no_mangle]
pub unsafe extern "C" fn rpc_portmap_reply(b: *const u8, len: usize, xid: u32) -> i32 {
    portmap_reply(bytes(b, len), xid)
}
#[no_mangle]
pub unsafe extern "C" fn rpc_export_reply(b: *const u8, len: usize, xid: u32, out: *mut LanExport, max: i32) -> i32 {
    let max = if out.is_null() { 0 } else { max.max(0) as usize };
    let Some(list) = export_reply(bytes(b, len), xid, max) else { return -1 };
    for (i, (path, _, open)) in list.iter().enumerate() {
        let e = &mut *out.add(i);
        e.path = *path;
        e.open = *open as i32;
    }
    list.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn smb_parse(b: *const u8, len: usize, m: *mut SmbMsg) -> i32 {
    let s = bytes(b, len);
    if len < SMB_HDR || m.is_null() || &s[..4] != b"\xfeSMB" || get16(&s[4..]) != SMB_HDR as u32 {
        return -1;
    }
    *m = SmbMsg {
        status: get32(&s[8..]),
        cmd: get16(&s[12..]) as u16,
        tree: get32(&s[36..]),
        session: get64(&s[40..]),
        body: b.add(SMB_HDR),
        blen: len - SMB_HDR,
    };
    0
}
#[no_mangle]
pub unsafe extern "C" fn smb_negotiate(out: *mut u8, cap: usize, msgid: u64) -> usize {
    negotiate(buf(out, cap), msgid)
}
#[no_mangle]
pub unsafe extern "C" fn smb_negotiate_signed(out: *mut u8, cap: usize, msgid: u64) -> usize {
    negotiate_signed(buf(out, cap), msgid)
}
#[no_mangle]
pub unsafe extern "C" fn smb_session_setup(
    out: *mut u8,
    cap: usize,
    msgid: u64,
    session: u64,
    blob: *const u8,
    blen: usize,
) -> usize {
    session_setup(buf(out, cap), msgid, session, bytes(blob, blen))
}
#[no_mangle]
pub unsafe extern "C" fn smb_tree_connect(out: *mut u8, cap: usize, msgid: u64, session: u64, ip: *const c_char) -> usize {
    tree_connect(buf(out, cap), msgid, session, cstr(ip))
}
#[no_mangle]
pub unsafe extern "C" fn smb_create_pipe(out: *mut u8, cap: usize, msgid: u64, session: u64, tree: u32) -> usize {
    create_pipe(buf(out, cap), msgid, session, tree)
}
#[no_mangle]
pub unsafe extern "C" fn smb_pipe_ioctl(
    out: *mut u8,
    cap: usize,
    msgid: u64,
    session: u64,
    tree: u32,
    fid: *const u8,
    inp: *const u8,
    ilen: usize,
) -> usize {
    let Some(fid) = (fid as *const [u8; 16]).as_ref() else { return 0 };
    pipe_ioctl(buf(out, cap), msgid, session, tree, fid, bytes(inp, ilen))
}

#[no_mangle]
pub unsafe extern "C" fn smb_negotiate_reply(m: *const SmbMsg, signing: *mut i32) -> i32 {
    let Some((m, b)) = body(m) else { return 0 };
    if m.status != 0 || m.cmd != C_NEGOTIATE || b.len() < 64 || get16(b) != 65 {
        return 0;
    }
    if !signing.is_null() {
        *signing = (get16(&b[2..]) & 2 != 0) as i32;
    }
    get16(&b[4..]) as i32
}
#[no_mangle]
pub unsafe extern "C" fn smb_session_reply(
    m: *const SmbMsg,
    blob: *mut *const u8,
    blen: *mut usize,
    flags: *mut u32,
) -> i32 {
    let Some((m, b)) = body(m) else { return -1 };
    if m.cmd != C_SESSION || b.len() < 8 || get16(b) != 9 {
        return -1;
    }
    *flags = get16(&b[2..]);
    let (off, l) = (get16(&b[4..]) as usize, get16(&b[6..]) as usize);
    *blob = std::ptr::null();
    *blen = 0;
    if l != 0 {
        if off < SMB_HDR + 8 || off - SMB_HDR + l > b.len() {
            return -1;
        }
        *blob = m.body.add(off - SMB_HDR);
        *blen = l;
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn smb_create_reply(m: *const SmbMsg, fid: *mut u8) -> i32 {
    let Some((m, b)) = body(m) else { return -1 };
    if m.status != 0 || m.cmd != C_CREATE || b.len() < 88 || get16(b) != 89 {
        return -1;
    }
    buf(fid, 16).copy_from_slice(&b[64..80]);
    0
}
#[no_mangle]
pub unsafe extern "C" fn smb_ioctl_reply(m: *const SmbMsg, out: *mut *const u8, olen: *mut usize) -> i32 {
    let Some((m, b)) = body(m) else { return -1 };
    if m.cmd != C_IOCTL || b.len() < 48 || get16(b) != 49 {
        return -1;
    }
    if m.status != 0 && m.status != 0x8000_0005 {
        return -1; // a truncated answer is still parsed
    }
    let (off, l) = (get32(&b[32..]) as usize, get32(&b[36..]) as usize);
    if off < SMB_HDR + 48 || off - SMB_HDR > b.len() || l > b.len() - (off - SMB_HDR) {
        return -1;
    }
    *out = m.body.add(off - SMB_HDR);
    *olen = l;
    0
}

#[no_mangle]
pub unsafe extern "C" fn ntlm_negotiate(o: *mut u8, cap: usize) -> usize {
    ntlm_neg(buf(o, cap))
}
#[no_mangle]
pub unsafe extern "C" fn ntlm_anonymous_auth(o: *mut u8, cap: usize) -> usize {
    ntlm_anon(buf(o, cap))
}
#[no_mangle]
pub unsafe extern "C" fn ntlm_challenge_names(
    b: *const u8,
    len: usize,
    name: *mut c_char,
    nn: usize,
    domain: *mut c_char,
    dn: usize,
) -> i32 {
    let (name, domain) = (buf(name.cast(), nn), buf(domain.cast(), dn));
    copy_cstr(name, b"");
    copy_cstr(domain, b"");
    let (n, d, ok) = challenge_names(bytes(b, len), name.len(), domain.len());
    copy_cstr(name, &n);
    copy_cstr(domain, &d);
    if ok { 0 } else { -1 }
}

#[no_mangle]
pub unsafe extern "C" fn dce_bind(o: *mut u8, cap: usize) -> usize {
    bind(buf(o, cap))
}
#[no_mangle]
pub unsafe extern "C" fn dce_bind_ack(b: *const u8, len: usize) -> i32 {
    bind_ack(bytes(b, len))
}
#[no_mangle]
pub unsafe extern "C" fn dce_share_enum(o: *mut u8, cap: usize, ip: *const c_char) -> usize {
    share_enum(buf(o, cap), cstr(ip))
}
#[no_mangle]
pub unsafe extern "C" fn dce_share_reply(b: *const u8, len: usize, out: *mut LanShare, max: i32) -> i32 {
    let max = if out.is_null() { 0 } else { max.max(0) as usize };
    let Some(list) = share_reply(bytes(b, len), max) else { return -1 };
    for (i, (name, comment)) in list.iter().enumerate() {
        let s = &mut *out.add(i);
        copy_cstr(&mut s.name, name);
        copy_cstr(&mut s.comment, comment);
    }
    list.len() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_matches_lanaddr() {
        assert_eq!(clean_text(b"  a\x01b \xff c  ", 64), b"ab  c");
        assert_eq!(clean_text("ação".as_bytes(), 4), "aç".as_bytes());
        assert!(clean_text(b"   ", 8).is_empty());
    }

    #[test]
    fn requests_have_their_lengths() {
        let mut o = [0u8; 512];
        assert_eq!(negotiate(&mut o, 1), 112);
        assert_eq!(negotiate_signed(&mut o, 1), 108);
        assert_eq!(&o[1..4], &[0, 0, 104]);
        assert_eq!(tree_connect(&mut o, 2, 3, b"192.168.0.10"), 4 + 64 + 8 + 2 * 19);
        assert_eq!(tree_connect(&mut o, 2, 3, b"1234567890123456789012345"), 0);
        assert_eq!(portmap_call(&mut o, 7), 60);
        assert_eq!(rd32(&o), 0x8000_0000 | 56);
        assert_eq!(share_enum(&mut o, b"10.0.0.1"), 24 + 4 + 12 + 24 + 32); // "\\\\10.0.0.1" + NUL, padded
        assert_eq!(bind(&mut o[..71]), 0);
    }

    #[test]
    fn export_reply_reads_paths_and_groups() {
        let mut b = Vec::new();
        for v in [9u32, 1, 0, 0, 0, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        for v in [1u32, 5] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(b"/srv\0\0\0\0"[..8].as_ref());
        b.truncate(b.len() - 3);
        b.extend_from_slice(&[0; 3]);
        for v in [1u32, 1] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(b"*\0\0\0");
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        // the 5-byte path has a NUL in it, so it is dropped but the reply is valid
        let r = export_reply(&b, 9, 8).unwrap();
        assert!(r.is_empty());
        assert!(export_reply(&b[..b.len() - 1], 9, 8).is_none());
        assert!(export_reply(&b, 8, 8).is_none());
    }
}
