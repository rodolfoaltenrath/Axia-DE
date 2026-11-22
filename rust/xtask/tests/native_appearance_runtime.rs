//! Native Clock consumes shared appearance without React theme facts.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built desktop and bundles; native appearance and pixels"]
fn native_clock_observes_appearance_without_js_theme_facts() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light","dark"] { for width in [640,1000] {
        let report=ui::run_ui_with_capture(repo,"native_appearance",DRIVER,mode,width,"appearance.png",|path|{
            let pixels=image::open(path).unwrap().into_rgb8();
            let green=pixels.pixels().filter(|p|p[0]==48&&p[1]==209&&p[2]==88).count();
            assert!(green>500,"native preference color was not painted: {mode}/{width}, {green}");
        });
        for flag in ["nativeCapability","noAppearanceFacts","withoutFacts","staleMirror","retainedAfterPoison","saved"] {
            assert_eq!(report[flag],true,"{mode}/{width}/{flag}: {report}");
        }
    } }
}
const DRIVER:&str=r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/sdk/nativeapps.js';
import {AppWindowContext} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
const app=nativeApp(lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(a=>a.id==='clock'));
const nativeCapability=app.appearanceSource==='native';
let handle=0,noAppearanceFacts=true,factsCount=0,last;
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
const inspect=raw=>{const facts=JSON.parse(raw);noAppearanceFacts&&=!('theme'in(facts.context||{}))&&!('scale'in(facts.context||{}));last=facts;};
lord.nativeViews.mount=(node,id,raw,...sources)=>{if(id==='clock'){inspect(raw);handle=mount(node,id,raw,...sources);return handle;}return mount(node,id,raw,...sources);};
lord.nativeViews.facts=(id,raw)=>{if(id===handle){inspect(raw);factsCount++;}return send(id,raw);};
settings.set({language:'en',mode:'@MODE@',accent:'azul',effects:false,nativeAppearance:'adwaita',clock:{}});
configureWindows({apps:[app],workArea:{x:0,y:0,w:@WIDTH@,h:700}});const id=wm.open('clock',{args:{tab:'timer'}});
const theme=makeTheme({mode:'@MODE@',accent:'rosa',effects:false});
render(h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:20}},
 h(AppWindowContext.Provider,{value:{id,args:{tab:'timer'},focused:true}},h(View,{style:{width:@WIDTH@-40,height:600}},h(app.Component))))));
async function run(){
 await until(()=>handle&&lord.nativeViews.findText(handle,'Start'));await sleep(200);
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 const count=factsCount;
 lord.settings.state({cmd:'patch',patch:{accent:'violeta'}});await sleep(150);
 lord.settings.state({cmd:'patch',patch:{accent:'verde'}});await sleep(150);
 const withoutFacts=factsCount===count,staleMirror=settings.get().accent==='azul';
 // A delayed old adapter cannot overwrite the appearance owner.
 send(handle,JSON.stringify({...last,context:{...last.context,theme:{mode:'light',accent:'#ff0000'},scale:{body:200}}}));
 await sleep(100);const retainedAfterPoison=!!lord.nativeViews.findText(handle,'Start');
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 settingsBridge.perform=perform;
 console.log('native_appearance-observation '+JSON.stringify({nativeCapability,noAppearanceFacts,withoutFacts,staleMirror,retainedAfterPoison,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('native_appearance driver: '+(error.stack||error))),20);
"#;
