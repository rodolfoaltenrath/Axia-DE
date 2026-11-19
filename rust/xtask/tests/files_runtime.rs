//! Native Files through the production mount, services and real compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media/axia-doc and npm ci; required by GPU=1 desktop CI"]
fn files_browser_is_native_with_old_preferences_and_uses_real_private_files() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        let kind = format!("files_{appearance}");
        let driver = DRIVER
            .replace("@APPEARANCE@", appearance)
            .replace("@KIND@", &kind);
        for mode in ["light", "dark"] {
            for width in [640u32, 1000] {
                let report = ui::run_ui_with_capture(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "files.png",
                    |path| {
                        let pixels = image::open(path).unwrap().into_rgb8();
                        let ink = pixels
                            .enumerate_pixels()
                            .filter(|(x, y, p)| {
                                *x > width / 3
                                    && (210..480).contains(y)
                                    && p[0] > 180
                                    && p[1] < 80
                                    && p[2] < 80
                            })
                            .count();
                        assert!(ink > 1000, "PDF marker was not painted: {appearance}/{mode}/{width}, {ink} red pixels");
                        for (channel, label) in [(1, "bottom"), (2, "top")] {
                            let edge = pixels
                                .enumerate_pixels()
                                .filter(|(x, y, p)| {
                                    *x > width / 3
                                        && (195..500).contains(y)
                                        && p[channel] > 180
                                        && p[0] < 80
                                        && p[3 - channel] < 100
                                })
                                .count();
                            assert!(edge > 100, "PDF {label} was cropped: {appearance}/{mode}/{width}, {edge} pixels");
                        }
                    },
                );
                for flag in [
                    "nativeBrowser",
                    "noReactBrowser",
                    "iconNative",
                    "listed",
                    "navigated",
                    "back",
                    "filtered",
                    "menuOpened",
                    "menuEscaped",
                    "created",
                    "renamed",
                    "bytesKept",
                    "listView",
                    "pdfThumbnail",
                    "pdfPreview",
                    "previewClosed",
                    "zoomCached",
                    "preferencesMigrated",
                    "appearanceCached",
                    "appearanceBounded",
                    "pickerOpen",
                    "pickerSave",
                    "pickerFolder",
                    "hotplug",
                    "mounted",
                    "ejected",
                    "menusPublished",
                    "menusStable",
                    "closed",
                    "saved",
                ] {
                    assert_eq!(
                        report[flag], true,
                        "{appearance}/{mode}/{width}/{flag}: {report}"
                    );
                }
                assert_eq!(
                    report["volumeCalls"],
                    serde_json::json!(["mount", "release"])
                );
                let rect = report["fileRect"].as_array().unwrap();
                assert!(
                    rect[2].as_f64().unwrap() > 0. && rect[3].as_f64().unwrap() > 0.,
                    "{report}"
                );
                assert!(
                    rect[0].as_f64().unwrap() >= 0.
                        && rect[0].as_f64().unwrap() + rect[2].as_f64().unwrap() <= width as f64,
                    "{report}"
                );
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/sdk/nativeapps.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {AppWindowContext} from '@ROOT@/sdk/components.jsx';
import {settings,migrateSettings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider,appAppearance} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {FilePicker} from '@ROOT@/sdk/filepicker.jsx';
import {t} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
const home=lord.fs.home(),base=home+'/files-integration',devicePath=home+'/files-device';
let handle=0,iconHandle=0,show,facts=[],legacyCreates=0,menuUpdates=0,setPicker,pickerId=0,pickerAction=null;const results=[],volumeCalls=[],choices=[];
const mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount,result=lord.nativeViews.result,request=lord.files.request,setMenus=wm.setMenus;
lord.nativeViews.mount=(node,app,text,...source)=>{const id=mount(node,app,text,...source);if(app==='files'){if(JSON.parse(text).icon)iconHandle=id;else handle=id;}return id;};
lord.nativeViews.unmount=id=>{if(id===handle)handle=0;if(id===iconHandle)iconHandle=0;return unmount(id);};
lord.nativeViews.result=(id,value)=>{results.push(value);return result(id,value);};
// Only disk facts and volume replies are simulated. No real host disk is changed.
lord.files.request=raw=>{let input=JSON.parse(raw);if(input.op==='create'&&input.mode==='browser')legacyCreates++;
 if(input.op==='volumeStart'){volumeCalls.push(input.action);facts=input.action==='mount'?disk(true):[];return{job:input.action==='mount'?1:2};}
 if(input.op==='volumePoll')return{done:true,result:input.job===1?devicePath:null};
 if(input.op==='poll'&&input.id===pickerId&&pickerAction){input={...input,op:'event',action:pickerAction};pickerAction=null;}
 const reply=request(JSON.stringify(input));if(input.op==='create'&&input.mode!=='browser')pickerId=reply.id;if(input.op==='close'&&input.id===pickerId)pickerId=0;return reply;};
lord.disks.list=()=>facts;
wm.setMenus=(...args)=>{menuUpdates++;return setMenus(...args);};
const disk=mounted=>[{name:'sdb',removable:true,transport:'usb',system:false,partitions:[{name:'sdb1',fstype:'exfat',label:'USB integration',mountpoint:mounted?devicePath:''}]}];
const app=nativeApp(lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='files'));
configureWindows({apps:[app],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',filesNativeView:false,reduceMotion:true});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
const winId=wm.open('files',{args:{path:base}});
const find=text=>handle?lord.nativeViews.findText(handle,text):0;
const visible=text=>{const node=find(text);return!!node&&lord.getLayout(node)[2]>0;};
const text=()=>handle?lord.nativeViews.text(handle)||'':'';
const act=(action,extra={})=>{lord.nativeViews.action(handle,{action,extra});globalThis.__lordNativeView();};
const key=(name,char='',mods=0)=>{__lordKey(0,1,name,char,name,mods);__lordKey(0,0,name,'',name,mods);globalThis.__lordNativeView();};
const click=(label,button=0)=>{const node=find(label),[x,y,w,h]=lord.getLayout(node);for(const type of ['down','up']){const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x+w/2,y+h/2),x+w/2,y+h/2,button);__lordEvent(type,target,x+w/2,y+h/2,button);globalThis.__lordNativeView();}};
function App(){const[shown,setShown]=useState(false),[picker,updatePicker]=useState(null);show=setShown;setPicker=updatePicker;return h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 shown?h(AppWindowContext.Provider,{value:{id:winId,args:{path:base},focused:true}},
 h(View,{style:{position:'absolute',left:8,top:8,width:@WIDTH@-16,height:684}},h(app.Component))):null,
 shown?h(View,{style:{position:'absolute',left:0,top:0,width:1,height:1,pointerEvents:'none'}},h(app.Icon,{size:1})):null,
 picker?h(FilePicker,{...picker,onPick:path=>{choices.push(path);updatePicker(null);},onClose:()=>updatePicker(null)}):null));}
enableShellLayers();render(h(App));
async function run(){
 const old={schema:1,externalWindowStyle:true,filesAppearance:'axia',mode:'light',files:{zoom:1.5}},migrated=migrateSettings(old);
 const retired=migrateSettings({schema:2,motion:{'*':'earth-spin'},motionUser:[],remoteDesks:[{host:'pc'}],loginWallpaper:'x',wallpaper:'event-horizon'});
 const preferencesMigrated=migrated.schema===3&&migrated.externalWindowStyle===false&&migrated.files===old.files&&migrated.filesAppearance==='axia'
  &&migrateSettings({...migrated,externalWindowStyle:true}).externalWindowStyle===true&&retired.schema===3&&!('motion'in retired)&&!('motionUser'in retired)&&!('remoteDesks'in retired)&&!('loginWallpaper'in retired)&&retired.wallpaper==='event-horizon';
 settings.set({nativeAppearance:'adwaita'});const appearance=lord.appearance.request;let styleCalls=0,appearanceBounded=true;lord.appearance.request=raw=>{styleCalls++;appearanceBounded&&=raw.length<512&&!raw.includes('wallpapers');return appearance(raw);};
 const styleSource={mode:'dark',accent:'#3584e4',effects:true,blur:24,panelBg:'rgba(36,36,42,0.72)',wallpapers:new Array(1000).fill('catalog')};
 const shared=appAppearance(styleSource);let same=true;for(let i=0;i<1000;i++)same&&=appAppearance(styleSource)===shared;
 const equal=appAppearance({...styleSource}),firstCalls=styleCalls,next=appAppearance({...styleSource,mode:'light'});
 const appearanceCached=same&&firstCalls===1&&styleCalls===2&&equal.chrome===shared.chrome&&shared.tokens.nativeStyle==='adwaita'&&shared.tokens.effects===true&&shared.tokens.blur===24&&shared.tokens.panelBg===styleSource.panelBg&&next.tokens.mode==='light';lord.appearance.request=appearance;settings.set({nativeAppearance:'@APPEARANCE@'});
 lord.fs.mkdir('files-integration');lord.fs.mkdir('files-integration/Folder');lord.fs.write('files-integration/Folder/inner.txt','inner bytes');
 lord.fs.write('files-integration/report.txt','preserved report bytes');lord.fs.mkdir('files-device');lord.fs.write('files-device/note.txt','Private simulated USB contents');
 // A known visible mark proves that the document pixels reached the scene.
 // This fixture needs neither fonts nor a second PDF generation service.
 const stream='0.85 0.1 0.1 rg 50 150 300 300 re f\n0.1 0.2 0.85 rg 150 540 100 40 re f\n0.1 0.8 0.2 rg 150 20 100 40 re f\n';
 const objects=['<</Type/Catalog/Pages 2 0 R>>','<</Type/Pages/Kids[3 0 R]/Count 1>>','<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 600]/Contents 4 0 R>>','<</Length '+stream.length+'>>\nstream\n'+stream+'endstream'];
 let pdf='%PDF-1.4\n';const offsets=[0];for(let i=0;i<objects.length;i++){offsets.push(pdf.length);pdf+=(i+1)+' 0 obj\n'+objects[i]+'\nendobj\n';}
 const xref=pdf.length;pdf+='xref\n0 5\n0000000000 65535 f \n'+offsets.slice(1).map(offset=>String(offset).padStart(10,'0')+' 00000 n \n').join('')+'trailer\n<</Size 5/Root 1 0 R>>\nstartxref\n'+xref+'\n%%EOF\n';lord.fs.write('files-integration/preview.pdf',pdf);
 show(true);await until(()=>visible('report.txt'));await sleep(100);
 const nativeBrowser=!!handle,noReactBrowser=legacyCreates===0,iconNative=!!iconHandle,listed=visible('report.txt')&&visible('Folder');
 const fileRect=lord.getLayout(find('report.txt'));
 click('Folder');await sleep(40);click('Folder');const navigated=await until(()=>visible('inner.txt')&&!find('report.txt'));
 act(['back']);const back=await until(()=>visible('report.txt'));
 act(['query'],{value:'report'});await until(()=>visible('report.txt')&&!find('preview.pdf'));const filtered=visible('report.txt')&&!find('preview.pdf');act(['query'],{value:''});await until(()=>visible('preview.pdf'));
 // Menus live in the popover layer, outside the mount's gathered scene text.
 // Caption lookup follows logical ownership and includes those portal nodes.
 await sleep(100);click('report.txt',2);const menuOpened=await until(()=>visible(t('files.menu.rename')));key('Escape');await sleep(100);const menuEscaped=!!handle&&!find(t('files.menu.rename'));
 act(['newFolder']);act(['dialogText'],{value:'Created folder'});act(['submit']);const created=await until(()=>visible('Created folder'));
 click('report.txt');await sleep(100);act(['rename']);act(['dialogText'],{value:'renamed.txt'});act(['submit']);const renamed=await until(()=>visible('renamed.txt')&&!find('report.txt'));
 const bytesKept=lord.fs.read('files-integration/renamed.txt')==='preserved report bytes';
 act(['view','list']);await sleep(130);const listView=visible('renamed.txt')&&lord.getLayout(find('renamed.txt'))[3]<fileRect[3];act(['view','icons']);await until(()=>visible('preview.pdf'));
 const thumb=await until(()=>results.some(value=>value.kind==='pdfThumbnail'&&value.result?.id));const pdfThumbnail=thumb;
 const thumbnails=results.filter(value=>value.kind==='pdfThumbnail').length;act(['zoom',1.5]);await sleep(130);act(['zoom',1]);await sleep(130);const zoomCached=results.filter(value=>value.kind==='pdfThumbnail').length===thumbnails;
 click('preview.pdf');await sleep(100);act(['preview']);const pdfPreview=await until(()=>results.some(value=>value.kind==='documentPreview'&&value.result?.id));await sleep(100);
 const image=lord.media.screenshot('screenshots');const saved=await media.saveImage(image,@IMAGE@);media.close(image);
 act(['cancelDialog']);await sleep(100);const previewClosed=visible('preview.pdf')&&!find('Cancel');
 const menuCount=menuUpdates;act(['scroll',20]);await sleep(120);act(['scroll',0]);await sleep(120);const menusPublished=menuCount>0,menusStable=menuCount===menuUpdates;
 setPicker({mode:'open',start:base});await until(()=>!!pickerId);await sleep(100);pickerAction=['openRaw',base+'/renamed.txt'];const pickerOpen=await until(()=>choices.at(-1)===base+'/renamed.txt');await until(()=>!pickerId);
 setPicker({mode:'save',start:base,name:'saved.txt'});await until(()=>!!pickerId);await sleep(100);pickerAction=['choose'];const pickerSave=await until(()=>choices.at(-1)===base+'/saved.txt');await until(()=>!pickerId);
 setPicker({mode:'folder',start:base});await until(()=>!!pickerId);await sleep(100);pickerAction=['choose'];const pickerFolder=await until(()=>choices.at(-1)===base);await until(()=>!pickerId);
 facts=disk(false);globalThis.__lordDisks('change');const hotplug=await until(()=>text().includes('USB integration'));
 act(['volumeOpen','sdb1']);const mounted=await until(()=>visible('note.txt')&&wm.get(winId)?.title==='files-device');
 act(['volumeRelease','sdb1']);const ejected=await until(()=>!text().includes('USB integration')&&wm.get(winId)?.title!=='files-device');
 show(false);await until(()=>!handle);const closed=!handle&&!iconHandle;
 console.log('@KIND@-observation '+JSON.stringify({nativeBrowser,noReactBrowser,iconNative,listed,fileRect,navigated,back,filtered,menuOpened,menuEscaped,created,renamed,bytesKept,listView,pdfThumbnail,pdfPreview,previewClosed,zoomCached,preferencesMigrated,appearanceCached,appearanceBounded,pickerOpen,pickerSave,pickerFolder,hotplug,mounted,ejected,volumeCalls,menusPublished,menusStable,closed,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('@KIND@ driver: '+(error.stack||error))),200);
"#;
