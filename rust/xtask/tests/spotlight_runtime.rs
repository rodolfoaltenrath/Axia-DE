//! Native Spotlight input, effects, cleanup and theme captures in a private session.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn spotlight_stays_native_with_legacy_preferences_and_keeps_keyboard_navigation() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640u32, 1000] {
            let report = ui::run_ui(repo, "spotlight", DRIVER, mode, width, "spotlight.png");
            for flag in [
                "appOpened",
                "arrowMoved",
                "tabChangedSection",
                "providerRan",
                "escaped",
                "outsideClosed",
                "focusRestored",
                "inputIsolated",
                "catalogSent",
                "cleaned",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{flag}: {report}");
            }
            assert_eq!(report["copied"], "360", "{report}");
            let bounds = report["bounds"].as_array().unwrap();
            assert!(
                bounds[0].as_f64().unwrap() >= 0. && bounds[1].as_f64().unwrap() >= 0.,
                "{report}"
            );
            assert!(
                bounds[0].as_f64().unwrap() + bounds[2].as_f64().unwrap() <= width as f64,
                "{report}"
            );
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {SpotlightPanel} from '@ROOT@/sdk/rustsearch.jsx';
import {spotlight,spotlightOpen,registerSpotlightSource} from '@ROOT@/sdk/search.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {clipboard} from '@ROOT@/sdk/clipboard.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
import {TextInput,focusToken} from '@ROOT@/sdk/components.jsx';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
let handle=0,last,catalogSent=false,opened,copied,ran=false,underlay='';
const request=lord.launcher.request.bind(lord.launcher),mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount,facts=lord.nativeViews.facts;
lord.launcher.request=raw=>{const input=JSON.parse(raw),reply=request(raw);if(input.op==='search'){if(input.cmd==='poll'){for(const update of reply.updates||[])last=update;}else if(reply?.id)last=reply;}return reply;};
const inspect=text=>{const data=JSON.parse(text);if(data.args?.owner==='spotlight'&&data.catalog?.some(app=>app.id==='notes'))catalogSent=true;};
lord.nativeViews.mount=(node,app,text)=>{inspect(text);const id=mount(node,app,text);if(JSON.parse(text).args?.owner==='spotlight')handle=id;return id;};
lord.nativeViews.facts=(id,text)=>{inspect(text);return facts(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)handle=0;return unmount(id);};
const find=text=>handle?lord.nativeViews.findText(handle,text):0;
const key=(name,char='',mods=0)=>{__lordKey(0,1,name,char,name,mods);__lordKey(0,0,name,'',name,mods);globalThis.__lordNativeView();};
async function type(text){for(const char of text){key(char,char);await sleep(15);}await until(()=>last?.query===text&&!last?.pending);await sleep(60);}
async function fresh(query){spotlight.close();await sleep(30);spotlight.open();await until(()=>!!handle);await sleep(50);if(query)await type(query);}
const selected=()=>last?.sections?.flatMap(section=>section.items)[last.selected]?.key;
const section=()=>last?.sections?.find(section=>section.items.some(item=>item.key===selected()))?.id;
const Icon=({size})=>h(View,{style:{width:size,height:size,backgroundColor:'#3489bd'}});
configureWindows({apps:[{id:'probe-a',name:'Probe Alpha',Icon},{id:'probe-b',name:'Probe Beta',Icon},{id:'notes',name:'Notes',Icon}],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
settings.set({language:'en',mode:'@MODE@',spotlightNativeView:false});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
wm.open=id=>{opened=id;return'probe-window';};clipboard.set=text=>{copied=text;};
const unregister=registerSpotlightSource('probe',query=>query==='probe'?[{key:'external',score:80,title:'Probe external',subtitle:'Native provider action',Icon,run:()=>{ran=true;}}]:[]);
function App(){const[value,setValue]=useState('');return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(View,{style:{position:'absolute',left:-300,top:0,width:200,height:40}},h(TextInput,{value,autoFocus:true,onChange:text=>{underlay=text;setValue(text);}})),h(SpotlightPanel)));}
render(h(App));
async function run(){
 const previous=focusToken();
 await fresh('Probe Alpha');key('Enter');await until(()=>opened==='probe-a');const appOpened=opened==='probe-a';
 await fresh('15% of 2400');key('Enter');await until(()=>copied==='360');
 await fresh('probe');await until(()=>!!find('Probe external'));const first=selected();key('ArrowDown');await sleep(50);const arrowMoved=selected()!==first;
 const before=section();key('Tab');await sleep(50);const tabChangedSection=section()!==before;
 // Click the real native provider row, independent of result ordering.
 const row=find('Probe external');const[x,y,w,height]=lord.getLayout(row);
 for(const type of ['down','up']){const target=lord.nativeViews.pointer(type,row,x+w/2,y+height/2,0);__lordEvent(type,target,x+w/2,y+height/2,0);}globalThis.__lordNativeView();
 await until(()=>ran);const providerRan=ran;
 await fresh('Notes');await until(()=>!!find('Notes'));await sleep(100);const bounds=lord.getLayout(find('Notes'));
 const image=lord.media.screenshot('screenshots');const saved=await media.saveImage(image,@IMAGE@);media.close(image);
 key('Escape');await until(()=>!spotlightOpen.get());const escaped=!spotlightOpen.get();
 await fresh('Notes');for(const type of ['down','up']){const node=lord.nativeViews.hitTest(2,698);const target=lord.nativeViews.pointer(type,node,2,698,0);__lordEvent(type,target,2,698,0);}globalThis.__lordNativeView();await until(()=>!spotlightOpen.get());const outsideClosed=!spotlightOpen.get();
 spotlight.close();await sleep(50);const stats=request(JSON.stringify({op:'search',cmd:'stats'}));const cleaned=!handle&&stats.clients===0&&stats.activeBytes===0;
 const inputIsolated=underlay==='';key('Z','Z');await sleep(50);const focusRestored=!!previous&&focusToken()===previous&&underlay==='Z';
 unregister();console.log('spotlight-observation '+JSON.stringify({appOpened,copied,arrowMoved,tabChangedSection,providerRan,escaped,outsideClosed,focusRestored,inputIsolated,catalogSent,cleaned,bounds,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('spotlight driver: '+(error.stack||error))),200);
"#;
