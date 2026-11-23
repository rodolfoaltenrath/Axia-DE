//! Real native widgets and async transport with simulated block devices only.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn disks_native_confirmations_smart_and_device_changes() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let ata: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo.join("server/tests/smart/ata-ssd-worn.json")).unwrap(),
    )
    .unwrap();
    let nvme: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo.join("server/tests/smart/nvme-x.json")).unwrap(),
    )
    .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 960] {
                let kind = format!("disks_{appearance}");
                let driver = DRIVER
                    .replace("@APPEARANCE@", appearance)
                    .replace("@ATA@", &ata.to_string())
                    .replace("@NVME@", &nvme.to_string());
                let report = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "disks.png",
                    40,
                    |path| {
                        let capture = image::open(path).unwrap();
                        assert_eq!((capture.width(), capture.height()), (width, 700));
                        if let Some(output) =
                            std::env::var_os(format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase()))
                        {
                            let out =
                                std::path::PathBuf::from(output).join(format!("{mode}-{width}"));
                            std::fs::create_dir_all(&out).unwrap();
                            for page in [
                                "health",
                                "attributes",
                                "partition",
                                "confirm",
                                "progress",
                                "own",
                                "nvme",
                            ] {
                                let source = std::path::PathBuf::from(format!(
                                    "{}.{page}.png",
                                    path.display()
                                ));
                                if source.exists() {
                                    std::fs::copy(source, out.join(format!("{page}.png"))).unwrap();
                                }
                            }
                        }
                    },
                );
                for flag in [
                    "compiled",
                    "nativeAppearance",
                    "initial",
                    "noBackground",
                    "systemProtected",
                    "fullSmart",
                    "sharedSummary",
                    "attributes",
                    "smartStarted",
                    "smartAborted",
                    "partition",
                    "mounted",
                    "unmounted",
                    "ejected",
                    "renamed",
                    "formatGuard",
                    "typedNative",
                    "formatted",
                    "progress",
                    "cancelled",
                    "image",
                    "hotplug",
                    "own",
                    "nvme",
                    "saved",
                    "closed",
                    "watchReleased",
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
import {smartSummaries} from '@ROOT@/sdk/smart.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='disks_@APPEARANCE@',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms)),clone=v=>JSON.parse(JSON.stringify(v));
let handle=0,window,stage='bootstrap',nativeAppearance=true,variant='ata',running=false,pendingErase;
const ata=@ATA@,nvme=@NVME@;
const system={name:'sda',model:'System SSD',vendor:'Example',serial:'SYS-TEST',size:64000000000,removable:false,transport:'sata',rotational:false,system:true,ro:false,table:'gpt',partitions:[{name:'sda1',number:1,size:60000000000,start:1048576,fstype:'ext4',label:'System',uuid:'SYSTEM-UUID',mountpoint:'/'}]};
const usb={name:'sdb',model:'Fixture USB',vendor:'Example',serial:'USB-TEST',size:32000000000,removable:true,transport:'usb',rotational:false,system:false,ro:false,table:'gpt',partitions:[{name:'sdb1',number:1,size:16000000000,start:1048576,fstype:'vfat',label:'DATA',uuid:'USB-UUID',mountpoint:'',used:1000000000,avail:15000000000}]};
const ownDisk={...clone(usb),name:'sdc',model:'Own SMART drive',serial:'OWN-TEST',partitions:[]};
const nvmeDisk={...clone(usb),name:'nvme0n1',model:'Fixture NVMe',serial:'NVME-TEST',transport:'nvme',removable:false,partitions:[]};
let devices=[system,usb,ownDisk,nvmeDisk];const watches=[],calls=[];
lord.disks.list=()=>clone(devices);lord.disks.tools=()=>({fat32:1,ext4:1,ntfs:1,smartctl:variant!=='own'});lord.disks.watch=on=>{watches.push(on);return true;};
const info=lord.system.info;lord.system.info=()=>({...info(),admin:true});lord.admin.available=()=>true;
const currentSmart=()=>{const value=clone(variant==='nvme'?nvme:ata);if(running&&value.ata_smart_data){value.ata_smart_data.self_test.status={value:249,string:'Self-test routine in progress',remaining_percent:90};}return value;};
admin.run=async(action,arg,log)=>{
 calls.push({action,arg});
 if(action==='smart-info'||action==='smart-health'){log(JSON.stringify(currentSmart()));return '1 0';}
 if(action==='smart-test'){running=true;log('{"smartctl":{"exit_status":0}}');return '1 0';}
 if(action==='smart-abort'){running=false;log('{"smartctl":{"exit_status":0}}');return '1 0';}
 if(action==='disk-smart'){log('smart '+JSON.stringify({kind:'ata',health:'ok',temp:38,hours:120,cycles:3,reallocated:0,pending:0,uncorrectable:0,status:1}));log('attr '+JSON.stringify({id:5,value:100,worst:100,thresh:10,raw:0,prefail:true,failing:false}));return '5 ms';}
 if(action==='disk-mount'){usb.partitions[0].mountpoint='/media/test/DATA';return '/media/test/DATA';}
 if(action==='disk-unmount'){usb.partitions[0].mountpoint='';return 'ok';}
 if(action==='disk-eject')return 'ok';
 if(action==='disk-label'){usb.partitions[0].label=arg.split('\n')[1];return usb.partitions[0].label;}
 if(action==='disk-format'){usb.partitions[0].label=arg.split('\n')[2];usb.partitions[0].uuid='FORMATTED-UUID';return 'sdb1';}
 if(action==='disk-erase'){log('progress done=16000000000 total=32000000000 phase=erase');return new Promise((resolve,reject)=>{pendingErase={resolve,reject};});}
 if(action==='disk-cancel'){pendingErase.reject(Error('disk.cancelledPartial'));return 'ok';}
 if(action==='disk-write-image')return 'sha256=deadbeef mbps=12.5';
 throw Error('unexpected simulated action '+action);
};
const home=lord.fs.home();try{lord.fs.mkdir(home+'/Downloads');}catch(_){}lord.fs.write(home+'/Downloads/installer.iso','SIMULATED IMAGE');
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',effects:false});
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw);if(id==='disks'&&!facts.icon)nativeAppearance&&=!('theme'in(facts.context||{}));const result=mount(node,id,raw,...rest);if(id==='disks'&&!facts.icon)handle=result;return result;};
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='disks');
configureWindows({apps:[nativeApp(manifest)]});window=wm.open('disks');
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},wm.get(window)?h(AppHost,{key:window,id:window,focused:true}):null));}
render(h(Harness));
const text=()=>lord.nativeViews.text(handle)||'';
async function until(fn,label){stage=label;for(let i=0;i<180&&!fn();i++)await sleep(20);if(!fn())throw Error('timeout: '+label+'; '+text());return true;}
const action=(cmd,key='')=>{lord.nativeViews.action(handle,{action:[cmd,key]});__lordNativeView();};
const count=name=>calls.filter(call=>call.action===name).length;
async function capture(suffix){await sleep(100);const image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@+suffix);media.close(image);return!!result?.ok;}
function wheel(notches){const x=@WIDTH@-50,y=580,target=lord.nativeViews.hitTest(x,y);lord.nativeViews.pointer('wheel',target,x,y,notches);__lordNativeView();}
async function closeDialog(){action('dialogClose');await until(()=>!text().includes('To confirm, type the disk name:')&&!text().includes('Choose a file system'),'dialog closed');await sleep(100);}
async function run(){
 const compiled=manifest?.nativeView==='rust';const initial=await until(()=>text().includes('/dev/sdb'),'initial USB selection');await sleep(150);const noBackground=calls.length===0;
 action('select',['sda','']);await until(()=>text().includes('This is the system disk:'),'system policy');action('act','format');await sleep(80);const systemProtected=count('disk-format')===0&&!text().includes('Format “System SSD”');
 action('select',['sdb','']);action('health');const fullSmart=await until(()=>text().includes('With warnings')&&text().includes('38'),'full SMART data');const sharedSummary=await until(()=>smartSummaries.get().sdb?.health==='warning','shared SMART summary');
 wheel(-8);await capture('.health.png');action('attrs');const attributes=await until(()=>text().includes('Wear_Leveling_Count'),'full attribute table');wheel(-100);await capture('.attributes.png');wheel(100);const saved=await capture('');
 action('test','short');const smartStarted=await until(()=>/Running: 10(?:\.0)?% done/.test(text())&&count('smart-health')>0,'self-test running');await sleep(100);action('test','abort');const smartAborted=await until(()=>count('smart-abort')===1&&!text().includes('Stop the test')&&text().includes('Short test'),'self-test stopped');await sleep(120);
 action('select',['sdb','sdb1']);const partition=await until(()=>text().includes('/dev/sdb1'),'partition details');await capture('.partition.png');
 action('act','mount');const mounted=await until(()=>text().includes('/media/test/DATA')&&count('disk-mount')===1,'mount result');await sleep(100);
 action('act','unmount');const unmounted=await until(()=>count('disk-unmount')===1&&text().includes('Unmounted.'),'unmount result');await sleep(100);
 action('select',['sdb','']);action('act','eject');const ejected=await until(()=>count('disk-eject')===1&&text().includes('Ejected.'),'eject result');await sleep(100);
 action('select',['sdb','sdb1']);action('act','label');action('label','renamed');action('dialogNext');const renamed=await until(()=>text().includes('It is now called')&&calls.some(c=>c.action==='disk-label'&&c.arg==='sdb1\nRENAMED'),'rename result');await closeDialog();
 action('select',['sdb','']);action('act','format');action('label','new usb');action('dialogNext');await until(()=>text().includes('To confirm, type the disk name:'),'format confirmation');await capture('.confirm.png');
 action('confirm');await sleep(80);action('typed','sda');action('confirm');await sleep(80);const formatGuard=count('disk-format')===0;
 const field=lord.nativeViews.findText(handle,'sda');if(!field)throw Error('confirmation input not found');const [x,y,w,h]=lord.getLayout(field),cx=x+w/2,cy=y+h/2;
 lord.nativeViews.pointer('down',field,cx,cy,0);lord.nativeViews.pointer('up',field,cx,cy,0);let typedNative=lord.nativeViews.key('a','',2);for(const ch of 'sdb')typedNative&&=lord.nativeViews.key(ch,ch,0);typedNative&&=lord.nativeViews.key('Enter','',0);__lordNativeView();
 const formatted=await until(()=>text().includes('is formatted.')&&calls.some(c=>c.action==='disk-format'&&c.arg==='sdb\nfat32\nNEW USB'),'format result');await closeDialog();
 action('act','erase');action('eraseMode','zero');action('dialogNext');action('typed','sdb');action('confirm');const progress=await until(()=>text().includes('(50%)'),'erase progress');await capture('.progress.png');action('cancelJob');const cancelled=await until(()=>text().includes('half written')&&count('disk-cancel')===1,'cancel result');await closeDialog();
 action('act','image');await until(()=>text().includes('installer.iso')&&!text().includes('That file cannot be found.'),'image lookup');action('dialogNext');action('typed','sdb');action('confirm');const image=await until(()=>text().includes('deadbeef')&&calls.some(c=>c.action==='disk-write-image'&&c.arg==='sdb\n'+home+'/Downloads/installer.iso'),'image verification');await closeDialog();
 action('act','format');action('dialogNext');action('typed','sdb');const before=count('disk-format');usb.serial='REPLACED-USB';usb.model='Replacement USB';globalThis.__lordDisks();await until(()=>text().includes('Replacement USB'),'hotplug update');action('confirm');const hotplug=await until(()=>text().includes('The selected disk changed')&&count('disk-format')===before,'reject changed device');await closeDialog();
 wm.close(window,true);await sleep(150);variant='own';window=wm.open('disks',{args:{disk:'sdc',nonce:1}});const own=await until(()=>text().includes('/dev/sdc')&&text().includes('Good')&&text().includes('120'),'own SMART deep link');action('attrs');await capture('.own.png');
 wm.close(window,true);await sleep(150);variant='nvme';window=wm.open('disks',{args:{disk:'nvme0n1',nonce:2}});await until(()=>text().includes('/dev/nvme0n1')&&text().includes('Good'),'NVMe deep link');action('attrs');const nvme=await until(()=>text().includes('Critical warning')&&text().includes('Read commands'),'NVMe attributes');wheel(-100);await capture('.nvme.png');
 wm.close(window,true);await sleep(150);const closed=lord.nativeViews.text(handle)===null,watchReleased=watches.at(-1)===false;
 console.log(kind+'-observation '+JSON.stringify({compiled,nativeAppearance,initial,noBackground,systemProtected,fullSmart,sharedSummary,attributes,smartStarted,smartAborted,partition,mounted,unmounted,ejected,renamed,formatGuard,typedNative,formatted,progress,cancelled,image,hotplug,own,nvme,saved,closed,watchReleased,calls}));
}
setTimeout(()=>run().catch(async error=>{await capture('');console.log(kind+'-observation '+JSON.stringify({saved:true,error:String(error),stage,text:text(),calls}));}),200);
"#;
