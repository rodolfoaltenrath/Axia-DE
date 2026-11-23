//! Physical app launch shortcuts use native preferences before the JS dispatcher.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn physical_launch_shortcuts_ignore_the_stale_js_profile() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo,"shell_launch",DRIVER,mode,width,"shell-launch.png");
            for flag in ["staleProfile","files","settings","monitor","noRepeat",
                "nativeOwnership","macosPasses","saved"] {
                assert_eq!(report[flag],true,"{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {useShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {media} from '@ROOT@/sdk/media.js';
settings.set({shortcutProfile:'macos'});
configureWindows({apps:['files','settings','monitor'].map(id=>({id,name:id,Component:()=>null,Icon:()=>null}))});
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
let probing=false,superTaps=0;const jsKeys=[],requests=[],opened=[];
const dispatch=globalThis.__lordKey;
globalThis.__lordKey=(...args)=>{if(probing)jsKeys.push([args[0],args[1]]);return dispatch(...args);};
const request=lord.nativeApps.request.bind(lord.nativeApps);
lord.nativeApps.request=text=>{if(probing)requests.push(JSON.parse(text));return request(text);};
const open=wm.open;wm.open=(...args)=>{if(probing)opened.push(args[0]);return open(...args);};
const input=(code,value)=>{if(!lord.nativeViews.inputKey(code,value))throw Error('physical input rejected');};
async function chord(mods,code){for(const mod of mods)input(mod,1);input(code,1);input(code,2);input(code,0);for(const mod of [...mods].reverse())input(mod,0);await sleep(60);}
function Harness(){useShortcuts({grabs:'flow',onSuperTap:()=>superTaps++});return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@'})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}}));}
render(h(Harness));
async function run(){
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 lord.settings.state({cmd:'patch',patch:{shortcutProfile:'linux'}});
 const staleProfile=settings.get().shortcutProfile==='macos';probing=true;
 await chord([125],18);const files=await until(()=>!!wm.get('files'));
 await chord([125],23);const preferences=await until(()=>!!wm.get('settings'));
 await chord([29,42],1);const monitor=await until(()=>!!wm.get('monitor'));
 const noRepeat=JSON.stringify(opened)===JSON.stringify(['files','settings','monitor']);
 // Opening a window refreshes the compatibility keyboard capture. It does
 // not send the physical key or ask JS to select the launch command.
 const nativeOwnership=superTaps===0&&jsKeys.every(([code,value])=>[125,29,42].includes(code)&&value===1)&&requests.every(input=>input.cmd==='shellKeyboard');
 lord.settings.state({cmd:'patch',patch:{shortcutProfile:'macos'}});
 const count=opened.length;await chord([125],18);await chord([125],23);await chord([29,42],1);
 const macosPasses=opened.length===count&&jsKeys.some(([code,value])=>code===18&&value===1);
 probing=false;settingsBridge.perform=perform;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('shell_launch-observation '+JSON.stringify({staleProfile,files,settings:preferences,monitor,noRepeat,nativeOwnership,macosPasses,saved:!!saved?.ok,jsKeys,requests,opened}));
}
setTimeout(()=>run().catch(error=>console.error('shell_launch driver: '+(error.stack||error))),200);
"#;
