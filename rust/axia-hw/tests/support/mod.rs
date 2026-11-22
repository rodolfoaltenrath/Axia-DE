//! Fake trees for the hardware readers' tests.
#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};

pub fn temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("axia-hw-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

pub fn put(root: &Path, rel: &str, text: &str) {
    putb(root, rel, text.as_bytes());
}

pub fn putb(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

pub fn lnk(root: &Path, rel: &str, target: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let _ = fs::remove_file(&p);
    std::os::unix::fs::symlink(target, p).unwrap();
}

pub fn link_driver(root: &Path, dir: &str, driver: &str) {
    fs::create_dir_all(root.join(dir)).unwrap();
    lnk(root, &format!("{dir}/driver"), &format!("../../../bus/drivers/{driver}"));
}

pub fn pci(root: &Path, slot: &str, vendor: &str, device: &str, cls: &str, driver: Option<&str>) {
    let d = format!("sys/bus/pci/devices/{slot}");
    put(root, &format!("{d}/vendor"), vendor);
    put(root, &format!("{d}/device"), device);
    put(root, &format!("{d}/class"), cls);
    if let Some(drv) = driver {
        link_driver(root, &d, drv);
    }
}

pub fn usb_dev(root: &Path, name: &str, vendor: &str, product: &str, dev: Option<&str>) {
    let d = format!("sys/bus/usb/devices/{name}");
    put(root, &format!("{d}/idVendor"), vendor);
    put(root, &format!("{d}/idProduct"), product);
    put(root, &format!("{d}/bDeviceClass"), "00\n");
    if let Some(dev) = dev {
        put(root, &format!("{d}/dev"), dev);
    }
}

pub fn usb_iface(root: &Path, name: &str, cls: &str, sub: &str, proto: &str, driver: Option<&str>) {
    let d = format!("sys/bus/usb/devices/{name}");
    put(root, &format!("{d}/bInterfaceClass"), cls);
    put(root, &format!("{d}/bInterfaceSubClass"), sub);
    put(root, &format!("{d}/bInterfaceProtocol"), proto);
    if let Some(drv) = driver {
        link_driver(root, &d, drv);
    }
}

pub fn dmi(root: &Path, vendor: &str, product: &str) {
    put(root, "sys/class/dmi/id/sys_vendor", vendor);
    put(root, "sys/class/dmi/id/product_name", product);
    put(root, "sys/class/dmi/id/chassis_type", "1\n");
}

pub const KINDS: [&str; 9] = ["vmware", "virtualbox", "qemu", "rpi5", "nvidia", "printer", "scanner", "firmware", "demo"];

/// The kinds of fake tree of the Store's hardware facts.
pub fn tree(root: &Path, kind: &str) {
    let demo = kind == "demo";
    put(root, "arch", "x86_64\n");
    // every PC has a host bridge and a USB root hub, which never suggest anything
    pci(root, "0000:00:00.0", "0x8086\n", "0x29c0\n", "0x060000\n", None);
    usb_dev(root, "usb1", "1d6b", "0002", Some("189:0\n"));
    match kind {
        "vmware" => {
            dmi(root, "VMware, Inc.\n", "VMware Virtual Platform\n");
            pci(root, "0000:00:0f.0", "0x15ad\n", "0x0405\n", "0x030000\n", Some("vmwgfx"));
        }
        "virtualbox" => {
            dmi(root, "innotek GmbH\n", "VirtualBox\n");
            pci(root, "0000:00:04.0", "0x80ee\n", "0xcafe\n", "0x088000\n", Some("vboxguest"));
        }
        "qemu" => {
            dmi(root, "QEMU\n", "Standard PC (Q35 + ICH9, 2009)\n");
            put(root, "sys/class/virtio-ports/vport2p1/name", "org.qemu.guest_agent.0\n");
        }
        "rpi5" => {
            put(root, "arch", "aarch64\n");
            putb(root, "proc/device-tree/model", b"Raspberry Pi 5 Model B Rev 1.1\0");
            putb(root, "proc/device-tree/compatible", b"raspberrypi,5-model-b\0brcm,bcm2712\0");
        }
        _ => dmi(root, "Example Computer Co.\n", "Desktop 3000\n"),
    }
    if demo {
        put(root, "sys/class/virtio-ports/vport2p1/name", "org.qemu.guest_agent.0\n");
    }
    if kind == "nvidia" || demo {
        pci(root, "0000:01:00.0", "0x10de\n", "0x2484\n", "0x030000\n", Some("nouveau"));
    }
    if kind == "printer" || demo {
        usb_dev(root, "1-2", "03f0", "2b17", Some("189:2\n"));
        usb_iface(root, "1-2:1.0", "07\n", "01\n", "02\n", Some("usblp"));
        usb_iface(root, "1-2:1.1", "07\n", "01\n", "04\n", None);
    }
    if kind == "scanner" {
        usb_dev(root, "1-3", "04b8", "0142", Some("189:3\n"));
        usb_iface(root, "1-3:1.0", "ff\n", "ff\n", "ff\n", None);
        put(root, "run/udev/data/c189:3", "I:123\nE:ID_VENDOR=EPSON\nE:libsane_matched=yes\nG:scanner\n");
    }
    if kind == "firmware" || demo {
        pci(root, "0000:02:00.0", "0x14e4\n", "0x43a0\n", "0x028000\n", None);
        put(
            root,
            "kmsg",
            "6,512,4100000,-;brcmfmac 0000:02:00.0: Direct firmware load for brcm/brcmfmac4360-pcie.bin failed with error -2\n SUBSYSTEM=pci\n DEVICE=+pci:0000:02:00.0\n3,513,4100100,-;brcmfmac 0000:02:00.0: brcmf_pcie_setup: Dongle setup failed\n6,514,4200000,-;bluetooth hci0: Direct firmware load for rtl_bt/rtl8761bu_fw.bin failed with error -2\n",
        );
        if !demo {
            put(root, "lib/firmware/rtl_nic/rtl8168h-2.fw", "x");
        }
    }
}

/// The set of fake power supplies of the C test: a mains adapter, a laptop battery and a mouse's.
pub fn supply(dir: &Path, dev: &str, name: &str, val: &str) {
    put(dir, &format!("{dev}/{name}"), &format!("{val}\n"));
}
