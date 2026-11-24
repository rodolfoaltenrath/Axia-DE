//! Native screenshot editor through real scene input and SDK image jobs.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn capture_selection_and_annotation_use_native_geometry() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 1000] {
                let driver = DRIVER
                    .replace("@APPEARANCE@", appearance)
                    .replace("@EDITORWIDTH@", if width == 760 { "380" } else { "1000" })
                    .replace("@HEIGHT@", if width == 760 { "360" } else { "680" });
                let kind = format!("screenshots_editor_{appearance}");
                let result = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "editor.png",
                    35,
                    |path| {
                        let image = image::open(path).unwrap();
                        assert_eq!((image.width(), image.height()), (width, 700));
                    },
                );
                for flag in [
                    "nativeAppearance",
                    "mapping",
                    "annotation",
                    "anchor",
                    "textCommitted",
                    "countScope",
                    "textCancelled",
                    "blur",
                    "undo",
                    "allTools",
                    "color",
                    "idle",
                    "copyError",
                    "expired",
                    "copied",
                    "save",
                    "original",
                    "replacement",
                    "singleClose",
                    "late",
                    "done",
                    "saved",
                ] {
                    assert_eq!(
                        result[flag], true,
                        "{appearance}/{mode}/{width}/{flag}: {result}"
                    );
                }
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View,shellHasKeyboard} from '@lordos/runtime';
import Screenshots,{Icon} from '@ROOT@/runtime/native_capture.js';
import {configureWindows,wm,useWindows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
import * as fs from '@ROOT@/sdk/fs.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {resetShortcuts,useShortcuts} from '@ROOT@/sdk/shortcuts.js';
const kind='screenshots_editor_@APPEARANCE@',sleep=ms=>new Promise(r=>setTimeout(r,ms)),annotations=[],closed=[],reports=[];
let current,editor=0,countdown=0,stage='bootstrap',nativeAppearance=true;const offset=@EDITORWIDTH@===380?{x:24,y:60}:{x:0,y:0};
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw),handle=mount(node,id,raw,...rest);if(id==='screenshots'&&facts.args?.view==='editor'){editor=handle;nativeAppearance&&=!('theme' in(facts.context||{}));}if(id==='screenshots'&&facts.args?.view==='countdown')countdown=handle;return handle;};
const action=lord.nativeViews.action;lord.nativeViews.action=(handle,input)=>{if(input.action?.[0]==='editorResult')reports.push(input.action[1]);return action(handle,input);};
const annotate=media.annotate,rawClose=media.close,rawSave=media.saveImage,rawCopy=media.copyImage;
media.annotate=(...args)=>{annotations.push(JSON.parse(JSON.stringify(args)));return annotate(...args);};media.close=id=>{closed.push(id);return rawClose(id);};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',nativeAppearance:'@APPEARANCE@',mode:'@MODE@',shortcutProfile:@EDITORWIDTH@===380?'linux':'macos',effects:false});enableShellLayers();resetShortcuts();
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon},{id:'notes',name:'Source',Component:()=>h(View),Icon:()=>null}]});
function Harness(){const state=useWindows();useShortcuts({grabs:'flow',onSuperTap:()=>{}});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},h(View,{style:{position:'absolute',left:offset.x,top:offset.y,width:@EDITORWIDTH@,height:@HEIGHT@}},current&&wm.get(current)?h(AppHost,{key:current,id:current,focused:true}):null),state.windows.filter(w=>w.id!==current&&w.app==='screenshots').map(w=>{const f=w.pip||wm.frame(w);return h(View,{key:w.id,style:{position:'absolute',left:f.x,top:f.y,width:f.w,height:f.h}},h(AppHost,{id:w.id,focused:state.focused===w.id}));})));}render(h(Harness));
const text=()=>lord.nativeViews.text(editor)||'',ops=()=>annotations.at(-1)?.[1]||[];
async function until(fn,label){stage=label;for(let n=0;n<180&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label+'; '+text());return true;}
function pointer(type,x,y,local=true){if(local){x+=offset.x;y+=offset.y;}const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,target,x,y,0);__lordNativeView();}
function key(name,char='',flags=0){const code=name==='Escape'?1:0;__lordKey(code,1,name,char,name,flags);__lordKey(code,0,name,'',name,flags);__lordNativeView();}
async function click(label){const node=lord.nativeViews.findText(editor,label);if(!node)throw Error('missing '+label+'; '+text());const[x,y,w,h]=lord.getLayout(node);if(x<offset.x||x+w>offset.x+@EDITORWIDTH@+1||y<offset.y||y+h>offset.y+@HEIGHT@)throw Error('offscreen '+label);pointer('down',x+w/2,y+h/2,false);pointer('up',x+w/2,y+h/2,false);await sleep(60);}
async function draw(){const top=@EDITORWIDTH@<700?72:44,h=@HEIGHT@-top;pointer('down',70,top+h*.3);pointer('move',@EDITORWIDTH@*.75,top+h*.8);await sleep(50);pointer('up',@EDITORWIDTH@*.75,top+h*.8);await sleep(70);}
async function snapshot(){await sleep(80);const image=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});const result=await rawSave(image,@IMAGE@);media.close(image);return!!result?.ok;}
async function run(){
 await sleep(120);let image=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});const info=media.info(image),originalPath=fs.home()+'/original.png';await rawSave(image,originalPath);const originalBytes=new Uint8Array(fs.read(originalPath,{binary:true}));
 current=wm.open('screenshots',{instance:'edit-test',args:{handle:image,path:originalPath}});await until(()=>editor&&text().includes('Save')&&annotations.length,'native editor');await sleep(100);
 await draw();const annotation=await until(()=>ops().some(o=>o.type==='arrow'&&o.x2>o.x1&&o.y2>o.y1),'arrow annotation');
 const top=@EDITORWIDTH@<700?72:44,h=@HEIGHT@-top,scale=Math.min(@EDITORWIDTH@/info.width,h/info.height),ox=(@EDITORWIDTH@-info.width*scale)/2,oy=top+(h-info.height*scale)/2,a=ops()[0];
 const mapping=Math.abs(a.x1-(70-ox)/scale)<.01&&Math.abs(a.y1-(top+h*.3-oy)/scale)<.01;
 await click('T');pointer('down',@EDITORWIDTH@*.4,top+h*.45);pointer('up',@EDITORWIDTH@*.4,top+h*.45);await until(()=>lord.nativeViews.findText(editor,'Type and press Enter'),'native text field');
 const field=lord.nativeViews.findText(editor,'Type and press Enter');await until(()=>lord.getLayout(field)[2]>0,'text field layout');const[fx,fy]=lord.getLayout(field),anchor=Math.abs(fx-offset.x-Math.min(@EDITORWIDTH@-260,@EDITORWIDTH@*.4))<1&&Math.abs(fy-offset.y-(top+h*.45))<1;
 key('A','A');const countId=wm.open('screenshots',{instance:'count',args:{secs:10,start:Date.now(),returnWindow:current}});wm.setPip(countId,{x:@WIDTH@/2-60,y:290,w:120,h:120});await until(()=>countdown&&lord.nativeViews.text(countdown)?.includes('10'),'native-field countdown');key('C','C');key('Escape');const countScope=await until(()=>!wm.get(countId)&&!shellHasKeyboard(),'countdown cancelled above native text');await sleep(100);key('B','B');key('Enter');const textCommitted=await until(()=>ops().some(o=>o.type==='text'&&o.text==='ACB'),'native text history restored');key('z','',@EDITORWIDTH@===380?2:16);const undo=await until(()=>ops().length===1,'undo');
 pointer('down',@EDITORWIDTH@*.4,top+h*.45);pointer('up',@EDITORWIDTH@*.4,top+h*.45);await until(()=>lord.nativeViews.findText(editor,'Type and press Enter'),'replacement text field');key('B','B');key('Escape');const textCancelled=await until(()=>!lord.nativeViews.findText(editor,'B')&&ops().length===1,'text cancel');
 for(const glyph of ['▢','#','/']){await click(glyph);await draw();}const allTools=['box','blur','line'].every(tool=>ops().some(o=>o.type===tool));
 pointer('down',251,20);pointer('up',251,20);await draw();const color=ops().some(o=>o.color==='#0a84ff');
 const count=annotations.length;pointer('move',@EDITORWIDTH@-15,@HEIGHT@-20);await sleep(100);const idle=count===annotations.length;
 const committed=ops().length;pointer('down',100,top+h*.3);pointer('move',@EDITORWIDTH@*.6,top+h*.6);await until(()=>ops().length===committed+1,'draft stroke');const other=wm.open('notes');const blur=await until(()=>ops().length===committed,'focus cancels draft');pointer('up',@EDITORWIDTH@*.6,top+h*.6);wm.close(other,true);wm.focus(current);await sleep(80);
 media.copyImage=async()=>({ok:false});await click('Copy');const copyError=await until(()=>text().includes('Could not save'),'copy failure');media.copyImage=rawCopy;
 await click('Copy');const copied=await until(()=>text().includes('Copied to the clipboard'),'copy result');await click('Save');const save=await until(()=>text().includes('Saved to Screenshots'),'save result');const afterBytes=new Uint8Array(fs.read(originalPath,{binary:true})),original=afterBytes.length===originalBytes.length&&afterBytes.every((b,i)=>b===originalBytes[i]);
 const saved=await snapshot();const expired=await until(()=>!text().includes('Saved to Screenshots'),'result expires');
 let resolve;media.copyImage=()=>new Promise(r=>{resolve=r;});await click('Copy');await until(()=>resolve,'pending copy');
 const old=image,next=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});wm.setArgs(current,{handle:next,path:null});await until(()=>closed.filter(id=>id===old).length===1&&annotations.at(-1)?.[0]===next,'replacement');const replacement=ops().length===0;
 const before=reports.length;resolve({ok:true});await sleep(120);const late=reports.length===before&&!text().includes('Copied to the clipboard');media.copyImage=rawCopy;
 await click('Done');const done=await until(()=>!wm.get(current),'done closes editor');await sleep(100);const previewHandles=[...new Set(reports.filter(r=>r.op==='annotate'&&r.handle>0).map(r=>r.handle))],singleClose=closed.filter(id=>id===old).length===1&&closed.filter(id=>id===next).length===1&&closed.every((id,i)=>closed.indexOf(id)===i)&&previewHandles.every(id=>closed.filter(h=>h===id).length===1);
 console.log(kind+'-observation '+JSON.stringify({nativeAppearance,mapping,annotation,anchor,anchorData:{fx,fy,offset,top,h},textCommitted,countScope,textCancelled,blur,undo,allTools,color,idle,copyError,expired,copied,save,original,replacement,singleClose,late,done,saved}));
}
setTimeout(()=>run().catch(async error=>{await snapshot();console.log(kind+'-observation '+JSON.stringify({error:String(error),stage,annotations,closed,reports,text:text()}));}),100);
"#;
