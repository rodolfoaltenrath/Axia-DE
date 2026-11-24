//! Native selector input through the real shortcut dispatcher and retained thumbnail.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn printscreen_selects_area_window_monitor_or_desktop_and_keeps_the_thumbnail() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 1000] {
                let kind = format!("capture_selector_{appearance}");
                let driver = DRIVER
                    .replace("@APPEARANCE@", appearance)
                    .replace("@PROFILE@", if width == 760 { "linux" } else { "macos" });
                let result = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "selector.png",
                    30,
                    |path| {
                        let png = image::open(path).unwrap();
                        assert_eq!((png.width(), png.height()), (width, 700));
                        if let Some(out) =
                            std::env::var_os(format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase()))
                        {
                            let out = std::path::PathBuf::from(out).join(format!("{mode}-{width}"));
                            std::fs::create_dir_all(&out).unwrap();
                            for name in ["thumbnail", "window", "monitor", "desktop"] {
                                let source = format!("{}.{name}.png", path.display());
                                if std::path::Path::new(&source).exists() {
                                    std::fs::copy(source, out.join(format!("{name}.png"))).unwrap();
                                }
                            }
                        }
                    },
                );
                for flag in [
                    "printscreen",
                    "nativeAppearance",
                    "inputFocus",
                    "inputRestored",
                    "noEarly",
                    "area",
                    "thumbnail",
                    "edit",
                    "window",
                    "fullscreen",
                    "pip",
                    "monitor",
                    "desktop",
                    "cancel",
                    "replacement",
                    "layoutChange",
                    "singleClose",
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
import {render,View,Text,screen,displayFor} from '@lordos/runtime';
import Screenshots,{Icon} from '@ROOT@/runtime/native_capture.js';
import {configureWindows,wm,useWindows,windows} from '@ROOT@/sdk/windows.js';
import {AppHost,TextInput,hasTextFocus} from '@ROOT@/sdk/components.jsx';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {perms} from '@ROOT@/sdk/privacy.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='capture_selector_@APPEARANCE@',sleep=ms=>new Promise(r=>setTimeout(r,ms));
let stage='bootstrap',handle=0,nativeAppearance=true;
const crops=[],closed=[],savedPaths=[],keyCalls=[],viewFacts=[],focusCalls=[];
const focus=lord.nativeViews.focus;lord.nativeViews.focus=(...args)=>{focusCalls.push(args);return focus(...args);};
const nativeKey=lord.nativeViews.key;lord.nativeViews.key=(...args)=>{const result=nativeKey(...args);keyCalls.push({args,result,focused:wm.focused(),keyboard:lord.nativeApps.request(JSON.stringify({cmd:'shellKeyboard'}))});return result;};
const rawCrop=media.crop,rawClose=media.close,rawSave=media.saveImage;
media.crop=(...args)=>{crops.push(args);return rawCrop(...args);};media.close=id=>{closed.push(id);return rawClose(id);};
media.saveImage=async(id,path)=>{const result=await rawSave(id,path);savedPaths.push(result);return result;};
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw),result=mount(node,id,raw,...rest);if(id==='screenshots'&&!facts.icon){handle=result;viewFacts.push(facts);nativeAppearance&&=!('theme' in(facts.context||{}));}return result;};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',shortcutProfile:'@PROFILE@',effects:false,screenshots:{timer:0,clipboard:false}});
screen.displays=[{id:0,name:'Main',x:0,y:0,w:@WIDTH@/2,h:700,scale:150,main:true},{id:1,name:'Second',x:@WIDTH@/2,y:100,w:@WIDTH@/2,h:500,scale:200,main:false}];
perms.set('screenshots','screen','allow');resetShortcuts();enableShellLayers();
const Source=()=>h(View,{style:{flex:1,backgroundColor:'#227755',padding:20}},h(Text,{style:{color:'#fff',fontSize:20}},'Capture source window'),h(TextInput,{autoFocus:true,value:'Source text',onChange:()=>{}}));
configureWindows({apps:[{id:'screenshots',name:'Screenshots',Component:Screenshots,Icon},{id:'notes',name:'Source',Component:Source,Icon:()=>null}]});
const source=wm.open('notes',{instance:'source',frame:{x:80,y:90,w:300,h:240}});
function Harness(){const state=useWindows();useShortcuts({grabs:'flow',onSuperTap:()=>{}});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},state.windows.map(w=>{
 const rect=w.id.includes('#overlay')?{x:0,y:0,w:@WIDTH@,h:700}:w.pip||(w.fullscreen?displayFor(w):wm.frame(w));
 return h(View,{key:w.id,style:{position:'absolute',left:rect.x,top:rect.y,width:rect.w,height:rect.h}},h(AppHost,{id:w.id,focused:state.focused===w.id}));
})));}render(h(Harness));
const overlay=()=>windows.get().find(w=>w.id==='screenshots#overlay'),thumb=()=>windows.get().find(w=>w.id==='screenshots#thumb');
async function until(fn,label){stage=label;for(let n=0;n<160&&!fn();n++)await sleep(20);if(!fn())throw Error('timeout '+label+'; '+(handle?lord.nativeViews.text(handle):''));return true;}
function pointer(type,x,y){const target=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,target,x,y,0);__lordNativeView();}
function key(name,char='',flags=0){const code=name==='PrintScreen'?99:0;__lordKey(code,1,name,char,name,flags);__lordKey(code,0,name,'',name,flags);__lordNativeView();}
async function click(label){const node=lord.nativeViews.findText(handle,label);if(!node)throw Error('missing control '+label);const[x,y,w,h]=lord.getLayout(node),cx=x+w/2,cy=y+h/2;if(!screen.displays.some(d=>cx>=d.x&&cy>=d.y&&cx<d.x+d.w&&cy<d.y+d.h))throw Error('control outside displays: '+label);pointer('down',cx,cy);pointer('up',cx,cy);await sleep(50);}
async function capture(suffix=''){await sleep(100);const image=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700});const result=await rawSave(image,@IMAGE@+suffix);media.close(image);return!!result?.ok;}
async function open(){key('PrintScreen');await until(()=>overlay()&&handle&&lord.nativeViews.text(handle).includes('All screens'),'PrintScreen selector');await sleep(100);return overlay().args.handle;}
async function closeScreens(){for(const w of windows.get())if(w.app==='screenshots')wm.close(w.id,true);await sleep(100);}
async function run(){
 await sleep(160);const hadInput=hasTextFocus(),before=crops.length,base=await open(),closeIndex=closed.length;const printscreen=!!overlay(),inputFocus=hadInput&&!hasTextFocus();
 pointer('down',400,300);pointer('move',100,120);await sleep(60);pointer('up',100,120);await sleep(80);
 const noEarly=crops.length===before,saved=await capture();key('Enter');key('Enter');
 await until(()=>thumb()?.args.path&&savedPaths.some(r=>r.ok),'saved thumbnail');
 const area=crops.length===before+1&&crops[before].slice(1).join(',')==='100,120,300,180';
 const thumbnail=thumb().pip.x===@WIDTH@/2-260&&thumb().pip.y===460&&thumb().args.handle>0&&!!lord.fs.stat(thumb().args.path);
 const singleClose=closed.slice(closeIndex).filter(id=>id===base).length===1;await capture('.thumbnail.png');
 const thumbFrame=thumb().pip;pointer('down',thumbFrame.x+thumbFrame.w/2,thumbFrame.y+thumbFrame.h/2);pointer('up',thumbFrame.x+thumbFrame.w/2,thumbFrame.y+thumbFrame.h/2);const edit=await until(()=>windows.get().some(w=>w.id.startsWith('screenshots#edit')&&w.args.handle),'thumbnail click opens editor');await closeScreens();
 let n=crops.length;await open();await click('Window');pointer('move',180,180);pointer('down',180,180);pointer('up',180,180);await capture('.window.png');key('Enter');await until(()=>crops.length===n+1&&thumb(),'window crop');
 const f=wm.frame(wm.get(source));const window=crops[n].slice(1).join(',')===[f.x,f.y,f.w,f.h].join(',');await closeScreens();
 wm.setFullscreen(source,true);await sleep(80);n=crops.length;await open();await click('Window');pointer('move',180,180);pointer('down',180,180);pointer('up',180,180);key('Enter');await until(()=>crops.length===n+1&&thumb(),'fullscreen window crop');
 const fullscreen=crops[n].slice(1).join(',')===[0,0,@WIDTH@/2,700].join(',');await closeScreens();wm.setFullscreen(source,false);
 wm.setPip(source,{x:@WIDTH@-220,y:150,w:200,h:160});await sleep(80);n=crops.length;await open();await click('Window');pointer('move',@WIDTH@-120,200);pointer('down',@WIDTH@-120,200);pointer('up',@WIDTH@-120,200);key('Enter');await until(()=>crops.length===n+1&&thumb(),'picture-in-picture crop');
 const pip=crops[n].slice(1).join(',')===[@WIDTH@-220,150,200,160].join(',');await closeScreens();wm.setPip(source,null);
 n=crops.length;await open();await click('Monitor');pointer('move',@WIDTH@-100,300);pointer('down',@WIDTH@-100,300);pointer('up',@WIDTH@-100,300);await capture('.monitor.png');await click('Capture');await until(()=>crops.length===n+1&&thumb(),'monitor crop');
 const monitor=crops[n].slice(1).join(',')===[@WIDTH@/2,100,@WIDTH@/2,500].join(',');await closeScreens();
 n=crops.length;await open();await click('All screens');await capture('.desktop.png');key('Enter');await until(()=>crops.length===n+1&&thumb(),'desktop crop');const desktop=crops[n].slice(1).join(',')==='0,0,@WIDTH@,700';await closeScreens();
 pointer('down',150,155);pointer('up',150,155);await until(()=>hasTextFocus(),'source input refocused');
 n=crops.length;await open();key('Escape');await until(()=>!overlay(),'cancel');await until(()=>hasTextFocus(),'source input restored');const inputRestored=hasTextFocus(),cancel=crops.length===n&&!thumb();
 const original=await open(),releaseIndex=closed.length,newImage=lord.media.screenshot('screenshots',{x:0,y:0,w:@WIDTH@,h:700}),old=overlay();
 wm.setArgs(old.id,{...old.args,handle:newImage,mode:'desktop'});await until(()=>closed.slice(releaseIndex).includes(original)&&lord.nativeViews.text(handle).includes('Capture all screens'),'replaced snapshot');
 n=crops.length;key('Enter');await until(()=>crops.length===n+1&&thumb(),'replacement capture');
 const replacement=crops[n][0]===newImage&&closed.slice(releaseIndex).filter(id=>id===original).length===1&&closed.slice(releaseIndex).filter(id=>id===newImage).length===1;await closeScreens();
 n=crops.length;await open();__lordScreen();await until(()=>!overlay(),'display layout changed');const layoutChange=crops.length===n&&!thumb();
 console.log(kind+'-observation '+JSON.stringify({printscreen,nativeAppearance,inputFocus,inputRestored,noEarly,area,thumbnail,edit,window,fullscreen,pip,monitor,desktop,cancel,replacement,layoutChange,singleClose,saved}));
}
setTimeout(()=>run().catch(async error=>{await capture();console.log(kind+'-observation '+JSON.stringify({error:String(error),stage,crops,closed,savedPaths,keyCalls,viewFacts,focusCalls,windows:windows.get()}));}),100);
"#;
