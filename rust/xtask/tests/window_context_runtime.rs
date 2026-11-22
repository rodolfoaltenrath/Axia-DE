//! Own-frame menus read native rows, placement and checks through the actual theme.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and bundles; required by GPU=1 desktop CI"]
fn own_frame_window_context_reads_native_state_and_preserves_commands() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light","dark"] {
        for width in [640,1000] {
            let report = ui::run_ui_with_capture(repo,"window_context",DRIVER,mode,width,"window-context.png",|path| {
                let pixels = image::open(path).unwrap().into_rgb8();
                let ink = if mode=="light" { [29,29,31] } else { [245,245,247] };
                assert!(pixels.enumerate_pixels().filter(|(x,y,pixel)| *x>=200 && *x<390 && *y>=160 && *y<300 && pixel.0==ink).count()>30,"native context menu ink");
                assert!(pixels.enumerate_pixels().filter(|(x,y,pixel)| *x>=410 && *x<630 && *y>=205 && *y<330 && pixel.0==ink).count()>20,"native desktop submenu must actually be painted");
            });
            for flag in ["bound","noRows","position","nativeChecks","nativeDesktops","mirrorStale","moved","escaped","minimized","staleAction","ownFrame","closed","saved"] {
                assert_eq!(report[flag],true,"{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View,Text} from '@lordos/runtime';
import {Window} from '@ROOT@/themes/default/components/Window.jsx';
import {configureWindows,wm,useWindows,getApp,setDesktops,desktopList} from '@ROOT@/sdk/windows.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {locale,t} from '@ROOT@/sdk/i18n.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {media} from '@ROOT@/sdk/media.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();
settings.set({language:'en',mode:'@MODE@',effects:false,reduceMotion:true,nativeAppearance:'axia'});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
const Body=()=>h(View,{style:{flex:1,padding:20}},h(Text,{style:{color:theme.text}},'Own content'));
configureWindows({apps:[{id:'probe',name:'Probe',Component:Body},{id:'own',name:'Own frame',ownFrame:true,Component:Body}],workArea:{x:10,y:28,w:@WIDTH@-20,h:650}});
setDesktops([10,20],10);
const id=wm.open('probe',{args:{csd:true},frame:{x:40,y:55,w:360,h:250}});
let handle=0,bound=false,noRows=true;
const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts;
function inspect(raw){const value=JSON.parse(raw);noRows&&=Object.keys(value.args||{}).every(key=>key==='id')&&!('theme'in(value.context||{}))&&!('scale'in(value.context||{}));}
lord.nativeViews.mount=(node,key,text,...sources)=>{const value=mount(node,key,text,...sources);if(key==='shell:window-context'){handle=value;bound=sources[0]===lord.windowList;inspect(text);}return value;};
lord.nativeViews.facts=(key,text)=>{if(key===handle)inspect(text);return facts(key,text);};
function App(){const{mounted,focused}=useWindows();return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},mounted.map(win=>h(Window,{key:win.id,win,app:getApp(win.id),t:theme,focused:win.id===focused}))));}
render(h(App));
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const find=label=>lord.nativeViews.findText(handle,label);
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const win=id=>state().windows.find(window=>window.id===id)?.w;
async function point(label,type){await until(()=>{const node=find(label);return node&&lord.getLayout(node)[2]>0;});const node=find(label),[x,y,w,h]=lord.getLayout(node);
 const owner=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x+w/2,y+h/2),x+w/2,y+h/2,0);__lordEvent(type,owner||0,x+w/2,y+h/2,0);__lordNativeView();}
async function click(label){await point(label,'down');await point(label,'up');}
async function open(id){wm.showWindowMenu(id,100,110);await until(()=>find(t('shell.window.minimize')));}
async function run(){
 await open(id);await sleep(70);const before=lord.getLayout(find(t('shell.window.minimize')));
 raw('patch',id,{windowMenu:{x:180,y:160}});raw('toggleMaximize',id);
 const position=await until(()=>{const frame=lord.getLayout(find(t('shell.window.minimize')));return frame[0]===before[0]+80&&frame[1]===before[1]+50;});
 const nativeChecks=await until(()=>find('✓'));
 raw('setDesktops',[10,20,30],10);raw('moveToDesktop',id,20);
 await point(t('shell.desktops.moveTo'),'move');const nativeDesktops=await until(()=>find(t('shell.desktops.name',{n:3})))&&!find(t('shell.desktops.name',{n:2}));
 const mirrorStale=desktopList.get().length===2&&wm.get(id).windowMenu.x===100&&!wm.get(id).maximized&&wm.get(id).desktop===10;
 await until(()=>{const node=find(t('shell.desktops.name',{n:3}));return node&&lord.getLayout(node)[2]>0;});await sleep(60);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 await point(t('shell.desktops.name',{n:3}),'down');const activeBeforeMove=state().active;await point(t('shell.desktops.name',{n:3}),'up');const moved=await until(()=>win(id)?.desktop===30&&!wm.get(id)?.windowMenu)&&state().active===activeBeforeMove;
 wm.focus(id);await open(id);lord.nativeViews.key('Escape','',0);__lordNativeView();const escaped=await until(()=>!wm.get(id)?.windowMenu);
 await open(id);await click(t('shell.window.minimize'));const minimized=await until(()=>win(id)?.minimized===true&&!wm.get(id)?.windowMenu);
 wm.focus(id);await open(id);raw('close',id);await until(()=>!lord.nativeViews.text(handle));
 let calls=0;const close=wm.close;wm.close=(...args)=>{calls++;return close(...args);};
 lord.nativeViews.action(handle,{action:['windowContext',{action:'close',id}]});__lordNativeView();wm.close=close;
 const staleAction=calls===0&&!!wm.get(id)&&!win(id);wm.close(id);
 const own=wm.open('own',{frame:{x:40,y:55,w:360,h:250}});await open(own);const ownFrame=!!find(t('shell.window.maximize'));
 await click(t('shell.window.close'));const closed=await until(()=>!wm.get(own));
 console.log('window_context-observation '+JSON.stringify({bound,noRows,position,nativeChecks,nativeDesktops,mirrorStale,moved,escaped,minimized,staleAction,ownFrame,closed,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('window_context driver: '+(error.message+' '+(error.stack||'')))),150);
"#;
