//! Native launcher: large catalogs, effects and captures through the real compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn launcher_is_native_and_reaches_the_end_of_large_catalogs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640u32, 1000, 1920] {
            let report = ui::run_ui(repo, "launcher", DRIVER, mode, width, "launcher.png");
            for flag in [
                "tailVisible",
                "tailHit",
                "openedTail",
                "searched",
                "categoryFiltered",
                "menuEscaped",
                "pinned",
                "unpinned",
                "installed",
                "removed",
                "dragClosed",
                "dragDidNotLaunch",
                "escaped",
                "outsideClosed",
                "inputIsolated",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{flag}: {report}");
            }
            for count in ["initialCount", "tailCount"] {
                let count = report[count].as_u64().unwrap();
                let columns = ((width - 48) / 130).max(1) as u64;
                assert!(
                    count > 0 && count <= columns * 7,
                    "unbounded native rows: {report}"
                );
            }
            assert_eq!(report["initialTail"], false, "{report}");
            let row = report["firstRow"].as_array().unwrap();
            assert_eq!(row.len(), ((width - 48) / 130).max(1) as usize, "{report}");
            if width == 1920 {
                let left = row[0][0].as_f64().unwrap();
                let last = row.last().unwrap();
                let right = last[0].as_f64().unwrap() + last[2].as_f64().unwrap();
                assert!(
                    right - left > width as f64 * 0.85,
                    "wide launcher stayed narrow: {report}"
                );
            }
            let rect = report["tailRect"].as_array().unwrap();
            assert!(
                rect[0].as_f64().unwrap() >= 0.
                    && rect[0].as_f64().unwrap() + rect[2].as_f64().unwrap() <= width as f64,
                "{report}"
            );
            assert!(
                rect[1].as_f64().unwrap() >= 190.
                    && rect[1].as_f64().unwrap() + rect[3].as_f64().unwrap() <= 700.,
                "{report}"
            );
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {RustLauncher} from '@ROOT@/sdk/rustlauncher.jsx';
import {configureWindows,setPackageApps} from '@ROOT@/sdk/windows.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {dock,dockDrag} from '@ROOT@/sdk/dock.js';
import {TextInput} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
let handle=0,show,underlay='';const opened=[];
const mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount;
lord.nativeViews.mount=(node,app,text,...source)=>{const id=mount(node,app,text,...source);if(app==='shell:launcher')handle=id;return id;};
lord.nativeViews.unmount=id=>{if(id===handle)handle=0;return unmount(id);};
const find=text=>handle?lord.nativeViews.findText(handle,text):0;
const count=()=>new Set((lord.nativeViews.text(handle)||'').match(/App \d{4}/g)||[]).size;
const key=(name,char='')=>{__lordKey(0,1,name,char,name,0);__lordKey(0,0,name,'',name,0);globalThis.__lordNativeView();};
const pointer=(type,node,x,y,button=0)=>{const target=lord.nativeViews.pointer(type,node,x,y,button);__lordEvent(type,target,x,y,button);globalThis.__lordNativeView();};
const click=(label,button=0)=>{const node=find(label),[x,y,w,h]=lord.getLayout(node);for(const type of ['down','up'])pointer(type,node,x+w/2,y+h/2,button);};
async function fresh(){show(false);await sleep(30);show(true);await until(()=>{const node=find('App 0016');return node&&lord.getLayout(node)[2]>0;});await sleep(40);}
async function type(text){for(const char of text){key(char,char);await sleep(12);}await sleep(80);}
const Icon=({size})=>h(View,{style:{width:size,height:size,backgroundColor:'#3489bd'}});
const apps=Array.from({length:2048},(_,i)=>({id:'test-'+String(i).padStart(4,'0'),name:'App '+String(i).padStart(4,'0'),category:i%2?'Game':'Utility',Icon}));
configureWindows({apps,workArea:{x:0,y:0,w:@WIDTH@,h:700}});
settings.set({language:'en',mode:'@MODE@',launcherNativeView:false,reduceMotion:true});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[shown,setShown]=useState(false),[value,setValue]=useState('');show=setShown;return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(View,{style:{position:'absolute',left:-300,top:0,width:200,height:40}},h(TextInput,{autoFocus:true,value,onChange:text=>{underlay=text;setValue(text);}})),
 shown?h(RustLauncher,{area:{x:0,y:0,w:@WIDTH@,h:700},onOpen:id=>opened.push(id),onClose:()=>setShown(false)}):null));}
enableShellLayers();render(h(App));
async function run(){
 await fresh();const initialCount=count(),initialTail=!!find('App 2047');
 const firstY=lord.getLayout(find('App 0000'))[1];
 const firstRow=apps.slice(0,32).map(app=>find(app.name)).filter(Boolean).map(node=>lord.getLayout(node)).filter(rect=>rect[1]===firstY);
 pointer('wheel',lord.nativeViews.hitTest(@WIDTH@/2,300),@WIDTH@/2,300,-100000);await until(()=>{const node=find('App 2047');return node&&lord.getLayout(node)[2]>0;});
 const tailVisible=!!find('App 2047'),tailRect=lord.getLayout(find('App 2047')),tailCount=count();
 const tailHit=lord.nativeViews.hitTest(tailRect[0]+tailRect[2]/2,tailRect[1]+tailRect[3]/2)===find('App 2047');
 const image=lord.media.screenshot('screenshots');const saved=await media.saveImage(image,@IMAGE@);media.close(image);
 click('App 2047');await until(()=>!handle);const openedTail=opened.at(-1)==='test-2047';
 await fresh();await type('App 0999');key('Enter');await until(()=>!handle);const searched=opened.at(-1)==='test-0999';
 await fresh();click('Games');await until(()=>!!find('App 0001')&&!find('App 0000'));const categoryFiltered=!!find('App 0001')&&!find('App 0000');
 click('App 0001',2);await until(()=>!!find('Keep in Dock'));key('Escape');await until(()=>!find('Keep in Dock'));const menuEscaped=!!handle&&!find('Keep in Dock');
 click('App 0001',2);await until(()=>!!find('Keep in Dock'));key('End');key('Enter');await until(()=>dock.isPinned('app:test-0001'));const pinned=dock.isPinned('app:test-0001')&&!!handle;
 click('App 0001',2);await until(()=>!!find('Keep in Dock'));click('Keep in Dock');await until(()=>!dock.isPinned('app:test-0001'));const unpinned=!dock.isPinned('app:test-0001')&&!!handle;
 await fresh();await type('New Package');setPackageApps([{id:'new-package',name:'New Package',package:true,Icon}]);
 await until(()=>(lord.nativeViews.text(handle)||'').split('New Package').length>=3);const installed=(lord.nativeViews.text(handle)||'').split('New Package').length>=3;
 setPackageApps([]);await sleep(80);const removed=(lord.nativeViews.text(handle)||'').split('New Package').length<3;
 await fresh();const node=find('App 0000'),[x,y,w,h]=lord.getLayout(node),before=opened.length;
 pointer('down',node,x+w/2,y+h/2);pointer('move',node,x+w/2+30,y+h/2+30);await until(()=>!handle&&!!dockDrag.get());const dragClosed=!handle&&!!dockDrag.get();
 pointer('up',0,@WIDTH@/2,500);await sleep(50);const dragDidNotLaunch=opened.length===before;
 await fresh();click('Games');key('Escape');await until(()=>!handle);const escaped=!handle;
 await fresh();const outsideNode=lord.nativeViews.hitTest(2,698);for(const event of ['down','up'])pointer(event,outsideNode,2,698);await until(()=>!handle);const outsideClosed=!handle;
 console.log('launcher-observation '+JSON.stringify({initialCount,initialTail,firstRow,tailCount,tailRect,tailVisible,tailHit,openedTail,searched,categoryFiltered,menuEscaped,pinned,unpinned,installed,removed,dragClosed,dragDidNotLaunch,escaped,outsideClosed,inputIsolated:underlay==='',saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('launcher driver: '+(error.stack||error))),200);
"#;
