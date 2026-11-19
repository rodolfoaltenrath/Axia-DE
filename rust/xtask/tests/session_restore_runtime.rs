//! Restore lifecycle and native prompt through the actual compositor and SDK adapter.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn native_session_restore_survives_remount_and_preserves_clean_exit_and_claims() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_timeout(
                repo,
                "session_restore",
                DRIVER,
                mode,
                width,
                "session-restore.png",
                35,
            );
            for flag in [
                "bound",
                "initialMarker",
                "prompt",
                "keyboardGate",
                "remounted",
                "acceptedOrDeclined",
                "claims",
                "nativeAutosave",
                "cleanExit",
                "resumed",
                "off",
                "noWindowTransport",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
            eprintln!("session restore {mode}/{width} verified");
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View,shellHasKeyboard} from '@lordos/runtime';
import {configureWindows,wm,placeLinuxWindow} from '@ROOT@/sdk/windows.js';
import {RestorePrompt,startSessionRestore,setRestoreMode,flushSession,resumeSession,restoredIds} from '@ROOT@/sdk/sessionrestore.jsx';
import {restoreBridge} from '@ROOT@/sdk/nativerestore.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
import {TextInput} from '@ROOT@/sdk/components.jsx';
import {textFieldActive} from '@ROOT@/sdk/textfocus.js';
locale.set({...locale.get(),lang:'en'});enableShellLayers();
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,attempts=150){for(let i=0;i<attempts&&!fn();i++)await sleep(20);return!!fn();}
const Icon=()=>null;
configureWindows({apps:[{id:'files',name:'Files',Component:()=>null,Icon},{id:'notes',name:'Notes',Component:()=>null,Icon},{id:'pkg',name:'Package',package:true,Component:()=>null,Icon}]});
const frame={x:40,y:60,w:300,h:220};
settings.set({sessionRestore:'ask',sessionBrowsers:false,sessionSaved:{clean:false,active:1,windows:[
 {id:'app:files',f:frame},{id:'app:notes',f:frame,desk:1,args:{slug:'saved',nonce:123,command:'bad'}},
 {id:'lpk:pkg',f:frame,desk:1,min:1},{id:'desktop:../../bin/sh',f:frame}]}});
wm.open('files',{instance:'existing',frame});
const source=lord.windowList,mount=lord.nativeViews.mount,send=lord.nativeViews.facts,request=lord.nativeApps.request;
let handle=0,mounts=0,bound=true,noWindowTransport=true,remount,opens=[],launches=[],claimOK=false;
const inspect=text=>{const f=JSON.parse(text);noWindowTransport&&=!('windows'in f)&&!('windows'in(f.args||{}));};
lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:restore'){inspect(text);bound&&=sources[0]===source;handle=mount(node,id,text,...sources);mounts++;return handle;}return mount(node,id,text,...sources);};
lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return send(id,text);};
lord.nativeApps.request=text=>{const f=JSON.parse(text);if(f.cmd==='sessionRestore')noWindowTransport&&=!('windows'in f)&&!('active'in f);return request.call(lord.nativeApps,text);};
const direct=(op,...args)=>JSON.parse(source.call(op,JSON.stringify([{},...args])));
const state=()=>direct('resync').state;
const perform=restoreBridge.perform;
restoreBridge.perform=effect=>{
 if(effect.op==='open')opens.push(effect.value);
 if(effect.op==='launch'){
   launches.push(effect.value);const claim=placeLinuxWindow(effect.value);
   if(claim){const id=direct('open',{app:'linux',instance:'55',args:{pkg:true,appId:'pkg'},frame:claim.frame,desktop:claim.desktop}).result;claim.then(id);claimOK=!!claim.frame&&claim.desktop===state().active;}
   return;
 }
 perform(effect);
};
function App(){const[rev,setRev]=useState(0);remount=()=>setRev(n=>n+1);return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',accent:'#3489bd',effects:{}})},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#202126':'#e9ebef'}},h(TextInput,{key:'field-'+rev,autoFocus:true,value:'',onChange:()=>{}}),h(RestorePrompt,{key:rev})));}
render(h(App));startSessionRestore();
const text=()=>lord.nativeViews.text(handle)||'';
const key=name=>{__lordKey(0,1,name,'',name,0);__lordKey(0,0,name,'',name,0);__lordNativeView();};
async function run(){
 await until(()=>handle>0);const marker=settings.get().sessionSaved;
 const initialMarker=marker?.clean===false&&marker.windows.length===3&&!JSON.stringify(marker).includes('command');
 const prompt=await until(()=>text().includes('Reopen your windows?'));
 const keyboardGate=await until(()=>shellHasKeyboard()&&!textFieldActive());
 const old=mounts;remount();await until(()=>mounts>old&&text().includes('Reopen your windows?'));
 const remounted=await until(()=>text().includes('Reopen your windows?')&&!textFieldActive()&&shellHasKeyboard())&&settings.get().sessionSaved.clean===false;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 let acceptedOrDeclined=false,claims=false,nativeAutosave=false;
 if('@MODE@'==='light'){
   key('Enter');await until(()=>opens.length===1);
   const pendingLogin=restoredIds.has('lpk:pkg');await until(()=>claimOK);
   const model=state(),note=model.windows.find(w=>w.w?.app==='notes'),pkg=model.windows.find(w=>w.w?.args?.pkg);
   acceptedOrDeclined=opens.length===1&&opens[0].app==='notes'&&opens[0].new===false&&pendingLogin&&await until(()=>!shellHasKeyboard());
   claims=launches.length===1&&claimOK&&pkg?.w.minimized===true&&note?.w.desktop===model.active;
   direct('setFrame',note.id,{x:90,y:80,w:320,h:240});
   nativeAutosave=await until(()=>settings.get().sessionSaved?.windows.some(e=>e.id==='app:notes'&&e.f.x===90),850);
 }else{
   key('Escape');acceptedOrDeclined=await until(()=>!shellHasKeyboard()&&settings.get().sessionSaved?.windows.length===1);
   claims=launches.length===0&&opens.length===0;
   direct('setFrame','files#existing',{x:90,y:80,w:320,h:240});
   nativeAutosave=await until(()=>settings.get().sessionSaved?.windows[0]?.f.x===90);
 }
 flushSession();const frozen=JSON.stringify(settings.get().sessionSaved),cleanExit=settings.get().sessionSaved?.clean===true;
 direct('setFrame','files#existing',{x:199,y:80,w:320,h:240});await sleep(100);
 const held=JSON.stringify(settings.get().sessionSaved)===frozen;resumeSession();
 const resumed=held&&settings.get().sessionSaved?.clean===false;
 setRestoreMode('off');await sleep(100);const off=settings.get().sessionSaved===null&&!shellHasKeyboard();
 console.log('session_restore-observation '+JSON.stringify({bound,initialMarker,prompt,keyboardGate,remounted,acceptedOrDeclined,claims,nativeAutosave,cleanExit,resumed,off,noWindowTransport,saved:!!saved?.ok,mounts,opens:opens.length,launches:launches.length}));
}
setTimeout(()=>run().catch(error=>console.error('session_restore driver: '+(error.stack||error))),50);
"#;
