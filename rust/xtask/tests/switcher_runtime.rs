//! Real native view, window model, shortcut dispatcher and pointer routing.
#![cfg(target_os = "linux")]
#[path = "../src/switcher_contract.rs"]
mod contract;
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn switcher_reads_native_state_and_renders_both_styles() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        let driver = contract::DRIVER.replace("@APPEARANCE@", appearance);
        for mode in ["light", "dark"] {
            for width in [640, 1000] {
                let report = ui::run_ui(
                    repo,
                    &format!("switcher_{appearance}"),
                    &driver,
                    mode,
                    width,
                    "switcher.png",
                );
                contract::verify(&report).unwrap();
            }
        }
    }
}
