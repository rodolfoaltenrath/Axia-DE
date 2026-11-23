//! Native panel, preferences, privacy and session recording generations.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn capture_panel_and_session_state_survive_close_and_late_recording_results() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 1000] {
                let kind = format!("capture_panel_{appearance}");
                let driver = DRIVER
                    .replace("@APPEARANCE@", appearance)
                    .replace("@PANELW@", if width == 760 { "380" } else { "440" })
                    .replace("@PANELH@", if width == 760 { "360" } else { "420" });
                let result = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "panel.png",
                    45,
                    |path| {
                        let image = image::open(path).unwrap();
                        assert_eq!((image.width(), image.height()), (width, 700));
                        let private =
                            std::path::PathBuf::from(format!("{}.window.png", path.display()));
                        if private.exists() {
                            let image = image::open(&private).unwrap().to_rgba8();
                            assert!(image.width() > 20 && image.height() > 20);
                            assert!(
                                image.pixels().all(|p| p[0] <= 1 && p[1] <= 1 && p[2] <= 1),
                                "protected window crop leaked visible content"
                            );
                            if let Some(out) = std::env::var_os(format!(
                                "AXIA_{}_OUTPUT",
                                kind.to_ascii_uppercase()
                            )) {
                                let out =
                                    std::path::PathBuf::from(out).join(format!("{mode}-{width}"));
                                std::fs::create_dir_all(&out).unwrap();
                                std::fs::copy(&private, out.join("protected-window.png")).unwrap();
                                let controls = format!("{}.controls.png", path.display());
                                if std::path::Path::new(&controls).exists() {
                                    std::fs::copy(controls, out.join("controls.png")).unwrap();
                                }
                            }
                        }
                    },
                );
                for flag in [
                    "nativeAppearance",
                    "preferences",
                    "panelKeys",
                    "persisted",
                    "privacy",
                    "freshPrivacy",
                    "full",
                    "window",
                    "area",
                    "recentPhoto",
                    "recording",
                    "recentVideo",
                    "cancelledPending",
                    "lateDiscarded",
                    "timedCancel",
                    "timedRecording",
                    "micRetained",
                    "micCleanup",
                    "singleClose",
                    "history",
                    "saved",
                    "masked",
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
import {render,View,Text} from '@lordos/runtime';
import Screenshots,{capture,Icon} from '@ROOT@/apps/screenshots/index.jsx';
import {configureWindows,wm,useWindows,windows,appMenus} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
import {perms,indicatorFacts} from '@ROOT@/sdk/privacy.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import * as fs from '@ROOT@/sdk/fs.js';
const kind='capture_panel_@APPEARANCE@',sleep=ms=>new Promise(r=>setTimeout(r,ms)),handles=new Map(),savedPaths=[],closed=[],stops=[],states=[],crops=[];
let stage='bootstrap',nativeAppearance=true,next=700000,defer=false,release=null,deferredId=0;
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw),handle=mount(node,id,raw,...rest);if(id==='screenshots'&&!facts.icon){handles.set(facts.args.view||'selector',handle);nativeAppearance&&=!('theme' in(facts.context||{}));}return handle;};
const create=lord.nativeApps.create;lord.nativeApps.create=id=>{const owner=create(id);return {request(raw){const result=owner.request(raw);if(id==='screenshots'&&JSON.parse(raw).cmd==='session')states.push(result.state);return result;}};};
const rawSave=media.saveImage,rawClose=lord.media.close,rawRecord=media.screenRecord,rawCrop=media.crop;
media.saveImage=async(...args)=>{const result=await rawSave(...args);savedPaths.push(result);return result;};
media.crop=(...args)=>{crops.push(args);return rawCrop(...args);};
lord.media.screenRecord=()=>++next;lord.media.stop=id=>{stops.push(id);return true;};lord.media.close=id=>{closed.push(id);if(id<700000)rawClose(id);};
media.screenRecord=async(...args)=>{const id=await rawRecord(...args);if(defer){deferredId=id;return await new Promise(resolve=>{release=()=>resolve(id);});}return id;};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',nativeAppearance:'@APPEARANCE@',mode:'@MODE@',shortcutProfile:'linux',effects:false,screenshots:{timer:0,clipboard:false,mic:true,future:7}});
perms.set('screenshots','screen','allow');perms.set('screenshots','microphone','allow');enableShellLayers();resetShortcuts();
const Source=()=>h(View,{style:{flex:1,padding:20,backgroundColor:'#227755'}},h(Text,{style:{fontSize:20,color:'#fff'}},'Private source window'));
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon},{id:'notes',name:'Source',Component:Source,Icon:()=>null},{id:'photos',name:'Photo viewer',Component:()=>h(View),Icon:()=>null},{id:'player',name:'Video viewer',Component:()=>h(View),Icon:()=>null}]});
const source=wm.open('notes',{instance:'source',frame:{x:440,y:90,w:240,h:210}});
function Harness(){const state=useWindows();useShortcuts({grabs:'flow',onSuperTap:()=>{}});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},state.windows.map(w=>{const f=w.id==='screenshots'?{x:24,y:50,w:@PANELW@,h:@PANELH@}:w.id==='screenshots#overlay'?{x:0,y:0,w:@WIDTH@,h:700}:w.pip||wm.frame(w);return h(View,{key:w.id,style:{position:'absolute',left:f.x,top:f.y,width:f.w,height:f.h}},h(AppHost,{id:w.id,focused:state.focused===w.id}));})));}render(h(Harness));
const pane=()=>wm.get('screenshots'),thumb=()=>wm.get('screenshots#thumb'),badge=()=>wm.get('screenshots#rec'),state=()=>states.at(-1)||{recent:[],recording:null};
const text=view=>lord.nativeViews.text(handles.get(view)||0)||'';
async function until(fn,label){stage=label;for(let n=0;n<400&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label+'; '+text('panel'));return true;}
function pointer(type,x,y){const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,target,x,y,0);__lordNativeView();}
function key(name){const code=name==='Escape'?1:0;__lordKey(code,1,name,'',name,0);__lordKey(code,0,name,'',name,0);__lordNativeView();}
async function click(view,label){const node=lord.nativeViews.findText(handles.get(view),label);if(!node)throw Error('missing '+view+': '+label);const[x,y,w,h]=lord.getLayout(node);pointer('down',x+w/2,y+h/2);pointer('up',x+w/2,y+h/2);await sleep(80);}
async function showPane(){wm.open('screenshots');await until(()=>pane()&&handles.get('panel')&&text('panel').includes('Record the microphone'),'native panel');await sleep(100);}
async function closeCaptures(){for(const w of windows.get())if(w.app==='screenshots')wm.close(w.id,true);await sleep(100);}
async function snapshot(suffix=''){await sleep(100);const image=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});const result=await rawSave(image,@IMAGE@+suffix);media.close(image);return!!result?.ok;}
function privacy(){const menu=appMenus.get()['screenshots'];if(!menu?.[0]?.items?.[0]?.onClick)throw Error('missing privacy menu');menu[0].items[0].onClick();__lordNativeView();}
function scrollToRecent(){const x=24+@PANELW@-40,y=50+@PANELH@-40;lord.nativeViews.pointer('wheel',lord.nativeViews.hitTest(x,y),x,y,-30);__lordNativeView();}
async function run(){
 await sleep(140);await showPane();for(const name of ['Space','Enter','Escape'])key(name);await sleep(100);const panelKeys=!!pane()&&text('panel').includes('Record the microphone')&&!text('panel').includes('Drag to select an area');if(!panelKeys)throw Error('panel received selector keyboard actions');await click('panel','10 s');await until(()=>settings.get().screenshots.timer===10,'timer persisted');await click('panel','None');
 const clipboard=lord.nativeViews.findText(handles.get('panel'),'Copy to clipboard instead of saving'),[cx,cy,cw,ch]=lord.getLayout(clipboard);pointer('down',24+@PANELW@-20-12-16,cy+ch/2);pointer('up',24+@PANELW@-20-12-16,cy+ch/2);await until(()=>settings.get().screenshots.clipboard===true,'clipboard persisted');
 const mic=lord.nativeViews.findText(handles.get('panel'),'Record the microphone'),[mx,my,mw,mh]=lord.getLayout(mic);pointer('down',24+@PANELW@-20-12-16,my+mh/2);pointer('up',24+@PANELW@-20-12-16,my+mh/2);await until(()=>settings.get().screenshots.mic===false,'microphone persisted');
 const preferences=settings.get().screenshots.future===7;wm.close('screenshots',true);await sleep(100);await showPane();const persisted=settings.get().screenshots.clipboard===true&&settings.get().screenshots.mic===false;
 settings.set({screenshots:{timer:0,clipboard:false,mic:true,future:7}});await sleep(100);privacy();const privacyOk=await until(()=>wm.get(source).protected,'privacy protects source');
 const fresh=wm.open('notes',{instance:'fresh',frame:{x:450,y:110,w:200,h:160}});wm.focus('screenshots');privacy();const freshPrivacy=await until(()=>wm.get(fresh).protected&&wm.get(source).protected,'fresh privacy target');
 await click('panel','Entire screen');const full=await until(()=>thumb()?.args.path&&savedPaths.some(r=>r.ok),'panel full capture');await closeCaptures();
 await showPane();await click('panel','Window');await until(()=>text('selector').includes('Select a window'),'panel window selector');await sleep(120);const f=wm.frame(wm.get(fresh));pointer('move',f.x+f.w/2,f.y+f.h/2);pointer('down',f.x+f.w/2,f.y+f.h/2);pointer('up',f.x+f.w/2,f.y+f.h/2);key('Enter');await until(()=>thumb()?.args.path,'window saved');const window=crops.at(-1)?.slice(1).join(',')===[f.x,f.y,f.w,f.h].join(',');
 fs.write(@IMAGE@+'.window.png',fs.read(thumb().args.path,{binary:true}));const masked=!!fs.stat(@IMAGE@+'.window.png');await closeCaptures();
 await showPane();await click('panel','Selection');await until(()=>text('selector').includes('Drag to select an area'),'panel area selector');await sleep(120);pointer('down',80,100);pointer('move',280,260);pointer('up',280,260);key('Enter');await until(()=>thumb()?.args.path,'area saved');const area=crops.at(-1)?.slice(1).join(',')==='80,100,200,160';await closeCaptures();
 await showPane();scrollToRecent();await sleep(100);const photo=state().recent.find(r=>!r.video);await click('panel',photo.path.split('/').pop());const photoWindow=windows.get().find(w=>w.app==='photos'&&w.args?.path===photo.path);const recentPhoto=!!photoWindow;wm.close(photoWindow.id,true);wm.focus('screenshots');await sleep(80);await closeCaptures();
 await showPane();await click('panel','Record screen');await until(()=>badge()&&state().recording,'panel record');const first=state().recording.id;const recording=indicatorFacts().some(d=>d.kind==='microphone');await showPane();await click('panel','Stop');wm.close('screenshots',true);await sleep(80);__lordMedia(first,'done',{ok:true,path:'/recording-one.mp4'});await until(()=>!badge()&&!state().recording,'record completion without mounted panel');await closeCaptures();
 await showPane();scrollToRecent();await sleep(100);await click('panel','recording-one.mp4');const videoWindow=windows.get().find(w=>w.app==='player'&&w.args?.path==='/recording-one.mp4'),recentVideo=!!videoWindow;wm.close(videoWindow.id,true);wm.focus('screenshots');await closeCaptures();
 defer=true;capture.record();await until(()=>release&&deferredId,'deferred recording');await showPane();await click('panel','Cancel');const cancelledPending=!state().pendingRecord&&!state().recording;
 defer=false;await click('panel','Record screen');await until(()=>badge()&&state().recording,'replacement recording');const current=state().recording.id,old=deferredId;release();await until(()=>closed.filter(id=>id===old).length===1,'late recording discarded');
 const lateDiscarded=state().recording.id===current&&!!badge(),micRetained=indicatorFacts().some(d=>d.kind==='microphone');__lordMedia(old,'done',{ok:true,path:'/must-not-appear.mp4'});await sleep(60);__lordMedia(current,'done',{ok:true,path:'/recording-two.mp4'});await until(()=>!state().recording&&!badge(),'current recording complete');
 settings.set({screenshots:{timer:5,clipboard:false,mic:true,future:7}});const beforeTimer=next;capture.record();await until(()=>wm.get('screenshots#count')&&state().pendingRecord,'recording countdown');key('Escape');const timedCancel=!state().pendingRecord&&!wm.get('screenshots#count')&&next===beforeTimer;
 capture.record();await until(()=>badge()&&state().recording,'timed recording starts after prior cancellation');const timed=state().recording.id,timedRecording=timed>current;capture.record();__lordMedia(timed,'done',{ok:true,path:'/timer.mp4'});await until(()=>!state().recording&&!badge(),'timed recording complete');
 const micCleanup=!indicatorFacts().some(d=>d.kind==='microphone'),singleClose=[first,old,current,timed].every(id=>closed.filter(h=>h===id).length===1),history=state().recent.length===6&&state().recent[0].path==='/timer.mp4'&&!state().recent.some(r=>r.path==='/must-not-appear.mp4');
 await showPane();scrollToRecent();await sleep(100);let saved=await snapshot();const sx=24+@PANELW@-40,sy=50+@PANELH@-40;lord.nativeViews.pointer('wheel',lord.nativeViews.hitTest(sx,sy),sx,sy,100);__lordNativeView();saved&&=await snapshot('.controls.png');
 console.log(kind+'-observation '+JSON.stringify({nativeAppearance,preferences,panelKeys,persisted,privacy:privacyOk,freshPrivacy,full,window,area,recentPhoto,recording,recentVideo,cancelledPending,lateDiscarded,timedCancel,timedRecording,micRetained,micCleanup,singleClose,history,saved,masked}));
}
setTimeout(()=>run().catch(async error=>{await snapshot();console.log(kind+'-observation '+JSON.stringify({error:String(error),stage,states,savedPaths,closed,stops,crops,texts:Object.fromEntries([...handles].map(([k])=>[k,text(k)])),windows:windows.get()}));}),100);
"#;
