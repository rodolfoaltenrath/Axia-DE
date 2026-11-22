//! Settings consumes native colors and compiled catalogs without theme facts.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and bundles; Settings appearance and wallpaper captures"]
fn settings_colors_and_catalogs_come_from_native_sources_with_stale_js_mirrors() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for section in ["appearance","wallpaper"] {
        let driver=DRIVER.replace("@SECTION@",section);
        let kind=format!("settings_appearance_{section}");
        for mode in ["light","dark"] { for width in [700,1000] {
            let report=ui::run_ui_with_capture(repo,&kind,&driver,mode,width,"settings.png",|path|{
                let pixels=image::open(path).unwrap().into_rgb8();
                let color=if mode=="light" {[46,52,54]} else {[238,238,236]};
                let dim=if mode=="light" {[104,108,109]} else {[170,170,170]};
                let text=pixels.pixels().filter(|p|p.0==color||p.0==dim).count();
                assert!(text>20,"native text palette was not painted: {section}/{mode}/{width}, {text}");
            });
            for flag in ["nativeCapability","mounted","noAppearanceFacts","withoutFacts","staleMirror","retainedAfterPoison","catalogSeen","choiceSaved","saved"] {
                assert_eq!(report[flag],true,"{section}/{mode}/{width}/{flag}: {report}");
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
const appId='settings',section='@SECTION@',kind='settings_appearance_'+section;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(a=>a.id===appId);
// Mount Settings through the session service lease. Isolated JSON/SDK contracts
// remain covered separately by app units and LPK integration.
const app=nativeApp({...manifest,process:'session'}),nativeCapability=manifest.appearanceSource==='native';
let handle=0,noAppearanceFacts=true,factsCount=0,last;
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
const inspect=raw=>{const facts=JSON.parse(raw);noAppearanceFacts&&=!('theme'in(facts.context||{}))&&!('scale'in(facts.context||{}));last=facts;};
lord.nativeViews.mount=(node,id,raw,...sources)=>{if(id===appId){inspect(raw);handle=mount(node,id,raw,...sources);return handle;}return mount(node,id,raw,...sources);};
lord.nativeViews.facts=(id,raw)=>{if(id===handle){inspect(raw);factsCount++;}return send(id,raw);};
const opposite='@MODE@'==='light'?'dark':'light';
settings.set({language:'en',mode:opposite,accent:'azul',effects:false,nativeAppearance:'axia'});
configureWindows({apps:[app],workArea:{x:0,y:0,w:@WIDTH@,h:700}});const id=wm.open(appId,{args:{section}});
const theme=makeTheme({mode:'@MODE@',accent:'rosa',effects:false});
render(h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:10}},
 h(AppWindowContext.Provider,{value:{id,args:{section},focused:true}},h(View,{style:{width:@WIDTH@-20,height:680}},h(app.Component))))));
async function run(){
 const mounted=await until(()=>handle&&lord.nativeViews.text(handle)?.length>0);await sleep(650);
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 const count=factsCount;
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',nativeAppearance:'adwaita'}});await sleep(200);
 const withoutFacts=factsCount===count,staleMirror=settings.get().mode===opposite&&settings.get().accent==='azul';
 send(handle,JSON.stringify({...last,context:{...last.context,theme:{mode:opposite,text:'#ff0000',accent:'#ff0000'},scale:{body:200}}}));
 await sleep(100);const retainedAfterPoison=!!lord.nativeViews.text(handle)?.length;
 const catalogSeen=section!=='wallpaper'||lord.nativeViews.text(handle).includes('Aurora');
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 const label=section==='appearance'?(opposite==='light'?'Light':'Dark'):'Eternal Dawn';
 const node=lord.nativeViews.findText(handle,label);
 if(node){const[x,y,w,h]=lord.getLayout(node);for(const type of ['down','up']){
  const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x+w/2,y+h/2),x+w/2,y+h/2,0);
  __lordEvent(type,target,x+w/2,y+h/2,0);globalThis.__lordNativeView();
 }}
 const choiceSaved=!!node&&await until(()=>{const state=lord.settings.state({cmd:'snapshot'}).state;return section==='appearance'?state.mode===opposite:state.wallpaper==='eternal-dawn';});
 settingsBridge.perform=perform;
 console.log(kind+'-observation '+JSON.stringify({nativeCapability,mounted,noAppearanceFacts,withoutFacts,staleMirror,retainedAfterPoison,catalogSeen,choiceSaved,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error(kind+' driver: '+(error.stack||error))),20);
"#;
