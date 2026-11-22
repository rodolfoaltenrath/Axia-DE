//! System Information's collector over fake trees (an AMD desktop, an Intel laptop, an Nvidia desktop, a
//! Raspberry Pi 5 with the LordOS kernel and a VM): what each report finds, that its JSON is valid, and
//! that none of the values a report must never carry (serial numbers, UUIDs, MAC addresses, a host
//! name, a user name, a monitor's serial, a MAC inside an interface name) reaches the JSON, the
//! Markdown or the `axia-fetch` summary.
mod support;

use axia_sysreport::{collect, fetch, out, Report};
use serde_json::Value;
use std::path::Path;
use support::*;

fn run(root: &Path, lordos: &str) -> (Report, Value, String, String) {
    let mut r = collect(root.to_str().unwrap(), None, None);
    r.sw.lordos = lordos.into();
    let (json, md) = (out::json(&r), out::markdown(&r));
    let v: Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{}: the JSON is invalid: {e}", root.display()));
    for s in SECRETS {
        assert!(!json.contains(s), "{}: the JSON carries {s:?}", root.display());
        assert!(!md.contains(s), "{}: the Markdown carries {s:?}", root.display());
    }
    (r, v, json, md)
}

fn tree(kind: &str) -> std::path::PathBuf {
    let d = temp_dir(kind);
    fixture(&d, kind);
    d
}

#[test]
fn every_machine_says_what_the_kernel_made_of_it() {
    for kind in KINDS {
        let d = tree(kind);
        let (r, _, json, md) = run(&d, "0.8.5-beta");
        assert!(r.schema == 1 && r.date.len() == 10, "{kind}: schema and date");
        assert!(r.sw.os == "Debian GNU/Linux 13 (trixie)" && r.sw.os_version == "13", "{kind}: os {}", r.sw.os);
        assert_eq!(r.sw.landlock_abi, -1, "{kind}: Landlock is unknown under a fake root");
        assert!(r.sw.bpf_lsm && r.sw.cgroup2 && r.sw.netperm && !r.sw.fand, "{kind}: lsm, cgroup, services");
        let kc = |n: &str| r.sw.kconf.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        assert_eq!((kc("CONFIG_SECURITY_LANDLOCK"), kc("CONFIG_BPF_LSM"), kc("CONFIG_ZRAM")), (Some("y"), Some("n"), Some("m")), "{kind}: kernel config");
        if kind != "rpi5" {
            assert_eq!(r.audio, ["HD-Audio Generic"], "{kind}: audio");
        }
        assert!(md.contains("# System report") && md.contains("| Axia DE | 0.8.5-beta |"), "{kind}: markdown head");
        assert!(json.ends_with("}\n"), "{kind}: one line");
    }
}

#[test]
fn an_amd_desktop() {
    let (r, _, json, md) = run(&tree("amd"), "x");
    let m = &r.machine;
    assert!(m.vendor == "Micro-Star International Co., Ltd." && m.version.is_empty(), "a placeholder is dropped");
    assert_eq!((m.chassis.as_str(), m.firmware.as_str(), m.secure_boot), ("desktop", "uefi", 1));
    assert!(m.board_version == "1.0" && json.contains("\"boardVersion\":\"1.0\"") && md.contains("(MS-7C02), 1.0 |"));
    assert_eq!((m.bios_vendor.as_str(), m.bios_version.as_str(), m.bios_date.as_str()), ("American Megatrends Inc.", "3.J0", "11/23/2023"));
    assert!(m.virt.is_empty(), "bare metal");
    let c = &r.cpu;
    assert_eq!((c.sockets, c.cores, c.threads), (1, 8, 16));
    assert_eq!((c.max_mhz, c.min_mhz, c.governor.as_str()), (4200, 800, "schedutil"));
    assert_eq!((c.l1d_kb, c.l2_kb, c.l3_kb), (32, 512, 32768));
    assert_eq!(c.flags, "sse4_2 avx avx2 fma aes sha_ni");
    assert_eq!((r.mem.total_kb, r.mem.swap_kb, r.mem.zram.len()), (32768000, 2097148, 0));
    let g = &r.gpu[0];
    assert!(r.gpu.len() == 1 && g.driver == "amdgpu" && g.render_node && g.boot_vga && g.connectors == 2 && g.connected == 1 && g.bus == "pci");
    assert!(g.device.contains("Radeon RX 470/480/570"), "gpu name {}", g.device);
    let d = &r.display[0];
    assert!(r.display.len() == 1 && d.name == "DELL U2720Q" && d.pnp == "DEL" && d.pref_w == 3840 && d.year == 2021 && d.diag_tenths > 270 && d.connector == "DP-1" && d.mode == "3840x2160");
    assert!(r.disk.len() == 2 && r.disk[0].name == "nvme0n1" && r.disk[0].transport == "nvme" && r.disk[0].link == "8.0 GT/s PCIe x4" && r.disk[1].transport == "sata" && r.disk[1].rotational);
    assert!(r.pci.len() == 3 && r.pci[0].class_name == "Non-Volatile memory controller" && r.pci[0].driver == "nvme");
    assert!(r.usb.len() == 1 && r.usb[0].product == "Unifying Receiver" && r.usb[0].driver == "usbhid" && r.usb[0].speed_mbps == 12);
    assert!(r.net.len() == 2 && r.net[0].kind == "virtual" && r.net[1].kind == "ethernet" && r.net[1].speed_mbps == 1000);
    let fans: i32 = r.thermal.iter().map(|t| t.fans).sum();
    assert!(r.thermal.len() == 2 && r.thermal[0].milli_c == 41250 && fans == 2);
    assert!(!r.pi.is_pi && !json.contains("raspberryPi"));
}

#[test]
fn an_intel_laptop() {
    let (r, _, _, md) = run(&tree("intel"), "x");
    assert!(r.machine.chassis == "laptop" && r.machine.version == "ThinkPad X1 Carbon Gen 9" && r.machine.secure_boot == -1);
    assert!(r.machine.board_version.is_empty(), "a placeholder board version is dropped");
    assert_eq!(r.cpu.flags, "sse4_2 avx avx2 fma avx512f aes vaes");
    assert!(r.display.len() == 1 && r.display[0].name.is_empty() && r.display[0].pnp == "AUO" && md.contains("| eDP-1 | AUO, 2020 |"), "a panel with no name");
    assert!(r.net.len() == 2 && r.net[0].kind == "ethernet" && r.net[0].driver == "r8152" && r.net[1].kind == "wifi" && r.net[1].driver == "iwlwifi");
    assert!(r.power.len() == 2 && r.power[0].kind == "ac" && r.power[0].online == 0 && r.power[1].kind == "battery" && r.power[1].capacity == 76 && r.power[1].health == 78);
    assert!(md.contains("holds 78 % of its design charge"));
    assert_eq!(r.bt_driver, ["btusb"]);
    assert!(r.pci[1].class_name.contains("Network controller") && r.pci[1].device.contains("AX210"));
}

#[test]
fn an_nvidia_desktop_with_nothing_plugged_in() {
    let (r, _, _, md) = run(&tree("nvidia"), "x");
    assert!(r.gpu.len() == 1 && r.gpu[0].vendor == "NVIDIA Corporation" && r.gpu[0].driver == "nouveau" && r.gpu[0].connected == 0);
    assert!(r.cpu.cores == 10 && r.cpu.threads == 20 && r.machine.chassis == "desktop");
    assert!(r.display.is_empty() && !md.contains("## Displays"));
}

#[test]
fn a_raspberry_pi_5_with_the_lordos_kernel() {
    let d = tree("rpi5");
    let (r, _, json, md) = run(&d, "x");
    assert!(r.pi.is_pi && r.pi.revision == "d04170" && r.pi.throttled == -1 && r.pi.uv_alarm == 0);
    assert!(r.machine.firmware == "dt" && r.machine.model == "Raspberry Pi 5 Model B Rev 1.0");
    assert!(r.pi.bootloader_version == "1accd6659983613cd104973ce2432da14f1a7682" && r.pi.bootloader_date == "2026-09-25", "{} {}", r.pi.bootloader_version, r.pi.bootloader_date);
    assert!(json.contains("\"bootloaderDate\":\"2026-09-25\"") && md.contains(", bootloader of 2026-09-25"));
    assert_eq!(r.audio, ["Raspberry Pi HDMI 0 audio", "Raspberry Pi HDMI 1 audio", "C-Media Electronics, Inc. Audio Adapter (Unitek Y-247A)"]);
    assert!(!json.contains("usb-1000480000"), "no USB path in the audio names");
    // the Pi image has no usb.ids
    let no_ids = collect(d.to_str().unwrap(), None, Some("/nonexistent/usb.ids"));
    assert_eq!(no_ids.audio[2], "C-Media Electronics Inc. USB Audio Device");
    assert!(r.cpu.vendor == "ARM" && r.cpu.model == "Cortex-A76" && r.cpu.arch == "aarch64");
    assert_eq!(r.cpu.flags, "asimd aes sha2 crc32 atomics");
    assert!(r.gpu.len() == 2 && r.gpu[0].bus == "platform" && r.gpu[0].driver == "v3d" && r.gpu[0].render_node && r.gpu[1].driver == "vc4-drm" && r.gpu[1].connectors == 2 && r.gpu[1].connected == 1);
    assert!(r.display.len() == 1 && r.display[0].card == "card1" && r.display[0].name == "LS27A600U");
    assert!(r.disk.len() == 1 && r.disk[0].transport == "mmc", "the SD card, its boot partitions skipped");
    assert_eq!(r.mem.zram, [(2097152, "zstd".to_string())]);
    assert!(json.contains("\"raspberryPi\":{") && json.contains("\"throttled\":null,\"underVoltageAlarm\":false}"));
    assert!(md.contains("no under-voltage in the last 2 s (rpi_volt); throttling not reported by this kernel"));
}

#[test]
fn a_virtual_machine() {
    let (r, _, json, md) = run(&tree("vm"), "x");
    assert!(r.machine.virt == "kvm" && r.machine.chassis == "vm" && r.machine.firmware == "bios");
    assert!(r.gpu.len() == 1 && r.gpu[0].device == "Virtio 1.0 GPU" && !r.gpu[0].render_node);
    assert!(r.display.len() == 1 && r.display[0].pref_w == 0 && md.contains("| Virtual-1 | — | — | 1280x800 |"), "a display without EDID");
    assert!(r.disk.len() == 1 && r.disk[0].transport == "virtio" && r.net.len() == 1 && r.net[0].speed_mbps == -1);
    assert!(json.contains("\"speedMbps\":null"), "an unknown speed is null");
}

#[test]
fn nothing_is_there_and_the_live_session_is_a_folder() {
    let d = temp_dir("empty");
    let (r, _, json, md) = run(&d, "x");
    assert!(r.gpu.is_empty() && r.disk.is_empty() && r.cpu.threads == 0 && md.contains("none found"));
    assert!(!r.sw.live && json.contains("\"live\":false"));
    mkdirs_in(&d, "run/live");
    let (r, _, json, _) = run(&d, "x");
    assert!(r.sw.live && json.contains("\"cgroup2\":false,\"live\":true"));
}

#[test]
fn the_json_has_the_same_shape_for_every_machine() {
    for kind in KINDS {
        let (_, v, _, _) = run(&tree(kind), "x");
        for key in ["schema", "date", "machine", "cpu", "memory", "gpus", "displays", "storage", "pci", "usb", "network", "audio", "bluetooth", "power", "thermal", "software"] {
            assert!(v.get(key).is_some(), "{kind}: no {key}");
        }
        assert_eq!(v.get("raspberryPi").is_some(), kind == "rpi5");
        assert_eq!(v["software"]["services"], serde_json::json!({ "lord-sessiond": false, "lord-netd": false, "axia-netperm": true, "axia-fand": false }));
        assert_eq!(v["schema"], 1);
    }
}

#[test]
fn what_the_pi_firmware_says_is_reported_and_what_it_does_not_is_left_out() {
    for c in pi_cases() {
        let d = temp_dir("pi");
        build_pi(&d, &c);
        let (r, _, json, md) = run(&d, "x");
        assert_eq!(r.pi.is_pi, c.pi, "{}: is_pi", c.name);
        assert_eq!((r.pi.throttled, r.pi.uv_alarm), (c.want_thr, c.want_uv), "{}: throttled and alarm", c.name);
        if let Some(w) = c.want_json {
            assert!(json.contains(w), "{}: json has {w}", c.name);
        }
        if let Some(w) = c.not_json {
            assert!(!json.contains(w), "{}: json lacks {w}", c.name);
        }
        if let Some(w) = c.want_md {
            assert!(md.contains(w), "{}: markdown has {w:?}\n{md}", c.name);
        }
        if let Some(w) = c.not_md {
            assert!(!md.contains(w), "{}: markdown lacks {w:?}", c.name);
        }
    }
}

#[test]
fn the_boot_medium_is_a_class_and_never_a_device() {
    for c in boot_cases() {
        let d = temp_dir("boot");
        (c.build)(&d);
        if c.live {
            mkdirs_in(&d, "run/live");
        }
        let (r, _, json, md) = run(&d, "x");
        let what = c.what;
        assert!(r.sw.live == c.live && r.sw.boot_medium == c.medium && r.sw.boot_link_mbps == c.mbps && r.sw.boot_toram == c.toram, "boot {what}: {:?} {} {} (want {:?} {} {})", r.sw.boot_medium, r.sw.boot_link_mbps, r.sw.boot_toram, c.medium, c.mbps, c.toram);
        assert!(json.contains(&format!("\"bootMedium\":\"{}\"", c.medium)), "boot {what}: JSON");
        if c.mbps < 0 {
            assert!(json.contains("\"bootLinkMbps\":null"), "boot {what}: no link speed");
        }
        assert!(json.contains(if c.toram { "\"bootToRam\":true" } else { "\"bootToRam\":false" }), "boot {what}: toram in the JSON");
        for n in BOOT_NEVER {
            assert!(!json.contains(n) && !md.contains(n), "boot {what}: carries {n:?}");
        }
        if c.live || !c.medium.is_empty() {
            assert!(md.contains(if c.live { "| Started | live session" } else { "| Started | installed" }), "boot {what}: Markdown row");
        }
        if c.mbps > 0 {
            assert!(md.contains(&format!("from a USB drive ({} Mbps)", c.mbps)), "boot {what}: Markdown speed");
        }
        assert_eq!(md.contains("copied to memory"), c.toram, "boot {what}: Markdown copy to memory");
        if c.medium == "internal" {
            assert!(md.contains("from an internal disk"), "boot {what}: Markdown internal");
        }
    }
}

#[test]
fn the_kernel_command_line_is_read_by_whole_words() {
    let w = "lordos.check=1";
    assert!(axia_sysreport::cmdline_has("boot=live lordos.check=1 lordos.lang=en\n", w), "in the middle");
    assert!(axia_sysreport::cmdline_has("boot=live lordos.check=1\n", w), "at the end");
    assert!(axia_sysreport::cmdline_has("lordos.check=1", w), "alone");
    assert!(!axia_sysreport::cmdline_has("boot=live lordos.lang=en\n", w), "not there");
    assert!(!axia_sysreport::cmdline_has("xlordos.check=1 lordos.check=10 lordos.check=0\n", w), "only the whole word");
    assert!(axia_sysreport::cmdline_has("fetch=http://x/y boot=live", "fetch="), "a word that ends in =");
}

#[test]
fn the_summary_for_a_screenshot_carries_no_names() {
    for kind in KINDS {
        let d = tree(kind);
        let (r, _, _, _) = run(&d, "0.8.5-beta");
        let live = fetch::live(d.to_str().unwrap(), true);
        assert!(live.user.is_empty() && live.host.is_empty(), "{kind}: --share has no names");
        for (i, c) in [fetch::Color::Plain, fetch::Color::C256, fetch::Color::TrueColor].into_iter().enumerate() {
            let o = fetch::render(&r, &live, c);
            assert!(o.contains("Axia DE 0.8.5-beta (Debian GNU/Linux 13 (trixie))") && o.contains("CPU"), "{kind}/{i}: fetch lines");
            for s in SECRETS {
                assert!(!o.contains(s), "{kind}/{i}: fetch --share carries {s:?}");
            }
            assert!(c != fetch::Color::Plain || !o.contains('\x1b'), "{kind}: a plain summary has escape codes");
            assert!(c != fetch::Color::TrueColor || o.contains("\x1b[38;2;180;108;255m"), "{kind}: true color mark");
            assert!(c != fetch::Color::C256 || o.contains("\x1b[38;5;135m"), "{kind}: 256-color mark");
        }
        let live = fetch::live(d.to_str().unwrap(), false);
        assert!(fetch::render(&r, &live, fetch::Color::Plain).contains("@secret-host"), "{kind}: without --share the host is on screen");
    }
}

#[test]
fn the_lines_of_a_summary_come_from_the_report() {
    let d = tree("amd");
    let (r, _, _, _) = run(&d, "0.8.5-beta");
    let live = fetch::Live { uptime_s: 93784, mem_used_kb: 8_000_000, disk_used: 120_000_000_000, disk_total: 500_000_000_000, packages: 1500, shell: "bash".into(), session: "Wayland".into(), ..Default::default() };
    let o = fetch::render(&r, &live, fetch::Color::Plain);
    for want in [
        "OS        Axia DE 0.8.5-beta (Debian GNU/Linux 13 (trixie))", "Computer  Micro-Star International Co., Ltd. MS-7C02", "Uptime    1 d 2 h 3 min", "Packages  1500 (rpm)", "Shell     bash · Wayland",
        "Display   DELL U2720Q, 3840 × 2160, 27\"", "CPU       AMD Ryzen 7 5800X 8-Core Processor (16) @ 4.20 GHz", "GPU       Ellesmere [Radeon RX 470/480/570/570X/580/580X/590] (amdgpu)", "Memory    8.2 GB / 34 GB (24 %)",
        "Disk (/)  120 GB / 500 GB (24 %)", "Isolation app firewall · cgroup v2",
    ] {
        assert!(o.contains(want), "{want:?} in\n{o}");
    }
}
