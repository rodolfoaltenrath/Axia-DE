//! Shared native appearance in the session views of the migrated apps.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
const APPS: &[&str] = &["notes","characters","help","backup","contacts","calendar","reminders","recorder","camera","scanner","logs","fonts","photos"];

#[test]
#[ignore = "requires built desktop and bundles; native app appearance and captures"]
fn migrated_app_views_observe_native_appearance_without_theme_transport() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for app in APPS {
        let driver=DRIVER.replace("@APP_ID@",app);
        let kind=format!("appearance_{app}");
        for mode in ["light","dark"] { for width in [640,1000] {
            let report=ui::run_ui_with_capture(repo,&kind,&driver,mode,width,"appearance.png",|path|{
                let pixels=image::open(path).unwrap().into_rgb8();
                if *app != "camera" {
                    let color=if mode=="light" {[46,52,54]} else {[238,238,236]};
                    let dim=if mode=="light" {[104,108,109]} else {[170,170,170]};
                    let text=pixels.pixels().filter(|p|p.0==color||p.0==dim).count();
                    assert!(text>20,"native text palette was not painted: {app}/{mode}/{width}, {text}");
                }
            });
            for flag in ["nativeCapability","mounted","noAppearanceFacts","withoutFacts","staleMirror","retainedAfterPoison","saved"] {
                assert_eq!(report[flag],true,"{app}/{mode}/{width}/{flag}: {report}");
            }
        } }
    }
}

const DRIVER:&str=r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/runtime/nativeapps.js';
import {AppWindowContext} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {media} from '@ROOT@/sdk/media.js';
const appId='@APP_ID@',kind='appearance_'+appId;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(a=>a.id===appId);
// Exercise the in-session view path, including the fallback of packaged PIM apps.
// Their isolated JSON/SDK contract remains covered by app units and LPK tests.
const app=nativeApp({...manifest,process:'session'}),nativeCapability=manifest.appearanceSource==='native';
let handle=0,noAppearanceFacts=true,factsCount=0,last;
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
const inspect=raw=>{const facts=JSON.parse(raw);noAppearanceFacts&&=!('theme'in(facts.context||{}))&&!('scale'in(facts.context||{}));last=facts;};
lord.nativeViews.mount=(node,id,raw,...sources)=>{if(id===appId){inspect(raw);handle=mount(node,id,raw,...sources);return handle;}return mount(node,id,raw,...sources);};
lord.nativeViews.facts=(id,raw)=>{if(id===handle){inspect(raw);factsCount++;}return send(id,raw);};
const opposite='@MODE@'==='light'?'dark':'light';
settings.set({language:'en',mode:opposite,accent:'azul',effects:false,nativeAppearance:'axia'});
configureWindows({apps:[app],workArea:{x:0,y:0,w:@WIDTH@,h:700}});const id=wm.open(appId);
const theme=makeTheme({mode:'@MODE@',accent:'rosa',effects:false});
render(h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:10}},
 h(AppWindowContext.Provider,{value:{id,args:{},focused:true}},h(View,{style:{width:@WIDTH@-20,height:680}},h(app.Component))))));
async function run(){
 const mounted=await until(()=>handle&&lord.nativeViews.text(handle)?.length>0);await sleep(250);
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 const count=factsCount;
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',nativeAppearance:'adwaita'}});await sleep(200);
 const withoutFacts=factsCount===count,staleMirror=settings.get().mode===opposite&&settings.get().accent==='azul';
 send(handle,JSON.stringify({...last,context:{...last.context,theme:{mode:opposite,text:'#ff0000',accent:'#ff0000'},scale:{body:200}}}));
 await sleep(100);const retainedAfterPoison=!!lord.nativeViews.text(handle)?.length;
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 settingsBridge.perform=perform;
 console.log(kind+'-observation '+JSON.stringify({nativeCapability,mounted,noAppearanceFacts,withoutFacts,staleMirror,retainedAfterPoison,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error(kind+' driver: '+(error.stack||error))),20);
"#;
