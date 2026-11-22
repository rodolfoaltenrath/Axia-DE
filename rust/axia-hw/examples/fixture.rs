//! Writes a fake tree for the Store's hardware facts (vmware, virtualbox, qemu, rpi5, nvidia, printer,
//! scanner, firmware, demo), for LORD_HWSCAN_ROOT (screenshots and manual tests):
//! `cargo run -p axia-hw --example fixture -- <dir> <kind>`.
#[path = "../tests/support/mod.rs"]
mod support;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || !support::KINDS.contains(&args[2].as_str()) {
        eprintln!("usage: fixture <dir> {}", support::KINDS.join("|"));
        std::process::exit(2);
    }
    let dir = std::path::Path::new(&args[1]);
    std::fs::create_dir_all(dir).unwrap();
    support::tree(dir, &args[2]);
    println!("fixture {} in {} (run axia-server with LORD_HWSCAN_ROOT={})", args[2], dir.display(), dir.display());
}
