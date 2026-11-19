//! The aggregate shell smoke previously driven by the Python packaging check.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and npm ci; required by make check for both GPU variants"]
fn shell_surfaces_use_real_native_window_state() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let source = include_str!("../../../server/tests/menubar_smoke.jsx")
        .replace("../../sdk/", "@ROOT@/sdk/")
        .replace("../../themes/", "@ROOT@/themes/")
        .replace(
            "console.error(e);",
            "console.error('shell_surfaces driver: '+e.message+'\\n'+(e.stack||''));",
        );
    let driver = format!("{PREFIX}\n{source}");
    let report = ui::run_ui(
        repo,
        "shell_surfaces",
        &driver,
        "dark",
        1000,
        "shell-surfaces.png",
    );
    for stage in [
        "menu bar",
        "title bar",
        "launcher",
        "spotlight",
        "banner",
        "menu",
        "dock parts",
    ] {
        assert_eq!(report["completed"][stage], true, "{stage}: {report}");
    }
    assert_eq!(report["saved"], true, "{report}");
    assert_eq!(report["errors"], serde_json::json!([]), "{report}");
}
const PREFIX: &str = r#"
import {media as captureMedia} from '@ROOT@/sdk/media.js';
const completed={},errors=[],originalLog=console.log.bind(console),originalError=console.error.bind(console);
console.error=(...args)=>{errors.push(args.map(String).join(' '));originalError(...args);};
console.log=(...args)=>{
 const text=String(args[0]);
 if(text.endsWith(' integration: passed'))completed[text.slice(0,-' integration: passed'.length)]=true;
 originalLog(...args);
 if(text==='dock parts integration: passed')setTimeout(async()=>{
  const image=lord.media.screenshot('screenshots'),saved=await captureMedia.saveImage(image,@IMAGE@);captureMedia.close(image);
  originalLog('shell_surfaces-observation '+JSON.stringify({completed,errors,saved:!!saved?.ok}));
 },50);
};
"#;
