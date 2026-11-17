#[path = "../../server/src/netperm/elf.rs"]
mod elf;
use std::{env, fs, path::PathBuf, process::Command};
fn run(mut c: Command) {
    let status = c.status().expect("start rustc");
    assert!(status.success(), "{c:?} failed");
}
fn rustc() -> Command {
    let mut c = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    c.env("RUSTC_BOOTSTRAP", "1");
    c
}
fn main() {
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../server/src/netperm");
    for f in ["kernel.rs", "shared.rs", "builtins.rs", "elf.rs"] {
        println!("cargo:rerun-if-changed={}", root.join(f).display());
    }
    if env::var_os("CARGO_FEATURE_BPF").is_none() {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let sys = out.join("sysroot");
    let lib = sys.join("lib/rustlib/bpfel-axia/lib");
    fs::create_dir_all(&lib).unwrap();
    // Rust's built-in BPF target emits bitcode for an external linker. We need a
    // single, fully inlined ELF object, so ask the same LLVM backend for machine code.
    let spec = rustc()
        .args([
            "-Zunstable-options",
            "--print",
            "target-spec-json",
            "--target",
            "bpfel-unknown-none",
        ])
        .output()
        .unwrap();
    assert!(spec.status.success(), "cannot query BPF target");
    let spec = String::from_utf8(spec.stdout)
        .unwrap()
        .replace("\"obj-is-bitcode\": true", "\"obj-is-bitcode\": false");
    let target = out.join("bpfel-axia.json");
    fs::write(&target, spec).unwrap();
    let sysroot = rustc().args(["--print", "sysroot"]).output().unwrap();
    let core = PathBuf::from(String::from_utf8(sysroot.stdout).unwrap().trim())
        .join("lib/rustlib/src/rust/library/core/src/lib.rs");
    assert!(
        core.exists(),
        "BPF=1 needs matching Rust sources: rustup component add rust-src"
    );
    let mut c = rustc();
    c.args([
        "-Zunstable-options",
        "--crate-name",
        "core",
        "--edition=2024",
        "--crate-type=rlib",
        "-C",
        "linker-plugin-lto",
        "-C",
        "opt-level=2",
        "-C",
        "panic=abort",
        "-Zforce-unstable-if-unmarked",
        "--target",
    ])
    .arg(&target)
    .arg("--out-dir")
    .arg(&lib)
    .arg(core);
    run(c);
    let mut c = rustc();
    c.args([
        "-Zunstable-options",
        "--crate-name",
        "compiler_builtins",
        "--crate-type=rlib",
        "--target",
    ])
    .arg(&target)
    .arg("--out-dir")
    .arg(&lib)
    .arg(root.join("builtins.rs"));
    run(c);
    let object = out.join("kernel.o");
    let mut c = rustc();
    c.args([
        "-Zunstable-options",
        "--edition=2021",
        "--crate-type=lib",
        "--target",
    ])
    .arg(&target)
    .arg("--sysroot")
    .arg(&sys)
    .args([
        "-C",
        "opt-level=2",
        "-C",
        "panic=abort",
        "-C",
        "overflow-checks=off",
        "--emit=obj",
        "-o",
    ])
    .arg(&object)
    .arg(root.join("kernel.rs"));
    run(c);
    let programs =
        elf::pack(&fs::read(object).unwrap()).expect("BPF object must be self-contained");
    assert_eq!(programs.len(), 11, "all hooks must be compiled");
    fs::write(out.join("programs.rs"), elf::emit(&programs)).unwrap();
}
