#[path = "host.rs"]
mod host;
fn main() {
    std::process::exit(axia_bench::helper_main())
}
