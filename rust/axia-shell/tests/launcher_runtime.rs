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
fn real_launcher_catalog_and_both_theme_menus() {
    let repo = std::env::var_os("AXIA_LAUNCHER_ROOT")
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
            "axia-launcher-{}-{}",
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
        .args(["--headless", "1000x700", "--timeout", "30", "--theme"])
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
    let deadline = Instant::now() + Duration::from_secs(45);
    // The headless session renders on its own; wait for the probe to finish.
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("launcher runtime timed out");
    }
    let stderr = fs::read_to_string(scratch.0.join("stderr.log")).unwrap();
    let log = fs::read_to_string(home.join(".cache/axia/session.log")).unwrap_or_default();
    assert!(child.wait().unwrap().success(), "{stderr}\n{log}");
    for error in [
        "ReferenceError:",
        "TypeError:",
        "SyntaxError:",
        "react (error)",
        "launcher driver:",
        "evento:",
        "key listener:",
    ] {
        assert!(!log.contains(error), "{log}");
    }
    let observations: Vec<Value> = log
        .lines()
        .filter_map(|line| line.split_once("launcher-observation "))
        .map(|(_, value)| serde_json::from_str(value).unwrap())
        .collect();
    if let Some(path) = std::env::var_os("AXIA_LAUNCHER_OBSERVATIONS") {
        fs::write(
            path,
            serde_json::to_vec_pretty(&json!({"observations":observations})).unwrap(),
        )
        .unwrap();
    }
    let get = |name: &str| {
        observations
            .iter()
            .find(|v| v["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}: {log}\n{stderr}"))
    };
    let initial = get("initial");
    assert_eq!(initial["order"], initial["expectedOrder"]);
    assert_eq!(initial["catalogs"], 1);
    assert!(initial["total"].as_u64().unwrap() >= 1024);
    let mounted = initial["labels"].as_array().unwrap();
    assert!(!mounted.is_empty());
    assert!(mounted.len() <= 48, "{initial}");
    let first = mounted.iter().find(|v| v["text"] == "App 0000").unwrap();
    let next = mounted.iter().find(|v| v["text"] == "App 0006").unwrap();
    assert_eq!(first, &json!({"text":"App 0000", "x":143.5, "y":235}));
    assert_eq!(next, &json!({"text":"App 0006", "x":143.5, "y":359}));
    assert_eq!(
        next["y"].as_f64().unwrap() - first["y"].as_f64().unwrap(),
        124.
    );
    assert_eq!(get("rerender")["catalogs"], 1);
    assert_eq!(get("rerender")["sameArray"], true);
    assert!(get("scrolled")["offset"].as_f64().unwrap() > 0.);
    assert_ne!(get("scrolled")["labels"], initial["labels"]);
    assert_eq!(get("searched")["total"], 1);
    assert_eq!(get("searched")["first"], "test-0999");
    assert_eq!(get("searched")["offset"], 0);
    assert_eq!(get("entered")["opened"], "test-0999");
    assert_eq!(get("category")["total"], 512);
    assert!(get("context")["texts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "Keep in Dock"));
    assert_eq!(get("drag")["closed"], true);
    assert_eq!(get("dock-pinned")["pinned"], true);
    assert_eq!(get("dock-pinned")["closed"], false);
    assert_eq!(get("dock-unpinned")["pinned"], false);
    assert_eq!(get("classic-pinned")["pinned"], true);
    assert_eq!(get("classic-pinned")["closed"], false);
    let home = get("classic-home")["labels"].as_array().unwrap();
    for name in ["App 0000", "App 0001", "App 0002"] {
        assert_eq!(home.iter().filter(|v| v["text"] == name).count(), 1);
    }
    assert!(get("power")["texts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "Log Out"));
    assert_eq!(get("classic-search")["result"], "app:test-0007");
    assert_eq!(get("classic")["catalogs"], 1);
    assert!(get("classic")["labels"].as_array().unwrap().len() <= 20);
    assert!(!get("classic")["labels"].as_array().unwrap().is_empty());
    assert!(get("classic-scrolled")["offset"].as_f64().unwrap() > 0.);
    assert_eq!(get("installed")["catalogs"], 2);
    assert_eq!(get("installed")["found"], "new-package");
    assert_eq!(get("installed")["searchVisible"], true);
    assert_eq!(get("removed")["catalogs"], 3);
    assert!(get("removed")["found"].is_null());
    assert_eq!(get("removed")["searchVisible"], false);
    assert_eq!(get("unicode")["greek"], "greek");
    assert_eq!(get("unicode")["accent"], 1); // Search retains the previous accent-sensitive behavior.
    assert_eq!(get("unicode")["turkish"], "turkish");
    assert_eq!(get("locale")["catalogs"], 4);
    assert!(get("locale")["texts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "Jogos"));
    assert!(get("resized")["labels"].as_array().unwrap().len() <= 36);
    assert!(get("touchpad")["offset"].as_f64().unwrap() > 0.);
    assert_eq!(get("empty")["total"], 0);
    assert!(get("empty")["labels"].as_array().unwrap().is_empty());
    assert_eq!(get("empty-enter")["opened"], "test-0999");
    println!("launcher real QuickJS/React: {} apps, {} mounted app labels; cache, search/Enter, category, wheel, context, drag, Classic and install/remove passed",
        initial["total"], mounted.len());
}

const DRIVER: &str = r#"
import { createElement as h, useState } from '@ROOT@/node_modules/react/index.js';
import { render, View } from '@ROOT@/runtime/index.js';
import { configureWindows, setPackageApps } from '@ROOT@/sdk/windows.js';
import { useLauncherApps } from '@ROOT@/sdk/apps.js';
import { RustLauncher } from '@ROOT@/sdk/rustlauncher.jsx';
import { locale } from '@ROOT@/sdk/i18n.js';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { settings } from '@ROOT@/sdk/settings.js';
import { searchEverything } from '@ROOT@/sdk/search.js';
import { Menu as ClassicMenu } from '@ROOT@/themes/classic/components/Menu.jsx';
const Launchpad = props => h(RustLauncher,{...props,mode:'flow'});
const StartMenu = props => h(RustLauncher,{...props,mode:'classic',Menu:ClassicMenu});


const records = new Map(); let root, catalogs = 0, sourceNames, lastPlan, opened = null, closed = false;
for (const method of ['createNode','setRoot','appendChild','insertBefore','removeChild','setText','destroy']) {
  const original = lord[method];
  lord[method] = (...args) => {
    const result = original(...args);
    if (method === 'createNode') records.set(result, { id: result });
    if (method === 'setRoot') root = args[0];
    if (method === 'appendChild' || method === 'insertBefore') records.get(args[1]).parent = args[0];
    if (method === 'removeChild') records.get(args[1]).parent = null;
    if (method === 'setText') records.get(args[0]).text = args[1];
    if (method === 'destroy') records.delete(args[0]);
    return result;
  };
}
const native = lord.launcher.request.bind(lord.launcher);
lord.launcher.request = (raw) => {
  const input = JSON.parse(raw), result = native(raw);
  if (input.op === 'sync') { catalogs++; sourceNames=result.source.map(a=>a.name); }
  if (['view','open','event','context','scroll'].includes(input.op)) lastPlan = result;
  return result;
};
const Icon = ({size}) => h(View, {style:{width:size,height:size,backgroundColor:'#3489bd'}});
const launcherView = options => {
  const plan=native(JSON.stringify({...options,op:'view'}));
  const catalog=native(JSON.stringify({op:'catalog'}));
  return {...plan,first:plan.first==null?null:catalog.source[plan.first]};
};
const own = Array.from({length:1024}, (_,i) => ({id:`test-${String(i).padStart(4,'0')}`, name:`App ${String(i).padStart(4,'0')}`,
  category:i%2 ? 'Game':'Utility', Icon, Component:() => null}));
for(const [id,name] of [['accent-composed','Árvore'],['accent-decomposed','A\u0301rvore'],['greek','ΟΣ'],['emoji','😀'],['turkish','İstanbul']])
  own.push({id,name,category:'Office',Icon,Component:()=>null});
configureWindows({apps:own});
let control, currentApps, previousApps;
const tokens = {effects:true, mode:'dark', text:'#fff',textDim:'#aaa',accent:'#3489bd',hover:'#444',control:'#333',
  panelBg:'#252529',panelFoot:'#202024',windowBorder:'#555',tileBorder:'#444',shadow:'none'};
function Harness() {
  const apps = useLauncherApps(); currentApps = apps;
  const [mode,setMode] = useState('default'), [tick,setTick] = useState(0), [area,setArea] = useState({x:0,y:0,w:1000,h:700});
  control = {setMode,setTick,setArea};
  return h(ThemeProvider,{tokens},h(View,{style:{width:1000,height:700}},
    mode.startsWith('classic') ? h(StartMenu,{key:mode,t:tokens,box:{left:0,top:0,width:640,height:700},onClose:()=>{closed=true;},isOnBar:()=>false})
      : h(Launchpad,{key:mode,t:tokens,apps,onOpen:id=>{opened=id;},onClose:()=>{closed=true;},area})));
}
const alive = n => { for(let i=0;n && i<100;i++,n=records.get(n.parent)) if(n.id===root) return true; return false; };
const texts = () => [...records.values()].filter(n=>n.text && alive(n));
const labelNodes = () => texts().filter(n=>/^App \d{4}$/.test(n.text));
const click = (node,button=0) => {const [x,y,w,hh]=lord.getLayout(node.parent); __lordEvent('down',node.parent,x+w/2,y+hh/2,button); __lordEvent('up',node.parent,x+w/2,y+hh/2,button);};
const word = text => texts().find(n=>n.text===text);
// The query itself is also a text node. Result rows start below the 72px header.
const resultVisible = title => texts().some(n=>n.text===title && lord.getLayout(n.parent)[1]>=72);
const snapshot = (name, extra={}) => {
  const data = {name,catalogs,stats:lord.stats(),total:lastPlan?.total,first:lastPlan?.first,
    offset:lastPlan?.offset,labels:labelNodes().map(n=>({text:n.text,x:lord.getLayout(n.parent)[0],y:lord.getLayout(n.parent)[1]})),...extra};
  console.log('launcher-observation '+JSON.stringify(data));
};
const pause = (ms=250) => new Promise(resolve=>setTimeout(resolve,ms));
const type = async text => {for(const c of text) {__lordKey(0,1,c,c,c,0);await pause(40);}await pause();};
const appNode = text => labelNodes().find(n=>n.text===text);
render(h(Harness));
async function run() {
  while(!appNode('App 0000') || !(lord.getLayout(appNode('App 0000').parent)[1]>0)) await pause(25);

  previousApps=currentApps;
  snapshot('initial',{order:currentApps.map(a=>a.id),expectedOrder:[...currentApps].sort((a,b)=>a.name.localeCompare(b.name)).map(a=>a.id)});
  control.setTick(v=>v+1);await pause();snapshot('rerender',{sameArray:previousApps===currentApps});
  let node=labelNodes()[0];__lordEvent('wheel',node.parent,300,300,-20);await pause();snapshot('scrolled');
  await type('App 0999');snapshot('searched');__lordKey(28,1,'Enter','','Enter',0);snapshot('entered',{opened});
  control.setMode('fresh');await pause();click(word('Games'));await pause();snapshot('category');
  control.setMode('context');await pause();click(appNode('App 0000'),2);await pause();snapshot('context',{texts:texts().map(n=>n.text)});
  closed=false;click(word('Keep in Dock'));await pause();snapshot('dock-pinned',{pinned:settings.get().dockPinned.includes('app:test-0000'),closed});
  click(appNode('App 0000'),2);await pause();click(word('Keep in Dock'));await pause();snapshot('dock-unpinned',{pinned:settings.get().dockPinned.includes('app:test-0000'),closed});
  control.setMode('drag');await pause();closed=false;node=appNode('App 0000');let [x,y]=lord.getLayout(node.parent);
  __lordEvent('down',node.parent,x+5,y+5,0);__lordEvent('move',node.parent,x+30,y+30,0);await pause();snapshot('drag',{closed});__lordEvent('up',node.parent,x+30,y+30,0);
  control.setMode('classic');await pause();click(word('All apps'));await pause();snapshot('classic');
  closed=false;click(appNode('App 0000'),2);await pause();click(word('Pin to Start'));await pause();snapshot('classic-pinned',{pinned:settings.get().startPinned.includes('test-0000'),closed});
  settings.set({dockRecentApps:['app:test-0002','app:test-0001','app:test-0002']});click(word('Back'));await pause();snapshot('classic-home');
  const power=[...records.values()].find(n=>alive(n) && lord.getLayout(n.id)[2]===40 && lord.getLayout(n.id)[3]===40);
  click({parent:power.id});await pause();snapshot('power',{texts:texts().map(n=>n.text)});__lordKey(1,1,'Escape','','Escape',0);await pause();
  click(word('All apps'));await pause();node=labelNodes()[0];__lordEvent('wheel',node.parent,200,200,-20);await pause();snapshot('classic-scrolled');
  control.setMode('classic-search');await pause();const originalRun=searchEverything.run;let result;
  searchEverything.run=it=>{result=it?.key;};await type('App 0007');__lordKey(28,1,'Enter','','Enter',0);snapshot('classic-search',{result});searchEverything.run=originalRun;
  control.setMode('classic-install');await pause();await type('New Package');
  setPackageApps([{id:'new-package',name:'New Package',package:true,Icon,Component:()=>null}]);await pause();snapshot('installed',{found:currentApps.find(a=>a.id==='new-package')?.id || null,searchVisible:resultVisible('New Package')});
  setPackageApps([]);await pause();snapshot('removed',{found:currentApps.find(a=>a.id==='new-package')?.id || null,searchVisible:resultVisible('New Package')});
  snapshot('unicode',{greek:launcherView({query:'ΟΣ'}).first?.id,accent:launcherView({query:'árvore'}).total,turkish:launcherView({query:'İSTANBUL'}).first?.id});
  locale.set({...locale.get(),lang:'pt'});control.setMode('locale');await pause();snapshot('locale',{texts:texts().map(n=>n.text)});
  control.setArea({x:0,y:0,w:800,h:600});await pause();snapshot('resized');node=labelNodes()[0];
  __lordGesture('scroll','begin',node.parent,300,300,0,0,0,0);__lordGesture('scroll','update',node.parent,300,300,0,900,0,0);await pause();snapshot('touchpad');
  await type('nonexistent-axia-fixture');snapshot('empty');__lordKey(28,1,'Enter','','Enter',0);snapshot('empty-enter',{opened});lord.quit();
}
run().catch(error=>console.error('launcher driver: '+error.stack));
"#;
