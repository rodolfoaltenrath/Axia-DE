//! Manual comparison of the three release archives in an actual private session.
#![cfg(target_os = "linux")]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const VARIANTS: [&str; 3] = ["fedora", "thin", "cgu8"];

fn profile_command(program: &str, build: &Value) -> Command {
    let mut command = Command::new(program);
    command
        .env("RUSTFLAGS", build["rustflags"].as_str().unwrap())
        .env(
            "CARGO_PROFILE_RELEASE_LTO",
            build["profile"]["lto"].as_str().unwrap(),
        )
        .env(
            "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
            build["profile"]["codegen_units"].as_str().unwrap(),
        )
        .env("CARGO_BUILD_JOBS", "2")
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");
    command
}

fn validate_workloads(reports: &[Value]) -> Result<(), String> {
    if reports.len() != 3 {
        return Err("three workload reports are required".into());
    }
    let first = &reports[0]["native"];
    for report in reports {
        let native = &report["native"];
        if native["status"] != "passed"
            || native["resolution"] != json!([1920, 1080])
            || native["scene_nodes"] != 529
            || native["scene_checksum"].as_u64().unwrap_or(0) == 0
            || native["scene_checksum"] != first["scene_checksum"]
            || native["shaping"] != first["shaping"]
            || native["shaping"].as_array().is_none_or(|a| a.len() != 5)
        {
            return Err("profile changes engine pixels, script metrics or scene contract".into());
        }
        for key in [
            "layout_paint_serial_fps",
            "layout_paint_four_threads_fps",
            "shaping_one_thread_runs_per_second",
        ] {
            let metric = &native[key];
            let mut samples = metric["samples"]
                .as_array()
                .ok_or("missing throughput samples")?
                .iter()
                .map(|v| {
                    v.as_f64()
                        .filter(|v| v.is_finite() && *v > 0.)
                        .ok_or("invalid throughput sample")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if samples.len() != 3
                || metric["sample_seconds"] != 1.0
                || metric["warmup_seconds"] != 0.1
            {
                return Err("incomplete throughput sample set".into());
            }
            samples.sort_by(f64::total_cmp);
            if metric["median"].as_f64() != Some(samples[1]) {
                return Err("incorrect throughput median".into());
            }
        }
    }
    Ok(())
}

// Reject mixed, partial or warm builds before replacing any executable. A
// successful link alone cannot establish that a profile comparison is valid.
fn validate_builds(builds: &[Value]) -> Result<(), String> {
    if builds.len() != VARIANTS.len() {
        return Err("three complete build reports are required".into());
    }
    let baseline = &builds[0];
    let mut targets = std::collections::HashSet::new();
    for (variant, build) in VARIANTS.iter().zip(builds) {
        let reject = |reason: &str| format!("{variant}: {reason}");
        if build["variant"] != *variant
            || build["exit_code"] != 0
            || build["memory"]["exit_code"] != 0
        {
            return Err(reject("wrong variant or unsuccessful build/collection"));
        }
        if !build["fresh"].as_array().is_some_and(Vec::is_empty)
            || !build["compiled"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "axia_shell"))
        {
            return Err(reject("the shell must be compiled in a fresh target"));
        }
        for key in ["revision", "rustc", "cargo", "fedora_rustflags"] {
            if build[key].as_str().is_none_or(str::is_empty) || build[key] != baseline[key] {
                return Err(reject(&format!("missing or inconsistent {key}")));
            }
        }
        if build["working_tree_changes"] != "" {
            return Err(reject("source checkout must be clean"));
        }
        let (lto, units) = if *variant == "thin" {
            ("thin", "1")
        } else if *variant == "cgu8" {
            ("false", "8")
        } else {
            ("false", "1")
        };
        if build["profile"] != json!({"lto":lto,"codegen_units":units,"jobs":2,"incremental":false})
        {
            return Err(reject("unexpected Cargo profile"));
        }
        let flags = baseline["fedora_rustflags"].as_str().unwrap();
        let expected = if units == "1" {
            flags.to_owned()
        } else {
            format!("{flags} -Ccodegen-units={units}")
        };
        if build["rustflags"] != expected {
            return Err(reject("unexpected effective Rust flags"));
        }
        let target = build["target_directory"]
            .as_str()
            .ok_or_else(|| reject("missing target directory"))?;
        if !Path::new(target).is_absolute() || !targets.insert(target) {
            return Err(reject("targets must be absolute and distinct"));
        }
        if build["archive_bytes"].as_u64().unwrap_or(0) == 0
            || build["memory"]["elapsed_seconds"]
                .as_f64()
                .is_none_or(|v| !v.is_finite() || v <= 0.)
            || build["memory"]["peak_single_rustc_rss_bytes"]
                .as_u64()
                .unwrap_or(0)
                == 0
            || build["memory"]["peak_process_tree_rss_bytes"]
                .as_u64()
                .unwrap_or(0)
                == 0
        {
            return Err(reject("incomplete archive or memory telemetry"));
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires three profile-check reports, build dependencies and a private staged installation"]
fn release_profiles_link_and_run_private_sessions() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let results =
        PathBuf::from(std::env::var_os("AXIA_PROFILE_RESULTS").expect("AXIA_PROFILE_RESULTS"));
    let stage = PathBuf::from(std::env::var_os("AXIA_PROFILE_STAGE").expect("AXIA_PROFILE_STAGE"));
    // Require an explicit marker before replacing any staged executable.
    assert!(
        stage.join(".axia-profile-stage").is_file(),
        "use a disposable marked stage"
    );
    let builds: Vec<Value> = VARIANTS
        .iter()
        .map(|variant| {
            serde_json::from_slice(&fs::read(results.join(format!("{variant}.json"))).unwrap())
                .unwrap()
        })
        .collect();
    validate_builds(&builds).expect("invalid profile comparison");
    for build in &builds {
        let archive =
            Path::new(build["target_directory"].as_str().unwrap()).join("release/libaxia_shell.a");
        assert_eq!(
            fs::metadata(&archive).unwrap().len(),
            build["archive_bytes"].as_u64().unwrap(),
            "archive changed since build measurement: {}",
            archive.display()
        );
    }
    let mut reports = vec![];
    for (variant, build) in VARIANTS.iter().zip(builds) {
        let archive =
            Path::new(build["target_directory"].as_str().unwrap()).join("release/libaxia_shell.a");
        let before_link = fs::metadata(&archive).unwrap();
        let executable = repo.join("server/build/axia-server");
        // Archives precede these links; removing only the generated executable
        // prevents Make from reusing a link against the preceding variant.
        if executable.exists() {
            fs::remove_file(&executable).unwrap();
        }
        let log_path = results.join(format!("{variant}-link.log"));
        let log = fs::File::create(&log_path).unwrap();
        // The compositor also links a Rust benchmark archive. Make cannot see
        // profile changes in an existing archive, and mixing its dependencies
        // with a differently optimized shell can duplicate exported C symbols.
        // Keep this non-native-host archive separate from both other variants
        // and the native workload below, which enables native-host.
        let bench_target = Path::new(build["target_directory"].as_str().unwrap()).join("bench-c");
        let status = profile_command("cargo", &build)
            .current_dir(repo)
            .args([
                "build",
                "--manifest-path",
                "rust/Cargo.toml",
                "-p",
                "axia-bench",
                "--lib",
                "--release",
                "--locked",
                "--offline",
                "--target-dir",
            ])
            .arg(&bench_target)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log.try_clone().unwrap()))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "C benchmark archive: {}",
            log_path.display()
        );
        let status = profile_command("make", &build)
            .current_dir(repo)
            .args([
                "-j2",
                "-C",
                "server",
                "build/axia-server",
                "GPU=1",
                "CFLAGS=-O2 -g -D_FORTIFY_SOURCE=3 -fstack-protector-strong",
            ])
            .arg(format!(
                "RUST_TARGET_DIR={}",
                build["target_directory"].as_str().unwrap()
            ))
            .arg(format!(
                "BENCH_LIB={}",
                bench_target.join("release/libaxia_bench.a").display()
            ))
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "{variant} link failed: {}",
            log_path.display()
        );
        let after_link = fs::metadata(&archive).unwrap();
        assert_eq!(
            (before_link.len(), before_link.modified().unwrap()),
            (after_link.len(), after_link.modified().unwrap()),
            "link rebuilt the measured Rust archive for {variant}"
        );
        let native_log = results.join(format!("{variant}-native-build.log"));
        let log = fs::File::create(&native_log).unwrap();
        let status = profile_command("make", &build)
            .current_dir(repo)
            .args([
                "-j2",
                "-C",
                "server",
                "build/bench-host/libbench_host.a",
                "GPU=1",
                "CFLAGS=-O2 -g -D_FORTIFY_SOURCE=3 -fstack-protector-strong",
            ])
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log.try_clone().unwrap()))
            .status()
            .unwrap();
        assert!(status.success(), "benchmark host: {}", native_log.display());
        let status = profile_command("cargo", &build)
            .current_dir(repo)
            .args([
                "build",
                "--manifest-path",
                "rust/Cargo.toml",
                "-p",
                "axia-bench",
                "--bin",
                "axia-profile-ui",
                "--features",
                "native-host,gpu",
                "--release",
                "--locked",
                "--offline",
                "--target-dir",
            ])
            .arg(build["target_directory"].as_str().unwrap())
            .env("AXIA_BENCH_HOST_DIR", repo.join("server/build/bench-host"))
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "native workload build: {}",
            native_log.display()
        );
        let after_native = fs::metadata(&archive).unwrap();
        assert_eq!(
            (before_link.len(), before_link.modified().unwrap()),
            (after_native.len(), after_native.modified().unwrap()),
            "native workload build changed the measured shell archive"
        );
        let native_output = results.join(format!("{variant}-native.json"));
        let native_result = Command::new(
            Path::new(build["target_directory"].as_str().unwrap()).join("release/axia-profile-ui"),
        )
        .args(["--output"])
        .arg(&native_output)
        .output()
        .unwrap();
        assert!(
            native_result.status.success(),
            "native workload: {}",
            String::from_utf8_lossy(&native_result.stderr)
        );
        let native: Value = serde_json::from_slice(&fs::read(native_output).unwrap()).unwrap();
        fs::copy(&executable, stage.join("bin/axia-server")).unwrap();
        let probe = results.join(format!("{variant}-idle.json"));
        let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["idle-check", "--installation"])
            .arg(&stage)
            .arg("--output")
            .arg(&probe)
            .args([
                "--warmup",
                "8",
                "--duration",
                "20",
                "--samples",
                "3",
                "--max-polls-per-second",
                "15",
                "--max-rss-mib",
                "120",
                "--enforce",
                "true",
            ])
            .output()
            .unwrap();
        let idle: Value = serde_json::from_slice(&fs::read(&probe).unwrap()).unwrap();
        reports.push(json!({"variant":variant,"build":build,"server_bytes":fs::metadata(&executable).unwrap().len(),"native":native,"idle":idle}));
        fs::write(
            results.join("runtime.json"),
            serde_json::to_string_pretty(&reports).unwrap() + "\n",
        )
        .unwrap();
        assert!(
            result.status.success(),
            "{variant}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(reports.last().unwrap()["idle"]["status"], "passed");
        println!("{variant}: {}", reports.last().unwrap()["idle"]["summary"]);
    }
    validate_workloads(&reports).expect("inconsistent engine results");
}

fn fixtures() -> Vec<Value> {
    VARIANTS.iter().map(|variant| {
        let (lto,units)=if *variant=="thin" {("thin","1")} else if *variant=="cgu8" {("false","8")} else {("false","1")};
        json!({"variant":variant,"revision":"same-source","working_tree_changes":"","rustc":"same-rustc","cargo":"same-cargo","fedora_rustflags":"-Cdebuginfo=2","rustflags":if units=="1" {"-Cdebuginfo=2".into()} else {format!("-Cdebuginfo=2 -Ccodegen-units={units}")},"profile":{"lto":lto,"codegen_units":units,"jobs":2,"incremental":false},"target_directory":format!("/profiles/{variant}"),"archive_bytes":42,"compiled":["axia_shell"],"fresh":[],"exit_code":0,"memory":{"exit_code":0,"elapsed_seconds":1.,"peak_single_rustc_rss_bytes":100,"peak_process_tree_rss_bytes":200}})
    }).collect()
}

#[test]
fn comparable_cold_profiles_are_accepted() {
    assert!(validate_builds(&fixtures()).is_ok());
}

#[test]
fn partial_and_mixed_profile_measurements_are_rejected() {
    assert!(validate_builds(&fixtures()[..2]).is_err());
    for (key, value) in [
        ("revision", json!("other-source")),
        ("rustc", json!("other-compiler")),
        ("cargo", json!("other-cargo")),
        ("fedora_rustflags", json!("other-flags")),
        ("working_tree_changes", json!(" M app.rs")),
        ("fresh", json!(["axia_shell"])),
        ("compiled", json!([])),
        ("exit_code", json!(137)),
        ("archive_bytes", Value::Null),
        ("target_directory", json!("/profiles/fedora")),
        ("rustflags", json!("wrong-flags")),
    ] {
        let mut reports = fixtures();
        reports[1][key] = value;
        assert!(validate_builds(&reports).is_err(), "accepted invalid {key}");
    }
    for key in [
        "elapsed_seconds",
        "peak_single_rustc_rss_bytes",
        "peak_process_tree_rss_bytes",
    ] {
        let mut reports = fixtures();
        reports[2]["memory"][key] = json!(0);
        assert!(validate_builds(&reports).is_err(), "accepted invalid {key}");
    }
    let mut reports = fixtures();
    reports[2]["profile"]["codegen_units"] = json!("1");
    assert!(validate_builds(&reports).is_err());
}

#[test]
fn throughput_comparison_rejects_changed_pixels_and_incomplete_samples() {
    let metric = json!({"samples":[2.,1.,3.],"median":2.,"sample_seconds":1.,"warmup_seconds":0.1});
    let native = json!({"status":"passed","resolution":[1920,1080],"scene_nodes":529,"scene_checksum":42,"shaping":[1,2,3,4,5],"layout_paint_serial_fps":metric,"layout_paint_four_threads_fps":metric,"shaping_one_thread_runs_per_second":metric});
    let reports = vec![json!({"native":native}); 3];
    assert!(validate_workloads(&reports).is_ok());
    for (key, value) in [
        ("scene_checksum", json!(99)),
        ("shaping", json!([1, 2, 3, 4, 6])),
        ("scene_nodes", json!(528)),
        ("status", json!("failed")),
    ] {
        let mut changed = reports.clone();
        changed[1]["native"][key] = value;
        assert!(validate_workloads(&changed).is_err());
    }
    for (key, value) in [
        ("samples", json!([1., 2.])),
        ("samples", json!([0., 2., 3.])),
        ("median", json!(3.)),
        ("sample_seconds", json!(0.5)),
    ] {
        let mut changed = reports.clone();
        changed[2]["native"]["layout_paint_serial_fps"][key] = value;
        assert!(validate_workloads(&changed).is_err());
    }
}
