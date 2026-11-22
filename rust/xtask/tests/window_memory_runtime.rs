//! The actual nonvisual service saves native geometry and restores native twins.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn remembered_places_read_native_windows_without_a_js_subscription() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(
                repo,
                "window_memory",
                DRIVER,
                mode,
                width,
                "window-memory.png",
            );
            for flag in [
                "configured",
                "savedNative",
                "filters",
                "ordered",
                "twins",
                "explicitPlacement",
                "missingPlace",
                "noWindowTransport",
                "nativePreferences",
                "noJsPersistence",
                "noObserver",
                "continued",
                "nativeDisplays",
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
import {configureWindows,wm,windows,registerBuiltinApp} from '@ROOT@/sdk/windows.js';
import {startWindowMemory,rememberedFrame} from '@ROOT@/sdk/windowmemory.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const Icon=()=>null;
const apps=[{id:'notes',name:'Notes',Component:()=>null,Icon},{id:'pkg',name:'Package',package:true,Component:()=>null,Icon}];
configureWindows({apps});
registerBuiltinApp({id:'panel',name:'Panel',builtin:true,Component:()=>null,Icon});
settings.set({windowPlaces:{'app:keep':{d:'Keep|DP-2',x:1,y:2,w:300,h:200,max:false}}});
wm.open('notes',{instance:'a',frame:{x:30,y:50,w:300,h:220}});
const nativeSettings=lord.settings.state;
const profile=()=>nativeSettings.call(lord.settings,{cmd:'snapshot'}).state;
const source=lord.windowList,mount=lord.nativeViews.mount,request=lord.nativeApps.request;
let configured=false,noWindowTransport=true,writes=0,stop,shown=true,observerMounts=0;
const perform=settingsBridge.perform;let hold=false;
settingsBridge.perform=effect=>{if(effect.reply?.state?.windowPlaces)writes++;if(!hold)perform(effect);};
let jsPersistence=0;
lord.settings.state=input=>{if(input.cmd==='patch'&&('windowPlaces'in(input.patch||{}))){jsPersistence++;throw Error('window memory wrote preferences through JS');}return nativeSettings.call(lord.settings,input);};
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='memoryMerge')throw Error('window places reentered JS');if(input.cmd==='memoryFrame')noWindowTransport&&=!('places'in input)&&!('settings'in input);if(input.cmd==='memoryStart'){configured=input.enabled;for(const key of ['windows','registry','catalog','settings','places','displays'])noWindowTransport&&=!(key in input);}return request.call(lord.nativeApps,text);};
lord.nativeViews.mount=(node,id,...args)=>{if(id==='shell:window-memory'){observerMounts++;throw Error('legacy memory observer mounted');}return mount(node,id,...args);};
const direct=(op,...args)=>source.call(op,JSON.stringify([{},...args]));
function App(){const[on,setOn]=useState(true);shown=on;stop=()=>setOn(false);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},on?h(View,{style:{width:1,height:1}}):null));}
render(h(App));
async function run(){
 startWindowMemory();await until(()=>configured);
 const read=windows.get;windows.get=()=>{throw Error('window memory read the JS mirror');};
 direct('setFrame','notes#a',{x:70,y:90,w:340,h:250});
 direct('open',{app:'notes',instance:'dialog',args:{dialog:true}});
 direct('open',{app:'panel'});
 direct('open',{app:'linux',instance:'1'});
 direct('open',{app:'linux',instance:'2',args:{pkg:true,appId:'pkg'}});
 const savedNative=await until(()=>settings.get().windowPlaces?.['app:notes']?.x===70);
 const places=settings.get().windowPlaces||{};
 const filters=Object.keys(places).length===3&&!!places['lpk:pkg']&&!places.panel;
 const ordered=Object.keys(places).join(',')==='app:keep,app:notes,lpk:pkg';
 direct('open',{app:'notes',instance:'b'});
 const frame=rememberedFrame('app:notes'),twins=frame?.x===160&&frame?.y===180;
 const explicit=rememberedFrame('app:notes',{shift:7,fixed:true,size:[210,160]});
 const explicitPlacement=explicit?.x===77&&explicit?.y===97&&explicit?.w===210&&explicit?.h===160&&explicit?.maximized===false;
 const missingPlace=rememberedFrame('app:missing')===null;

 hold=true;
 nativeSettings.call(lord.settings,{cmd:'patch',patch:{windowPlaces:{...profile().windowPlaces,'app:notes':{...profile().windowPlaces['app:notes'],x:101}}}});
 const current=rememberedFrame('app:notes');
 const nativePreferences=current?.x===191&&settings.get().windowPlaces['app:notes'].x===70;
 nativeSettings.call(lord.settings,{cmd:'patch',patch:{windowPlaces:{...profile().windowPlaces,'app:notes':{...profile().windowPlaces['app:notes'],x:70}}}});
 hold=false;
 direct('close','notes#b'); // the twin was only needed to verify restore cascading
 stop();await until(()=>!shown);const count=writes;
 direct('setFrame','notes#a',{x:199,y:90,w:340,h:250});
 const continued=await until(()=>profile().windowPlaces['app:notes'].x===199)&&writes>count&&!shown;
 const physical=screen.displays[0],original={name:physical.name,x:physical.x};
 physical.name='Forged JS display';physical.x=100;
 direct('setFrame','notes#a',{x:249,y:90,w:340,h:250});
 const nativeDisplays=await until(()=>profile().windowPlaces['app:notes'].x===249)&&profile().windowPlaces['app:notes'].d.startsWith(original.name+'|');
 physical.name=original.name;physical.x=original.x;
 const noObserver=observerMounts===0;
 windows.get=read;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('window_memory-observation '+JSON.stringify({configured,savedNative,filters,ordered,twins,explicitPlacement,missingPlace,noWindowTransport,noObserver,continued,nativeDisplays,nativePreferences,noJsPersistence:jsPersistence===0,saved:!!saved?.ok,writes,frame,places}));
}
setTimeout(()=>run().catch(error=>console.error('window_memory driver: '+(error.stack||error))),150);
"#;
