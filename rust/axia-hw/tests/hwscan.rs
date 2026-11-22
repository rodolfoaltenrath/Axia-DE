//! The Store suggestions' hardware facts over fake trees: DMI, device tree, PCI, USB (interfaces,
//! drivers, udev's libsane flag), virtio ports and the firmware misses of a kernel log (kmsg and
//! dmesg formats, names already present, unsafe names).
mod support;

use axia_hw::hwscan::{self, FwMiss};
use serde_json::Value;
use support::*;

fn facts(kind: &str) -> Value {
    let d = temp_dir(kind);
    tree(&d, kind);
    let j = hwscan::json(d.to_str().unwrap());
    let v: Value = serde_json::from_slice(&j).unwrap_or_else(|e| panic!("{kind}: the facts are not JSON: {e}"));
    assert_eq!(v["fixture"], true, "{kind}: fixture flag");
    v
}

fn pci_by<'a>(v: &'a Value, vendor: &str) -> Option<&'a Value> {
    v["pci"].as_array()?.iter().find(|p| p["vendor"] == vendor)
}

#[test]
fn a_vmware_guest() {
    let v = facts("vmware");
    assert_eq!(v["dmi"]["sysVendor"], "VMware, Inc.");
    let g = pci_by(&v, "15ad").unwrap();
    assert_eq!((g["device"].as_str(), g["class"].as_str(), g["driver"].as_str()), (Some("0405"), Some("030000"), Some("vmwgfx")));
    assert_eq!(v["usb"].as_array().unwrap().len(), 0, "the root hub is not listed");
    assert_eq!(v["firmware"]["readable"], false, "no kmsg file → not readable");
}

#[test]
fn virtualbox_and_qemu_guests() {
    let v = facts("virtualbox");
    assert_eq!(v["dmi"]["productName"], "VirtualBox");
    assert!(pci_by(&v, "80ee").is_some(), "guest device");
    let v = facts("qemu");
    assert_eq!(v["virtio"][0], "org.qemu.guest_agent.0", "guest agent port");
}

#[test]
fn a_raspberry_pi_5_has_a_device_tree_and_no_dmi() {
    let v = facts("rpi5");
    assert_eq!(v["model"], "Raspberry Pi 5 Model B Rev 1.1");
    assert_eq!((v["compatible"][0].as_str(), v["compatible"][1].as_str()), (Some("raspberrypi,5-model-b"), Some("brcm,bcm2712")));
    assert_eq!(v["arch"], "aarch64");
    assert_eq!(v["dmi"]["sysVendor"], "");
}

#[test]
fn a_gpu_a_printer_a_scanner() {
    let v = facts("nvidia");
    let g = pci_by(&v, "10de").unwrap();
    assert_eq!((g["class"].as_str(), g["driver"].as_str()), (Some("030000"), Some("nouveau")));
    let v = facts("printer");
    let u = &v["usb"][0];
    assert_eq!((u["vendor"].as_str(), u["product"].as_str(), u["driver"].as_str()), (Some("03f0"), Some("2b17"), Some("usblp")));
    assert_eq!(u["ifaces"], serde_json::json!(["070102", "070104"]), "interfaces 07/01/02 and 07/01/04 (IPP over USB)");
    assert_eq!(u["sane"], false, "not a scanner");
    let v = facts("scanner");
    assert_eq!((v["usb"][0]["vendor"].as_str(), v["usb"][0]["sane"].as_bool()), (Some("04b8"), Some(true)), "udev's libsane flag");
}

#[test]
fn firmware_a_driver_asked_for_and_did_not_find() {
    let v = facts("firmware");
    assert_eq!(v["firmware"]["readable"], true);
    let m = v["firmware"]["missing"].as_array().unwrap();
    assert_eq!(m.len(), 2, "two misses");
    assert_eq!((m[0]["file"].as_str(), m[0]["driver"].as_str()), (Some("brcm/brcmfmac4360-pcie.bin"), Some("brcmfmac")));
    assert_eq!((m[1]["file"].as_str(), m[1]["driver"].as_str()), (Some("rtl_bt/rtl8761bu_fw.bin"), Some("bluetooth")));
}

#[test]
fn this_machine_is_valid_json_and_not_a_fixture() {
    let v: Value = serde_json::from_slice(&hwscan::json("")).unwrap();
    assert_eq!(v["fixture"], false);
    assert!(v["arch"].as_str().is_some_and(|a| !a.is_empty()));
}

fn miss(file: &str, driver: &str) -> FwMiss {
    FwMiss { file: file.into(), driver: driver.into() }
}

#[test]
fn the_kernel_log_is_parsed_for_missing_firmware() {
    let log = "[    3.1] iwlwifi 0000:00:14.3: Direct firmware load for iwlwifi-so-a0-gf-a0-89.ucode failed with error -2\n\
[    3.2] iwlwifi 0000:00:14.3: Direct firmware load for iwlwifi-so-a0-gf-a0-89.ucode failed with error -2\n\
[    3.3] r8169 0000:03:00.0: firmware: failed to load rtl_nic/rtl8168h-2.fw (-2)\n\
[    3.4] evil 0000:00:01.0: Direct firmware load for ../../etc/shadow failed with error -2\n\
[    3.5] evil 0000:00:01.0: Direct firmware load for /etc/shadow failed with error -2\n\
[    3.6] evil 0000:00:01.0: Direct firmware load for a b failed with error -2\n\
[    3.7] amdgpu: Direct firmware load for amdgpu/navi10_pfp.bin failed with error -2\n\
[    3.8] some unrelated line about firmware\n";
    let m = hwscan::fw_parse(log.as_bytes(), 8);
    assert_eq!(m, [miss("iwlwifi-so-a0-gf-a0-89.ucode", "iwlwifi"), miss("rtl_nic/rtl8168h-2.fw", "r8169"), miss("amdgpu/navi10_pfp.bin", "")], "3 unique valid misses; no device, no driver name");
    assert_eq!(hwscan::fw_parse(log.as_bytes(), 1).len(), 1, "max respected");
    for bad in ["", "/x", "../x", "a/../b", ".hidden", "a//b", "a b", "a;b", "a\\b", "a\n"] {
        assert!(!hwscan::fw_name_valid(bad.as_bytes()), "name rejected: {bad:?}");
    }
    for good in ["iwlwifi-cc-a0-77.ucode", "rtl_bt/rtl8761bu_fw.bin", "brcm/brcmfmac43455-sdio.raspberrypi,5-model-b.bin", "intel/ibt-0040-0041.sfi", "mediatek/WIFI_RAM_CODE_MT7922_1.bin"] {
        assert!(hwscan::fw_name_valid(good.as_bytes()), "name accepted: {good}");
    }
    // a long line without the end of the pattern, and an empty log
    let mut big = vec![b'x'; 600];
    big[..25].copy_from_slice(b"Direct firmware load for ");
    assert!(hwscan::fw_parse(&big, 8).is_empty(), "truncated line");
    assert!(hwscan::fw_parse(b"", 8).is_empty());
    assert!(hwscan::fw_parse(b"\n\n \n", 8).is_empty(), "empty lines");
}

#[test]
fn firmware_installed_since_the_miss_is_dropped() {
    let root = temp_dir("fwpresent");
    put(&root, "lib/firmware/rtl_nic/rtl8168h-2.fw.zst", "x");
    put(&root, "lib/firmware/updates/iwlwifi-cc-a0-77.ucode", "x");
    let r = root.to_str().unwrap();
    assert!(hwscan::fw_present(r, "rtl_nic/rtl8168h-2.fw"), ".zst");
    assert!(hwscan::fw_present(r, "iwlwifi-cc-a0-77.ucode"), "updates/");
    assert!(!hwscan::fw_present(r, "rtl_bt/rtl8761bu_fw.bin"), "missing");
    assert!(!hwscan::fw_present(r, "../firmware/rtl_nic/rtl8168h-2.fw.zst"), "unsafe name");
    let log = "4,1,1,-;r8169 0000:03:00.0: Direct firmware load for rtl_nic/rtl8168h-2.fw failed with error -2\n4,2,2,-;bluetooth hci0: Direct firmware load for rtl_bt/rtl8761bu_fw.bin failed with error -2\n";
    assert_eq!(hwscan::fw_missing(r, log.as_bytes(), 4), [miss("rtl_bt/rtl8761bu_fw.bin", "bluetooth")], "installed since → dropped");
}

#[test]
fn the_kernel_log_of_a_fixture_is_a_file_and_a_refusal_is_told() {
    let root = temp_dir("kmsg");
    assert_eq!(hwscan::read_kmsg(root.to_str().unwrap()), Err(false), "no log: not refused, just missing");
    put(&root, "kmsg", "6,1,1,-;hello\n");
    assert_eq!(hwscan::read_kmsg(root.to_str().unwrap()).unwrap(), b"6,1,1,-;hello\n");
    // a link in the place of the log is not followed
    let other = temp_dir("kmsg-link");
    put(&other, "real", "6,1,1,-;secret\n");
    lnk(&other, "kmsg", other.join("real").to_str().unwrap());
    assert!(hwscan::read_kmsg(other.to_str().unwrap()).is_err());
}
