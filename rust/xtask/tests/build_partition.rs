//! Build boundaries that keep app growth out of the shell compilation unit.
use serde_json::Value;
use std::{path::Path, process::Command};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
}

fn output(command: &mut Command) -> String {
    let result = command.output().expect("start build graph command");
    assert!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

// A marker in a dry run identifies expanded Cargo recipes without executing
// builds, relying on a shell parser or matching comments containing "cargo".
fn cargo_recipes(graph: &str) -> Vec<Vec<&str>> {
    graph
        .lines()
        .filter_map(|line| {
            let words: Vec<_> = line.split_whitespace().collect();
            let start = words.iter().position(|word| *word == "axia-cargo-probe")?;
            Some(words[start + 1..].to_vec())
        })
        .collect()
}

fn target<'a>(args: &[&'a str]) -> &'a str {
    args[args.iter().position(|arg| *arg == "--target-dir").unwrap() + 1]
}

#[test]
fn apps_have_no_production_dependency_on_shell_or_other_apps() {
    let text = output(Command::new(env!("CARGO")).current_dir(repo()).args([
        "metadata",
        "--manifest-path",
        "rust/Cargo.toml",
        "--format-version",
        "1",
        "--no-deps",
        "--locked",
        "--offline",
    ]));
    // Cargo also exposes target-specific dependencies and the real package name
    // behind dependency aliases, which the former TOML text check missed.
    let metadata: Value = serde_json::from_str(&text).unwrap();
    let apps: Vec<_> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|package| package["name"].as_str().unwrap().starts_with("axia-app-"))
        .collect();
    assert!(!apps.is_empty(), "no app crates discovered");
    for app in apps {
        for dependency in app["dependencies"].as_array().unwrap() {
            if !dependency["kind"].is_null() {
                continue;
            }
            let name = dependency["name"].as_str().unwrap();
            assert!(
                name != "axia-shell" && !name.starts_with("axia-app-"),
                "{} depends on {name}",
                app["name"]
            );
        }
    }
}

#[test]
#[ignore = "requires Make and the Fedora desktop build dependencies; run in desktop CI"]
fn native_benchmark_reuses_shell_artifacts_without_overwriting_c_archive() {
    let graph = output(Command::new("make").current_dir(repo()).args([
        "-n",
        "-B",
        "-C",
        "server",
        "build/axia-bench",
        "build/bench-rust/release/libaxia_bench.a",
        "lordbench-test",
        "GPU=1",
        "CARGO=axia-cargo-probe",
    ]));
    let cargo = cargo_recipes(&graph);
    let find = |predicate: &dyn Fn(&[&str]) -> bool| {
        cargo.iter().position(|args| predicate(args)).unwrap()
    };
    let shell = find(&|args| args.contains(&"build") && !args.contains(&"-p"));
    let native = find(&|args| args.contains(&"--bin") && args.contains(&"axia-bench"));
    let native_test = find(&|args| args.contains(&"--test") && args.contains(&"lordbench-native"));
    let base = find(&|args| {
        args.contains(&"axia-bench") && args.contains(&"--lib") && !args.contains(&"--features")
    });
    assert_eq!(target(&cargo[shell]), target(&cargo[native]));
    assert_eq!(target(&cargo[shell]), target(&cargo[native_test]));
    assert_ne!(target(&cargo[shell]), target(&cargo[base]));
    assert!(shell < native && shell < native_test);
}

#[test]
#[ignore = "requires Make and the Fedora desktop build dependencies; run in desktop CI"]
fn rpm_platform_checks_do_not_compile_rust_test_profiles() {
    let graph = output(Command::new("make").current_dir(repo()).args([
        "-n",
        "-C",
        "server",
        "test-platform",
        "SKIP_RUST_TESTS=1",
        "CARGO=axia-cargo-probe",
    ]));
    assert!(cargo_recipes(&graph)
        .iter()
        .all(|args| args.first() != Some(&"test")));
    assert!(graph.contains("./build/switcher-ffi-test"));
    assert!(graph.contains("./build/settings-hw-test"));
}

#[test]
#[ignore = "requires Make and Fedora build dependencies; incremental native metadata"]
fn wallpaper_metadata_invalidates_the_native_shell_archive() {
    let graph = output(Command::new("make").current_dir(repo()).args([
        "-n", "-C", "server", "-W", "../assets/wallpapers/index.json",
        "build/rust/release/libaxia_shell.a", "GPU=1", "CARGO=axia-cargo-probe",
    ]));
    assert!(cargo_recipes(&graph).iter().any(|args| args.first() == Some(&"build") && !args.contains(&"-p")),
        "a wallpaper index change must rebuild the shell archive: {graph}");
}
