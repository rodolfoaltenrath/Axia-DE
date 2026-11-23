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
        for flag in ["selection", "crop", "mapping", "annotation", "anchor", "saved"] {
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
const sleep=ms=>new Promise(r=>setTimeout(r,ms)),requests=[],crops=[],annotations=[];
let current,stage='bootstrap';
locale.set({...locale.get(),lang:'en'});settings.set({mode:'@MODE@',effects:false,screenshots:{clipboard:true}});
const create=lord.nativeApps.create;
lord.nativeApps.create=id=>{const model=create(id);return{request(raw){if(id==='screenshots')requests.push(JSON.parse(raw));return model.request(raw);}};};
const crop=media.crop,annotate=media.annotate;
media.crop=(...args)=>{crops.push(args);return crop(...args);};
media.annotate=(...args)=>{annotations.push(JSON.parse(JSON.stringify(args)));return annotate(...args);};
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon}]});
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},current&&wm.get(current)?h(AppHost,{key:current,id:current,focused:true}):null));}
render(h(Harness));
async function until(fn,label){stage=label;for(let n=0;n<120&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label);return true;}
function pointer(type,x,y){const target=lord.nativeViews.hitTest(x,y);__lordEvent(type,target,x,y,0);}
async function run(){
 await sleep(120);let image=lord.media.screenshot('screenshots');current=wm.open('screenshots',{instance:'overlay',args:{handle:image,mode:'area'}});await sleep(180);
 pointer('down',300,300);pointer('move',100,120);await sleep(100);pointer('up',100,120);
 const selection=await until(()=>requests.some(r=>r.cmd==='selection'),'selection input');
 const cropped=await until(()=>crops.length>0,'crop');const crop=cropped&&crops[0].slice(1).join(',')==='100,120,200,180';
 await sleep(180);for(const w of windows.get())wm.close(w.id,true);
 image=lord.media.screenshot('screenshots');current=wm.open('screenshots',{instance:'edit-test',args:{handle:image,path:null}});await sleep(200);
 pointer('down',140,190);pointer('move',370,340);await sleep(100);pointer('up',370,340);await sleep(100);
 const mapping=await until(()=>requests.some(r=>r.cmd==='map'),'editor mapping');
 const annotation=await until(()=>annotations.some(a=>a[1].some(op=>op.type==='arrow'&&op.x2>op.x1&&op.y2>op.y1)),'arrow annotation');
 // Activate the text tool and place its real input over the image.
 pointer('down',87,20);pointer('up',87,20);await sleep(80);pointer('down',200,250);pointer('up',200,250);
 await until(()=>requests.some(r=>r.cmd==='anchor'),'text input anchor');
 const input=requests.filter(r=>r.cmd==='anchor').slice(-1)[0],native=lord.nativeApps.create('screenshots');
 const anchor=native.request(JSON.stringify(input));
 const anchorOk=Math.abs(anchor.left-(200-input.viewport.x))<1e-8&&Math.abs(anchor.top-(250-input.viewport.y))<1e-8;
 image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('screenshots_geometry-observation '+JSON.stringify({selection,crop,mapping,annotation,anchor:anchorOk,saved:!!result?.ok,requests:requests.map(r=>r.cmd)}));
}
setTimeout(()=>run().catch(async error=>{const image=lord.media.screenshot('screenshots');await media.saveImage(image,@IMAGE@);media.close(image);console.log('screenshots_geometry-observation '+JSON.stringify({error:String(error),stage,requests,crops,annotations}));}),100);
"#;
