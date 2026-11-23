//! Real touch routing, native recognition and session effects, without JS accumulation.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built desktop and npm ci; required by GPU=1 CI"]
fn system_swipes_use_native_recognition_and_current_session_targets() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light","dark"] {
        for width in [640,1000] {
            let report = ui::run_ui_with_capture(repo,"system_swipes",DRIVER,mode,width,"system-swipes.png",|path| {
                let image = image::open(path).unwrap().into_rgb8();
                let ink = if mode=="light" { [29,29,31] } else { [245,245,247] };
                assert!(image.pixels().filter(|p|p.0==ink).count()>10,"native desktop indicator caption");
            });
            for flag in ["stale","horizontal","threshold","cancelled","vertical","rtl","currentLocale",
                "singleDesktop","appGestures","threeDrag","priority","noRawSystemEvents","noRequests","mirrored","saved"] {
                assert_eq!(report[flag],true,"{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {configureWindows,windows,wm,setDesktops,activeDesktop} from '@ROOT@/sdk/windows.js';
import {registerGestureActions,lastSwipe} from '@ROOT@/sdk/trackpad.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale,direction} from '@ROOT@/sdk/i18n.js';
import {localeBridge} from '@ROOT@/sdk/nativelocale.js';
import {openSwitcher,cancelSwitcher,switcherOpen} from '@ROOT@/sdk/appswitcher.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
configureWindows({apps:['one','two'].map(id=>({id,name:id,Component:()=>null,Icon}))});
settings.set({mode:'@MODE@',accent:'verde',effects:false,shortcutProfile:'macos'});
locale.set({...locale.get(),lang:'en',dir:'ltr'});
let rawSystem=0,rawApp=0,requests=0,up=0,down=0,fingers=0;
const dispatch=globalThis.__lordGesture;
globalThis.__lordGesture=(...args)=>{if(args[0]==='swipe'){if(args[8])rawSystem++;else rawApp++;}return dispatch(...args);};
const request=lord.nativeApps.request;
lord.nativeApps.request=text=>{if(['desktopSession','desktopCommand','windowCommand'].includes(JSON.parse(text).cmd))requests++;return request.call(lord.nativeApps,text);};
registerGestureActions({up:g=>{up++;fingers=g.fingers;},down:()=>{down++;},left:()=>{throw Error('JS horizontal action');},right:()=>{throw Error('JS horizontal action');}});
const raw=(op,...args)=>JSON.parse(lord.windowList.call(op,JSON.stringify([{},...args])));
const state=()=>raw('resync').state;
const input=(phase,n,x=0,y=0)=>{if(!lord.nativeViews.inputSwipe(phase,n,x,y))throw Error('swipe injection rejected');};
const swipe=(x,y=0,n=3)=>{input(1,n);input(2,n,x,y);input(3,n);};
render(h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'verde',effects:{}})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}})));
async function run(){
 lord.input.configure({swipe3:true,swipe4:true,threeDrag:false});
 setDesktops([10,20,30],10);raw('open',{app:'one'});raw('open',{app:'two'});
 const stale=windows.get().length===0;
 input(1,3);input(2,3,-25,10);input(2,3,-40,0);input(3,3);
 const horizontal=state().active===20&&lastSwipe.get()?.dir==='left';
 const previous=lastSwipe.get();swipe(-59.9);const threshold=state().active===20&&lastSwipe.get()===previous;
 input(1,3);input(2,3,-100);input(4,3);input(3,3);const cancelled=state().active===20&&lastSwipe.get()===previous;
 swipe(-60,-60,4);input(3,4);swipe(0,100);
 const vertical=up===1&&down===1&&fingers===4&&state().active===20;
 const perform=localeBridge.perform;localeBridge.perform=()=>{};
 lord.locale.state({cmd:'patch',value:{dir:'rtl'}});swipe(-100);
 const rtl=direction()==='ltr'&&state().active===10;
 input(1,3);input(2,3,-100);lord.locale.state({cmd:'patch',value:{dir:'ltr'}});input(3,3);
 const currentLocale=state().active===20;localeBridge.perform=perform;
 raw('setDesktops',[10],10);raw('focus','two');swipe(-100);const first=state().views.front==='one';swipe(100);
 const singleDesktop=first&&state().views.front==='two';
 const beforeApp=rawApp,last=lastSwipe.get();swipe(-100,0,2);
 const appGestures=rawApp===beforeApp+3&&lastSwipe.get()===last&&state().views.front==='two';
 raw('setDesktops',[10,20],10);lord.input.configure({threeDrag:true});
 swipe(-100);const threeKept=state().active===10;swipe(-100,0,4);
 const threeDrag=threeKept&&state().active===20&&lastSwipe.get()?.fingers===4;
 lord.input.configure({threeDrag:false});openSwitcher(8);const opened=switcherOpen(),beforePriority=lastSwipe.get();
 swipe(100);swipe(0,-100);const priority=opened&&switcherOpen()&&state().active===20&&up===1&&lastSwipe.get()===beforePriority;cancelSwitcher();
 raw('setDesktops',[10,20,30],10);swipe(-100);await sleep(100);
 const mirrored=activeDesktop.get()===20&&wm.focused()===state().views.front;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('system_swipes-observation '+JSON.stringify({stale,horizontal,threshold,cancelled,vertical,rtl,currentLocale,singleDesktop,appGestures,threeDrag,priority,noRawSystemEvents:rawSystem===0,noRequests:requests===0,mirrored,saved:!!saved?.ok,rawSystem,rawApp,requests}));
}
setTimeout(()=>run().catch(error=>console.error('system_swipes driver: '+(error.stack||error))),200);
"#;
