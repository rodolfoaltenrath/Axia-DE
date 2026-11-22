//! Native desktop revisions, expiry and the boundary shortcut through the real host.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn desktop_indicator_uses_native_state_and_expiry() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "desktop_hud", DRIVER, mode, width, "desktop-hud.png");
            for flag in [
                "bound",
                "initiallyHidden",
                "direct",
                "mirrorStale",
                "extended",
                "expired",
                "boundary",
                "nativeCommands",
                "nativeSwipes",
                "started",
                "nativeCount",
                "handoff",
                "positions",
                "remounted",
                "withoutHud",
                "overviewHidden",
                "singleHidden",
                "noWindowTransport",
                "nativePreferences",
                "noJsPersistence",
                "unmounted",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {setDesktops,setActiveDesktop,activeDesktop,desktopList,windows} from '@ROOT@/sdk/windows.js';
import {DesktopHud,nextDesktopBy,addDesktopAndShow,closeActiveDesktop,startDesktops,desktopAt,activeIndex,indexOfDesktop,desktopOfSurface,desktopsForThemeSwitch,desktopSwipe} from '@ROOT@/sdk/desktops.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();
settings.set({desktopCount:1,themeDesktops:null});
const nativeSettings=lord.settings.state;
const profile=()=>nativeSettings.call(lord.settings,{cmd:'snapshot'}).state;
nativeSettings.call(lord.settings,{cmd:'patch',patch:{desktopCount:3,themeDesktops:{active:0,surfaces:{71:1,72:2}}}});
const nativePreferences=settings.get().desktopCount===1&&profile().desktopCount===3;
let jsPersistence=0;
lord.settings.state=input=>{if(input.cmd==='patch'&&['desktopCount','themeDesktops'].some(key=>key in(input.patch||{}))){jsPersistence++;throw Error('desktops wrote preferences through JS');}return nativeSettings.call(lord.settings,input);};
startDesktops();
const started=settings.get().themeDesktops===null&&desktopList.get().length===3&&activeDesktop.get()===1;
setDesktops([10,20,30],10);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
const source=lord.windowList,mount=lord.nativeViews.mount,send=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
let handle=0,bound=false,legacyObservers=0,noWindowTransport=true,unmounted=false,hide,remount,mounts=0;
const request=lord.nativeApps.request;
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='desktopCommand')noWindowTransport&&=!('active'in input)&&!('desktops'in input)&&!('id'in input);if(input.cmd==='desktopSession')noWindowTransport&&=!('settings'in input)&&!('windows'in input)&&!('desktops'in input)&&!('active'in input);return request.call(lord.nativeApps,text);};
function inspect(text){const input=JSON.parse(text);noWindowTransport&&=!('windows'in input)&&!('desktops'in(input.args||{}))&&!('active'in(input.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:desktops')legacyObservers++;if(id==='shell:desktop-hud'){inspect(text);bound=sources[0]===source;mounts++;handle=mount(node,id,text,...sources);return handle;}return mount(node,id,text,...sources);};
lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return send(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounted=true;return unmount(id);};
function App(){const[on,setOn]=useState(true),[rev,setRev]=useState(0);hide=()=>setOn(false);remount=()=>setRev(n=>n+1);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},on?h(DesktopHud,{key:rev}):null));}
render(h(App));
const text=()=>lord.nativeViews.text(handle)||'';
const direct=(op,...args)=>source.call(op,JSON.stringify([{},...args]));
async function run(){
 await until(()=>handle>0);const initiallyHidden=!text().includes('Desktop');
 direct('setDesktops',[10,20,30,40],10);
 const nativeCount=await until(()=>settings.get().desktopCount===4)&&desktopList.get().length===3;
 direct('open',{app:'linux',instance:'71',desktop:30,args:{surface:71}});direct('setActiveDesktop',20);
 const get=windows.get;windows.get=()=>{throw Error('desktop persistence read the JS window mirror');};
 let handoff=false,positions=false;
 try{
   const kept=desktopsForThemeSwitch();handoff=kept.active===1&&kept.surfaces['71']===2&&profile().themeDesktops.active===1&&profile().themeDesktops.surfaces['71']===2;
   positions=activeIndex()===1&&indexOfDesktop(30)===2&&indexOfDesktop(999)===0&&desktopOfSurface(71)===20&&desktopOfSurface(72)===30&&desktopOfSurface(999)===undefined;
 }finally{windows.get=get;}
 positions&&=desktopAt(5)===42;
 await until(()=>settings.get().desktopCount===6);
 const before=mounts;remount();await until(()=>mounts>before);direct('createDesktop');
 const remounted=await until(()=>settings.get().desktopCount===7);
 startDesktops();const preserved=JSON.parse(direct('resync')).state;
 positions&&=preserved.desktops.length===7&&preserved.active===20&&preserved.windows.some(w=>w.id==='linux#71');
 unmounted=false;direct('close','linux#71');setDesktops([10,20,30],10);await until(()=>settings.get().desktopCount===3);await sleep(850);
 direct('setActiveDesktop',20);const directShown=await until(()=>text().includes('Desktop 2'));
 const mirrorStale=activeDesktop.get()===10;
 await sleep(450);direct('setActiveDesktop',30);await until(()=>text().includes('Desktop 3'));await sleep(450);
 const extended=text().includes('Desktop 3');
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 const expired=await until(()=>!text().includes('Desktop'));
 nextDesktopBy(1);const boundary=await until(()=>text().includes('Desktop 3'));
 nextDesktopBy(-1);
 const state=()=>JSON.parse(direct('resync')).state;
 const stepped=state().active===20;
 direct('setActiveDesktop',30);direct('open',{app:'notes',instance:'moved'});
 const closed=closeActiveDesktop(),afterClose=state();
 const moved=afterClose.windows.find(w=>w.id==='notes#moved');
 const added=addDesktopAndShow(),afterAdd=state();
 const nativeCommands=stepped&&closed===true&&afterClose.active===20&&afterClose.desktops.length===2&&moved?.w.desktop===20&&added===21&&afterAdd.active===21;
 direct('setOverview',true);const overviewHidden=await until(()=>!text().includes('Desktop'));
 direct('setOverview',false);await sleep(50);const noReplay=!text().includes('Desktop');
 direct('setDesktops',[10],10);lord.nativeViews.action(handle,{action:['show']});await sleep(50);
 const singleHidden=!text().includes('Desktop');hide();await until(()=>unmounted);
 // The mirror still has multiple desktops; native state has one and must cycle windows.
 direct('close','notes#moved');direct('open',{app:'first'});direct('open',{app:'second'});
 const listGet=desktopList.get;desktopList.get=()=>{throw Error('swipe read desktop mirror');};
 let nativeSwipes=false;
 try{
   desktopSwipe('left')();const first=state().windows.at(-1)?.id==='first';
   direct('setDesktops',[10,20],10);desktopSwipe('left')();const switched=state().active===20;
   desktopSwipe('left')();const boundary=state().active===20;
   locale.set({...locale.get(),dir:'rtl'});desktopSwipe('left')();const rtl=state().active===10;
   locale.set({...locale.get(),dir:'ltr'});nativeSwipes=first&&switched&&boundary&&rtl;
 }finally{desktopList.get=listGet;}
 direct('setDesktops',[10],10);
 direct('createDesktop');const withoutHud=legacyObservers===0&&await until(()=>settings.get().desktopCount===2);
 console.log('desktop_hud-observation '+JSON.stringify({bound,initiallyHidden,started,nativeCount,handoff,positions,remounted,withoutHud,direct:directShown,mirrorStale,extended,expired,boundary,nativeCommands,nativeSwipes,overviewHidden:overviewHidden&&noReplay,singleHidden,noWindowTransport,unmounted,nativePreferences,noJsPersistence:jsPersistence===0,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('desktop_hud driver: '+(error.stack||error))),150);
"#;
