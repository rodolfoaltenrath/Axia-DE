//! Native theme publication through the real broker and isolated app host.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use std::{fs, path::Path, os::unix::fs::PermissionsExt};

fn prepare(home: &Path) {
    for id in ["theme-probe-a", "theme-probe-b"] {
        let package = home.join(".local/share/axia/apps").join(id);
        fs::create_dir_all(&package).unwrap();
        fs::create_dir_all(home.join(".local/share/axia/appdata").join(id)).unwrap();
        fs::write(package.join("manifest.json"), serde_json::json!({"id":id,"name":"Theme probe","version":"1","permissions":[],"size":[320,180]}).to_string()).unwrap();
        fs::write(package.join("app.js"), CHILD).unwrap();
    }
    fn own(path: &Path) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::chown(path.as_ptr(),65534,65534) },0);
        }
    }
    fn tree(path: &Path) {
        own(path);
        if path.is_dir() { for entry in fs::read_dir(path).unwrap() { tree(&entry.unwrap().path()); } }
    }
    tree(&home.join(".local"));
}

#[test]
#[ignore = "requires built desktop and app-host bundle; real isolated app processes"]
fn isolated_apps_receive_native_tokens_before_render_and_after_theme_unmount() {
    let repo=Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light","dark"] { for width in [640,1000] {
        let report=ui::run_ui_with_home(repo,"theme_tokens",DRIVER,mode,width,"tokens.png",prepare);
        for flag in ["beforeRender","mirrorStale","liveChanged","lateApp","providerKeptNative","afterUnmount","catalog","noJsTheme","saved"] {
            assert_eq!(report[flag],true,"{mode}/{width}/{flag}: {report}");
        }
    } }
}

const CHILD: &str = r#"
globalThis.__lordosApp={default:function Probe(){
 const React=__lordos.require('react'),sdk=__lordos.require('@lordos/sdk'),t=sdk.useTheme(),l=sdk.useLocale();
 React.useEffect(()=>{lord.fs.write('theme.json',JSON.stringify({mode:t.mode,accent:t.accent,scrollBars:t.scrollBars,
  reduceMotion:t.reduceMotion,radius:t.controlRadius,effects:t.effects,blur:t.blur,
  lang:l.lang,region:l.region,dir:sdk.direction(),accents:Object.keys(t.accents||{}),wallpapers:t.wallpapers}));},[t.mode,t.accent,t.scrollBars,t.reduceMotion,t.controlRadius,t.effects,t.blur,l.lang,l.region,l.dir]);
 React.useEffect(()=>{const timer=setTimeout(()=>sdk.wm.close(lord.appId),18000);return()=>clearTimeout(timer);},[]);
 return React.createElement(sdk.Text,{style:{color:t.text}},'Isolated appearance probe');
}};
"#;

const DRIVER: &str = r#"
const noJsPublisher=typeof lord.broker.publish==='undefined';let jsTheme=0;
lord.broker.publish=()=>{jsTheme++;throw Error('JS theme publication');};
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<250&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h},{render,View,Text},{settings},{settingsBridge},{locale,localeBridge},{ThemeProvider},{makeTheme},{configureWindows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/settings.js'),import('@ROOT@/sdk/nativesettings.js'),
  Promise.all([import('@ROOT@/sdk/i18n.js'),import('@ROOT@/sdk/nativelocale.js')]).then(([a,b])=>({...a,...b})),
  import('@ROOT@/sdk/theme.js'),import('@ROOT@/themes/default/tokens.js'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js')]);
 configureWindows({apps:[]});
 const read=id=>{try{return JSON.parse(lord.fs.read('.local/share/axia/appdata/'+id+'/theme.json'));}catch{return null;}};
 const a='theme-probe-a',b='theme-probe-b';let beforeRender=false,mirrorStale=false,liveChanged=false,lateApp=false,providerKeptNative=false,afterUnmount=false,catalog=false,saved=null;
 try{
  settings.set({mode:'@MODE@',accent:'azul',effects:false,nativeAppearance:'axia',scrollBars:'auto',reduceMotion:false,language:'en',region:'en_US'});
  await sleep(100);
  if(!lord.apps.launchPackage(a))throw Error('First isolated app did not start');
  beforeRender=await until(()=>{const t=read(a);return t?.mode==='@MODE@'&&t.accent==='#0a84ff'&&t.radius===7&&t.effects===false&&t.lang==='en'&&t.region==='en_US'&&t.dir==='ltr';});
  const perform=settingsBridge.perform,localePerform=localeBridge.perform;settingsBridge.perform=()=>{};localeBridge.perform=()=>{};
  lord.settings.state({cmd:'patch',patch:{accent:'violeta',effects:true,nativeAppearance:'adwaita',scrollBars:'always',reduceMotion:true}});
  lord.locale.state({cmd:'patch',value:{lang:'pt',region:'pt_BR',dir:'rtl'}});
  const changed=id=>{const t=read(id);return t?.accent==='#bf5af2'&&t.radius===4&&t.scrollBars==='always'&&t.reduceMotion===true&&t.effects===true&&t.blur===24&&t.lang==='pt'&&t.region==='pt_BR'&&t.dir==='rtl';};
  liveChanged=await until(()=>changed(a));mirrorStale=settings.get().accent==='azul'&&settings.get().reduceMotion===false&&locale.get().lang==='en';
  if(!lord.apps.launchPackage(b))throw Error('Late isolated app did not start');
  lateApp=await until(()=>changed(b));
  const initial=read(b);catalog=initial?.accents?.join(',')==='azul,violeta,rosa,naranja,verde,grafito'
   &&initial.wallpapers.some(w=>w.id==='violet-horizon'&&w.variants?.length)
   &&initial.wallpapers.slice(-4).map(w=>w.id).join(',')==='aurora,ocean,sunset,graphite';
  function present(){const t=makeTheme({mode:'@MODE@',accent:'azul',effects:false});render(h(ThemeProvider,{tokens:t},
    h(View,{style:{width:@WIDTH@,height:700,backgroundColor:t.windowBg,padding:28,gap:18}},
    h(Text,{style:{color:t.text,fontSize:22}},'Native appearance for isolated apps'),
    h(Text,{style:{color:t.text}},'Live preferences and startup state come from Rust'))));}
  present();await sleep(150);providerKeptNative=changed(a)&&changed(b);
  render(h(View,{style:{width:1,height:1}}));await sleep(50);
  lord.settings.state({cmd:'patch',patch:{accent:'verde',effects:false,nativeAppearance:'axia',scrollBars:'scrolling',reduceMotion:false}});
  lord.locale.state({cmd:'patch',value:{lang:'en',region:'en_US',dir:'ltr'}});
  afterUnmount=await until(()=>[a,b].every(id=>{const t=read(id);return t?.accent==='#30d158'&&t.radius===7&&t.scrollBars==='scrolling'&&t.reduceMotion===false&&t.effects===false&&t.lang==='en'&&t.region==='en_US'&&t.dir==='ltr';}));
  settingsBridge.perform=perform;localeBridge.perform=localePerform;
  present();await sleep(150);const source=lord.media.screenshot('screenshots');saved=await media.saveImage(source,@IMAGE@);media.close(source);
 }finally{lord.broker.quit(a);lord.broker.quit(b);}
 console.log('theme_tokens-observation '+JSON.stringify({beforeRender,mirrorStale,liveChanged,lateApp,providerKeptNative,afterUnmount,catalog,noJsTheme:noJsPublisher&&jsTheme===0,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('theme_tokens driver: '+(error.stack||error))),20);
"#;
