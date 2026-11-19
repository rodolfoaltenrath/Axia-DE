//! The real menu bar reads WindowState, builds native rows and sends only commands.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn native_window_menu_reads_current_titles_and_executes_live_actions() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "window_menu", DRIVER, mode, width, "window-menu.png");
            for flag in [
                "bound",
                "initial",
                "directTitle",
                "directMembership",
                "desktopSuffix",
                "focusDesktop",
                "rawMinimize",
                "freshArrange",
                "bringAll",
                "staleAction",
                "legacyMenu",
                "noJsRows",
                "noWindowTransport",
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
import {render,View,screen} from '@lordos/runtime';
import {configureWindows,registerBuiltinApp,wm,setDesktops,setActiveDesktop} from '@ROOT@/sdk/windows.js';
import {NativeView} from '@ROOT@/sdk/nativeview.js';
import {NATIVE_MENUBAR,barBridge} from '@ROOT@/sdk/nativemenubar.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {locale,t} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:['notes','calculator'].map(id=>({id,name:id==='notes'?'Notes':'Calculator',Component:()=>null,Icon}))});
registerBuiltinApp({id:'panel',name:'Hidden panel',builtin:true,Component:()=>null,Icon});
let handle=0,bound=false,noWindowTransport=true,unmounted=false,hideBar;
const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
function inspect(text){const input=JSON.parse(text);noWindowTransport&&=!('windows'in input)&&!('windowRows'in input.args);}
lord.nativeViews.mount=(node,id,text,...source)=>{const value=mount(node,id,text,...source);if(id==='shell:menubar'){handle=value;bound=source[0]===lord.windowList;inspect(text);}return value;};
lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return facts(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounted=true;return unmount(id);};
let jsRows=0,help=0,prepared=0;
barBridge.menus={get window(){jsRows++;throw Error('Window rows reached JS');},help:{items:[{label:'Legacy help',onClick:()=>help++}]}};
barBridge.prepare=()=>prepared++;
function Harness(){const[shown,setShown]=useState(true);hideBar=()=>setShown(false);
 return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'azul',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee',padding:20}},
 shown&&h(NativeView,{manifest:NATIVE_MENUBAR,stateSource:lord.windowList,args:{bar:{items:[{id:'window',label:'Window'},{id:'help',label:'Help'}],height:28},displays:screen.displays},style:{width:@WIDTH@-40,height:28,flex:0}})));}
render(h(Harness));
const has=label=>!!lord.nativeViews.findText(handle,label);
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const win=id=>state().windows.find(w=>w.id===id)?.w;
function point(label,type){const node=lord.nativeViews.findText(handle,label);if(!node)return;
 const[x,y,w,h]=lord.getLayout(node),owner=lord.nativeViews.pointer(type,node,x+w/2,y+h/2,0);__lordEvent(type,owner||0,x+w/2,y+h/2,0);__lordNativeView();}
function click(label){point(label,'down');point(label,'up');}
async function open(){click('Window');await until(()=>has(t('shell.window.minimize')));}
async function run(){
 setDesktops([1,2],1);const a=wm.open('notes',{instance:'a'}),calc=wm.open('calculator'),away=wm.open('notes',{instance:'away',desktop:2});wm.open('panel');
 wm.setTitle(a,'Alpha');wm.setTitle(calc,'Beta');wm.setTitle(away,'Away');wm.focus(calc);
 raw('open',{app:'notes',instance:'native'});raw('patch','notes#native',{title:'Native title'});const rawOnly=!wm.get('notes#native');
 await until(()=>handle&&has('Window'));await open();
 const initial=rawOnly&&has('Native title')&&has('Alpha')&&!has('Hidden panel');
 raw('patch','notes#native',{title:'Fresh title'});const directTitle=await until(()=>has('Fresh title')&&!has('Native title'));
 raw('open',{app:'notes',instance:'temporary'});raw('patch','notes#temporary',{title:'Added title'});const added=await until(()=>has('Added title'));
 raw('close','notes#temporary');const directMembership=added&&await until(()=>!has('Added title'));
 const suffix='Away · '+t('shell.desktops.name',{n:2}),desktopSuffix=has(suffix);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 click(suffix);const focusDesktop=await until(()=>state().active===2&&state().views.front===away);
 setActiveDesktop(1);wm.focus('notes#native');await open();click(t('shell.window.minimize'));
 const rawMinimize=await until(()=>win('notes#native')?.minimized===true);
 wm.focus(calc);raw('open',{app:'notes',instance:'latest'});raw('patch','notes#latest',{title:'Latest'});await open();
 point(t('shell.window.moveResize'),'move');await until(()=>has(t('shell.window.arrange.leftRight')));
 raw('close',calc);await until(()=>!has('Beta'));point(t('shell.window.moveResize'),'move');await sleep(50);
 click(t('shell.window.arrange.leftRight'));
 const freshArrange=await until(()=>win('notes#latest')?.snapped==='left'&&win(a)?.snapped==='right'&&!win(calc)&&!win(away)?.snapped);
 await open();click(t('shell.window.bringAll'));const bringAll=await until(()=>state().views.front==='notes#latest'&&win('notes#native')?.minimized===true);
 await open();raw('close','notes#latest');let closes=0;const close=wm.close;wm.close=(...args)=>{closes++;return close(...args);};
 lord.nativeViews.action(handle,{action:['windowMenu',{action:'close',id:'notes#latest'}]});__lordNativeView();wm.close=close;
 const staleAction=closes===0&&!!win(a);
 click('Help');await until(()=>has('Legacy help'));click('Legacy help');const legacyMenu=help===1;
 hideBar();await until(()=>unmounted&&!lord.nativeViews.text(handle));
 console.log('window_menu-observation '+JSON.stringify({bound,initial,directTitle,directMembership,desktopSuffix,focusDesktop,rawMinimize,freshArrange,bringAll,staleAction,legacyMenu,noJsRows:jsRows===0,noWindowTransport,unmounted,saved:!!saved?.ok,prepared}));
}
setTimeout(()=>run().catch(error=>console.error('window_menu driver: '+(error.stack||error))),200);
"#;
