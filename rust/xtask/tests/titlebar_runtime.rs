//! Real window frame controls, focus and native header captures.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn window_headers_stay_native_and_preserve_window_manager_actions() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        let kind = format!("titlebar_{appearance}");
        let driver = DRIVER
            .replace("@APPEARANCE@", appearance)
            .replace("@KIND@", &kind);
        for mode in ["light", "dark"] {
            for width in [640u32, 1000] {
                let report = ui::run_ui(repo, &kind, &driver, mode, width, "titlebar.png");
                for flag in [
                    "nativeHeaders",
                    "nativeStateTransport",
                    "directModelTitle",
                    "sourceChecked",
                    "focused",
                    "retitled",
                    "moved",
                    "sceneMoved",
                    "maximized",
                    "restored",
                    "doubleMaximized",
                    "minimized",
                    "reopened",
                    "guardKept",
                    "closed",
                    "noControlDrag",
                    "csdHasNoHeader",
                    "ownHasNoHeader",
                    "saved",
                ] {
                    assert_eq!(report[flag], true, "{appearance}/{flag}: {report}");
                }
                let rect = report["titleRect"].as_array().unwrap();
                assert!(
                    rect[0].as_f64().unwrap() >= 0.
                        && rect[0].as_f64().unwrap() + rect[2].as_f64().unwrap() <= width as f64,
                    "{report}"
                );
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View,Text} from '@lordos/runtime';
import {Window} from '@ROOT@/themes/default/components/Window.jsx';
import {configureWindows,wm,useWindows,getApp} from '@ROOT@/sdk/windows.js';
import {settings,useSettings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const handles=new Map(),argsById=new Map(),drags=[],boundSources=[];
const mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount,facts=lord.nativeViews.facts;
lord.nativeViews.mount=(node,app,text,...source)=>{const id=mount(node,app,text,...source);if(app==='shell:titlebar'){const args=JSON.parse(text).args;handles.set(args.id,id);argsById.set(args.id,args);boundSources.push(source[0]===lord.windowList);}return id;};
lord.nativeViews.facts=(id,text)=>{for(const[win,handle]of handles)if(handle===id)argsById.set(win,JSON.parse(text).args);return facts(id,text);};
lord.nativeViews.unmount=id=>{for(const[win,handle]of handles)if(handle===id){handles.delete(win);argsById.delete(win);}return unmount(id);};
const begin=wm.beginMove;wm.beginMove=id=>{const result=begin(id);drags.push({id,result});return result;};
const find=(id,label)=>handles.has(id)?lord.nativeViews.findText(handles.get(id),label):0;
const pointer=(type,node,x,y)=>{const target=lord.nativeViews.pointer(type,node,x,y,0);__lordEvent(type,target,x,y,0);globalThis.__lordNativeView();};
const point=(id)=>{const node=find(id,wm.get(id)?.title||'Probe');const[x,y,w,h]=lord.getLayout(node);return[x+w/2,y+h/2];};
const press=(node)=>{const[x,y,w,h]=lord.getLayout(node);for(const type of ['down','up'])pointer(type,lord.nativeViews.hitTest(x+w/2,y+h/2),x+w/2,y+h/2);};
async function control(id,key){
 const frame=wm.frame(wm.get(id)),args=argsById.get(id),glyph=args.controls.find(control=>control.key===key).glyph;
 // Axia's lights reveal their glyphs when the pointer enters the group.
 const x=frame.x+20,y=frame.y+20;pointer('move',lord.nativeViews.hitTest(x,y),x,y);
 await until(()=>{const node=find(id,glyph);return node&&lord.getLayout(node)[2]>0;});press(find(id,glyph));await sleep(70);
}
const Body=()=>h(View,{style:{flex:1,padding:20}},h(Text,{style:{fontSize:14,color:theme.text}},'Window body'));
configureWindows({apps:[{id:'probe',name:'Probe',Component:Body,minSize:[180,120]},{id:'own-probe',name:'Own frame',Component:Body,ownFrame:true,minSize:[180,120]}],workArea:{x:10,y:28,w:@WIDTH@-20,h:650},chrome:{w:2,h:43}});
settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',titleBarNativeView:false,reduceMotion:true});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
const secondary=wm.open('probe',{new:true,frame:{x:25,y:55,w:Math.min(440,@WIDTH@-100),h:230}});wm.setTitle(secondary,'Inactive window');
const primary=wm.open('probe',{new:true,frame:{x:40,y:150,w:Math.min(520,@WIDTH@-80),h:320}});wm.setTitle(primary,'Active window');
function App(){useSettings();const{mounted,focused}=useWindows();return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 mounted.map(win=>h(Window,{key:win.id,win,app:getApp(win.id),t:theme,focused:win.id===focused}))));}
render(h(App));
async function run(){
 await until(()=>{const node=find(primary,'Active window');return node&&lord.getLayout(node)[2]>0;});await sleep(100);
 const nativeHeaders=handles.has(primary)&&handles.has(secondary);
 const nativeStateTransport=boundSources.every(Boolean)&&[...argsById.values()].every(args=>!('title'in args)&&!('focused'in args));
 const previousTitle=wm.get(primary).title;
 // The SDK mirror and React props stay unchanged. Only the Rust subscription
 // can deliver this mutation to the native header.
 lord.windowList.call('patch',JSON.stringify([{},primary,{title:'Direct Rust model title'}]));
 const directModelTitle=await until(()=>!!find(primary,'Direct Rust model title'))&&wm.get(primary).title===previousTitle;
 lord.windowList.call('patch',JSON.stringify([{},primary,{title:previousTitle}]));await until(()=>!!find(primary,previousTitle));
 let sourceChecked=false;try{mount(0,'shell:titlebar',JSON.stringify({args:{id:primary}}),{});}catch(error){sourceChecked=error instanceof TypeError;}
 let[x,y]=point(secondary);const target=lord.nativeViews.hitTest(x,y);for(const type of ['down','up'])pointer(type,target,x,y);await until(()=>wm.focused()===secondary);
 wm.focus(primary);await sleep(70);const focused=wm.focused()===primary;
 wm.setTitle(primary,'A long updated title — Axia window');await until(()=>!!find(primary,'A long updated title — Axia window'));const retitled=!!find(primary,'A long updated title — Axia window');
 [x,y]=point(primary);const before={...wm.frame(wm.get(primary))};pointer('down',lord.nativeViews.hitTest(x,y),x,y);pointer('move',0,x+30,y+25);pointer('up',0,x+30,y+25);
 await until(()=>wm.frame(wm.get(primary)).x!==before.x);const after=wm.frame(wm.get(primary));const moved=after.x===before.x+30&&after.y===before.y+25;
 const sceneMoved=await until(()=>{const node=find(primary,'A long updated title — Axia window');return node&&lord.getLayout(node)[0]>=after.x;});const titleRect=lord.getLayout(find(primary,'A long updated title — Axia window'));
 const image=lord.media.screenshot('screenshots');const saved=await media.saveImage(image,@IMAGE@);media.close(image);
 let count=drags.length;await control(primary,'max');const maximized=!!wm.get(primary)?.maximized;await control(primary,'max');const restored=!wm.get(primary)?.maximized;let noControlDrag=drags.length===count;
 [x,y]=point(primary);for(let i=0;i<2;i++)for(const type of ['down','up'])pointer(type,lord.nativeViews.hitTest(x,y),x,y);
 await until(()=>!!wm.get(primary)?.maximized);const doubleMaximized=!!wm.get(primary)?.maximized;await control(primary,'max');
 count=drags.length;await control(primary,'min');const minimized=!!wm.get(primary)?.minimized;
 wm.focus(primary);await until(()=>!wm.get(primary)?.minimized&&!!handles.get(primary));await sleep(70);const reopened=!wm.get(primary)?.minimized;
 const unguard=wm.setCloseGuard(primary,()=>false);await control(primary,'close');const guardKept=!!wm.get(primary);unguard();await control(primary,'close');await until(()=>!wm.get(primary));const closed=!wm.get(primary);noControlDrag=noControlDrag&&drags.length===count;
 const csd=wm.open('probe',{new:true,args:{csd:true},frame:{x:40,y:400,w:260,h:150}});const own=wm.open('own-probe',{frame:{x:70,y:420,w:260,h:150}});await sleep(100);
 const csdHasNoHeader=!handles.has(csd),ownHasNoHeader=!handles.has(own);
 console.log('@KIND@-observation '+JSON.stringify({nativeHeaders,nativeStateTransport,directModelTitle,sourceChecked,focused,retitled,moved,sceneMoved,maximized,restored,doubleMaximized,minimized,reopened,guardKept,closed,noControlDrag,csdHasNoHeader,ownHasNoHeader,titleRect,after,drags,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('@KIND@ driver: '+(error.stack||error))),200);
"#;
