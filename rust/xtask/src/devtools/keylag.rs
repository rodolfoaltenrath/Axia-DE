//! Where does the typing delay come from? (docs/TROUBLESHOOTING.md, "Typing feels slow")
//!
//!   cargo xtask keylag [SECONDS]        (default 30)
//!       Run it as the logged-in user, in the Terminal (keys typed there are never traced).
//!       1. It prints what matters for input latency: version, GPU or CPU composition, virtual
//!          machine, guest tools, the boot flags, and whether Chromium complained about the compositor.
//!       2. If the session was started with the key latency trace (boot menu: add lordos.inputdebug=1
//!          to the kernel line), it asks you to type in the slow app for a few seconds and prints, per
//!          stage, p50/p95/max in ms after the kernel's evdev timestamp: axia-server read it (rx), the
//!          app got it (flush), the app answered (commit) and the frame with it was on screen (shown).
//!          The trace has timings only: no key, character or modifier.
//!       3. It says which stage is slow and what to try.
//!   sudo cargo xtask keylag-inject [COUNT [INTERVAL_MS]]
//!       Types through a virtual uinput keyboard at known times (the latency harness). Needs write
//!       access to /dev/uinput (root). Each press is printed as "inject t=<CLOCK_MONOTONIC ms>" right
//!       after the write; the key is released 60 ms later. Keys cycle over a..z so a text field shows them.

use crate::{at, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

/// p50, p95 and max of the values of a field (`rx=`, `flush=`…) in the trace lines, `-1` left out.
pub fn stats(lines: &[String], field: &str) -> Option<(f64, f64, f64)> {
    let mut values: Vec<f64> = lines
        .iter()
        .filter_map(|line| {
            let at = line.find(&format!(" {field}="))? + field.len() + 2;
            let text: String = line[at..].chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '.')).collect();
            text.parse::<f64>().ok()
        })
        .filter(|v| *v >= 0.0)
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    let pick = |q: f64| values[((n - 1) as f64 * q) as usize];
    Some((pick(0.50), pick(0.95), values[n - 1]))
}

fn cell(s: Option<(f64, f64, f64)>) -> String {
    match s {
        Some((a, b, c)) => format!("{a:7.1} {b:7.1} {c:7.1}"),
        None => "  -      -      -".into(),
    }
}

/// Share of key presses arriving less than 15 ms after the previous one (from the `ev=` times), in %.
pub fn clump_percent(lines: &[String]) -> u32 {
    let times: Vec<f64> = lines
        .iter()
        .filter_map(|l| {
            let at = l.find(" ev=")? + 4;
            l[at..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().parse().ok()
        })
        .collect();
    let close = times.windows(2).filter(|w| w[1] - w[0] < 15.0).count();
    (100 * close / times.len().saturating_sub(1).max(1)) as u32
}

/// The verdict: the stage that is slow, from the p95 of each stage and the clump share.
pub fn verdict(rx: i64, flush: i64, commit: i64, shown: i64, clump: u32) -> Vec<&'static str> {
    if rx > 30 {
        vec!["-> axia-server is slow to read keys (busy main loop). Note what is open and report it with this output."]
    } else if commit - flush > 50 {
        vec!["-> The app itself is slow to answer (commit - flush). Try it with fewer tabs or without the video."]
    } else if shown - commit > 50 {
        vec![
            "-> Frames reach the screen late (shown - commit): compositor presentation. Report it with this output;",
            "   meanwhile boot with lordos.gpu=0 (CPU composition).",
        ]
    } else if shown <= 40 && clump >= 20 {
        vec![
            "-> Inside the guest every stage is fast, but keys arrive in clumps: the delay is before the guest",
            "   kernel, in the hypervisor. On VMware try: 'Accelerate 3D graphics' off, fewer vCPUs, or in the",
            "   .vmx keyboard.vusb.enable = \"TRUE\" (docs/TROUBLESHOOTING.md).",
        ]
    } else if shown <= 40 {
        vec![
            "-> Every stage inside the guest is fast (p95 under 40 ms). If typing still feels slow, the delay is",
            "   before the guest kernel (the hypervisor or the host): see docs/TROUBLESHOOTING.md.",
        ]
    } else {
        vec!["-> Mixed result: keep this output and report it."]
    }
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

pub fn keylag(args: &[String]) -> Result<ExitCode> {
    let seconds: u64 = args.first().map_or(Ok(30), |s| s.parse().map_err(|_| "SECONDS must be a number".to_string()))?;
    let uid = command_text("id", &["-u"]).unwrap_or_default();
    let rt = PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{uid}")));
    let trace = rt.join("axia-keylag.log");
    let logs = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/axia");
    let (slog, alog) = (logs.join("session.log"), logs.join("apps.log"));
    println!("== Axia key latency check");
    let version = command_text("dpkg-query", &["-W", "-f", "${Version}", "lordos"]).unwrap_or_else(|| "?".into());
    let virt = command_text("systemd-detect-virt", &[]).unwrap_or_else(|| "?".into());
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let session = fs::read_to_string(&slog).ok();
    let comp = match &session {
        Some(text) if text.contains("gpu: GPU composition") => {
            let line = text.lines().find(|l| l.contains("gpu: GPU composition")).unwrap_or("");
            let inner = line.split("composition (").nth(1).and_then(|r| r.split(',').next()).unwrap_or("");
            format!("GPU ({inner})")
        }
        Some(text) if text.contains("gpu:") => "CPU".into(),
        _ => "unknown".into(),
    };
    let tools = if command_text("vmtoolsd", &["--version"]).is_some() {
        let active = command_text("systemctl", &["is-active", "open-vm-tools"]).unwrap_or_default();
        let mut text = format!("installed, {active}");
        if let Some(sync) = command_text("vmware-toolbox-cmd", &["timesync", "status"]) {
            text += &format!(", host time sync {sync}");
        }
        text
    } else {
        "not installed".into()
    };
    println!("version:        {version}");
    println!("virtualization: {virt}");
    println!("composition:    {comp}");
    println!("guest tools:    {tools}");
    let flags: Vec<&str> = cmdline.split_whitespace().collect();
    if flags.contains(&"lordos.gpu=0") {
        println!("boot flag:      lordos.gpu=0 (CPU composition forced)");
    }
    if flags.contains(&"lordos.inputdebug=1") {
        println!("boot flag:      lordos.inputdebug=1 (key latency trace)");
    }
    let buggy = fs::read_to_string(&alog).map_or(0, |t| t.lines().filter(|l| l.contains("buggy presentation feedback")).count());
    println!("Chromium:       \"buggy presentation feedback\" {buggy} time(s) in this session's app log\n");

    if !session.as_deref().is_some_and(|t| t.contains("keylag: key latency trace on")) {
        println!("No key latency trace in this session.");
        if buggy > 0 && comp.starts_with("GPU") {
            println!("-> Chromium's warning on the GPU path is the presentation bug fixed in LordOS 0.8.1: a Linux");
            println!("   app's new frame was only shown when something else on the screen changed (up to ~1 s late).");
            println!("   To confirm on this build in 2 minutes: reboot, press 'e' in the boot menu, add lordos.gpu=0");
            println!("   at the end of the 'linux' line, Ctrl+X, then type in Chromium. If the delay is gone, that was it.");
        }
        println!("With LordOS 0.8.1 or later: add lordos.inputdebug=1 the same way (one boot only) and run this again.");
        return Ok(ExitCode::SUCCESS);
    }
    let before = fs::read_to_string(&trace).map_or(0, |t| t.lines().count());
    println!("Click the slow app (Chromium, Firefox...) and type normally for {seconds} seconds, starting now.");
    println!("(The trace stops by itself 120 s after the first traced key; log out and in for a new one.)");
    for left in (1..=seconds).rev() {
        print!("\r{left:3} s ");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("\r       ");
    let Ok(text) = fs::read_to_string(&trace) else {
        println!("Nothing was traced: no key reached a Linux app (keys for Axia's own apps are never traced).");
        return Ok(ExitCode::from(1));
    };
    let run: Vec<String> = text.lines().skip(before).filter(|l| l.starts_with("keylag n=") && l.contains(" press ")).map(String::from).collect();
    if let Some(stop) = text.lines().find(|l| l.starts_with("keylag stop")) {
        println!("note: {stop}");
    }
    if run.len() < 5 {
        println!("Only {} key presses traced: type a bit more and run it again.", run.len());
        return Ok(ExitCode::from(1));
    }
    println!("\n{} key presses (ms after the kernel event)    p50     p95     max", run.len());
    for (field, label) in [
        ("rx", "axia-server read the key        (rx)"),
        ("flush", "the app received it         (flush)"),
        ("commit", "the app drew its answer    (commit)"),
        ("shown", "that frame was on screen    (shown)"),
    ] {
        println!("{label:<44} {}", cell(stats(&run, field)));
    }
    let p95 = |field: &str| stats(&run, field).map_or(0, |s| s.1 as i64);
    let clump = clump_percent(&run);
    println!("\nkey presses arriving less than 15 ms after the previous one: {clump} %\n");
    for line in verdict(p95("rx"), p95("flush"), p95("commit"), p95("shown"), clump) {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

// ---- the uinput harness ----

const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const LETTERS: [u16; 26] = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44];

/// A `struct input_event` of a 64-bit kernel: seconds, microseconds, type, code, value.
pub fn input_event(seconds: i64, micros: i64, kind: u16, code: u16, value: i32) -> [u8; 24] {
    let mut out = [0u8; 24];
    out[0..8].copy_from_slice(&seconds.to_ne_bytes());
    out[8..16].copy_from_slice(&micros.to_ne_bytes());
    out[16..18].copy_from_slice(&kind.to_ne_bytes());
    out[18..20].copy_from_slice(&code.to_ne_bytes());
    out[20..24].copy_from_slice(&value.to_ne_bytes());
    out
}

/// `struct uinput_user_dev`: name[80], input_id (bus, vendor, product, version), ff_effects_max and
/// the four abs arrays of 64 entries.
pub fn user_dev(name: &str) -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out[..name.len()].copy_from_slice(name.as_bytes());
    for v in [3u16, 0x1234, 0x5678, 1] {
        out.extend(v.to_ne_bytes());
    }
    out.extend(0i32.to_ne_bytes());
    out.resize(out.len() + 4 * 64 * 4, 0);
    out
}

pub fn keylag_inject(args: &[String]) -> Result<ExitCode> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let count: usize = args.first().map_or(Ok(100), |s| s.parse().map_err(|_| "COUNT must be a number".to_string()))?;
    let interval: f64 = args.get(1).map_or(Ok(180.0), |s| s.parse().map_err(|_| "INTERVAL_MS must be a number".to_string()))?;
    let mut device = at(Path::new("/dev/uinput"), fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open("/dev/uinput"))?;
    let fd = device.as_raw_fd();
    let ioctl = |request: libc::c_ulong, value: libc::c_int| {
        // SAFETY: a plain uinput request on a descriptor we own.
        if unsafe { libc::ioctl(fd, request, value) } < 0 { Err(std::io::Error::last_os_error().to_string()) } else { Ok(()) }
    };
    ioctl(UI_SET_EVBIT, 1)?;
    for code in 1..128 {
        ioctl(UI_SET_KEYBIT, code)?;
    }
    device.write_all(&user_dev("keylag virtual keyboard")).map_err(|e| e.to_string())?;
    ioctl(UI_DEV_CREATE, 0)?;
    std::thread::sleep(Duration::from_secs(2)); // the session sees it (inotify) and lends it to the session
    let mut event = |kind: u16, code: u16, value: i32| -> Result<()> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
        device.write_all(&input_event(now.as_secs() as i64, i64::from(now.subsec_micros()), kind, code, value)).map_err(|e| e.to_string())
    };
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    let mut state = u64::from(seed) | 1;
    let result = (|| -> Result<()> {
        for i in 0..count {
            let code = LETTERS[i % LETTERS.len()];
            event(1, code, 1)?;
            event(0, 0, 0)?;
            let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            // SAFETY: `ts` is a valid timespec.
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
            println!("inject t={:.3}", ts.tv_sec as f64 * 1e3 + ts.tv_nsec as f64 / 1e6);
            std::thread::sleep(Duration::from_millis(60));
            event(1, code, 0)?;
            event(0, 0, 0)?;
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let jitter = (state % 8001) as f64 / 100.0 - 40.0; // -40..+40 ms
            std::thread::sleep(Duration::from_secs_f64(((interval + jitter) / 1e3 - 0.06).max(0.0)));
        }
        Ok(())
    })();
    std::thread::sleep(Duration::from_millis(500));
    ioctl(UI_DEV_DESTROY, 0)?;
    result.map(|()| ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[(f64, f64, f64)]) -> Vec<String> {
        values.iter().enumerate().map(|(i, (rx, ev, shown))| format!("keylag n={i} press rx={rx} flush=2.0 commit=3.0 shown={shown} ev={ev} x")).collect()
    }

    #[test]
    fn stats_pick_the_nearest_rank_and_skip_negatives() {
        let l = lines(&[(1.0, 0.0, 10.0), (2.0, 100.0, 20.0), (-1.0, 200.0, 30.0), (4.0, 300.0, 40.0), (3.0, 400.0, 50.0)]);
        assert_eq!(stats(&l, "rx"), Some((2.0, 3.0, 4.0))); // 1,2,3,4 → p50 index 1, p95 index 2
        assert_eq!(stats(&l, "shown"), Some((30.0, 40.0, 50.0)));
        assert_eq!(stats(&l, "nope"), None);
        assert_eq!(cell(None), "  -      -      -");
        assert_eq!(cell(Some((1.0, 2.5, 3.0))), "    1.0     2.5     3.0");
    }

    #[test]
    fn keys_in_clumps_are_counted_between_neighbours() {
        let l = lines(&[(1.0, 0.0, 1.0), (1.0, 5.0, 1.0), (1.0, 10.0, 1.0), (1.0, 100.0, 1.0)]);
        assert_eq!(clump_percent(&l), 66);
        assert_eq!(clump_percent(&[]), 0);
    }

    #[test]
    fn the_verdict_names_the_slow_stage() {
        assert!(verdict(40, 1, 1, 1, 0)[0].contains("slow to read keys"));
        assert!(verdict(1, 10, 80, 90, 0)[0].contains("slow to answer"));
        assert!(verdict(1, 10, 20, 90, 0)[0].contains("Frames reach the screen late"));
        assert!(verdict(1, 10, 20, 30, 50)[0].contains("arrive in clumps"));
        assert!(verdict(1, 10, 20, 30, 0)[0].contains("Every stage inside the guest is fast"));
        assert!(verdict(1, 10, 20, 60, 0)[0].contains("Mixed result"));
    }

    #[test]
    fn the_uinput_structures_have_the_kernels_sizes_and_fields() {
        let event = input_event(10, 20, 1, 30, 1);
        assert_eq!(event.len(), 24);
        assert_eq!(i64::from_ne_bytes(event[..8].try_into().unwrap()), 10);
        assert_eq!((u16::from_ne_bytes(event[16..18].try_into().unwrap()), u16::from_ne_bytes(event[18..20].try_into().unwrap())), (1, 30));
        let dev = user_dev("keylag virtual keyboard");
        assert_eq!(dev.len(), 1116);
        assert_eq!(&dev[..7], b"keylag ");
        assert_eq!(u16::from_ne_bytes(dev[80..82].try_into().unwrap()), 3);
    }
}
