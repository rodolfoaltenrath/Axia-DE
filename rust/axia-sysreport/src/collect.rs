//! System Information's collector: every read is a small file opened with O_NOFOLLOW where it is a
//! leaf, cut to its first line and to printable ASCII; directories are listed in name order so two
//! runs on one machine give the same report. The structures have no place for serial numbers, UUIDs,
//! MAC or IP addresses, host or user names, Wi-Fi network names or home-folder paths: the report is
//! built from a fixed list of fields (network interfaces are reported by kind and driver, not by
//! name, as a name can carry the MAC).
//!
//! `root` prefixes every path ("" for this machine; tests point it at a fake tree). Facts that only
//! the running system can give (the Landlock ABI) are left unknown under a fake root.
use crate::fsutil::*;
use crate::rpifw;
use axia_codecs::edid::{self, Edid};
use axia_codecs::ids;

pub const SR_NAME: usize = 96;
const MAX_GPU: usize = 8;
const MAX_DISPLAY: usize = 12;
const MAX_DISK: usize = 32;
const MAX_PCI: usize = 96;
const MAX_USB: usize = 64;
const MAX_NET: usize = 16;
const MAX_AUDIO: usize = 8;
const MAX_POWER: usize = 8;
const MAX_THERMAL: usize = 16;
const MAX_KCONF: usize = 16;
const MAX_ZRAM: usize = 4;
const UNKNOWN_TEMP: i32 = i32::MIN;

#[derive(Clone, Debug, Default)]
pub struct Machine {
    pub vendor: String,
    pub product: String,
    pub version: String,
    pub board_vendor: String,
    pub board_name: String,
    pub board_version: String,
    /// "desktop", "laptop", "server", "vm" …, "" unknown.
    pub chassis: String,
    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_date: String,
    /// Device tree model (ARM boards).
    pub model: String,
    /// "uefi", "bios", "dt" (device tree), "".
    pub firmware: String,
    /// 1 on, 0 off, -1 unknown.
    pub secure_boot: i32,
    /// Hypervisor ("kvm", "vmware" …), "" bare metal or unknown.
    pub virt: String,
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub model: String,
    pub vendor: String,
    pub arch: String,
    pub sockets: i32,
    pub cores: i32,
    pub threads: i32,
    pub min_mhz: i32,
    pub max_mhz: i32,
    pub governor: String,
    /// Per core (L1, L2 on most) or shared (L3); 0 unknown.
    pub l1d_kb: i32,
    pub l1i_kb: i32,
    pub l2_kb: i32,
    pub l3_kb: i32,
    /// The instruction sets that matter, space-separated.
    pub flags: String,
    pub microcode: String,
}

#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub total_kb: u64,
    pub swap_kb: u64,
    /// (size in KiB, algorithm).
    pub zram: Vec<(u64, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Gpu {
    pub card: String,
    pub bus: String,
    pub vendor_id: String,
    pub device_id: String,
    pub vendor: String,
    pub device: String,
    pub driver: String,
    pub boot_vga: bool,
    pub render_node: bool,
    pub connectors: i32,
    pub connected: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Display {
    pub connector: String,
    pub card: String,
    pub connected: bool,
    pub enabled: bool,
    pub name: String,
    pub pnp: String,
    pub product: String,
    pub year: i32,
    pub width_cm: i32,
    pub height_cm: i32,
    pub diag_tenths: i32,
    pub pref_w: i32,
    pub pref_h: i32,
    pub mode: String,
}

#[derive(Clone, Debug, Default)]
pub struct Disk {
    pub name: String,
    pub model: String,
    pub vendor: String,
    pub bytes: u64,
    pub rotational: bool,
    pub removable: bool,
    pub transport: String,
    pub link: String,
}

#[derive(Clone, Debug, Default)]
pub struct Pci {
    pub slot_class: String,
    pub vendor_id: String,
    pub device_id: String,
    pub vendor: String,
    pub device: String,
    pub class_name: String,
    pub driver: String,
}

#[derive(Clone, Debug, Default)]
pub struct Usb {
    pub vendor_id: String,
    pub product_id: String,
    pub vendor: String,
    pub product: String,
    pub cls: String,
    pub speed_mbps: i32,
    pub driver: String,
}

#[derive(Clone, Debug, Default)]
pub struct Net {
    /// "ethernet", "wifi", "virtual", "other".
    pub kind: String,
    pub driver: String,
    /// -1 unknown or down.
    pub speed_mbps: i32,
    pub up: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Power {
    /// "battery", "ac", "usb", "ups".
    pub kind: String,
    pub vendor: String,
    pub model: String,
    pub technology: String,
    /// %, -1 unknown.
    pub capacity: i32,
    /// Full charge as % of the design capacity, -1 unknown.
    pub health: i32,
    pub status: String,
    /// AC: 1 plugged, -1 unknown.
    pub online: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Thermal {
    /// Thermal zone type or hwmon chip name.
    pub kind: String,
    /// i32::MIN unknown.
    pub milli_c: i32,
    /// hwmon: fan inputs.
    pub fans: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Pi {
    pub model: String,
    pub revision: String,
    pub bootloader_version: String,
    pub bootloader_date: String,
    pub throttled: i32,
    pub uv_alarm: i32,
    pub is_pi: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Software {
    pub os: String,
    pub os_id: String,
    pub os_version: String,
    pub kernel: String,
    /// LordOS version, filled by the caller.
    pub lordos: String,
    pub lsm: String,
    /// -1 unknown, 0 unavailable.
    pub landlock_abi: i32,
    pub bpf_lsm: bool,
    pub cgroup2: bool,
    pub live: bool,
    /// What the system started from: "internal", "usb", "sd", "emmc", "optical", "network" or "" unknown;
    /// never a device name.
    pub boot_medium: String,
    /// A USB medium's link speed, -1 unknown or not USB.
    pub boot_link_mbps: i32,
    pub boot_toram: bool,
    /// (CONFIG_… name, "y" / "m" / "n" / "?" or "" unknown).
    pub kconf: Vec<(String, String)>,
    /// Their sockets in /run/lordos.
    pub sessiond: bool,
    pub netperm: bool,
    pub fand: bool,
    pub netd: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub schema: i32,
    /// YYYY-MM-DD.
    pub date: String,
    pub machine: Machine,
    pub cpu: Cpu,
    pub mem: Memory,
    pub gpu: Vec<Gpu>,
    pub display: Vec<Display>,
    pub disk: Vec<Disk>,
    pub pci: Vec<Pci>,
    pub usb: Vec<Usb>,
    pub net: Vec<Net>,
    pub audio: Vec<String>,
    pub bt_driver: Vec<String>,
    pub power: Vec<Power>,
    pub thermal: Vec<Thermal>,
    pub pi: Pi,
    pub sw: Software,
}

// ---- machine ----

fn chassis_name(t: i64) -> &'static str {
    match t {
        3..=7 | 15 | 16 | 24 => "desktop",
        8..=10 | 14 | 31 | 32 => "laptop",
        11 => "handheld",
        13 => "all-in-one",
        17 | 23 | 25 | 28 | 29 => "server",
        30 => "tablet",
        35 | 36 => "mini-pc",
        1 => "other",
        _ => "",
    }
}

/// DMI vendor strings firmware leaves unset.
fn drop_placeholder(s: &mut String) {
    const JUNK: [&str; 10] = [
        "To be filled by O.E.M.", "To Be Filled By O.E.M.", "Default string", "System Product Name", "System manufacturer", "Not Applicable",
        "Not Specified", "None", "0123456789", "Type1ProductConfigId",
    ];
    if JUNK.contains(&s.as_str()) {
        s.clear();
    }
}

fn collect_machine(r: &mut Report, root: &str, cpuinfo: &str) {
    let m = &mut r.machine;
    let dmi = "/sys/class/dmi/id";
    let a = |n: &str| attr(root, dmi, n, SR_NAME);
    m.vendor = a("sys_vendor");
    m.product = a("product_name");
    m.version = a("product_version");
    m.board_vendor = a("board_vendor");
    m.board_name = a("board_name");
    m.board_version = a("board_version");
    m.bios_vendor = a("bios_vendor");
    m.bios_version = a("bios_version");
    m.bios_date = attr(root, dmi, "bios_date", 24);
    for s in [&mut m.vendor, &mut m.product, &mut m.version, &mut m.board_vendor, &mut m.board_name, &mut m.board_version] {
        drop_placeholder(s);
    }
    m.chassis = chassis_name(attr_num(root, dmi, "chassis_type", 2)).to_string();
    m.model = read_attr(&format!("{root}/proc/device-tree/model"), SR_NAME);

    if exists(root, "/sys/firmware/efi") {
        m.firmware = "uefi".into();
    } else if !m.bios_vendor.is_empty() {
        m.firmware = "bios".into();
    } else if !m.model.is_empty() {
        m.firmware = "dt".into();
    }
    m.secure_boot = -1;
    if let Some(sb) = read_file(&format!("{root}/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c"), 16) {
        if sb.len() >= 5 {
            m.secure_boot = (sb[4] == 1) as i32; // 4 bytes of attributes, then the value
        }
    }
    // virtualization: the CPU's "hypervisor" flag, then who the DMI says made the machine
    let flags = field(cpuinfo, "flags", 4096);
    let mut hv = flags.contains(" hypervisor");
    let compat = read_attr(&format!("{root}/proc/device-tree/compatible"), 256);
    if compat.starts_with("linux,dummy-virt") {
        hv = true;
    }
    if hv {
        let v = if m.vendor.contains("QEMU") || compat.contains("dummy-virt") {
            "kvm"
        } else if m.vendor.contains("VMware") {
            "vmware"
        } else if m.vendor.contains("innotek") || m.product.contains("VirtualBox") {
            "virtualbox"
        } else if m.vendor.contains("Microsoft") {
            "hyper-v"
        } else if m.vendor.contains("Xen") {
            "xen"
        } else if m.vendor.contains("Parallels") {
            "parallels"
        } else {
            "vm"
        };
        m.virt = v.into();
        if m.chassis.is_empty() || m.chassis == "other" {
            m.chassis = "vm".into();
        }
    }
}

// ---- CPU and memory ----

fn arm_core(part: u64) -> Option<&'static str> {
    Some(match part {
        0xd03 => "Cortex-A53",
        0xd04 => "Cortex-A35",
        0xd05 => "Cortex-A55",
        0xd07 => "Cortex-A57",
        0xd08 => "Cortex-A72",
        0xd09 => "Cortex-A73",
        0xd0a => "Cortex-A75",
        0xd0b => "Cortex-A76",
        0xd0c => "Neoverse-N1",
        0xd0d => "Cortex-A77",
        0xd41 => "Cortex-A78",
        0xd44 => "Cortex-X1",
        0xd46 => "Cortex-A510",
        0xd47 => "Cortex-A710",
        0xd80 => "Cortex-A520",
        0xd81 => "Cortex-A720",
        _ => return None,
    })
}

/// "48K", "1024 K", "32M" → KiB.
fn cache_kb(s: &str) -> i32 {
    let t = s.trim_start();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    let mut v = atoi(s);
    if t[digits..].trim_start_matches(' ').starts_with('M') {
        v *= 1024;
    }
    if v > 0 && v < 1 << 30 { v as i32 } else { 0 }
}

fn strtoul0(s: &str) -> u64 {
    strtoll0(s).map_or(0, |v| v as u64)
}

fn collect_cpu(r: &mut Report, root: &str, cpuinfo: &str) {
    let c = &mut r.cpu;
    c.model = field(cpuinfo, "model name", SR_NAME);
    c.vendor = field(cpuinfo, "vendor_id", 32);
    c.microcode = field(cpuinfo, "microcode", 24);
    if c.vendor.is_empty() {
        let (imp, part) = (field(cpuinfo, "CPU implementer", 16), field(cpuinfo, "CPU part", 16));
        if !imp.is_empty() {
            c.vendor = match strtoul0(&imp) {
                0x41 => "ARM",
                0x51 => "Qualcomm",
                0x61 => "Apple",
                _ => "other",
            }
            .into();
        }
        if c.model.is_empty() && !part.is_empty() {
            c.model = arm_core(strtoul0(&part)).unwrap_or("").into();
        }
    }
    c.arch = read_attr(&format!("{root}/proc/sys/kernel/arch"), 16);
    if c.arch.is_empty() && root.is_empty() {
        // SAFETY: uname fills the struct.
        let mut u: libc::utsname = unsafe { std::mem::zeroed() };
        if unsafe { libc::uname(&mut u) } == 0 {
            let m: Vec<u8> = u.machine.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            c.arch = fit(&String::from_utf8_lossy(&m), 16);
        }
    }
    // instruction sets that matter for LordOS's own code and for apps
    const X86: [&str; 8] = ["sse4_2", "avx", "avx2", "fma", "avx512f", "aes", "sha_ni", "vaes"];
    const ARM: [&str; 7] = ["asimd", "sve", "sve2", "aes", "sha2", "crc32", "atomics"];
    let mut flags = field(cpuinfo, "flags", 4096);
    let is_arm = flags.is_empty();
    if is_arm {
        flags = field(cpuinfo, "Features", 4096);
    }
    let have: Vec<&str> = flags.split(' ').collect();
    let mut out = String::new();
    for w in if is_arm { &ARM[..] } else { &X86[..] } {
        if have.contains(w) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(w);
            out = fit(&out, 160);
        }
    }
    c.flags = out;

    // topology: unique (package, core) pairs among the CPUs listed in sysfs
    let cpus = list_dir(root, "/sys/devices/system/cpu");
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    let mut maxpk: i64 = -1;
    for n in &cpus {
        if !n.starts_with("cpu") || !n.as_bytes().get(3).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        let d = format!("/sys/devices/system/cpu/{n}");
        if attr_num(root, &d, "online", 1) == 0 {
            continue;
        }
        c.threads += 1;
        let t = format!("{d}/topology");
        let pkg = attr_num(root, &t, "physical_package_id", 0);
        let core = attr_num(root, &t, "core_id", c.threads as i64);
        maxpk = maxpk.max(pkg);
        if !pairs.contains(&(pkg, core)) && pairs.len() < 1024 {
            pairs.push((pkg, core));
        }
    }
    c.cores = pairs.len() as i32;
    c.sockets = (maxpk + 1) as i32;
    if c.threads == 0 {
        // no sysfs: count /proc/cpuinfo's processors
        c.threads = cpuinfo.lines().filter(|l| l.starts_with("processor")).count() as i32;
        c.cores = c.threads;
        c.sockets = (c.threads != 0) as i32;
    }
    let f = "/sys/devices/system/cpu/cpu0/cpufreq";
    c.min_mhz = (attr_num(root, f, "cpuinfo_min_freq", 0) / 1000) as i32;
    c.max_mhz = (attr_num(root, f, "cpuinfo_max_freq", 0) / 1000) as i32;
    c.governor = attr(root, f, "scaling_governor", 24);
    for idx in list_dir(root, "/sys/devices/system/cpu/cpu0/cache") {
        if !idx.starts_with("index") {
            continue;
        }
        let d = format!("/sys/devices/system/cpu/cpu0/cache/{idx}");
        let lvl = attr_num(root, &d, "level", 0);
        let kind = attr(root, &d, "type", 24);
        let kb = cache_kb(&attr(root, &d, "size", 24));
        if lvl == 1 && kind == "Data" {
            c.l1d_kb = kb;
        } else if lvl == 1 && kind == "Instruction" {
            c.l1i_kb = kb;
        } else if lvl == 2 {
            c.l2_kb = kb;
        } else if lvl == 3 {
            c.l3_kb = kb;
        }
    }
}

fn strtoull10(s: &str) -> u64 {
    s.trim_start().bytes().take_while(u8::is_ascii_digit).fold(0u64, |a, c| a.saturating_mul(10).saturating_add((c - b'0') as u64))
}

fn collect_memory(r: &mut Report, root: &str) {
    if let Some(t) = read_text(&format!("{root}/proc/meminfo"), 16384) {
        r.mem.total_kb = strtoull10(&field(&t, "MemTotal", 64));
        r.mem.swap_kb = strtoull10(&field(&t, "SwapTotal", 64));
    }
    for b in list_dir(root, "/sys/block") {
        if r.mem.zram.len() >= MAX_ZRAM {
            break;
        }
        if !b.starts_with("zram") {
            continue;
        }
        let d = format!("/sys/block/{b}");
        let size = attr_num(root, &d, "disksize", 0);
        if size <= 0 {
            continue;
        }
        // "lzo [lz4] zstd": the one in brackets
        let algo = attr(root, &d, "comp_algorithm", 128);
        let name = match (algo.find('['), algo.find(']')) {
            (Some(a), Some(e)) if e > a => fit(&algo[a + 1..e], 16),
            _ => String::new(),
        };
        r.mem.zram.push((size as u64 / 1024, name));
    }
}

// ---- graphics and displays ----

fn collect_gpus(r: &mut Report, root: &str, pci: Option<&[u8]>) {
    let e = list_dir(root, "/sys/class/drm");
    for n in &e {
        if r.gpu.len() >= MAX_GPU {
            break;
        }
        if !n.starts_with("card") || n.contains('-') {
            continue;
        }
        let mut g = Gpu { card: fit(n, 12), ..Gpu::default() };
        let d = format!("/sys/class/drm/{n}/device");
        g.driver = link_base(root, &d, "driver", 32);
        g.vendor_id = hex_attr(root, &d, "vendor", 4);
        g.device_id = hex_attr(root, &d, "device", 4);
        g.bus = if g.vendor_id.is_empty() { "platform" } else { "pci" }.into();
        if !g.vendor_id.is_empty() {
            let (v, dn) = lookup_ids(pci, &g.vendor_id, Some(&g.device_id));
            g.vendor = v;
            g.device = dn;
        }
        g.boot_vga = attr_num(root, &d, "boot_vga", 0) == 1;
        g.render_node = list_dir(root, &format!("{d}/drm")).iter().any(|x| x.starts_with("renderD"));
        for c in &e {
            if !c.starts_with(n.as_str()) || c.as_bytes().get(n.len()) != Some(&b'-') || c.contains("Writeback") {
                continue;
            }
            let st = attr(root, &format!("/sys/class/drm/{c}"), "status", 24);
            g.connectors += 1;
            if st == "connected" {
                g.connected += 1;
            }
        }
        r.gpu.push(g);
    }
}

/// vendor and device names from a hwdata list (both "" when it is not there or not listed).
fn lookup_ids(db: Option<&[u8]>, vendor: &str, device: Option<&str>) -> (String, String) {
    let Some(t) = db else { return (String::new(), String::new()) };
    let (v, d) = ids::lookup(t, vendor.as_bytes(), device.map(str::as_bytes), SR_NAME, SR_NAME);
    (String::from_utf8_lossy(ids::name_text(&v)).into_owned(), String::from_utf8_lossy(ids::name_text(&d)).into_owned())
}

fn collect_displays(r: &mut Report, root: &str) {
    for n in list_dir(root, "/sys/class/drm") {
        if r.display.len() >= MAX_DISPLAY {
            break;
        }
        let Some(dash) = n.find('-') else { continue };
        if !n.starts_with("card") || n.contains("Writeback") {
            continue;
        }
        let d = format!("/sys/class/drm/{n}");
        if attr(root, &d, "status", 24) != "connected" {
            continue;
        }
        let mut x = Display { connected: true, ..Display::default() };
        x.enabled = attr(root, &d, "enabled", 24) == "enabled";
        x.card = n[..dash.min(11)].to_string();
        x.connector = fit(&n[dash + 1..], 32);
        x.mode = attr(root, &d, "modes", 24);
        if let Some(raw) = read_file(&format!("{root}{d}/edid"), 512) {
            let mut ed = Edid::zeroed();
            if raw.len() >= 128 && edid::parse(&raw, &mut ed) {
                // the identity without the serial: name, maker, product, year, size, native mode
                x.name = fit(&String::from_utf8_lossy(ed.name_text()), 16);
                x.pnp = fit(&String::from_utf8_lossy(ed.vendor_text()), 4);
                x.product = format!("{:04x}", ed.product);
                x.year = ed.year;
                x.width_cm = ed.width_cm;
                x.height_cm = ed.height_cm;
                x.diag_tenths = edid::diagonal_tenths(&ed);
                x.pref_w = ed.pref_w;
                x.pref_h = ed.pref_h;
            }
        }
        r.display.push(x);
    }
}

// ---- storage ----

fn collect_disks(r: &mut Report, root: &str) {
    const SKIP: [&str; 8] = ["loop", "ram", "zram", "dm-", "md", "nbd", "mtdblock", "fd"];
    for b in list_dir(root, "/sys/block") {
        if r.disk.len() >= MAX_DISK {
            break;
        }
        if b.contains("boot") || b.contains("rpmb") || SKIP.iter().any(|p| b.starts_with(p)) {
            continue;
        }
        let d = format!("/sys/block/{b}");
        let sectors = attr_num(root, &d, "size", 0);
        if sectors <= 0 {
            continue; // an empty card reader
        }
        let mut k = Disk { name: fit(&b, 16), bytes: sectors as u64 * 512, ..Disk::default() };
        k.rotational = attr_num(root, &format!("{d}/queue"), "rotational", 0) == 1;
        k.removable = attr_num(root, &d, "removable", 0) == 1;
        let q = format!("{d}/device");
        k.model = attr(root, &q, "model", SR_NAME);
        k.vendor = attr(root, &q, "vendor", SR_NAME);
        if b.starts_with("nvme") {
            k.transport = "nvme".into();
            let q = format!("{d}/device/device");
            let (spd, wid) = (attr(root, &q, "current_link_speed", 32), attr(root, &q, "current_link_width", 8));
            if !spd.is_empty() && !wid.is_empty() {
                k.link = fit(&format!("{} x{}", fit(&spd, 21), fit(&wid, 5)), 32);
            }
        } else if b.starts_with("mmcblk") {
            k.transport = "mmc".into();
        } else if b.starts_with("vd") {
            k.transport = "virtio".into();
        } else {
            let l = std::fs::read_link(format!("{root}{d}")).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            k.transport = if l.contains("/usb") {
                "usb"
            } else if l.contains("/ata") {
                "sata"
            } else {
                "scsi"
            }
            .into();
        }
        r.disk.push(k);
    }
}

// ---- buses ----

fn collect_pci(r: &mut Report, root: &str, ids_db: Option<&[u8]>) {
    for n in list_dir(root, "/sys/bus/pci/devices") {
        if r.pci.len() >= MAX_PCI {
            break;
        }
        let d = format!("/sys/bus/pci/devices/{n}");
        let mut x = Pci { vendor_id: hex_attr(root, &d, "vendor", 4), device_id: hex_attr(root, &d, "device", 4), slot_class: hex_attr(root, &d, "class", 6), ..Pci::default() };
        if x.vendor_id.is_empty() {
            continue;
        }
        x.driver = link_base(root, &d, "driver", 32);
        let (v, dn) = lookup_ids(ids_db, &x.vendor_id, Some(&x.device_id));
        x.vendor = v;
        x.device = dn;
        if let Some(t) = ids_db {
            let (c, s) = ids::pci_class(t, x.slot_class.as_bytes(), 48, 48);
            let (c, s) = (String::from_utf8_lossy(ids::name_text(&c)).into_owned(), String::from_utf8_lossy(ids::name_text(&s)).into_owned());
            x.class_name = if s.is_empty() { c } else { s };
        }
        r.pci.push(x);
    }
}

fn collect_usb(r: &mut Report, root: &str, ids_db: Option<&[u8]>) {
    for n in list_dir(root, "/sys/bus/usb/devices") {
        if r.usb.len() >= MAX_USB {
            break;
        }
        if n.contains(':') || n.starts_with("usb") {
            continue; // interfaces and root hubs
        }
        let d = format!("/sys/bus/usb/devices/{n}");
        let mut u = Usb { vendor_id: hex_attr(root, &d, "idVendor", 4), product_id: hex_attr(root, &d, "idProduct", 4), ..Usb::default() };
        if u.vendor_id.is_empty() {
            continue;
        }
        u.cls = hex_attr(root, &d, "bDeviceClass", 2);
        u.speed_mbps = atoi(&attr(root, &d, "speed", 16)) as i32;
        u.driver = link_base(root, &format!("{d}/{n}:1.0"), "driver", 32);
        let (v, p) = lookup_ids(ids_db, &u.vendor_id, Some(&u.product_id));
        u.vendor = v;
        u.product = p;
        r.usb.push(u);
    }
}

// ---- network, audio, Bluetooth, power, thermal ----

fn collect_net(r: &mut Report, root: &str) {
    for n in list_dir(root, "/sys/class/net") {
        if r.net.len() >= MAX_NET {
            break;
        }
        if n == "lo" {
            continue;
        }
        let d = format!("/sys/class/net/{n}");
        let has_dev = exists(root, &format!("{d}/device"));
        let wifi = exists(root, &format!("{d}/wireless")) || exists(root, &format!("{d}/phy80211"));
        let ty = attr_num(root, &d, "type", 1);
        let mut x = Net { kind: if wifi { "wifi" } else if !has_dev { "virtual" } else if ty == 1 { "ethernet" } else { "other" }.into(), ..Net::default() };
        x.driver = link_base(root, &format!("{d}/device"), "driver", 32);
        x.up = attr(root, &d, "operstate", 16) == "up";
        let speed = if x.up { attr_num(root, &d, "speed", -1) as i32 } else { -1 };
        x.speed_mbps = if speed <= 0 { -1 } else { speed };
        r.net.push(x);
    }
}

/// A card's name for people. ALSA's short name is often just its id ("vc4-hdmi-0"), and a USB card's is the
/// chip's generic string, so: a USB card by its IDs in usb.ids (/proc/asound/cardN/usbid) or, where that list
/// isn't installed, by the start of ALSA's long name ("C-Media Electronics Inc. USB Audio Device at
/// usb-1000…", the bus path dropped); the Raspberry Pi's two HDMI outputs by their port; anything else as ALSA
/// calls it.
fn audio_name(root: &str, usb: Option<&[u8]>, card: i64, driver: &str, short: &str, long: &str) -> String {
    let id = attr(root, &format!("/proc/asound/card{card}"), "usbid", 16);
    if id.len() == 9 && id.as_bytes()[4] == b':' {
        let (vn, dn) = lookup_ids(usb, &id[..4], Some(&id[5..]));
        if !dn.is_empty() {
            return fit(&format!("{} {}", fit(&vn, 41), fit(&dn, 55)), SR_NAME);
        }
        let long = long.trim_start_matches(' ');
        if let Some(at) = long.find(" at ") {
            if at > 0 {
                return long.bytes().take(at).take(SR_NAME - 1).map(|c| if (0x20..0x7f).contains(&c) { c as char } else { '?' }).collect();
            }
        }
    }
    if driver == "vc4-hdmi" {
        // sscanf("vc4-hdmi-%d"): blanks, an optional sign, at least one digit
        if let Some(rest) = short.strip_prefix("vc4-hdmi-") {
            let t = rest.trim_start();
            let digits = t.strip_prefix(['-', '+']).unwrap_or(t);
            if digits.starts_with(|c: char| c.is_ascii_digit()) {
                return format!("Raspberry Pi HDMI {} audio", atoi(t));
            }
        }
    }
    fit(short, SR_NAME)
}

fn collect_audio(r: &mut Report, root: &str, usb: Option<&[u8]>) {
    let Some(txt) = read_text(&format!("{root}/proc/asound/cards"), 16384) else { return };
    let lines: Vec<&str> = txt.split('\n').collect();
    // " 0 [PCH            ]: HDA-Intel - HDA Intel PCH": the text after " - "
    for (i, l) in lines.iter().enumerate() {
        if r.audio.len() >= MAX_AUDIO {
            break;
        }
        let (Some(_), Some(end)) = (l.find('['), l.find("]: ")) else { continue };
        let Some(dash) = l[end..].find(" - ").map(|k| k + end) else { continue };
        let name: String = l[dash + 3..].bytes().take(SR_NAME - 1).map(|c| if (0x20..0x7f).contains(&c) { c as char } else { '?' }).collect();
        let dl = (dash - end).wrapping_sub(3);
        let driver = if dl < 32 { l[end + 3..end + 3 + dl].to_string() } else { String::new() };
        let long = lines.get(i + 1).copied().unwrap_or("");
        r.audio.push(audio_name(root, usb, atoi(l), &driver, &name, long));
    }
}

fn collect_bt(r: &mut Report, root: &str) {
    for n in list_dir(root, "/sys/class/bluetooth") {
        if r.bt_driver.len() >= 4 {
            break;
        }
        if !n.starts_with("hci") || n.contains(':') {
            continue;
        }
        r.bt_driver.push(link_base(root, &format!("/sys/class/bluetooth/{n}/device"), "driver", 32));
    }
}

fn collect_power(r: &mut Report, root: &str) {
    for n in list_dir(root, "/sys/class/power_supply") {
        if r.power.len() >= MAX_POWER {
            break;
        }
        let d = format!("/sys/class/power_supply/{n}");
        if attr(root, &d, "scope", 24) == "Device" {
            continue; // a mouse's or headset's battery
        }
        let ty = attr(root, &d, "type", 24);
        let mut x = Power {
            kind: match ty.as_str() {
                "Battery" => "battery",
                "Mains" => "ac",
                t if t.starts_with("USB") => "usb",
                "UPS" => "ups",
                _ => "other",
            }
            .into(),
            vendor: attr(root, &d, "manufacturer", SR_NAME),
            model: attr(root, &d, "model_name", SR_NAME),
            technology: attr(root, &d, "technology", 16),
            status: attr(root, &d, "status", 16),
            capacity: attr_num(root, &d, "capacity", -1) as i32,
            online: attr_num(root, &d, "online", -1) as i32,
            ..Power::default()
        };
        let (mut full, mut design) = (attr_num(root, &d, "energy_full", -1), attr_num(root, &d, "energy_full_design", -1));
        if full < 0 || design <= 0 {
            full = attr_num(root, &d, "charge_full", -1);
            design = attr_num(root, &d, "charge_full_design", -1);
        }
        x.health = if full >= 0 && design > 0 { (full * 100 / design) as i32 } else { -1 };
        r.power.push(x);
    }
}

fn collect_thermal(r: &mut Report, root: &str) {
    for n in list_dir(root, "/sys/class/thermal") {
        if r.thermal.len() >= MAX_THERMAL {
            break;
        }
        if !n.starts_with("thermal_zone") {
            continue;
        }
        let d = format!("/sys/class/thermal/{n}");
        r.thermal.push(Thermal { kind: attr(root, &d, "type", 32), milli_c: attr_num(root, &d, "temp", UNKNOWN_TEMP as i64) as i32, fans: 0 });
    }
    for n in list_dir(root, "/sys/class/hwmon") {
        if r.thermal.len() >= MAX_THERMAL {
            break;
        }
        let d = format!("/sys/class/hwmon/{n}");
        let kind = attr(root, &d, "name", 32);
        if kind.is_empty() {
            continue;
        }
        let fans = list_dir(root, &d).iter().filter(|f| f.starts_with("fan") && f.contains("_input")).count() as i32;
        r.thermal.push(Thermal { kind, milli_c: attr_num(root, &d, "temp1_input", UNKNOWN_TEMP as i64) as i32, fans });
    }
}

// ---- Raspberry Pi ----

/// The UTC date of a Unix time as YYYY-MM-DD (gmtime).
fn utc_date(t: i64) -> String {
    let days = t.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}")
}

fn collect_pi(r: &mut Report, root: &str, cpuinfo: &str) {
    let mut compat = read_file(&format!("{root}/proc/device-tree/compatible"), 511).unwrap_or_default();
    for c in compat.iter_mut() {
        if *c == 0 {
            *c = b' '; // NUL-separated list
        }
    }
    let compat = String::from_utf8_lossy(&compat).into_owned();
    r.pi.is_pi = compat.contains("raspberrypi");
    r.pi.throttled = -1;
    r.pi.uv_alarm = -1;
    if !r.pi.is_pi {
        return;
    }
    r.pi.model = r.machine.model.clone();
    r.pi.revision = field(cpuinfo, "Revision", 16);
    // the bootloader that started this boot: the device tree the firmware hands over, world-readable, where
    // rpi-eeprom-update itself looks first. version: a git hash; build-timestamp: big-endian seconds.
    let ver = read_attr(&format!("{root}/proc/device-tree/chosen/bootloader/version"), 64);
    if ver.len() >= 7 && ver.len() < 44 && ver.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        r.pi.bootloader_version = ver;
    }
    if let Some(ts) = read_file(&format!("{root}/proc/device-tree/chosen/bootloader/build-timestamp"), 8) {
        if ts.len() == 4 {
            let t = u32::from_be_bytes([ts[0], ts[1], ts[2], ts[3]]) as i64;
            if t >= 1_000_000_000 {
                r.pi.bootloader_date = utc_date(t);
            }
        }
    }
    // what the firmware says about power and throttling: rpi_volt's under-voltage alarm (the only one the LordOS
    // rpi-6.18.y kernel has) and get_throttled where an older kernel has it
    let fw = rpifw::read(root);
    r.pi.throttled = fw.throttled;
    r.pi.uv_alarm = fw.uv_alarm;
}

// ---- the boot medium ----
//
// What the system started from, as one word: the device under / (installed) or under live-boot's
// /run/live/medium (live), found in /proc/self/mountinfo by its major:minor and classified by its sysfs
// path, all world-readable. Device mapper and RAID layers are followed through "slaves". Only the class
// leaves this function (and a USB medium's link speed): never the device's name, its model, a serial or a
// path.

const BOOT_MAX_DEPTH: i32 = 4;

/// The sysfs path ("/sys/devices/…") a block link points at; "" when it isn't one.
fn blk_sysfs(root: &str, link: &str) -> String {
    let Ok(l) = std::fs::read_link(format!("{root}{link}")) else { return String::new() };
    let l = l.to_string_lossy();
    let l = &l[..l.len().min(1023)];
    let Some(i) = l.find("devices/") else { return String::new() };
    let d = &l[i..];
    if d.contains("/../") || d.contains("/./") {
        return String::new();
    }
    fit(&format!("/sys/{d}"), 1024)
}

fn base_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A block device name as the kernel writes them ([A-Za-z0-9_-], at most 31 characters).
fn blk_name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 31 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// attr() for the long sysfs paths of the boot medium: nothing ("") when it does not fit.
fn attr_long(root: &str, dir: &str, name: &str, size: usize) -> String {
    let p = format!("{root}{dir}/{name}");
    if p.len() >= 2400 { String::new() } else { read_attr(&p, size) }
}

/// A USB device's link speed in Mbps: the deepest "<bus>-<port>[.<port>…]" component after "usbN" in a
/// disk's sysfs path holds "speed" (1.5, 12, 480, 5000…); -1 when it can't be read.
fn usb_link_mbps(root: &str, dev: &str) -> i32 {
    if dev.len() >= 1024 {
        return -1;
    }
    let mut best = String::new();
    let mut after_usb = false;
    let b = dev.as_bytes();
    for i in 1..=b.len() {
        if i < b.len() && b[i] != b'/' {
            continue;
        }
        let mut s = i;
        while s > 0 && b[s - 1] != b'/' {
            s -= 1;
        }
        let c = &dev[s..i];
        if c.len() > 3 && c.starts_with("usb") && c.as_bytes()[3].is_ascii_digit() {
            after_usb = true;
            continue;
        }
        if !after_usb || c.is_empty() || !c.as_bytes()[0].is_ascii_digit() {
            continue;
        }
        let dashes = c.bytes().filter(|&x| x == b'-').count();
        let ok = c.bytes().all(|x| x == b'-' || x.is_ascii_digit() || x == b'.');
        if ok && dashes == 1 {
            best = dev[..i].to_string();
        }
    }
    if best.is_empty() {
        return -1;
    }
    let v = attr_long(root, &best, "speed", 32);
    if v.is_empty() {
        return -1;
    }
    // strtod: the number at the start
    let end = v.bytes().position(|c| !(c.is_ascii_digit() || c == b'.' || c == b'-' || c == b'+' || c == b'e' || c == b'E')).unwrap_or(v.len());
    let Some(mbps) = (0..=end).rev().find_map(|k| v[..k].parse::<f64>().ok()) else { return -1 };
    if mbps <= 0.0 || mbps > 1_000_000.0 {
        return -1;
    }
    if mbps < 1.0 { 1 } else { mbps as i32 }
}

/// Classify the block device at `<root><link>` (a /sys/dev/block or /sys/class/block symlink).
fn boot_classify(s: &mut Software, root: &str, link: &str, depth: i32) {
    let dev = blk_sysfs(root, link);
    if dev.is_empty() || depth > BOOT_MAX_DEPTH {
        return;
    }
    // a partition's disk is its parent folder
    let mut disk = dev.clone();
    if std::fs::symlink_metadata(format!("{root}{dev}/partition")).is_ok() {
        if let Some(sl) = disk.rfind('/') {
            if sl != 0 {
                disk.truncate(sl);
            }
        }
    }
    let name = base_of(&disk).to_string();
    if !blk_name_ok(&name) {
        return;
    }
    // device mapper (LUKS, LVM) and software RAID: the first layer below
    if name.starts_with("dm-") || name.starts_with("md") {
        let b = list_dir(root, &format!("{dev}/slaves"));
        if let Some(first) = b.first().filter(|f| blk_name_ok(f)) {
            boot_classify(s, root, &format!("/sys/class/block/{first}"), depth + 1);
        }
        return;
    }
    let cls: &str = if name.starts_with("loop") || name.starts_with("zram") || name.starts_with("ram") {
        ""
    } else if name.starts_with("nbd") || disk.contains("/session") {
        "network" // NBD, iSCSI
    } else if name.starts_with("sr") {
        "optical"
    } else if disk.contains("/usb") {
        "usb"
    } else if name.starts_with("mmcblk") {
        let d = format!("{disk}/device");
        if d.len() >= 1100 {
            return;
        }
        match attr_long(root, &d, "type", 16).as_str() {
            "SD" => "sd",
            "MMC" => "emmc",
            _ => "",
        }
    } else {
        "internal"
    };
    s.boot_medium = fit(cls, 12);
    if cls == "usb" {
        s.boot_link_mbps = usb_link_mbps(root, &disk);
    }
}

/// Whether the kernel command line holds the word `w` (or `w=…` when `w` ends in '='). Nothing of it is
/// kept.
pub fn cmdline_has(cmdline: &str, w: &str) -> bool {
    let n = w.len();
    let b = cmdline.as_bytes();
    let mut from = 0;
    while let Some(i) = cmdline[from..].find(w) {
        let p = from + i;
        let start = p == 0 || b[p - 1] == b' ';
        let e = b.get(p + n).copied().unwrap_or(0);
        if start && (w.ends_with('=') || e == 0 || e == b' ' || e == b'\n') {
            return true;
        }
        from = p + n;
    }
    false
}

fn collect_boot(s: &mut Software, root: &str) {
    s.boot_medium.clear();
    s.boot_link_mbps = -1;
    let cmd = read_text(&format!("{root}/proc/cmdline"), 4096);
    if let (true, Some(c)) = (s.live, cmd.as_deref()) {
        s.boot_toram = cmdline_has(c, "toram") || cmdline_has(c, "toram=");
    }
    let fetched = s.live && cmd.as_deref().is_some_and(|c| cmdline_has(c, "fetch=") || cmdline_has(c, "netboot="));
    let want = if s.live { "/run/live/medium" } else { "/" };
    let mi = read_text(&format!("{root}/proc/self/mountinfo"), 1 << 20).unwrap_or_default();
    let (mut ma, mut mn, mut found) = (0u32, 0u32, false);
    let (mut fstype, mut source) = (String::new(), String::new());
    // "<id> <parent> <major>:<minor> <root> <mount point> <options> [optional…] - <fstype> <source> <super>";
    // the last line for a mount point is the one on top
    for line in mi.split('\n') {
        let mut w = line.split_ascii_whitespace();
        let (Some(_), Some(_), Some(dev), Some(_), Some(point)) = (w.next(), w.next(), w.next(), w.next(), w.next()) else { continue };
        if point != want || fit(point, 256) != point {
            continue;
        }
        let Some((a, b)) = dev.split_once(':') else { continue };
        let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else { continue };
        let Some(dash) = line.find(" - ") else { continue };
        let mut t = line[dash + 3..].split_ascii_whitespace();
        let Some(ft) = t.next() else { continue };
        ma = a;
        mn = b;
        found = true;
        fstype = fit(ft, 32);
        source = t.next().map(|x| fit(x, 128)).unwrap_or_default();
    }
    if found && matches!(fstype.as_str(), "nfs" | "nfs4" | "cifs" | "smb3") {
        s.boot_medium = "network".into();
    } else if found && ma != 0 {
        boot_classify(s, root, &format!("/sys/dev/block/{ma}:{mn}"), 0);
    } else if found && source.starts_with("/dev/") && blk_name_ok(base_of(&source)) {
        // btrfs and others show an anonymous 0:N device: the source names the block device
        boot_classify(s, root, &format!("/sys/class/block/{}", base_of(&source)), 0);
    }
    if s.boot_medium.is_empty() && fetched {
        s.boot_medium = "network".into();
    }
}

// ---- software ----

fn collect_software(r: &mut Report, root: &str) {
    let s = &mut r.sw;
    let txt = read_text(&format!("{root}/etc/os-release"), 8192).or_else(|| read_text(&format!("{root}/usr/lib/os-release"), 8192));
    if let Some(t) = txt {
        s.os = osr_field(&t, "PRETTY_NAME", SR_NAME);
        s.os_id = osr_field(&t, "ID", 24);
        s.os_version = osr_field(&t, "VERSION_ID", 24);
    }
    s.kernel = read_attr(&format!("{root}/proc/sys/kernel/osrelease"), 64);
    s.lsm = read_attr(&format!("{root}/sys/kernel/security/lsm"), SR_NAME);
    s.bpf_lsm = s.lsm.contains("bpf");
    s.cgroup2 = exists(root, "/sys/fs/cgroup/cgroup.controllers");
    s.landlock_abi = -1;
    if root.is_empty() {
        // LANDLOCK_CREATE_RULESET_VERSION
        // SAFETY: the syscall takes no pointers here.
        let abi = unsafe { libc::syscall(libc::SYS_landlock_create_ruleset, std::ptr::null::<u8>(), 0usize, 1u32) };
        s.landlock_abi = if abi > 0 { abi as i32 } else { 0 };
    }
    // the kernel options LordOS's isolation, firewall and memory handling depend on
    const OPTS: [&str; 14] = [
        "CONFIG_SECURITY_LANDLOCK", "CONFIG_SECCOMP_FILTER", "CONFIG_USER_NS", "CONFIG_BPF_SYSCALL", "CONFIG_BPF_JIT", "CONFIG_CGROUP_BPF",
        "CONFIG_BPF_LSM", "CONFIG_INET_DIAG_DESTROY", "CONFIG_INET_UDP_DIAG", "CONFIG_MEMCG", "CONFIG_PSI", "CONFIG_ZRAM", "CONFIG_DRM", "CONFIG_UHID",
    ];
    let txt = if !s.kernel.is_empty() && !s.kernel.contains('/') { read_text(&format!("{root}/boot/config-{}", s.kernel), 1 << 20) } else { None };
    for o in OPTS.iter().take(MAX_KCONF) {
        let mut value = String::new();
        if let Some(t) = &txt {
            // absent or "# … is not set": n
            value = match t.find(&format!("\n{o}=")) {
                Some(i) => match t.as_bytes().get(i + o.len() + 2) {
                    Some(&c @ (b'y' | b'm')) => (c as char).to_string(),
                    _ => "?".into(),
                },
                None => "n".into(),
            };
        }
        s.kconf.push((fit(o, 40), value));
    }
    s.sessiond = exists(root, "/run/lordos/cgroup.sock");
    s.netperm = exists(root, "/run/lordos/netperm.sock");
    s.fand = exists(root, "/run/lordos/fand.sock");
    s.netd = exists(root, "/run/lordos/netd.sock");
    // the live session from the ISO (live-boot's marker, as LordBench reads it)
    s.live = exists(root, "/run/live");
    collect_boot(s, root);
}

// ---- all of it ----

fn today() -> String {
    // SAFETY: localtime_r and strftime fill local buffers.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        let mut b = [0 as libc::c_char; 16];
        let n = libc::strftime(b.as_mut_ptr(), b.len(), c"%Y-%m-%d".as_ptr(), &tm);
        b[..n].iter().map(|&c| c as u8 as char).collect()
    }
}

/// Fills a report from the tree at `root` ("" = this machine). `pci_ids` / `usb_ids` are the name
/// lists (`None`: the Debian paths under root). Always succeeds: what can't be read stays empty or
/// unknown.
pub fn collect(root: &str, pci_ids: Option<&str>, usb_ids: Option<&str>) -> Report {
    let mut r = Report { schema: 1, date: today(), ..Report::default() };
    let cpuinfo = read_text(&format!("{root}/proc/cpuinfo"), 1 << 20).unwrap_or_default();
    let ci = cpuinfo.as_str();
    let pci = ids::load(pci_ids.map_or_else(|| format!("{root}/usr/share/misc/pci.ids"), str::to_string).as_bytes());
    let usb = ids::load(usb_ids.map_or_else(|| format!("{root}/usr/share/misc/usb.ids"), str::to_string).as_bytes());
    collect_machine(&mut r, root, ci);
    collect_cpu(&mut r, root, ci);
    collect_memory(&mut r, root);
    collect_gpus(&mut r, root, pci.as_deref());
    collect_displays(&mut r, root);
    collect_disks(&mut r, root);
    collect_pci(&mut r, root, pci.as_deref());
    collect_usb(&mut r, root, usb.as_deref());
    collect_net(&mut r, root);
    collect_audio(&mut r, root, usb.as_deref());
    collect_bt(&mut r, root);
    collect_power(&mut r, root);
    collect_thermal(&mut r, root);
    collect_pi(&mut r, root, ci);
    collect_software(&mut r, root);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_utc() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_000_000_000), "2001-09-09");
        assert_eq!(utc_date(1_790_000_000), "2026-09-21");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn words_in_the_kernel_command_line() {
        assert!(cmdline_has("quiet rd.live.image toram", "toram"));
        assert!(cmdline_has("toram=1 quiet", "toram="));
        assert!(cmdline_has("a fetch=http://x", "fetch="));
        assert!(!cmdline_has("quiet mytoram", "toram"));
        assert!(!cmdline_has("quiet toramx", "toram"));
        assert!(!cmdline_has("", "toram"));
        assert!(cmdline_has("toram\n", "toram"));
    }

    #[test]
    fn cache_sizes_and_chassis() {
        assert_eq!((cache_kb("48K"), cache_kb("1024 K"), cache_kb("32M"), cache_kb("x"), cache_kb("0K")), (48, 1024, 32768, 0, 0));
        assert_eq!((chassis_name(3), chassis_name(10), chassis_name(1), chassis_name(2), chassis_name(35)), ("desktop", "laptop", "other", "", "mini-pc"));
    }

    #[test]
    fn a_walk_up_the_usb_tree_finds_the_speed_holder() {
        let d = std::env::temp_dir().join(format!("axia-sysreport-usb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let dev = "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1.4/2-1.4:1.0/host6/target6:0:0/6:0:0:0/block/sda";
        let hub = "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1.4";
        std::fs::create_dir_all(format!("{}{hub}", d.display())).unwrap();
        std::fs::write(format!("{}{hub}/speed", d.display()), "5000\n").unwrap();
        assert_eq!(usb_link_mbps(d.to_str().unwrap(), dev), 5000);
        assert_eq!(usb_link_mbps(d.to_str().unwrap(), "/sys/devices/pci0000:00/0000:00:1f.2/ata1/host0/block/sda"), -1);
    }
}
