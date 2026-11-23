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
                "physicalCycle",
                "physicalMinimize",
                "physicalHideOthers",
                "physicalDesktop",
                "nativeProfile",
                "nativeKeyOwnership",
                "nativeMirror",
                "switcherMounted",
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
import {openSwitcher,cancelSwitcher} from '@ROOT@/sdk/appswitcher.jsx';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
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
let handle=0,probingKeys=false,superTaps=0;const jsKeys=[];
const dispatchKey=globalThis.__lordKey;
globalThis.__lordKey=(...args)=>{if(probingKeys)jsKeys.push([args[0],args[1]]);return dispatchKey(...args);};
const input=(code,value)=>{if(!lord.nativeViews.inputKey(code,value))throw Error('physical input rejected '+code);};
const chord=(modifiers,code)=>{for(const mod of modifiers)input(mod,1);input(code,1);input(code,0);for(const mod of [...modifiers].reverse())input(mod,0);};
resetShortcuts();
function Harness(){useShortcuts({grabs:'flow',onSuperTap:()=>{superTaps++;}});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'azul',effects:{}})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}}));}
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
 const physicalRequests=requests.length;probingKeys=true;
 chord([125],35);
 const shortcuts=minimized('calculator#native')&&!minimized(b);
 raw('focus',b);chord([125],41);const forward=focused()===b;
 // Only b is visible in this group on desktop 1: unminimize another member, then cycle both ways.
 raw('focus','notes#fresh');raw('moveToDesktop','notes#fresh',1);raw('focus',b);
 chord([125],41);const next=focused()==='notes#fresh';chord([125,42],41);
 const physicalCycle=forward&&next&&focused()===b;
 input(125,1);input(50,1);const minimizedOnly=minimized(b)&&!minimized('notes#fresh');
 raw('focus','notes#fresh');input(125,0);input(50,2);input(50,0);
 const physicalMinimize=minimizedOnly&&!minimized('notes#fresh');
 raw('focus',calc);chord([125,56],35);
 const physicalHideOthers=minimized('notes#fresh')&&!minimized(calc)&&!minimized(panel);
 const pipBeforeDesktop=minimized(pipWindow);
 chord([125],87);const hiddenDesktop=minimized(calc)&&!minimized(panel)&&minimized(pipWindow)===pipBeforeDesktop;
 chord([125],87);const physicalDesktop=hiddenDesktop&&!minimized(calc)&&minimized('notes#fresh');
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 lord.settings.state({cmd:'patch',patch:{shortcutProfile:'linux'}});
 const staleProfile=settings.get().shortcutProfile==='macos';
 raw('focus','calculator#native');chord([125],35);
 const linuxMinimize=minimized('calculator#native')&&!minimized(calc);
 raw('focus','calculator#native');chord([56],41);const linuxCycle=focused()===calc;
 chord([125],32);const linuxDesktop=minimized(calc)&&minimized('calculator#native');
 chord([125],32);
 const nativeProfile=staleProfile&&linuxMinimize&&linuxCycle&&linuxDesktop&&!minimized(calc)&&!minimized('calculator#native');
 const nativeMirror=await until(()=>wm.focused()===focused()&&wm.get('notes#fresh')?.minimized===true&&wm.get(calc)?.minimized===false);
 lord.settings.state({cmd:'patch',patch:{shortcutProfile:'macos'}});settingsBridge.perform=perform;
 probingKeys=false;
 const nativeKeyOwnership=superTaps===0&&jsKeys.every(([code,value])=>[125,56,42].includes(code)&&value===1)&&
   requests.slice(physicalRequests).every(input=>!['windowCommand','switcherPrepare','switcherKey'].includes(input.cmd));
 wm.focus(b);
 const one=wm.open('linux',{instance:11,args:{appId:'org.test.terminal'}}),two=wm.open('linux',{instance:12,args:{appId:'org.test.terminal'}});
 cycleAppWindows();const linuxGrouping=focused()===one;hideApp(two);
 const grouped=linuxGrouping&&minimized(one)&&minimized(two)&&!minimized(b);
 const metadataCached=requests.filter(input=>input.cmd==='metadata').length===metadataBefore;
 const noWindowTransport=requests.filter(input=>input.cmd==='windowCommand').every(input=>!('windows'in input)&&!('ids'in input)&&!('groups'in input)&&!('catalog'in input)&&!('registry'in input));
 wm.focus(b);openSwitcher(8);const switcherMounted=await until(()=>{handle=request(JSON.stringify({cmd:'switcherStatus'})).handle;return handle&&lord.nativeViews.text(handle);});await sleep(100);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);cancelSwitcher();
 console.log('window_commands-observation '+JSON.stringify({stale,cycle,back,hideOthers:hideOthersResult,hide,showAll,desktop,restore,separateDesktops,linuxGrouping:grouped,shortcuts,physicalCycle,physicalMinimize,physicalHideOthers,physicalDesktop,nativeProfile,nativeKeyOwnership,nativeMirror,switcherMounted,jsKeys,pip,metadataCached,noWindowTransport,saved:!!saved?.ok,requests}));
}
setTimeout(()=>run().catch(error=>console.error('window_commands driver: '+(error.stack||error))),200);
"#;
