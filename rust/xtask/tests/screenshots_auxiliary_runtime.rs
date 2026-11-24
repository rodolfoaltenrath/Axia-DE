//! Native auxiliary windows through the compositor and production SDK transport.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn thumbnail_deadlines_countdown_and_recording_badge_are_native() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 1000] {
                let kind = format!("capture_auxiliary_{appearance}");
                let driver = DRIVER.replace("@APPEARANCE@", appearance).replace(
                    "@DEADLINES@",
                    if appearance == "axia" && mode == "light" && width == 760 {
                        "true"
                    } else {
                        "false"
                    },
                );
                let result = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "auxiliary.png",
                    50,
                    |path| {
                        let png = image::open(path).unwrap();
                        assert_eq!((png.width(), png.height()), (width, 700));
                        if let Some(out) =
                            std::env::var_os(format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase()))
                        {
                            let out = std::path::PathBuf::from(out).join(format!("{mode}-{width}"));
                            std::fs::create_dir_all(&out).unwrap();
                            for name in ["countdown", "recording"] {
                                let source = format!("{}.{name}.png", path.display());
                                if std::path::Path::new(&source).exists() {
                                    std::fs::copy(source, out.join(format!("{name}.png"))).unwrap();
                                }
                            }
                        }
                    },
                );
                for flag in [
                    "nativeAppearance",
                    "clipboardCaption",
                    "hover",
                    "expiry",
                    "replacement",
                    "transfer",
                    "cancel",
                    "countFocusRestored",
                    "closedCountdown",
                    "completed",
                    "recording",
                    "stopRetry",
                    "stopOnce",
                    "micCleanup",
                    "recordingClosed",
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
import {render,View,Text} from '@lordos/runtime';
import Screenshots,{capture,Icon} from '@ROOT@/runtime/native_capture.js';
import {configureWindows,wm,useWindows,windows} from '@ROOT@/sdk/windows.js';
import {AppHost,TextInput,hasTextFocus} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
import {perms,indicatorFacts} from '@ROOT@/sdk/privacy.js';
const kind='capture_auxiliary_@APPEARANCE@',sleep=ms=>new Promise(r=>setTimeout(r,ms)),deadlines=@DEADLINES@;
let stage='bootstrap',nativeAppearance=true,nextRecord=600000;const handles=new Map(),closed=[],stops=[],frames=[],trace=[];
const rawTake=lord.nativeViews.take;lord.nativeViews.take=id=>{const effects=rawTake(id);if(effects?.length)trace.push({at:Date.now(),id,effects,stage});return effects;};
const rawWindowClose=wm.close;wm.close=(id,...args)=>{trace.push({at:Date.now(),close:id,args:wm.get(id)?.args,stage});return rawWindowClose(id,...args);};
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw),handle=mount(node,id,raw,...rest);if(id==='screenshots'&&!facts.icon){handles.set(facts.args.view||'selector',handle);nativeAppearance&&=!('theme' in(facts.context||{}));}return handle;};
const rawFrame=media.screenshot,rawClose=lord.media.close,rawSave=media.saveImage;
media.screenshot=async(...args)=>{const image=await rawFrame(...args);frames.push(image);return image;};
lord.media.close=id=>{closed.push(id);if(id<600000)rawClose(id);};
lord.media.screenRecord=()=>++nextRecord;lord.media.stop=id=>{stops.push(id);if(stops.filter(x=>x===id).length===1)return false;setTimeout(()=>__lordMedia(id,'done',{ok:true,path:'/fixture.mp4'}),200);return true;};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',effects:false,screenshots:{timer:0,clipboard:true,mic:true}});
perms.set('screenshots','screen','allow');perms.set('screenshots','microphone','allow');
const Source=()=>h(View,{style:{flex:1,backgroundColor:'#227755',padding:20}},h(Text,{style:{fontSize:20,color:'#fff'}},'Capture source'),h(TextInput,{autoFocus:true,value:'Source text',onChange:()=>{}}));
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon},{id:'notes',name:'Source',Component:Source,Icon:()=>null}]});
const source=wm.open('notes',{frame:{x:40,y:70,w:300,h:240}});
function Harness(){const state=useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},state.windows.map(w=>{const f=w.pip||wm.frame(w);return h(View,{key:w.id,style:{position:'absolute',left:f.x,top:f.y,width:f.w,height:f.h}},h(AppHost,{id:w.id,focused:state.focused===w.id}));})));}render(h(Harness));
const thumb=()=>wm.get('screenshots#thumb'),count=()=>wm.get('screenshots#count'),badge=()=>wm.get('screenshots#rec');
const text=view=>lord.nativeViews.text(handles.get(view)||0)||'';
async function until(fn,label){stage=label;for(let n=0;n<350&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label);return true;}
function pointer(type,x,y){const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,target,x,y,0);__lordNativeView();}
function key(name){__lordKey(0,1,name,'',name,0);__lordKey(0,0,name,'',name,0);__lordNativeView();}
async function click(view,label){const node=lord.nativeViews.findText(handles.get(view),label);if(!node)throw Error('missing '+view+': '+label);const[x,y,w,h]=lord.getLayout(node);pointer('down',x+w/2,y+h/2);pointer('up',x+w/2,y+h/2);await sleep(20);}
async function image(suffix=''){await sleep(80);const id=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});const result=await rawSave(id,@IMAGE@+suffix);media.close(id);return!!result?.ok;}
async function closeScreens(){for(const w of windows.get())if(w.app==='screenshots')wm.close(w.id,true);await sleep(80);}
function openThumb(handle,path=null){const id=wm.open('screenshots',{instance:'thumb',args:{handle,path}});wm.setPip(id,{x:@WIDTH@-260,y:460,w:240,h:150});return id;}
async function run(){
 await sleep(140);let id=lord.media.screenshot('screenshots');openThumb(id);await until(()=>text('thumbnail').includes('Copied'),'clipboard thumbnail');const clipboardCaption=text('thumbnail').includes('Copied'),saved=await image();
 pointer('move',@WIDTH@-140,520);let hover=true,expiry=true;
 if(deadlines){await sleep(8300);hover=!!thumb();pointer('move',10,10);await sleep(4000);expiry=!!thumb();await until(()=>!thumb(),'thumbnail expiry');expiry&&=closed.filter(x=>x===id).length===1;}
 else {pointer('move',10,10);wm.close(thumb().id,true);await sleep(80);}
 id=lord.media.screenshot('screenshots');openThumb(id);await sleep(120);const next=lord.media.screenshot('screenshots');wm.setArgs(thumb().id,{handle:next,path:'/saved.png'});await until(()=>closed.filter(x=>x===id).length===1&&text('thumbnail').includes('Saved to Screenshots'),'thumbnail replacement');
 const replacement=closed.filter(x=>x===id).length===1;pointer('down',@WIDTH@-140,520);pointer('up',@WIDTH@-140,520);await until(()=>wm.get('screenshots#edit'+next)&&!thumb(),'thumbnail transfer');
 let transfer=!closed.includes(next);await closeScreens();transfer&&=closed.filter(x=>x===next).length===1;
 pointer('down',120,130);pointer('up',120,130);await until(()=>hasTextFocus(),'source text focused');
 settings.set({screenshots:{timer:5,clipboard:true,mic:true}});let before=frames.length;capture.full();await until(()=>count()&&text('countdown').includes('5')&&!hasTextFocus(),'countdown cancel');wm.focus(count().id);await sleep(40);key('Escape');await until(()=>!count(),'Escape cancels countdown');await until(()=>hasTextFocus(),'source text focus restored');
 const cancel=frames.length===before,countFocusRestored=hasTextFocus();
 capture.full();await until(()=>count(),'countdown close');wm.close(count().id,true);if(deadlines)await sleep(5300);else await sleep(100);const closedCountdown=frames.length===before;
 capture.full();await until(()=>count()&&text('countdown').includes('5'),'countdown start');if(!await image('.countdown.png'))throw Error('countdown capture failed');wm.focus(source);await until(()=>thumb()&&frames.length===before+1,'background countdown completes');const completed=!!thumb()&&text('thumbnail').includes('Copied');await closeScreens();
 settings.set({screenshots:{timer:0,clipboard:true,mic:true}});capture.record();await until(()=>badge()&&text('recording').includes('Stop'),'recording badge');const rec=nextRecord,mic=indicatorFacts().some(d=>d.kind==='microphone');await until(()=>text('recording').includes('0:01'),'elapsed second');if(!await image('.recording.png'))throw Error('recording capture failed');const recording=mic&&!!badge();
 await click('recording','Stop');const stopRetry=stops.filter(x=>x===rec).length===1&&!!badge();await click('recording','Stop');await click('recording','Stop');const stopOnce=stops.filter(x=>x===rec).length===2;await until(()=>!badge(),'recording complete');const micCleanup=!indicatorFacts().some(d=>d.kind==='microphone'),recordingClosed=closed.filter(x=>x===rec).length===1;
 console.log(kind+'-observation '+JSON.stringify({nativeAppearance,clipboardCaption,hover,expiry,replacement,transfer,cancel,countFocusRestored,closedCountdown,completed,recording,stopRetry,stopOnce,micCleanup,recordingClosed,saved}));
}
setTimeout(()=>run().catch(async error=>{await image();console.log(kind+'-observation '+JSON.stringify({error:String(error),stage,closed,stops,frames,trace,windows:windows.get(),texts:Object.fromEntries([...handles].map(([k])=>[k,text(k)]))}));}),100);
"#;
