//! Shared native appearance in the session views of the migrated apps.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[path = "support/app_appearance.rs"]
mod appearance;
use appearance::DRIVER;
const APPS: &[&str] = &["notes","characters","help","backup","contacts","calendar","reminders","recorder","camera","scanner","logs","fonts","photos"];

#[test]
#[ignore = "requires built desktop and bundles; native app appearance and captures"]
fn migrated_app_views_observe_native_appearance_without_theme_transport() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for app in APPS {
        let driver=DRIVER.replace("@APP_ID@",app);
        let kind=format!("appearance_{app}");
        for mode in ["light","dark"] { for width in [640,1000] {
            let report=ui::run_ui_with_capture(repo,&kind,&driver,mode,width,"appearance.png",|path|{
                let pixels=image::open(path).unwrap().into_rgb8();
                if *app != "camera" {
                    let color=if mode=="light" {[46,52,54]} else {[238,238,236]};
                    let dim=if mode=="light" {[104,108,109]} else {[170,170,170]};
                    let text=pixels.pixels().filter(|p|p.0==color||p.0==dim).count();
                    assert!(text>20,"native text palette was not painted: {app}/{mode}/{width}, {text}");
                }
            });
            for flag in ["nativeCapability","mounted","noAppearanceFacts","withoutFacts","staleMirror","retainedAfterPoison","saved"] {
                assert_eq!(report[flag],true,"{app}/{mode}/{width}/{flag}: {report}");
            }
        } }
    }
}
