//! Axia's sound client.
//!
//! Speaks the PulseAudio native protocol with pipewire-pulse (the part of PipeWire that serves it
//! at $XDG_RUNTIME_DIR/pulse/native): no libpulse or libpipewire. Keeps a copy of the outputs
//! (sinks), inputs (sources), apps playing (sink inputs) and apps recording (source outputs);
//! subscribes to changes and notifies JS with "state" when something changed and all replies have
//! arrived. If PipeWire restarts, it reconnects every 2 s.
//!
//! Extras: level meter for an input (a normal 48 kHz recording stream; the level is the largest
//! sample of each block, see `meter`) and system sounds synthesized here (uploaded once to the
//! server's sample cache and then played with PLAY_SAMPLE).
//!
//! The C ABI of server/src/sound/snd.h is at the end of the file. The state is one process-wide
//! value, as the C client's was; it is never locked while a callback runs.

use crate::pulse::*;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::os::fd::IntoRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::sync::{LazyLock, Mutex, MutexGuard};

const PROTOCOL_VERSION: u32 = 32; // ports with "available", no availability groups (v34)
const MAX_DEV: usize = 32;
const MAX_STREAMS: usize = 64;
const MAX_PORTS: usize = 16;
const MAX_PENDING: usize = 256;

pub const SND_SINK: c_int = 0;
pub const SND_SOURCE: c_int = 1;
pub const SND_INPUT: c_int = 2;
pub const SND_OUTPUT: c_int = 3;

/// Text copied into a fixed-size C field: cut to `cap - 1` bytes.
fn copy(src: Option<&[u8]>, cap: usize) -> Vec<u8> {
    let s = src.unwrap_or(&[]);
    s[..s.len().min(cap - 1)].to_vec()
}

#[derive(Clone, Default)]
struct Port {
    name: Vec<u8>,
    desc: Vec<u8>,
    priority: u32,
    available: u32,
}

#[derive(Clone)]
struct Dev {
    index: u32,
    monitor: u32,
    card: u32,
    state: u32,
    base: u32, // monitor: for a sink, its source; for a source, its sink
    name: Vec<u8>,
    desc: Vec<u8>,
    icon: Vec<u8>,
    form: Vec<u8>,
    bus: Vec<u8>,
    ch: u8,
    map: [u8; PA_CHANNELS_MAX],
    vol: [u32; PA_CHANNELS_MAX],
    mute: bool,
    ports: Vec<Port>,
    active: Vec<u8>,
}

impl Default for Dev {
    fn default() -> Self {
        Self {
            index: 0,
            monitor: 0,
            card: 0,
            state: 0,
            base: 0,
            name: vec![],
            desc: vec![],
            icon: vec![],
            form: vec![],
            bus: vec![],
            ch: 0,
            map: [0; PA_CHANNELS_MAX],
            vol: [0; PA_CHANNELS_MAX],
            mute: false,
            ports: vec![],
            active: vec![],
        }
    }
}

#[derive(Clone)]
struct Stream {
    index: u32,
    client: u32,
    dev: u32,
    pid: u32,
    name: Vec<u8>,
    app: Vec<u8>,
    icon: Vec<u8>,
    binary: Vec<u8>,
    role: Vec<u8>,
    appid: Vec<u8>,
    ch: u8,
    vol: [u32; PA_CHANNELS_MAX],
    mute: bool,
    corked: bool,
    has_volume: bool,
    writable: bool,
}

impl Default for Stream {
    fn default() -> Self {
        Self {
            index: 0,
            client: 0,
            dev: 0,
            pid: 0,
            name: vec![],
            app: vec![],
            icon: vec![],
            binary: vec![],
            role: vec![],
            appid: vec![],
            ch: 0,
            vol: [0; PA_CHANNELS_MAX],
            mute: false,
            corked: false,
            has_volume: false,
            writable: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    None,
    Auth,
    Name,
    Server,
    List,
    One,
    Ack,
    Record,
    Upload,
}

#[derive(Clone, Copy)]
struct Pending {
    tag: u32,
    kind: Kind,
    what: u32,
    arg: u32,
}

const EMPTY: Pending = Pending { tag: 0, kind: Kind::None, what: 0, arg: 0 };

struct State {
    fd: c_int,
    timer: c_int, // socket; reconnection timer
    connected: bool,
    version: u32,
    tag: u32,
    pend: Vec<Pending>,
    info_pending: i32,
    dirty: bool,
    inbuf: Vec<u8>, // incoming bytes; `in_len` of them are used
    in_len: usize,
    out: Vec<u8>, // what's left to write
    server_name: Vec<u8>,
    server_version: Vec<u8>,
    default_sink: Vec<u8>,
    default_source: Vec<u8>,
    sinks: Vec<Dev>,
    sources: Vec<Dev>,
    inputs: Vec<Stream>,
    outputs: Vec<Stream>,
    meter_source: u32,
    meter_channel: u32,
    peak: f64,
    peak_ready: bool,
    debug: bool, // LORD_SND_DEBUG=1: every packet to stderr
    samples_uploaded: u32,
    uploading: u32,
    play_after: u32, // bits: pop, notify, error
}

impl State {
    fn new() -> Self {
        Self {
            fd: -1,
            timer: -1,
            connected: false,
            version: 0,
            tag: 0,
            pend: vec![EMPTY; MAX_PENDING],
            info_pending: 0,
            dirty: false,
            inbuf: Vec::new(),
            in_len: 0,
            out: Vec::new(),
            server_name: vec![],
            server_version: vec![],
            default_sink: vec![],
            default_source: vec![],
            sinks: vec![],
            sources: vec![],
            inputs: vec![],
            outputs: vec![],
            meter_source: PA_INVALID_INDEX,
            meter_channel: PA_INVALID_INDEX,
            peak: 0.0,
            peak_ready: false,
            debug: false,
            samples_uploaded: 0,
            uploading: 0,
            play_after: 0,
        }
    }
}

// ---- I/O ----

impl State {
    fn flush_out(&mut self) -> i32 {
        while !self.out.is_empty() {
            // SAFETY: a plain send of our own buffer on our own descriptor.
            let w = unsafe { libc::send(self.fd, self.out.as_ptr() as *const c_void, self.out.len(), libc::MSG_NOSIGNAL) };
            if w < 0 {
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::EAGAIN) => return 0,
                    _ => return -1,
                }
            }
            if w == 0 {
                return -1;
            }
            self.out.drain(..w as usize);
        }
        0
    }

    fn queue_bytes(&mut self, p: &[u8]) -> i32 {
        if self.fd < 0 {
            return -1;
        }
        if self.out.len() + p.len() > PA_FRAME_MAX {
            return -1;
        }
        self.out.extend_from_slice(p);
        if self.flush_out() != 0 {
            self.disconnect("write error");
            return -1;
        }
        0
    }

    fn pend_add(&mut self, tag: u32, kind: Kind, what: u32, arg: u32) -> bool {
        for slot in self.pend.iter_mut() {
            if slot.kind == Kind::None {
                *slot = Pending { tag, kind, what, arg };
                if matches!(kind, Kind::List | Kind::One | Kind::Server) {
                    self.info_pending += 1;
                }
                return true;
            }
        }
        false
    }

    /// Starts a command: a tagstruct with the command and a new tag.
    fn cmd_begin(&mut self, cmd: u32) -> (Writer, u32) {
        let mut w = Writer::new();
        let tag = self.tag;
        self.tag = self.tag.wrapping_add(1);
        w.u32(cmd);
        w.u32(tag);
        (w, tag)
    }

    fn cmd_send(&mut self, w: Writer, tag: u32, kind: Kind, what: u32, arg: u32) -> i32 {
        let packet = w.finish();
        if self.pend_add(tag, kind, what, arg) {
            self.queue_bytes(&packet)
        } else {
            -1
        }
    }
}

// ---- model ----

fn is_list_cmd(what: u32) -> bool {
    matches!(what, PA_CMD_GET_SINK_INFO_LIST | PA_CMD_GET_SOURCE_INFO_LIST | PA_CMD_GET_SINK_INPUT_INFO_LIST | PA_CMD_GET_SOURCE_OUTPUT_INFO_LIST)
}

fn is_dev_cmd(what: u32) -> bool {
    matches!(what, PA_CMD_GET_SINK_INFO | PA_CMD_GET_SINK_INFO_LIST | PA_CMD_GET_SOURCE_INFO | PA_CMD_GET_SOURCE_INFO_LIST)
}

fn parse_dev(r: &mut Reader) -> Option<Dev> {
    let mut d = Dev { index: r.u32(), ..Dev::default() };
    d.name = copy(r.str(), 256);
    d.desc = copy(r.str(), 256);
    r.ss();
    let mut map = [0u8; PA_CHANNELS_MAX];
    r.cmap(&mut map);
    d.map = map;
    r.u32(); // module
    let mut vol = [0u32; PA_CHANNELS_MAX];
    d.ch = r.cvol(&mut vol);
    d.vol = vol;
    d.mute = r.bool();
    d.monitor = r.u32();
    r.str(); // monitor name
    r.u64(); // latency
    r.str(); // driver
    r.u32(); // flags
    let (mut icon, mut form, mut bus) = (vec![], vec![], vec![]);
    r.proplist(|key, val, _| match key {
        b"device.icon_name" => icon = copy(Some(val), 96),
        b"device.form_factor" => form = copy(Some(val), 32),
        b"device.bus" => bus = copy(Some(val), 32),
        _ => {}
    });
    (d.icon, d.form, d.bus) = (icon, form, bus);
    r.u64(); // requested latency
    d.base = r.vol();
    d.state = r.u32();
    r.u32(); // volume steps
    d.card = r.u32();
    let np = r.u32();
    for _ in 0..np {
        if r.err {
            break;
        }
        let p = Port { name: copy(r.str(), 128), desc: copy(r.str(), 128), priority: r.u32(), available: r.u32() }; // v24: 0 unknown, 1 no, 2 yes
        if d.ports.len() < MAX_PORTS {
            d.ports.push(p);
        }
    }
    d.active = copy(r.str(), 128);
    let nf = r.u8(); // formats (v21 sinks, v22 sources)
    for _ in 0..nf {
        if r.err {
            break;
        }
        r.skip_format();
    }
    (!r.err).then_some(d)
}

fn parse_stream(r: &mut Reader, output: bool) -> Option<Stream> {
    let mut s = Stream { index: r.u32(), ..Stream::default() };
    s.name = copy(r.str(), 256);
    r.u32(); // module
    s.client = r.u32();
    s.dev = r.u32();
    r.ss();
    let mut map = [0u8; PA_CHANNELS_MAX];
    r.cmap(&mut map);
    if !output {
        s.ch = r.cvol(&mut s.vol);
    }
    r.u64();
    r.u64();
    r.str(); // resample method
    r.str(); // driver
    if !output {
        s.mute = r.bool();
    }
    let (mut app, mut icon, mut binary, mut appid, mut role) = (vec![], vec![], vec![], vec![], vec![]);
    let mut pid = 0u32;
    let mut media_icon: Option<Vec<u8>> = None;
    r.proplist(|key, val, _| match key {
        b"application.name" => app = copy(Some(val), 128),
        b"application.icon_name" => icon = copy(Some(val), 96),
        b"application.process.binary" => binary = copy(Some(val), 64),
        b"application.process.id" => pid = parse_ulong(val),
        b"application.id" => appid = copy(Some(val), 128),
        b"media.role" => role = copy(Some(val), 32),
        b"media.icon_name" => {
            if icon.is_empty() {
                media_icon = Some(copy(Some(val), 96));
            }
        }
        _ => {}
    });
    // the C callback ran in order: `media.icon_name` only fills an icon still empty at that point
    if let Some(m) = media_icon {
        if icon.is_empty() {
            icon = m;
        }
    }
    (s.app, s.icon, s.binary, s.appid, s.role, s.pid) = (app, icon, binary, appid, role, pid);
    s.corked = r.bool();
    if output {
        s.ch = r.cvol(&mut s.vol);
        s.mute = r.bool();
    }
    s.has_volume = r.bool();
    s.writable = r.bool();
    r.skip_format();
    (!r.err).then_some(s)
}

/// `strtoul(val, NULL, 10)` truncated to 32 bits.
fn parse_ulong(val: &[u8]) -> u32 {
    let text = String::from_utf8_lossy(val);
    let text = text.trim_start();
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u64>().unwrap_or(0) as u32
}

impl State {
    fn dev_list(&mut self, what: u32) -> &mut Vec<Dev> {
        if what == PA_CMD_GET_SINK_INFO || what == PA_CMD_GET_SINK_INFO_LIST {
            &mut self.sinks
        } else {
            &mut self.sources
        }
    }

    fn on_info(&mut self, what: u32, r: &mut Reader) {
        let list = is_list_cmd(what);
        if is_dev_cmd(what) {
            let l = self.dev_list(what);
            if list {
                l.clear();
            }
            while !r.eof() && !r.err {
                let Some(d) = parse_dev(r) else { break };
                match l.iter().position(|x| x.index == d.index) {
                    Some(i) => l[i] = d,
                    None if l.len() < MAX_DEV => l.push(d),
                    None => {}
                }
            }
        } else {
            let inputs = what == PA_CMD_GET_SINK_INPUT_INFO || what == PA_CMD_GET_SINK_INPUT_INFO_LIST;
            let l = if inputs { &mut self.inputs } else { &mut self.outputs };
            if list {
                l.clear();
            }
            while !r.eof() && !r.err {
                let Some(s) = parse_stream(r, !inputs) else { break };
                match l.iter().position(|x| x.index == s.index) {
                    Some(i) => l[i] = s,
                    None if l.len() < MAX_STREAMS => l.push(s),
                    None => {}
                }
            }
        }
        if r.err {
            eprintln!("sound: reply {what} with unexpected format");
        }
        self.dirty = true;
    }

    fn remove_index(&mut self, facility: u32, index: u32) {
        // the last element takes the removed one's place, as in the C client
        fn drop_at<T>(l: &mut Vec<T>, at: usize) {
            l.swap_remove(at);
        }
        if facility == PA_SUB_SINK || facility == PA_SUB_SOURCE {
            let l = if facility == PA_SUB_SINK { &mut self.sinks } else { &mut self.sources };
            if let Some(i) = l.iter().position(|d| d.index == index) {
                drop_at(l, i);
            }
        } else {
            let l = if facility == PA_SUB_SINK_INPUT { &mut self.inputs } else { &mut self.outputs };
            if let Some(i) = l.iter().position(|s| s.index == index) {
                drop_at(l, i);
            }
        }
    }

    fn request_info(&mut self, cmd: u32, index: u32) {
        let (mut w, tag) = self.cmd_begin(cmd);
        let list = is_list_cmd(cmd);
        if !list {
            w.u32(index);
            if cmd == PA_CMD_GET_SINK_INFO || cmd == PA_CMD_GET_SOURCE_INFO {
                w.str(None);
            }
        }
        self.cmd_send(w, tag, if list { Kind::List } else { Kind::One }, cmd, index);
    }

    fn request_server(&mut self) {
        let (w, tag) = self.cmd_begin(PA_CMD_GET_SERVER_INFO);
        self.cmd_send(w, tag, Kind::Server, 0, 0);
    }

    fn request_all(&mut self) {
        self.request_server();
        self.request_info(PA_CMD_GET_SINK_INFO_LIST, 0);
        self.request_info(PA_CMD_GET_SOURCE_INFO_LIST, 0);
        self.request_info(PA_CMD_GET_SINK_INPUT_INFO_LIST, 0);
        self.request_info(PA_CMD_GET_SOURCE_OUTPUT_INFO_LIST, 0);
    }

    fn on_subscribe_event(&mut self, ev: u32, index: u32) {
        let facility = ev & PA_SUB_FACILITY_MASK;
        let kind = ev & PA_SUB_TYPE_MASK;
        const GET: [u32; 4] = [PA_CMD_GET_SINK_INFO, PA_CMD_GET_SOURCE_INFO, PA_CMD_GET_SINK_INPUT_INFO, PA_CMD_GET_SOURCE_OUTPUT_INFO];
        if facility == PA_SUB_SERVER {
            self.request_server();
            return;
        }
        if facility == PA_SUB_CARD {
            // the profile changed: the ports of its sinks/sources
            self.request_info(PA_CMD_GET_SINK_INFO_LIST, 0);
            self.request_info(PA_CMD_GET_SOURCE_INFO_LIST, 0);
            return;
        }
        if facility > PA_SUB_SOURCE_OUTPUT {
            return;
        }
        if kind == PA_SUB_REMOVE {
            self.remove_index(facility, index);
            self.dirty = true;
            return;
        }
        self.request_info(GET[facility as usize], index);
    }

    // ---- handshake ----

    fn send_client_name(&mut self) {
        let (mut w, tag) = self.cmd_begin(PA_CMD_SET_CLIENT_NAME);
        let pid = std::process::id().to_string();
        w.proplist(&[
            ("application.name", "LordOS"),
            ("application.id", "org.lordos.Server"),
            ("application.icon_name", "audio-card"),
            ("application.process.id", &pid),
            ("application.process.binary", "axia-server"),
        ]);
        self.cmd_send(w, tag, Kind::Name, 0, 0);
    }

    fn after_connected(&mut self) {
        let (mut w, tag) = self.cmd_begin(PA_CMD_SUBSCRIBE);
        w.u32(PA_SUB_MASK_SINK | PA_SUB_MASK_SOURCE | PA_SUB_MASK_SINK_INPUT | PA_SUB_MASK_SOURCE_OUTPUT | PA_SUB_MASK_SERVER | PA_SUB_MASK_CARD);
        self.cmd_send(w, tag, Kind::Ack, PA_CMD_SUBSCRIBE, 0);
        self.request_all();
        if self.meter_source != PA_INVALID_INDEX {
            let m = self.meter_source;
            self.meter_source = PA_INVALID_INDEX;
            self.meter(m);
        }
    }

    fn arm_timer(&mut self, on: bool) {
        if self.timer < 0 {
            // SAFETY: a plain timerfd.
            self.timer = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_NONBLOCK | libc::TFD_CLOEXEC) };
        }
        if self.timer < 0 {
            return;
        }
        let secs = if on { 2 } else { 0 };
        let spec = libc::itimerspec { it_interval: libc::timespec { tv_sec: secs, tv_nsec: 0 }, it_value: libc::timespec { tv_sec: secs, tv_nsec: 0 } };
        // SAFETY: `spec` is a valid itimerspec.
        unsafe { libc::timerfd_settime(self.timer, 0, &spec, std::ptr::null_mut()) };
    }

    fn try_connect(&mut self) -> i32 {
        let path = socket_path();
        // only the user's own server (the socket lives in their runtime folder)
        let Ok(meta) = std::fs::metadata(&path) else { return -1 };
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        if !std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) || meta.uid() != uid {
            return -1;
        }
        let Ok(stream) = UnixStream::connect(&path) else { return -1 };
        if stream.set_nonblocking(true).is_err() {
            return -1;
        }
        self.fd = stream.into_raw_fd();
        self.tag = 0;
        self.in_len = 0;
        self.out.clear();
        self.info_pending = 0;
        self.pend.iter_mut().for_each(|p| *p = EMPTY);
        self.samples_uploaded = 0;
        self.uploading = 0;
        self.play_after = 0;
        self.meter_channel = PA_INVALID_INDEX;

        // cookie: PulseAudio's if it exists (pipewire-pulse doesn't require it)
        let mut cookie = [0u8; 256];
        if let Some(home) = std::env::var_os("HOME") {
            let file = std::path::Path::new(&home).join(".config/pulse/cookie");
            if let Ok(mut f) = std::fs::File::open(file) {
                let _ = std::io::Read::read(&mut f, &mut cookie);
            }
        }
        let (mut w, tag) = self.cmd_begin(PA_CMD_AUTH);
        w.u32(PROTOCOL_VERSION); // no shared memory: data goes over the socket
        w.arb(&cookie);
        if self.cmd_send(w, tag, Kind::Auth, 0, 0) != 0 {
            return -1;
        }
        self.arm_timer(false);
        0
    }

    fn disconnect(&mut self, why: &str) {
        if self.fd >= 0 {
            // SAFETY: closing our own descriptor.
            unsafe { libc::close(self.fd) };
            self.fd = -1;
            if self.connected {
                eprintln!("sound: disconnected from PipeWire ({why}); retrying");
            }
        }
        self.connected = false;
        self.sinks.clear();
        self.sources.clear();
        self.inputs.clear();
        self.outputs.clear();
        self.server_name.clear();
        self.default_sink.clear();
        self.default_source.clear();
        self.dirty = true;
        self.arm_timer(true);
    }

    fn start(&mut self) -> i32 {
        self.debug = std::env::var_os("LORD_SND_DEBUG").is_some();
        if self.try_connect() != 0 {
            self.disconnect("no server");
            return -1;
        }
        0
    }

    fn stop(&mut self) {
        // SAFETY: closing our own descriptors.
        unsafe {
            if self.fd >= 0 {
                libc::close(self.fd);
            }
            if self.timer >= 0 {
                libc::close(self.timer);
            }
        }
        self.fd = -1;
        self.timer = -1;
        self.inbuf = Vec::new();
        self.out = Vec::new();
        self.in_len = 0;
    }

    fn poll_fd(&self) -> c_int {
        if self.fd >= 0 {
            self.fd
        } else {
            self.timer
        }
    }

    // ---- incoming packets ----

    fn on_reply(&mut self, p: Pending, r: &mut Reader) {
        match p.kind {
            Kind::Auth => {
                self.version = r.u32() & 0xffff;
                if self.version > PROTOCOL_VERSION {
                    self.version = PROTOCOL_VERSION;
                }
                if self.version < PROTOCOL_VERSION {
                    self.disconnect("old protocol version");
                    return;
                }
                self.send_client_name();
            }
            Kind::Name => {
                self.connected = true;
                self.dirty = true;
                eprintln!("sound: connected to PipeWire (protocol {})", self.version);
                self.after_connected();
            }
            Kind::Server => {
                self.server_name = copy(r.str(), 64);
                self.server_version = copy(r.str(), 32);
                r.str();
                r.str(); // user, host
                r.ss();
                self.default_sink = copy(r.str(), 256);
                self.default_source = copy(r.str(), 256);
                self.dirty = true;
            }
            Kind::List | Kind::One => self.on_info(p.what, r),
            Kind::Record => {
                let ch = r.u32();
                if r.err {
                    return;
                }
                if self.meter_source == PA_INVALID_INDEX || self.meter_source != p.arg {
                    // it was turned off in the meantime
                    let (mut w, tag) = self.cmd_begin(PA_CMD_DELETE_RECORD_STREAM);
                    w.u32(ch);
                    self.cmd_send(w, tag, Kind::Ack, PA_CMD_DELETE_RECORD_STREAM, 0);
                    return;
                }
                self.meter_channel = ch;
            }
            Kind::Upload => {
                let ch = r.u32();
                let _len = r.u32();
                if r.err || p.arg > 2 {
                    return;
                }
                self.upload_data(p.arg, ch);
            }
            Kind::Ack | Kind::None => {}
        }
    }

    fn on_error(&mut self, p: Pending, code: u32) {
        if p.kind == Kind::One {
            return; // it disappeared between the notice and the request
        }
        eprintln!("sound: command {}: error {code}", p.what);
        if matches!(p.kind, Kind::Auth | Kind::Name) {
            self.disconnect("connection refused");
        }
    }

    fn on_packet(&mut self, channel: u32, d: &[u8]) {
        if channel != 0xffff_ffff {
            // data: the meter (float32 samples; the level is the largest)
            if channel == self.meter_channel && d.len() >= 4 {
                let mut m = 0f32;
                for c in d.chunks_exact(4) {
                    let v = f32::from_ne_bytes([c[0], c[1], c[2], c[3]]).abs();
                    if v > m {
                        m = v;
                    }
                }
                self.peak = f64::from(if m > 1.0 { 1.0 } else { m });
                self.peak_ready = true;
            }
            return;
        }
        let mut r = Reader::new(d);
        let cmd = r.u32();
        let tag = r.u32();
        if r.err {
            return;
        }
        if self.debug {
            eprintln!("sound: <- command {cmd} tag {} ({} bytes)", tag as i32, d.len());
        }
        if cmd == PA_CMD_REPLY || cmd == PA_CMD_ERROR {
            for i in 0..MAX_PENDING {
                if self.pend[i].kind == Kind::None || self.pend[i].tag != tag {
                    continue;
                }
                let p = self.pend[i];
                self.pend[i].kind = Kind::None;
                if matches!(p.kind, Kind::List | Kind::One | Kind::Server) {
                    self.info_pending -= 1;
                }
                if cmd == PA_CMD_REPLY {
                    self.on_reply(p, &mut r);
                } else {
                    let code = r.u32();
                    self.on_error(p, code);
                }
                return;
            }
            return;
        }
        if cmd == PA_CMD_SUBSCRIBE_EVENT {
            let (ev, idx) = (r.u32(), r.u32());
            if !r.err {
                self.on_subscribe_event(ev, idx);
            }
            return;
        }
        if cmd == PA_CMD_RECORD_STREAM_KILLED {
            // the meter's source disappeared
            let ch = r.u32();
            if ch == self.meter_channel {
                self.meter_channel = PA_INVALID_INDEX;
                self.meter_source = PA_INVALID_INDEX;
            }
        }
        // REQUEST, STARTED, OVERFLOW...: we don't play our own streams (samples go to the cache)
    }

    fn dispatch(&mut self) {
        if self.fd < 0 {
            let mut x = 0u64;
            // SAFETY: reading the 8-byte expiration count of our timerfd.
            if self.timer >= 0 && unsafe { libc::read(self.timer, &mut x as *mut u64 as *mut c_void, 8) } > 0 {
                self.try_connect();
            }
            return;
        }
        loop {
            // keep 64 KiB of room, doubling the buffer as the C client did
            if self.inbuf.len() - self.in_len < 65536 {
                let mut cap = if self.inbuf.is_empty() { 65536 } else { self.inbuf.len() * 2 };
                while cap - self.in_len < 65536 {
                    cap *= 2;
                }
                if cap > PA_FRAME_MAX + 65536 {
                    self.disconnect("packet too large");
                    return;
                }
                self.inbuf.resize(cap, 0);
            }
            // SAFETY: receiving into the free part of our own buffer.
            let n = unsafe { libc::recv(self.fd, self.inbuf[self.in_len..].as_mut_ptr() as *mut c_void, self.inbuf.len() - self.in_len, 0) };
            if n < 0 {
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::EAGAIN) => break,
                    _ => {
                        self.disconnect("read error");
                        return;
                    }
                }
            }
            if n == 0 {
                self.disconnect("connection closed");
                return;
            }
            self.in_len += n as usize;
            let mut off = 0usize;
            while self.in_len - off >= PA_DESCRIPTOR_SIZE {
                let (len, ch, _offset, _flags) = descriptor(&self.inbuf[off..off + PA_DESCRIPTOR_SIZE]);
                if len as usize > PA_FRAME_MAX {
                    self.disconnect("invalid packet");
                    return;
                }
                if self.in_len - off < PA_DESCRIPTOR_SIZE + len as usize {
                    break;
                }
                let packet = self.inbuf[off + PA_DESCRIPTOR_SIZE..off + PA_DESCRIPTOR_SIZE + len as usize].to_vec();
                self.on_packet(ch, &packet);
                if self.fd < 0 {
                    return;
                }
                off += PA_DESCRIPTOR_SIZE + len as usize;
            }
            self.inbuf.copy_within(off..self.in_len, 0);
            self.in_len -= off;
        }
        if self.flush_out() != 0 {
            self.disconnect("write error");
        }
    }
}

fn socket_path() -> std::path::PathBuf {
    if let Ok(ps) = std::env::var("PULSE_SERVER") {
        if let Some(rest) = ps.strip_prefix("unix:") {
            return rest.into();
        }
    }
    let runtime = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(rt) if !rt.is_empty() => rt,
        // SAFETY: getuid has no preconditions.
        _ => format!("/run/user/{}", unsafe { libc::getuid() }),
    };
    format!("{runtime}/pulse/native").into()
}

// ---- state as JSON ----

fn bstr(b: &mut Vec<u8>, s: &[u8]) {
    b.push(b'"');
    for &c in s {
        match c {
            b'"' | b'\\' => {
                b.push(b'\\');
                b.push(c);
            }
            c if c < 0x20 => b.extend(format!("\\u{c:04x}").bytes()),
            c => b.push(c),
        }
    }
    b.push(b'"');
}

fn text(b: &mut Vec<u8>, s: &str) {
    b.extend(s.as_bytes());
}

fn bvol(b: &mut Vec<u8>, ch: u8, v: &[u32]) {
    text(b, "\"volume\":[");
    let mut top = 0f64;
    for i in 0..ch as usize {
        let x = f64::from(v[i]) / f64::from(PA_VOLUME_NORM);
        if x > top {
            top = x;
        }
        text(b, &format!("{}{x:.4}", if i > 0 { "," } else { "" }));
    }
    text(b, &format!("],\"level\":{top:.4}"));
}

fn bidx(b: &mut Vec<u8>, key: &str, v: u32) {
    if v == PA_INVALID_INDEX {
        text(b, &format!(",\"{key}\":-1"));
    } else {
        text(b, &format!(",\"{key}\":{v}"));
    }
}

fn flag(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

impl State {
    fn bdevs(&self, b: &mut Vec<u8>, list: &[Dev], source: bool) {
        for (i, d) in list.iter().enumerate() {
            text(b, &format!("{}{{\"index\":{},\"name\":", if i > 0 { "," } else { "" }, d.index));
            bstr(b, &d.name);
            text(b, ",\"description\":");
            bstr(b, if d.desc.is_empty() { &d.name } else { &d.desc });
            text(b, ",\"icon\":");
            bstr(b, &d.icon);
            text(b, ",\"formFactor\":");
            bstr(b, &d.form);
            text(b, ",\"bus\":");
            bstr(b, &d.bus);
            text(b, ",");
            bvol(b, d.ch, &d.vol);
            text(b, &format!(",\"mute\":{},\"state\":{}", flag(d.mute), d.state));
            bidx(b, if source { "monitorOf" } else { "monitor" }, d.monitor);
            bidx(b, "card", d.card);
            let default = if source { &self.default_source } else { &self.default_sink };
            text(b, &format!(",\"default\":{}", flag(d.name == *default)));
            text(b, ",\"ports\":[");
            for (k, p) in d.ports.iter().enumerate() {
                text(b, &format!("{}{{\"name\":", if k > 0 { "," } else { "" }));
                bstr(b, &p.name);
                text(b, ",\"description\":");
                bstr(b, &p.desc);
                let available = match p.available {
                    1 => "false",
                    2 => "true",
                    _ => "null",
                };
                text(b, &format!(",\"priority\":{},\"available\":{available}}}", p.priority));
            }
            text(b, "],\"activePort\":");
            bstr(b, &d.active);
            text(b, "}");
        }
    }

    fn bstreams(&self, b: &mut Vec<u8>, list: &[Stream], output: bool) {
        let mut first = true;
        for s in list {
            // our own streams (meter, system sounds) aren't shown: otherwise the microphone
            // meter would make it look like someone is recording
            if s.appid == b"org.lordos.Server" {
                continue;
            }
            text(b, &format!("{}{{\"index\":{},\"name\":", if first { "" } else { "," }, s.index));
            first = false;
            bstr(b, &s.name);
            text(b, ",\"app\":");
            bstr(b, if s.app.is_empty() { &s.binary } else { &s.app });
            text(b, ",\"appId\":");
            bstr(b, &s.appid);
            text(b, ",\"icon\":");
            bstr(b, &s.icon);
            text(b, ",\"binary\":");
            bstr(b, &s.binary);
            text(b, ",\"role\":");
            bstr(b, &s.role);
            text(b, &format!(",\"pid\":{}", s.pid));
            bidx(b, if output { "source" } else { "sink" }, s.dev);
            text(b, ",");
            bvol(b, s.ch, &s.vol);
            text(b, &format!(",\"mute\":{},\"corked\":{},\"hasVolume\":{},\"volumeWritable\":{}}}", flag(s.mute), flag(s.corked), flag(s.has_volume), flag(s.writable)));
        }
    }

    /// The whole state, as the JSON docs/SOUND.md describes.
    fn state_json(&self) -> Vec<u8> {
        let mut b = Vec::new();
        text(&mut b, &format!("{{\"connected\":{},\"server\":", flag(self.connected)));
        bstr(&mut b, &self.server_name);
        text(&mut b, ",\"version\":");
        bstr(&mut b, &self.server_version);
        text(&mut b, ",\"defaultSink\":");
        bstr(&mut b, &self.default_sink);
        text(&mut b, ",\"defaultSource\":");
        bstr(&mut b, &self.default_source);
        bidx(&mut b, "meter", self.meter_source);
        text(&mut b, ",\"sinks\":[");
        self.bdevs(&mut b, &self.sinks, false);
        text(&mut b, "],\"sources\":[");
        self.bdevs(&mut b, &self.sources, true);
        text(&mut b, "],\"inputs\":[");
        self.bstreams(&mut b, &self.inputs, false);
        text(&mut b, "],\"outputs\":[");
        self.bstreams(&mut b, &self.outputs, true);
        text(&mut b, "]}");
        b
    }

    /// What `snd_events` has to report: the state if it changed and every reply has arrived, and the peak.
    fn events(&mut self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        if self.fd >= 0 && !self.out.is_empty() && self.flush_out() != 0 {
            self.disconnect("write error");
        }
        let mut state = None;
        if self.dirty && self.info_pending <= 0 {
            self.dirty = false;
            state = Some(self.state_json());
        }
        let mut peak = None;
        if self.peak_ready {
            self.peak_ready = false;
            peak = Some(format!("{{\"source\":{},\"level\":{:.4}}}", self.meter_source, self.peak).into_bytes());
        }
        (state, peak)
    }

    // ---- commands ----

    fn find_vol(&self, kind: c_int, index: u32) -> Option<(u8, [u32; PA_CHANNELS_MAX])> {
        if kind == SND_SINK || kind == SND_SOURCE {
            let l = if kind == SND_SINK { &self.sinks } else { &self.sources };
            l.iter().find(|d| d.index == index).map(|d| (d.ch, d.vol))
        } else {
            let l = if kind == SND_INPUT { &self.inputs } else { &self.outputs };
            l.iter().find(|s| s.index == index).map(|s| (s.ch, s.vol))
        }
    }

    fn set_volume(&mut self, kind: c_int, index: u32, v: &[f64]) -> i32 {
        if !self.connected || v.is_empty() || v.len() > PA_CHANNELS_MAX || !(SND_SINK..=SND_OUTPUT).contains(&kind) {
            return -1;
        }
        let Some((ch, _)) = self.find_vol(kind, index) else { return -1 };
        if ch == 0 {
            return -1;
        }
        // fewer values: the last one is repeated
        let vol: Vec<u32> = (0..ch as usize).map(|i| to_vol(v[i.min(v.len() - 1)])).collect();
        const CMD: [u32; 4] = [PA_CMD_SET_SINK_VOLUME, PA_CMD_SET_SOURCE_VOLUME, PA_CMD_SET_SINK_INPUT_VOLUME, PA_CMD_SET_SOURCE_OUTPUT_VOLUME];
        let cmd = CMD[kind as usize];
        let (mut w, tag) = self.cmd_begin(cmd);
        w.u32(index);
        if kind == SND_SINK || kind == SND_SOURCE {
            w.str(None);
        }
        w.cvol(&vol);
        self.cmd_send(w, tag, Kind::Ack, cmd, index)
    }

    fn set_level(&mut self, kind: c_int, index: u32, level: f64) -> i32 {
        if !(SND_SINK..=SND_OUTPUT).contains(&kind) {
            return -1;
        }
        let Some((ch, cur)) = self.find_vol(kind, index) else { return -1 };
        if ch == 0 {
            return -1;
        }
        let top = cur[..ch as usize].iter().copied().max().unwrap_or(0);
        let v: Vec<f64> = (0..ch as usize).map(|i| if top > 0 { level * f64::from(cur[i]) / f64::from(top) } else { level }).collect();
        self.set_volume(kind, index, &v)
    }

    fn set_mute(&mut self, kind: c_int, index: u32, mute: bool) -> i32 {
        if !self.connected || !(SND_SINK..=SND_OUTPUT).contains(&kind) {
            return -1;
        }
        const CMD: [u32; 4] = [PA_CMD_SET_SINK_MUTE, PA_CMD_SET_SOURCE_MUTE, PA_CMD_SET_SINK_INPUT_MUTE, PA_CMD_SET_SOURCE_OUTPUT_MUTE];
        let cmd = CMD[kind as usize];
        let (mut w, tag) = self.cmd_begin(cmd);
        w.u32(index);
        if kind == SND_SINK || kind == SND_SOURCE {
            w.str(None);
        }
        w.bool(mute);
        self.cmd_send(w, tag, Kind::Ack, cmd, index)
    }

    fn dev_exists(&self, kind: c_int, name: Option<&[u8]>, index: u32) -> bool {
        let l = if kind == SND_SINK { &self.sinks } else { &self.sources };
        l.iter().any(|d| match name {
            Some(n) => d.name == n,
            None => d.index == index,
        })
    }

    fn set_default(&mut self, kind: c_int, name: &[u8]) -> i32 {
        if !self.connected || (kind != SND_SINK && kind != SND_SOURCE) || !self.dev_exists(kind, Some(name), 0) {
            return -1;
        }
        let cmd = if kind == SND_SINK { PA_CMD_SET_DEFAULT_SINK } else { PA_CMD_SET_DEFAULT_SOURCE };
        let (mut w, tag) = self.cmd_begin(cmd);
        w.str_bytes(Some(name));
        self.cmd_send(w, tag, Kind::Ack, cmd, 0)
    }

    fn set_port(&mut self, kind: c_int, index: u32, port: &[u8]) -> i32 {
        if !self.connected || (kind != SND_SINK && kind != SND_SOURCE) {
            return -1;
        }
        let l = if kind == SND_SINK { &self.sinks } else { &self.sources };
        let ok = l.iter().any(|d| d.index == index && d.ports.iter().any(|p| p.name == port));
        if !ok {
            return -1;
        }
        let cmd = if kind == SND_SINK { PA_CMD_SET_SINK_PORT } else { PA_CMD_SET_SOURCE_PORT };
        let (mut w, tag) = self.cmd_begin(cmd);
        w.u32(index);
        w.str(None);
        w.str_bytes(Some(port));
        self.cmd_send(w, tag, Kind::Ack, cmd, index)
    }

    fn move_stream(&mut self, kind: c_int, index: u32, device: u32) -> i32 {
        if !self.connected || (kind != SND_INPUT && kind != SND_OUTPUT) {
            return -1;
        }
        if !self.dev_exists(if kind == SND_INPUT { SND_SINK } else { SND_SOURCE }, None, device) {
            return -1;
        }
        let cmd = if kind == SND_INPUT { PA_CMD_MOVE_SINK_INPUT } else { PA_CMD_MOVE_SOURCE_OUTPUT };
        let (mut w, tag) = self.cmd_begin(cmd);
        w.u32(index);
        w.u32(device);
        w.str(None);
        self.cmd_send(w, tag, Kind::Ack, cmd, index)
    }

    /// Meter: a plain 1-channel float recording stream at 48 kHz, read in blocks of 2048 samples (~23
    /// levels a second); the level is the largest sample of each block (`on_packet`). A normal-rate
    /// stream is what every recorder asks for (an earlier 25 Hz peak-detect stream kept PipeWire busy).
    fn meter(&mut self, source: u32) -> i32 {
        if self.meter_channel != PA_INVALID_INDEX && self.fd >= 0 {
            let (mut w, tag) = self.cmd_begin(PA_CMD_DELETE_RECORD_STREAM);
            w.u32(self.meter_channel);
            self.cmd_send(w, tag, Kind::Ack, PA_CMD_DELETE_RECORD_STREAM, 0);
            self.meter_channel = PA_INVALID_INDEX;
        }
        self.meter_source = source;
        self.peak = 0.0;
        if source == PA_INVALID_INDEX {
            return 0;
        }
        if !self.connected {
            return 0; // created on connect
        }
        if !self.dev_exists(SND_SOURCE, None, source) {
            self.meter_source = PA_INVALID_INDEX;
            return -1;
        }
        let (mut w, tag) = self.cmd_begin(PA_CMD_CREATE_RECORD_STREAM);
        w.ss(PA_SAMPLE_FLOAT32LE, 1, 48000);
        w.cmap(&[0]); // PA_CHANNEL_POSITION_MONO
        w.u32(source);
        w.str(None);
        w.u32(0xffff_ffff); // maxlength: the server's
        w.bool(false); // corked
        w.u32(2048 * 4); // fragsize: 2048 samples, ~43 ms
        for _ in 0..5 {
            w.bool(false); // no_remap … fix_channels
        }
        w.bool(true); // no_move
        w.bool(false); // variable_rate
        w.bool(false); // peak_detect: no, computed here
        w.bool(true); // adjust_latency
        w.proplist(&[("media.name", "LordOS: level meter"), ("application.id", "org.lordos.Server")]);
        w.u32(PA_INVALID_INDEX); // direct_on_input
        w.bool(false); // early_requests
        w.bool(true); // dont_inhibit_auto_suspend
        w.bool(false); // fail_on_suspend
        w.u8(1); // one format: PCM
        w.format_pcm();
        w.cvol(&[PA_VOLUME_NORM]);
        w.bool(false); // muted
        for _ in 0..4 {
            w.bool(false); // volume_set, muted_set, relative, passthrough
        }
        self.cmd_send(w, tag, Kind::Record, PA_CMD_CREATE_RECORD_STREAM, source)
    }

    // ---- system sounds ----

    fn upload_data(&mut self, which: u32, channel: u32) {
        let samples = synth(which as usize);
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        for chunk in bytes.chunks(65536) {
            self.queue_bytes(&data_packet(channel, chunk));
        }
        let (mut w, tag) = self.cmd_begin(PA_CMD_FINISH_UPLOAD_STREAM);
        w.u32(channel);
        self.cmd_send(w, tag, Kind::Ack, PA_CMD_FINISH_UPLOAD_STREAM, 0);
        self.samples_uploaded |= 1 << which;
        self.uploading &= !(1 << which);
        if self.play_after & (1 << which) != 0 {
            self.play_after &= !(1 << which);
            self.play_sample(which as usize);
        }
    }

    /// Goes after the upload on the same socket: the server already has it when this arrives.
    fn play_sample(&mut self, which: usize) -> i32 {
        let (mut w, tag) = self.cmd_begin(PA_CMD_PLAY_SAMPLE);
        w.u32(PA_INVALID_INDEX);
        w.str(None); // the default output
        w.u32(PA_VOLUME_NORM);
        w.str(Some(SAMPLE_NAMES[which]));
        w.proplist(&[("media.role", "event"), ("application.name", "LordOS"), ("application.id", "org.lordos.Server"), ("event.id", SAMPLE_NAMES[which])]);
        self.cmd_send(w, tag, Kind::Ack, PA_CMD_PLAY_SAMPLE, 0)
    }

    fn play(&mut self, name: &[u8]) -> i32 {
        let which = match name {
            b"pop" => 0usize,
            b"notify" => 1,
            b"error" => 2,
            _ => return -1,
        };
        if !self.connected {
            return -1;
        }
        let bit = 1u32 << which;
        if self.samples_uploaded & bit != 0 {
            return self.play_sample(which);
        }
        self.play_after |= bit;
        if self.uploading & bit != 0 {
            return 0;
        }
        let bytes = (synth(which).len() * 2) as u32;
        let (mut w, tag) = self.cmd_begin(PA_CMD_CREATE_UPLOAD_STREAM);
        w.str(Some(SAMPLE_NAMES[which]));
        w.ss(PA_SAMPLE_S16LE, 1, SR as u32);
        w.cmap(&[0]);
        w.u32(bytes);
        w.proplist(&[("media.role", "event"), ("event.id", SAMPLE_NAMES[which])]);
        if self.cmd_send(w, tag, Kind::Upload, PA_CMD_CREATE_UPLOAD_STREAM, which as u32) != 0 {
            return -1;
        }
        self.uploading |= bit;
        0
    }
}

fn to_vol(x: f64) -> u32 {
    let x = if x >= 0.0 { x } else { 0.0 }; // NaN too
    let v = x * f64::from(PA_VOLUME_NORM) + 0.5;
    (if v > f64::from(PA_VOLUME_MAX) { f64::from(PA_VOLUME_MAX) } else { v }) as u32
}

const SAMPLE_NAMES: [&str; 3] = ["lordos-pop", "lordos-notify", "lordos-error"];
const SR: usize = 44100;

/// Synthesizes the sound: sines with an envelope (short attack, exponential decay). s16le mono.
pub fn synth(which: usize) -> Vec<i16> {
    // (f1, f2, duration, split)
    const TONE: [(f64, f64, f64, f64); 3] = [
        (880.0, 0.0, 0.07, 0.0),    // pop: a short tap
        (1046.5, 1568.0, 0.32, 0.12), // notify: C6 → G6
        (311.0, 233.0, 0.30, 0.13), // error: two low notes
    ];
    let (f1, f2, dur, split) = TONE[which];
    let n = (dur * SR as f64) as usize;
    (0..n)
        .map(|i| {
            let tt = i as f64 / SR as f64;
            let (mut f, mut t0) = (f1, 0.0);
            if f2 != 0.0 && tt >= split {
                f = f2;
                t0 = split;
            }
            let local = tt - t0;
            let env = (1.0 - (-local * 900.0).exp()) * (-local * if which > 0 { 14.0 } else { 45.0 }).exp();
            let v = (2.0 * std::f64::consts::PI * f * local).sin() * 0.8 + (4.0 * std::f64::consts::PI * f * local).sin() * 0.12;
            (v * env * 0.5 * 32767.0) as i16
        })
        .collect()
}

// ---- the C ABI of snd.h ----

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub type SndEventFn = Option<unsafe extern "C" fn(kind: *const c_char, json: *const c_char)>;

/// After dropping privileges; retries by itself if there's no server.
#[no_mangle]
pub extern "C" fn snd_start() -> c_int {
    state().start()
}
#[no_mangle]
pub extern "C" fn snd_stop() {
    state().stop()
}
/// For poll: the socket, or a timer while reconnecting.
#[no_mangle]
pub extern "C" fn snd_fd() -> c_int {
    state().poll_fd()
}
#[no_mangle]
pub extern "C" fn snd_dispatch() {
    state().dispatch()
}
/// Reports "state" (the full state, like `snd_state_json`) and "peak" to `f`, with no lock held.
#[no_mangle]
pub unsafe extern "C" fn snd_events(f: SndEventFn) {
    let (state_json, peak_json) = state().events();
    let Some(f) = f else { return };
    let call = |kind: &CStr, json: Vec<u8>| {
        if let Ok(c) = CString::new(json) {
            f(kind.as_ptr(), c.as_ptr());
        }
    };
    if let Some(j) = state_json {
        call(c"state", j);
    }
    if let Some(j) = peak_json {
        call(c"peak", j);
    }
}
/// Malloc'd; the caller frees it.
#[no_mangle]
pub extern "C" fn snd_state_json() -> *mut c_char {
    let json = state().state_json();
    // SAFETY: a malloc'd copy with its terminator, freed by the C caller.
    unsafe {
        let p = libc::malloc(json.len() + 1) as *mut u8;
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(json.as_ptr(), p, json.len());
        *p.add(json.len()) = 0;
        p as *mut c_char
    }
}
#[no_mangle]
pub unsafe extern "C" fn snd_set_volume(kind: c_int, index: u32, v: *const f64, n: c_int) -> c_int {
    if v.is_null() || n < 1 {
        return -1;
    }
    let values = std::slice::from_raw_parts(v, n as usize).to_vec();
    state().set_volume(kind, index, &values)
}
#[no_mangle]
pub extern "C" fn snd_set_level(kind: c_int, index: u32, level: f64) -> c_int {
    state().set_level(kind, index, level)
}
#[no_mangle]
pub extern "C" fn snd_set_mute(kind: c_int, index: u32, mute: c_int) -> c_int {
    state().set_mute(kind, index, mute != 0)
}
#[no_mangle]
pub unsafe extern "C" fn snd_set_default(kind: c_int, name: *const c_char) -> c_int {
    if name.is_null() {
        return -1;
    }
    state().set_default(kind, CStr::from_ptr(name).to_bytes())
}
#[no_mangle]
pub unsafe extern "C" fn snd_set_port(kind: c_int, index: u32, port: *const c_char) -> c_int {
    if port.is_null() {
        return -1;
    }
    state().set_port(kind, index, CStr::from_ptr(port).to_bytes())
}
#[no_mangle]
pub extern "C" fn snd_move(kind: c_int, index: u32, device: u32) -> c_int {
    state().move_stream(kind, index, device)
}
/// Peak meter of an input; 0xffffffff turns it off.
#[no_mangle]
pub extern "C" fn snd_meter(source: u32) -> c_int {
    state().meter(source)
}
/// A system sound: "pop", "notify" or "error".
#[no_mangle]
pub unsafe extern "C" fn snd_play(name: *const c_char) -> c_int {
    if name.is_null() {
        return -1;
    }
    state().play(CStr::from_ptr(name).to_bytes())
}
