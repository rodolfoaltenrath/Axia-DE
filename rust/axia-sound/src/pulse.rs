//! Serialization of the PulseAudio native protocol ("tagstruct" and packets), the one
//! pipewire-pulse speaks. No libpulse: only what `snd` and the media code use. See docs/SOUND.md.
//!
//! A "tagstruct" is a sequence of values, each one preceded by a byte saying its type ('L' u32,
//! 't' string, 'v' per-channel volume, 'P' property list...). Integers are big endian. Each packet
//! carries a 20-byte descriptor: length, channel (-1 = control), 64-bit offset and flags.
//!
//! `Writer` and `Reader` are the Rust API; the `ts_*` functions below are the C ABI of pulse.h,
//! working on the `TsOut` and `TsIn` structs of that header.

use std::ffi::{c_char, c_int, c_void};

pub const PA_CHANNELS_MAX: usize = 32;
pub const PA_INVALID_INDEX: u32 = 0xffff_ffff;
pub const PA_VOLUME_NORM: u32 = 0x10000;
/// The most Axia lets it go up: 150 %.
pub const PA_VOLUME_MAX: u32 = PA_VOLUME_NORM * 3 / 2;
pub const PA_DESCRIPTOR_SIZE: usize = 20;
pub const PA_FRAME_MAX: usize = 16 * 1024 * 1024;

// commands we use (pulsecore/native-common.h)
pub const PA_CMD_ERROR: u32 = 0;
pub const PA_CMD_REPLY: u32 = 2;
pub const PA_CMD_CREATE_RECORD_STREAM: u32 = 5;
pub const PA_CMD_DELETE_RECORD_STREAM: u32 = 6;
pub const PA_CMD_AUTH: u32 = 8;
pub const PA_CMD_SET_CLIENT_NAME: u32 = 9;
pub const PA_CMD_CREATE_UPLOAD_STREAM: u32 = 15;
pub const PA_CMD_FINISH_UPLOAD_STREAM: u32 = 17;
pub const PA_CMD_PLAY_SAMPLE: u32 = 18;
pub const PA_CMD_GET_SERVER_INFO: u32 = 20;
pub const PA_CMD_GET_SINK_INFO: u32 = 21;
pub const PA_CMD_GET_SINK_INFO_LIST: u32 = 22;
pub const PA_CMD_GET_SOURCE_INFO: u32 = 23;
pub const PA_CMD_GET_SOURCE_INFO_LIST: u32 = 24;
pub const PA_CMD_GET_SINK_INPUT_INFO: u32 = 29;
pub const PA_CMD_GET_SINK_INPUT_INFO_LIST: u32 = 30;
pub const PA_CMD_GET_SOURCE_OUTPUT_INFO: u32 = 31;
pub const PA_CMD_GET_SOURCE_OUTPUT_INFO_LIST: u32 = 32;
pub const PA_CMD_SUBSCRIBE: u32 = 35;
pub const PA_CMD_SET_SINK_VOLUME: u32 = 36;
pub const PA_CMD_SET_SINK_INPUT_VOLUME: u32 = 37;
pub const PA_CMD_SET_SOURCE_VOLUME: u32 = 38;
pub const PA_CMD_SET_SINK_MUTE: u32 = 39;
pub const PA_CMD_SET_SOURCE_MUTE: u32 = 40;
pub const PA_CMD_SET_DEFAULT_SINK: u32 = 44;
pub const PA_CMD_SET_DEFAULT_SOURCE: u32 = 45;
pub const PA_CMD_SUBSCRIBE_EVENT: u32 = 66;
pub const PA_CMD_RECORD_STREAM_KILLED: u32 = 65;
pub const PA_CMD_MOVE_SINK_INPUT: u32 = 67;
pub const PA_CMD_MOVE_SOURCE_OUTPUT: u32 = 68;
pub const PA_CMD_SET_SINK_INPUT_MUTE: u32 = 69;
pub const PA_CMD_SET_SINK_PORT: u32 = 96;
pub const PA_CMD_SET_SOURCE_PORT: u32 = 97;
pub const PA_CMD_SET_SOURCE_OUTPUT_VOLUME: u32 = 98;
pub const PA_CMD_SET_SOURCE_OUTPUT_MUTE: u32 = 99;

// subscription masks and event facility/type
pub const PA_SUB_MASK_SINK: u32 = 0x1;
pub const PA_SUB_MASK_SOURCE: u32 = 0x2;
pub const PA_SUB_MASK_SINK_INPUT: u32 = 0x4;
pub const PA_SUB_MASK_SOURCE_OUTPUT: u32 = 0x8;
pub const PA_SUB_MASK_SERVER: u32 = 0x80;
pub const PA_SUB_MASK_CARD: u32 = 0x200;
pub const PA_SUB_SINK: u32 = 0;
pub const PA_SUB_SOURCE: u32 = 1;
pub const PA_SUB_SINK_INPUT: u32 = 2;
pub const PA_SUB_SOURCE_OUTPUT: u32 = 3;
pub const PA_SUB_SERVER: u32 = 7;
pub const PA_SUB_CARD: u32 = 9;
pub const PA_SUB_FACILITY_MASK: u32 = 0xf;
pub const PA_SUB_REMOVE: u32 = 0x20;
pub const PA_SUB_TYPE_MASK: u32 = 0x30;

pub const PA_SAMPLE_S16LE: u8 = 3;
pub const PA_SAMPLE_FLOAT32LE: u8 = 5;

const T_STRING: u8 = b't';
const T_STRING_NULL: u8 = b'N';
const T_U32: u8 = b'L';
const T_U8: u8 = b'B';
const T_U64: u8 = b'R';
const T_S64: u8 = b'r';
const T_SAMPLE_SPEC: u8 = b'a';
const T_ARBITRARY: u8 = b'x';
const T_TRUE: u8 = b'1';
const T_FALSE: u8 = b'0';
const T_TIMEVAL: u8 = b'T';
const T_USEC: u8 = b'U';
const T_CHANNEL_MAP: u8 = b'm';
const T_CVOLUME: u8 = b'v';
const T_PROPLIST: u8 = b'P';
const T_VOLUME: u8 = b'V';
const T_FORMAT_INFO: u8 = b'f';

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn get_be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

// ---- writing ----

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.push(T_U32);
    b.extend(be32(v));
}
fn put_u8(b: &mut Vec<u8>, v: u8) {
    b.extend([T_U8, v]);
}
fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.push(T_USEC);
    b.extend(be32((v >> 32) as u32));
    b.extend(be32(v as u32));
}
fn put_timeval(b: &mut Vec<u8>, sec: u32, usec: u32) {
    b.push(T_TIMEVAL);
    b.extend(be32(sec));
    b.extend(be32(usec));
}
fn put_bool(b: &mut Vec<u8>, v: bool) {
    b.push(if v { T_TRUE } else { T_FALSE });
}
fn put_str(b: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        None => b.push(T_STRING_NULL),
        Some(s) => {
            b.push(T_STRING);
            b.extend(s);
            b.push(0);
        }
    }
}
fn put_arb(b: &mut Vec<u8>, data: &[u8]) {
    b.push(T_ARBITRARY);
    b.extend(be32(data.len() as u32));
    b.extend(data);
}
fn put_ss(b: &mut Vec<u8>, format: u8, channels: u8, rate: u32) {
    b.extend([T_SAMPLE_SPEC, format, channels]);
    b.extend(be32(rate));
}
fn put_cmap(b: &mut Vec<u8>, pos: &[u8]) {
    b.extend([T_CHANNEL_MAP, pos.len() as u8]);
    b.extend(pos);
}
fn put_cvol(b: &mut Vec<u8>, v: &[u32]) {
    b.extend([T_CVOLUME, v.len() as u8]);
    for x in v {
        b.extend(be32(*x));
    }
}
/// `kv`: key and value pairs; the value travels with its trailing 0, as libpulse does.
fn put_proplist(b: &mut Vec<u8>, kv: &[(&[u8], &[u8])]) {
    b.push(T_PROPLIST);
    for (key, value) in kv {
        put_str(b, Some(key));
        put_u32(b, value.len() as u32 + 1);
        let mut with_nul = value.to_vec();
        with_nul.push(0);
        put_arb(b, &with_nul);
    }
    put_str(b, None);
}
fn put_format_pcm(b: &mut Vec<u8>) {
    b.push(T_FORMAT_INFO);
    put_u8(b, 1); // PA_ENCODING_PCM
    put_proplist(b, &[]);
}

/// A control packet under construction: 20 bytes of descriptor, then the values.
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: vec![0; PA_DESCRIPTOR_SIZE] }
    }
    pub fn u32(&mut self, v: u32) {
        put_u32(&mut self.buf, v)
    }
    pub fn u8(&mut self, v: u8) {
        put_u8(&mut self.buf, v)
    }
    pub fn u64(&mut self, v: u64) {
        put_u64(&mut self.buf, v)
    }
    pub fn timeval(&mut self, sec: u32, usec: u32) {
        put_timeval(&mut self.buf, sec, usec)
    }
    pub fn bool(&mut self, v: bool) {
        put_bool(&mut self.buf, v)
    }
    pub fn str(&mut self, s: Option<&str>) {
        put_str(&mut self.buf, s.map(str::as_bytes))
    }
    /// A string given as bytes (a device name need not be UTF-8).
    pub fn str_bytes(&mut self, s: Option<&[u8]>) {
        put_str(&mut self.buf, s)
    }
    pub fn arb(&mut self, data: &[u8]) {
        put_arb(&mut self.buf, data)
    }
    pub fn ss(&mut self, format: u8, channels: u8, rate: u32) {
        put_ss(&mut self.buf, format, channels, rate)
    }
    pub fn cmap(&mut self, pos: &[u8]) {
        put_cmap(&mut self.buf, pos)
    }
    pub fn cvol(&mut self, v: &[u32]) {
        put_cvol(&mut self.buf, v)
    }
    /// A single volume ('V').
    pub fn vol(&mut self, v: u32) {
        self.buf.push(T_VOLUME);
        self.buf.extend(be32(v));
    }
    pub fn proplist(&mut self, kv: &[(&str, &str)]) {
        let pairs: Vec<(&[u8], &[u8])> = kv.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes())).collect();
        put_proplist(&mut self.buf, &pairs)
    }
    pub fn format_pcm(&mut self) {
        put_format_pcm(&mut self.buf)
    }
    /// Closes the control packet (channel -1) by filling in the descriptor.
    pub fn finish(mut self) -> Vec<u8> {
        let len = (self.buf.len() - PA_DESCRIPTOR_SIZE) as u32;
        self.buf[0..4].copy_from_slice(&be32(len));
        self.buf[4..8].copy_from_slice(&be32(0xffff_ffff));
        self.buf[8..20].fill(0);
        self.buf
    }
}

/// A data packet (memblock) for a channel: descriptor and bytes.
pub fn data_packet(channel: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PA_DESCRIPTOR_SIZE + data.len());
    out.extend(be32(data.len() as u32));
    out.extend(be32(channel));
    out.extend([0u8; 12]);
    out.extend(data);
    out
}

/// A descriptor: length, channel, offset and flags.
pub fn descriptor(d: &[u8]) -> (u32, u32, u64, u32) {
    (get_be32(d), get_be32(&d[4..]), u64::from(get_be32(&d[8..])) << 32 | u64::from(get_be32(&d[12..])), get_be32(&d[16..]))
}

// ---- reading ----

/// A reader of a tagstruct. A wrong type or the end of the data sets `err`; every read after that
/// gives zero, so the caller can read a whole record and check once.
pub struct Reader<'a> {
    pub d: &'a [u8],
    pub pos: usize,
    pub err: bool,
}

impl<'a> Reader<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Self { d, pos: 0, err: false }
    }
    pub fn eof(&self) -> bool {
        self.pos >= self.d.len()
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.err || self.d.len() - self.pos.min(self.d.len()) < n {
            self.err = true;
            return None;
        }
        let p = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Some(p)
    }
    fn expect(&mut self, tag: u8) -> bool {
        match self.take(1) {
            None => false,
            Some(p) if p[0] == tag => true,
            Some(_) => {
                self.err = true;
                false
            }
        }
    }
    pub fn u32(&mut self) -> u32 {
        if !self.expect(T_U32) {
            return 0;
        }
        self.take(4).map_or(0, get_be32)
    }
    pub fn u8(&mut self) -> u8 {
        if !self.expect(T_U8) {
            return 0;
        }
        self.take(1).map_or(0, |p| p[0])
    }
    /// 'U', 'R' or 'r'.
    pub fn u64(&mut self) -> u64 {
        let Some(tag) = self.take(1) else { return 0 };
        if !matches!(tag[0], T_USEC | T_U64 | T_S64) {
            self.err = true;
            return 0;
        }
        self.take(8).map_or(0, |p| u64::from(get_be32(p)) << 32 | u64::from(get_be32(&p[4..])))
    }
    pub fn bool(&mut self) -> bool {
        let Some(p) = self.take(1) else { return false };
        if p[0] == T_TRUE {
            return true;
        }
        if p[0] != T_FALSE {
            self.err = true;
        }
        false
    }
    /// A string without its terminator, pointing into the buffer; `None` for 'N' or an error.
    pub fn str(&mut self) -> Option<&'a [u8]> {
        let tag = self.take(1)?;
        if tag[0] == T_STRING_NULL {
            return None;
        }
        if tag[0] != T_STRING {
            self.err = true;
            return None;
        }
        let rest = &self.d[self.pos..];
        let Some(z) = rest.iter().position(|&b| b == 0) else {
            self.err = true;
            return None;
        };
        self.pos += z + 1;
        Some(&rest[..z])
    }
    pub fn arb(&mut self) -> Option<&'a [u8]> {
        if !self.expect(T_ARBITRARY) {
            return None;
        }
        let len = get_be32(self.take(4)?) as usize;
        self.take(len)
    }
    pub fn ss(&mut self) -> (u8, u8, u32) {
        if !self.expect(T_SAMPLE_SPEC) {
            return (0, 0, 0);
        }
        self.take(6).map_or((0, 0, 0), |p| (p[0], p[1], get_be32(&p[2..])))
    }
    pub fn cmap(&mut self, out: &mut [u8; PA_CHANNELS_MAX]) -> u8 {
        if !self.expect(T_CHANNEL_MAP) {
            return 0;
        }
        let Some(p) = self.take(1) else { return 0 };
        if p[0] as usize > PA_CHANNELS_MAX {
            self.err = true;
            return 0;
        }
        let n = p[0] as usize;
        let Some(d) = self.take(n) else { return 0 };
        out[..n].copy_from_slice(d);
        n as u8
    }
    pub fn cvol(&mut self, out: &mut [u32; PA_CHANNELS_MAX]) -> u8 {
        if !self.expect(T_CVOLUME) {
            return 0;
        }
        let Some(p) = self.take(1) else { return 0 };
        if p[0] as usize > PA_CHANNELS_MAX {
            self.err = true;
            return 0;
        }
        let n = p[0] as usize;
        let Some(d) = self.take(4 * n) else { return 0 };
        for i in 0..n {
            out[i] = get_be32(&d[4 * i..]);
        }
        n as u8
    }
    pub fn vol(&mut self) -> u32 {
        if !self.expect(T_VOLUME) {
            return 0;
        }
        self.take(4).map_or(0, get_be32)
    }
    /// Calls `f(key, value, terminated)` for each property; a value comes without its terminator, and
    /// `terminated` says whether the buffer holds one right after it. A value that is not terminated and
    /// does not fit 512 bytes is not reported.
    pub fn proplist(&mut self, mut f: impl FnMut(&'a [u8], &'a [u8], bool)) {
        if !self.expect(T_PROPLIST) {
            return;
        }
        for _ in 0..4096 {
            if self.err {
                return;
            }
            let Some(key) = self.str() else { return }; // 'N' ends the list
            if self.err {
                return;
            }
            let n = self.u32() as usize;
            let val = self.arb();
            let got = val.map_or(0, <[u8]>::len);
            if self.err || got != n {
                self.err = true;
                return;
            }
            let val = val.unwrap_or(&[]);
            if got > 0 && val[got - 1] == 0 {
                f(key, &val[..got - 1], true);
            } else if got < 512 {
                f(key, val, false);
            }
        }
        self.err = true;
    }
    pub fn skip_format(&mut self) {
        if !self.expect(T_FORMAT_INFO) {
            return;
        }
        self.u8();
        self.proplist(|_, _, _| {});
    }
    /// Any value (to skip the fields we don't use).
    pub fn skip(&mut self) {
        if self.err || self.eof() {
            self.err = true;
            return;
        }
        let mut map = [0u8; PA_CHANNELS_MAX];
        let mut vol = [0u32; PA_CHANNELS_MAX];
        match self.d[self.pos] {
            T_STRING | T_STRING_NULL => {
                self.str();
            }
            T_U32 => {
                self.u32();
            }
            T_U8 => {
                self.u8();
            }
            T_U64 | T_S64 | T_USEC => {
                self.u64();
            }
            T_TRUE | T_FALSE => {
                self.bool();
            }
            T_SAMPLE_SPEC => {
                self.ss();
            }
            T_ARBITRARY => {
                self.arb();
            }
            T_TIMEVAL => {
                self.take(9);
            }
            T_CHANNEL_MAP => {
                self.cmap(&mut map);
            }
            T_CVOLUME => {
                self.cvol(&mut vol);
            }
            T_PROPLIST => self.proplist(|_, _, _| {}),
            T_VOLUME => {
                self.vol();
            }
            T_FORMAT_INFO => self.skip_format(),
            _ => self.err = true,
        }
    }
}

// ---- the C ABI of pulse.h ----

#[repr(C)]
pub struct TsOut {
    d: *mut u8,
    len: usize,
    cap: usize,
    err: c_int,
}

#[repr(C)]
pub struct TsIn {
    d: *const u8,
    len: usize,
    pos: usize,
    err: c_int,
}

impl TsOut {
    /// The packet bytes (for tests and for Rust callers of the C interface).
    pub fn d(&self) -> *const u8 {
        self.d
    }
}

pub type TsPropFn = Option<unsafe extern "C" fn(ud: *mut c_void, key: *const c_char, val: *const c_char, len: u32)>;

/// Appends bytes to a `TsOut`, growing its malloc'd buffer as the C code did.
unsafe fn append(t: &mut TsOut, data: &[u8]) {
    if t.err != 0 {
        return;
    }
    if t.len + data.len() > t.cap {
        let mut cap = if t.cap > 0 { t.cap * 2 } else { 256 };
        while cap < t.len + data.len() {
            cap *= 2;
        }
        let d = libc::realloc(t.d as *mut c_void, cap) as *mut u8;
        if d.is_null() {
            t.err = 1;
            return;
        }
        t.d = d;
        t.cap = cap;
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), t.d.add(t.len), data.len());
    t.len += data.len();
}

unsafe fn out(t: *mut TsOut, f: impl FnOnce(&mut Vec<u8>)) {
    let mut bytes = Vec::new();
    f(&mut bytes);
    append(&mut *t, &bytes);
}

#[no_mangle]
pub unsafe extern "C" fn ts_init(t: *mut TsOut) {
    std::ptr::write(t, TsOut { d: std::ptr::null_mut(), len: 0, cap: 0, err: 0 });
    append(&mut *t, &[0u8; PA_DESCRIPTOR_SIZE]);
}
#[no_mangle]
pub unsafe extern "C" fn ts_free(t: *mut TsOut) {
    libc::free((*t).d as *mut c_void);
    std::ptr::write(t, TsOut { d: std::ptr::null_mut(), len: 0, cap: 0, err: 0 });
}
#[no_mangle]
pub unsafe extern "C" fn ts_u32(t: *mut TsOut, v: u32) {
    out(t, |b| put_u32(b, v))
}
#[no_mangle]
pub unsafe extern "C" fn ts_u8(t: *mut TsOut, v: u8) {
    out(t, |b| put_u8(b, v))
}
#[no_mangle]
pub unsafe extern "C" fn ts_u64(t: *mut TsOut, v: u64) {
    out(t, |b| put_u64(b, v))
}
#[no_mangle]
pub unsafe extern "C" fn ts_bool(t: *mut TsOut, v: c_int) {
    out(t, |b| put_bool(b, v != 0))
}
#[no_mangle]
pub unsafe extern "C" fn ts_timeval(t: *mut TsOut, sec: u32, usec: u32) {
    out(t, |b| put_timeval(b, sec, usec))
}
#[no_mangle]
pub unsafe extern "C" fn ts_str(t: *mut TsOut, s: *const c_char) {
    let text = (!s.is_null()).then(|| std::ffi::CStr::from_ptr(s).to_bytes());
    out(t, |b| put_str(b, text))
}
#[no_mangle]
pub unsafe extern "C" fn ts_arb(t: *mut TsOut, p: *const c_void, n: u32) {
    let data = if n == 0 { &[][..] } else { std::slice::from_raw_parts(p as *const u8, n as usize) };
    out(t, |b| put_arb(b, data))
}
#[no_mangle]
pub unsafe extern "C" fn ts_ss(t: *mut TsOut, format: u8, channels: u8, rate: u32) {
    out(t, |b| put_ss(b, format, channels, rate))
}
#[no_mangle]
pub unsafe extern "C" fn ts_cmap(t: *mut TsOut, n: u8, pos: *const u8) {
    out(t, |b| put_cmap(b, std::slice::from_raw_parts(pos, n as usize)))
}
#[no_mangle]
pub unsafe extern "C" fn ts_cvol(t: *mut TsOut, n: u8, v: *const u32) {
    out(t, |b| put_cvol(b, std::slice::from_raw_parts(v, n as usize)))
}
#[no_mangle]
pub unsafe extern "C" fn ts_proplist(t: *mut TsOut, kv: *const *const c_char, pairs: c_int) {
    let mut list: Vec<(&[u8], &[u8])> = Vec::new();
    for i in 0..pairs.max(0) as usize {
        list.push((std::ffi::CStr::from_ptr(*kv.add(2 * i)).to_bytes(), std::ffi::CStr::from_ptr(*kv.add(2 * i + 1)).to_bytes()));
    }
    out(t, |b| put_proplist(b, &list))
}
#[no_mangle]
pub unsafe extern "C" fn ts_format_pcm(t: *mut TsOut) {
    out(t, put_format_pcm)
}
/// Closes the control packet: fills in the descriptor. Returns the total length (0 on error).
#[no_mangle]
pub unsafe extern "C" fn ts_finish(t: *mut TsOut) -> usize {
    let t = &mut *t;
    if t.err != 0 || t.len < PA_DESCRIPTOR_SIZE {
        return 0;
    }
    let d = std::slice::from_raw_parts_mut(t.d, t.len);
    d[0..4].copy_from_slice(&be32((t.len - PA_DESCRIPTOR_SIZE) as u32));
    d[4..8].copy_from_slice(&be32(0xffff_ffff));
    d[8..20].fill(0);
    t.len
}
#[no_mangle]
pub unsafe extern "C" fn ts_data_packet(channel: u32, p: *const c_void, n: u32, total: *mut usize) -> *mut u8 {
    let data = if n == 0 { &[][..] } else { std::slice::from_raw_parts(p as *const u8, n as usize) };
    let packet = data_packet(channel, data);
    let out = libc::malloc(packet.len().max(1)) as *mut u8;
    if out.is_null() {
        return out;
    }
    std::ptr::copy_nonoverlapping(packet.as_ptr(), out, packet.len());
    *total = packet.len();
    out
}
#[no_mangle]
pub unsafe extern "C" fn pa_descriptor_read(d: *const u8, len: *mut u32, channel: *mut u32, offset: *mut u64, flags: *mut u32) {
    let (l, c, o, f) = descriptor(std::slice::from_raw_parts(d, PA_DESCRIPTOR_SIZE));
    *len = l;
    *channel = c;
    *offset = o;
    *flags = f;
}

/// Runs a read on the `TsIn` as a `Reader` and stores the position and the error flag back.
unsafe fn read<R>(t: *mut TsIn, f: impl FnOnce(&mut Reader<'static>) -> R) -> R {
    let st = &mut *t;
    let data: &'static [u8] = if st.len == 0 { &[] } else { std::slice::from_raw_parts(st.d, st.len) };
    let mut r = Reader { d: data, pos: st.pos, err: st.err != 0 };
    let result = f(&mut r);
    st.pos = r.pos;
    st.err = c_int::from(r.err);
    result
}

#[no_mangle]
pub unsafe extern "C" fn ts_in(t: *mut TsIn, d: *const u8, len: usize) {
    std::ptr::write(t, TsIn { d, len, pos: 0, err: 0 });
}
#[no_mangle]
pub unsafe extern "C" fn ts_eof(t: *const TsIn) -> c_int {
    c_int::from((*t).pos >= (*t).len)
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_u32(t: *mut TsIn) -> u32 {
    read(t, |r| r.u32())
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_u8(t: *mut TsIn) -> u8 {
    read(t, |r| r.u8())
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_u64(t: *mut TsIn) -> u64 {
    read(t, |r| r.u64())
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_bool(t: *mut TsIn) -> c_int {
    read(t, |r| c_int::from(r.bool()))
}
/// The string points into the buffer (which holds its terminator); NULL for 'N' or an error.
#[no_mangle]
pub unsafe extern "C" fn ts_get_str(t: *mut TsIn) -> *const c_char {
    read(t, |r| r.str().map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char))
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_arb(t: *mut TsIn, n: *mut u32) -> *const u8 {
    *n = 0;
    read(t, |r| match r.arb() {
        Some(d) => {
            *n = d.len() as u32;
            d.as_ptr()
        }
        None => std::ptr::null(),
    })
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_ss(t: *mut TsIn, format: *mut u8, channels: *mut u8, rate: *mut u32) {
    let (f, c, r) = read(t, |r| r.ss());
    *format = f;
    *channels = c;
    *rate = r;
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_cmap(t: *mut TsIn, pos: *mut u8) -> u8 {
    let mut map = [0u8; PA_CHANNELS_MAX];
    let n = read(t, |r| r.cmap(&mut map));
    std::ptr::copy_nonoverlapping(map.as_ptr(), pos, n as usize);
    n
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_cvol(t: *mut TsIn, v: *mut u32) -> u8 {
    let mut vol = [0u32; PA_CHANNELS_MAX];
    let n = read(t, |r| r.cvol(&mut vol));
    std::ptr::copy_nonoverlapping(vol.as_ptr(), v, n as usize);
    n
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_vol(t: *mut TsIn) -> u32 {
    read(t, |r| r.vol())
}
#[no_mangle]
pub unsafe extern "C" fn ts_get_proplist(t: *mut TsIn, f: TsPropFn, ud: *mut c_void) {
    let st = &mut *t;
    let data: &[u8] = if st.len == 0 { &[] } else { std::slice::from_raw_parts(st.d, st.len) };
    let mut r = Reader { d: data, pos: st.pos, err: st.err != 0 };
    // the callbacks take C strings: a value that came without its terminator is copied to end it
    let mut found: Vec<(&[u8], &[u8], bool)> = Vec::new();
    r.proplist(|key, val, terminated| found.push((key, val, terminated)));
    st.pos = r.pos;
    st.err = c_int::from(r.err);
    if let Some(f) = f {
        for (key, val, terminated) in found {
            if terminated {
                f(ud, key.as_ptr() as *const c_char, val.as_ptr() as *const c_char, val.len() as u32);
            } else {
                let mut tmp = [0u8; 512];
                tmp[..val.len()].copy_from_slice(val);
                f(ud, key.as_ptr() as *const c_char, tmp.as_ptr() as *const c_char, val.len() as u32);
            }
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn ts_skip_format(t: *mut TsIn) {
    read(t, |r| r.skip_format())
}
#[no_mangle]
pub unsafe extern "C" fn ts_skip(t: *mut TsIn) {
    read(t, |r| r.skip())
}
