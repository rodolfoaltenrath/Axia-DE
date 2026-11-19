//! Native desktop revisions, expiry and the boundary shortcut through the real host.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn desktop_indicator_uses_native_state_and_expiry() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "desktop_hud", DRIVER, mode, width, "desktop-hud.png");
            for flag in ["bound", "initiallyHidden", "direct", "mirrorStale", "extended", "expired",
                "boundary", "overviewHidden", "singleHidden", "noWindowTransport", "unmounted", "saved"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {setDesktops,setActiveDesktop,activeDesktop} from '@ROOT@/sdk/windows.js';
import {DesktopHud,nextDesktopBy} from '@ROOT@/sdk/desktops.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();setDesktops([10,20,30],10);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
const source=lord.windowList,mount=lord.nativeViews.mount,send=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
let handle=0,bound=false,noWindowTransport=true,unmounted=false,hide;
function inspect(text){const input=JSON.parse(text);noWindowTransport&&=!('windows'in input)&&!('desktops'in(input.args||{}))&&!('active'in(input.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:desktop-hud'){inspect(text);bound=sources[0]===source;handle=mount(node,id,text,...sources);return handle;}return mount(node,id,text,...sources);};
lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return send(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounted=true;return unmount(id);};
function App(){const[on,setOn]=useState(true);hide=()=>setOn(false);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},on?h(DesktopHud):null));}
render(h(App));
const text=()=>lord.nativeViews.text(handle)||'';
const direct=(op,...args)=>source.call(op,JSON.stringify([{},...args]));
async function run(){
 await until(()=>handle>0);const initiallyHidden=!text().includes('Desktop');
 direct('setActiveDesktop',20);const directShown=await until(()=>text().includes('Desktop 2'));
 const mirrorStale=activeDesktop.get()===10;
 await sleep(450);direct('setActiveDesktop',30);await until(()=>text().includes('Desktop 3'));await sleep(450);
 const extended=text().includes('Desktop 3');
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 const expired=await until(()=>!text().includes('Desktop'));
 setActiveDesktop(20);setActiveDesktop(30);await until(()=>!text().includes('Desktop'));
 nextDesktopBy(1);const boundary=await until(()=>text().includes('Desktop 3'));
 direct('setOverview',true);const overviewHidden=await until(()=>!text().includes('Desktop'));
 direct('setOverview',false);await sleep(50);const noReplay=!text().includes('Desktop');
 direct('setDesktops',[10],10);lord.nativeViews.action(handle,{action:['show']});await sleep(50);
 const singleHidden=!text().includes('Desktop');hide();await until(()=>unmounted);
 console.log('desktop_hud-observation '+JSON.stringify({bound,initiallyHidden,direct:directShown,mirrorStale,extended,expired,boundary,overviewHidden:overviewHidden&&noReplay,singleHidden,noWindowTransport,unmounted,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('desktop_hud driver: '+(error.stack||error))),150);
"#;
