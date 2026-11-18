//! Private Linux session measurements. No Python or shell test harness.
use crate::{at, options, Result};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

#[cfg(target_os = "linux")]
mod linux;

pub(super) struct Config {
    installation: PathBuf,
    output: PathBuf,
    warmup: Duration,
    duration: Duration,
    samples: usize,
    max_polls: f64,
    max_rss: f64,
    enforce: bool,
}

fn positive(value: &str) -> Result<f64> {
    let value: f64 = value.parse().map_err(|_| "expected a number")?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err("expected a positive finite number".into())
    }
}

impl Config {
    fn parse(args: &[String]) -> Result<Self> {
        let args = options(
            args,
            &[
                "installation",
                "output",
                "warmup",
                "duration",
                "samples",
                "max-polls-per-second",
                "max-rss-mib",
                "enforce",
            ],
        )?;
        let required = |key| {
            args.get(key)
                .cloned()
                .ok_or_else(|| format!("idle-check requires --{key}"))
        };
        let number =
            |key, default: &str| positive(args.get(key).map(String::as_str).unwrap_or(default));
        let seconds = |key, default| {
            let duration = Duration::try_from_secs_f64(number(key, default)?)
                .map_err(|_| format!("--{key} is outside the supported duration range"))?;
            if duration.is_zero() {
                return Err(format!("--{key} must be at least one nanosecond"));
            }
            Ok(duration)
        };
        let samples = args
            .get("samples")
            .map(String::as_str)
            .unwrap_or("3")
            .parse::<usize>()
            .map_err(|_| "--samples must be a positive odd integer")?;
        if samples == 0 || samples % 2 == 0 {
            return Err("--samples must be a positive odd integer".into());
        }
        let enforce = match args.get("enforce").map(String::as_str).unwrap_or("true") {
            "true" => true,
            "false" => false,
            _ => return Err("--enforce must be true or false".into()),
        };
        let max_rss = number("max-rss-mib", "120")? * 1048576.0;
        if !max_rss.is_finite() {
            return Err("RSS budget is outside the supported range".into());
        }
        Ok(Self {
            installation: required("installation")?.into(),
            output: required("output")?.into(),
            warmup: seconds("warmup", "8")?,
            duration: seconds("duration", "20")?,
            samples,
            max_polls: number("max-polls-per-second", "15")?,
            max_rss,
            enforce,
        })
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        values[middle - 1] / 2.0 + values[middle] / 2.0
    } else {
        values[middle]
    }
}

fn evaluate(samples: &[Value], expected: usize, max_polls: f64, max_rss: f64) -> Result<Value> {
    if samples.is_empty() || samples.len() != expected {
        return Err("incomplete sample set".into());
    }
    let metric = |key| -> Result<f64> {
        let mut values = samples
            .iter()
            .map(|sample| {
                sample[key]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .ok_or_else(|| format!("invalid sample metric: {key}"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(median(&mut values))
    };
    let polls = metric("poll_calls_per_second")?;
    let rss = metric("rss_bytes_median")?;
    let mut exceeded = vec![];
    if polls > max_polls {
        exceeded.push("poll_calls_per_second");
    }
    if rss > max_rss {
        exceeded.push("rss_bytes_median");
    }
    Ok(json!({"poll_calls_per_second": polls, "rss_bytes_median": rss, "exceeded": exceeded}))
}

fn poll_count(text: &str, since: f64, until: f64) -> Result<u64> {
    let mut count = 0;
    let mut blocked = false;
    for line in text.lines() {
        let Some((timestamp, call)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(start) = timestamp.parse::<f64>() else {
            continue;
        };
        let call = call.trim_start();
        if !["poll(", "ppoll(", "epoll_wait(", "epoll_pwait("]
            .iter()
            .any(|name| call.starts_with(name))
            || !call
                .split(')')
                .skip(1)
                .any(|result| result.trim_start().starts_with("= "))
            || call.contains("<unfinished ...>")
        {
            continue;
        }
        if since <= start && start < until {
            count += 1;
        }
        if let Some((_, duration)) = line.rsplit_once('<') {
            if let Some(duration) = duration.trim_end().strip_suffix('>') {
                if let Ok(elapsed) = duration.parse::<f64>() {
                    blocked |= start <= since && start + elapsed >= until;
                }
            }
        }
    }
    if count == 0 && !blocked {
        return Err("no complete main-thread poll calls in the collection window".into());
    }
    Ok(count)
}

pub fn execute(args: &[String]) -> Result<()> {
    let config = Config::parse(args)?;
    run(&config)
}

fn run(config: &Config) -> Result<()> {
    let parent = config
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    at(parent, fs::create_dir_all(parent))?;
    let mut report = json!({"status":"invalid", "samples":[], "scope":"main thread, software, offline",
        "budgets":{"poll_calls_per_second":config.max_polls,"rss_bytes_median":config.max_rss}, "enforced":config.enforce});
    #[cfg(target_os = "linux")]
    let result = linux::collect_all(config, &mut report);
    #[cfg(not(target_os = "linux"))]
    let result: Result<()> = Err("idle-check requires Linux with strace and /proc".into());
    let result = result.and_then(|()| {
        let summary = evaluate(
            report["samples"].as_array().unwrap(),
            config.samples,
            config.max_polls,
            config.max_rss,
        )?;
        report["status"] = json!(if summary["exceeded"].as_array().unwrap().is_empty() {
            "passed"
        } else {
            "exceeded"
        });
        report["summary"] = summary;
        Ok(())
    });
    if let Err(error) = &result {
        report["error"] = json!(error);
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
    at(&config.output, fs::write(&config.output, text))?;
    println!("{}", report.get("summary").unwrap_or(&report));
    result?;
    if config.enforce && report["status"] == "exceeded" {
        return Err("private session exceeded idle budgets".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_window_rejects_unfinished_and_unrelated_calls() {
        let trace = "9.5 poll([], 0, 5) = 0 <0.005>\n10.0 poll([], 0, 5) = 0 <0.005>\n10.1 ppoll([], 0, NULL, NULL, 8) = 0 <0.005>\n10.2 epoll_wait(4, [], 1, 0) = 0 <0.0001>\n10.3 read(5, x, 2) = 0 <0.0001>\n10.4 poll([], 0, 5 <unfinished ...>\n11.0 poll([], 0, 5) = 0 <0.005>";
        assert_eq!(poll_count(trace, 10., 11.).unwrap(), 3);
    }
    #[test]
    fn blocked_session_can_have_zero_wakeups_but_missing_trace_cannot() {
        assert_eq!(
            poll_count("9.0 poll([], 0, -1) = -1 EINTR <22.0>", 10., 30.).unwrap(),
            0
        );
        for text in ["", "9.0 poll([], 0, 1) = 0 <0.001>"] {
            assert!(poll_count(text, 10., 30.).is_err());
        }
    }

    #[test]
    fn aligned_strace_results_are_counted() {
        assert_eq!(
            poll_count("10.0 poll([], 0, 1)    = 0 (Timeout) <0.001>", 10., 11.).unwrap(),
            1
        );
    }
    fn sample(polls: f64, rss: f64) -> Value {
        json!({"poll_calls_per_second": polls,"rss_bytes_median":rss})
    }
    #[test]
    fn both_median_budgets_are_enforced() {
        let result = evaluate(
            &[sample(16., 121.), sample(18., 122.), sample(1., 1.)],
            3,
            15.,
            120.,
        )
        .unwrap();
        assert_eq!(
            result["exceeded"],
            json!(["poll_calls_per_second", "rss_bytes_median"])
        );
    }
    #[test]
    fn outlier_does_not_replace_median_and_exact_budget_passes() {
        assert_eq!(
            evaluate(
                &[sample(12., 100.), sample(13., 100.), sample(100., 100.)],
                3,
                15.,
                120.
            )
            .unwrap()["exceeded"],
            json!([])
        );
        assert_eq!(
            evaluate(&[sample(15., 120.)], 1, 15., 120.).unwrap()["exceeded"],
            json!([])
        );
    }
    #[test]
    fn partial_and_corrupt_samples_cannot_pass() {
        assert!(evaluate(&[sample(1., 1.)], 3, 15., 120.).is_err());
        assert!(evaluate(&[json!({})], 1, 15., 120.).is_err());
    }
    #[test]
    fn nonfinite_and_nonpositive_budgets_are_rejected() {
        for value in ["NaN", "inf", "-1", "0", "1e999"] {
            assert!(positive(value).is_err());
        }
    }
    #[test]
    fn invalid_installation_fails_during_calibration_and_preserves_report() {
        let temp = crate::TestDir::new("idle-missing");
        let config = Config {
            installation: temp.0.join("missing"),
            output: temp.0.join("report.json"),
            warmup: Duration::from_secs(1),
            duration: Duration::from_secs(1),
            samples: 1,
            max_polls: 15.,
            max_rss: 120. * 1048576.,
            enforce: false,
        };
        assert!(run(&config).is_err());
        let report: Value = serde_json::from_slice(&fs::read(config.output).unwrap()).unwrap();
        assert_eq!(report["status"], "invalid");
        assert!(report["error"].is_string());
    }
    #[test]
    fn cli_rejects_even_samples_and_invalid_enforcement() {
        for (option, value) in [
            ("samples", "2"),
            ("samples", "0"),
            ("enforce", "yes"),
            ("duration", "1e100"),
        ] {
            let args = [
                "--installation",
                "unused",
                "--output",
                "unused",
                &format!("--{option}"),
                value,
            ]
            .map(str::to_owned);
            assert!(Config::parse(&args).is_err());
        }
    }
}
