//! A real GTK application asks the desktop's file portal for a file, as PhotoCraft does: `zenity`
//! with a very large filter list, once to open and once to save. The dialog has to appear, and the
//! chosen path has to come back to the application.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The bus and the application, stopped when the test ends however it ends.
#[derive(Default)]
struct Processes(Vec<Child>);
impl Drop for Processes {
    fn drop(&mut self) {
        for child in &mut self.0 {
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            let _ = child.wait();
        }
    }
}

/// The compositor runs as nobody when the test runs as root: so do the bus and the application.
fn as_session_user(command: &mut Command) {
    command.process_group(0);
    if unsafe { libc::geteuid() } == 0 {
        unsafe {
            command.pre_exec(|| {
                if libc::setgroups(0, std::ptr::null()) != 0 || libc::setgid(65534) != 0 || libc::setuid(65534) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}
fn own(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    if unsafe { libc::geteuid() } == 0 {
        let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(path.as_ptr(), 65534, 65534) }, 0);
    }
}

/// 35 filters, the first with 213 patterns: three spellings of 71 formats, PNG among them.
fn filters() -> Vec<String> {
    let mut formats = vec!["png".to_owned(), "pcraft".to_owned()];
    formats.extend((0..69).map(|i| format!("fmt{i}")));
    let spellings = |e: &str| {
        let classes: String = e.chars().map(|c| if c.is_ascii_alphabetic() { format!("[{c}{}]", c.to_ascii_uppercase()) } else { c.to_string() }).collect();
        format!("*.{e} *.{} *.{classes}", e.to_uppercase())
    };
    let mut out = vec![format!("All Formats | {}", formats.iter().map(|e| spellings(e)).collect::<Vec<_>>().join(" "))];
    out.extend(formats.iter().take(34).map(|e| format!("{} | {}", e.to_uppercase(), spellings(e))));
    out
}

/// Waits for the desktop's portal, then runs one dialog after the other, keeping what each answers.
fn application(home: &Path) -> Child {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let filters = filters().iter().map(|f| format!("--file-filter {}", quote(f))).collect::<Vec<_>>().join(" ");
    let folder = home.join("Documentos");
    let script = format!(
        "for i in $(seq 200); do grep -q 'portal: org.freedesktop.portal' {log} 2>/dev/null && break; sleep 0.1; done\n\
         zenity --file-selection --filename={open} {filters} > {home}/open.out 2> {home}/open.err; echo $? > {home}/open.status\n\
         zenity --file-selection --save --filename={save} {filters} > {home}/save.out 2> {home}/save.err; echo $? > {home}/save.status\n",
        log = quote(&home.join(".cache/axia/session.log").to_string_lossy()),
        open = quote(&format!("{}/", folder.to_string_lossy())),
        save = quote(&folder.join("novo.pcraft").to_string_lossy()),
        home = quote(&home.to_string_lossy()),
    );
    let runtime = home.join("runtime");
    let mut command = Command::new("sh");
    command
        .args(["-c", &script])
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", runtime.join("bus").display()))
        .env("WAYLAND_DISPLAY", "wayland-0")
        .env("GDK_BACKEND", "wayland")
        .env("GTK_USE_PORTAL", "1")
        .env("GSETTINGS_BACKEND", "keyfile")
        .env("NO_AT_BRIDGE", "1")
        .env_remove("DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    as_session_user(&mut command);
    command.spawn().unwrap()
}

fn start(home: &Path, processes: &mut Processes) {
    let folder = home.join("Documentos");
    fs::create_dir_all(&folder).unwrap();
    own(&folder, 0o755);
    let picture = folder.join("foto.png");
    fs::write(&picture, b"not really a picture").unwrap();
    own(&picture, 0o644);
    own(&home.join("runtime"), 0o700);
    let socket: PathBuf = home.join("runtime/bus");
    let mut bus = Command::new("dbus-daemon");
    bus.args(["--session", "--nofork", &format!("--address=unix:path={}", socket.display())]).stdout(Stdio::null()).stderr(Stdio::null());
    as_session_user(&mut bus);
    processes.0.push(bus.spawn().expect("dbus-daemon"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "the bus did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    processes.0.push(application(home));
}

#[test]
#[ignore = "requires built desktop, npm ci, dbus-daemon and zenity; headless compositor integration"]
fn a_gtk_application_with_a_huge_filter_list_opens_and_saves_files() {
    assert!(Command::new("zenity").arg("--version").output().is_ok_and(|o| o.status.success()), "this test needs zenity");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let mut processes = Processes::default();
    let report = ui::run_ui_with_home(repo, "portal_chooser", DRIVER, "light", 1000, "chooser.png", |home| start(home, &mut processes));
    for flag in ["openShown", "openListed", "openAnswered", "saveShown", "saveAnswered"] {
        assert_eq!(report[flag], true, "{flag}: {report}");
    }
    assert_eq!(report["opened"].as_str().map(|s| s.trim().ends_with("/Documentos/foto.png")), Some(true), "{report}");
    assert_eq!(report["saved"].as_str().map(|s| s.trim().ends_with("/Documentos/novo.pcraft")), Some(true), "{report}");
}

const DRIVER: &str = r#"
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,steps=300){for(let i=0;i<steps&&!fn();i++)await sleep(20);return!!fn();}
// what the dialog's Rust side was asked and shows
const created=[];let drawn='';
const request=lord.files.request;
lord.files.request=text=>{const result=request(text);const input=JSON.parse(text);if(input.op==='create')created.push(input);
 if(result?.tree)drawn=JSON.stringify(result.tree);return result;};
async function run(){
 const [{createElement:h},{render,View},{Powerbox},{configureWindows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/powerbox.jsx'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js'),import('@ROOT@/sdk/wayland.js')]);
 configureWindows({apps:[],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
 render(h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'#d8d8dd'}},h(Powerbox)));
 const report={};
 const read=name=>{try{return lord.fs.read(name);}catch{return'';}};
 const key=name=>{__lordKey(0,1,name,'',name,0);__lordKey(0,0,name,'',name,0);};
 report.openShown=await until(()=>created.length===1);
 report.openMode=created[0]?.mode;
 report.openListed=await until(()=>drawn.includes('foto.png'));
 await sleep(300);
 key('ArrowDown');await sleep(150);key('Enter');
 report.openAnswered=await until(()=>read('open.status').trim()==='0');
 report.opened=read('open.out');report.openError=read('open.status')+read('open.err').slice(0,400);
 report.saveShown=await until(()=>created.length===2);
 report.saveMode=created[1]?.mode;
 await sleep(500);
 const source=lord.media.screenshot('screenshots');await media.saveImage(source,@IMAGE@);media.close(source);
 key('Enter');
 report.saveAnswered=await until(()=>read('save.status').trim()==='0');
 report.saved=read('save.out');report.saveError=read('save.status')+read('save.err').slice(0,400);
 console.log('portal_chooser-observation '+JSON.stringify(report));
}
setTimeout(()=>run().catch(error=>console.error('portal_chooser driver: '+(error.stack||error))),20);
"#;
