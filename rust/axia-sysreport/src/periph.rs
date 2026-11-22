//! The peripherals of this machine for System Information's Hardware tab: keyboards, mice,
//! receivers and what else hangs off USB, read without root from sysfs. The devices behind a wireless
//! receiver come from the input class, with their battery from the HID++ power supplies.
//!
//! This is NOT part of the report. A device names itself, and a name can carry a person's ("Ana's
//! keyboard"), so the structures of the report have no place for these strings and the `axia-sysreport`
//! and `axia-fetch` commands never print them. Serial numbers (the USB "serial" attribute, a power
//! supply's serial_number) and addresses (an input device's "uniq" and "phys") are never read.
//! Bluetooth devices come from BlueZ, in the app.
use crate::fsutil::{atoi, fit, list_dir, read_attr_limit};
use axia_codecs::ids;

/// The first line of `<root><dir>/<name>`, printable ASCII only ("" when missing); this reader looks at 1023 bytes.
fn attr(root: &str, dir: &str, name: &str, size: usize) -> String {
    read_attr_limit(&format!("{root}{dir}/{name}"), size, 1023)
}

const MAX: usize = 48;
const NAME_MAX: usize = 96;

struct Periph {
    name: String,
    kind: &'static str,
    conn: &'static str,
    /// A receiver's device: its HID id, to find its battery.
    hid: String,
    battery: i32,
}

fn strtol16(s: &str) -> i64 {
    let t = s.trim_start();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let v = t.bytes().map_while(|c| (c as char).to_digit(16)).fold(0i64, |a, d| a.saturating_mul(16).saturating_add(d as i64));
    if neg { -v } else { v }
}

fn add<'a>(l: &'a mut Vec<Periph>, name: &str, kind: &'static str, conn: &'static str) -> Option<&'a mut Periph> {
    // one device, several interfaces or input nodes
    if l.iter().any(|p| p.name == name && p.conn == conn) || l.len() >= MAX {
        return None;
    }
    l.push(Periph { name: fit(name, NAME_MAX), kind, conn, hid: String::new(), battery: -1 });
    l.last_mut()
}

// ---- USB ----

fn named_receiver(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    ["receiver", "unifying", "nano", "dongle"].iter().any(|w| l.contains(w))
}

/// What a USB device is, from its interfaces (class, boot protocol) and its name.
fn usb_kind(root: &str, all: &[String], dev: &str, name: &str) -> &'static str {
    let (mut hid, mut kbd, mut mouse, mut audio, mut video, mut storage, mut printer, mut net, mut bt) = (false, false, false, false, false, false, false, false, false);
    for v in all {
        if !v.starts_with(dev) || v.as_bytes().get(dev.len()) != Some(&b':') {
            continue;
        }
        let dir = format!("/sys/bus/usb/devices/{v}");
        let (c, p) = (strtol16(&attr(root, &dir, "bInterfaceClass", 8)), strtol16(&attr(root, &dir, "bInterfaceProtocol", 8)));
        match c {
            3 => {
                hid = true;
                kbd |= p == 1;
                mouse |= p == 2;
            }
            1 => audio = true,
            0x0e => video = true,
            8 => storage = true,
            7 => printer = true,
            2 | 0x0a => net = true,
            0xe0 => bt = true,
            _ => {}
        }
    }
    if audio {
        "audio" // a sound card's buttons are HID too
    } else if video {
        "camera"
    } else if hid && named_receiver(name) {
        "receiver"
    } else if kbd {
        "keyboard"
    } else if mouse {
        "mouse"
    } else if hid {
        "input"
    } else if storage {
        "storage"
    } else if printer {
        "printer"
    } else if net {
        "network"
    } else if bt {
        "bluetooth"
    } else {
        "other"
    }
}

fn collect_usb(l: &mut Vec<Periph>, root: &str, ids_db: Option<&[u8]>) {
    let v = list_dir(root, "/sys/bus/usb/devices");
    let v: Vec<String> = {
        // sorted by name like qsort(strcmp), not naturally
        let mut v = v;
        v.sort();
        v
    };
    for dev in &v {
        if dev.contains(':') {
            continue; // an interface, read with its device
        }
        let dir = format!("/sys/bus/usb/devices/{dev}");
        let (vid, pid, cls) = (attr(root, &dir, "idVendor", 8), attr(root, &dir, "idProduct", 8), attr(root, &dir, "bDeviceClass", 8));
        if vid.len() != 4 || pid.len() != 4 || vid == "1d6b" || strtol16(&cls) == 9 {
            continue; // root hubs, hubs
        }
        let mut maker = attr(root, &dir, "manufacturer", 64);
        let mut product = attr(root, &dir, "product", 64);
        let (vn, dn) = match ids_db {
            Some(t) => {
                let (a, b) = ids::lookup(t, vid.as_bytes(), Some(pid.as_bytes()), 64, 64);
                (String::from_utf8_lossy(ids::name_text(&a)).into_owned(), String::from_utf8_lossy(ids::name_text(&b)).into_owned())
            }
            None => (String::new(), String::new()),
        };
        if product.is_empty() {
            product = dn;
        }
        if maker.is_empty() {
            maker = vn;
        }
        let name = if product.is_empty() {
            format!("{vid}:{pid}")
        } else if maker.is_empty() || product.to_ascii_lowercase().starts_with(&maker.to_ascii_lowercase()) {
            fit(&product, NAME_MAX)
        } else {
            fit(&format!("{} {}", fit(&maker, 41), fit(&product, 55)), NAME_MAX)
        };
        let kind = usb_kind(root, &v, dev, &name);
        add(l, &name, kind, "usb");
    }
}

// ---- input devices ----

/// Bit `bit` of a capabilities attribute: hexadecimal words, the highest first.
fn cap_bit(root: &str, dir: &str, name: &str, bit: usize) -> bool {
    let v = attr(root, &format!("{dir}/capabilities"), name, 1024);
    let w = size_of::<libc::c_ulong>() * 8;
    let mut e = v.len();
    let mut word = 0;
    let b = v.as_bytes();
    while e > 0 {
        let mut s = e;
        while s > 0 && b[s - 1] != b' ' {
            s -= 1;
        }
        if word == bit / w {
            let n = u64::from_str_radix(v[s..e].trim_start_matches("0x"), 16).unwrap_or_else(|_| {
                // strtoul: the hex digits at the start
                v[s..e].bytes().map_while(|c| (c as char).to_digit(16)).fold(0u64, |a, d| a << 4 | d as u64)
            });
            return (n >> (bit % w)) & 1 == 1;
        }
        e = s;
        while e > 0 && b[e - 1] == b' ' {
            e -= 1;
        }
        word += 1;
    }
    false
}

const REL_X: usize = 0;
const KEY_Q: usize = 16;
const KEY_A: usize = 30;
const BTN_LEFT: usize = 272;
const BTN_SOUTH: usize = 304;
const BTN_TOOL_FINGER: usize = 325;
const BTN_TOUCH: usize = 330;

fn input_kind(root: &str, dir: &str) -> Option<&'static str> {
    if cap_bit(root, dir, "key", BTN_TOUCH) && cap_bit(root, dir, "key", BTN_TOOL_FINGER) {
        Some("touchpad")
    } else if cap_bit(root, dir, "rel", REL_X) && cap_bit(root, dir, "key", BTN_LEFT) {
        Some("mouse")
    } else if cap_bit(root, dir, "key", KEY_A) && cap_bit(root, dir, "key", KEY_Q) {
        Some("keyboard")
    } else if cap_bit(root, dir, "key", BTN_SOUTH) {
        Some("gamepad")
    } else {
        None // power button, media keys, a monitor's jack
    }
}

fn is_hid_id(s: &str) -> bool {
    // "0003:046D:4054.000D"
    let b = s.as_bytes();
    b.len() == 19 && b[4] == b':' && b[9] == b':' && b[14] == b'.' && b.iter().enumerate().all(|(i, c)| matches!(i, 4 | 9 | 14) || c.is_ascii_hexdigit())
}

/// A device behind a receiver is a HID device inside the receiver's: two ids in a row in its sysfs path. The inner
/// one is returned.
fn behind_receiver(path: &str) -> Option<String> {
    let mut count = 0;
    for part in path.split('/') {
        if is_hid_id(part) {
            count += 1;
            if count == 2 {
                return Some(fit(part, 24));
            }
        } else {
            count = 0;
        }
    }
    None
}

fn collect_input(l: &mut Vec<Periph>, root: &str) {
    let mut v = list_dir(root, "/sys/class/input");
    v.sort();
    for n in &v {
        if !n.starts_with("input") || !n.as_bytes().get(5).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        let dir = format!("/sys/class/input/{n}");
        let link = std::fs::read_link(format!("{root}{dir}")).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let b = strtol16(&attr(root, &format!("{dir}/id"), "bustype", 8));
        let mut hid = String::new();
        let conn: Option<&'static str> = match b {
            0x03 => behind_receiver(&link).map(|h| {
                hid = h;
                "receiver"
            }), // else: read as a USB device
            0x11 | 0x18 => Some("internal"), // i8042, I2C: a laptop's keyboard and touchpad
            _ => None,
        };
        let kind = conn.and_then(|_| input_kind(root, &dir)); // Bluetooth (0x05) comes from BlueZ
        let (Some(kind), Some(conn)) = (kind, conn) else { continue };
        let name = attr(root, &dir, "name", NAME_MAX);
        if name.is_empty() {
            continue;
        }
        if let Some(x) = add(l, &name, kind, conn) {
            x.hid = hid;
        }
    }
}

/// The batteries of the devices behind a receiver: a power supply of scope "Device" under the same HID id, whose
/// model name is also better than the generic one the input node has.
fn collect_batteries(l: &mut [Periph], root: &str) {
    let mut v = list_dir(root, "/sys/class/power_supply");
    v.sort();
    for n in &v {
        let dir = format!("/sys/class/power_supply/{n}");
        if attr(root, &dir, "scope", 16) != "Device" {
            continue;
        }
        let Ok(link) = std::fs::read_link(format!("{root}{dir}/device")) else { continue };
        let link = link.to_string_lossy().into_owned();
        let base = link.rsplit('/').next().unwrap_or(&link).to_string();
        let cap = attr(root, &dir, "capacity", 8);
        let model = attr(root, &dir, "model_name", NAME_MAX);
        for x in l.iter_mut() {
            if x.hid.is_empty() || x.hid != base {
                continue;
            }
            if !cap.is_empty() && cap.bytes().all(|c| c.is_ascii_digit()) && atoi(&cap) <= 100 {
                x.battery = atoi(&cap) as i32;
            }
            if !model.is_empty() {
                x.name = model.clone();
            }
        }
    }
}

/// `[{ name, kind, connection, battery }]`: kind: keyboard, mouse, receiver, touchpad, gamepad, input, audio,
/// camera, storage, printer, network, bluetooth, other; connection: usb, receiver, internal; battery: 0..100, or
/// -1 unknown. `root` prefixes every path; `usb_ids` is the name list (`None`: the Debian path under root).
pub fn periph_json(root: &str, usb_ids: Option<&str>) -> String {
    let db = ids::load(usb_ids.map_or_else(|| format!("{root}/usr/share/misc/usb.ids"), str::to_string).as_bytes());
    let mut l: Vec<Periph> = Vec::new();
    collect_usb(&mut l, root, db.as_deref());
    collect_input(&mut l, root);
    collect_batteries(&mut l, root);
    let mut o = String::from("[");
    for (i, x) in l.iter().enumerate() {
        o.push_str(if i > 0 { ",{\"name\":" } else { "{\"name\":" });
        o.push('"');
        for c in x.name.bytes() {
            match c {
                b'"' | b'\\' => {
                    o.push('\\');
                    o.push(c as char);
                }
                c if c < 0x20 => o.push_str(&format!("\\u{c:04x}")),
                c => o.push(c as char),
            }
        }
        o.push_str(&format!("\",\"kind\":\"{}\",\"connection\":\"{}\",\"battery\":{}}}", x.kind, x.conn, x.battery));
    }
    o.push(']');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_ids_and_receivers() {
        assert!(is_hid_id("0003:046D:4054.000D") && !is_hid_id("0003:046D:4054.00") && !is_hid_id("0003-046D:4054.000D"));
        let p = "../../devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.2/0003:046D:C52B.0003/0003:046D:4054.000D/input/input27";
        assert_eq!(behind_receiver(p).as_deref(), Some("0003:046D:4054.000D"));
        assert_eq!(behind_receiver("../../devices/usb1/1-2/1-2:1.0/0003:046D:C52B.0003/input/input5"), None);
        assert!(named_receiver("Logitech USB Receiver") && named_receiver("UNIFYING thing") && !named_receiver("Keyboard"));
        assert_eq!((strtol16("0x1f"), strtol16("c52b"), strtol16(""), strtol16("zz")), (31, 0xc52b, 0, 0));
    }
}
