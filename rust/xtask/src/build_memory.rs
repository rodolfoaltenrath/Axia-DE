//! Build telemetry: sampled RSS, not an exact high-water mark or unique memory.
use crate::{at, options, Result};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, PartialEq)]
struct Process {
    parent: u32,
    rss: u64,
    name: String,
}

fn parse_stat(text: &str, page_size: u64) -> Option<Process> {
    // comm can contain spaces and parentheses; the final ')' terminates it.
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    if end <= start {
        return None;
    }
    let fields: Vec<_> = text.get(end + 1..)?.split_whitespace().collect();
    Some(Process {
        parent: fields.get(1)?.parse().ok()?,
        rss: fields
            .get(21)?
            .parse::<u64>()
            .ok()?
            .checked_mul(page_size)?,
        name: text.get(start + 1..end)?.to_owned(),
    })
}

fn sample(root: u32, table: &HashMap<u32, Process>) -> (u64, u64) {
    let mut descendants = HashSet::from([root]);
    loop {
        let before = descendants.len();
        for (&pid, process) in table {
            if descendants.contains(&process.parent) {
                descendants.insert(pid);
            }
        }
        if before == descendants.len() {
            break;
        }
    }
    let mut total: u64 = 0;
    let mut compiler = 0;
    for pid in descendants {
        if let Some(process) = table.get(&pid) {
            total = total.saturating_add(process.rss);
            if process.name == "rustc" {
                compiler = compiler.max(process.rss);
            }
        }
    }
    (total, compiler)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        io,
        os::unix::process::{CommandExt, ExitStatusExt},
        process::{Child, Command},
        sync::atomic::{AtomicI32, Ordering},
        thread,
    };

    static INTERRUPTED: AtomicI32 = AtomicI32::new(0);
    extern "C" fn interrupted(signal: i32) {
        INTERRUPTED.store(signal, Ordering::Relaxed);
    }

    struct Signals(Vec<(i32, libc::sigaction)>);
    impl Signals {
        fn new() -> Result<Self> {
            INTERRUPTED.store(0, Ordering::Relaxed);
            let mut signals = Self(vec![]);
            for signal in [libc::SIGINT, libc::SIGTERM] {
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = interrupted as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }
                let mut previous = std::mem::MaybeUninit::uninit();
                if unsafe { libc::sigaction(signal, &action, previous.as_mut_ptr()) } != 0 {
                    return Err(io::Error::last_os_error().to_string());
                }
                signals.0.push((signal, unsafe { previous.assume_init() }));
            }
            Ok(signals)
        }
    }
    impl Drop for Signals {
        fn drop(&mut self) {
            for (signal, previous) in &self.0 {
                unsafe {
                    libc::sigaction(*signal, previous, std::ptr::null_mut());
                }
            }
        }
    }

    struct Build(Child, bool);
    impl Build {
        fn stop(&mut self) {
            if self.1 {
                return;
            }
            self.1 = true;
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGTERM);
            }
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(3) {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
    impl Drop for Build {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn processes(page_size: u64) -> Result<HashMap<u32, Process>> {
        let proc = Path::new("/proc");
        let mut table = HashMap::new();
        for entry in at(proc, fs::read_dir(proc))? {
            let Ok(entry) = entry else { continue };
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            // Processes may disappear or deny access while the snapshot is read.
            let Ok(text) = fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            if let Some(process) = parse_stat(&text, page_size) {
                table.insert(pid, process);
            }
        }
        Ok(table)
    }

    pub(super) fn collect(command: &[String], report: &mut serde_json::Value) -> Result<u8> {
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if size <= 0 {
            return Err("cannot determine Linux page size".into());
        }
        let _signals = Signals::new()?;
        let child = Command::new(&command[0])
            .args(&command[1..])
            .process_group(0)
            .spawn()
            .map_err(|e| format!("{}: {e}", command[0]))?;
        let mut build = Build(child, false);
        loop {
            let (tree, compiler) = sample(build.0.id(), &processes(size as u64)?);
            let peak = |key: &str, value| report[key].as_u64().unwrap_or(0).max(value);
            let peak_tree = peak("peak_process_tree_rss_bytes", tree);
            let peak_compiler = peak("peak_single_rustc_rss_bytes", compiler);
            report["peak_process_tree_rss_bytes"] = json!(peak_tree);
            report["peak_single_rustc_rss_bytes"] = json!(peak_compiler);
            let signal = INTERRUPTED.load(Ordering::Relaxed);
            if signal != 0 {
                build.stop();
                report["interrupted_signal"] = json!(signal);
                return Ok((128 + signal) as u8);
            }
            if let Some(status) = build.0.try_wait().map_err(|e| e.to_string())? {
                return Ok(status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
                    as u8);
            }
            thread::sleep(INTERVAL);
        }
    }
}

pub fn execute(args: &[String]) -> Result<u8> {
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or("build-memory requires -- before the build command")?;
    let values = options(&args[..separator], &["output"])?;
    let output = values
        .get("output")
        .ok_or("build-memory requires --output")?;
    let command = &args[separator + 1..];
    if command.is_empty() {
        return Err("a build command is required after --".into());
    }
    let output = Path::new(output);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    at(parent, fs::create_dir_all(parent))?;
    let start = Instant::now();
    let mut report = json!({"command":command,"exit_code":null,
        "sample_interval_seconds":INTERVAL.as_secs_f64(),
        "peak_process_tree_rss_bytes":0,"peak_single_rustc_rss_bytes":0,
        "note":"Sampled RSS; shared pages may be counted in multiple processes. This is not total runner memory or an exact high-water mark."});
    #[cfg(target_os = "linux")]
    let result = linux::collect(command, &mut report);
    #[cfg(not(target_os = "linux"))]
    let result: Result<u8> = Err("build-memory requires Linux with /proc".into());
    report["elapsed_seconds"] = json!((start.elapsed().as_secs_f64() * 100.).round() / 100.);
    match &result {
        Ok(code) => report["exit_code"] = json!(code),
        Err(error) => report["error"] = json!(error),
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    at(output, fs::write(output, format!("{text}\n")))?;
    println!("{text}");
    if report["peak_process_tree_rss_bytes"].as_u64().unwrap_or(0) > 11 * 1024 * 1024 * 1024 {
        println!("::warning::Build process RSS exceeded 11 GiB; review memory growth.");
    }
    if report["peak_single_rustc_rss_bytes"].as_u64().unwrap_or(0) > 5 * 1024 * 1024 * 1024 {
        println!("::warning::A rustc process exceeded 5 GiB; consider another crate split.");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn process(parent: u32, rss: u64, name: &str) -> Process {
        Process {
            parent,
            rss,
            name: name.into(),
        }
    }
    #[test]
    fn only_descendants_contribute_to_build_and_compiler_peaks() {
        let table = HashMap::from([
            (10, process(1, 100, "make")),
            (20, process(10, 200, "cargo")),
            (30, process(20, 300, "rustc")),
            (31, process(20, 400, "rustc")),
            (40, process(1, 9000, "rustc")),
        ]);
        assert_eq!(sample(10, &table), (1000, 400));
        assert_eq!(sample(99, &table), (0, 0));
    }
    #[test]
    fn cycles_and_missing_parents_do_not_add_unrelated_processes() {
        let table = HashMap::from([
            (10, process(1, 100, "make")),
            (20, process(21, 200, "rustc")),
            (21, process(20, 200, "rustc")),
        ]);
        assert_eq!(sample(10, &table), (100, 0));
    }
    fn stat(name: &str, rss: &str) -> String {
        let mut fields = vec!["0"; 22];
        fields[0] = "S";
        fields[1] = "10";
        fields[21] = rss;
        format!("20 ({name}) {}", fields.join(" "))
    }
    #[test]
    fn proc_stat_handles_spaces_parentheses_and_nonstandard_page_sizes() {
        assert_eq!(
            parse_stat(&stat("worker (test) )", "7"), 16384),
            Some(process(10, 114688, "worker (test) )"))
        );
    }
    #[test]
    fn malformed_and_overflowing_proc_records_are_ignored() {
        for text in [
            "",
            "20 (rustc)",
            &stat("rustc", "-1"),
            &stat("rustc", "18446744073709551615"),
        ] {
            assert_eq!(parse_stat(text, 4096), None);
        }
    }
    #[test]
    fn command_separator_and_program_are_required() {
        for args in [
            vec!["--output", "unused"],
            vec!["--output", "unused", "--"],
            vec!["--", "true"],
        ] {
            assert!(execute(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
        }
    }
}
