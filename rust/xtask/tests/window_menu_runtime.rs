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
            let report = ui::run_ui_with_capture(repo, "window_menu", DRIVER, mode, width, "window-menu.png", |path| {
                let pixels = image::open(path).unwrap().into_rgb8();
                let ink = if mode == "light" { [29,29,31] } else { [245,245,247] };
                assert!(pixels.pixels().filter(|p| p.0 == ink).count() > 20,
                    "current shell ink was not painted: {mode}/{width}");
            });
            for flag in [
                "noDisplayTransport",
                "physicalDisplays",
                "noAppearanceFacts",
                "withoutAppearanceFacts",
                "staleAppearanceMirror",
                "appearanceRetained",
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
                "nativeDesktopMoves",
                "nativeStateActions",
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
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {locale,t} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const opposite='@MODE@'==='light'?'dark':'light';
settings.set({language:'en',mode:opposite,accent:'azul',effects:true,nativeAppearance:'axia'});
let noAppearanceFacts=true,appearanceFacts=0,lastAppearanceFacts;
const appearanceSend=lord.nativeViews.facts;
function inspectAppearance(raw){const input=JSON.parse(raw);lastAppearanceFacts=input;noAppearanceFacts&&=!('theme'in(input.context||{}))&&!('scale'in(input.context||{}));}
async function changeAppearance(){
 await sleep(250);const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 const before=appearanceFacts;
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',effects:false,nativeAppearance:'adwaita'}});await sleep(200);
 const withoutAppearanceFacts=appearanceFacts===before,staleAppearanceMirror=settings.get().mode===opposite;
 appearanceSend(handle,JSON.stringify({...lastAppearanceFacts,args:{...lastAppearanceFacts.args,displays:[{id:90,name:'Forged first',x:-10000,y:0,w:100,h:700},{id:91,name:'Forged second',x:0,y:0,w:10000,h:700}]},context:{...lastAppearanceFacts.context,theme:{mode:opposite,menuText:'#ff0000',effects:true},scale:{body:200}}}));
 await sleep(100);settingsBridge.perform=perform;
 return {withoutAppearanceFacts,staleAppearanceMirror};
}
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:['notes','calculator'].map(id=>({id,name:id==='notes'?'Notes':'Calculator',Component:()=>null,Icon}))});
registerBuiltinApp({id:'panel',name:'Hidden panel',builtin:true,Component:()=>null,Icon});
let handle=0,bound=false,noWindowTransport=true,noDisplayTransport=true,unmounted=false,hideBar;
const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
function inspect(text){const input=JSON.parse(text);noWindowTransport&&=!('windows'in input)&&!('windowRows'in input.args);noDisplayTransport&&=!('displays'in input.args);}
lord.nativeViews.mount=(node,id,text,...source)=>{const value=mount(node,id,text,...source);if(id==='shell:menubar'){handle=value;bound=source[0]===lord.windowList;inspect(text);inspectAppearance(text);}return value;};
lord.nativeViews.facts=(id,text)=>{if(id===handle){inspect(text);inspectAppearance(text);appearanceFacts++;}return facts(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounted=true;return unmount(id);};
let jsRows=0,help=0,prepared=0;
barBridge.menus={get window(){jsRows++;throw Error('Window rows reached JS');},help:{items:[{label:'Legacy help',onClick:()=>help++}]}};
barBridge.prepare=()=>prepared++;
function Harness(){const[shown,setShown]=useState(true);hideBar=()=>setShown(false);
 return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'azul',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee',padding:20}},
 shown&&h(NativeView,{manifest:NATIVE_MENUBAR,stateSource:lord.windowList,args:{bar:{items:[{id:'window',label:'Window'},{id:'help',label:'Help'}],height:28}},style:{width:@WIDTH@-40,height:28,flex:0}})));}
render(h(Harness));
const has=label=>!!lord.nativeViews.findText(handle,label);
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const win=id=>state().windows.find(w=>w.id===id)?.w;
function point(label,type){const node=lord.nativeViews.findText(handle,label);if(!node)return;
 const[x,y,w,h]=lord.getLayout(node),owner=lord.nativeViews.pointer(type,node,x+w/2,y+h/2,0);__lordEvent(type,owner||0,x+w/2,y+h/2,0);__lordNativeView();}
function click(label){point(label,'down');point(label,'up');}
let legacyStateActions=0,checkingStateAction=false;
for(const name of ['focus','minimize','toggleMaximize','moveToDesktop']){
 const original=wm[name];wm[name]=(...args)=>{if(checkingStateAction)legacyStateActions++;return original(...args);};
}
const request=lord.nativeApps.request;
lord.nativeApps.request=function(text){const input=JSON.parse(text);if(checkingStateAction&&input.cmd==='desktopSession'&&['move','moveNew'].includes(input.action))legacyStateActions++;return request.call(this,text);};
function nativeClick(label){checkingStateAction=true;try{click(label);}finally{checkingStateAction=false;}}

async function open(){click('Window');await until(()=>has(t('shell.window.minimize')));}
async function run(){
 setDesktops([1,2],1);const a=wm.open('notes',{instance:'a'}),calc=wm.open('calculator'),away=wm.open('notes',{instance:'away',desktop:2});wm.open('panel');
 wm.setTitle(a,'Alpha');wm.setTitle(calc,'Beta');wm.setTitle(away,'Away');wm.focus(calc);
 raw('open',{app:'notes',instance:'native'});raw('patch','notes#native',{title:'Native title'});const rawOnly=!wm.get('notes#native');
 await until(()=>handle&&has('Window'));await open();
 const appearance=await changeAppearance();
 const physicalDisplays=!has('Move to Forged first (1)')&&!has('Move to Forged second (2)');
 const appearanceRetained=has('Alpha')&&has('Native title');
 const initial=rawOnly&&has('Native title')&&has('Alpha')&&!has('Hidden panel');
 raw('patch','notes#native',{title:'Fresh title'});const directTitle=await until(()=>has('Fresh title')&&!has('Native title'));
 raw('open',{app:'notes',instance:'temporary'});raw('patch','notes#temporary',{title:'Added title'});const added=await until(()=>has('Added title'));
 raw('close','notes#temporary');const directMembership=added&&await until(()=>!has('Added title'));
 const suffix='Away · '+t('shell.desktops.name',{n:2}),desktopSuffix=has(suffix);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 nativeClick(suffix);const focusDesktop=await until(()=>state().active===2&&state().views.front===away);
 setActiveDesktop(1);wm.focus('notes#native');await open();nativeClick(t('shell.window.minimize'));
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
 const call=lord.windowList.call;let legacyMoves=0;
 lord.windowList.call=function(op,...args){if(op==='createDesktop'||op==='moveToDesktop')legacyMoves++;return call.call(this,op,...args);};
 let nativeDesktopMoves=false;
 try{
   wm.focus(a);await open();point(t('shell.desktops.moveTo'),'move');await until(()=>has(t('shell.desktops.name',{n:2})));
   const beforeMove=state();nativeClick(t('shell.desktops.name',{n:2}));
   const moved=await until(()=>win(a)?.desktop===2);
   const afterMove=state();
   wm.focus(a);await open();point(t('shell.desktops.moveTo'),'move');await until(()=>has(t('shell.desktops.new')));
   const beforeNew=state();nativeClick(t('shell.desktops.new'));
   const created=await until(()=>state().desktops.length===beforeNew.desktops.length+1);
   const afterNew=state();
   nativeDesktopMoves=moved&&created&&afterMove.active===beforeMove.active&&afterMove.desktops.length===beforeMove.desktops.length
     &&win(a)?.desktop===afterNew.desktops.at(-1)&&afterNew.active===beforeNew.active&&legacyMoves===0;
 }finally{lord.windowList.call=call;}
 wm.focus(a);await open();nativeClick(t('shell.window.zoom'));const zoomed=await until(()=>win(a)?.maximized===true);
 await open();nativeClick(t('shell.window.zoom'));const restored=await until(()=>win(a)?.maximized===false);
 const nativeStateActions=zoomed&&restored&&legacyStateActions===0;


 click('Help');await until(()=>has('Legacy help'));click('Legacy help');const legacyMenu=help===1;
 hideBar();await until(()=>unmounted&&!lord.nativeViews.text(handle));
 console.log('window_menu-observation '+JSON.stringify({noDisplayTransport,physicalDisplays,noAppearanceFacts,...appearance,appearanceRetained,bound,initial,directTitle,directMembership,desktopSuffix,focusDesktop,rawMinimize,freshArrange,bringAll,staleAction,nativeDesktopMoves,nativeStateActions,legacyMenu,noJsRows:jsRows===0,noWindowTransport,unmounted,saved:!!saved?.ok,prepared}));
}
setTimeout(()=>run().catch(error=>console.error('window_menu driver: '+(error.stack||error))),200);
"#;
