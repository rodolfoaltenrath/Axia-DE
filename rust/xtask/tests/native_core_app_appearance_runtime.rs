//! Remaining core app views consume native colors without a JSON theme overlay.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[path = "support/app_appearance.rs"]
mod appearance;
const APPS: &[&str] = &["calculator","monitor","about","store","forcequit","appabout","sysprobe"];

#[test]
#[ignore = "requires built desktop and bundles; core app native appearance captures"]
fn core_app_views_follow_native_appearance_with_stale_js_mirrors() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for app in APPS {
        let driver=appearance::DRIVER.replace("@APP_ID@",app);
        let kind=format!("appearance_{app}");
        for mode in ["light","dark"] { for width in [640,1000] {
            let report=ui::run_ui_with_capture(repo,&kind,&driver,mode,width,"appearance.png",|path|{
                let pixels=image::open(path).unwrap().into_rgb8();
                if ["monitor","store"].contains(app) {
                    let bg=match (*app,mode) {
                        ("monitor","light")=>[239,238,243],("monitor",_)=>[27,27,30],
                        (_,"light")=>[255,255,255],_=>[30,30,33],
                    };
                    assert!(pixels.pixels().filter(|p|p.0==bg).count()>1000,
                        "app palette mode was not updated: {app}/{mode}/{width}");
                } else {
                    let color=if mode=="light" {[46,52,54]} else {[238,238,236]};
                    let dim=if mode=="light" {[104,108,109]} else {[170,170,170]};
                    assert!(pixels.pixels().filter(|p|p.0==color||p.0==dim).count()>20,
                        "native text palette was not painted: {app}/{mode}/{width}");
                }
            });
            for flag in ["nativeCapability","mounted","noAppearanceFacts","withoutFacts","staleMirror","retainedAfterPoison","saved"] {
                assert_eq!(report[flag],true,"{app}/{mode}/{width}/{flag}: {report}");
            }
        } }
    }
}
