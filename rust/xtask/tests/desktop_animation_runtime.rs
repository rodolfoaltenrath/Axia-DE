//! Real WindowShell bindings and native completion, with JS animation entry points disabled.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built desktop and npm ci; required by GPU=1 CI"]
fn desktop_slides_use_native_bindings_and_finish_without_js_callbacks() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_capture(
                repo,
                "desktop_animation",
                DRIVER,
                mode,
                width,
                "desktop-animation.png",
                |path| {
                    let image = image::open(path).unwrap().to_rgba8();
                    assert_eq!(
                        image.get_pixel(400, 180).0,
                        [46, 129, 198, 255],
                        "the arriving frame must restore its original opacity and position"
                    );
                    let reduced = image::open(format!("{}.reduced.png", path.display())).unwrap().to_rgba8();
                    assert_eq!(reduced.get_pixel(400, 180).0, [46,129,198,255],
                        "native reduced motion must render the final frame immediately");
                    let edited = image::open(format!("{}.edited.png", path.display()))
                        .unwrap()
                        .to_rgba8();
                    let background = if mode == "dark" {
                        [32, 33, 38, 255]
                    } else {
                        [233, 235, 239, 255]
                    };
                    assert_eq!(
                        edited.get_pixel(310, 180).0,
                        background,
                        "an explicit transform must survive native cancellation and completion"
                    );
                },
            );
            for flag in [
                "bound",
                "noJsAnimation",
                "noCallbacks",
                "noFrameRenders",
                "remounted",
                "reduced",
                "closed",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {configureWindows,wm,useWindows,setActiveDesktop} from '@ROOT@/sdk/windows.js';
import {WindowShell} from '@ROOT@/sdk/windowshell.jsx';
import {DesktopSession,startDesktops} from '@ROOT@/sdk/desktops.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
locale.set({...locale.get(),lang:'@MODE@'==='dark'?'ar':'en',dir:'@MODE@'==='dark'?'rtl':'ltr'});
settings.set({desktopCount:2,reduceMotion:false});startDesktops();
configureWindows({apps:[{id:'notes',name:'Notes',Component:()=>null,Icon:()=>null}]});
const a=wm.open('notes',{instance:'a',desktop:1,frame:{x:60,y:120,w:220,h:160}}),
 b=wm.open('notes',{instance:'b',desktop:2,frame:{x:300,y:120,w:220,h:160}});
setActiveDesktop(1);
const source=lord.windowList,bind=source.bindFx,bindings=new Map();let bindingCalls=0,unbindings=0;
source.bindFx=(id,node)=>{bindingCalls++;if(node)bindings.set(id,node);else{bindings.delete(id);unbindings++;}return bind.call(source,id,node);};
let jsAnimations=0,callbacks=0,renders=0,remount,handle=0,serviceFacts=0,noPreferencePayload=true;
lord.anim.slide=()=>{jsAnimations++;throw Error('desktop slide ran through JS');};
lord.anim.clear=()=>{jsAnimations++;throw Error('desktop clear ran through JS');};
globalThis.__lordAnimDone=()=>{callbacks++;};
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
lord.nativeViews.mount=(node,id,text,...sources)=>{const h=mount(node,id,text,...sources);if(id==='shell:desktops'){handle=h;noPreferencePayload&&=!('reduceMotion'in(JSON.parse(text).args||{}));}return h;};
lord.nativeViews.facts=(id,text)=>{if(id===handle){serviceFacts++;noPreferencePayload&&=!('reduceMotion'in(JSON.parse(text).args||{}));}return send(id,text);};
function Windows(){const{mounted}=useWindows();renders++;return mounted.map(win=>h(WindowShell,{key:win.id,win,style:{backgroundColor:win.id===b?'#2e81c6':'#d04a4a'}},null));}
function App(){const[rev,setRev]=useState(0);remount=()=>setRev(n=>n+1);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},h(Windows),h(DesktopSession,{key:rev})));}
render(h(App));
async function run(){
 await until(()=>bindings.has(a)&&bindings.has(b)&&handle>0);await sleep(100);
 const bound=bindingCalls>=2;setActiveDesktop(2);await sleep(60);const before=renders;await sleep(230);const noFrameRenders=renders===before;
 setActiveDesktop(1);await sleep(25);setActiveDesktop(2);await sleep(25);setActiveDesktop(1);await sleep(250);
 const old=handle;remount();const remounted=await until(()=>handle!==old);
 const beforePreferences=serviceFacts;
 lord.settings.state({cmd:'patch',patch:{reduceMotion:true}});await sleep(40);
 const stale=settings.get().reduceMotion===false;
 setActiveDesktop(2);await sleep(20);
 const reducedImage=lord.media.screenshot('screenshots'),reducedSaved=await media.saveImage(reducedImage,@IMAGE@+'.reduced.png');media.close(reducedImage);
 const reduced=stale&&noPreferencePayload&&serviceFacts===beforePreferences&&!!reducedSaved?.ok;
 setActiveDesktop(1);await sleep(60);
 settings.set({reduceMotion:false});await sleep(80);setActiveDesktop(2);await sleep(300);
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 setActiveDesktop(1);await sleep(25);setActiveDesktop(2);await sleep(30);
 lord.setStyle(bindings.get(b),'translateX',23);await sleep(240);
 const edited=lord.media.screenshot('screenshots');await media.saveImage(edited,@IMAGE@+'.edited.png');media.close(edited);
 wm.close(b);const closed=await until(()=>!bindings.has(b)&&unbindings>0);
 console.log('desktop_animation-observation '+JSON.stringify({bound,noJsAnimation:jsAnimations===0,noCallbacks:callbacks===0,noFrameRenders,remounted,reduced,closed,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('desktop_animation driver: '+(error.stack||error))),100);
"#;
