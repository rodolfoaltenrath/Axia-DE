//! Native session mutations through the real QuickJS controller and SDK adapter.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn session_window_commands_use_current_native_membership() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(
                repo,
                "window_commands",
                DRIVER,
                mode,
                width,
                "window-commands.png",
            );
            for flag in [
                "stale",
                "cycle",
                "back",
                "hideOthers",
                "hide",
                "showAll",
                "desktop",
                "restore",
                "separateDesktops",
                "linuxGrouping",
                "shortcuts",
                "pip",
                "metadataCached",
                "noWindowTransport",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {configureWindows,registerBuiltinApp,wm,setDesktops,setActiveDesktop} from '@ROOT@/sdk/windows.js';
import {cycleAppWindows,hideApp,hideOthers,showAllApps,toggleShowDesktop} from '@ROOT@/sdk/appcontrol.js';
import {AppSwitcher,openSwitcher,cancelSwitcher} from '@ROOT@/sdk/appswitcher.jsx';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
locale.set({...locale.get(),lang:'en'});settings.set({shortcutProfile:'macos'});
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:['notes','calculator','forcequit'].map(id=>({id,name:id==='notes'?'Notes':id,Component:()=>null,Icon}))});
for(const id of ['linux','panel'])registerBuiltinApp({id,name:id,builtin:true,Component:()=>null,Icon});
const request=lord.nativeApps.request.bind(lord.nativeApps),requests=[];
lord.nativeApps.request=text=>{requests.push(JSON.parse(text));return request(text);};
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const minimized=id=>{const w=state().windows.find(w=>w.id===id);return w?.w?.minimized===true;};
const focused=()=>state().views.front;
let handle=0;const mount=lord.nativeViews.mount;
lord.nativeViews.mount=(node,id,text,...source)=>{const value=mount(node,id,text,...source);if(id==='shell:switcher')handle=value;return value;};
resetShortcuts();
function Harness(){useShortcuts({grabs:'flow',onSuperTap:()=>{}});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'azul',effects:{}})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},h(AppSwitcher)));}
render(h(Harness));
async function run(){
 setDesktops([1,2],1);const panel=wm.open('panel'),a=wm.open('notes',{instance:'a'}),b=wm.open('notes',{instance:'b'}),calc=wm.open('calculator');
 raw('open',{app:'notes',instance:'fresh'});const stale=!wm.get('notes#fresh')&&wm.focused()===calc;
 cycleAppWindows();const cycle=focused()===a&&wm.focused()===a&&!!wm.get('notes#fresh');
 const metadataBefore=requests.filter(input=>input.cmd==='metadata').length;
 cycleAppWindows(true);const back=focused()==='notes#fresh';
 hideOthers();const others=minimized(calc)&&!minimized(a)&&!minimized(b)&&!minimized(panel);
 const force=wm.open('forcequit'),pipWindow=wm.open('linux',{instance:'pip',args:{appId:'video-pip'}});
 raw('setPip',pipWindow,{x:5,y:5,w:160,h:100});wm.focus('notes#fresh');hideOthers();const hideOthersResult=others&&minimized(force);
 const pipHidden=minimized(pipWindow);
 hideApp();const hide=[a,b,'notes#fresh'].every(minimized)&&!minimized(panel);
 showAllApps();const showAll=[a,b,'notes#fresh',calc,force].every(id=>!minimized(id));
 wm.minimize(calc);wm.minimize(force);toggleShowDesktop();
 const desktop=[a,b,'notes#fresh'].every(minimized)&&!minimized(panel);
 const pip=pipHidden&&!minimized(pipWindow);
 raw('close',a);raw('moveToDesktop','notes#fresh',2);toggleShowDesktop();
 const restore=!minimized(b)&&minimized(calc)&&minimized('notes#fresh')&&focused()===b;
 setActiveDesktop(2);const away=wm.open('notes',{instance:'away'});toggleShowDesktop();toggleShowDesktop();
 const separateDesktops=!minimized(away)&&minimized('notes#fresh')&&!minimized(b);
 setActiveDesktop(1);wm.focus(b);
 // Mac profile Meta+H must resolve the current owner in Rust, even with a stale mirror.
 raw('open',{app:'calculator',instance:'native'});
 __lordKey(35,1,'h','h','h',16);__lordKey(35,0,'','','',16);__lordKey(125,0,'Meta','','',0);
 const shortcuts=minimized('calculator#native')&&!minimized(b);
 const one=wm.open('linux',{instance:11,args:{appId:'org.test.terminal'}}),two=wm.open('linux',{instance:12,args:{appId:'org.test.terminal'}});
 cycleAppWindows();const linuxGrouping=focused()===one;hideApp(two);
 const grouped=linuxGrouping&&minimized(one)&&minimized(two)&&!minimized(b);
 const metadataCached=requests.filter(input=>input.cmd==='metadata').length===metadataBefore;
 const noWindowTransport=requests.filter(input=>input.cmd==='windowCommand').every(input=>!('windows'in input)&&!('ids'in input)&&!('groups'in input)&&!('catalog'in input)&&!('registry'in input));
 wm.focus(b);openSwitcher(8);await until(()=>handle&&lord.nativeViews.text(handle));await sleep(100);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);cancelSwitcher();
 console.log('window_commands-observation '+JSON.stringify({stale,cycle,back,hideOthers:hideOthersResult,hide,showAll,desktop,restore,separateDesktops,linuxGrouping:grouped,shortcuts,pip,metadataCached,noWindowTransport,saved:!!saved?.ok,requests}));
}
setTimeout(()=>run().catch(error=>console.error('window_commands driver: '+(error.stack||error))),200);
"#;
