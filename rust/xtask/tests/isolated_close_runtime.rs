//! Closing a system app that runs in its own process, the way the title bar's × does: the
//! compositor asks the app's window to close and the app, which hears of it through its host,
//! closes it. Real isolated processes.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and app-host bundle; real isolated app processes"]
fn the_title_bar_closes_isolated_system_apps() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let report = ui::run_ui_with_timeout(repo, "isolated_close", DRIVER, "light", 1000, "closed.png", 40);
    for app in ["calendar", "contacts", "reminders"] {
        for step in ["opened", "stayed", "closed"] {
            assert_eq!(report[app][step], true, "{app}/{step}: {report}");
        }
    }
}

const DRIVER: &str = r#"
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,steps=400){for(let i=0;i<steps&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h},{render,View},{configureWindows,wm,windows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js'),import('@ROOT@/sdk/wayland.js')]);
 configureWindows({apps:[],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
 render(h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'#e8e8ec'}}));
 const report={};
 for(const app of ['calendar','contacts','reminders']){
  const mine=()=>windows.get().filter(w=>w.args?.surface&&w.args?.appId===app);
  const step=report[app]={};
  if(!lord.apps.launchPackage(app))throw Error(app+' did not start');
  step.opened=await until(()=>mine().length===1);
  await sleep(700);
  step.stayed=mine().length===1;
  // what the title bar's × does: for a window of another process it only asks (false)
  if(step.stayed)step.asked=wm.close(mine()[0].id);
  step.closed=step.stayed&&await until(()=>mine().length===0,200);
  lord.broker.quit(app);
 }
 const source=lord.media.screenshot('screenshots');await media.saveImage(source,@IMAGE@);media.close(source);
 console.log('isolated_close-observation '+JSON.stringify(report));
}
setTimeout(()=>run().catch(error=>console.error('isolated_close driver: '+(error.stack||error))),20);
"#;
