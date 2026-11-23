//! Native Sysinfo pages, private-query protection and exact exports in the compositor.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn sysinfo_native_pages_and_private_exports() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 960] {
                let driver = DRIVER.replace("@APPEARANCE@", appearance);
                let kind = format!("sysinfo_{appearance}");
                let report = ui::run_ui_with_capture(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "sysinfo.png",
                    |path| {
                        let protected = image::open(format!("{}.protected.png", path.display()))
                            .unwrap()
                            .into_rgb8();
                        assert_eq!(
                            protected.get_pixel(width / 2, 330).0,
                            [0, 0, 0],
                            "private content must be black in captures"
                        );
                        let normal = image::open(path).unwrap().into_rgb8();
                        assert_ne!(
                            normal.get_pixel(width / 2, 330).0,
                            [0, 0, 0],
                            "protection must be released"
                        );
                        if let Some(output) =
                            std::env::var_os(format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase()))
                        {
                            let out =
                                std::path::PathBuf::from(output).join(format!("{mode}-{width}"));
                            std::fs::create_dir_all(&out).unwrap();
                            for page in
                                ["hardware", "compat", "perf", "report", "protected", "share"]
                            {
                                std::fs::copy(
                                    format!("{}.{page}.png", path.display()),
                                    out.join(format!("{page}.png")),
                                )
                                .unwrap();
                            }
                        }
                    },
                );
                for flag in [
                    "compiled",
                    "nativeAppearance",
                    "overview",
                    "hardware",
                    "protected",
                    "hidden",
                    "compat",
                    "checks",
                    "perf",
                    "exactReport",
                    "exactJson",
                    "exactMarkdown",
                    "exactShare",
                    "noPrivateExport",
                    "saved",
                    "closed",
                ] {
                    assert_eq!(
                        report[flag], true,
                        "{appearance}/{mode}/{width}/{flag}: {report}"
                    );
                }
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/runtime/nativeapps.js';
import {configureWindows,wm,useWindows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {admin} from '@ROOT@/sdk/admin.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='sysinfo_@APPEARANCE@',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<150&&!fn();i++)await sleep(20);return!!fn();}
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',effects:false});
const fixture={date:'2026-11-23',machine:{vendor:'ACME',product:'Native machine',firmware:'uefi'},
 cpu:{model:'Example CPU',arch:'x86_64',cores:4,threads:8,maxMHz:3200},memory:{totalKB:8388608,swapKB:0},
 session:{composition:'gpu',renderer:'Fixture GPU',fixture:true},gpus:[{card:'card0',vendor:'Example',device:'Fixture GPU',driver:'example',vendorId:'8086',deviceId:'1234',renderNode:'renderD128',connected:1,connectors:1}],
 software:{lordos:'0.8.7',os:'Fedora 44',kernel:'6.18',landlockAbi:6,bpfLsm:true,cgroup2:true,live:false},storage:[{name:'nvme0n1',model:'Fixture SSD',bytes:107374182400}],displays:[]};
lord.sysinfo.report=()=>fixture;lord.sysinfo.markdown=()=> '# Fixture report\nNative hardware\n';
lord.sysinfo.peripherals=()=>[{name:'PRIVATE-PERIPHERAL',kind:'keyboard',connection:'usb'}];
const info=lord.system.info;lord.system.info=()=>({...info(),admin:true});lord.admin.available=()=>true;
admin.run=async(action,arg,log)=>{if(action==='hw-serial-numbers')log('serial system PRIVATE-SERIAL');else if(action==='hw-memory-modules')log('mem 8192|DDR4|3200|3200|Example|Part|DIMM0');else throw Error('unexpected admin action');};
lord.sysinfo.bench.saved=slot=>slot===0?{at:1795450000,mode:'quick',status:'done',lordbench:1,lordos:'0.8.7',seconds:20,results:{'cpu.sha256.1':{value:321,unit:'MB/s'}}}:null;
lord.sysinfo.bench.poll=()=>({state:'idle'});
lord.apps.launchWith=()=>false;
let handle=0,nativeAppearance=true;const mount=lord.nativeViews.mount;
lord.nativeViews.mount=(node,id,raw,...args)=>{const facts=JSON.parse(raw);if(id==='sysinfo'&&!facts.icon)nativeAppearance&&=!('theme'in(facts.context||{}));const result=mount(node,id,raw,...args);if(id==='sysinfo'&&!facts.icon)handle=result;return result;};
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='sysinfo');
const app=nativeApp(manifest);configureWindows({apps:[app]});const window=wm.open('sysinfo');
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},wm.get(window)?h(AppHost,{id:window,focused:true}):null));}
render(h(Harness));
const text=()=>lord.nativeViews.text(handle)||'';
const action=(cmd,key='')=>{lord.nativeViews.action(handle,{action:[cmd,key]});__lordNativeView();};
const capture=async suffix=>{await sleep(100);const image=lord.media.screenshot('screenshots'),result=await media.saveImage(image,@IMAGE@+suffix);media.close(image);return!!result?.ok;};
async function run(){
 const compiled=manifest?.nativeView==='rust';const overview=await until(()=>text().includes('ACME Native machine'));
 action('tab','hardware');await until(()=>text().includes('UEFI')||text().includes('uefi'));
 const hardware=text().includes('ACME');await capture('.hardware.png');
 action('serials');const protectedResult=await until(()=>text().includes('PRIVATE-SERIAL')&&wm.get(window)?.protected);
 const protectedCapture=await capture('.protected.png');
 action('hideSerials');const hidden=await until(()=>!text().includes('PRIVATE-SERIAL')&&!wm.get(window)?.protected);
 action('tab','compat');const compat=await until(()=>text().includes('Suspend isn\'t checked in this version.')&&text().includes('Works'));await capture('.compat.png');
 action('tab','checks');const checks=await until(()=>text().includes('Sandbox tests')&&text().includes('Coming next'));
 action('tab','perf');const perf=await until(()=>text().includes('321')&&text().includes('MB/s'));await capture('.perf.png');
 action('tab','report');action('format','json');await sleep(100);await capture('.report.png');
 action('copy');await until(()=>lord.clipboard.get()?.includes('Native machine'));
 const canonical=value=>Array.isArray(value)?value.map(canonical):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])):value;
 const exactReport=JSON.stringify(canonical(JSON.parse(lord.clipboard.get())))===JSON.stringify(canonical(fixture));
 action('save');await until(()=>text().includes('lordos-report-2026-11-23.json'));
 const documents=lord.fs.home()+'/Documents/';
 const exactJson=lord.fs.read(documents+'lordos-report-2026-11-23.json')===lord.clipboard.get();
 action('format','markdown');action('save');await until(()=>text().includes('lordos-report-2026-11-23.md'));
 const exactMarkdown=lord.fs.read(documents+'lordos-report-2026-11-23.md')==='# Fixture report\nNative hardware\n';
 action('share');await sleep(100);await capture('.share.png');action('save');
 const files=await until(()=>text().includes('lordos-hwreport-2026-11-23.json'));
 // The public repository is not configured. Keep this proof local: prevent opening the browser.
 action('shareBack');action('tab','overview');await sleep(100);const saved=await capture('');
 const path=lord.fs.home()+'/Documents/lordos-hwreport-2026-11-23.json';
 let exported=null;try{exported=JSON.parse(lord.fs.read(path));}catch(_){}
 const exactShare=files&&exported?.format==='lordos-hwreport'&&exported?.system?.machine?.vendor==='ACME'&&exported?.bench?.results?.['cpu.sha256.1']?.value===321;
 const noPrivateExport=exported&&!JSON.stringify(exported).includes('PRIVATE-');
 wm.close(window,true);await sleep(100);const closed=lord.nativeViews.text(handle)===null;
 console.log(kind+'-observation '+JSON.stringify({compiled,nativeAppearance,overview,hardware,protected:protectedResult&&protectedCapture,hidden,compat,checks,perf,exactReport,exactJson,exactMarkdown,exactShare,noPrivateExport,saved,closed,text:text()}));
}
setTimeout(()=>run().catch(error=>console.error(kind+' driver: '+(error.stack||error))),200);
"#;
