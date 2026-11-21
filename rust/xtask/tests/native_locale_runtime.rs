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
                "nativeTranslation",
                "staleTranslation",
                "nativeFormatting",
                "nativeLanguages",
                "fixedPrecision",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
        }
    }
}
const DRIVER: &str = r#"
const timeout=globalThis.setTimeout;let legacy=0,translating=false;
for(const [object,key]of [[lord.locale,'get'],[lord.locale,'languages'],[lord.text,'setLanguage'],[lord.text,'setDirection']])object[key]=()=>{legacy++;throw Error('locale used JS '+key);};
// Launcher catalog import is a separate consumer; formatting stays entirely native.
// No catalog namespace may cross JS while exercising the migrated public API.
const readCatalog=lord.locale.catalog;lord.locale.catalog=(lang,ns)=>{if(!translating&&ns==='apps')return readCatalog(lang,ns);legacy++;throw Error('translation used JS catalog '+ns);};
const sleep=ms=>new Promise(resolve=>timeout(resolve,ms));
async function until(fn){for(let i=0;i<180&&!fn();i++)await sleep(20);return!!fn();}
async function run(){
 const [{createElement:h,useState},{render,View,Text},{SettingsSession},{locale,direction,reloadCatalogs,t,tOptional,appName,formatDate,formatTime,formatNumber,monthName,weekdayName,languages},{localeBridge},{settings},{useStore},{ThemeProvider},{makeTheme},{configureWindows},{media}]=await Promise.all([
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
  h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg,padding:30}},h(SettingsSession,{key:revision}),h(Text,{style:{fontSize:18,color:theme.text}},'Locale: '+value.lang+' / '+direction()+' / '+t('clock.timer.done')),h(Text,{style:{fontSize:15,color:theme.text,marginTop:12}},formatDate(new Date(2026,10,13),'long')+' | '+formatTime(new Date(2026,10,13,13,7,9),{seconds:true})+' | '+formatNumber(1234.5,2))));}
 render(h(App));await until(()=>handle);hold=true;
 if(!lord.locale.installPack('ar',{meta:JSON.stringify({direction:'rtl'})}))throw Error('RTL fixture install failed');
 const direct=lord.locale.state({cmd:'patch',value:{lang:'ar'}});
 const observed=await until(()=>seen.some(reply=>reply.revision===direct.revision));
 const mirrorStale=locale.get().lang==='en';const old=seen.find(reply=>reply.revision===direct.revision);
 lord.locale.installPack('ar',{probe:JSON.stringify({greeting:'Native {name}',items:{one:'one:{count}',other:'other:{count}'}})});
 translating=true;
 const nativeTranslation=t('probe.greeting',{name:{x:1}})==='Native [object Object]'&&t('probe.items',{count:'01'})==='one:01'&&t('probe.items',{count:1.5})==='other:1.5'&&tOptional('probe.missing')===null&&t('probe.missing {value}',{value:'ok'})==='probe.missing ok'&&appName({id:'clock',name:'fallback'})==='Clock'&&appName({id:'probe-unknown',name:'Manifest'})==='Manifest';
 lord.locale.state({cmd:'patch',value:{lang:'pt',region:'en_US'}});
 const date=new Date(2026,10,13,13,7,9);
 const nativeFormatting=locale.get().lang==='en'&&formatDate(date,'long')==='sexta-feira, 13 de novembro de 2026'&&formatDate(date,'numeric')==='11/13/2026'&&formatTime(date,{seconds:true})==='1:07:09 PM'&&formatNumber(1234.5,2)==='1,234.50'&&monthName(10)==='novembro'&&weekdayName(5)==='sex'&&monthName('1')==='fevereiro'&&formatDate(new Date(NaN),'numeric')==='NaN/NaN/NaN'&&formatTime(new Date(NaN),{seconds:true})==='12:NaN:NaN PM';
 const choices=languages();
 const nativeLanguages=choices.some(l=>l.id==='en'&&l.name==='English'&&!l.user)&&choices.some(l=>l.id==='ar'&&l.user);
 const cases=[2.5,-2.5,1.005,2.55,2.35,0.125,-0.001,-0,1e20,1e21,5e-324,NaN,Infinity,-Infinity,0.3];
 let seed=12345;for(let i=0;i<80;i++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;cases.push((seed-2147483648)*Math.pow(10,(i%35)-20));}
 const expected=cases.flatMap(value=>[0,1,2,10,50,100].map(digits=>({value,digits,text:value.toFixed(digits).split('.').map((part,i)=>i?part:part.replace(/\B(?=(\d{3})+(?!\d))/g,',')).join('.')})));
 const fixed=Number.prototype.toFixed;Number.prototype.toFixed=()=>{legacy++;throw Error('formatting used JS toFixed');};
 let fixedPrecision=true;
 for(const c of expected){const actual=formatNumber(c.value,c.digits);if(actual!==c.text){fixedPrecision=false;console.log('fixed mismatch '+JSON.stringify({value:String(c.value),digits:c.digits,expected:c.text,actual}));break;}}
 try{formatNumber(1,101);fixedPrecision=false;}catch(error){fixedPrecision&&=error instanceof RangeError;}
 Number.prototype.toFixed=fixed;
 lord.locale.state({cmd:'patch',value:{lang:'ar',region:''}});
 translating=false;
 locale.set(value=>({...value,region:value.lang==='ar'?'ar_EG':'wrong'}));
 const nativeUpdater=locale.get().lang==='ar'&&locale.get().region==='ar_EG'&&direction()==='rtl';
 hold=false;if(old)perform({kind:'localeState',reply:old});const staleRejected=locale.get().region==='ar_EG';
 const before=lord.locale.state({cmd:'snapshot'}).revision;locale.set({lang:'ar',region:'ar_EG'});
 const quiet=lord.locale.state({cmd:'snapshot'}).revision===before;
 lord.settings.state({cmd:'patch',patch:{language:'pt',region:'pt_BR'}});
 translating=true;
 const staleTranslation=locale.get().lang==='ar'&&appName({id:'clock',name:'fallback'})==='Relógio';
 translating=false;
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
 console.log('native_locale-observation '+JSON.stringify({bound,noPayload,mirrorStale,observed,nativeUpdater,staleRejected,quiet,savedProfile,explicitDirection,remounted,packReload,singleReload,automatic,nativeTranslation,staleTranslation,nativeFormatting,nativeLanguages,fixedPrecision,noJsLocale:legacy===0,legacy,saved:!!saved?.ok}));
}
run().catch(error=>console.error('native_locale driver: '+(error.stack||error)));
"#;
