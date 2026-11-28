//! The compiled Terminal over real PTYs. Rust owns the assertions, captures,
//! isolated account and compositor lifetime; the driver reaches the current session adapter.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn terminal_runs_shell_input_splits_tabs_remounts_searches_and_releases_sessions() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        // The headless compositor requires 640 px; constrain the app to its 360 px minimum inside it.
        for width in [640, 1000] {
            let driver = DRIVER.replace("@APPWIDTH@", if width == 640 { "360" } else { "1000" });
            let report = ui::run_ui_with_capture_timeout(
                repo,
                "terminal",
                &driver,
                mode,
                width,
                "terminal.png",
                45,
                |path| {
                    let capture = image::open(path).unwrap().into_rgb8();
                    let green = capture
                        .pixels()
                        .filter(|p| p[1] > 150 && p[0] < 100 && p[2] < 100)
                        .count();
                    assert!(
                        green > 30,
                        "{mode}/{width}: real ANSI shell output was not painted ({green})"
                    );
                },
            );
            assert!(report["error"].is_null(), "{mode}/{width}: {report}");
            for flag in [
                "compiled",
                "shell",
                "typed",
                "split",
                "tab",
                "remounted",
                "searched",
                "geometry",
                "painted",
                "guarded",
                "cancelled",
                "closed",
                "released",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/runtime/nativeapps.js';
import {configureWindows,wm,useWindows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='terminal',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms)),report={};
let handle=0,stage='bootstrap',window;
locale.set({...locale.get(),lang:'en'});
settings.set({language:'en',mode:'@MODE@',effects:false,terminal:{}});
const mount=lord.nativeViews.mount;
lord.nativeViews.mount=(node,id,raw,...rest)=>{const result=mount(node,id,raw,...rest);if(id==='terminal'&&!JSON.parse(raw).icon)handle=result;return result;};
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='terminal');
configureWindows({apps:[nativeApp(manifest)],workArea:{x:0,y:0,w:@APPWIDTH@,h:700}});
window=wm.open('terminal');
function Harness(){useWindows();const w=wm.get(window);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@APPWIDTH@,height:700}},w&&!w.minimized?h(AppHost,{key:window,id:window,focused:true}):null));}
render(h(Harness));
const ids=()=>lord.term.list();
const text=()=>lord.nativeViews.text(handle)||'';
const output=sid=>lord.term.text(sid,'screen')||'';
const read=name=>{try{return lord.fs.read(lord.fs.home()+'/'+name);}catch(_){return null;}};
const action=(cmd,key)=>{lord.nativeViews.action(handle,{action:[cmd,key]});__lordNativeView();};
const key=(name,char='',mods=0)=>{__lordKey(0,1,name,char,char||name,mods);__lordKey(0,0,name,'',char||name,mods);__lordNativeView();};
const type=word=>{for(const ch of word)key(ch,ch);};
async function until(fn,label){stage=label;console.log('terminal-stage '+label);for(let i=0;i<350;i++){if(fn())return true;await sleep(20);}throw Error('timeout: '+label+'; '+text());}
async function capture(){await sleep(250);const image=lord.media.screenshot('screenshots');try{return!!(await media.saveImage(image,@IMAGE@))?.ok;}finally{media.close(image);}}
async function run(){
 report.compiled=manifest?.nativeView==='rust';
 await until(()=>handle&&ids().length===1&&lord.term.info(ids()[0])?.alive,'shell started');
 const first=ids()[0];
 report.shell=!!lord.term.info(first).shell;
 // A file proves execution: echoed input alone cannot satisfy this observation.
 type("printf '\\033[38;2;0;255;0mAXIA_PTY_42\\033[0m\\n'; printf executed > axia-terminal-proof");key('Enter');
 report.typed=await until(()=>read('axia-terminal-proof')==='executed'&&output(first).includes('AXIA_PTY_42'),'keyboard to PTY');
 action('do','splitRight');await until(()=>ids().length===2,'split creates a second PTY');
 const second=ids().find(s=>s!==first);
 report.split=await until(()=>lord.term.info(second)?.cols===lord.term.info(first)?.cols,'split mounts both PTYs');
 type('printf pane > axia-pane-proof');key('Enter');
 await until(()=>read('axia-pane-proof')==='pane','input reaches the split');
 action('do','newTab');report.tab=await until(()=>ids().length===3&&text().includes('❖2'),'new tab');
 action('do','closeTab');await until(()=>ids().length===2,'close tab releases its PTY');
 const old=handle;wm.minimize(window);await sleep(300);wm.focus(window);
 report.remounted=await until(()=>handle!==old&&ids().includes(first)&&ids().includes(second),'remount preserves PTYs');
 action('paneDown',first);action('do','find');await sleep(200);type('AXIA_PTY_42');
 report.searched=await until(()=>/\d+ of \d+/.test(text()),'native search');
 action('findClose');report.geometry=await until(()=>/[1-9]\d*×[1-9]\d*/.test(text()),'status follows PTY geometry');report.painted=await capture();
 // Keep the shell alive while a foreground process triggers the Rust close guard.
 type('sleep 30');key('Enter');
 await until(()=>lord.term.info(first)?.busy,'foreground process');
 await until(()=>text().includes('sleep'),'native foreground status');
 await sleep(200);wm.close(window);
 report.guarded=await until(()=>!!wm.get(window)&&text().includes('Close the window?')&&text().includes('sleep'),'close guard');
 action('dialogCancel');await until(()=>!text().includes('Close the window?'),'cancel close');
 report.cancelled=!!wm.get(window)&&!!lord.term.info(first)?.alive;
 wm.close(window);await until(()=>text().includes('Close the window?'),'second close guard');action('dialogConfirm');
 report.closed=await until(()=>!wm.get(window),'confirm closes window');
 report.released=await until(()=>ids().length===0,'window releases all PTYs');
 console.log(kind+'-observation '+JSON.stringify(report));
}
setTimeout(()=>run().catch(async error=>{console.log('terminal-failure '+JSON.stringify({error:String(error),stage,text:text(),sessions:ids().map(sid=>({sid,output:output(sid)}))}));try{await capture();}catch(_){}console.log(kind+'-observation '+JSON.stringify({...report,error:String(error),stage,text:text()}));}),200);
"#;
