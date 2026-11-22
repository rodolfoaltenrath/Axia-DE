//! Real native view, window model, shortcut dispatcher and pointer routing.
#![cfg(target_os = "linux")]
#[path = "../src/switcher_contract.rs"]
mod contract;
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn switcher_reads_native_state_and_renders_both_styles() {
    std::env::set_var("AXIA_UI_FULLSCREEN_CLIENT", env!("CARGO_BIN_EXE_xtask"));
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        let driver = contract::DRIVER.replace("@APPEARANCE@", appearance);
        for mode in ["light", "dark"] {
            for width in [640, 1000] {
                let mut captured = None;
                let report = ui::run_ui_with_capture(
                    repo,
                    &format!("switcher_{appearance}"),
                    &driver,
                    mode,
                    width,
                    "switcher.png",
                    |path| { captured = Some(image::open(path).unwrap().into_rgb8()); },
                );
                contract::verify(&report).unwrap();
                let pixels = captured.unwrap();
                let rect = report["rect"].as_array().unwrap();
                let columns = ((width - 96) / 104).max(1).min(18);
                let x = width / 2 + (columns - 1) * 52 + 44;
                let y = (rect[1].as_f64().unwrap() - 62.) as u32;
                let actual = pixels.get_pixel(x,y).0;
                let bg = if mode == "light" { [242.,242.,245.] } else { [42.,42.,48.] };
                for channel in 0..3 {
                    let expected = [48.,209.,88.][channel] / 3. + bg[channel] * 2. / 3.;
                    assert!((actual[channel] as f64 - expected).abs() <= 3.,
                        "native switcher highlight/material: {appearance}/{mode}/{width}: {actual:?}");
                }
            }
        }
    }
}
