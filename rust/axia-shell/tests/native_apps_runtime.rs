//! Optional real compositor/QuickJS/React integration check. All assertions and
//! process/filesystem setup are Rust; the embedded module only drives the JS UI
//! and reports observations from its existing host calls.
#![cfg(target_os = "linux")]

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires built axia-server and local esbuild; see docs/RUST.md"]
fn real_compiled_app_widgets_and_effects() {
    let repo = std::env::var_os("AXIA_NATIVE_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        });
    let repo = repo.canonicalize().unwrap();
    let scratch = Scratch(std::env::temp_dir().join(format!(
            "axia-native-apps-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&scratch.0).unwrap();
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
    let home = scratch.0.join("home");
    for path in [
        &home,
        &home.join("runtime"),
        &home.join(".config"),
        &home.join(".config/axia"),
    ] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::chown(name.as_ptr(), 65534, 65534) }, 0);
        }
    }
    let driver = scratch.0.join("probe.jsx");
    fs::write(&driver, DRIVER.replace("@ROOT@", repo.to_str().unwrap())).unwrap();
    let bundle = scratch.0.join("probe.bundle.js");
    let output = Command::new(repo.join("node_modules/.bin/esbuild"))
        .arg(&driver)
        .args([
            "--bundle",
            "--format=iife",
            "--target=es2020",
            "--jsx=automatic",
        ])
        .arg(format!("--outfile={}", bundle.display()))
        .arg(format!(
            "--alias:@lordos/runtime={}/runtime/index.js",
            repo.display()
        ))
        .arg(format!(
            "--alias:@lordos/sdk={}/sdk/index.js",
            repo.display()
        ))
        .args([
            "--define:process.env.NODE_ENV=\"production\"",
            "--define:__LORD_STANDALONE__=true",
        ])
        .env("NODE_PATH", repo.join("node_modules"))
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "esbuild: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut command = Command::new(repo.join("server/build/axia-server"));
    command
        .args(["--headless", "1000x700", "--timeout", "16", "--theme"])
        .arg(&bundle)
        .arg("--fonts")
        .arg(repo.join("assets/fonts"))
        .current_dir(repo.join("server"))
        .env("HOME", &home)
        .env("USER", "nobody")
        .env("XDG_RUNTIME_DIR", home.join("runtime"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LORD_GPU", "0")
        .env("AXIA_NET_OFFLINE", "1")
        .env("LANG", "en_US.UTF-8")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env_remove("SUDO_USER")
        .stdout(Stdio::null())
        .stderr(fs::File::create(scratch.0.join("stderr.log")).unwrap());
    if unsafe { libc::geteuid() } == 0 {
        unsafe {
            command.pre_exec(|| {
                if libc::setgroups(0, std::ptr::null()) != 0
                    || libc::setgid(65534) != 0
                    || libc::setuid(65534) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    // The headless session renders on its own; wait for the probe to finish.
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("native app runtime timed out");
    }
    let stderr = fs::read_to_string(scratch.0.join("stderr.log")).unwrap();
    let log = fs::read_to_string(home.join(".cache/axia/session.log")).unwrap_or_default();
    assert!(child.wait().unwrap().success(), "{stderr}\n{log}");
    for error in [
        "ReferenceError:",
        "TypeError:",
        "SyntaxError:",
        "react (error)",
        "native app driver:",
        "evento:",
        "key listener:",
    ] {
        assert!(!log.contains(error), "{log}");
    }
    let observations: Vec<Value> = log
        .lines()
        .filter_map(|line| line.split_once("native-app-observation "))
        .map(|(_, value)| serde_json::from_str(value).unwrap())
        .collect();
    let get = |name: &str| {
        observations
            .iter()
            .find(|v| v["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}: {log}"))
    };
    assert_eq!(get("native-files-media")["result"],json!({"kind":"mediaPreview","generation":7,"result":991}));
    assert_eq!(get("native-files-media")["args"],json!(["files","/preview.mp4",{"audio":1}]));
    assert_eq!(
        get("catalog")["ids"],
        json!([
            "about",
            "appabout",
            "calculator",
            "characters",
            "clock",
            "files",
            "forcequit",
            "help",
            "monitor",
            "store",
            "logs",
            "fonts",
            "scanner",
            "recorder",
            "backup",
            "camera"
        ])
    );
    assert_eq!(get("invalid-id")["rejected"], true);
    assert_eq!(get("app-models")["settings"], "appearance");
    assert_eq!(get("app-models")["schemes"], 16);
    assert_eq!(get("app-models")["profile"], 13);
    assert_eq!(get("app-models")["categories"], 16);
    assert_eq!(get("app-models")["checks"], 6);
    assert_eq!(get("app-models")["shortcut"], "pasteEscaped");
    assert_eq!(get("app-models")["hardware"], 10);
    assert_eq!(get("app-models")["benchmarkCategories"], 6);
    assert_eq!(get("app-models")["stability"], "pass");
    assert!(get("help-service")["count"].as_u64().unwrap() > 20);
    assert_eq!(get("help-service")["title"], "Clock");
    assert_eq!(get("help-service")["portuguese"], "Relógio");
    assert_eq!(get("help-service")["fallback"], "Clock");
    assert_eq!(get("help-service")["score"], 90);
    assert_eq!(get("help-service")["accentScore"], 90);
    assert_eq!(get("help-service")["short"], 0);
    assert_eq!(get("help-service")["raw"], true);
    assert_eq!(get("help-service")["invalid"], true);
    assert_eq!(get("help-service")["link"], true);
    assert_eq!(get("help-service")["external"], false);
    assert_eq!(get("clock-service")["alarms"][0]["label"], "Native clock test");
    assert_eq!(get("clock-service")["timer"]["state"], "paused");
    assert_eq!(get("clock-service")["timer"]["duration"], 30000);
    assert_eq!(get("clock-service")["stopwatch"]["running"], false);
    assert_eq!(get("clock-service")["stopwatch"]["laps"].as_array().unwrap().len(), 1);
    assert_eq!(get("external-activation")["focused"], true);
    assert_eq!(get("external-activation")["minimized"], false);
    assert_eq!(get("empty")["groups"], 0);
    assert_eq!(get("empty")["react"],0);
    assert!(get("empty")["text"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "No app is open"));
    assert_eq!(get("closed")["windows"], 0,"{}",get("closed"));
    assert_eq!(get("closed")["guards"], 0);
    assert_eq!(get("closed")["calls"], json!([]));
    assert_eq!(get("retry")["error"], "app.forceQuitRefused");
    assert_eq!(get("retry")["calls"], json!([["kill", 801], ["kill", 802]]));
    assert_eq!(get("x11")["error"], "app.forceQuitX11");
    assert_eq!(get("x11")["calls"], json!([["close", 803]]));
    assert_eq!(get("selection-removed")["calls"], json!([]));
    assert_eq!(get("remount")["calls"], json!([]));
    let has_text = |name: &str, text: &str| {
        get(name)["text"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == text)
    };
    assert!(has_text("about", "Axia"));
    assert!(has_text("about", "System Report…"));
    assert!(has_text("about", "QuickJS + React 19.3.1"));
    assert_eq!(get("about")["react"], 0, "About content bypasses React");
    assert!(has_text("clock-native-timer", "Start"));
    assert!(has_text("clock-native-timer", "3 min"));
    assert!(has_text("clock-native-running", "Pause"));
    assert!(has_text("clock-native-paused", "Resume"));
    assert!(has_text("clock-native-world", "Local time"));
    assert_eq!(get("clock-native-timer")["react"], 0);
    assert!(has_text("clock-native-alarms", "No alarms yet."));
    assert!(has_text("clock-native-stopwatch", "00:00.00"));
    assert!(has_text("help-native-article", "Open Clock"));
    assert_eq!(get("help-native-article")["react"],0,"{}",get("help-native-article"));
    assert!(has_text("help-native-tour", "Next"));
    assert!(has_text("help-native-tour", "Skip"));
    assert!(has_text("help-native-missing", "That article was not found."));
    assert!(has_text("characters", "Emoji"));
    assert!(has_text("characters", "Symbols"));
    assert!(has_text("characters", "Choose a character"));
    assert_eq!(get("characters")["react"], 0, "Characters content bypasses React");
    assert!(has_text("calculator-precedence", "14"));
    assert_eq!(
        get("calculator-precedence")["react"],
        0,
        "calculator text bypasses React"
    );
    assert!(has_text("calculator-keyboard-error", "Error"));
    assert!(has_text("calculator-keyboard", "3"));
    assert!(has_text("calculator-remount", "0"));
    assert_eq!(get("appabout")["title"], "About Calculator");
    assert_eq!(get("appabout")["react"],0);
    assert_eq!(get("appabout-missing")["react"],0);
    assert!(has_text("appabout", "Version 0.9.0"));
    assert_eq!(get("appabout-missing")["title"], "About missing");
    assert!(!has_text("appabout-missing", "Version 0.9.0"));
    assert!(has_text("monitor-memory", "available for apps"));
    assert_eq!(
        get("monitor-memory")["react"],
        0,
        "no Monitor text goes through React"
    );
    assert_eq!(
        get("monitor-memory")["pixels"],
        0,
        "rasters reach the scene from Rust"
    );
    assert_eq!(get("monitor-memory")["title"], "Monitor");
    assert!(get("monitor-memory")["handle"].as_u64().unwrap_or(0)>0);
    assert!(has_text("monitor-overview", "SUMMARY"));
    assert_eq!(
        get("monitor-unmounted")["texts"],
        0,
        "closing the window unmounts the view"
    );
    for text in [
        "Discover",
        "Create",
        "Updates",
        "Meet Axia Store",
        "Axia Essentials",
        "OPEN",
    ] {
        assert!(has_text("store", text), "{text}");
    }
    assert_eq!(get("store")["title"], "Axia Store");
    assert!(has_text("store-categories", "Utilities"));
    for tab in [
        "overview",
        "cpu",
        "memory",
        "network",
        "disks",
        "processes",
        "cooling",
        "voltage",
    ] {
        assert!(
            get(&format!("monitor-{tab}"))["texts"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    }
    println!("native Rust apps: catalog, Force Quit, Calculator, About, Characters, Clock, Help article/tour/fallback and content bridge, Monitor and Store passed");
}

const DRIVER: &str = r#"
import { createElement as h, useState } from '@ROOT@/node_modules/react/index.js';
import { View, render } from '@ROOT@/runtime/index.js';
import { nativeApp } from '@ROOT@/sdk/nativeapps.js';
import { filesEffect } from '@ROOT@/sdk/nativefiles.js';
import { media } from '@ROOT@/sdk/media.js';
import { help } from '@ROOT@/sdk/help.js';
import { locale } from '@ROOT@/sdk/i18n.js';
import '@ROOT@/sdk/wayland.js';
import { clockAlarms, clockTimer, timerStart, timerPause, timerReset, stopwatch, swStart, swStop, swLap, swReset } from '@ROOT@/sdk/alarms.js';
import { configureWindows, windows, wm, registerBuiltinApp } from '@ROOT@/sdk/windows.js';
import { runningApps, forceQuit } from '@ROOT@/sdk/appcontrol.js';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { AppWindowContext } from '@ROOT@/sdk/components.jsx';
const catalog=lord.nativeApps.request(JSON.stringify({cmd:'catalog'}));
const compiled=catalog.map(nativeApp), force=compiled.find(app=>app.id==='forcequit');
const Icon=({size})=>h(View,{style:{width:size,height:size,backgroundColor:'#3489bd'}});
configureWindows({apps:[...compiled,{id:'demo',name:'Demo',Component:()=>null,Icon},{id:'isolated',name:'Isolated',isolated:true,Component:()=>null,Icon}]});
registerBuiltinApp({id:'linux',name:'Linux',builtin:true,Component:()=>null,Icon});
const nodes=new Map();let root,control,guards=0,calls=[],pixelBytes=0;
const originalSetProp=lord.setProp;
lord.setProp=(id,key,value)=>{if(key==='pixels'&&value?.data)pixelBytes=Math.max(pixelBytes,value.data.length);return originalSetProp(id,key,value);};
for(const method of ['createNode','appendChild','insertBefore','removeChild','destroy','setRoot','setText']) {
  const original=lord[method];lord[method]=(...args)=>{
    const result=original(...args);
    if(method==='createNode')nodes.set(result,{id:result});
    if(method==='setRoot')root=args[0];
    if(method==='appendChild'||method==='insertBefore')nodes.get(args[1]).parent=args[0];
    if(method==='removeChild')nodes.get(args[1]).parent=null;
    if(method==='destroy') {
      // C destroys the whole subtree. Purge its tracked descendants too, before
      // Rust reuses those node IDs for another app's native widgets.
      const gone=new Set([args[0]]);
      for(const id of gone)for(const n of nodes.values())if(n.parent===id)gone.add(n.id);
      for(const id of gone)nodes.delete(id);
    }
    if(method==='setText')nodes.get(args[0]).text=args[1];
    return result;
  };
}
const alive=n=>{for(let i=0;n&&i<100;i++,n=nodes.get(n.parent))if(n.id===root)return true;return false;};
const texts=()=>[...nodes.values()].filter(n=>n.text&&alive(n));
const nativeTexts=()=>{const out=[];for(let h=1;h<256;h++){const t=lord.nativeViews.text(h);if(t)out.push(...t.split('\n'));}return out;};
const click=label=>{
  let target=0;
  for(let h=1;h<256&&!target;h++)target=lord.nativeViews.findText(h,label);
  if(!target){const node=texts().find(n=>n.text===label);target=node?.parent||0;}
  if(!target)throw new Error('native app driver: missing '+label);
  const [x,y,w,height]=lord.getLayout(target);
  for(const type of ['down','up']) {
    const mount=lord.nativeViews.pointer(type,target,x+w/2,y+height/2,0);
    __lordEvent(type,mount,x+w/2,y+height/2,0);
  }
  __lordNativeView();
};
const pause=()=>new Promise(resolve=>setTimeout(resolve,90));
const record=(name,extra={})=>console.log('native-app-observation '+JSON.stringify({name,...extra}));
const tokens={mode:'dark',text:'#fff',textDim:'#aaa',accent:'#3584e4',separator:'#555',control:'#333',controlRadius:6};
function Harness(){const[instance,setInstance]=useState(0),[mode,setMode]=useState('forcequit'),[args,setArgs]=useState({});control={setInstance,setMode,setArgs};const app=compiled.find(app=>app.id===mode);return h(ThemeProvider,{tokens},h(AppWindowContext.Provider,{value:{id:mode,focused:true,args}},h(View,{style:{width:mode==='monitor'?1000:mode==='help'?860:420,height:mode==='monitor'?700:460}},h(app.Component,{key:mode+':'+instance}))));}
render(h(Harness));
async function run(){
  await pause(); record('catalog',{ids:catalog.map(app=>app.id)});
  // The native preview must never allocate React player state/listeners:
  // its Rust owner closes the low-level source without passing through that SDK.
  const oldPlay=lord.media.play,oldTracked=media.play,oldResult=lord.nativeViews.result;
  let previewArgs,previewResult;
  try {
    lord.media.play=(...args)=>{previewArgs=args;return 991;};
    media.play=()=>{throw new Error('native preview entered React player tracking');};
    lord.nativeViews.result=(handle,result)=>{previewResult=result;};
    filesEffect({kind:'mediaPreview',generation:7,path:'/preview.mp4'},{handle:0});
    await Promise.resolve();await Promise.resolve();await Promise.resolve();
    record('native-files-media',{args:previewArgs,result:previewResult});
  } finally {lord.media.play=oldPlay;media.play=oldTracked;lord.nativeViews.result=oldResult;}

  let rejected=false;try{lord.nativeApps.create('forcequit\0other');}catch(error){rejected=true;}record('invalid-id',{rejected});
  const settingsModel=lord.nativeApps.create('settings');settingsModel.request(JSON.stringify({cmd:'select',section:'themes'}));
  const terminalModel=lord.nativeApps.create('terminal'),passwordsModel=lord.nativeApps.create('passwords'),sysinfoModel=lord.nativeApps.create('sysinfo');
  record('app-models',{settings:settingsModel.request(JSON.stringify({cmd:'current'})),
    schemes:terminalModel.request(JSON.stringify({cmd:'catalog'})).presets.length,
    profile:terminalModel.request(JSON.stringify({cmd:'profile',settings:{},id:'missing'})).fontSize,
    categories:passwordsModel.request(JSON.stringify({cmd:'catalog'})).builtin.length,
    checks:sysinfoModel.request(JSON.stringify({cmd:'evaluate',value:{}})).length,
    shortcut:terminalModel.request(JSON.stringify({cmd:'action',event:{code:47,key:'v',meta:true,shift:true}})),
    hardware:sysinfoModel.request(JSON.stringify({cmd:'compatibility',args:[{}]})).length,
    benchmarkCategories:sysinfoModel.request(JSON.stringify({cmd:'perfCatalog'})).categories.length,
    stability:sysinfoModel.request(JSON.stringify({cmd:'stability',args:[{}]})).state});
  record('empty',{groups:runningApps().length,text:nativeTexts(),react:texts().length});
  const english=help.read('clock'),count=help.list().length,score=help.search('clock')[0]?.score;
  const originalLocale=locale.get();
  locale.set({...originalLocale,lang:'pt-BR'});
  const portuguese=help.read('clock')?.title,accentScore=help.search('RELOGIO')[0]?.score;
  locale.set({...originalLocale,lang:'zz'});
  const fallback=help.read('clock')?.title;
  locale.set(originalLocale);
  const link=help.link('app:demo');const linked=windows.get().find(w=>w.app==='demo');if(linked)wm.close(linked.id,true);
  record('help-service',{count,title:english?.title,portuguese,fallback,score,accentScore,short:help.search('c').length,
    raw:lord.help.read('en','clock').startsWith('---\n'),invalid:lord.help.read('en','../clock')===null,
    link,external:help.link('https://example.org')});
  const alarmId=clockAlarms.add({h:23,m:45,label:'Native clock test'});
  timerStart(30);timerPause();swStart();await pause();swLap();swStop();
  record('clock-service',{alarms:clockAlarms.list(),timer:clockTimer.get(),stopwatch:stopwatch.get()});
  clockAlarms.remove(alarmId);timerReset();swReset();
  const browser=wm.open('linux',{instance:804,args:{surface:804}});
  wm.minimize(browser);__lordWayland(804,'activate');
  record('external-activation',{focused:wm.focused()===browser,minimized:wm.get(browser).minimized});wm.close(browser,true);
  const a=wm.open('demo',{instance:'a'}),b=wm.open('demo',{instance:'b'});
  wm.setCloseGuard(a,()=>{guards++;return false;});wm.setCloseGuard(b,()=>{guards++;return false;});
  await pause();click('Demo');await pause();click('Force Quit');await pause();
  record('closed',{windows:windows.get().length,guards,calls});
  lord.wayland.kill=id=>{calls.push(['kill',id]);return id===801?'app.forceQuitRefused':'';};
  lord.wayland.close=id=>{calls.push(['close',id]);};
  wm.open('linux',{instance:801,args:{surface:801,appId:'isolated',pkg:true}});
  wm.open('linux',{instance:802,args:{surface:802,appId:'isolated',pkg:true}});
  calls=[];const retry=forceQuit('isolated');record('retry',{error:retry,calls});
  for (const win of windows.get()) wm.close(win.id, true);
  wm.open('linux',{instance:803,args:{surface:803,appId:'xterm',x11:true}});
  calls=[];const x11=forceQuit(runningApps()[0].key);record('x11',{error:x11,calls});for (const win of windows.get()) wm.close(win.id, true);
  wm.open('demo');await pause();click('Demo');await pause();wm.close('demo',true);await pause();
  calls=[];click('Force Quit');await pause();record('selection-removed',{calls});
  wm.open('demo');await pause();click('Demo');await pause();control.setInstance(1);await pause();
  calls=[];click('Force Quit');await pause();record('remount',{calls});
  control.setMode('calculator');await pause();
  const key=(name,char='')=>{__lordKey(0,1,name,char,name,0);__lordKey(0,0,name,'',name,0);};
  for(const label of ['2','+','3','×','4','Enter']){key(label,label);await pause();}
  record('calculator-precedence',{text:nativeTexts(),react:texts().length});
  key('Escape');key('1','1');key('/','/');key('0','0');key('Enter');await pause();
  record('calculator-keyboard-error',{text:nativeTexts()});
  key('Escape');key('1','1');key(',', ',');key('5','5');key('*','*');key('2','2');key('Enter');await pause();
  record('calculator-keyboard',{text:nativeTexts()});
  control.setInstance(2);await pause();record('calculator-remount',{text:nativeTexts()});
  wm.open('appabout',{args:{key:'calculator'}});control.setArgs({key:'calculator'});control.setMode('appabout');await pause();
  record('appabout',{title:wm.get('appabout').title,text:nativeTexts(),react:texts().length});
  control.setArgs({key:'missing'});await pause();record('appabout-missing',{title:wm.get('appabout').title,text:nativeTexts(),react:texts().length});
  wm.close('appabout',true);
  wm.open('about');control.setArgs({});control.setMode('about');await pause();
  record('about',{text:nativeTexts(),react:texts().length});
  wm.open('characters');control.setArgs({});control.setMode('characters');await pause();
  record('characters',{text:nativeTexts(),react:texts().length});wm.close('characters',true);
  wm.open('clock');control.setArgs({tab:'timer'});control.setMode('clock');await pause();
  record('clock-native-timer',{text:nativeTexts(),react:texts().length});
  timerStart(30);await pause();record('clock-native-running',{text:nativeTexts()});
  timerPause();await pause();record('clock-native-paused',{text:nativeTexts()});timerReset();
  control.setArgs({tab:'world'});await pause();await pause();record('clock-native-world',{text:nativeTexts()});
  control.setArgs({tab:'alarms'});await pause();record('clock-native-alarms',{text:nativeTexts()});
  control.setArgs({tab:'stopwatch'});await pause();record('clock-native-stopwatch',{text:nativeTexts()});
  wm.close('clock',true);
  wm.open('help');control.setArgs({article:'clock',nonce:1});control.setMode('help');await pause();
  record('help-native-article',{text:nativeTexts(),react:texts().length,reactLabels:texts().map(n=>({id:n.id,text:n.text,parent:n.parent}))});
  control.setArgs({tour:true,nonce:2});await pause();record('help-native-tour',{text:nativeTexts()});
  control.setArgs({article:'missing',nonce:3});await pause();record('help-native-missing',{text:nativeTexts()});
  wm.close('help',true);
  // Monitor is a native view: Rust mounts its nodes, so React creates none of its texts or pixels.
  const reactTexts=texts().length,reactPixels=pixelBytes;
  wm.open('monitor');control.setArgs({tab:'memory'});control.setMode('monitor');
  for(let i=0;i<30&&!nativeTexts().includes('available for apps');i++)await pause();
  let monitorHandle=0;for(let h=1;h<256&&!monitorHandle;h++)if(lord.nativeViews.findText(h,'available for apps'))monitorHandle=h;
  record('monitor-memory',{handle:monitorHandle,text:nativeTexts(),texts:nativeTexts().length,react:texts().filter(n=>n.text==='available for apps').length,
    pixels:pixelBytes-reactPixels,title:wm.get('monitor')?.title});
  for(const tab of ['overview','cpu','network','disks','processes','cooling','voltage']){
    control.setArgs({tab});await pause();await pause();
    record('monitor-'+tab,{text:nativeTexts(),texts:nativeTexts().length});
  }
  control.setMode('forcequit');await pause();
  record('monitor-unmounted',{texts:(lord.nativeViews.text(monitorHandle)||'').length,reactTexts});
  // Axia Store: a native view too, with the sidebar, the editorial cards and the apps.
  wm.open('store');control.setArgs({});control.setMode('store');
  for(let i=0;i<30&&!nativeTexts().includes('Discover');i++)await pause();
  record('store',{text:nativeTexts(),title:wm.get('store')?.title});
  control.setArgs({page:'categories'});await pause();await pause();
  record('store-categories',{text:nativeTexts()});
  lord.quit();
}
setTimeout(()=>run().catch(error=>{console.error('native app driver:',error);lord.quit();}),200);
"#;
