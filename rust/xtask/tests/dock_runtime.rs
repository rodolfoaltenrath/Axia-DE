//! Dock native tooltip/drag integration with the real session and policy.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn dock_parts_stay_native_in_all_positions_and_keep_drag_policy() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640u32, 1000] {
            let report = ui::run_ui(repo, "dock", DRIVER, mode, width, "dock.png");
            let positions = report["positions"].as_array().unwrap();
            assert_eq!(positions.len(), 3, "{report}");
            for (position, expected) in positions.iter().zip(["bottom", "left", "right"]) {
                assert_eq!(position["position"], expected);
                assert_eq!(position["tip"], true, "{report}");
                let rect = position["rect"].as_array().unwrap();
                assert!(
                    rect[0].as_f64().unwrap() >= 0. && rect[1].as_f64().unwrap() >= 0.,
                    "{report}"
                );
                assert!(
                    rect[0].as_f64().unwrap() + rect[2].as_f64().unwrap() <= width as f64,
                    "{report}"
                );
            }
            assert_eq!(report["lockedKept"], true, "{report}");
            assert_eq!(report["cancelKept"], true, "{report}");
            assert_eq!(report["removing"], true, "{report}");
            assert_eq!(report["unpinned"], true, "{report}");
            assert_eq!(report["opens"], 0, "drag release opened an app: {report}");
            let icons = report["ghostIcons"].as_array().unwrap();
            assert!(
                icons.iter().any(|id| id == "files") && icons.iter().any(|id| id == "notes"),
                "{report}"
            );
            assert!(
                !icons
                    .iter()
                    .any(|id| id.as_str().unwrap_or_default().starts_with("app:")),
                "typed Dock IDs reached icon renderer: {report}"
            );
            assert_eq!(report["saved"], true, "{report}");
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {Dock} from '@ROOT@/themes/default/components/Dock.jsx';
import {dock,dockDrag} from '@ROOT@/sdk/dock.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {configureWindows} from '@ROOT@/sdk/windows.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
const icons={},ghostIcons=[];let change,opens=0;
const inspect=text=>{const args=JSON.parse(text).args;if(args?.owner==='dock-ghost')ghostIcons.push(args.tree.children.find(child=>child.kind==='appIcon')?.app||'');};
const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts;
lord.nativeViews.mount=(node,app,text)=>{inspect(text);return mount(node,app,text);};
lord.nativeViews.facts=(handle,text)=>{inspect(text);return facts(handle,text);};
const Files=({size})=>h(View,{ref:node=>{icons.files=node;},style:{width:size,height:size,backgroundColor:'#5c946e'}});
const Notes=({size})=>h(View,{ref:node=>{icons.notes=node;},style:{width:size,height:size,backgroundColor:'#d6a529'}});
const items=[{id:'app:files',name:'Files',Icon:Files,pinned:true},{id:'app:notes',name:'Notes',Icon:Notes,pinned:true}];
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[position,set]=useState('bottom');change=set;return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(Dock,{t:theme,items,running:[],onOpen:()=>opens++,position,size:54,magnify:false,area:{x:0,y:0,w:@WIDTH@,h:700}})));}
const event=(type,node,x,y)=>{const rect=node?lord.getLayout(node):[0,0,0,0];__lordEvent(type,node||0,x??rect[0]+rect[2]/2,y??rect[1]+rect[3]/2,0);globalThis.__lordNativeView();};
const escape=()=>{__lordKey(1,1,'Escape','','Escape',0);__lordKey(1,0,'Escape','','Escape',0);};
async function drag(name){event('down',icons[name].id);event('move',0,@WIDTH@/2,220);await until(()=>!!dockDrag.get());}
async function run(){const positions=[];
 for(const position of ['bottom','left','right']){event('move',0,@WIDTH@/2,150);change(position);await sleep(100);event('move',icons.files.id);
  const tip=await until(()=>find('Files'));positions.push({position,tip,rect:tip?lord.getLayout(find('Files')):[]});}
 event('move',0,@WIDTH@/2,150);change('bottom');await sleep(100);
 await drag('files');await sleep(400);event('up',0,@WIDTH@/2,220);await until(()=>!dockDrag.get());const lockedKept=dock.isPinned('app:files');
 await drag('notes');escape();await until(()=>!dockDrag.get());const cancelKept=dock.isPinned('app:notes');
 await drag('notes');await until(()=>dockDrag.get()?.removing);const removing=!!dockDrag.get()?.removing;
 await sleep(150);const handle=lord.media.screenshot('screenshots');const saved=await media.saveImage(handle,@IMAGE@);media.close(handle);
 event('up',0,@WIDTH@/2,220);await until(()=>!dockDrag.get());const unpinned=!dock.isPinned('app:notes');
 console.log('dock-observation '+JSON.stringify({positions,lockedKept,cancelKept,removing,unpinned,opens,ghostIcons,saved:!!saved?.ok}));}
settings.set({language:'en',dockNativeView:false,reduceMotion:true,dockPinned:['app:files','app:notes'],nativeAppearance:'axia'});
configureWindows({apps:[{id:'files',name:'Files',Icon:Files},{id:'notes',name:'Notes',Icon:Notes}],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
enableShellLayers();render(h(App));setTimeout(()=>run().catch(error=>console.error('dock driver: '+(error.stack||error))),200);
"#;
