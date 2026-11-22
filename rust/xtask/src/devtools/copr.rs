//! Submits one checked source RPM to Copr, then tolerates temporary failures of read-only polling.
//!
//!   cargo xtask copr-publish [--packages DIR] [--report FILE] [--build-id N]
//!
//! The environment brings `COPR_PROJECT` (OWNER/PROJECT) and `COPR_CONFIG` (the INI of copr-cli with
//! the credentials). The upload is one `copr-cli build --nowait` run that is never retried: an
//! interrupted response could hide an accepted build. The accepted build id is recorded in the report
//! before the polling starts, and the polling (plain GETs of the public API, through curl) survives
//! short outages with a growing wait. `--build-id` only watches an existing build, with no
//! credentials and no upload, to resume after an outage.

use crate::{at, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const COPR_URL: &str = "https://copr.fedorainfracloud.org";
const FAILED: [&str; 3] = ["failed", "canceled", "cancelled"];

/// Why one status request did not give a state.
#[derive(Debug)]
pub enum Fetch {
    /// The server answered with this HTTP status.
    Http(u16),
    /// No answer, a timeout, or an answer that is not the expected JSON.
    Transient,
}

/// `%g` formatting of a wait in seconds.
fn seconds(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn shown(state: &Value) -> String {
    match state {
        Value::String(s) => format!("'{s}'"),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

pub struct Timing<'a> {
    pub sleep: &'a mut dyn FnMut(f64),
    pub now: &'a mut dyn FnMut() -> f64,
    pub timeout: f64,
    pub max_errors: u32,
}

pub fn watch(
    build_id: u64,
    base_url: &str,
    fetch: &mut dyn FnMut(&str, u64, f64) -> std::result::Result<Value, Fetch>,
    timing: Timing,
) -> Result<()> {
    let Timing { sleep, now, timeout, max_errors } = timing;
    let deadline = now() + timeout;
    let mut errors = 0u32;
    while now() < deadline {
        let delay;
        match fetch(base_url, build_id, (deadline - now()).min(30.0)) {
            Err(error) => {
                if let Fetch::Http(code) = error {
                    if code != 408 && code != 429 && code < 500 {
                        return Err(format!("Copr status request failed: HTTP {code}"));
                    }
                }
                errors += 1;
                if errors >= max_errors {
                    return Err(format!("Copr status unavailable after {errors} consecutive errors; resume with --build-id {build_id}"));
                }
                delay = (15.0 * 2f64.powi(errors as i32 - 1)).min(120.0).min((deadline - now()).max(0.0));
                println!("Copr build {build_id}: status temporarily unavailable; retrying in {}s", seconds(delay));
            }
            Ok(state) => {
                errors = 0;
                let text = state.as_str().unwrap_or("");
                println!("Copr build {build_id}: {}", if state.is_string() { text.to_string() } else { shown(&state) });
                if text == "succeeded" || text == "skipped" {
                    return Ok(());
                }
                if FAILED.contains(&text) {
                    return Err(format!("Copr build {build_id} {text}"));
                }
                if !state.is_string() || text.is_empty() || text == "unknown" {
                    return Err(format!("Copr build {build_id}: unexpected state {}", shown(&state)));
                }
                delay = (deadline - now()).max(0.0).min(30.0);
            }
        }
        sleep(delay);
    }
    Err(format!("Timed out waiting for Copr build {build_id}; resume with --build-id {build_id}"))
}

/// One GET of the build's status through curl.
fn curl_state(base_url: &str, build_id: u64, timeout: f64) -> std::result::Result<Value, Fetch> {
    let url = format!("{base_url}/api_3/build/{build_id}");
    let output = Command::new("curl")
        .args(["-sS", "--max-time", &format!("{:.0}", timeout.max(1.0)), "-w", "\n%{http_code}", &url])
        .output()
        .map_err(|_| Fetch::Transient)?;
    if !output.status.success() {
        return Err(Fetch::Transient);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, code) = text.rsplit_once('\n').ok_or(Fetch::Transient)?;
    let code: u16 = code.trim().parse().map_err(|_| Fetch::Transient)?;
    if code != 200 {
        return Err(Fetch::Http(code));
    }
    let json: Value = serde_json::from_str(body).map_err(|_| Fetch::Transient)?;
    json.get("state").cloned().ok_or(Fetch::Transient)
}

fn watch_for_real(build_id: u64, base_url: &str) -> Result<()> {
    let start = Instant::now();
    let mut sleep = |s: f64| std::thread::sleep(Duration::from_secs_f64(s));
    let mut now = || start.elapsed().as_secs_f64();
    watch(build_id, base_url, &mut curl_state, Timing { sleep: &mut sleep, now: &mut now, timeout: 7200.0, max_errors: 8 })
}

pub fn record_build(build_id: u64, base_url: &str, report: &Path) -> Result<()> {
    let url = format!("{base_url}/coprs/build/{build_id}");
    println!("Copr build: {url}");
    if let Some(parent) = report.parent().filter(|p| !p.as_os_str().is_empty()) {
        at(parent, fs::create_dir_all(parent))?;
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({"build_id": build_id, "url": url})).map_err(|e| e.to_string())?;
    at(report, fs::write(report, format!("{text}\n")))?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let mut file = at(Path::new(&summary), fs::OpenOptions::new().append(true).create(true).open(&summary))?;
        at(Path::new(&summary), write!(file, "\nCopr build: [{build_id}]({url})\n"))?;
    }
    Ok(())
}

/// Uploads once (never retried), records the accepted build, then waits for it.
pub fn publish(
    upload: &mut dyn FnMut() -> Result<u64>,
    base_url: &str,
    report: &Path,
    wait: &mut dyn FnMut(u64, &str) -> Result<()>,
) -> Result<()> {
    let build_id = upload()?;
    record_build(build_id, base_url, report)?;
    wait(build_id, base_url)
}

/// The sections of an INI file, with lower-case keys: `[section] key = value` or `key: value`.
pub fn parse_ini(text: &str) -> Option<HashMap<String, HashMap<String, String>>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('[') {
            let name = name.strip_suffix(']')?;
            out.entry(name.to_string()).or_default();
            section = Some(name.to_string());
            continue;
        }
        let at = trimmed.find(['=', ':'])?;
        let (key, value) = (trimmed[..at].trim().to_lowercase(), trimmed[at + 1..].trim().to_string());
        out.get_mut(section.as_ref()?)?.insert(key, value);
    }
    Some(out)
}

fn valid_project(project: &str) -> bool {
    let Some((owner, name)) = project.split_once('/') else { return false };
    let owner = owner.strip_prefix('@').unwrap_or(owner);
    !owner.is_empty()
        && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

/// The build id of `copr-cli build --nowait` ("Created builds: 123").
fn created_build(output: &str) -> Option<u64> {
    let rest = &output[output.find("Created builds:")? + "Created builds:".len()..];
    rest.split_whitespace().next()?.parse().ok()
}

fn find_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in super::entries(dir) {
        let path = entry.path();
        if path.is_dir() {
            find_sources(&path, out);
        } else if path.to_string_lossy().ends_with(".src.rpm") {
            out.push(path);
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode> {
    let (mut packages, mut report, mut build_id) = (PathBuf::from("packages"), PathBuf::from("build/copr/build.json"), None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| rest.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--packages" => packages = PathBuf::from(value("--packages")?),
            "--report" => report = PathBuf::from(value("--report")?),
            "--build-id" => build_id = Some(value("--build-id")?.parse::<i64>().map_err(|_| "--build-id must be a whole number".to_string())?),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    if let Some(id) = build_id {
        if id <= 0 {
            return Err("--build-id must be positive".into());
        }
        record_build(id as u64, COPR_URL, &report)?;
        watch_for_real(id as u64, COPR_URL)?;
        return Ok(ExitCode::SUCCESS);
    }
    let project = std::env::var("COPR_PROJECT").unwrap_or_default();
    if !valid_project(&project) {
        return Err("COPR_PROJECT must be OWNER/PROJECT".into());
    }
    let config = std::env::var("COPR_CONFIG").unwrap_or_default();
    let parsed = parse_ini(&config).ok_or("COPR_CONFIG must be a valid INI file")?;
    let cli = parsed.get("copr-cli");
    if cli.is_none_or(|s| ["login", "username", "token"].iter().any(|k| s.get(*k).is_none_or(String::is_empty))) {
        return Err("COPR_CONFIG is missing Copr credentials".into());
    }
    let base_url = cli.and_then(|s| s.get("copr_url")).map_or(COPR_URL, String::as_str).trim_end_matches('/').to_string();
    let mut sources = Vec::new();
    find_sources(&packages, &mut sources);
    if sources.len() != 1 {
        return Err("Expected exactly one checked source RPM".into());
    }
    let dir = std::env::temp_dir().join(format!("axia-copr-{}", std::process::id()));
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let _ = fs::remove_dir_all(&dir);
        at(&dir, fs::DirBuilder::new().mode(0o700).create(&dir))?;
        let path = dir.join("copr");
        let mut file = at(&path, fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path))?;
        std::io::Write::write_all(&mut file, config.as_bytes()).map_err(|e| e.to_string())?;
    }
    let config_file = dir.join("copr");
    let mut upload = || -> Result<u64> {
        let output = Command::new("copr-cli")
            .env_remove("COPR_CONFIG")
            .arg("--config")
            .arg(&config_file)
            .args(["build", "--nowait", &project])
            .arg(&sources[0])
            .output()
            .map_err(|e| format!("copr-cli: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        // the output can carry server-supplied text and the status is all that is reported
        if !output.status.success() {
            return Err(format!("copr-cli build failed ({})", output.status));
        }
        created_build(&text).ok_or_else(|| "copr-cli did not report a build id".to_string())
    };
    let result = publish(&mut upload, &base_url, &report, &mut |id, base| watch_for_real(id, base));
    let _ = fs::remove_dir_all(&dir);
    result.map(|()| ExitCode::SUCCESS)
}

pub fn execute(_repo: &Path, args: &[String]) -> Result<ExitCode> {
    // API errors can contain server-supplied text; nothing but our own messages is printed.
    run(args).map_err(|message| format!("Copr publication failed: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;
    use std::cell::{Cell, RefCell};

    struct Clock {
        time: Cell<f64>,
        delays: RefCell<Vec<f64>>,
    }

    fn clock() -> Clock {
        Clock { time: Cell::new(0.0), delays: RefCell::new(Vec::new()) }
    }

    fn run_watch(responses: Vec<std::result::Result<Value, Fetch>>, timeout: f64, clock: &Clock) -> (Result<()>, usize) {
        let mut queue = responses.into_iter();
        let mut calls = 0;
        let mut fetch = |_: &str, _: u64, _: f64| {
            calls += 1;
            queue.next().expect("more requests than the test expected")
        };
        let mut sleep = |s: f64| {
            clock.time.set(clock.time.get() + s);
            clock.delays.borrow_mut().push(s);
        };
        let mut now = || clock.time.get();
        let result = watch(42, "https://copr.example", &mut fetch, Timing { sleep: &mut sleep, now: &mut now, timeout, max_errors: 8 });
        (result, calls)
    }

    fn state(s: &str) -> std::result::Result<Value, Fetch> {
        Ok(Value::String(s.into()))
    }

    #[test]
    fn an_accepted_build_is_recorded_before_polling_and_never_resubmitted() {
        let dir = TestDir::new("copr-record");
        let report = dir.0.join("sub/build.json");
        let uploads = Cell::new(0);
        let clock = clock();
        let mut responses = vec![Err(Fetch::Transient), state("running"), Err(Fetch::Http(503)), state("succeeded")].into_iter();
        let result = publish(
            &mut || {
                uploads.set(uploads.get() + 1);
                Ok(42)
            },
            "https://copr.example",
            &report,
            &mut |id, base| {
                let recorded: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
                assert_eq!(recorded["build_id"], 42);
                let mut fetch = |b: &str, i: u64, _: f64| {
                    assert_eq!((b, i), ("https://copr.example", 42));
                    responses.next().unwrap()
                };
                let mut sleep = |s: f64| {
                    clock.time.set(clock.time.get() + s);
                    clock.delays.borrow_mut().push(s);
                };
                let mut now = || clock.time.get();
                watch(id, base, &mut fetch, Timing { sleep: &mut sleep, now: &mut now, timeout: 7200.0, max_errors: 8 })
            },
        );
        result.unwrap();
        assert_eq!(uploads.get(), 1);
        assert_eq!(clock.delays.borrow().as_slice(), [15.0, 30.0, 15.0]);
        let recorded: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        assert_eq!(recorded["url"], "https://copr.example/coprs/build/42");
        assert!(fs::read_to_string(&report).unwrap().ends_with("}\n"));
    }

    #[test]
    fn an_upload_error_is_not_retried_or_reported_as_success() {
        let dir = TestDir::new("copr-upload");
        let report = dir.0.join("build.json");
        let (uploads, waits) = (Cell::new(0), Cell::new(0));
        let result = publish(
            &mut || {
                uploads.set(uploads.get() + 1);
                Err("ambiguous upload response".into())
            },
            "https://copr.example",
            &report,
            &mut |_, _| {
                waits.set(waits.get() + 1);
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err(), "ambiguous upload response");
        assert_eq!((uploads.get(), waits.get()), (1, 0));
        assert!(!report.exists());
    }

    #[test]
    fn failed_canceled_and_unknown_builds_fail_immediately() {
        for bad in ["failed", "canceled", "unknown"] {
            let c = clock();
            let (result, _) = run_watch(vec![state(bad)], 7200.0, &c);
            assert!(result.is_err(), "{bad}");
            assert!(c.delays.borrow().is_empty());
        }
        let c = clock();
        assert!(run_watch(vec![Ok(Value::Null)], 7200.0, &c).0.unwrap_err().contains("unexpected state None"));
        assert!(run_watch(vec![state("")], 7200.0, &clock()).0.unwrap_err().contains("unexpected state ''"));
    }

    #[test]
    fn permanent_http_errors_are_not_retried() {
        for code in [401, 403, 404] {
            let c = clock();
            let (result, calls) = run_watch(vec![Err(Fetch::Http(code))], 7200.0, &c);
            assert_eq!(result.unwrap_err(), format!("Copr status request failed: HTTP {code}"));
            assert_eq!(calls, 1);
            assert!(c.delays.borrow().is_empty());
        }
    }

    #[test]
    fn transient_rate_limit_timeout_and_bad_json_recover_with_growing_waits() {
        let c = clock();
        let (result, _) = run_watch(vec![Err(Fetch::Http(429)), Err(Fetch::Transient), Err(Fetch::Transient), state("succeeded")], 7200.0, &c);
        result.unwrap();
        assert_eq!(c.delays.borrow().as_slice(), [15.0, 30.0, 60.0]);
    }

    #[test]
    fn a_persistent_outage_stops_with_the_existing_id() {
        let c = clock();
        let (result, calls) = run_watch((0..8).map(|_| Err(Fetch::Transient)).collect(), 7200.0, &c);
        assert!(result.unwrap_err().contains("--build-id 42"));
        assert_eq!(calls, 8);
        assert!(c.delays.borrow().iter().all(|&d| d <= 120.0));
    }

    #[test]
    fn the_total_deadline_is_bounded_and_skipped_is_terminal() {
        let c = clock();
        let (result, _) = run_watch((0..8).map(|_| Err(Fetch::Transient)).collect(), 20.0, &c);
        assert!(result.unwrap_err().starts_with("Timed out waiting for Copr build 42"));
        assert_eq!(c.time.get(), 20.0);
        let c = clock();
        run_watch(vec![state("skipped")], 7200.0, &c).0.unwrap();
        assert!(c.delays.borrow().is_empty());
    }

    #[test]
    fn config_project_and_output_parsing() {
        let ini = parse_ini("# note\n[copr-cli]\nlogin = abc\nUsername: me\ntoken=xyz\ncopr_url = https://c.example/\n").unwrap();
        assert_eq!(ini["copr-cli"]["username"], "me");
        assert_eq!(ini["copr-cli"]["copr_url"], "https://c.example/");
        assert!(parse_ini("no section = 1").is_none() && parse_ini("[open\nx=1").is_none() && parse_ini("[a]\nnovalue").is_none());
        assert!(valid_project("owner/project") && valid_project("@group/pro.ject-1") && !valid_project("owner") && !valid_project("a/b/c") && !valid_project("o w/p") && !valid_project("/p"));
        assert_eq!(created_build("Uploading...\nBuild was added to x:\n  https://c/coprs/build/11350158/\nCreated builds: 11350158\n"), Some(11350158));
        assert_eq!(created_build("nothing"), None);
        assert_eq!((seconds(15.0), seconds(0.5), seconds(2.25)), ("15".into(), "0.5".into(), "2.25".into()));
    }
}
