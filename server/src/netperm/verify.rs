//! Explicit verifier fixture: loads programs and maps; never attaches any hook.
use axia_netperm::bpf::{self, Maps};
fn main() {
    if unsafe { libc::geteuid() } != 0 {
        println!("npverify-test: kernel verifier skipped: needs root");
        return;
    }
    assert_eq!(
        bpf::PROGRAMS.len(),
        bpf::SPECS.len(),
        "build with --features bpf"
    );
    let maps = Maps::create().expect("create verifier maps");
    for spec in bpf::SPECS {
        let p = bpf::PROGRAMS
            .iter()
            .find(|p| p.section == spec.section)
            .expect("missing hook");
        let fd = bpf::load(spec, p, &maps).unwrap_or_else(|e| panic!("{}: {e}", spec.section));
        println!(
            "npverify-test: {} verified ({} instructions)",
            spec.section,
            p.insns.len()
        );
        drop(fd);
    }
}
