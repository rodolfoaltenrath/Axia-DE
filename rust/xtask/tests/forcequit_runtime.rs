//! Force Quit membership from the authoritative model, through the real bridge.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn forcequit_reads_windows_without_a_js_snapshot() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_capture(repo, "forcequit", DRIVER, mode, width, "forcequit.png", |path| {
                let pixels = image::open(path).unwrap().into_rgb8();
                let ink = if mode == "light" { [29,29,31] } else { [245,245,247] };
                assert!(pixels.pixels().filter(|pixel| pixel.0 == ink).count() > 20,
                    "native Force Quit text must be painted: {mode}/{width}");
            });
            for flag in ["nativeCatalogCapability", "noCatalogTransport", "nativeCatalogRevision", "staleCatalogFacts", "nativeBuiltinPolicy", "bound", "initial", "filtered", "directRemoved", "directAdded", "argsChanged", "staleMirror", "selectionRemoved", "noQuit", "serviceGroups", "freshHandle", "freshClose", "metadataCached", "metadataSurvivedUnmount", "localeResent", "noWindowTransport", "saved"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/sdk/nativeapps.js';
import {configureWindows,wm,registerBuiltinApp} from '@ROOT@/sdk/windows.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {AppWindowContext} from '@ROOT@/sdk/components.jsx';
import {locale} from '@ROOT@/sdk/i18n.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {media} from '@ROOT@/sdk/media.js';
import {nativeCatalogFacts} from '@ROOT@/sdk/nativecatalog.js';
import {runningApps,forceQuit} from '@ROOT@/sdk/appcontrol.js';
locale.set({...locale.get(),lang:'en'});
settings.set({mode:'@MODE@',accent:'azul',effects:false,nativeAppearance:'axia'});
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='forcequit'),app=nativeApp(manifest);
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:[app,{id:'notes',name:'Notes',Component:()=>null,Icon},{id:'pkg',name:'Package',isolated:true,Component:()=>null,Icon}]});
registerBuiltinApp({id:'linux',name:'Linux',builtin:true,Component:()=>null,Icon});
registerBuiltinApp({id:'panel',name:'Hidden panel',builtin:true,Component:()=>null,Icon});
const a=wm.open('notes',{instance:'a'}),b=wm.open('notes',{instance:'b'});
wm.open('linux',{instance:11,args:{appId:'pkg',pkg:true,surface:11}});wm.open('panel');wm.open('forcequit');
let handle=0,bound=false,noCatalogTransport=true,viewFacts=0,lastViewFacts,viewUnmounted=false,closeView;const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
const requests=[],request=lord.nativeApps.request.bind(lord.nativeApps);
lord.nativeApps.request=text=>{requests.push(JSON.parse(text));return request(text);};
function inspect(text,source){const input=JSON.parse(text);bound=source===lord.windowList&&!('windows'in input);noCatalogTransport&&=!('registry'in input)&&!('catalog'in input);lastViewFacts=input;}
lord.nativeViews.mount=(node,id,text,...source)=>{const h=mount(node,id,text,...source);if(id==='forcequit'&&!JSON.parse(text).icon){handle=h;inspect(text,source[0]);}return h;};
lord.nativeViews.facts=(id,text)=>{if(id===handle){inspect(text,lord.windowList);viewFacts++;}return facts(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)viewUnmounted=true;return unmount(id);};
function Harness(){const[shown,setShown]=useState(true);closeView=()=>setShown(false);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee',padding:20,alignItems:'center'}},
 shown?h(AppWindowContext.Provider,{value:{id:'forcequit',focused:true,args:{}}},h(View,{style:{width:420,height:460}},h(app.Component))):null));}
render(h(Harness));
const has=label=>!!lord.nativeViews.findText(handle,label);
const raw=(op,...args)=>lord.windowList.call(op,JSON.stringify([{},...args]));
function click(label){const node=lord.nativeViews.findText(handle,label),r=lord.getLayout(node),x=r[0]+r[2]/2,y=r[1]+r[3]/2;
 for(const type of ['down','up']){const owner=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,owner||0,x,y,0);__lordNativeView();}}
async function run(){
 await until(()=>handle&&has('Notes')&&has('2 windows')&&has('Package'));
 const initial=has('Notes')&&has('2 windows')&&has('Package'),filtered=!has('Hidden panel')&&!has('Linux');
 const nativeCatalogCapability=manifest.catalogSource==='native';
 const metadata=nativeCatalogFacts(true).get(),beforeCatalogFacts=viewFacts;
 const publish=(name,builtin=false)=>request(JSON.stringify({cmd:'metadata',...metadata,registry:metadata.registry.map(app=>app.id==='notes'?{...app,name,builtin}:app)}));
 click('Notes');publish('Native Notes');
 const nativeCatalogRevision=await until(()=>has('Native Notes')&&!has('Notes'))&&viewFacts===beforeCatalogFacts&&nativeCatalogFacts(true).get().registry.find(app=>app.id==='notes').name==='Notes';
 facts(handle,JSON.stringify({...lastViewFacts,registry:[{id:'notes',name:'Forged Notes',builtin:true},{id:'panel',name:'Forged panel'}],catalog:[]}));__lordNativeView();await sleep(60);
 const staleCatalogFacts=has('Native Notes')&&has('2 windows')&&!has('Forged Notes')&&!has('Forged panel');
 publish('Native Notes',true);const hidden=await until(()=>!has('Native Notes'));
 publish('Notes');const nativeBuiltinPolicy=hidden&&await until(()=>has('Notes')&&has('2 windows'));
 runningApps();runningApps();
 raw('close',a);const directRemoved=await until(()=>!has('2 windows'));
 raw('open',{app:'notes',instance:'c'});const directAdded=await until(()=>has('2 windows'));
 raw('setArgs','linux#11',{appId:'org.test.fresh-app',surface:12,pkg:false});
 const argsChanged=await until(()=>has('Fresh app')&&!has('Package'));
 const staleMirror=!!wm.get(a)&&!wm.get('notes#c')&&wm.get('linux#11').args.appId==='pkg';
 const groups=lord.nativeApps.request(JSON.stringify({cmd:'groups'})).groups;
 const notes=groups.find(group=>group.key==='notes');
 const serviceGroups=notes?.windows.includes(b)&&notes?.windows.includes('notes#c')&&!notes?.windows.includes(a)
   &&groups.some(group=>group.key==='linux:org.test.fresh-app');
 const originalKill=lord.wayland.kill,kills=[];lord.wayland.kill=id=>{kills.push(id);return '';};
 raw('setArgs','linux#11',{appId:'org.test.fresh-app',surface:987,pkg:false});
 const error=forceQuit('linux:org.test.fresh-app');lord.wayland.kill=originalKill;
 const freshHandle=error===''&&kills.length===1&&kills[0]===987;
 raw('open',{app:'scratch',instance:'fresh'});const absentFromMirror=!wm.get('scratch#fresh');
 const closeError=forceQuit('scratch');
 const freshClose=absentFromMirror&&closeError===''&&!lord.nativeApps.request(JSON.stringify({cmd:'groups'})).groups.some(group=>group.key==='scratch');
 const metadataCached=requests.filter(input=>input.cmd==='metadata').length===1;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 click('Notes');let quits=0;const close=wm.close,kill=lord.wayland.kill;wm.close=(...args)=>{quits++;return close(...args);};lord.wayland.kill=()=>{quits++;return '';};
 raw('close',b);raw('close','notes#c');lord.nativeViews.action(handle,{action:['quit']});__lordNativeView();
 await until(()=>!has('Notes'));const selectionRemoved=!has('Notes'),noQuit=quits===0;
 lord.wayland.kill=kill;wm.close=close;
 const before=requests.filter(input=>input.cmd==='metadata').length;
 closeView();await until(()=>viewUnmounted&&!lord.nativeViews.text(handle));runningApps();
 const metadataSurvivedUnmount=viewUnmounted&&!lord.nativeViews.text(handle)&&requests.filter(input=>input.cmd==='metadata').length===before;
 locale.set({...locale.get(),lang:'pt'});await sleep(80);runningApps();
 const localeResent=requests.filter(input=>input.cmd==='metadata').length===before+1;
 const noWindowTransport=requests.every(input=>!('windows'in input));
 console.log('forcequit-observation '+JSON.stringify({nativeCatalogCapability,noCatalogTransport,nativeCatalogRevision,staleCatalogFacts,nativeBuiltinPolicy,bound,initial,filtered,directRemoved,directAdded,argsChanged,staleMirror,selectionRemoved,noQuit,serviceGroups,freshHandle,freshClose,metadataCached,metadataSurvivedUnmount,localeResent,noWindowTransport,saved:!!saved?.ok,kills,text:lord.nativeViews.text(handle)}));
}
setTimeout(()=>run().catch(error=>console.error('forcequit driver: '+(error.stack||error))),200);
"#;
