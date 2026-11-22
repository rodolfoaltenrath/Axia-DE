//! Writes a fake tree (amd, intel, nvidia, rpi5, vm) for `axia-sysreport report --root <dir>`:
//! `cargo run -p axia-sysreport --example fixture -- <dir> <kind>`.
#[path = "../tests/support/mod.rs"]
mod support;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || !support::KINDS.contains(&args[2].as_str()) {
        eprintln!("usage: fixture <dir> amd|intel|nvidia|rpi5|vm");
        std::process::exit(2);
    }
    let dir = std::path::Path::new(&args[1]);
    std::fs::create_dir_all(dir).unwrap();
    support::fixture(dir, &args[2]);
}
