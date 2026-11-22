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
                "bound",
                "savedNative",
                "filters",
                "ordered",
                "twins",
                "noWindowTransport",
                "nativePreferences",
                "noJsPersistence",
                "nonvisual",
                "stopped",
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
import {configureWindows,wm,windows,registerBuiltinApp} from '@ROOT@/sdk/windows.js';
import {WindowMemory,rememberedFrame} from '@ROOT@/sdk/windowmemory.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const Icon=()=>null;
configureWindows({apps:[{id:'notes',name:'Notes',Component:()=>null,Icon},{id:'pkg',name:'Package',package:true,Component:()=>null,Icon}]});
registerBuiltinApp({id:'panel',name:'Panel',builtin:true,Component:()=>null,Icon});
settings.set({windowPlaces:{'app:keep':{d:'Keep|DP-2',x:1,y:2,w:300,h:200,max:false}}});
wm.open('notes',{instance:'a',frame:{x:30,y:50,w:300,h:220}});
const nativeSettings=lord.settings.state;
const profile=()=>nativeSettings.call(lord.settings,{cmd:'snapshot'}).state;
const source=lord.windowList,mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount,request=lord.nativeApps.request;
let handle=0,bound=false,noWindowTransport=true,writes=0,stop,unmounted=false;
const perform=settingsBridge.perform;let hold=false;
settingsBridge.perform=effect=>{if(effect.reply?.state?.windowPlaces)writes++;if(!hold)perform(effect);};
let jsPersistence=0;
lord.settings.state=input=>{if(input.cmd==='patch'&&('windowPlaces'in(input.patch||{}))){jsPersistence++;throw Error('window memory wrote preferences through JS');}return nativeSettings.call(lord.settings,input);};
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='memoryMerge')throw Error('window places reentered JS');if(input.cmd==='memoryFrame')noWindowTransport&&=!('places'in input)&&!('settings'in input);return request.call(lord.nativeApps,text);};
const inspect=text=>{const facts=JSON.parse(text);noWindowTransport&&=!('windows'in facts)&&!('windows'in(facts.args||{}));};
lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:window-memory'){inspect(text);bound=sources[0]===source;handle=mount(node,id,text,...sources);return handle;}return mount(node,id,text,...sources);};
const send=lord.nativeViews.facts;lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return send(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounted=true;return unmount(id);};
const direct=(op,...args)=>source.call(op,JSON.stringify([{},...args]));
function App(){const[on,setOn]=useState(true);stop=()=>setOn(false);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},on?h(WindowMemory):null));}
render(h(App));
async function run(){
 await until(()=>handle>0);
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
 hold=true;
 nativeSettings.call(lord.settings,{cmd:'patch',patch:{windowPlaces:{...profile().windowPlaces,'app:notes':{...profile().windowPlaces['app:notes'],x:101}}}});
 const current=rememberedFrame('app:notes');
 const nativePreferences=current?.x===191&&settings.get().windowPlaces['app:notes'].x===70;
 nativeSettings.call(lord.settings,{cmd:'patch',patch:{windowPlaces:{...profile().windowPlaces,'app:notes':{...profile().windowPlaces['app:notes'],x:70}}}});
 hold=false;
 const nonvisual=(lord.nativeViews.text(handle)||'')==='';
 stop();await until(()=>unmounted);const count=writes;
 direct('setFrame','notes#a',{x:199,y:90,w:340,h:250});await sleep(1000);
 const stopped=unmounted&&writes===count&&profile().windowPlaces['app:notes'].x===70;
 windows.get=read;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('window_memory-observation '+JSON.stringify({bound,savedNative,filters,ordered,twins,noWindowTransport,nonvisual,stopped,nativePreferences,noJsPersistence:jsPersistence===0,saved:!!saved?.ok,writes,frame,places}));
}
setTimeout(()=>run().catch(error=>console.error('window_memory driver: '+(error.stack||error))),150);
"#;
