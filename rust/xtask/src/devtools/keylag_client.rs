//! A minimal Wayland client for the key latency harness (docs/TROUBLESHOOTING.md).
//!
//!   cargo xtask keylag-client
//!
//! A 320x200 window that repaints itself in a new color on every key press and asks for
//! presentation feedback for that frame. It prints, in CLOCK_MONOTONIC ms:
//!
//!   client key t=81234.901          wl_keyboard.key (press) arrived
//!   client shown t=81251.300        the frame drawn for it was presented
//!
//! (no key identity is printed, like the compositor's trace). Run inside a session:
//! `XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0 cargo xtask keylag-client`.
//!
//! It speaks the Wayland wire protocol directly (wl_compositor, wl_shm, xdg_shell, wl_seat and
//! presentation-time), so it needs no libwayland and no generated protocol headers.

use crate::Result;
use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

const W: u32 = 320;
const H: u32 = 200;

fn mono_ms() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 * 1e3 + ts.tv_nsec as f64 / 1e6
}

/// An outgoing message under construction.
struct Message {
    id: u32,
    opcode: u16,
    body: Vec<u8>,
}

impl Message {
    fn new(id: u32, opcode: u16) -> Self {
        Self { id, opcode, body: Vec::new() }
    }
    fn u32(mut self, v: u32) -> Self {
        self.body.extend(v.to_ne_bytes());
        self
    }
    fn i32(self, v: i32) -> Self {
        self.u32(v as u32)
    }
    fn string(mut self, s: &str) -> Self {
        self.body.extend(((s.len() + 1) as u32).to_ne_bytes());
        self.body.extend(s.as_bytes());
        self.body.push(0);
        while self.body.len() % 4 != 0 {
            self.body.push(0);
        }
        self
    }
    fn bytes(&self) -> Vec<u8> {
        let size = 8 + self.body.len();
        let mut out = Vec::with_capacity(size);
        out.extend(self.id.to_ne_bytes());
        out.extend((((size as u32) << 16) | u32::from(self.opcode)).to_ne_bytes());
        out.extend(&self.body);
        out
    }
}

/// A reader of an incoming event's arguments.
struct Args<'a>(&'a [u8]);

impl Args<'_> {
    fn u32(&mut self) -> u32 {
        let v = self.0.get(..4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_ne_bytes);
        self.0 = &self.0[4.min(self.0.len())..];
        v
    }
    fn string(&mut self) -> String {
        let len = self.u32() as usize;
        let text = String::from_utf8_lossy(&self.0[..len.saturating_sub(1).min(self.0.len())]).into_owned();
        self.0 = &self.0[(len + 3 & !3).min(self.0.len())..];
        text
    }
}

/// What each object id stands for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Display,
    Registry,
    Sync,
    Compositor,
    Shm,
    WmBase,
    Seat,
    Presentation,
    Pool,
    Buffer(usize),
    Keyboard,
    Surface,
    XdgSurface,
    Toplevel,
    Feedback,
}

struct Client {
    socket: UnixStream,
    next_id: u32,
    kinds: HashMap<u32, Kind>,
    send_fds: Vec<RawFd>,
}

impl Client {
    fn new_id(&mut self, kind: Kind) -> u32 {
        self.next_id += 1;
        self.kinds.insert(self.next_id, kind);
        self.next_id
    }

    fn send(&mut self, message: Message) -> Result<()> {
        let bytes = message.bytes();
        if self.send_fds.is_empty() {
            return self.socket.write_all(&bytes).map_err(|e| e.to_string());
        }
        let fds: Vec<RawFd> = std::mem::take(&mut self.send_fds);
        let mut control = vec![0u8; unsafe { libc::CMSG_SPACE((fds.len() * 4) as u32) } as usize];
        let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut _, iov_len: bytes.len() };
        // SAFETY: the buffers outlive the sendmsg call, and the control buffer is sized for the descriptors.
        unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr() as *mut _;
            msg.msg_controllen = control.len() as _;
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN((fds.len() * 4) as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr() as *const u8, libc::CMSG_DATA(cmsg), fds.len() * 4);
            if libc::sendmsg(self.socket.as_raw_fd(), &msg, 0) < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }

    /// Reads whatever the server sent; descriptors that come with it (the keymap) are closed.
    fn receive(&self, buffer: &mut Vec<u8>) -> Result<bool> {
        let mut data = [0u8; 4096];
        let mut control = [0u8; 256];
        let mut iov = libc::iovec { iov_base: data.as_mut_ptr() as *mut _, iov_len: data.len() };
        // SAFETY: the buffers are valid for the call; received descriptors are wrapped and dropped.
        let n = unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr() as *mut _;
            msg.msg_controllen = control.len() as _;
            let n = libc::recvmsg(self.socket.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC);
            let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let count = ((*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                    for i in 0..count {
                        let fd = std::ptr::read_unaligned((libc::CMSG_DATA(cmsg) as *const i32).add(i));
                        drop(OwnedFd::from_raw_fd(fd));
                    }
                }
                cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
            }
            n
        };
        if n < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        buffer.extend_from_slice(&data[..n as usize]);
        Ok(n > 0)
    }

    /// The events in `buffer`, as (object, opcode, arguments); the rest stays for the next read.
    fn events(buffer: &mut Vec<u8>) -> Vec<(u32, u16, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 0;
        while buffer.len() - at >= 8 {
            let id = u32::from_ne_bytes(buffer[at..at + 4].try_into().unwrap());
            let word = u32::from_ne_bytes(buffer[at + 4..at + 8].try_into().unwrap());
            let size = (word >> 16) as usize;
            if size < 8 || buffer.len() - at < size {
                break;
            }
            out.push((id, (word & 0xffff) as u16, buffer[at + 8..at + size].to_vec()));
            at += size;
        }
        buffer.drain(..at);
        out
    }
}

struct Window {
    buffers: [u32; 2],
    pixels: [*mut u32; 2],
    current: usize,
    surface: u32,
    presentation: Option<u32>,
    configured: bool,
}

fn draw(client: &mut Client, win: &mut Window, code: u32) -> Result<()> {
    win.current ^= 1;
    let color = 0xff00_0000 | (code.wrapping_mul(2_654_435_761) & 0x00ff_ffff);
    // SAFETY: the mapping holds two W*H buffers and lives as long as the client.
    unsafe { std::slice::from_raw_parts_mut(win.pixels[win.current], (W * H) as usize).fill(color) };
    client.send(Message::new(win.surface, 1).u32(win.buffers[win.current]).i32(0).i32(0))?; // attach
    client.send(Message::new(win.surface, 2).i32(0).i32(0).i32(W as i32).i32(H as i32))?; // damage
    if let Some(presentation) = win.presentation {
        let feedback = client.new_id(Kind::Feedback);
        client.send(Message::new(presentation, 1).u32(win.surface).u32(feedback))?;
    }
    client.send(Message::new(win.surface, 6)) // commit
}

pub fn execute(_repo: &std::path::Path, args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() {
        return Err("keylag-client takes no arguments".into());
    }
    let (Some(dir), display) = (std::env::var_os("XDG_RUNTIME_DIR"), std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into())) else {
        eprintln!("keylag-client: no Wayland display");
        return Ok(ExitCode::from(1));
    };
    let path = std::path::Path::new(&dir).join(display);
    let Ok(socket) = UnixStream::connect(&path) else {
        eprintln!("keylag-client: no Wayland display");
        return Ok(ExitCode::from(1));
    };
    let mut client = Client { socket, next_id: 1, kinds: HashMap::from([(1, Kind::Display)]), send_fds: Vec::new() };
    let registry = client.new_id(Kind::Registry);
    client.send(Message::new(1, 1).u32(registry))?; // get_registry
    let sync = client.new_id(Kind::Sync);
    client.send(Message::new(1, 0).u32(sync))?; // sync

    // objects bound from the registry
    let (mut compositor, mut shm, mut wm_base, mut seat, mut presentation) = (None, None, None, None, None);
    let mut buffer = Vec::new();
    let mut window: Option<Window> = None;
    let mut mapping: Option<*mut u32> = None;
    let mut keyboard_focus_said = false;
    loop {
        if !client.receive(&mut buffer)? {
            return Ok(ExitCode::SUCCESS); // the compositor went away
        }
        for (id, opcode, data) in Client::events(&mut buffer) {
            let mut a = Args(&data);
            match (client.kinds.get(&id).copied(), opcode) {
                (Some(Kind::Display), 0) => {
                    eprintln!("keylag-client: protocol error on object {} (code {})", a.u32(), a.u32());
                    return Ok(ExitCode::from(1));
                }
                (Some(Kind::Display), 1) => {
                    client.kinds.remove(&a.u32());
                }
                (Some(Kind::Registry), 0) => {
                    let (name, interface, version) = (a.u32(), a.string(), a.u32());
                    let (kind, bind_version, slot): (Kind, u32, &mut Option<u32>) = match interface.as_str() {
                        "wl_compositor" => (Kind::Compositor, 4, &mut compositor),
                        "wl_shm" => (Kind::Shm, 1, &mut shm),
                        "xdg_wm_base" => (Kind::WmBase, 1, &mut wm_base),
                        "wl_seat" => (Kind::Seat, version.min(5), &mut seat),
                        "wp_presentation" => (Kind::Presentation, 1, &mut presentation),
                        _ => continue,
                    };
                    let new = client.new_id(kind);
                    client.send(Message::new(registry, 0).u32(name).string(&interface).u32(bind_version).u32(new))?;
                    *slot = Some(new);
                }
                (Some(Kind::Sync), 0) => {
                    // the globals are in: build the window
                    let (Some(compositor), Some(shm), Some(wm_base), Some(seat)) = (compositor, shm, wm_base, seat) else {
                        eprintln!("keylag-client: missing globals");
                        return Ok(ExitCode::from(1));
                    };
                    if presentation.is_none() {
                        eprintln!("keylag-client: no wp_presentation (no 'shown' lines)");
                    }
                    let one = (W * H * 4) as usize;
                    // SAFETY: a fresh anonymous file, sized and mapped read-write; it lives for the whole run.
                    let (fd, map) = unsafe {
                        let fd = libc::memfd_create(c"keylag".as_ptr(), libc::MFD_CLOEXEC);
                        if fd < 0 || libc::ftruncate(fd, (one * 2) as i64) != 0 {
                            return Err("memfd_create failed".into());
                        }
                        let map = libc::mmap(std::ptr::null_mut(), one * 2, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
                        if map == libc::MAP_FAILED {
                            return Err("mmap failed".into());
                        }
                        (fd, map as *mut u32)
                    };
                    mapping = Some(map);
                    let pool = client.new_id(Kind::Pool);
                    client.send_fds.push(fd);
                    client.send(Message::new(shm, 0).u32(pool).i32((one * 2) as i32))?; // create_pool
                    let mut buffers = [0u32; 2];
                    for (i, slot) in buffers.iter_mut().enumerate() {
                        *slot = client.new_id(Kind::Buffer(i));
                        client.send(Message::new(pool, 0).u32(*slot).i32((i * one) as i32).i32(W as i32).i32(H as i32).i32((W * 4) as i32).u32(1))?; // XRGB8888
                    }
                    let keyboard = client.new_id(Kind::Keyboard);
                    client.send(Message::new(seat, 1).u32(keyboard))?; // get_keyboard
                    let surface = client.new_id(Kind::Surface);
                    client.send(Message::new(compositor, 0).u32(surface))?; // create_surface
                    let xdg = client.new_id(Kind::XdgSurface);
                    client.send(Message::new(wm_base, 2).u32(xdg).u32(surface))?; // get_xdg_surface
                    let toplevel = client.new_id(Kind::Toplevel);
                    client.send(Message::new(xdg, 1).u32(toplevel))?; // get_toplevel
                    client.send(Message::new(toplevel, 2).string("keylag"))?; // set_title
                    client.send(Message::new(surface, 6))?; // commit
                    window = Some(Window { buffers, pixels: [map, map.wrapping_add((W * H) as usize)], current: 0, surface, presentation, configured: false });
                }
                (Some(Kind::WmBase), 0) => client.send(Message::new(wm_base.unwrap_or(0), 3).u32(a.u32()))?, // ping → pong
                (Some(Kind::XdgSurface), 0) => {
                    let serial = a.u32();
                    client.send(Message::new(id, 4).u32(serial))?; // ack_configure
                    if let Some(win) = window.as_mut() {
                        if !win.configured {
                            win.configured = true;
                            draw(&mut client, win, 0)?;
                        }
                    }
                }
                (Some(Kind::Toplevel), 1) => return Ok(ExitCode::SUCCESS), // close
                (Some(Kind::Keyboard), 1) => {
                    if !keyboard_focus_said {
                        keyboard_focus_said = true;
                    }
                    eprintln!("keylag-client: keyboard focus");
                }
                (Some(Kind::Keyboard), 3) => {
                    let (_serial, _time, key, state) = (a.u32(), a.u32(), a.u32(), a.u32());
                    if state == 1 {
                        println!("client key t={:.3}", mono_ms());
                        let _ = std::io::stdout().flush();
                        if let Some(win) = window.as_mut() {
                            draw(&mut client, win, key)?;
                        }
                    }
                }
                (Some(Kind::Feedback), 1) => {
                    let (hi, lo, ns) = (a.u32(), a.u32(), a.u32());
                    println!("client shown t={:.3}", ((u64::from(hi) << 32) | u64::from(lo)) as f64 * 1e3 + f64::from(ns) / 1e6);
                    let _ = std::io::stdout().flush();
                    client.kinds.remove(&id);
                }
                (Some(Kind::Feedback), 2) => {
                    println!("client discarded t={:.3}", mono_ms());
                    let _ = std::io::stdout().flush();
                    client.kinds.remove(&id);
                }
                _ => {}
            }
        }
        let _ = mapping;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_have_the_wire_layout_with_padded_strings() {
        let m = Message::new(2, 0).u32(5).string("wl_seat").u32(5).u32(9);
        let b = m.bytes();
        assert_eq!(u32::from_ne_bytes(b[0..4].try_into().unwrap()), 2);
        let word = u32::from_ne_bytes(b[4..8].try_into().unwrap());
        assert_eq!((word >> 16) as usize, b.len());
        assert_eq!(word & 0xffff, 0);
        assert_eq!(b.len() % 4, 0);
        // name, then the string length 8 ("wl_seat" and its NUL), the text, the version and the new id
        assert_eq!(u32::from_ne_bytes(b[12..16].try_into().unwrap()), 8);
        assert_eq!(&b[16..23], b"wl_seat");
        assert_eq!(b.len(), 8 + 4 + 4 + 8 + 4 + 4);
        assert_eq!(Message::new(1, 7).string("abc").bytes().len(), 8 + 4 + 4);
    }

    #[test]
    fn events_are_split_and_a_partial_one_waits() {
        let one = Message::new(7, 3).u32(10).u32(20).bytes();
        let two = Message::new(8, 1).bytes();
        let mut buffer = [one.clone(), two.clone(), one[..10].to_vec()].concat();
        let events = Client::events(&mut buffer);
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].0, events[0].1, events[0].2.len()), (7, 3, 8));
        assert_eq!((events[1].0, events[1].1, events[1].2.len()), (8, 1, 0));
        assert_eq!(buffer.len(), 10);
        buffer.extend_from_slice(&one[10..]);
        assert_eq!(Client::events(&mut buffer).len(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn arguments_read_numbers_and_padded_strings() {
        let m = Message::new(2, 0).u32(3).string("xdg_wm_base").u32(1);
        let mut a = Args(&m.body);
        assert_eq!((a.u32(), a.string(), a.u32()), (3, "xdg_wm_base".to_string(), 1));
        assert_eq!(Args(&[]).u32(), 0);
    }
}
