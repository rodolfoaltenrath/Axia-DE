//! Uses the prebundled driver, so RPM checks need neither Node nor test-profile builds.
#[cfg(target_os = "linux")]
#[path = "../tests/support/ui.rs"]
mod ui;
#[cfg(target_os = "linux")]
#[path = "../tests/support/fullscreen_client.rs"]
mod fullscreen_client;

pub fn execute(repo: &std::path::Path, args: &[String]) -> crate::Result<()> {
    let values = crate::options(args, &["installation", "bundle", "output", "client-home"])?;
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = values.get("client-home") { return fullscreen_client::run(std::path::Path::new(home)); }
        std::env::set_var("AXIA_UI_FULLSCREEN_CLIENT", std::env::current_exe().map_err(|e|e.to_string())?);
        if let Some(root) = values.get("installation") {
            let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
            std::env::set_var("AXIA_UI_SERVER", root.join("bin/axia-server"));
            std::env::set_var("AXIA_UI_FONTS", root.join("assets/fonts"));
        }
        let bundle = values
            .get("bundle")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| repo.join("tests/switcher-native.bundle.js"));
        std::env::set_var(
            "AXIA_UI_PREBUILT",
            std::fs::canonicalize(bundle).map_err(|error| error.to_string())?,
        );
        if let Some(output) = values.get("output") {
            std::env::set_var("AXIA_SWITCHER_AXIA_OUTPUT", output);
        }
        let report = ui::run_ui(
            repo,
            "switcher_axia",
            crate::switcher_contract::DRIVER,
            "dark",
            1000,
            "switcher.png",
        );
        crate::switcher_contract::verify(&report)?;
        println!("Native switcher integration: passed");
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (repo, values);
        Err("switcher-check requires Linux".into())
    }
}
