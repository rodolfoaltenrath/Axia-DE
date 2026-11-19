//! Native group resolution and graceful quit through real menu and shortcut adapters.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn app_menus_and_quit_use_live_native_groups() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "app_menus", DRIVER, mode, width, "app-menu.png");
            for flag in [
                "nativeGroup",
                "metadataOnly",
                "compatibility",
                "freshQuit",
                "guards",
                "dockQuit",
                "freshSurface",
                "packageAccepted",
                "packageFallback",
                "shortcut",
                "metadataCached",
                "noWindowTransport",
                "noForceQuit",
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
import {configureWindows,registerBuiltinApp,wm,setDesktops,setActiveDesktop} from '@ROOT@/sdk/windows.js';
import {appMenuItems,appGroup,quitAppGroup} from '@ROOT@/sdk/appcontrol.js';
import {dockMenu} from '@ROOT@/sdk/dock.js';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {Menu} from '@ROOT@/sdk/menu.jsx';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {locale,t} from '@ROOT@/sdk/i18n.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
locale.set({...locale.get(),lang:'en'});settings.set({shortcutProfile:'macos'});
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:[{id:'notes',name:'Notes',Component:()=>null,Icon},{id:'calculator',name:'Calculator',Component:()=>null,Icon},
 {id:'pkg',name:'Package',isolated:true,Component:()=>null,Icon}]});
registerBuiltinApp({id:'linux',name:'Linux',builtin:true,Component:()=>null,Icon});
const request=lord.nativeApps.request.bind(lord.nativeApps),requests=[],replies=[];
lord.nativeApps.request=text=>{const input=JSON.parse(text),reply=request(text);requests.push(input);if(input.cmd==='groupInfo')replies.push(reply);return reply;};
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const exists=id=>state().windows.some(w=>w.id===id);
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
function click(label){const node=find(label);if(!node)return;const[x,y,w,h]=lord.getLayout(node);
 for(const type of ['down','up']){const mount=lord.nativeViews.pointer(type,node,x+w/2,y+h/2,0);__lordEvent(type,mount,x+w/2,y+h/2,0);}__lordNativeView();}
let show,shown=false;
resetShortcuts();enableShellLayers();
function Harness(){const[items,setItems]=useState(null);show=items=>{shown=!!items;setItems(items);};useShortcuts({grabs:'flow',onSuperTap:()=>{}});
 return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'azul',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},
 items&&h(Menu,{x:40,y:60,items,onClose:()=>show(null)})));}
render(h(Harness));
async function run(){
 setDesktops([1,2],1);const a=wm.open('notes',{instance:'a'}),b=wm.open('notes',{instance:'b'}),calc=wm.open('calculator');
 raw('open',{app:'notes',instance:'menu'});const stale=!wm.get('notes#menu');
 const items=appMenuItems('notes#menu'),quitLabel=t('shell.app.quit',{app:'Notes'});
 const nativeGroup=stale&&items.some(item=>item?.label===quitLabel);
 const legacy=appGroup('notes#menu');const compatibility=legacy?.key==='notes'&&legacy.windows.every(w=>w&&typeof w==='object'&&typeof w.id==='string')&&appGroup()===null&&appMenuItems().length===0;
 let guarded=0;wm.setCloseGuard(a,()=>{guarded++;return false;});wm.moveToDesktop(a,2);setActiveDesktop(1);
 show(items);await until(()=>find(quitLabel));raw('open',{app:'notes',instance:'after-menu'});raw('close',b);click(quitLabel);await until(()=>!shown);
 const freshQuit=!exists('notes#menu')&&!exists('notes#after-menu')&&!exists(b)&&exists(calc);
 const guards=guarded===1&&exists(a)&&state().active===2;
 wm.close(a,true);setActiveDesktop(1);
 let kills=0,closes=[];const close=lord.wayland.close,kill=lord.wayland.kill,brokerQuit=lord.broker.quit;
 lord.wayland.close=id=>{closes.push(id);};lord.wayland.kill=()=>{kills++;return '';};
 const linux=wm.open('linux',{instance:10,args:{appId:'editor-test',surface:20}});
 const dockItems=dockMenu({id:'linux:editor-test',launcherId:null,windows:[wm.get(linux)],app:null});
 raw('setArgs',linux,{surface:202});raw('open',{app:'linux',instance:11,args:{appId:'editor-test',surface:22}});wm.focus(calc);
 dockItems.find(item=>item?.label===t('shell.dock.quit'))?.onClick();
 const dockQuit=exists(calc)&&state().views.front===calc;
 const freshSurface=closes.length===2&&closes[0]===202&&closes[1]===22;
 raw('open',{app:'linux',instance:30,args:{appId:'pkg',pkg:true,surface:303}});
 const before=closes.length,packages=[];lord.broker.quit=id=>{packages.push(id);return true;};quitAppGroup({key:'pkg'});
 const packageAccepted=packages.length===1&&packages[0]==='pkg'&&closes.length===before&&exists('linux#30');
 lord.broker.quit=id=>{packages.push(id);raw('setArgs','linux#30',{surface:333});return false;};quitAppGroup({key:'pkg'});
 const packageFallback=packages.length===2&&closes.length===before+1&&closes[closes.length-1]===333;
 wm.focus(calc);raw('open',{app:'notes',instance:'shortcut'});
 __lordKey(16,1,'q','q','q',16);__lordKey(16,0,'','','',16);__lordKey(125,0,'Meta','','',0);
 const shortcut=!exists('notes#shortcut')&&exists(calc);
 lord.wayland.close=close;lord.wayland.kill=kill;lord.broker.quit=brokerQuit;
 const noForceQuit=kills===0&&!requests.some(input=>input.cmd==='quit');
 const metadataOnly=replies.length>0&&replies.every(reply=>reply.group&&!('windows'in reply.group));
 const metadataCached=requests.filter(input=>input.cmd==='metadata').length===1;
 const noWindowTransport=requests.filter(input=>['groupInfo','quitApp','quitAppNext'].includes(input.cmd)).every(input=>!('windows'in input)&&!('ids'in input)&&!('groups'in input)&&!('registry'in input)&&!('catalog'in input));
 wm.focus(calc);show(appMenuItems(calc));await until(()=>find(t('shell.app.quit',{app:'Calculator'})));await sleep(100);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('app_menus-observation '+JSON.stringify({nativeGroup,metadataOnly,compatibility,freshQuit,guards,dockQuit,freshSurface,packageAccepted,packageFallback,shortcut,metadataCached,noWindowTransport,noForceQuit,saved:!!saved?.ok,closes,packages,requests}));
}
setTimeout(()=>run().catch(error=>console.error('app_menus driver: '+(error.stack||error))),200);
"#;
