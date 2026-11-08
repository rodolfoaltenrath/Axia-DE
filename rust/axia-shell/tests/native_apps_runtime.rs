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
        .args(["--headless", "1000x700", "--timeout", "10", "--theme"])
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
    assert_eq!(
        get("catalog")["ids"],
        json!(["files", "forcequit"])
    );
    assert_eq!(get("invalid-id")["rejected"], true);
    assert_eq!(get("empty")["groups"], 0);
    assert!(get("empty")["text"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "No app is open"));
    assert_eq!(get("closed")["windows"], 0);
    assert_eq!(get("closed")["guards"], 0);
    assert_eq!(get("closed")["calls"], json!([]));
    assert_eq!(get("retry")["error"], "app.forceQuitRefused");
    assert_eq!(get("retry")["calls"], json!([["kill", 801], ["kill", 802]]));
    assert_eq!(get("x11")["error"], "app.forceQuitX11");
    assert_eq!(get("x11")["calls"], json!([["close", 803]]));
    assert_eq!(get("selection-removed")["calls"], json!([]));
    assert_eq!(get("remount")["calls"], json!([]));
    println!("native Rust apps: Force Quit actions and catalog passed");
}

const DRIVER: &str = r#"
import { createElement as h, useState } from '@ROOT@/node_modules/react/index.js';
import { View, render } from '@ROOT@/runtime/index.js';
import { nativeApp } from '@ROOT@/sdk/nativeapps.js';
import { configureWindows, windows, wm, registerBuiltinApp } from '@ROOT@/sdk/windows.js';
import { runningApps, forceQuit } from '@ROOT@/sdk/appcontrol.js';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { AppWindowContext } from '@ROOT@/sdk/components.jsx';
const catalog=lord.nativeApps.request(JSON.stringify({cmd:'catalog'}));
const compiled=catalog.map(nativeApp), force=compiled.find(app=>app.id==='forcequit');
const Icon=({size})=>h(View,{style:{width:size,height:size,backgroundColor:'#3489bd'}});
configureWindows({apps:[...compiled,{id:'demo',name:'Demo',Component:()=>null,Icon},{id:'isolated',name:'Isolated',isolated:true,Component:()=>null,Icon}]});
registerBuiltinApp({id:'linux',name:'Linux',builtin:true,Component:()=>null,Icon});
const nodes=new Map();let root,control,guards=0,calls=[];
for(const method of ['createNode','appendChild','insertBefore','removeChild','destroy','setRoot','setText']) {
  const original=lord[method];lord[method]=(...args)=>{
    const result=original(...args);
    if(method==='createNode')nodes.set(result,{id:result});
    if(method==='setRoot')root=args[0];
    if(method==='appendChild'||method==='insertBefore')nodes.get(args[1]).parent=args[0];
    if(method==='removeChild')nodes.get(args[1]).parent=null;
    if(method==='destroy')nodes.delete(args[0]);
    if(method==='setText')nodes.get(args[0]).text=args[1];
    return result;
  };
}
const alive=n=>{for(let i=0;n&&i<100;i++,n=nodes.get(n.parent))if(n.id===root)return true;return false;};
const texts=()=>[...nodes.values()].filter(n=>n.text&&alive(n));
const click=label=>{
  const node=texts().find(n=>n.text===label);
  if(!node)throw new Error('native app driver: missing '+label);
  const [x,y,w,height]=lord.getLayout(node.parent);
  __lordEvent('down',node.parent,x+w/2,y+height/2,0);
  __lordEvent('up',node.parent,x+w/2,y+height/2,0);
};
const pause=()=>new Promise(resolve=>setTimeout(resolve,90));
const record=(name,extra={})=>console.log('native-app-observation '+JSON.stringify({name,...extra}));
const tokens={mode:'dark',text:'#fff',textDim:'#aaa',accent:'#3584e4',separator:'#555',control:'#333',controlRadius:6};
function Harness(){const[instance,setInstance]=useState(0),[mode,setMode]=useState('forcequit'),[args,setArgs]=useState({});control={setInstance,setMode,setArgs};const app=compiled.find(app=>app.id===mode);return h(ThemeProvider,{tokens},h(AppWindowContext.Provider,{value:{id:mode,focused:true,args}},h(View,{style:{width:420,height:460}},h(app.Component,{key:mode+':'+instance}))));}
render(h(Harness));
async function run(){
  await pause(); record('catalog',{ids:catalog.map(app=>app.id)});
  let rejected=false;try{lord.nativeApps.create('forcequit\0other');}catch(error){rejected=true;}record('invalid-id',{rejected});
  record('empty',{groups:runningApps().length,text:texts().map(n=>n.text)});
  const a=wm.open('demo',{instance:'a'}),b=wm.open('demo',{instance:'b'});
  wm.setCloseGuard(a,()=>{guards++;return false;});wm.setCloseGuard(b,()=>{guards++;return false;});
  await pause();click('Demo');await pause();click('Force Quit');await pause();
  record('closed',{windows:windows.get().length,guards,calls});
  lord.wayland.kill=id=>{calls.push(['kill',id]);return id===801?'app.forceQuitRefused':'';};
  lord.wayland.close=id=>{calls.push(['close',id]);};
  wm.open('linux',{instance:801,args:{surface:801,appId:'isolated',pkg:true}});
  wm.open('linux',{instance:802,args:{surface:802,appId:'isolated',pkg:true}});
  calls=[];const retry=forceQuit('isolated');record('retry',{error:retry,calls});
  windows.set([]);
  wm.open('linux',{instance:803,args:{surface:803,appId:'xterm',x11:true}});
  calls=[];const x11=forceQuit(runningApps()[0].key);record('x11',{error:x11,calls});windows.set([]);
  wm.open('demo');await pause();click('Demo');await pause();wm.close('demo',true);await pause();
  calls=[];click('Force Quit');await pause();record('selection-removed',{calls});
  wm.open('demo');await pause();click('Demo');await pause();control.setInstance(1);await pause();
  calls=[];click('Force Quit');await pause();record('remount',{calls});
  lord.quit();
}
setTimeout(()=>run().catch(error=>{console.error('native app driver:',error);lord.quit();}),200);
"#;
