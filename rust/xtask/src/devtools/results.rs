//! The evidence for one machine and one Axia version, written to results/<version>/<machine>/
//! (docs/RESULTS.md). Run it as the user, inside a session (the Test Bench is a Wayland client):
//!
//!   cargo xtask collect-results MACHINE [--no-memory] [--no-lordbench] [--bench-seconds N] [--version V]
//!
//! MACHINE is a short name, [a-z0-9-] (e.g. vm116-headless, pi5-8gb-hdmi). It writes:
//!   sysreport.json   axia-sysreport report --json (no serial numbers, addresses or names)
//!   testbench.json   axia-testbench --gl --auto N (the Wayland checklist, frame times, 3D renderer)
//!   memory.txt       whole system (`free -m`) and per process (PSS from /proc/<pid>/smaps_rollup); with
//!                    sudo, root's processes too
//!   boot.txt         systemd-analyze, and the time to the desktop's first frame from the kernel's start
//!   boot-times.jsonl every recorded boot on this machine: seconds to the first frame, version, kernel
//!   lordbench.json   axia-sysreport bench --full --json (LordBench, about 5 minutes; --no-lordbench skips it)
//!   README.md        when, what version, the conditions to fill in, and these exact commands
//! The version is the installed one (software.lordos in the report); --version names it for a
//! development build, e.g. 0.8.5-dev. Nothing is sent anywhere.

use crate::{at, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

pub struct Options {
    pub machine: String,
    pub memory: bool,
    pub lordbench: bool,
    pub seconds: u64,
    pub version: Option<String>,
}

pub fn parse_args(args: &[String]) -> std::result::Result<Options, String> {
    let usage = "usage: collect-results <machine [a-z0-9-]> [--no-memory] [--no-lordbench] [--bench-seconds N] [--version V]";
    let machine = args.first().cloned().unwrap_or_default();
    if machine.is_empty() || !machine.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(usage.into());
    }
    let mut options = Options { machine, memory: true, lordbench: true, seconds: 30, version: None };
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--no-memory" => options.memory = false,
            "--no-lordbench" => options.lordbench = false,
            "--bench-seconds" => {
                let value = rest.next().ok_or(usage)?;
                options.seconds = value.parse().map_err(|_| "--bench-seconds takes a number".to_string())?;
            }
            "--version" => options.version = Some(rest.next().ok_or(usage)?.clone()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// The version as a folder name: plain characters only.
pub fn clean_version(v: &str) -> String {
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+~-".contains(&b)) {
        "unknown".into()
    } else {
        v.to_string()
    }
}

/// `printf "%8.1f"`
fn mb(kib: f64) -> String {
    format!("{:8.1}", kib / 1024.0)
}

/// The PSS list: one line per process, largest first, and the total. `processes` are (KiB, comm).
pub fn pss_lines(mut processes: Vec<(u64, String)>) -> Vec<String> {
    // `sort -rn` on "KiB comm": by number, then the rest of the line, both reversed
    processes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| format!("{} {}", b.0, b.1).cmp(&format!("{} {}", a.0, a.1))));
    let total: u64 = processes.iter().map(|p| p.0).sum();
    let mut lines: Vec<String> = processes
        .iter()
        .map(|(k, comm)| format!("{} {}", mb(*k as f64), format!(" {}", comm.split_whitespace().collect::<Vec<_>>().join(" "))))
        .collect();
    lines.push(format!("{}  total of the processes listed", mb(total as f64)));
    lines
}

fn processes() -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(rollup) = fs::read_to_string(entry.path().join("smaps_rollup")) else { continue };
        let Some(kib) = rollup.lines().find_map(|l| l.strip_prefix("Pss:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<u64>().ok()) else { continue };
        let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default().trim_end_matches('\n').to_string();
        out.push((kib, comm));
    }
    out
}

fn available(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// journalctl, through `sudo -n` when that works without a password (all entries if allowed).
fn journal(args: &[&str]) -> String {
    let sudo = Command::new("sudo").args(["-n", "true"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    let out = if sudo { Command::new("sudo").arg("-n").arg("journalctl").args(args).output() } else { Command::new("journalctl").args(args).output() };
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// Every `first frame on the monitor N s after the kernel started` of the journal (grep -o).
pub fn first_frames(log: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in log.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("first frame on the monitor ") {
            let tail = &rest[i + "first frame on the monitor ".len()..];
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if !digits.is_empty() && tail[digits.len()..].starts_with(" s after the kernel started") {
                out.push(format!("first frame on the monitor {digits} s after the kernel started"));
            }
            rest = &rest[i + 1..];
        }
    }
    out
}

/// The kernel's first line and the splash's, without the host name (the results are published).
pub fn boot_marks(log: &str) -> Vec<String> {
    let (mut kernel, mut splash) = (false, false);
    let mut out = Vec::new();
    for line in log.lines() {
        let take = if line.contains("kernel: Linux version") && !kernel {
            kernel = true;
            true
        } else if line.contains("Started plymouth-start.service") && !splash {
            splash = true;
            true
        } else {
            false
        };
        if take {
            // s/^(\[[^]]*\]) [^ ]+ /\1 /
            out.push(match line.strip_prefix('[').and_then(|r| r.find(']').map(|i| (i, r))) {
                Some((i, r)) => {
                    let (stamp, rest) = (&r[..i], &r[i + 1..]);
                    match rest.strip_prefix(' ').and_then(|r| r.split_once(' ')) {
                        Some((_host, tail)) => format!("[{stamp}] {tail}"),
                        None => line.to_string(),
                    }
                }
                None => line.to_string(),
            });
        }
    }
    out
}

fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}Z", rem / 3600, rem % 3600 / 60)
}

pub fn readme(machine: &str, version: &str, seconds: u64, report_version: &str, now: &str) -> String {
    format!(
        "# {machine} — Axia {version}\n\nCollected {now} with `cargo xtask collect-results {machine}` (docs/RESULTS.md); the report's\nown version field says `{report_version}`.\n\n**Conditions** (fill in): hardware or VM and its resources; monitor (model, mode) or headless; how long\nthe system was idle before measuring; which apps were open; power source and profile.\n\nReproduce:\n\n```sh\naxia-sysreport report --json > sysreport.json\naxia-testbench --gl --auto {seconds} > testbench.json      # inside an Axia session\nfree -m                                                  # whole system\nsudo sh -c 'for f in /proc/[0-9]*/smaps_rollup; do ...'  # PSS per process: see xtask/src/devtools/results.rs\nsystemd-analyze\naxia-sysreport bench --full --json > lordbench.json\n```\n"
    )
}

fn with_timeout(command: &mut Command, seconds: u64) -> Result<Option<Vec<u8>>> {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut data);
        data
    });
    let ok = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status.success(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let data = reader.join().unwrap_or_default();
    Ok(ok.then_some(data))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let options = match parse_args(args) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("{message}");
            return Ok(ExitCode::from(2));
        }
    };
    if !available("axia-sysreport") {
        eprintln!("axia-sysreport not installed");
        return Ok(ExitCode::from(1));
    }
    let report = output("axia-sysreport", &["report", "--json"]).unwrap_or_default();
    let reported = serde_json::from_str::<serde_json::Value>(&report).ok().and_then(|v| v["software"]["lordos"].as_str().map(String::from));
    let version = clean_version(&options.version.clone().or(reported.clone()).unwrap_or_else(|| "unknown".into()));
    let root: PathBuf = if repo.join("rust/xtask").is_dir() { repo.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())? };
    let dir = root.join("results").join(&version).join(&options.machine);
    at(&dir, fs::create_dir_all(&dir))?;
    let put = |name: &str, data: &[u8]| at(&dir.join(name), fs::write(dir.join(name), data));
    put("sysreport.json", output("axia-sysreport", &["report", "--json"]).unwrap_or_default().as_bytes())?;

    if std::env::var_os("WAYLAND_DISPLAY").is_some() && available("axia-testbench") {
        let mut command = Command::new("axia-testbench");
        command.args(["--gl", "--auto", &options.seconds.to_string()]);
        match with_timeout(&mut command, options.seconds + 30)? {
            Some(data) => put("testbench.json", &data)?,
            None => {
                put("testbench.json", b"")?;
                eprintln!("the Test Bench ended with an error");
            }
        }
    } else {
        eprintln!("no Wayland session (WAYLAND_DISPLAY) or axia-testbench missing: testbench.json skipped");
    }

    if options.memory {
        let root_user = fs::read_to_string("/proc/self/status").is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("0")));
        let mut text = String::from("# free -m (whole system; 'used' excludes the reclaimable page cache)\n");
        text += &output("free", &["-m"]).unwrap_or_default();
        text += "\n# PSS per process, MB (shared pages divided among the processes that map them)\n";
        text += &format!("# {}\n", if root_user { "all processes" } else { "only processes this user can read; run with sudo for all" });
        text += &pss_lines(processes()).join("\n");
        text += "\n";
        put("memory.txt", text.as_bytes())?;
    }

    let mut boot = output("systemd-analyze", &[]).unwrap_or_default();
    boot += "\n# the desktop's first frame, from the kernel's start (lord-sessiond); the splash's own timer starts later\n";
    let frames = first_frames(&journal(&["-b", "-u", "lordos", "--no-pager"]));
    if frames.is_empty() {
        boot += "(not in the journal: run with sudo, or as a member of the adm or systemd-journal group)\n";
    } else {
        boot += &(frames.join("\n") + "\n");
    }
    boot += "# the boot splash started: subtract the first line's monotonic time from the second's\n";
    let marks = boot_marks(&journal(&["-b", "-o", "short-monotonic", "--no-pager"]));
    if !marks.is_empty() {
        boot += &(marks.join("\n") + "\n");
    }
    put("boot.txt", boot.as_bytes())?;
    // every boot's time to the first frame on this machine, with its version (lord-sessiond; no names)
    if let Ok(times) = fs::read("/var/lib/lordos/boot-times.jsonl") {
        put("boot-times.jsonl", &times)?;
    }

    // last: its load would disturb the memory and frame times above
    if options.lordbench {
        match output("axia-sysreport", &["bench", "--full", "--json"]) {
            Some(json) => put("lordbench.json", json.as_bytes())?,
            None => eprintln!("LordBench ended with an error: lordbench.json skipped"),
        }
    }
    let written = fs::read_to_string(dir.join("sysreport.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v["software"]["lordos"].as_str().map(String::from)).unwrap_or_default();
    put("README.md", readme(&options.machine, &version, options.seconds, &written, &timestamp()).as_bytes())?;
    println!("written to {}", dir.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_follow_the_old_script() {
        let o = parse_args(&strings(&["vm-1", "--no-memory", "--bench-seconds", "12", "--version", "0.9-dev", "--no-lordbench"])).unwrap();
        assert_eq!((o.machine.as_str(), o.memory, o.lordbench, o.seconds, o.version.as_deref()), ("vm-1", false, false, 12, Some("0.9-dev")));
        for bad in [vec![], vec!["Bad_Name"], vec!["ok", "--wat"], vec!["ok", "--bench-seconds", "x"], vec!["ok", "--bench-seconds"]] {
            assert!(parse_args(&strings(&bad)).is_err(), "{bad:?}");
        }
        assert_eq!((clean_version("0.8.5~rc1-beta"), clean_version("a b"), clean_version("")), ("0.8.5~rc1-beta".into(), "unknown".into(), "unknown".into()));
    }

    #[test]
    fn the_pss_list_is_sorted_with_awks_spacing_and_a_total() {
        let lines = pss_lines(vec![(2048, "small".into()), (10240, "Web   Content".into()), (2048, "alpha".into())]);
        assert_eq!(lines, ["    10.0  Web Content", "     2.0  small", "     2.0  alpha", "    14.0  total of the processes listed"]);
        assert_eq!(pss_lines(vec![]), ["     0.0  total of the processes listed"]);
    }

    #[test]
    fn the_journal_lines_are_picked_and_the_host_name_dropped() {
        let log = "[    0.000000] host kernel: Linux version 6.1\n[    2.5] host systemd[1]: Started plymouth-start.service - x\n[    9.0] host kernel: Linux version again\n[ 11.0] host systemd[1]: Started plymouth-start.service - y\n";
        assert_eq!(boot_marks(log), ["[    0.000000] kernel: Linux version 6.1", "[    2.5] systemd[1]: Started plymouth-start.service - x"]);
        let journal = "x lordos: first frame on the monitor 4.25 s after the kernel started\nnoise\nfirst frame on the monitor 9 s after the kernel started and first frame on the monitor z s after the kernel started";
        assert_eq!(first_frames(journal), ["first frame on the monitor 4.25 s after the kernel started", "first frame on the monitor 9 s after the kernel started"]);
    }

    #[test]
    fn the_readme_names_the_machine_the_commands_and_the_time() {
        let text = readme("pi5", "0.9.0", 30, "0.9.0", "2026-11-22T21:05Z");
        assert!(text.starts_with("# pi5 — Axia 0.9.0\n\nCollected 2026-11-22T21:05Z with `cargo xtask collect-results pi5`"));
        assert!(text.contains("axia-testbench --gl --auto 30 > testbench.json") && text.ends_with("```\n"));
        assert_eq!(timestamp().len(), 17);
    }
}
