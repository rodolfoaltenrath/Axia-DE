//! Locale owner, compatibility revisions and native text application in the compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built desktop and npm ci; required by GPU=1 CI"]
fn locale_state_is_native_with_stale_mirrors_and_no_js_text_setters() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(
                repo,
                "native_locale",
                DRIVER,
                mode,
                width,
                "native-locale.png",
            );
            for flag in [
                "bound",
                "noPayload",
                "mirrorStale",
                "observed",
                "nativeUpdater",
                "staleRejected",
                "quiet",
                "savedProfile",
                "explicitDirection",
                "remounted",
                "packReload",
                "singleReload",
                "automatic",
                "noJsLocale",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
const timeout=globalThis.setTimeout;let legacy=0;
for(const [object,key]of [[lord.locale,'get'],[lord.text,'setLanguage'],[lord.text,'setDirection']])object[key]=()=>{legacy++;throw Error('locale used JS '+key);};
const sleep=ms=>new Promise(resolve=>timeout(resolve,ms));
async function until(fn){for(let i=0;i<180&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h,useState},{render,View,Text},{SettingsSession},{locale,direction,reloadCatalogs,t},{localeBridge},{settings},{useStore},{ThemeProvider},{makeTheme},{configureWindows},{media}]=await Promise.all([
  import('react'),import('@lordos/runtime'),import('@ROOT@/sdk/settingssession.js'),import('@ROOT@/sdk/i18n.js'),import('@ROOT@/sdk/nativelocale.js'),import('@ROOT@/sdk/settings.js'),
  import('@ROOT@/sdk/store.js'),import('@ROOT@/sdk/theme.js'),import('@ROOT@/themes/default/tokens.js'),import('@ROOT@/sdk/windows.js'),import('@ROOT@/sdk/media.js')]);
 settings.set({language:'en',region:'',mode:'@MODE@'});configureWindows({apps:[]});
 let handle=0,bound=true,noPayload=true,remount,hold=false;const seen=[];
 const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts;
 const inspect=text=>{const value=JSON.parse(text);noPayload&&=Object.keys(value.args||{}).length===0&&!('locale'in value);};
 lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:settings'){inspect(text);bound&&=sources[0]===lord.windowList;handle=mount(node,id,text,...sources);return handle;}return mount(node,id,text,...sources);};
 lord.nativeViews.facts=(id,text)=>{if(id===handle)inspect(text);return facts(id,text);};
 const perform=localeBridge.perform;localeBridge.perform=effect=>{seen.push(effect.reply);if(!hold)perform(effect);};
 const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
 function App(){const[revision,setRevision]=useState(0);remount=()=>setRevision(n=>n+1);const value=useStore(locale);return h(ThemeProvider,{tokens:theme},
  h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:30}},h(SettingsSession,{key:revision}),h(Text,{style:{fontSize:18,color:theme.text}},'Locale: '+value.lang+' / '+direction())));}
 render(h(App));await until(()=>handle);hold=true;
 if(!lord.locale.installPack('ar',{meta:JSON.stringify({direction:'rtl'})}))throw Error('RTL fixture install failed');
 const direct=lord.locale.state({cmd:'patch',value:{lang:'ar'}});
 const observed=await until(()=>seen.some(reply=>reply.revision===direct.revision));
 const mirrorStale=locale.get().lang==='en';const old=seen.find(reply=>reply.revision===direct.revision);
 locale.set(value=>({...value,region:value.lang==='ar'?'ar_EG':'wrong'}));
 const nativeUpdater=locale.get().lang==='ar'&&locale.get().region==='ar_EG'&&direction()==='rtl';
 hold=false;if(old)perform({kind:'localeState',reply:old});const staleRejected=locale.get().region==='ar_EG';
 const before=lord.locale.state({cmd:'snapshot'}).revision;locale.set({lang:'ar',region:'ar_EG'});
 const quiet=lord.locale.state({cmd:'snapshot'}).revision===before;
 lord.settings.state({cmd:'patch',patch:{language:'pt',region:'pt_BR'}});
 const savedProfile=await until(()=>locale.get().lang==='pt'&&locale.get().region==='pt_BR'&&direction()==='ltr');
 locale.set({dir:'rtl'});settings.set({accent:'green'});const explicitDirection=direction()==='rtl'&&locale.get().dir==='rtl';
 const previous=handle;remount();const remounted=await until(()=>handle!==previous)&&direction()==='rtl';
 t('clock.timer.done');const generation=locale.get().revision;
 const installed=lord.locale.installPack('pt',{clock:JSON.stringify({'timer.done':'Native locale reload'})});
 const packReload=installed&&await until(()=>locale.get().revision>generation&&t('clock.timer.done')==='Native locale reload');
 let updates=0;const off=locale.subscribe(()=>updates++);reloadCatalogs();await sleep(120);off();const singleReload=updates===1;
 locale.set({dir:null});settings.set({language:null});lord.locale.set({LANG:'pt_BR.UTF-8'});
 const automatic=await until(()=>locale.get().lang==='pt'&&direction()==='ltr');
 await sleep(80);
 const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
 console.log('native_locale-observation '+JSON.stringify({bound,noPayload,mirrorStale,observed,nativeUpdater,staleRejected,quiet,savedProfile,explicitDirection,remounted,packReload,singleReload,automatic,noJsLocale:legacy===0,legacy,saved:!!saved?.ok}));
}
run().catch(error=>console.error('native_locale driver: '+(error.stack||error)));
"#;
