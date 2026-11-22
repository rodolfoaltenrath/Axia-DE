//! Native settings drive GTK integration before rendering and with a stale JS mirror.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and npm ci; headless compositor integration"]
fn portal_preferences_apply_without_a_theme_or_js_portal_effect() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "portal_appearance", DRIVER, mode, width, "portal.png");
            for flag in ["beforeRender", "nativeChanged", "mirrorStale", "providerKeptNative",
                "afterUnmount", "customPreserved", "noJsPortal", "noLegacySetter", "saved"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}

const DRIVER: &str = r#"
const noLegacySetter=typeof lord.portal.set==='undefined';
let jsPortalCalls=0;
lord.portal.set=()=>{jsPortalCalls++;throw Error('JS portal appearance effect');};
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h},{render,View,Text},{settings},{settingsBridge},{ThemeProvider},{makeTheme},{configureWindows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/settings.js'),import('@ROOT@/sdk/nativesettings.js'),
  import('@ROOT@/sdk/theme.js'),import('@ROOT@/themes/default/tokens.js'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js')]);
 configureWindows({apps:[]});
 const keyfile='.config/glib-2.0/settings/keyfile';
 const read=()=>{try{return lord.fs.read(keyfile);}catch{return '';}};
 const matches=(name,left,dark)=>{const text=read();return text.includes("accent-color='"+name+"'")
  &&text.includes("button-layout='"+(left?'close,minimize,maximize:':':minimize,maximize,close')+"'")
  &&text.includes("color-scheme='"+(dark?'prefer-dark':'default')+"'");};
 settings.set({mode:'@MODE@',accent:'azul',externalWindowStyle:false});
 const beforeRender=await until(()=>matches('blue',false,'@MODE@'==='dark'));
 lord.fs.write(keyfile,read()+'\n[custom]\nkeep=preserved\n');
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};
 const opposite='@MODE@'==='dark'?'light':'dark';
 lord.settings.state({cmd:'patch',patch:{mode:opposite,accent:'violeta',externalWindowStyle:true}});
 const nativeChanged=await until(()=>matches('purple',true,opposite==='dark'));
 const mirrorStale=settings.get().mode==='@MODE@'&&settings.get().accent==='azul'&&settings.get().externalWindowStyle===false;
 // Rendering a provider with the old values must not undo the native settings.
 function present(accent){const t=makeTheme({mode:'@MODE@',accent,effects:false});render(h(ThemeProvider,{tokens:t},
  h(View,{style:{width:@WIDTH@,height:700,backgroundColor:t.windowBg,padding:30,gap:20}},
   h(Text,{style:{color:t.text,fontSize:22}},'Native portal preferences'),
   h(Text,{style:{color:t.text,fontSize:16}},'GTK appearance follows Rust without a theme observer'),
   h(View,{style:{width:180,height:60,borderRadius:8,backgroundColor:t.accent}}))));}
 present('azul');await sleep(200);
 const providerKeptNative=matches('purple',true,opposite==='dark');
 render(h(View,{style:{width:1,height:1}}));await sleep(50);
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',externalWindowStyle:false}});
 const afterUnmount=await until(()=>matches('green',false,'@MODE@'==='dark'));
 const customPreserved=read().includes('[custom]\nkeep=preserved');
 settingsBridge.perform=perform;
 present('verde');await sleep(150);const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 console.log('portal_appearance-observation '+JSON.stringify({beforeRender,nativeChanged,mirrorStale,providerKeptNative,afterUnmount,customPreserved,noLegacySetter,noJsPortal:jsPortalCalls===0,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(error=>console.error('portal_appearance driver: '+(error.stack||error))),20);
"#;
