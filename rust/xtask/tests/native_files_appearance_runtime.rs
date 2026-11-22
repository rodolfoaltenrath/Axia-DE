//! Files browser, dialogs and surviving transfers consume the resident native palette.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and bundles; Files appearance and captures"]
fn files_surfaces_follow_native_appearance_without_js_theme_requests() {
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for surface in ["browser","open","save","folder","transfer"] {
        let driver=DRIVER.replace("@SURFACE@",surface);
        let kind=format!("files_appearance_{surface}");
        for mode in ["light","dark"] { for width in [640,1000] {
            let report=ui::run_ui_with_capture(repo,&kind,&driver,mode,width,"appearance.png",|path|{
                let pixels=image::open(path).unwrap().into_rgb8();
                let color=if mode=="light" {[46,52,54]} else {[238,238,236]};
                let dim=if mode=="light" {[104,108,109]} else {[170,170,170]};
                assert!(pixels.pixels().filter(|p|p.0==color||p.0==dim).count()>20,
                    "native text palette missing: {surface}/{mode}/{width}");
            });
            for flag in ["capability","mounted","noThemeFacts","noThemeRequests","withoutFacts","staleMirror","nativePalette","afterPoison","orphaned","saved"] {
                assert_eq!(report[flag],true,"{surface}/{mode}/{width}/{flag}: {report}");
            }
        } }
    }
}

const DRIVER:&str=r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/sdk/nativeapps.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {AppWindowContext} from '@ROOT@/sdk/components.jsx';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {FilePicker} from '@ROOT@/sdk/filepicker.jsx';
import {RustTransfers} from '@ROOT@/sdk/rustfiles.js';
import {media} from '@ROOT@/sdk/media.js';
const surface='@SURFACE@',kind='files_appearance_'+surface;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
let handle=0,serviceId=0,attached=false,lastFacts,lastAppearance,noThemeFacts=true,noThemeRequests=true,factsCount=0;
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts,request=lord.files.request;
const inspect=raw=>{lastFacts=JSON.parse(raw);noThemeFacts&&=!('theme'in(lastFacts.context||{}))&&!('scale'in(lastFacts.context||{}));};
lord.nativeViews.mount=(node,id,raw,...sources)=>{if(id==='files'&&!JSON.parse(raw).icon){inspect(raw);handle=mount(node,id,raw,...sources);return handle;}return mount(node,id,raw,...sources);};
lord.nativeViews.facts=(id,raw)=>{if(id===handle){factsCount++;inspect(raw);}return send(id,raw);};
lord.files.request=raw=>{const input=JSON.parse(raw);noThemeRequests&&=!('theme'in input);const reply=request(raw);
 if(input.op==='create')serviceId=reply.id;if(input.op==='transferAttach')attached=true;
 if(reply?.appearance)lastAppearance=reply.appearance;return reply;};
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(a=>a.id==='files');
const app=nativeApp(manifest),capability=manifest.appearanceSource==='native';
configureWindows({apps:[app],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
const opposite='@MODE@'==='light'?'dark':'light';
settings.set({language:'en',mode:opposite,accent:'azul',nativeAppearance:'axia',effects:false});
lord.fs.mkdir('appearance-files');lord.fs.write('appearance-files/keep.txt','unchanged bytes');
const base=lord.fs.home()+'/appearance-files';
if(surface==='transfer')lord.files.request(JSON.stringify({op:'create',mode:'browser',args:{path:base}}));
const id=wm.open('files',{args:{path:base}}),theme={...makeTheme({mode:opposite,accent:'rosa',effects:false}),text:'#ff0000'};
const child=surface==='browser'?h(app.Component):surface==='transfer'?h(RustTransfers,{owner:serviceId}):h(FilePicker,{mode:surface,start:base,name:'save.txt',onPick:()=>{},onClose:()=>{}});
enableShellLayers();
render(h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(AppWindowContext.Provider,{value:{id,args:{path:base},focused:true}},h(View,{style:{width:@WIDTH@,height:700}},child)))));
async function run(){
 const mounted=await until(()=>surface==='browser'?handle&&lord.nativeViews.text(handle).includes('keep.txt'):serviceId&&(surface!=='transfer'||attached));
 await sleep(250);
 let orphaned=true;if(surface==='transfer'){lord.files.request(JSON.stringify({op:'close',id:serviceId}));orphaned=attached;}
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};const count=factsCount;
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',nativeAppearance:'adwaita'}});
 const current=()=>lastAppearance?.mode==='@MODE@'&&lastAppearance?.accent==='#30d158'&&lastAppearance?.nativeStyle==='adwaita';
 const nativePalette=surface==='browser'?true:await until(current);await sleep(250);
 const withoutFacts=count===factsCount,staleMirror=settings.get().mode===opposite&&settings.get().accent==='azul';
 const poison={mode:opposite,text:'#ff0000',accent:'#ff0000',nativeAppearance:'axia'};
 let servicePoisonRejected=true;
 if(surface==='browser')send(handle,JSON.stringify({...lastFacts,context:{...lastFacts.context,theme:poison,scale:{body:200}}}));
 else {const reply=request(JSON.stringify({op:surface==='transfer'?'transferPoll':'poll',id:serviceId,theme:poison}));servicePoisonRejected=!reply?.appearance||(reply.appearance.mode==='@MODE@'&&reply.appearance.accent==='#30d158');}
 await sleep(200);
 const afterPoison=servicePoisonRejected&&(surface==='browser'?lord.nativeViews.text(handle).includes('keep.txt'):current());
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 settingsBridge.perform=perform;render(h(View,{style:{width:1,height:1}}));
 console.log(kind+'-observation '+JSON.stringify({capability,mounted,noThemeFacts,noThemeRequests,withoutFacts,staleMirror,nativePalette,afterPoison,orphaned,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error(kind+' driver: '+(error.stack||error))),20);
"#;
