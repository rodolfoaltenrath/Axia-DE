//! Real compositor checks; Rust owns assertions, process lifetime and evidence.
#![cfg(target_os = "linux")]
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Session(std::process::Child);
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let start = Instant::now();
        while matches!(self.0.try_wait(), Ok(None)) && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn owned(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    if unsafe { libc::geteuid() } == 0 {
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(name.as_ptr(), 65534, 65534) }, 0);
    }
}

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn native_context_menus_ignore_legacy_settings_and_preserve_interaction() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640u32, 1000] {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let scratch = Scratch(PathBuf::from(format!(
                "/var/tmp/axia-menus-{}-{stamp}",
                std::process::id()
            )));
            fs::create_dir(&scratch.0).unwrap();
            fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
            let home = scratch.0.join("home");
            for path in [
                home.clone(),
                home.join("runtime"),
                home.join(".config"),
                home.join(".config/axia"),
            ] {
                fs::create_dir(&path).unwrap();
                owned(&path);
            }
            let permissions = home.join(".config/axia/permisos.json");
            fs::write(
                &permissions,
                r#"{"apps":{"screenshots":{"screen":{"state":"allow"}}}}"#,
            )
            .unwrap();
            owned(&permissions);
            let image = home.join("menu.png");
            let driver = scratch.0.join("probe.jsx");
            fs::write(
                &driver,
                DRIVER
                    .replace("@ROOT@", repo.to_str().unwrap())
                    .replace("@MODE@", mode)
                    .replace("@WIDTH@", &width.to_string())
                    .replace("@IMAGE@", &serde_json::to_string(&image).unwrap()),
            )
            .unwrap();
            let bundle = scratch.0.join("probe.bundle.js");
            let output = Command::new(repo.join("node_modules/.bin/esbuild"))
                .current_dir(repo)
                .arg(&driver)
                .args([
                    "--bundle",
                    "--format=iife",
                    "--jsx=automatic",
                    "--target=es2020",
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
                    "--define:__AXIA_APP_HOST__=false",
                ])
                .env("NODE_PATH", repo.join("node_modules"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut command = Command::new(repo.join("server/build/axia-server"));
            command
                .current_dir(repo.join("server"))
                .args([
                    "--headless",
                    &format!("{width}x700"),
                    "--timeout",
                    "18",
                    "--theme",
                ])
                .arg(&bundle)
                .arg("--fonts")
                .arg(repo.join("assets/fonts"))
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", home.join("runtime"))
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_CACHE_HOME", home.join(".cache"))
                .env("XDG_DATA_HOME", home.join(".local/share"))
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("LORD_GPU", "0")
                .env("AXIA_NET_OFFLINE", "1")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .env_remove("PULSE_SERVER")
                .stdout(Stdio::null())
                .stderr(fs::File::create(scratch.0.join("stderr.log")).unwrap())
                .process_group(0);
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
            let mut session = Session(command.spawn().unwrap());
            let log_path = home.join(".cache/axia/session.log");
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let text = fs::read_to_string(&log_path).unwrap_or_default();
                if text.contains("menus-observation ")
                    || text.contains("menus driver:")
                    || session.0.try_wait().unwrap().is_some()
                    || Instant::now() >= deadline
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            drop(session);
            let log = fs::read_to_string(&log_path).unwrap_or_default();
            if let Some(destination) = std::env::var_os("AXIA_MENUS_OUTPUT") {
                let destination = PathBuf::from(destination).join(format!("{mode}-{width}"));
                fs::create_dir_all(&destination).unwrap();
                fs::write(destination.join("session.log"), &log).unwrap();
                fs::copy(scratch.0.join("stderr.log"), destination.join("stderr.log")).unwrap();
                if image.is_file() {
                    fs::copy(&image, destination.join("menu.png")).unwrap();
                }
            }
            for error in [
                "menus driver:",
                "TypeError:",
                "ReferenceError:",
                "SyntaxError:",
                "react (error)",
            ] {
                assert!(!log.contains(error), "{mode}/{width}: {log}");
            }
            let report: Value = serde_json::from_str(
                log.lines()
                    .find_map(|line| {
                        line.split_once("menus-observation ")
                            .map(|(_, value)| value)
                    })
                    .expect(&log),
            )
            .unwrap();
            assert_eq!(report["ran"], json!(["alpha", "child"]), "{report}");
            assert_eq!(report["disabledRan"], false);
            assert_eq!(report["escaped"], true);
            assert_eq!(report["outsideClosed"], true);
            assert_eq!(report["saved"], true, "{report}");
            let png = fs::read(image).unwrap();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), width);
            assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 700);
        }
    }
}

const DRIVER: &str = r#"
import { createElement as h, useState, useRef } from 'react';
import { render, View } from '@lordos/runtime';
import { Menu } from '@ROOT@/sdk/menu.jsx';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { makeTheme } from '@ROOT@/themes/default/tokens.js';
import { settings } from '@ROOT@/sdk/settings.js';
import { enableShellLayers } from '@ROOT@/sdk/layers.js';
import { media } from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
let show, outside, closed=0, disabledRan=false, ran=[];
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[visible,set]=useState(true);show=set;const ref=useRef();
 return h(ThemeProvider,{tokens:theme},h(View,{ref,style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='light'?'#eeeeee':'#182036'},onMouseMove:()=>{}},
   h(View,{ref:n=>{outside=n;},style:{width:30,height:30}}),visible&&h(Menu,{x:40,y:60,onClose:()=>{closed++;set(false);},items:[
    {label:'Alpha',onClick:()=>ran.push('alpha')},'-',{label:'Disabled',disabled:true,onClick:()=>{disabledRan=true;}},
    {label:'Branch',submenu:[{label:'Child',onClick:()=>ran.push('child')}]}]})));}
const pump=()=>globalThis.__lordNativeView();
const key=k=>{lord.nativeViews.key(k,'',0);pump();};
const press=node=>{const[x,y,w,hh]=lord.getLayout(node);for(const type of ['down','up']){const mount=lord.nativeViews.pointer(type,node,x+w/2,y+hh/2,0);__lordEvent(type,mount,x+w/2,y+hh/2,0);}pump();};
async function open(){show(true);for(let i=0;i<100&&!find('Alpha');i++)await sleep(20);}
async function run(){await open();press(find('Disabled'));await sleep(50);press(find('Alpha'));await sleep(60);
 await open();key('ArrowDown');key('ArrowDown');key('ArrowRight');await sleep(100);key('ArrowDown');key('Enter');await sleep(60);
 await open();let before=closed;key('Escape');await sleep(60);const escaped=closed>before;
 await open();before=closed;press(outside.id);await sleep(60);const outsideClosed=closed>before;
 await open();await sleep(200);const handle=lord.media.screenshot('screenshots');const saved=await media.saveImage(handle,@IMAGE@);media.close(handle);
 console.log('menus-observation '+JSON.stringify({ran,disabledRan,escaped,outsideClosed,saved:!!saved?.ok}));}
settings.set({language:'en',menuNativeView:false,nativeAppearance:'axia'});enableShellLayers();render(h(App));
setTimeout(()=>run().catch(e=>console.error('menus driver: '+(e.stack||e))),200);
"#;
