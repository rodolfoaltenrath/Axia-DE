//! Transitional capture UI calling production Rust geometry with real scene input.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn capture_selection_and_annotation_use_native_geometry() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        let result = ui::run_ui_with_capture(repo, "screenshots_geometry", DRIVER, mode, 1000, "editor.png", |path| {
            let image=image::open(path).unwrap();
            assert_eq!((image.width(),image.height()),(1000,700));
        });
        for flag in ["selection", "crop", "mapping", "annotation", "anchor", "textCommitted", "undo", "allTools", "color", "idle", "saved"] {
            assert_eq!(result[flag],true,"{mode}/{flag}: {result}");
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import Screenshots,{Icon} from '@ROOT@/apps/screenshots/index.jsx';
import {configureWindows,wm,useWindows,windows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(r=>setTimeout(r,ms)),requests=[],crops=[],annotations=[],markup=[];
let current,selector=0,stage='bootstrap';
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,...args)=>{const handle=mount(node,id,...args);if(id==='screenshots')selector=handle;return handle;};
locale.set({...locale.get(),lang:'en'});settings.set({mode:'@MODE@',effects:false,screenshots:{clipboard:true}});
const create=lord.nativeApps.create;
lord.nativeApps.create=id=>{const model=create(id);return{request(raw){const input=JSON.parse(raw);if(id==='screenshots')requests.push(input);const reply=model.request(raw);if(id==='screenshots'&&input.cmd==='markup')markup.push({input,reply});return reply;}};};
const crop=media.crop,annotate=media.annotate;
media.crop=(...args)=>{crops.push(args);return crop(...args);};
media.annotate=(...args)=>{annotations.push(JSON.parse(JSON.stringify(args)));return annotate(...args);};
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon}]});
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},current&&wm.get(current)?h(AppHost,{key:current,id:current,focused:true}):null));}
render(h(Harness));
async function until(fn,label){stage=label;for(let n=0;n<120&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label);return true;}
function pointer(type,x,y){const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,target,x,y,0);__lordNativeView();}
function key(name,char='',flags=0){__lordKey(0,1,name,char,name,flags);__lordKey(0,0,name,'',name,flags);__lordNativeView();}
async function run(){
 await sleep(120);let image=lord.media.screenshot('screenshots');current=wm.open('screenshots',{instance:'overlay',args:{handle:image,mode:'area'}});await sleep(180);
 pointer('down',300,300);pointer('move',100,120);await sleep(100);pointer('up',100,120);
 const selection=await until(()=>selector&&lord.nativeViews.text(selector).includes('Capture'),'native selector');
 if(crops.length)throw Error('capture happened before confirmation');key('Enter');
 const cropped=await until(()=>crops.length>0,'crop');const crop=cropped&&crops[0].slice(1).join(',')==='100,120,200,180';
 await sleep(180);for(const w of windows.get())wm.close(w.id,true);
 image=lord.media.screenshot('screenshots');current=wm.open('screenshots',{instance:'edit-test',args:{handle:image,path:null}});await sleep(200);
 pointer('down',140,190);pointer('move',370,340);await sleep(100);pointer('up',370,340);await sleep(100);
 const mapping=await until(()=>requests.some(r=>r.cmd==='markup'&&r.op==='down'),'editor mapping');
 const annotation=await until(()=>annotations.some(a=>a[1].some(op=>op.type==='arrow'&&op.x2>op.x1&&op.y2>op.y1)),'arrow annotation');
 // Activate the text tool and place its real input over the image.
 pointer('down',87,20);pointer('up',87,20);await sleep(80);pointer('down',200,250);pointer('up',200,250);
 await until(()=>requests.some(r=>r.cmd==='anchor'),'text input anchor');
 const input=requests.filter(r=>r.cmd==='anchor').slice(-1)[0],native=lord.nativeApps.create('screenshots');
 const anchor=native.request(JSON.stringify(input));
 const anchorOk=Math.abs(anchor.left-(200-input.viewport.x))<1e-8&&Math.abs(anchor.top-(250-input.viewport.y))<1e-8;
 key('A','A');key('Enter');
 const textCommitted=await until(()=>markup.some(m=>m.input.op==='textSubmit'&&m.reply.ops.some(o=>o.type==='text'&&o.text==='A')),'native text history');
 key('z','',2);const undo=await until(()=>markup.some(m=>m.input.op==='undo'&&m.reply.ops.length===1),'native undo');
 for(const x of [58,116,145]){pointer('down',x,20);pointer('up',x,20);await sleep(40);pointer('down',180,420);pointer('move',340,490);await sleep(40);pointer('up',340,490);await sleep(40);}
 const allTools=await until(()=>['box','blur','line'].every(tool=>markup.some(m=>m.reply.ops.some(o=>o.type===tool))),'native tools');
 pointer('down',280,20);pointer('up',280,20);await sleep(40);pointer('down',400,420);pointer('move',540,490);await sleep(40);pointer('up',540,490);
 const color=await until(()=>markup.some(m=>m.reply.ops.some(o=>o.color==='#0a84ff')),'native color');
 await sleep(80);const count=annotations.length;pointer('move',650,550);await sleep(80);const idle=annotations.length===count;
 image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('screenshots_geometry-observation '+JSON.stringify({selection,crop,mapping,annotation,anchor:anchorOk,textCommitted,undo,allTools,color,idle,saved:!!result?.ok,requests:requests.map(r=>[r.cmd,r.op])}));
}
setTimeout(()=>run().catch(async error=>{const image=lord.media.screenshot('screenshots');await media.saveImage(image,@IMAGE@);media.close(image);console.log('screenshots_geometry-observation '+JSON.stringify({error:String(error),stage,requests,crops,annotations}));}),100);
"#;
