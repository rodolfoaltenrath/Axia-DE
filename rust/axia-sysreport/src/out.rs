//! The report as JSON (schema 1) and as a Markdown summary. Both print only the structure's fields,
//! so neither can carry more than the collector kept.
use crate::collect::Report;

/// A JSON object or array being written, with its separators.
struct J {
    b: String,
}

impl J {
    fn s(&mut self, v: &str) {
        self.b.push('"');
        for c in v.bytes() {
            match c {
                b'"' | b'\\' => {
                    self.b.push('\\');
                    self.b.push(c as char);
                }
                c if c < 0x20 => self.b.push_str(&format!("\\u{c:04x}")),
                c => self.b.push(c as char),
            }
        }
        self.b.push('"');
    }
    fn key(&mut self, first: &mut bool, key: &str) {
        self.b.push_str(if *first { "" } else { "," });
        *first = false;
        self.b.push_str(&format!("\"{key}\":"));
    }
    fn js(&mut self, f: &mut bool, key: &str, v: &str) {
        self.key(f, key);
        self.s(v);
    }
    fn ji(&mut self, f: &mut bool, key: &str, v: i64) {
        self.key(f, key);
        self.b.push_str(&v.to_string());
    }
    fn jb(&mut self, f: &mut bool, key: &str, v: bool) {
        self.key(f, key);
        self.b.push_str(if v { "true" } else { "false" });
    }
    /// `unknown` (-1 or INT32_MIN) means null.
    fn jn(&mut self, f: &mut bool, key: &str, v: i64, unknown: i64) {
        self.key(f, key);
        if v == unknown { self.b.push_str("null") } else { self.b.push_str(&v.to_string()) }
    }
}

/// The report as JSON (schema 1), one line and a newline.
pub fn json(r: &Report) -> String {
    let mut j = J { b: String::from("{") };
    let mut f = true;
    j.ji(&mut f, "schema", r.schema as i64);
    j.js(&mut f, "date", &r.date);

    let m = &r.machine;
    j.key(&mut f, "machine");
    j.b.push('{');
    let mut g = true;
    j.js(&mut g, "vendor", &m.vendor);
    j.js(&mut g, "product", &m.product);
    j.js(&mut g, "version", &m.version);
    j.js(&mut g, "boardVendor", &m.board_vendor);
    j.js(&mut g, "boardName", &m.board_name);
    j.js(&mut g, "boardVersion", &m.board_version);
    j.js(&mut g, "chassis", &m.chassis);
    j.js(&mut g, "model", &m.model);
    j.js(&mut g, "biosVendor", &m.bios_vendor);
    j.js(&mut g, "biosVersion", &m.bios_version);
    j.js(&mut g, "biosDate", &m.bios_date);
    j.js(&mut g, "firmware", &m.firmware);
    j.key(&mut g, "secureBoot");
    j.b.push_str(if m.secure_boot < 0 { "null" } else if m.secure_boot != 0 { "true" } else { "false" });
    j.js(&mut g, "virtualization", &m.virt);
    j.b.push('}');

    let c = &r.cpu;
    j.key(&mut f, "cpu");
    j.b.push('{');
    g = true;
    j.js(&mut g, "model", &c.model);
    j.js(&mut g, "vendor", &c.vendor);
    j.js(&mut g, "arch", &c.arch);
    j.ji(&mut g, "sockets", c.sockets as i64);
    j.ji(&mut g, "cores", c.cores as i64);
    j.ji(&mut g, "threads", c.threads as i64);
    j.ji(&mut g, "minMHz", c.min_mhz as i64);
    j.ji(&mut g, "maxMHz", c.max_mhz as i64);
    j.js(&mut g, "governor", &c.governor);
    j.ji(&mut g, "l1dKB", c.l1d_kb as i64);
    j.ji(&mut g, "l1iKB", c.l1i_kb as i64);
    j.ji(&mut g, "l2KB", c.l2_kb as i64);
    j.ji(&mut g, "l3KB", c.l3_kb as i64);
    j.js(&mut g, "flags", &c.flags);
    j.js(&mut g, "microcode", &c.microcode);
    j.b.push('}');

    j.key(&mut f, "memory");
    j.b.push('{');
    g = true;
    j.ji(&mut g, "totalKB", r.mem.total_kb as i64);
    j.ji(&mut g, "swapKB", r.mem.swap_kb as i64);
    j.key(&mut g, "zram");
    j.b.push('[');
    for (i, (kb, algo)) in r.mem.zram.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.ji(&mut h, "sizeKB", *kb as i64);
        j.js(&mut h, "algorithm", algo);
        j.b.push('}');
    }
    j.b.push_str("]}");

    j.key(&mut f, "gpus");
    j.b.push('[');
    for (i, x) in r.gpu.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "card", &x.card);
        j.js(&mut h, "bus", &x.bus);
        j.js(&mut h, "vendorId", &x.vendor_id);
        j.js(&mut h, "deviceId", &x.device_id);
        j.js(&mut h, "vendor", &x.vendor);
        j.js(&mut h, "device", &x.device);
        j.js(&mut h, "driver", &x.driver);
        j.jb(&mut h, "bootVga", x.boot_vga);
        j.jb(&mut h, "renderNode", x.render_node);
        j.ji(&mut h, "connectors", x.connectors as i64);
        j.ji(&mut h, "connected", x.connected as i64);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "displays");
    j.b.push('[');
    for (i, x) in r.display.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "card", &x.card);
        j.js(&mut h, "connector", &x.connector);
        j.jb(&mut h, "enabled", x.enabled);
        j.js(&mut h, "name", &x.name);
        j.js(&mut h, "manufacturer", &x.pnp);
        j.js(&mut h, "product", &x.product);
        j.ji(&mut h, "year", x.year as i64);
        j.ji(&mut h, "widthCm", x.width_cm as i64);
        j.ji(&mut h, "heightCm", x.height_cm as i64);
        j.ji(&mut h, "diagonalTenths", x.diag_tenths as i64);
        j.ji(&mut h, "nativeWidth", x.pref_w as i64);
        j.ji(&mut h, "nativeHeight", x.pref_h as i64);
        j.js(&mut h, "mode", &x.mode);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "storage");
    j.b.push('[');
    for (i, x) in r.disk.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "name", &x.name);
        j.js(&mut h, "model", &x.model);
        j.js(&mut h, "vendor", &x.vendor);
        j.ji(&mut h, "bytes", x.bytes as i64);
        j.jb(&mut h, "rotational", x.rotational);
        j.jb(&mut h, "removable", x.removable);
        j.js(&mut h, "transport", &x.transport);
        j.js(&mut h, "link", &x.link);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "pci");
    j.b.push('[');
    for (i, x) in r.pci.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "class", &x.slot_class);
        j.js(&mut h, "className", &x.class_name);
        j.js(&mut h, "vendorId", &x.vendor_id);
        j.js(&mut h, "deviceId", &x.device_id);
        j.js(&mut h, "vendor", &x.vendor);
        j.js(&mut h, "device", &x.device);
        j.js(&mut h, "driver", &x.driver);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "usb");
    j.b.push('[');
    for (i, x) in r.usb.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "vendorId", &x.vendor_id);
        j.js(&mut h, "productId", &x.product_id);
        j.js(&mut h, "vendor", &x.vendor);
        j.js(&mut h, "product", &x.product);
        j.js(&mut h, "class", &x.cls);
        j.ji(&mut h, "speedMbps", x.speed_mbps as i64);
        j.js(&mut h, "driver", &x.driver);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "network");
    j.b.push('[');
    for (i, x) in r.net.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "kind", &x.kind);
        j.js(&mut h, "driver", &x.driver);
        j.jb(&mut h, "up", x.up);
        j.jn(&mut h, "speedMbps", x.speed_mbps as i64, -1);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "audio");
    j.b.push('[');
    for (i, a) in r.audio.iter().enumerate() {
        if i > 0 {
            j.b.push(',');
        }
        j.s(a);
    }
    j.b.push(']');
    j.key(&mut f, "bluetooth");
    j.b.push('[');
    for (i, d) in r.bt_driver.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "driver", d);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "power");
    j.b.push('[');
    for (i, x) in r.power.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "kind", &x.kind);
        j.js(&mut h, "vendor", &x.vendor);
        j.js(&mut h, "model", &x.model);
        j.js(&mut h, "technology", &x.technology);
        j.js(&mut h, "status", &x.status);
        j.jn(&mut h, "capacity", x.capacity as i64, -1);
        j.jn(&mut h, "health", x.health as i64, -1);
        j.jn(&mut h, "online", x.online as i64, -1);
        j.b.push('}');
    }
    j.b.push(']');

    j.key(&mut f, "thermal");
    j.b.push('[');
    for (i, x) in r.thermal.iter().enumerate() {
        let mut h = true;
        j.b.push_str(if i > 0 { ",{" } else { "{" });
        j.js(&mut h, "sensor", &x.kind);
        j.jn(&mut h, "milliC", x.milli_c as i64, i32::MIN as i64);
        j.ji(&mut h, "fans", x.fans as i64);
        j.b.push('}');
    }
    j.b.push(']');

    if r.pi.is_pi {
        j.key(&mut f, "raspberryPi");
        j.b.push('{');
        g = true;
        j.js(&mut g, "model", &r.pi.model);
        j.js(&mut g, "revision", &r.pi.revision);
        j.js(&mut g, "bootloaderVersion", &r.pi.bootloader_version);
        j.js(&mut g, "bootloaderDate", &r.pi.bootloader_date);
        j.jn(&mut g, "throttled", r.pi.throttled as i64, -1);
        if r.pi.uv_alarm >= 0 {
            j.jb(&mut g, "underVoltageAlarm", r.pi.uv_alarm != 0);
        } else {
            j.key(&mut g, "underVoltageAlarm");
            j.b.push_str("null");
        }
        j.b.push('}');
    }

    let s = &r.sw;
    j.key(&mut f, "software");
    j.b.push('{');
    g = true;
    j.js(&mut g, "os", &s.os);
    j.js(&mut g, "osId", &s.os_id);
    j.js(&mut g, "osVersion", &s.os_version);
    j.js(&mut g, "kernel", &s.kernel);
    j.js(&mut g, "lordos", &s.lordos);
    j.js(&mut g, "lsm", &s.lsm);
    j.jn(&mut g, "landlockAbi", s.landlock_abi as i64, -1);
    j.jb(&mut g, "bpfLsm", s.bpf_lsm);
    j.jb(&mut g, "cgroup2", s.cgroup2);
    j.jb(&mut g, "live", s.live);
    j.js(&mut g, "bootMedium", &s.boot_medium);
    j.jn(&mut g, "bootLinkMbps", s.boot_link_mbps as i64, -1);
    j.jb(&mut g, "bootToRam", s.boot_toram);
    j.key(&mut g, "kernelConfig");
    j.b.push('{');
    for (i, (name, value)) in s.kconf.iter().enumerate() {
        if i > 0 {
            j.b.push(',');
        }
        j.s(name);
        j.b.push(':');
        if value.is_empty() { j.b.push_str("null") } else { j.s(value) }
    }
    j.b.push('}');
    j.key(&mut g, "services");
    j.b.push('{');
    let mut h = true;
    j.jb(&mut h, "lord-sessiond", s.sessiond);
    j.jb(&mut h, "lord-netd", s.netd);
    j.jb(&mut h, "axia-netperm", s.netperm);
    j.jb(&mut h, "axia-fand", s.fand);
    j.b.push_str("}}");

    j.b.push_str("}\n");
    j.b
}

// ---- Markdown ----

/// A table cell: '|' and '`' would break the table; "—" for empty.
fn cell(b: &mut String, s: &str) {
    if s.is_empty() {
        b.push_str(" — |");
        return;
    }
    b.push(' ');
    for c in s.chars() {
        b.push(if c == '|' || c == '`' { '/' } else { c });
    }
    b.push_str(" |");
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1} TB", bytes as f64 / 1e12)
    } else if bytes >= 1_000_000_000 {
        format!("{:.0} GB", bytes as f64 / 1e9)
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

fn two(a: &str, b: &str) -> String {
    format!("{a}{}{b}", if !a.is_empty() && !b.is_empty() { " " } else { "" })
}

/// The report as a Markdown summary.
pub fn markdown(r: &Report) -> String {
    let mut b = String::new();
    let m = &r.machine;
    b.push_str("# System report\n\n");
    b.push_str(&format!(
        "Made on {} by Axia DE's System Information (schema {}). It lists the hardware and what the system made of it; it holds no serial numbers, addresses, host or user names.\n\n",
        r.date, r.schema
    ));

    b.push_str("## Overview\n\n| | |\n|---|---|\n");
    let mut t = two(&m.vendor, &m.product);
    if t.is_empty() {
        t = m.model.clone();
    }
    b.push_str("| Computer |");
    cell(&mut b, &t);
    b.push('\n');
    b.push_str("| Kind |");
    cell(&mut b, if m.virt.is_empty() { &m.chassis } else { &m.virt });
    b.push('\n');
    let mut t = two(&m.board_vendor, &m.board_name);
    if !m.board_version.is_empty() && t.len() + m.board_version.len() + 2 < 256 {
        t.push_str(", ");
        t.push_str(&m.board_version);
    }
    b.push_str("| Board |");
    cell(&mut b, &t);
    b.push('\n');
    let t = format!(
        "{}{}{}{}{}",
        m.firmware,
        if m.bios_version.is_empty() { "" } else { ", " },
        m.bios_version,
        if m.bios_date.is_empty() { "" } else { ", " },
        m.bios_date
    );
    b.push_str("| Firmware |");
    cell(&mut b, &t);
    b.push('\n');
    b.push_str("| Secure Boot |");
    cell(&mut b, if m.secure_boot < 0 { "" } else if m.secure_boot != 0 { "on" } else { "off" });
    b.push('\n');
    b.push_str("| System |");
    cell(&mut b, &r.sw.os);
    b.push('\n');
    b.push_str("| Kernel |");
    cell(&mut b, &r.sw.kernel);
    b.push('\n');
    b.push_str("| Axia DE |");
    cell(&mut b, &r.sw.lordos);
    b.push('\n');
    if r.sw.live || !r.sw.boot_medium.is_empty() {
        const MEDIA: [(&str, &str); 6] = [
            ("internal", "an internal disk"),
            ("usb", "a USB drive"),
            ("sd", "an SD card"),
            ("emmc", "built-in flash memory (eMMC)"),
            ("optical", "a DVD or CD"),
            ("network", "the network"),
        ];
        let from = MEDIA.iter().filter(|(k, _)| *k == r.sw.boot_medium).map(|(_, v)| *v).last().unwrap_or("");
        let mut bm = format!("{}{}{}", if r.sw.live { "live session" } else { "installed" }, if from.is_empty() { "" } else { ", from " }, from);
        if !from.is_empty() && r.sw.boot_link_mbps > 0 {
            bm.push_str(&format!(" ({} Mbps)", r.sw.boot_link_mbps));
        }
        if r.sw.boot_toram {
            bm.push_str(", copied to memory");
        }
        b.push_str("| Started |");
        cell(&mut b, &bm);
        b.push('\n');
    }
    b.push('\n');

    let c = &r.cpu;
    b.push_str("## Processor\n\n| | |\n|---|---|\n");
    b.push_str("| Model |");
    cell(&mut b, &c.model);
    b.push('\n');
    let t = format!("{}, {} socket{}, {} cores, {} threads", c.arch, c.sockets, if c.sockets == 1 { "" } else { "s" }, c.cores, c.threads);
    b.push_str("| Layout |");
    cell(&mut b, &t);
    b.push('\n');
    if c.max_mhz != 0 {
        let t = format!("{}–{} MHz{}{}", c.min_mhz, c.max_mhz, if c.governor.is_empty() { "" } else { ", governor " }, c.governor);
        b.push_str("| Frequency |");
        cell(&mut b, &t);
        b.push('\n');
    }
    let t = format!("L1d {} KB, L1i {} KB, L2 {} KB, L3 {} KB", c.l1d_kb, c.l1i_kb, c.l2_kb, c.l3_kb);
    b.push_str("| Cache |");
    cell(&mut b, &t);
    b.push('\n');
    b.push_str("| Instructions |");
    cell(&mut b, &c.flags);
    b.push_str("\n\n");

    b.push_str("## Memory\n\n");
    b.push_str(&format!("{}, swap {}", size_text(r.mem.total_kb * 1024), if r.mem.swap_kb != 0 { size_text(r.mem.swap_kb * 1024) } else { "none".into() }));
    for (kb, algo) in &r.mem.zram {
        b.push_str(&format!(", zram {} ({})", size_text(kb * 1024), if algo.is_empty() { "?" } else { algo }));
    }
    b.push_str(".\n\n");

    b.push_str("## Graphics\n\n| Card | Device | Driver | Render node | Connected outputs |\n|---|---|---|---|---|\n");
    for g in &r.gpu {
        b.push('|');
        cell(&mut b, &g.card);
        let mut t = two(&g.vendor, &g.device);
        if t.is_empty() && !g.vendor_id.is_empty() {
            t = format!("{}:{}", g.vendor_id, g.device_id);
        }
        cell(&mut b, &t);
        cell(&mut b, &g.driver);
        cell(&mut b, if g.render_node { "yes" } else { "no" });
        cell(&mut b, &format!("{} of {}", g.connected, g.connectors));
        b.push('\n');
    }
    if r.gpu.is_empty() {
        b.push_str("| — | none found | | | |\n");
    }
    b.push('\n');

    if !r.display.is_empty() {
        b.push_str("## Displays\n\n| Output | Monitor | Size | Native mode |\n|---|---|---|---|\n");
        for x in &r.display {
            b.push('|');
            cell(&mut b, &x.connector);
            let who = if x.name.is_empty() { &x.pnp } else { &x.name };
            let t = if x.year != 0 { format!("{who}, {}", x.year) } else { who.clone() };
            cell(&mut b, &t);
            let t = if x.diag_tenths != 0 { format!("{}.{}\" ({} × {} cm)", x.diag_tenths / 10, x.diag_tenths % 10, x.width_cm, x.height_cm) } else { String::new() };
            cell(&mut b, &t);
            let t = if x.pref_w != 0 { format!("{} × {}", x.pref_w, x.pref_h) } else { x.mode.clone() };
            cell(&mut b, &t);
            b.push('\n');
        }
        b.push('\n');
    }

    b.push_str("## Storage\n\n| Disk | Model | Size | Kind |\n|---|---|---|---|\n");
    for x in &r.disk {
        b.push('|');
        cell(&mut b, &x.name);
        cell(&mut b, &x.model);
        cell(&mut b, &size_text(x.bytes));
        let t = format!(
            "{}{}{}{}{}",
            x.transport,
            if x.rotational { ", spinning" } else { "" },
            if x.removable { ", removable" } else { "" },
            if x.link.is_empty() { "" } else { ", " },
            x.link
        );
        cell(&mut b, &t);
        b.push('\n');
    }
    b.push('\n');

    b.push_str("## Network, audio and Bluetooth\n\n");
    for x in &r.net {
        b.push_str(&format!("- {}{}{}{}", x.kind, if x.driver.is_empty() { "" } else { " (" }, x.driver, if x.driver.is_empty() { "" } else { ")" }));
        if x.up && x.speed_mbps > 0 {
            b.push_str(&format!(", up at {} Mbit/s", x.speed_mbps));
        } else {
            b.push_str(&format!(", {}", if x.up { "up" } else { "down" }));
        }
        b.push('\n');
    }
    for a in &r.audio {
        b.push_str(&format!("- audio: {a}\n"));
    }
    for d in &r.bt_driver {
        b.push_str(&format!("- Bluetooth{}{}\n", if d.is_empty() { "" } else { ": " }, d));
    }
    b.push('\n');

    if !r.power.is_empty() {
        b.push_str("## Power\n\n");
        for x in &r.power {
            if x.kind == "battery" {
                b.push_str("- battery");
                let t = two(&x.vendor, &x.model);
                if !t.is_empty() {
                    b.push_str(&format!(" {t}"));
                }
                if x.capacity >= 0 {
                    b.push_str(&format!(", {} %", x.capacity));
                }
                if x.health >= 0 {
                    b.push_str(&format!(", holds {} % of its design charge", x.health));
                }
                if !x.status.is_empty() {
                    b.push_str(&format!(", {}", x.status));
                }
                b.push('\n');
            } else {
                b.push_str(&format!("- {}{}\n", x.kind, if x.online == 1 { ", plugged in" } else if x.online == 0 { ", unplugged" } else { "" }));
            }
        }
        b.push('\n');
    }

    if r.pi.is_pi {
        b.push_str(&format!("## Raspberry Pi\n\n{}, revision {}", r.pi.model, if r.pi.revision.is_empty() { "unknown" } else { &r.pi.revision }));
        if !r.pi.bootloader_date.is_empty() {
            b.push_str(&format!(", bootloader of {}", r.pi.bootloader_date));
        }
        // rpi_volt clears the sticky bits every 2 s, so with it they cover that window, not the time since boot
        let since = if r.pi.uv_alarm >= 0 { "in the last 2 s" } else { "since boot" };
        if r.pi.throttled >= 0 {
            let v = r.pi.throttled as u32;
            b.push_str(&format!("; {}", if v & 0x1 != 0 { "under-voltage now" } else if v & 0x10000 != 0 { "under-voltage" } else { "no under-voltage" }));
            if v & 0x1 == 0 {
                b.push_str(&format!(" {since}"));
            }
            b.push_str(&format!("; {}", if v & 0xe != 0 { "slowed down now" } else if v & 0xe0000 != 0 { "slowed down" } else { "not slowed down" }));
            if v & 0xe == 0 {
                b.push_str(&format!(" {since}"));
            }
            b.push_str(&format!(" (get_throttled 0x{v:x})"));
        } else if r.pi.uv_alarm >= 0 {
            b.push_str(&format!("; {} in the last 2 s (rpi_volt); throttling not reported by this kernel", if r.pi.uv_alarm != 0 { "under-voltage" } else { "no under-voltage" }));
        }
        b.push_str(".\n\n");
    }

    let s = &r.sw;
    b.push_str("## Kernel features Axia DE uses\n\n| | |\n|---|---|\n");
    b.push_str("| Security modules |");
    cell(&mut b, &s.lsm);
    b.push('\n');
    if s.landlock_abi >= 0 {
        b.push_str("| Landlock ABI |");
        cell(&mut b, &if s.landlock_abi != 0 { s.landlock_abi.to_string() } else { "unavailable".into() });
        b.push('\n');
    }
    b.push_str("| cgroup v2 |");
    cell(&mut b, if s.cgroup2 { "yes" } else { "no" });
    b.push('\n');
    for (name, value) in &s.kconf {
        b.push('|');
        cell(&mut b, name);
        cell(&mut b, match value.as_str() {
            "y" => "built in",
            "m" => "module",
            "n" => "not set",
            "" => "unknown",
            v => v,
        });
        b.push('\n');
    }
    b.push_str("\n## Devices\n\n| Kind | Device | Driver |\n|---|---|---|\n");
    for x in &r.pci {
        b.push('|');
        cell(&mut b, if x.class_name.is_empty() { &x.slot_class } else { &x.class_name });
        let mut t = two(&x.vendor, &x.device);
        if t.is_empty() {
            t = format!("{}:{}", x.vendor_id, x.device_id);
        }
        cell(&mut b, &t);
        cell(&mut b, &x.driver);
        b.push('\n');
    }
    for x in &r.usb {
        b.push_str("| USB |");
        let mut t = two(&x.vendor, &x.product);
        if t.is_empty() {
            t = format!("{}:{}", x.vendor_id, x.product_id);
        }
        cell(&mut b, &t);
        cell(&mut b, &x.driver);
        b.push('\n');
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_cells() {
        assert_eq!((size_text(32_768_000 * 1024).as_str(), size_text(2_147_483_648).as_str(), size_text(1_000_204_886_016).as_str(), size_text(8_192_000).as_str()), ("34 GB", "2 GB", "1.0 TB", "8 MB"));
        let mut b = String::new();
        cell(&mut b, "a|b`c");
        cell(&mut b, "");
        assert_eq!(b, " a/b/c | — |");
        assert_eq!(two("A", ""), "A");
        assert_eq!(two("A", "B"), "A B");
    }
}
