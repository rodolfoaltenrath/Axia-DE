//! Native keyboard ownership through the real bridge and compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn model_focus_and_shell_input_priority() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "native_focus", DRIVER, mode, width, "native-focus.png");
            for flag in ["bound", "initial", "direct", "mirrorStale", "textGate", "overlayGate", "restored", "saved"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View,Text,shellHasKeyboard} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/sdk/nativeapps.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {AppWindowContext,TextInput,clearTextFocus} from '@ROOT@/sdk/components.jsx';
import {textFieldActive} from '@ROOT@/sdk/textfocus.js';
import {enableShellLayers,Layer} from '@ROOT@/sdk/layers.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='calculator');
const app=nativeApp(manifest);configureWindows({apps:[app]});
const a=wm.open('calculator',{instance:'a'}),b=wm.open('calculator',{instance:'b'});
const handles=new Map(),sources=[];
const mount=lord.nativeViews.mount;
lord.nativeViews.mount=(node,id,text,...source)=>{const handle=mount(node,id,text,...source),facts=JSON.parse(text);
 if(id==='calculator'&&!facts.icon){handles.set(facts.windowId,handle);sources.push(source[0]===lord.windowList&&!('focused'in facts));}return handle;};
let control,typed='';
function App(){const[field,setField]=useState(false),[overlay,setOverlay]=useState(false),[value,setValue]=useState('');control={setField,setOverlay};
 return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee',padding:12,gap:12}},
 h(View,{style:{flexDirection:'row',height:520,gap:12}},...[a,b].map(id=>h(AppWindowContext.Provider,{key:id,value:{id,focused:false,args:{}}},
 h(View,{style:{flex:1}},h(app.Component))))),
 field?h(TextInput,{autoFocus:true,value,onChange:text=>{typed=text;setValue(text);}}):null,
 overlay?h(Layer,{name:'overlay'},h(View,{style:{position:'absolute',left:20,top:560,width:300,height:70}},h(Text,null,'Shell overlay'))):null));}
enableShellLayers();render(h(App));
const key=(name,char='')=>{__lordKey(0,1,name,char,name,0);__lordKey(0,0,name,'',name,0);__lordNativeView();};
const has=(id,value)=>(lord.nativeViews.text(handles.get(id))||'').split('\n').includes(value);
const directFocus=id=>lord.windowList.call('focus',JSON.stringify([{},id]));
async function run(){
 await until(()=>handles.size===2&&has(a,'0')&&has(b,'0'));
 key('2','2');await sleep(50);const initial=has(a,'0')&&has(b,'2');
 directFocus(a);key('7','7');await sleep(50);const direct=has(a,'7')&&has(b,'2'),mirrorStale=wm.focused()===b;
 control.setField(true);await until(textFieldActive);key('4','4');await sleep(50);
 const textGate=typed==='4'&&has(a,'7')&&has(b,'2');clearTextFocus();control.setField(false);await until(()=>!textFieldActive());
 control.setOverlay(true);await until(shellHasKeyboard);key('5','5');await sleep(50);
 const overlayGate=has(a,'7')&&has(b,'2');control.setOverlay(false);await until(()=>!shellHasKeyboard());
 key('8','8');await sleep(50);const restored=has(a,'78')&&has(b,'2');
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('native_focus-observation '+JSON.stringify({bound:sources.length===2&&sources.every(Boolean),initial,direct,mirrorStale,textGate,overlayGate,restored,saved:!!saved?.ok,typed,textA:lord.nativeViews.text(handles.get(a)),textB:lord.nativeViews.text(handles.get(b))}));
}
setTimeout(()=>run().catch(error=>console.error('native_focus driver: '+(error.stack||error))),200);
"#;
