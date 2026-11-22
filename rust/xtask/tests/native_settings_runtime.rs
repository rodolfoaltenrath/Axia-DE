//! Native preference ownership and compatibility updates in the real compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built desktop and npm ci; required by GPU=1 CI"]
fn settings_owner_survives_stale_mirrors_remount_and_durable_save() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(
                repo,
                "native_settings",
                DRIVER,
                mode,
                width,
                "native-settings.png",
            );
            for flag in [
                "noObserver",
                "backgroundObserved",
                "noPayload",
                "nativeObserved",
                "mirrorStale",
                "nativeUpdater",
                "staleRejected",
                "quiet",
                "shared",
                "ownKeys",
                "durable",
                "remounted",
                "noDuplicateSave",
                "saved",
                "withoutView",
                "noJsIO",
                "legacySaved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
const timeout=globalThis.setTimeout,load=lord.settings.load,save=lord.settings.save;
let jsLoads=0,jsSaves=0,jsDeadlines=0;
lord.settings.load=()=>{jsLoads++;throw Error('settings loaded through JS');};
lord.settings.save=()=>{jsSaves++;throw Error('settings saved through JS');};
globalThis.setTimeout=(fn,ms,...args)=>{if(ms===300)jsDeadlines++;return timeout(fn,ms,...args);};
const sleep=ms=>new Promise(resolve=>timeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h,useState},{render,View,Text},{settings,settingsRevision,saveSettingsNow},{settingsBridge},{useStore},{ThemeProvider},{makeTheme},{configureWindows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/settings.js'),import('@ROOT@/sdk/nativesettings.js'),
  import('@ROOT@/sdk/store.js'),import('@ROOT@/sdk/theme.js'),import('@ROOT@/themes/default/tokens.js'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js'),import('@ROOT@/sdk/nativeview.js')]);
 let noObserver=true,noPayload=true,visualMounts=0,renderedRevision=-1,remount,hold=false,seen=[];
 const mount=lord.nativeViews.mount,drain=lord.nativeApps.takeServices;
 lord.nativeViews.mount=(node,id,...args)=>{visualMounts++;if(id==='shell:settings'){noObserver=false;throw Error('legacy preference observer mounted');}return mount(node,id,...args);};
 lord.nativeApps.takeServices=(...args)=>{noPayload&&=args.length===0;return drain.apply(lord.nativeApps,args);};
 const perform=settingsBridge.perform;
 settingsBridge.perform=effect=>{seen.push(effect);if(!hold)perform(effect);};
 settings.set({mode:'@MODE@',nativeSettingsProbe:0,stablePrefs:{value:7}});
 await until(()=>!lord.settings.state({cmd:'snapshot'}).dirty);
 const firstDisk=JSON.parse(load.call(lord.settings));
 const withoutView=visualMounts===0&&firstDisk.nativeSettingsProbe===0;
 lord.settings.state({cmd:'patch',patch:{nativeBackgroundProbe:17}});
 const backgroundObserved=await until(()=>settings.get().nativeBackgroundProbe===17)&&visualMounts===0;
 configureWindows({apps:[]});
 const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
 function App(){const[rev,setRev]=useState(0);renderedRevision=rev;remount=()=>setRev(n=>n+1);const value=useStore(settings);return h(ThemeProvider,{tokens:theme},
  h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:30},key:rev},
  h(Text,{style:{fontSize:18,color:theme.text}},'Native preferences: '+value.nativeSettingsProbe)));}
 render(h(App));await until(()=>renderedRevision>=0);hold=true;
 const stable=settings.get().stablePrefs;
 const direct=lord.settings.state({cmd:'patch',patch:{nativeSettingsProbe:9}});
 const nativeObserved=await until(()=>seen.some(e=>e.reply?.revision===direct.revision));
 const mirrorStale=settings.get().nativeSettingsProbe===0;
 const old=seen.find(e=>e.reply?.revision===direct.revision);
 settings.set(s=>({...s,fromNative:s.nativeSettingsProbe+1}));
 const nativeUpdater=settings.get().fromNative===10&&settings.get().nativeSettingsProbe===9;
 const shared=settings.get().stablePrefs===stable;
 hold=false;if(old)perform(old);
 const staleRejected=settings.get().fromNative===10;
 const revision=settingsRevision();settings.set({fromNative:10});
 const quiet=settingsRevision()===revision;
 settings.set(JSON.parse('{"__proto__":{}}'));
 const ownKeys=Object.hasOwn(settings.get(),'__proto__')&&settings.get().__proto__!==Object.prototype;
 settings.set({savedProbe:'kept'});const persisted=saveSettingsNow(true);
 const disk=JSON.parse(load.call(lord.settings));
 const status=lord.settings.state({cmd:'snapshot'});
 const durable=persisted&&disk.savedProbe==='kept'&&disk.fromNative===10&&disk.nativeSettingsProbe===9&&!status.dirty&&!status.pending;
 const previous=renderedRevision;remount();const remounted=await until(()=>renderedRevision>previous)&&settings.get().savedProbe==='kept';
 await sleep(450);const after=lord.settings.state({cmd:'snapshot'}),noDuplicateSave=!after.dirty&&!after.pending;
 const legacy={...after.state,legacyProbe:'from-file'};
 const accepted=save.call(lord.settings,JSON.stringify(legacy),true);
 const legacySaved=accepted&&await until(()=>settings.get().legacyProbe==='from-file')&&!lord.settings.state({cmd:'snapshot'}).dirty;
 const noJsIO=jsLoads===0&&jsSaves===0&&jsDeadlines===0;
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 console.log('native_settings-observation '+JSON.stringify({noObserver,backgroundObserved,noPayload,nativeObserved,mirrorStale,nativeUpdater,staleRejected,quiet,shared,ownKeys,durable,remounted,noDuplicateSave,withoutView,noJsIO,legacySaved,jsLoads,jsSaves,jsDeadlines,saved:!!saved?.ok}));
}
run().catch(error=>console.error('native_settings driver: '+(error.stack||error)));
"#;
