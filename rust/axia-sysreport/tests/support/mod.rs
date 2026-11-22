//! Fake trees for System Information's tests: an AMD desktop, an Intel laptop, an Nvidia desktop, a
//! Raspberry Pi 5 (with the LordOS kernel) and a VM. Each is full of things a report must never carry
//! (serial numbers, UUIDs, MAC addresses, a host name, a user name, a monitor's serial, a MAC inside an
//! interface name). `put` and friends build what the collector reads.
#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};

pub fn temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("axia-sysreport-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

pub fn putn(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

pub fn put(root: &Path, rel: &str, text: &str) {
    putn(root, rel, text.as_bytes());
}

pub fn mkdirs_in(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel)).unwrap();
}

pub fn lnk(root: &Path, rel: &str, target: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let _ = fs::remove_file(&p);
    std::os::unix::fs::symlink(target, &p).unwrap();
}

pub fn driver(root: &Path, dir: &str, name: &str) {
    lnk(root, &format!("{dir}/driver"), &format!("../../../bus/drivers/{name}"));
}

/// A 128-byte EDID 1.4 with a name, a serial descriptor and a native mode.
#[allow(clippy::too_many_arguments)]
pub fn edid(pnp: &str, product: u16, name: &str, serial: &str, w: u32, h: u32, wcm: u8, hcm: u8, year: u32) -> [u8; 128] {
    let mut e = [0u8; 128];
    e[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
    let p = pnp.as_bytes();
    let m = (((p[0] - b'A' + 1) as u16) << 10) | (((p[1] - b'A' + 1) as u16) << 5) | (p[2] - b'A' + 1) as u16;
    e[8] = (m >> 8) as u8;
    e[9] = m as u8;
    e[10] = product as u8;
    e[11] = (product >> 8) as u8;
    e[12..16].copy_from_slice(&[0xef, 0xbe, 0xad, 0xde]); // serial 0xdeadbeef
    e[16] = 10;
    e[17] = (year - 1990) as u8;
    e[18] = 1;
    e[19] = 4;
    e[20] = 0x80;
    e[21] = wcm;
    e[22] = hcm;
    let t = &mut e[54..72]; // detailed timing
    t[0] = 0x56;
    t[1] = 0x5e;
    t[2] = w as u8;
    t[4] = ((w >> 8) << 4) as u8;
    t[5] = h as u8;
    t[7] = ((h >> 8) << 4) as u8;
    for (k, (txt, tag)) in [(name, 0xfcu8), (serial, 0xff)].iter().enumerate() {
        let d = &mut e[72 + 18 * k..72 + 18 * k + 18];
        d[3] = *tag;
        let b = txt.as_bytes();
        for i in 0..13 {
            d[5 + i] = if i < b.len() { b[i] } else if i == b.len() { 0x0a } else { 0x20 };
        }
    }
    e[108 + 3] = 0x10; // dummy descriptor
    let sum: u32 = e[..127].iter().map(|&b| b as u32).sum();
    e[127] = (256 - sum % 256) as u8;
    e
}

pub const PCI_IDS: &str = "# pci.ids (a few lines)\n\
1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\
\t67df  Ellesmere [Radeon RX 470/480/570/570X/580/580X/590]\n\
\t\t1002 0b31  Radeon RX 580\n\
\tab38  Navi 10 HDMI Audio\n\
10de  NVIDIA Corporation\n\
\t2684  AD102 [GeForce RTX 4090]\n\
144d  Samsung Electronics Co Ltd\n\
\ta808  NVMe SSD Controller SM981/PM981/PM983\n\
1af4  Red Hat, Inc.\n\
\t1050  Virtio 1.0 GPU\n\
8086  Intel Corporation\n\
\t9a49  TigerLake-LP GT2 [Iris Xe Graphics]\n\
\t2725  Wi-Fi 6E(802.11ax) AX210/AX1675* 2x2 [Typhoon Peak]\n\
C 02  Network controller\n\
\t80  Network controller\n\
C 03  Display controller\n\
\t00  VGA compatible controller\n\
\t80  Display controller\n\
C 01  Mass storage controller\n\
\t08  Non-Volatile memory controller\n";

pub const USB_IDS: &str = "046d  Logitech, Inc.\n\
\tc52b  Unifying Receiver\n\
0d8c  C-Media Electronics, Inc.\n\
\t0014  Audio Adapter (Unitek Y-247A)\n\
0bda  Realtek Semiconductor Corp.\n\
\t8153  RTL8153 Gigabit Ethernet Adapter\n\
C 00  (Defined at Interface level)\n";

/// The things no report may carry.
pub const SECRETS: [&str; 10] = ["SECRET", "secret", "deadbeef", "DEADBEEF", "de:ad:be:ef", "4c4c4544", "enxdeadbeef", "SECRETMON", "secretuser", "/home/"];

pub fn common_secrets(r: &Path) {
    put(r, "proc/sys/kernel/hostname", "secret-host\n");
    put(r, "sys/class/dmi/id/product_serial", "SECRET-SERIAL-123\n");
    put(r, "sys/class/dmi/id/product_uuid", "4c4c4544-0042-3510-8051-b7c04f4d4d32\n");
    put(r, "sys/class/dmi/id/board_serial", "SECRET-BOARD\n");
    put(r, "sys/class/dmi/id/chassis_serial", "SECRET-CHASSIS\n");
    put(r, "etc/hostname", "secret-host\n");
    put(r, "etc/passwd", "secretuser:x:1000:1000:Secret User,,,:/home/secretuser:/bin/bash\n");
}

pub fn kernel(r: &Path, rel: &str) {
    put(r, "proc/sys/kernel/osrelease", rel);
    put(r, &format!("boot/config-{rel}"), "#\n# Linux kernel config\n#\nCONFIG_SECURITY_LANDLOCK=y\nCONFIG_BPF_SYSCALL=y\n# CONFIG_BPF_LSM is not set\nCONFIG_ZRAM=m\nCONFIG_USER_NS=y\n");
    put(r, "sys/kernel/security/lsm", "lockdown,capability,landlock,yama,apparmor,bpf");
    put(r, "sys/fs/cgroup/cgroup.controllers", "cpu memory io pids");
    put(r, "etc/os-release", "PRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nID=debian\nVERSION_ID=\"13\"\n");
    put(r, "usr/share/misc/pci.ids", PCI_IDS);
    put(r, "usr/share/misc/usb.ids", USB_IDS);
}

pub fn cpus(r: &Path, threads: usize, per_core: usize) {
    for i in 0..threads {
        put(r, &format!("sys/devices/system/cpu/cpu{i}/topology/core_id"), &format!("{}\n", i / per_core));
        put(r, &format!("sys/devices/system/cpu/cpu{i}/topology/physical_package_id"), "0\n");
    }
    let c = "sys/devices/system/cpu/cpu0";
    put(r, &format!("{c}/cpufreq/cpuinfo_min_freq"), "800000\n");
    put(r, &format!("{c}/cpufreq/cpuinfo_max_freq"), "4200000\n");
    put(r, &format!("{c}/cpufreq/scaling_governor"), "schedutil\n");
    for (i, (lvl, ty, size)) in [("1", "Data", "32K"), ("1", "Instruction", "32K"), ("2", "Unified", "512K"), ("3", "Unified", "32768K")].iter().enumerate() {
        put(r, &format!("{c}/cache/index{i}/level"), lvl);
        put(r, &format!("{c}/cache/index{i}/type"), ty);
        put(r, &format!("{c}/cache/index{i}/size"), size);
    }
}

/// A PCI device under /sys/bus/pci/devices.
pub fn pcidev(r: &Path, slot: &str, ven: &str, dev: &str, cls: &str, drv: Option<&str>) {
    let d = format!("sys/bus/pci/devices/{slot}");
    put(r, &format!("{d}/vendor"), ven);
    put(r, &format!("{d}/device"), dev);
    put(r, &format!("{d}/class"), cls);
    if let Some(drv) = drv {
        driver(r, &d, drv);
    }
}

/// A DRM card: device ids (or none for a platform GPU), driver, a render node and connectors.
pub fn card(r: &Path, n: u32, ven: Option<(&str, &str)>, drv: &str, render: bool, boot: bool) {
    let d = format!("sys/class/drm/card{n}/device");
    if let Some((v, dev)) = ven {
        put(r, &format!("{d}/vendor"), v);
        put(r, &format!("{d}/device"), dev);
    }
    if boot {
        put(r, &format!("{d}/boot_vga"), "1\n");
    }
    driver(r, &d, drv);
    if render {
        put(r, &format!("{d}/drm/renderD{}/dev", 128 + n), "226:128\n");
    }
    put(r, &format!("{d}/serial"), "SECRET-GPU\n");
}

pub fn conn(r: &Path, n: u32, name: &str, connected: bool, ed: Option<&[u8; 128]>, modes: Option<&str>) {
    let d = format!("sys/class/drm/card{n}-{name}");
    put(r, &format!("{d}/status"), if connected { "connected\n" } else { "disconnected\n" });
    put(r, &format!("{d}/enabled"), if connected { "enabled\n" } else { "disabled\n" });
    if let Some(e) = ed {
        putn(r, &format!("{d}/edid"), e);
    }
    if let Some(m) = modes {
        put(r, &format!("{d}/modes"), m);
    }
}

pub fn netif(r: &Path, name: &str, drv: Option<&str>, wifi: bool, speed: Option<&str>) {
    let d = format!("sys/class/net/{name}");
    put(r, &format!("{d}/address"), "de:ad:be:ef:00:01\n");
    put(r, &format!("{d}/type"), "1\n");
    put(r, &format!("{d}/operstate"), if speed.is_some() { "up\n" } else { "down\n" });
    if let Some(s) = speed {
        put(r, &format!("{d}/speed"), s);
    }
    if let Some(drv) = drv {
        driver(r, &format!("{d}/device"), drv);
    }
    if wifi {
        put(r, &format!("{d}/wireless/x"), "");
    }
}

pub fn disk(r: &Path, name: &str, model: &str, sectors: &str, rot: bool, link: Option<&str>) {
    let d = format!("sys/block/{name}");
    if let Some(link) = link {
        // sdX is a link into the device tree: its path says the bus
        put(r, &format!("sys/devices/{link}/block/{name}/size"), sectors);
        lnk(r, &d, &format!("../devices/{link}/block/{name}"));
    } else {
        put(r, &format!("{d}/size"), sectors);
    }
    put(r, &format!("{d}/queue/rotational"), if rot { "1\n" } else { "0\n" });
    put(r, &format!("{d}/removable"), "0\n");
    put(r, &format!("{d}/device/model"), model);
    put(r, &format!("{d}/device/serial"), "SECRET-DISK\n");
    put(r, &format!("{d}/device/wwid"), "eui.SECRET0000\n");
}

pub const KINDS: [&str; 5] = ["amd", "intel", "nvidia", "rpi5", "vm"];

pub fn fixture(r: &Path, kind: &str) {
    common_secrets(r);
    kernel(r, "6.12.43+deb13-amd64");
    put(r, "proc/meminfo", "MemTotal:       32768000 kB\nMemFree:  1 kB\nSwapTotal:       2097148 kB\n");
    put(r, "proc/asound/cards", " 0 [Generic        ]: HDA-Intel - HD-Audio Generic\n                      HD-Audio Generic at 0xfcd00000 irq 89\n");
    put(r, "run/lordos/netperm.sock", "");
    match kind {
        "amd" => {
            put(r, "sys/class/dmi/id/sys_vendor", "Micro-Star International Co., Ltd.\n");
            put(r, "sys/class/dmi/id/product_name", "MS-7C02\n");
            put(r, "sys/class/dmi/id/product_version", "To be filled by O.E.M.\n");
            put(r, "sys/class/dmi/id/board_vendor", "Micro-Star International Co., Ltd.\n");
            put(r, "sys/class/dmi/id/board_name", "B450 TOMAHAWK MAX (MS-7C02)\n");
            put(r, "sys/class/dmi/id/board_version", "1.0\n");
            put(r, "sys/class/dmi/id/chassis_type", "3\n");
            put(r, "sys/class/dmi/id/bios_vendor", "American Megatrends Inc.\n");
            put(r, "sys/class/dmi/id/bios_version", "3.J0\n");
            put(r, "sys/class/dmi/id/bios_date", "11/23/2023\n");
            putn(r, "sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c", &[6, 0, 0, 0, 1]);
            put(r, "proc/cpuinfo", "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 7 5800X 8-Core Processor\nmicrocode\t: 0xa201205\nflags\t\t: fpu sse4_2 avx avx2 fma aes sha_ni\n");
            cpus(r, 16, 2);
            card(r, 0, Some(("0x1002", "0x67df")), "amdgpu", true, true);
            let ed = edid("DEL", 0xa0c4, "DELL U2720Q", "SECRETMON01", 3840, 2160, 60, 34, 2021);
            conn(r, 0, "DP-1", true, Some(&ed), Some("3840x2160\n2560x1440\n"));
            conn(r, 0, "HDMI-A-1", false, None, None);
            pcidev(r, "0000:0a:00.0", "0x1002", "0x67df", "0x030000", Some("amdgpu"));
            pcidev(r, "0000:0a:00.1", "0x1002", "0xab38", "0x040300", Some("snd_hda_intel"));
            pcidev(r, "0000:01:00.0", "0x144d", "0xa808", "0x010802", Some("nvme"));
            disk(r, "nvme0n1", "Samsung SSD 970 EVO Plus 1TB\n", "1953525168\n", false, None);
            put(r, "sys/block/nvme0n1/device/device/current_link_speed", "8.0 GT/s PCIe\n");
            put(r, "sys/block/nvme0n1/device/device/current_link_width", "4\n");
            disk(r, "sda", "WDC WD40EFRX-68N\n", "7814037168\n", true, Some("pci0000:00/0000:00:01.3/ata1/host0/target0:0:0/0:0:0:0"));
            disk(r, "loop0", "", "1000\n", false, None);
            netif(r, "enp5s0", Some("r8169"), false, Some("1000\n"));
            netif(r, "docker0", None, false, None);
            put(r, "sys/bus/usb/devices/usb1/idVendor", "1d6b\n");
            put(r, "sys/bus/usb/devices/1-2/idVendor", "046d\n");
            put(r, "sys/bus/usb/devices/1-2/idProduct", "c52b\n");
            put(r, "sys/bus/usb/devices/1-2/speed", "12\n");
            put(r, "sys/bus/usb/devices/1-2/bDeviceClass", "00\n");
            put(r, "sys/bus/usb/devices/1-2/serial", "SECRET-USB\n");
            put(r, "sys/bus/usb/devices/1-2/product", "secretuser's receiver\n");
            driver(r, "sys/bus/usb/devices/1-2/1-2:1.0", "usbhid");
            put(r, "sys/class/hwmon/hwmon0/name", "k10temp");
            put(r, "sys/class/hwmon/hwmon0/temp1_input", "41250");
            put(r, "sys/class/hwmon/hwmon1/name", "nct6797");
            put(r, "sys/class/hwmon/hwmon1/fan1_input", "900");
            put(r, "sys/class/hwmon/hwmon1/fan2_input", "0");
        }
        "intel" => {
            put(r, "sys/class/dmi/id/sys_vendor", "LENOVO\n");
            put(r, "sys/class/dmi/id/product_name", "20XW0026US\n");
            put(r, "sys/class/dmi/id/product_version", "ThinkPad X1 Carbon Gen 9\n");
            put(r, "sys/class/dmi/id/board_version", "Not Specified\n");
            put(r, "sys/class/dmi/id/chassis_type", "10\n");
            put(r, "sys/class/dmi/id/bios_vendor", "LENOVO\n");
            put(r, "sys/firmware/efi/x", "");
            put(r, "proc/cpuinfo", "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: 11th Gen Intel(R) Core(TM) i7-1165G7 @ 2.80GHz\nflags\t\t: fpu sse4_2 avx avx2 avx512f fma aes vaes\n");
            cpus(r, 8, 2);
            card(r, 0, Some(("0x8086", "0x9a49")), "i915", true, true);
            let ed = edid("AUO", 0x2336, "", "SECRETMON02", 1920, 1200, 30, 19, 2020);
            conn(r, 0, "eDP-1", true, Some(&ed), Some("1920x1200\n"));
            pcidev(r, "0000:00:02.0", "0x8086", "0x9a49", "0x030000", Some("i915"));
            pcidev(r, "0000:00:14.3", "0x8086", "0x2725", "0x028000", Some("iwlwifi"));
            netif(r, "wlp0s20f3", Some("iwlwifi"), true, Some("866\n"));
            netif(r, "enxdeadbeef0001", Some("r8152"), false, Some("1000\n")); // the name carries the MAC
            put(r, "sys/class/power_supply/BAT0/type", "Battery\n");
            put(r, "sys/class/power_supply/BAT0/manufacturer", "SMP\n");
            put(r, "sys/class/power_supply/BAT0/model_name", "5B10W13975\n");
            put(r, "sys/class/power_supply/BAT0/technology", "Li-poly\n");
            put(r, "sys/class/power_supply/BAT0/capacity", "76\n");
            put(r, "sys/class/power_supply/BAT0/status", "Discharging\n");
            put(r, "sys/class/power_supply/BAT0/energy_full", "45000000\n");
            put(r, "sys/class/power_supply/BAT0/energy_full_design", "57000000\n");
            put(r, "sys/class/power_supply/BAT0/serial_number", "SECRET-BAT\n");
            put(r, "sys/class/power_supply/AC/type", "Mains\n");
            put(r, "sys/class/power_supply/AC/online", "0\n");
            put(r, "sys/class/power_supply/hidpp_battery_0/type", "Battery\n");
            put(r, "sys/class/power_supply/hidpp_battery_0/scope", "Device\n");
            put(r, "sys/class/bluetooth/hci0/address", "de:ad:be:ef:00:02\n");
            driver(r, "sys/class/bluetooth/hci0/device", "btusb");
            put(r, "sys/class/bluetooth/hci0:256/x", "");
            put(r, "sys/class/thermal/thermal_zone0/type", "x86_pkg_temp");
            put(r, "sys/class/thermal/thermal_zone0/temp", "52000");
        }
        "nvidia" => {
            put(r, "sys/class/dmi/id/sys_vendor", "ASUS\n");
            put(r, "sys/class/dmi/id/chassis_type", "7\n");
            put(r, "proc/cpuinfo", "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Core(TM) i9-10900\nflags\t\t: avx2\n");
            cpus(r, 20, 2);
            card(r, 0, Some(("0x10de", "0x2684")), "nouveau", true, true);
            conn(r, 0, "DP-1", false, None, None);
            pcidev(r, "0000:01:00.0", "0x10de", "0x2684", "0x030000", Some("nouveau"));
        }
        "rpi5" => {
            putn(r, "proc/device-tree/model", b"Raspberry Pi 5 Model B Rev 1.0\0");
            putn(r, "proc/device-tree/compatible", b"raspberrypi,5-model-b\0brcm,bcm2712\0");
            putn(r, "proc/device-tree/serial-number", b"10000000deadbeef\0");
            putn(r, "proc/device-tree/chosen/bootloader/version", b"1accd6659983613cd104973ce2432da14f1a7682\0");
            putn(r, "proc/device-tree/chosen/bootloader/build-timestamp", &[0x6a, 0xb6, 0x9a, 0x4d]);
            // the cards of a Pi 5 in an Argon ONE V5 case, as ALSA lists them
            put(
                r,
                "proc/asound/cards",
                " 0 [vc4hdmi0       ]: vc4-hdmi - vc4-hdmi-0\n                      vc4-hdmi-0\n 1 [vc4hdmi1       ]: vc4-hdmi - vc4-hdmi-1\n                      vc4-hdmi-1\n 2 [Device         ]: USB-Audio - USB Audio Device\n                      C-Media Electronics Inc. USB Audio Device at usb-1000480000.usb-1.4, full speed\n",
            );
            put(r, "proc/asound/card2/usbid", "0d8c:0014\n");
            put(
                r,
                "proc/cpuinfo",
                "processor\t: 0\nBogoMIPS\t: 108.00\nFeatures\t: fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics\nCPU implementer\t: 0x41\nCPU part\t: 0xd0b\n\nprocessor\t: 1\n\nRevision\t: d04170\nSerial\t\t: 10000000deadbeef\nModel\t\t: Raspberry Pi 5 Model B Rev 1.0\n",
            );
            cpus(r, 4, 1);
            put(r, "proc/sys/kernel/arch", "aarch64\n");
            card(r, 0, None, "v3d", true, false);
            card(r, 1, None, "vc4-drm", false, false);
            let ed = edid("SAM", 0x7155, "LS27A600U", "SECRETMON03", 2560, 1440, 60, 34, 2022);
            conn(r, 1, "HDMI-A-1", true, Some(&ed), Some("2560x1440\n"));
            conn(r, 1, "HDMI-A-2", false, None, None);
            conn(r, 1, "Writeback-1", false, None, None);
            disk(r, "mmcblk0", "", "62333952\n", false, None);
            disk(r, "mmcblk0boot0", "", "8192\n", false, None);
            // the LordOS rpi-6.18.y kernel: no get_throttled under soc@107c000000:firmware, only rpi_volt's alarm
            mkdirs_in(r, "sys/devices/platform/soc@107c000000/soc@107c000000:firmware");
            put(r, "sys/class/hwmon/hwmon1/name", "rpi_volt\n");
            put(r, "sys/class/hwmon/hwmon1/in0_lcrit_alarm", "0\n");
            put(r, "sys/block/zram0/disksize", "2147483648\n");
            put(r, "sys/block/zram0/comp_algorithm", "lzo lzo-rle lz4 [zstd]\n");
            put(r, "sys/class/hwmon/hwmon2/name", "pwmfan");
            put(r, "sys/class/hwmon/hwmon2/fan1_input", "2900");
        }
        "vm" => {
            put(r, "sys/class/dmi/id/sys_vendor", "QEMU\n");
            put(r, "sys/class/dmi/id/product_name", "Standard PC (Q35 + ICH9, 2009)\n");
            put(r, "sys/class/dmi/id/chassis_type", "1\n");
            put(r, "sys/class/dmi/id/bios_vendor", "SeaBIOS\n");
            put(r, "proc/cpuinfo", "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel Xeon Processor (Skylake, IBRS)\nflags\t\t: fpu sse4_2 avx avx2 hypervisor\n");
            cpus(r, 4, 1);
            card(r, 0, Some(("0x1af4", "0x1050")), "virtio-pci", false, true);
            conn(r, 0, "Virtual-1", true, None, Some("1280x800\n1024x768\n"));
            disk(r, "vda", "", "67108864\n", true, None);
            netif(r, "ens18", Some("virtio_net"), false, Some("-1\n"));
        }
        _ => panic!("unknown fixture {kind}"),
    }
}

/// An input node under /sys/class/input: a link into sysfs, as the real ones are. The address
/// attributes are the things nobody may read.
pub fn input_node(r: &Path, n: u32, path: &str, bus: &str, name: &str, key: &str, rel: &str) {
    let real = format!("sys/devices/{path}/input/input{n}");
    put(r, &format!("{real}/name"), name);
    put(r, &format!("{real}/id/bustype"), bus);
    put(r, &format!("{real}/uniq"), "de:ad:be:ef:00:03\n");
    put(r, &format!("{real}/phys"), "de:ad:be:ef:00:04\n");
    put(r, &format!("{real}/capabilities/key"), key);
    put(r, &format!("{real}/capabilities/rel"), rel);
    lnk(r, &format!("sys/class/input/input{n}"), &format!("../../devices/{path}/input/input{n}"));
}

pub fn usb_dev(r: &Path, dev: &str, ven: &str, prod: &str, cls: &str, maker: Option<&str>, product: Option<&str>) {
    let d = format!("sys/bus/usb/devices/{dev}");
    put(r, &format!("{d}/idVendor"), ven);
    put(r, &format!("{d}/idProduct"), prod);
    put(r, &format!("{d}/bDeviceClass"), cls);
    put(r, &format!("{d}/serial"), "SECRET-USB\n");
    if let Some(m) = maker {
        put(r, &format!("{d}/manufacturer"), m);
    }
    if let Some(p) = product {
        put(r, &format!("{d}/product"), p);
    }
}

pub fn usb_if(r: &Path, ifc: &str, cls: &str, proto: &str) {
    put(r, &format!("sys/bus/usb/devices/{ifc}/bInterfaceClass"), cls);
    put(r, &format!("sys/bus/usb/devices/{ifc}/bInterfaceProtocol"), proto);
}

/// A block device `dev` ("devices/…/block/<disk>[/<part>]") with major:minor; a partition gets its file.
pub fn boot_dev(root: &Path, majmin: &str, dev: &str, part: bool) {
    put(root, &format!("sys/{dev}/dev"), majmin);
    if part {
        put(root, &format!("sys/{dev}/partition"), "1\n");
    }
    let t = format!("../../{dev}");
    lnk(root, &format!("sys/dev/block/{majmin}"), &t);
    lnk(root, &format!("sys/class/block/{}", dev.rsplit('/').next().unwrap()), &t);
}

// ---- the Raspberry Pi firmware's power and throttling ----

pub struct PiCase {
    pub name: &'static str,
    pub pi: bool,
    /// (path, content) of a get_throttled file.
    pub throttled: Option<(&'static str, &'static str)>,
    /// The rpi_volt hwmon: None = no hwmon, Some("") = no alarm file.
    pub alarm: Option<&'static str>,
    pub want_thr: i32,
    pub want_uv: i32,
    pub want_json: Option<&'static str>,
    pub not_json: Option<&'static str>,
    pub want_md: Option<&'static str>,
    pub not_md: Option<&'static str>,
}

pub fn build_pi(root: &Path, c: &PiCase) {
    mkdirs_in(root, "sys/devices/platform/soc@107c000000/soc@107c000000:firmware");
    if let Some((p, t)) = c.throttled {
        put(root, p, t);
    }
    if let Some(a) = c.alarm {
        put(root, "sys/class/hwmon/hwmon0/name", "rpi_volt\n");
        if !a.is_empty() {
            put(root, "sys/class/hwmon/hwmon0/in0_lcrit_alarm", a);
        }
    }
    put(root, "sys/class/hwmon/hwmon1/name", "cpu_thermal\n");
    if c.pi {
        putn(root, "proc/device-tree/compatible", b"raspberrypi,5-model-b\0brcm,bcm2712\0");
    }
}

const P5: &str = "sys/devices/platform/soc@107c000000/soc@107c000000:firmware/get_throttled";
const P4: &str = "sys/devices/platform/soc/soc:firmware/get_throttled";

pub fn pi_cases() -> Vec<PiCase> {
    let c = |name, pi, throttled, alarm, want_thr, want_uv, want_json, not_json, want_md, not_md| PiCase { name, pi, throttled, alarm, want_thr, want_uv, want_json, not_json, want_md, not_md };
    vec![
        // a Pi 5 with the LordOS kernel: rpi_volt only
        c("lordos ok", true, None, Some("0\n"), -1, 0, Some("\"throttled\":null,\"underVoltageAlarm\":false}"), None, Some("no under-voltage in the last 2 s (rpi_volt); throttling not reported by this kernel"), Some("since boot")),
        c("lordos alarm", true, None, Some("1\n"), -1, 1, Some("\"throttled\":null,\"underVoltageAlarm\":true}"), None, Some("; under-voltage in the last 2 s (rpi_volt)"), Some("slowed")),
        c("lordos bad alarm", true, None, Some("yes\n"), -1, -1, Some("\"throttled\":null,\"underVoltageAlarm\":null}"), None, None, Some("voltage")),
        c("lordos alarm 01", true, None, Some("01\n"), -1, -1, Some("\"underVoltageAlarm\":null}"), None, None, Some("voltage")),
        c("lordos no alarm file", true, None, Some(""), -1, -1, Some("\"underVoltageAlarm\":null}"), None, None, Some("voltage")),
        // an older kernel with get_throttled and no rpi_volt: the sticky bits are since boot
        c("old pi4 sticky", true, Some((P4, "0x50000\n")), None, 0x50000, -1, Some("\"throttled\":327680,\"underVoltageAlarm\":null}"), None, Some("; under-voltage since boot; slowed down since boot (get_throttled 0x50000)"), None),
        c("old pi5 clear", true, Some((P5, "0\n")), None, 0, -1, Some("\"throttled\":0"), None, Some("; no under-voltage since boot; not slowed down since boot (get_throttled 0x0)"), None),
        c("old now bits", true, Some((P5, "5\n")), None, 5, -1, None, None, Some("; under-voltage now; slowed down now (get_throttled 0x5)"), Some("since boot")),
        // both: rpi_volt clears the sticky bits every 2 s, so they don't cover the time since boot
        c("old with rpi_volt", true, Some((P5, "0x20000\n")), Some("0\n"), 0x20000, 0, Some("\"underVoltageAlarm\":false"), None, Some("; no under-voltage in the last 2 s; slowed down in the last 2 s (get_throttled 0x20000)"), Some("since boot")),
        c("old bad value", true, Some((P5, "0x5 junk\n")), None, -1, -1, Some("\"throttled\":null"), None, None, Some("voltage")),
        c("old second line", true, Some((P5, "0\n0\n")), None, -1, -1, Some("\"throttled\":null"), None, None, Some("voltage")),
        // a Pi that shows neither: model and revision only
        c("pi nothing", true, None, None, -1, -1, Some("\"throttled\":null,\"underVoltageAlarm\":null}"), None, Some("## Raspberry Pi"), Some("voltage")),
        // not a Pi (no raspberrypi in the device tree), even with the files: no Pi section
        c("not a pi", false, Some((P4, "0x50005\n")), Some("1\n"), -1, -1, None, Some("raspberryPi"), None, Some("Raspberry Pi")),
    ]
}

// ---- the boot medium ----

pub struct BootCase {
    pub what: &'static str,
    pub build: fn(&Path),
    pub live: bool,
    pub medium: &'static str,
    pub mbps: i32,
    pub toram: bool,
}

/// What a boot report must never carry.
pub const BOOT_NEVER: [&str; 9] = ["sdb", "nvme0n1", "mmcblk0", "dm-0", "sr0", "SECRET-STICK", "/run/live/medium", "BOOT_IMAGE", "server.lan"];

const NVME: &str = "devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/nvme0n1";
const USB3: &str = "devices/pci0000:00/0000:00:14.0/usb2/2-3";
const MMC_SD: &str = "devices/platform/axi/1000fff000.mmc/mmc_host/mmc0/mmc0:aaaa/block/mmcblk0";
const MMC_EMMC: &str = "devices/platform/soc/mmc0/mmc_host/mmc0/mmc0:0001/block/mmcblk0";

pub fn boot_cases() -> Vec<BootCase> {
    let c = |what, build: fn(&Path), live, medium, mbps, toram| BootCase { what, build, live, medium, mbps, toram };
    vec![
        // installed on an NVMe disk
        c("nvme", |r| {
            put(r, "proc/self/mountinfo", "22 1 0:21 / /proc rw - proc proc rw\n29 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw\n");
            put(r, "proc/cmdline", "BOOT_IMAGE=/boot/vmlinuz root=UUID=SECRET-UUID ro quiet\n");
            boot_dev(r, "259:2", &format!("{NVME}/nvme0n1p2"), true);
        }, false, "internal", -1, false),
        // live from a USB 3 stick
        c("live usb3", |r| {
            put(r, "proc/self/mountinfo", "29 1 0:30 / / rw - overlay overlay rw\n41 29 8:17 / /run/live/medium ro - iso9660 /dev/sdb1 ro\n");
            put(r, "proc/cmdline", "BOOT_IMAGE=/live/vmlinuz boot=live components quiet\n");
            boot_dev(r, "8:17", &format!("{USB3}/2-3:1.0/host4/target4:0:0/4:0:0:0/block/sdb/sdb1"), true);
            put(r, &format!("sys/{USB3}/speed"), "5000\n");
            put(r, &format!("sys/{USB3}/serial"), "SECRET-STICK\n");
        }, true, "usb", 5000, false),
        // live from a USB 2 stick behind a hub, copied to RAM (medium still mounted)
        c("live usb2 toram", |r| {
            put(r, "proc/self/mountinfo", "41 29 8:17 / /run/live/medium ro - iso9660 /dev/sdb1 ro\n");
            put(r, "proc/cmdline", "BOOT_IMAGE=/live/vmlinuz boot=live toram lordos.lang=en\n");
            boot_dev(r, "8:17", "devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2.4/1-2.4:1.0/host5/target5:0:0/5:0:0:0/block/sdb/sdb1", true);
            put(r, "sys/devices/pci0000:00/0000:00:14.0/usb1/1-2/speed", "480\n");
            put(r, "sys/devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2.4/speed", "480\n");
        }, true, "usb", 480, true),
        // copied to RAM and the medium let go: the class is unknown, the copy is not
        c("toram, medium gone", |r| {
            put(r, "proc/self/mountinfo", "29 1 0:30 / / rw - overlay overlay rw\n");
            put(r, "proc/cmdline", "boot=live toram\n");
        }, true, "", -1, true),
        // "notoram" and toram on an installed system are not the live copy
        c("toram when installed", |r| {
            put(r, "proc/self/mountinfo", "29 1 259:2 / / rw - ext4 /dev/nvme0n1p2 rw\n");
            put(r, "proc/cmdline", "notoram toram\n");
            boot_dev(r, "259:2", &format!("{NVME}/nvme0n1p2"), true);
        }, false, "internal", -1, false),
        c("notoram", |r| put(r, "proc/cmdline", "boot=live notoram toramx\n"), true, "", -1, false),
        // a Raspberry Pi's SD card, and an eMMC
        c("sd", |r| {
            put(r, "proc/self/mountinfo", "29 1 179:2 / / rw - ext4 /dev/mmcblk0p2 rw\n");
            boot_dev(r, "179:2", &format!("{MMC_SD}/mmcblk0p2"), true);
            lnk(r, &format!("sys/{MMC_SD}/device"), "../../../mmc0:aaaa");
            put(r, "sys/devices/platform/axi/1000fff000.mmc/mmc_host/mmc0/mmc0:aaaa/type", "SD\n");
            put(r, "sys/devices/platform/axi/1000fff000.mmc/mmc_host/mmc0/mmc0:aaaa/serial", "SECRET-STICK\n");
        }, false, "sd", -1, false),
        c("emmc", |r| {
            put(r, "proc/self/mountinfo", "29 1 179:2 / / rw - ext4 /dev/mmcblk0p2 rw\n");
            boot_dev(r, "179:2", &format!("{MMC_EMMC}/mmcblk0p2"), true);
            lnk(r, &format!("sys/{MMC_EMMC}/device"), "../../../mmc0:0001");
            put(r, "sys/devices/platform/soc/mmc0/mmc_host/mmc0/mmc0:0001/type", "MMC\n");
        }, false, "emmc", -1, false),
        // a card whose type can't be read: unknown, not a guess
        c("mmc unknown", |r| {
            put(r, "proc/self/mountinfo", "29 1 179:2 / / rw - ext4 /dev/mmcblk0p2 rw\n");
            boot_dev(r, "179:2", &format!("{MMC_EMMC}/mmcblk0p2"), true);
        }, false, "", -1, false),
        // encrypted root (device mapper) on the NVMe disk, and on a USB disk
        c("luks nvme", |r| {
            put(r, "proc/self/mountinfo", "29 1 254:0 / / rw - ext4 /dev/mapper/root rw\n");
            boot_dev(r, "254:0", "devices/virtual/block/dm-0", false);
            mkdirs_in(r, "sys/devices/virtual/block/dm-0/slaves/nvme0n1p3");
            boot_dev(r, "259:3", &format!("{NVME}/nvme0n1p3"), true);
        }, false, "internal", -1, false),
        c("lvm on luks on usb", |r| {
            put(r, "proc/self/mountinfo", "29 1 254:1 / / rw - ext4 /dev/mapper/root rw\n");
            boot_dev(r, "254:1", "devices/virtual/block/dm-1", false);
            mkdirs_in(r, "sys/devices/virtual/block/dm-1/slaves/dm-0");
            boot_dev(r, "254:0", "devices/virtual/block/dm-0", false);
            mkdirs_in(r, "sys/devices/virtual/block/dm-0/slaves/sdb2");
            boot_dev(r, "8:18", &format!("{USB3}/2-3:1.0/host4/target4:0:0/4:0:0:0/block/sdb/sdb2"), true);
            put(r, &format!("sys/{USB3}/speed"), "10000\n");
        }, false, "usb", 10000, false),
        // a loop of device mapper layers ends, unknown
        c("dm loop", |r| {
            put(r, "proc/self/mountinfo", "29 1 254:0 / / rw - ext4 /dev/mapper/root rw\n");
            boot_dev(r, "254:0", "devices/virtual/block/dm-0", false);
            mkdirs_in(r, "sys/devices/virtual/block/dm-0/slaves/dm-0");
        }, false, "", -1, false),
        // btrfs shows an anonymous device: the source names the disk
        c("btrfs sata", |r| {
            put(r, "proc/self/mountinfo", "29 1 0:31 /@ / rw - btrfs /dev/sdb2 rw\n");
            boot_dev(r, "8:18", "devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sdb/sdb2", true);
        }, false, "internal", -1, false),
        // the last mount on / wins
        c("stacked mounts", |r| {
            put(r, "proc/self/mountinfo", "1 0 0:2 / / rw - rootfs rootfs rw\n29 1 179:2 / / rw - ext4 /dev/mmcblk0p2 rw\n");
            boot_dev(r, "179:2", &format!("{MMC_EMMC}/mmcblk0p2"), true);
            lnk(r, &format!("sys/{MMC_EMMC}/device"), "../../../mmc0:0001");
            put(r, "sys/devices/platform/soc/mmc0/mmc_host/mmc0/mmc0:0001/type", "SD\n");
        }, false, "sd", -1, false),
        // a live DVD (or a virtual machine's CD drive)
        c("optical", |r| {
            put(r, "proc/self/mountinfo", "41 29 11:0 / /run/live/medium ro - iso9660 /dev/sr0 ro\n");
            boot_dev(r, "11:0", "devices/pci0000:00/0000:00:1f.2/ata2/host1/target1:0:0/1:0:0:0/block/sr0", false);
        }, true, "optical", -1, false),
        // network: an NFS root, iSCSI, NBD, and a live system fetched over the network
        c("nfs", |r| put(r, "proc/self/mountinfo", "29 1 0:23 / / rw - nfs4 server.lan:/export/root rw\n"), false, "network", -1, false),
        c("iscsi", |r| {
            put(r, "proc/self/mountinfo", "29 1 8:33 / / rw - ext4 /dev/sdc1 rw\n");
            boot_dev(r, "8:33", "devices/platform/host5/session1/target5:0:0/5:0:0:0/block/sdc/sdc1", true);
        }, false, "network", -1, false),
        c("nbd", |r| {
            put(r, "proc/self/mountinfo", "29 1 43:0 / / rw - ext4 /dev/nbd0 rw\n");
            boot_dev(r, "43:0", "devices/virtual/block/nbd0", false);
        }, false, "network", -1, false),
        c("live fetch", |r| put(r, "proc/cmdline", "boot=live fetch=http://server.lan/live/filesystem.squashfs\n"), true, "network", -1, false),
        // a virtual machine's disk, a live ISO on a loop device, and a link that climbs out of /sys
        c("virtio", |r| {
            put(r, "proc/self/mountinfo", "29 1 253:1 / / rw - ext4 /dev/vda1 rw\n");
            boot_dev(r, "253:1", "devices/pci0000:00/0000:00:04.0/virtio1/block/vda/vda1", true);
        }, false, "internal", -1, false),
        c("loop", |r| {
            put(r, "proc/self/mountinfo", "41 29 7:0 / /run/live/medium ro - iso9660 /dev/loop0 ro\n");
            boot_dev(r, "7:0", "devices/virtual/block/loop0", false);
        }, true, "", -1, false),
        c("escaping link", |r| {
            put(r, "proc/self/mountinfo", "29 1 8:1 / / rw - ext4 /dev/sda1 rw\n");
            lnk(r, "sys/dev/block/8:1", "../../devices/../../../etc/block/sda/sda1");
        }, false, "", -1, false),
        // USB without a readable speed: the class, no number
        c("usb no speed", |r| {
            put(r, "proc/self/mountinfo", "29 1 8:17 / / rw - ext4 /dev/sdb1 rw\n");
            boot_dev(r, "8:17", &format!("{USB3}/2-3:1.0/host4/target4:0:0/4:0:0:0/block/sdb/sdb1"), true);
            put(r, &format!("sys/{USB3}/speed"), "fast\n");
        }, false, "usb", -1, false),
    ]
}

// ---- the peripherals ----

/// The tree of the peripherals test: what each device is and how it hangs off the machine.
pub fn periph_tree(root: &Path) {
    common_secrets(root);
    put(root, "usr/share/misc/usb.ids", USB_IDS);
    usb_dev(root, "usb5", "1d6b", "0002", "09", Some("Linux xhci"), Some("Root Hub"));
    usb_dev(root, "5-1", "1a86", "8091", "09", None, Some("USB HUB"));
    usb_dev(root, "5-1.2", "046d", "c534", "00", Some("Logitech"), Some("USB Receiver"));
    usb_if(root, "5-1.2:1.0", "03", "01");
    usb_if(root, "5-1.2:1.1", "03", "02");
    usb_dev(root, "5-1.4", "0d8c", "0014", "00", Some("C-Media Electronics Inc."), Some("USB Audio Device"));
    usb_if(root, "5-1.4:1.0", "01", "00");
    usb_if(root, "5-1.4:1.3", "03", "00");
    usb_dev(root, "1-1", "04d9", "0024", "00", Some("secretuser"), Some("secretuser's keyboard"));
    usb_if(root, "1-1:1.0", "03", "01");
    usb_dev(root, "1-2", "046d", "c077", "00", Some("Logitech"), Some("Logitech USB Optical Mouse"));
    usb_if(root, "1-2:1.0", "03", "02");
    usb_dev(root, "1-3", "0781", "5583", "00", Some("SanDisk"), Some("Ultra Fit"));
    usb_if(root, "1-3:1.0", "08", "50");
    usb_dev(root, "1-4", "046d", "c52b", "00", None, None); // no strings: usb.ids; "Receiver" names it
    usb_if(root, "1-4:1.0", "03", "01");
    // behind the receiver: a mouse with a HID++ battery, and a keyboard without one
    let mouse = "platform/axi/1000480000.usb/usb5/5-1/5-1.2/5-1.2:1.1/0003:046D:C534.000C/0003:046D:4054.000D";
    let keys = "platform/axi/1000480000.usb/usb5/5-1/5-1.2/5-1.2:1.1/0003:046D:C534.000C/0003:046D:408A.000E";
    // key: BTN_LEFT..BTN_TASK in the 5th word from the right; rel: X, Y, WHEEL
    input_node(root, 26, mouse, "0003\n", "Logitech Wireless Mouse\n", "ffff0000 0 0 0 0\n", "143\n");
    input_node(root, 27, keys, "0003\n", "Logitech Wireless Keyboard PID:408a\n", "fffffffffffffffe\n", "0\n");
    put(root, "sys/class/power_supply/hidpp_battery_0/scope", "Device\n");
    put(root, "sys/class/power_supply/hidpp_battery_0/capacity", "64\n");
    put(root, "sys/class/power_supply/hidpp_battery_0/model_name", "MX Master 3\n");
    put(root, "sys/class/power_supply/hidpp_battery_0/serial_number", "SECRET-BAT\n");
    lnk(root, "sys/class/power_supply/hidpp_battery_0/device", "../../../0003:046D:4054.000D");
    // a direct USB device's node, a laptop's keyboard and touchpad, a Bluetooth keyboard (BlueZ's), the rest
    input_node(root, 2, "pci0000:00/usb1/1-1/1-1:1.0/0003:04D9:0024.0001", "0003\n", "input-not-listed-twice\n", "fffffffffffffffe\n", "0\n");
    input_node(root, 3, "platform/i8042/serio0", "0011\n", "AT Translated Set 2 keyboard\n", "fffffffffffffffe\n", "0\n");
    input_node(root, 4, "pci0000:00/i2c-1/i2c-SYNA", "0018\n", "SYNA2B33:00 Touchpad\n", "420 0 0 0 0 0\n", "0\n");
    input_node(root, 5, "virtual/misc/uhid/0005:046D:B35B.0009", "0005\n", "MX Keys\n", "fffffffffffffffe\n", "0\n");
    input_node(root, 6, "platform/pwr_button", "0019\n", "pwr_button\n", "10000000000000 0\n", "0\n");
    input_node(root, 7, "platform/soc/107c701400.hdmi", "001e\n", "vc4-hdmi-0\n", "fffffffffffffffe\n", "3\n");
}

/// Facts' fake session.
pub fn facts_tree(root: &Path) {
    put(root, "proc/self/status", "Name:\tlord-server\nUmask:\t0022\nState:\tS\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\n");
    put(root, "proc/self/cgroup", "0::/system.slice/lordos.service/lordos-session-1000/server\n");
    let b = "sys/fs/cgroup/system.slice/lordos.service/lordos-session-1000";
    mkdirs_in(root, &format!("{b}/server"));
    mkdirs_in(root, &format!("{b}/app-lordos-media.scope"));
    mkdirs_in(root, &format!("{b}/app-lordos-chromium.scope"));
    put(root, &format!("{b}/cgroup.procs"), "");
}
