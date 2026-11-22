fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(axia_fan::daemon::main(&argv));
}
