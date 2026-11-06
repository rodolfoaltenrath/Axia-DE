//! Real compositor/QuickJS/React probe. Fixture setup, assertions and measurements
//! are Rust; the temporary module only operates the existing UI and reports facts.
#![cfg(target_os = "linux")]
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn own(path: &Path) {
    if unsafe { libc::geteuid() } == 0 {
        let p = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(p.as_ptr(), 65534, 65534) }, 0);
    }
}
fn directory(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    own(path);
}
fn file(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    own(path);
}
#[test]
#[ignore = "requires built compositor and local esbuild; see docs/SPOTLIGHT.md"]
fn real_search_geometry_input_previews_and_resident_lifetime() {
    let repo = std::env::var_os("AXIA_SEARCH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_owned()
        })
        .canonicalize()
        .unwrap();
    let scratch = Scratch(std::env::temp_dir().join(format!(
            "axia-search-runtime-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    directory(&scratch.0);
    let home = scratch.0.join("home");
    directory(&home);
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    for name in [
        "runtime",
        ".config",
        ".config/axia",
        ".cache",
        ".local",
        ".local/share",
        "Documents",
        "Documents/samples",
        "Notes",
        "Notes/budget",
    ] {
        directory(&home.join(name));
    }
    for i in 0..3000 {
        file(
            &home.join(format!("Documents/samples/sample-{i:04}.txt")),
            "sample\n",
        );
    }
    file(
        &home.join("Documents/budget-fixture.txt"),
        "search preview sentinel\nsecond line",
    );
    file(
        &home.join("Notes/budget/nota.md"),
        "---\ntitle: Budget note\n---\nprivate budget contents",
    );
    image::RgbaImage::from_pixel(800, 600, image::Rgba([120, 30, 80, 255]))
        .save(home.join("Documents/photo-fixture.png"))
        .unwrap();
    own(&home.join("Documents/photo-fixture.png"));
    let baseline = std::env::var_os("AXIA_SEARCH_BASELINE");
    let reference = if let Some(source) = &baseline {
        let source = fs::read_to_string(source)
            .unwrap()
            .replace("'./", &format!("'{}/sdk/", repo.display()));
        let path = scratch.0.join("reference.js");
        fs::write(&path, source).unwrap();
        format!("import * as Reference from '{}';", path.display())
    } else {
        "const Reference=null;".into()
    };
    let driver = scratch.0.join("probe.jsx");
    fs::write(
        &driver,
        DRIVER
            .replace("@ROOT@", repo.to_str().unwrap())
            .replace("@REFERENCE@", &reference),
    )
    .unwrap();
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
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut command = Command::new(repo.join("server/build/axia-server"));
    command
        .args(["--headless", "1000x700", "--timeout", "45", "--theme"])
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
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut viewer = loop {
        if let Ok(viewer) = UnixStream::connect(home.join("runtime/axia/remote.sock")) {
            break viewer;
        }
        if child.try_wait().unwrap().is_some() || Instant::now() > deadline {
            let _ = child.kill();
            panic!(
                "{}",
                fs::read_to_string(scratch.0.join("stderr.log")).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    viewer
        .write_all(&[9, 0, 0, 0, 32, 2, 0, 24, 0, 44, 1, 0, 0])
        .unwrap();
    viewer
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut buffer = [0; 65536];
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        let _ = viewer.read(&mut buffer);
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("search probe timed out");
    }
    let stderr = fs::read_to_string(scratch.0.join("stderr.log")).unwrap();
    let log = fs::read_to_string(home.join(".cache/axia/session.log")).unwrap_or_default();
    assert!(child.wait().unwrap().success(), "{stderr}\n{log}");
    for error in [
        "ReferenceError:",
        "TypeError:",
        "SyntaxError:",
        "react (error)",
        "search driver:",
        "key listener:",
        "evento:",
    ] {
        assert!(!log.contains(error), "{log}\n{stderr}");
    }
    let observations = log
        .lines()
        .filter_map(|l| l.split_once("search-observation "))
        .map(|(_, s)| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    if let Some(path) = std::env::var_os("AXIA_SEARCH_OBSERVATIONS") {
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
    assert_eq!(get("shortcut")["open"], true);
    assert_eq!(get("button")["open"], true);
    assert_eq!(get("app")["selected"], "app:test-0042");
    assert_eq!(get("entered")["opened"], "test-0042");
    assert_eq!(get("calculated")["copied"], "360");
    assert_eq!(get("converted")["copied"], "1500");
    assert!(get("file")["texts"].as_array().unwrap().iter().any(|v| v
        .as_str()
        .is_some_and(|s| s.contains("search preview sentinel"))));
    assert_eq!(get("edit")["opened"], "editor");
    assert_eq!(get("reveal")["opened"], "files");
    assert_eq!(
        get("reveal")["args"]["args"]["select"],
        "budget-fixture.txt"
    );
    assert_eq!(get("photo")["image"], true);
    assert!(get("photo")["stats"]["managedImageKB"].as_u64().unwrap() > 0);
    assert_eq!(get("notes")["notes"], true);
    assert_eq!(get("help")["help"], true);
    assert_eq!(get("clipboard")["used"], "clip-fixture");
    assert_eq!(get("external")["ran"], true);
    assert_eq!(get("empty")["items"], 0);
    for stage in ["closed", "cycles", "idle"] {
        assert_eq!(get(stage)["native"]["clients"], 0);
        assert_eq!(get(stage)["native"]["activeBytes"], 0);
        assert_eq!(get(stage)["native"]["clipboardBytes"], 0);
        assert_eq!(get(stage)["stats"]["managedImageKB"], 0);
    }
    assert_eq!(get("idle")["polls"], get("cycles")["polls"]);
    if baseline.is_some() {
        assert_eq!(
            get("reference")["layout"],
            get("app")["layout"],
            "visible UI geometry changed"
        );
        for case in get("math-parity")["cases"].as_array().unwrap() {
            assert_eq!(case["reference"], case["rust"], "{case}");
        }
    }
    let warm = get("cycles-warm")["stats"]["rssKB"].as_u64().unwrap();
    let late = get("cycles")["stats"]["rssKB"].as_u64().unwrap();
    assert!(
        late <= warm + 16 * 1024,
        "resident RSS kept growing: {warm} -> {late} KiB"
    );
    println!("search real UI: shortcut/button, matching, geometry, Enter/Tab/reveal, calc/conversion, files/photo/notes/help/clipboard/provider, cleanup and idle passed; warm/late RSS {warm}/{late} KiB");
}

const DRIVER: &str = r#"
import {createElement as h,useState} from '@ROOT@/node_modules/react/index.js';
import {render,View} from '@ROOT@/runtime/index.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {spotlight,spotlightOpen,calc,convert,registerSpotlightSource} from '@ROOT@/sdk/search.js';
import {SpotlightPanel} from '@ROOT@/sdk/rustsearch.jsx';
import {MenuBar} from '@ROOT@/themes/default/components/MenuBar.jsx';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {clipboard,clipboardHistory} from '@ROOT@/sdk/clipboard.js';
@REFERENCE@
const records=new Map();let root, pendingInput, keyTimes=[], polls=0,lastReply;
for(const method of ['createNode','setRoot','appendChild','insertBefore','removeChild','setText','setStyle','setProp','destroy']){
 const original=lord[method];lord[method]=(...a)=>{const result=original(...a);
 if(method==='createNode')records.set(result,{id:result,style:{},props:{}});
 if(method==='setRoot')root=a[0];
 if(method==='appendChild'||method==='insertBefore')records.get(a[1]).parent=a[0];
 if(method==='removeChild')records.get(a[1]).parent=null;
 if(method==='setText'){records.get(a[0]).text=a[1];if(pendingInput&&a[1]===pendingInput.text){keyTimes.push(lord.now()-pendingInput.at);pendingInput=null;}}
 if(method==='setStyle')records.get(a[0]).style[a[1]]=a[2];
 if(method==='setProp')records.get(a[0]).props[a[1]]=a[2];
 if(method==='destroy')records.delete(a[0]);return result;};
}
const native=lord.launcher.request.bind(lord.launcher);
lord.launcher.request=raw=>{const input=JSON.parse(raw),result=native(raw);if(input.op==='search'){if(input.cmd==='poll'){polls++;for(const update of result.updates)lastReply=update;}else if(result?.id)lastReply=result;}return result;};
const stats=()=>native(JSON.stringify({op:'search',cmd:'stats'}));
const alive=n=>{for(let i=0;n&&i<100;i++,n=records.get(n.parent))if(n.id===root)return true;return false;};
const texts=()=>[...records.values()].filter(n=>n.text&&alive(n));
const layout=()=>texts().filter(n=>lord.getLayout(n.parent)[1]>=112).map(n=>{const [x,y,w,h]=lord.getLayout(n.parent);return{text:n.text,x,y,w,h};});
const selected=()=>lastReply?.sections?.flatMap(s=>s.items)[lastReply.selected]?.key;
const snapshot=(name,extra={})=>console.log('search-observation '+JSON.stringify({name,stats:lord.stats(),native:stats(),polls,selected:selected(),...extra}));
const pause=(ms=120)=>new Promise(resolve=>setTimeout(resolve,ms));
const key=(code,name,char='',mods=0)=>{__lordKey(code,1,name,char,name,mods);__lordKey(code,0,name,'',name,mods);};
async function type(text){let value='';for(const c of text){value+=c;pendingInput={text:value,at:lord.now()};key(0,c,c);await pause(20);}await pause(250);}
async function waitReady(){for(let i=0;i<200;i++){if(!lastReply?.pending)return;await pause(20);}throw Error('search worker did not settle');}
const Icon=({size})=>h(View,{style:{width:size,height:size,backgroundColor:'#3489bd'}});
const apps=Array.from({length:512},(_,i)=>({id:`test-${String(i).padStart(4,'0')}`,name:`App ${String(i).padStart(4,'0')}`,Icon,Component:()=>null}));
configureWindows({apps:[...apps,{id:'editor',name:'Editor',Icon,Component:()=>null}]});
const theme={effects:true,mode:'dark',text:'#fff',textDim:'#aaa',accent:'#3489bd',separator:'#555',control:'#333',menuText:'#fff',menuHover:'#444',menuBg:'#222',blur:24};
let setMode;
function Harness(){useShortcuts({grabs:'flow'});const [mode,set]=useState('new');setMode=set;return h(ThemeProvider,{tokens:theme},h(View,{style:{width:1000,height:700}},h(MenuBar,{t:theme,height:28,focused:null,onLaunchpad:()=>{},onControlCenter:()=>{},area:{x:0,y:0,w:1000,h:700}}),mode==='reference'?h(Reference.SpotlightPanel):h(SpotlightPanel)));}
resetShortcuts();render(h(Harness));
let opened,args,copied,used;const oldOpen=wm.open,oldCopy=clipboard.set,oldUse=clipboard.use;
wm.open=(id,options)=>{opened=id;args=options;return 'probe-window';};clipboard.set=text=>{copied=text;};clipboard.use=it=>{used=it.id;};
async function fresh(query){spotlight.close();await pause(30);spotlight.open();await pause(40);if(query)await type(query);await waitReady();await pause(60);}
async function run(){
 await pause(250);
 if(Reference){
  const cases=['2*(3+4)','15% de 2400','15% of 2400','2^3^2','-2^2','2**3','sqrt(2)','1,5+2','1/0','10 km to mi','72 f to c','1,5 gb to mb'].map(text=>({text,reference:{calc:Reference.calc(text),convert:Reference.convert(text)},rust:{calc:calc(text),convert:convert(text)}}));snapshot('math-parity',{cases});
  setMode('reference');await pause();Reference.spotlight.open();await pause();keyTimes=[];await type('App 0042');snapshot('reference',{layout:layout(),keyToNodeMs:keyTimes});Reference.spotlight.close();await pause();setMode('new');await pause();
 }
 key(57,'Space',' ',16);await pause();snapshot('shortcut',{open:spotlightOpen.get()});key(1,'Escape');await pause();
 const circle=[...records.values()].find(n=>alive(n)&&Math.abs(Number(n.style.width)-11.7)<0.001&&Math.abs(Number(n.style.height)-11.7)<0.001);
 if(!circle)throw Error('search button circle not found');const button=records.get(records.get(circle.parent).parent);const [x,y,w,hh]=lord.getLayout(button.id);__lordEvent('down',button.id,x+w/2,y+hh/2,0);__lordEvent('up',button.id,x+w/2,y+hh/2,0);await pause();snapshot('button',{open:spotlightOpen.get()});
 keyTimes=[];await type('App 0042');await waitReady();snapshot('app',{layout:layout(),keyToNodeMs:keyTimes});key(28,'Enter');await pause();snapshot('entered',{opened});
 await fresh('15% of 2400');key(28,'Enter');await pause();snapshot('calculated',{copied});
 await fresh('1,5 gb to mb');key(28,'Enter');await pause();snapshot('converted',{copied});
 await fresh('budget-fixture');snapshot('file',{texts:texts().map(n=>n.text)});key(28,'Enter');await pause();snapshot('edit',{opened,args});await fresh('budget-fixture');key(28,'Enter','',16);await pause();snapshot('reveal',{opened,args});
 await fresh('photo-fixture');snapshot('photo',{image:[...records.values()].some(n=>alive(n)&&String(n.props.src||'').includes('/axia/search/preview-'))});spotlight.close();await pause();
 configureWindows({apps:[...apps,...['notes','help','editor'].map(id=>({id,name:id,Icon,Component:()=>null}))]});await pause();
 await fresh('budget');snapshot('notes',{notes:lastReply.sections.some(s=>s.id==='notes')});
 await fresh('clipboard');snapshot('help',{help:lastReply.sections.some(s=>s.id==='help')});spotlight.close();await pause();
 clipboardHistory.set([{id:'clip-fixture',time:123,text:'clipneedle copied text'}]);await fresh('clipneedle');key(28,'Enter');await pause();snapshot('clipboard',{used});
 let ran=false;const unregister=registerSpotlightSource('probe',q=>q==='providertest'?[{key:'provider:test',score:80,title:'Provider result',subtitle:'host callback',Icon,run:()=>{ran=true;}}]:[]);
 await fresh('providertest');await pause();key(28,'Enter');await pause();snapshot('external',{ran});unregister();
 await fresh('nonexistent-axia-search-fixture');snapshot('empty',{items:lastReply.sections.flatMap(s=>s.items).length});key(28,'Enter');spotlight.close();await pause();snapshot('closed');
 for(let i=0;i<160;i++){spotlight.open();await pause(10);for(const c of (i%4===0?'photo-fixture':'App 0')){key(0,c,c);await pause(0);}await waitReady();await pause(10);spotlight.close();await pause(10);if(i===79)snapshot('cycles-warm');}
 await pause(200);snapshot('cycles');await pause(500);snapshot('idle');wm.open=oldOpen;clipboard.set=oldCopy;clipboard.use=oldUse;lord.quit();
}
run().catch(error=>console.error('search driver: '+error.stack));
"#;
