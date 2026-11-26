//! The compiled Text Editor in the real compositor: open a file, type through the keyboard path,
//! save, select a word with the pointer, search from the find field, jump to a line and answer the
//! close guard. The captures prove the highlighted text was painted in both modes.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn editor_edits_saves_selects_searches_and_guards_closing() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_capture_timeout(repo, "editor", DRIVER, mode, width, "editor.png", 40, |path| {
                let capture = image::open(path).unwrap().into_rgb8();
                assert_eq!((capture.width(), capture.height()), (width, 700));
                let near = |want: [u8; 3]| capture.pixels().filter(|p| p.0.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 12)).count();
                let (background, keyword) =
                    if mode == "light" { ([255, 255, 255], [155, 35, 147]) } else { ([30, 30, 35], [252, 95, 163]) };
                assert!(near(background) > 40_000, "{mode}/{width}: the text area was not painted");
                assert!(near(keyword) > 20, "{mode}/{width}: no highlighted keyword was painted");
                if let Some(output) = std::env::var_os("AXIA_EDITOR_OUTPUT") {
                    let out = std::path::PathBuf::from(output).join(format!("{mode}-{width}"));
                    std::fs::create_dir_all(&out).unwrap();
                    std::fs::copy(path, out.join("editor.png")).unwrap();
                }
            });
            for flag in ["compiled", "opened", "tree", "typed", "saved", "pointer", "found", "jumped", "guarded", "closed"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/runtime/nativeapps.js';
import {configureWindows,wm,useWindows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='editor',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let handle=0,window,stage='bootstrap';
const report={};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',effects:false});
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const result=mount(node,id,raw,...rest);if(id==='editor'&&!JSON.parse(raw).icon)handle=result;return result;};
const home=lord.fs.home();try{lord.fs.mkdir(home+'/Code');}catch(_){}
const path=home+'/Code/sample.rs';
lord.fs.write(path,'// Axia editor probe\nfn main() {\n    let total = 40 + 2;\n}\n');
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='editor');
configureWindows({apps:[nativeApp(manifest)],workArea:{x:0,y:0,w:@WIDTH@,h:700}});window=wm.open('editor',{args:{path}});
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},wm.get(window)?h(AppHost,{key:window,id:window,focused:true}):null));}
render(h(Harness));
const text=()=>lord.nativeViews.text(handle)||'';
async function until(fn,label,steps=250){stage=label;for(let i=0;i<steps&&!fn();i++)await sleep(20);if(!fn())throw Error('timeout: '+label+'; '+text());return true;}
const key=(name,char='',mods=0)=>{__lordKey(0,1,name,char,char||name,mods);__lordKey(0,0,name,'',char||name,mods);__lordNativeView();};
const type=word=>{for(const ch of word)key(ch,ch);};
const read=()=>lord.fs.read(path);
function click(label){const node=lord.nativeViews.findText(handle,label);if(!node)throw Error('no text node: '+label);const [x,y,w,h_]=lord.getLayout(node),cx=x+w/2,cy=y+h_/2;
 for(const phase of ['down','up']){const target=lord.nativeViews.pointer(phase,node,cx,cy,0);__lordEvent(phase,target,cx,cy,0);}__lordNativeView();}
async function capture(){await sleep(200);const image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@);media.close(image);return!!result?.ok;}
async function run(){
 report.compiled=manifest?.nativeView==='rust'&&manifest?.process!=='isolated';
 report.opened=await until(()=>text().includes('Line 1, column 1')&&text().includes('Rust'),'file opened');
 report.tree=await until(()=>text().includes('Code')&&text().includes('sample.rs'),'project panel');
 key('End','',2);type('// done');
 report.typed=await until(()=>text().includes('Line 5, column 8'),'typing');
 key('s','s',2);report.saved=await until(()=>read().endsWith('}\n// done'),'saved');
 await sleep(300);
 click('main');click('main');
 report.pointer=await until(()=>text().includes('4 characters selected')&&text().includes('Line 2'),'double click selects the word');
 key('f','f',2);await until(()=>text().includes('1 of 1'),'find opens with the selection');
 for(let i=0;i<4;i++)key('Backspace');type('total');
 report.found=await until(()=>/1 of 1/.test(text()),'search from the field');
 key('Enter');await sleep(100);key('Escape');
 key('l','l',2);await sleep(150);type('3');key('Enter');
 report.jumped=await until(()=>text().includes('Line 3, column 1'),'go to line');
 const painted=await capture();
 type('x');await sleep(500);
 wm.close(window);__lordNativeView();
 report.guarded=await until(()=>text().includes('Save changes to')&&!!wm.get(window),'close guard');
 lord.nativeViews.action(handle,{action:['discard']});__lordNativeView();
 report.closed=await until(()=>!wm.get(window),'closed after discarding');
 console.log(kind+'-observation '+JSON.stringify({...report,painted}));
}
setTimeout(()=>run().catch(async error=>{await capture();console.log(kind+'-observation '+JSON.stringify({...report,error:String(error),stage,text:text()}));}),200);
"#;
