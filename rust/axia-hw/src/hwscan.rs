//! Read-only hardware facts for the Store's "Suggested for your computer" (apps/store,
//! sdk/suggestengine.js, assets/store/suggestions.json; see docs/PACKAGES.md).
//!
//! Everything here reads world-readable kernel files without root: DMI strings
//! (/sys/class/dmi/id), the device tree model (/proc/device-tree), PCI and USB IDs and their drivers
//! (/sys/bus/pci, /sys/bus/usb), the virtio serial port names (/sys/class/virtio-ports), the
//! "libsane_matched" flag udev keeps for scanners (/run/udev/data) and, when the kernel allows it
//! (kernel.dmesg_restrict=0), the firmware files a driver asked for and didn't find (/dev/kmsg).
//! Nothing is written, nothing is executed and nothing goes over the network.
//!
//! With dmesg_restrict=1 (Debian's default) the session can't read the kernel log; then the root
//! helper's fixed action `hw-firmware-missing` (no arguments) reads it and returns only the
//! validated names of missing firmware files.
//!
//! `root` prefixes every path ("" for the real system); the tests and the fixture mode
//! (LORD_HWSCAN_ROOT, development only) point it at a fake tree, where the kernel log is the plain
//! file `<root>/kmsg`.
use crate::quote;
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

/// Missing firmware files reported.
pub const FW_MAX: usize = 64;
const KMSG_MAX: usize = 4 << 20;
/// The longest firmware name, with its NUL (`HwFwMiss.file`).
pub const FW_FILE: usize = 200;
/// The longest driver name, with its NUL.
pub const FW_DRIVER: usize = 33;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FwMiss {
    /// Relative to /lib/firmware.
    pub file: String,
    /// The driver or subsystem that asked ("iwlwifi", "bluetooth"), or "".
    pub driver: String,
}

fn printable(c: u8) -> char {
    if (0x20..0x7f).contains(&c) { c as char } else { '?' }
}

/// A small sysfs/proc file: its first line, trimmed, printable ASCII only ('\0' inside a device tree
/// string ends it), at most `size - 1` characters. "" when missing.
fn read_attr(path: &str, size: usize) -> String {
    let Ok(f) = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path) else { return String::new() };
    let mut buf = Vec::new();
    if f.take(255).read_to_end(&mut buf).is_err() || buf.is_empty() {
        return String::new();
    }
    let mut out: String = buf.iter().take_while(|&&c| c != 0 && c != b'\n').take(size - 1).map(|&c| printable(c)).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn attr_at(root: &str, dir: &str, name: &str, size: usize) -> String {
    read_attr(&format!("{root}{dir}/{name}"), size)
}

/// A hex ID attribute ("0x10de" or "10de") as lowercase hex of `digits` digits; "" if not hex.
fn hex_attr(root: &str, dir: &str, name: &str, digits: usize) -> String {
    let v = attr_at(root, dir, name, 32);
    let s = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")).unwrap_or(&v);
    if s.is_empty() || s.len() > digits || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    format!("{}{}", "0".repeat(digits - s.len()), s.to_ascii_lowercase())
}

/// The driver bound to a device: the last component of its "driver" link.
fn driver_of(root: &str, dir: &str, size: usize) -> String {
    let Ok(l) = fs::read_link(format!("{root}{dir}/driver")) else { return String::new() };
    let l = l.as_os_str().as_encoded_bytes();
    let l = &l[..l.len().min(511)];
    let b = l.rsplit(|&c| c == b'/').next().unwrap_or(l);
    b.iter().filter(|c| c.is_ascii_alphanumeric() || **c == b'_' || **c == b'-').take(size - 1).map(|&c| c as char).collect()
}

/// The entries of a directory, sorted by name (so the JSON is stable): no dot entries, no name over 64
/// bytes, at most 1024.
fn list_dir(path: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(path)
        .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| !n.starts_with('.') && n.len() <= 64).take(1024).collect())
        .unwrap_or_default();
    v.sort();
    v
}

// ---- DMI and device tree ----

fn put_dmi(o: &mut Vec<u8>, root: &str) {
    const F: [(&str, &str); 5] = [("sysVendor", "sys_vendor"), ("productName", "product_name"), ("boardVendor", "board_vendor"), ("boardName", "board_name"), ("chassisType", "chassis_type")];
    o.extend_from_slice(b"\"dmi\":{");
    for (i, (key, file)) in F.iter().enumerate() {
        o.extend_from_slice(format!("{}\"{key}\":", if i > 0 { "," } else { "" }).as_bytes());
        quote(attr_at(root, "/sys/class/dmi/id", file, 96).as_bytes(), o);
    }
    o.push(b'}');
}

fn put_devicetree(o: &mut Vec<u8>, root: &str) {
    o.extend_from_slice(b",\"model\":");
    quote(read_attr(&format!("{root}/proc/device-tree/model"), 128).as_bytes(), o);
    // compatible: NUL-separated strings
    o.extend_from_slice(b",\"compatible\":[");
    if let Ok(f) = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(format!("{root}/proc/device-tree/compatible")) {
        let mut buf = Vec::new();
        let _ = f.take(511).read_to_end(&mut buf);
        let mut first = true;
        for part in buf.split(|&c| c == 0) {
            let s: String = part.iter().filter(|c| c.is_ascii_alphanumeric() || b",.-_+".contains(c)).take(95).map(|&c| c as char).collect();
            if !s.is_empty() {
                if !first {
                    o.push(b',');
                }
                first = false;
                quote(s.as_bytes(), o);
            }
        }
    }
    o.push(b']');
}

// ---- PCI ----

fn put_pci(o: &mut Vec<u8>, root: &str) {
    o.extend_from_slice(b",\"pci\":[");
    let mut n = 0;
    for name in list_dir(&format!("{root}/sys/bus/pci/devices")) {
        if n >= 256 {
            break;
        }
        let dir = format!("/sys/bus/pci/devices/{name}");
        let (vendor, device, cls) = (hex_attr(root, &dir, "vendor", 4), hex_attr(root, &dir, "device", 4), hex_attr(root, &dir, "class", 6));
        if vendor.is_empty() || device.is_empty() {
            continue;
        }
        let drv = driver_of(root, &dir, 33);
        o.extend_from_slice(format!("{}{{\"vendor\":\"{vendor}\",\"device\":\"{device}\",\"class\":\"{cls}\",\"driver\":", if n > 0 { "," } else { "" }).as_bytes());
        n += 1;
        quote(drv.as_bytes(), o);
        o.push(b'}');
    }
    o.push(b']');
}

// ---- USB ----

/// udev's database entry for a character device ("189:3" → /run/udev/data/c189:3) has
/// "E:libsane_matched=yes" when libsane's udev rules know the device as a scanner.
fn sane_matched(root: &str, dir: &str) -> bool {
    let dev = attr_at(root, dir, "dev", 32);
    let Some((a, b)) = dev.split_once(':') else { return false };
    let (Ok(ma), Ok(mi)) = (a.trim().parse::<u32>(), b.parse::<u32>()) else { return false };
    let Ok(f) = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(format!("{root}/run/udev/data/c{ma}:{mi}")) else { return false };
    let mut buf = Vec::new();
    if f.take(8191).read_to_end(&mut buf).is_err() {
        return false;
    }
    buf.split(|&c| c == b'\n').any(|l| l == b"E:libsane_matched=yes")
}

fn put_usb(o: &mut Vec<u8>, root: &str) {
    let all = list_dir(&format!("{root}/sys/bus/usb/devices"));
    o.extend_from_slice(b",\"usb\":[");
    let mut n = 0;
    for dev in &all {
        if n >= 256 {
            break;
        }
        if dev.contains(':') {
            continue; // an interface, listed under its device
        }
        let dir = format!("/sys/bus/usb/devices/{dev}");
        let (vendor, product) = (hex_attr(root, &dir, "idVendor", 4), hex_attr(root, &dir, "idProduct", 4));
        if vendor.is_empty() || product.is_empty() || vendor == "1d6b" {
            continue; // no IDs, or a root hub
        }
        let cls = hex_attr(root, &dir, "bDeviceClass", 2);
        o.extend_from_slice(format!("{}{{\"vendor\":\"{vendor}\",\"product\":\"{product}\",\"class\":\"{cls}\",\"ifaces\":[", if n > 0 { "," } else { "" }).as_bytes());
        n += 1;
        // its interfaces: "<device>:<config>.<n>"; the first bound driver names the device
        let mut drv = String::new();
        let mut fi = 0;
        for k in &all {
            if !k.starts_with(dev.as_str()) || k.as_bytes().get(dev.len()) != Some(&b':') {
                continue;
            }
            let idir = format!("/sys/bus/usb/devices/{k}");
            let (ic, is, ip) = (hex_attr(root, &idir, "bInterfaceClass", 2), hex_attr(root, &idir, "bInterfaceSubClass", 2), hex_attr(root, &idir, "bInterfaceProtocol", 2));
            if ic.is_empty() {
                continue;
            }
            o.extend_from_slice(format!("{}\"{ic}{}{}\"", if fi > 0 { "," } else { "" }, if is.is_empty() { "00" } else { &is }, if ip.is_empty() { "00" } else { &ip }).as_bytes());
            fi += 1;
            if drv.is_empty() {
                drv = driver_of(root, &idir, 33);
            }
        }
        o.extend_from_slice(b"],\"driver\":");
        quote(drv.as_bytes(), o);
        o.extend_from_slice(format!(",\"sane\":{}}}", sane_matched(root, &dir)).as_bytes());
    }
    o.push(b']');
}

// ---- virtio serial ports (a QEMU guest agent channel is named "org.qemu.guest_agent.0") ----

fn put_virtio(o: &mut Vec<u8>, root: &str) {
    o.extend_from_slice(b",\"virtio\":[");
    let mut n = 0;
    for p in list_dir(&format!("{root}/sys/class/virtio-ports")) {
        if n >= 32 {
            break;
        }
        let name = attr_at(root, &format!("/sys/class/virtio-ports/{p}"), "name", 96);
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) {
            continue;
        }
        if n > 0 {
            o.push(b',');
        }
        n += 1;
        quote(name.as_bytes(), o);
    }
    o.push(b']');
}

// ---- firmware misses ----

/// A safe relative firmware path: [A-Za-z0-9._+,-/], no "..", no "//", no leading '/' or '.', at most 199.
pub fn fw_name_valid(s: &[u8]) -> bool {
    let n = s.len();
    if n == 0 || n >= FW_FILE || s[0] == b'/' || s[0] == b'.' || s.windows(2).any(|w| w == b".." || w == b"//") {
        return false;
    }
    s.iter().all(|c| c.is_ascii_alphanumeric() || b"._+,-/".contains(c))
}

/// `<root>/lib/firmware[/updates]/<name>[.xz|.zst]` exists (installed since the miss).
pub fn fw_present(root: &str, name: &str) -> bool {
    if !fw_name_valid(name.as_bytes()) {
        return false;
    }
    for d in ["/lib/firmware/updates/", "/lib/firmware/", "/usr/lib/firmware/"] {
        for e in ["", ".xz", ".zst"] {
            if fs::metadata(format!("{root}{d}{name}{e}")).is_ok_and(|m| m.is_file()) {
                return true;
            }
        }
    }
    false
}

/// The driver name at the start of a message: "iwlwifi 0000:00:14.3: …" → "iwlwifi".
fn msg_driver(msg: &[u8]) -> String {
    let Some(sp) = msg.iter().position(|&c| c == b' ') else { return String::new() };
    if sp == 0 || sp >= FW_DRIVER {
        return String::new();
    }
    let rest = &msg[sp + 1..];
    let Some(sp2) = rest.iter().position(|&c| c == b' ') else { return String::new() }; // "<driver> <device>: "
    if sp2 + 1 < 3 || rest[sp2 - 1] != b':' {
        return String::new();
    }
    if !msg[..sp].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-') {
        return String::new();
    }
    String::from_utf8_lossy(&msg[..sp]).into_owned()
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Parses kernel log text (kmsg records "pri,seq,ts,flags;message" or dmesg lines) for the firmware
/// loader's "Direct firmware load for X failed" and "firmware: failed to load X" lines. Up to `max`
/// unique, valid names.
pub fn fw_parse(text: &[u8], max: usize) -> Vec<FwMiss> {
    const PAT: [(&[u8], &[u8]); 2] = [(b"Direct firmware load for ", b" failed with error "), (b"firmware: failed to load ", b" (")];
    let mut out: Vec<FwMiss> = Vec::new();
    let mut l = 0;
    while l < text.len() && out.len() < max {
        let nl = text[l..].iter().position(|&c| c == b'\n').map(|i| l + i);
        let le = nl.unwrap_or(text.len());
        let next = nl.map_or(text.len(), |n| n + 1);
        // kmsg: "6,1234,5678,-;message"; continuation lines start with a space
        if text[l] == b' ' {
            l = next;
            continue;
        }
        let line = &text[l..le];
        let first = line.first().copied().unwrap_or(b'\n');
        let mut msg = 0;
        if let Some(semi) = line.iter().position(|&c| c == b';').filter(|_| first.is_ascii_digit()) {
            msg = semi + 1;
        } else if first == b'[' {
            // dmesg: "[    5.1] message"
            if let Some(rb) = line.iter().position(|&c| c == b']') {
                msg = rb + 1 + (rb + 1 < line.len() && line[rb + 1] == b' ') as usize;
            }
        }
        let m = &line[msg..];
        for (pre, post) in PAT {
            let Some(a) = find(m, pre, 0).map(|i| i + pre.len()) else { continue };
            let Some(z) = find(m, post, a) else { break };
            if z == a || z - a >= FW_FILE || !fw_name_valid(&m[a..z]) {
                break;
            }
            let file = String::from_utf8_lossy(&m[a..z]).into_owned();
            if !out.iter().any(|x| x.file == file) {
                out.push(FwMiss { file, driver: msg_driver(m) });
            }
            break;
        }
        l = next;
    }
    out
}

/// Reads the kernel log ring: `<root>/kmsg` when root is non-empty, /dev/kmsg otherwise (non-blocking, every
/// record still in the ring, at most 4 MB). `Err(true)` when the kernel refuses (dmesg_restrict).
pub fn read_kmsg(root: &str) -> Result<Vec<u8>, bool> {
    let fixture = !root.is_empty();
    let path = if fixture { format!("{root}/kmsg") } else { "/dev/kmsg".to_string() };
    let mut opts = fs::OpenOptions::new();
    opts.read(true).custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | if fixture { libc::O_NOFOLLOW } else { 0 });
    let mut f = opts.open(&path).map_err(|e| matches!(e.raw_os_error(), Some(libc::EPERM | libc::EACCES)))?;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 8192];
    loop {
        if buf.len() >= KMSG_MAX {
            break;
        }
        match f.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => match e.raw_os_error() {
                Some(libc::EINTR | libc::EPIPE) => continue, // a record was overwritten: go on
                Some(libc::EPERM | libc::EACCES) if buf.is_empty() => return Err(true),
                _ => break, // EAGAIN: the end of the ring
            },
        }
    }
    Ok(buf)
}

/// The missing firmware files: parse the log and drop the ones present now.
pub fn fw_missing(root: &str, text: &[u8], max: usize) -> Vec<FwMiss> {
    fw_parse(text, max.min(FW_MAX)).into_iter().filter(|m| !fw_present(root, &m.file)).collect()
}

fn put_firmware(o: &mut Vec<u8>, root: &str) {
    let log = read_kmsg(root).ok();
    o.extend_from_slice(format!(",\"firmware\":{{\"readable\":{},\"missing\":[", log.is_some()).as_bytes());
    if let Some(log) = log {
        for (i, m) in fw_missing(root, &log, FW_MAX).iter().enumerate() {
            o.extend_from_slice(if i > 0 { b",{\"file\":" } else { b"{\"file\":" });
            quote(m.file.as_bytes(), o);
            o.extend_from_slice(b",\"driver\":");
            quote(m.driver.as_bytes(), o);
            o.push(b'}');
        }
    }
    o.extend_from_slice(b"]}");
}

/// The facts as JSON:
///   { fixture, arch, dmi: { sysVendor, productName, boardVendor, boardName, chassisType },
///     model, compatible: [], pci: [{ vendor, device, class, driver }],
///     usb: [{ vendor, product, class, ifaces: ["070104", …], driver, sane }], virtio: [names],
///     firmware: { readable, missing: [{ file, driver }] } }
/// IDs are lowercase hex without "0x". firmware.readable is false when the kernel log can't be read
/// without root (the Store then asks hw-firmware-missing).
pub fn json(root: &str) -> Vec<u8> {
    let machine = || {
        // SAFETY: uname fills the struct.
        let mut u: libc::utsname = unsafe { std::mem::zeroed() };
        if unsafe { libc::uname(&mut u) } != 0 {
            return "unknown".to_string();
        }
        let m: Vec<u8> = u.machine.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        String::from_utf8_lossy(&m).into_owned()
    };
    let arch = if root.is_empty() {
        machine()
    } else {
        // a fixture may name its architecture
        let a = read_attr(&format!("{root}/arch"), 72);
        if a.is_empty() { machine() } else { a }
    };
    let mut o: Vec<u8> = format!("{{\"fixture\":{},\"arch\":", !root.is_empty()).into_bytes();
    quote(arch.as_bytes(), &mut o);
    o.push(b',');
    put_dmi(&mut o, root);
    put_devicetree(&mut o, root);
    put_pci(&mut o, root);
    put_usb(&mut o, root);
    put_virtio(&mut o, root);
    put_firmware(&mut o, root);
    o.push(b'}');
    o
}
