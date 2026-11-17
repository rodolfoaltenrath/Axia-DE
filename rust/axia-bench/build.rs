//! Native host archive comes from the shared engine/sandbox build, never from a
//! benchmark C implementation. Cargo owns the complete Rust dependency graph.
fn main() {
    println!("cargo:rerun-if-env-changed=AXIA_BENCH_HOST_DIR");
    if std::env::var_os("CARGO_FEATURE_NATIVE_HOST").is_none() {
        return;
    }
    let dir = std::env::var_os("AXIA_BENCH_HOST_DIR").expect("native-host needs AXIA_BENCH_HOST_DIR; use make -C server build/axia-bench or lordbench-test");
    let dir = std::path::PathBuf::from(dir);
    println!(
        "cargo:rerun-if-changed={}",
        dir.join("libbench_host.a").display()
    );
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=bench_host");
    for lib in [
        "gio-2.0",
        "gobject-2.0",
        "glib-2.0",
        "xkbcommon",
        "dl",
        "m",
        "pthread",
    ] {
        println!("cargo:rustc-link-lib={lib}");
    }
}
