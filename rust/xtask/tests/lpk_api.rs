//! Contract assertions are Rust; Node evaluates the actual JavaScript loader.
use serde_json::{json, Value};
use std::{path::Path, process::Command};

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn clock_ringing_stops_when_delivery_is_suppressed_or_the_banner_leaves() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import {build} from 'esbuild';
const root=process.argv[1];let now=1000,next=0,sounds=0,policy={},event=null,last;
Date.now=()=>now;
const intervals=new Map();
globalThis.setInterval=(fn,ms)=>{const id=++next;intervals.set(id,{fn,ms});return id;};
globalThis.clearInterval=id=>intervals.delete(id);
const store=initial=>{let value=initial;const subs=new Set();return{get:()=>value,set:patch=>{value=typeof patch==='function'?patch(value):patch;for(const fn of subs)fn();},subscribe:fn=>{subs.add(fn);return()=>subs.delete(fn);}};};
const notifications=store([]),settings=store({clock:{}});
const sound={play:()=>sounds++};
const probe={notifications,settings,sound,View:()=>null,Text:()=>null,t:key=>key,formatTime:()=>'',
 appNotifyPrefs:()=>({allow:true,sound:true,style:'banner',...policy}),dndActive:()=>!!policy.dnd,
 notify:options=>{last=options;if(policy.allow===false)return 0;const id=++next;
   if(!policy.dnd&&policy.style!=='none'){notifications.set([...notifications.get(),{...options,id}]);if(policy.sound!==false)sound.play();}return id;},
 dismissNotification:()=>{},ringingNotifications:store([]),registerStatusItem:()=>{},wm:{},NativeView:()=>null,clockBridge:{}};
probe.syncNativeCatalog=()=>{throw Error('isolated clock reached session catalog');};
globalThis.__clockProbe=probe;
globalThis.lord={clock:{request:()=>{const effects=event?[event]:[];event=null;return{timer:{state:'idle',duration:1000,endsAt:0,left:0},stopwatch:{running:false,start:0,acc:0,laps:[]},nextAlarm:null,wait:null,effects};}}};
const names={runtime:['View','Text'],'settings.js':['settings'],'i18n.js':['t','formatTime','locale'],
 'nativeappmodel.js':['nativeAppRequest'],'nativenotifications.js':['notificationBridge'],'nativecatalog.js':['syncNativeCatalog'],
 'notify.js':['notify','dismissNotification','notifications','appNotifyPrefs','dndActive','ringingNotifications'],
 'sound.js':['sound'],'statusitems.js':['registerStatusItem'],'windows.js':['wm'],'nativeview.js':['NativeView'],'nativeclock.js':['clockBridge']};
const result=await build({entryPoints:[path.join(root,'sdk/alarms.js')],bundle:true,write:false,format:'esm',define:{__AXIA_APP_HOST__:'true'},plugins:[{name:'clock-services',setup(build){
 build.onResolve({filter:/.*/},args=>{const name=args.path==='@lordos/runtime'?'runtime':path.basename(args.path);if(names[name]&&args.kind!=='entry-point')return{path:name,namespace:'probe'};});
 build.onLoad({filter:/.*/,namespace:'probe'},args=>({contents:names[args.path].map(name=>`export const ${name}=globalThis.__clockProbe.${name};`).join('\n')}));
 }}]});
const sdk=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
function reset(p={}){for(const n of notifications.get())n.onClose?.();notifications.set([]);intervals.clear();sdk.ringing.set([]);policy=p;sounds=0;now=1000;}
function fire(alarm=false){event=alarm?{kind:'alarm',alarm:{id:'test',label:'Alarm'},due:now}:{kind:'timer'};sdk.clockRequest('arm');}
function tick(at){now=at;for(const [id,task] of [...intervals])if(intervals.has(id))task.fn();}
const report={};
for(const [name,p] of Object.entries({denied:{allow:false},dnd:{dnd:true},silent:{sound:false},historyOnly:{style:'none'}})){
 reset(p);fire();report[name]=intervals.size===0&&sdk.ringing.get().length===0&&sounds===0;
}
reset();fire();const armed=intervals.size===1&&sdk.ringing.get().length===1;tick(3500);
report.audible=armed&&sounds===2;
notifications.set([]);const after=sounds;tick(6000);
report.evicted=intervals.size===0&&sdk.ringing.get().length===0&&sounds===after;
reset();fire(true);last.onClose();tick(3500);
report.closed=intervals.size===0&&sdk.ringing.get().length===0&&sounds===1;
reset();fire();policy={sound:false};settings.set({clock:{}});tick(3500);
report.mutedWhileRinging=intervals.size===0&&sdk.ringing.get().length===0&&sounds===1;
reset();fire();tick(61000);
report.deadline=intervals.size===0&&sdk.ringing.get().length===0&&sounds===1;
console.log(JSON.stringify(report));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for flag in [
        "denied",
        "dnd",
        "silent",
        "historyOnly",
        "audible",
        "evicted",
        "closed",
        "mutedWhileRinging",
        "deadline",
    ] {
        assert_eq!(report[flag], true, "{flag}: {report}");
    }
}

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn isolated_notifications_keep_callbacks_and_deadlines_without_session_authority() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import {build} from 'esbuild';
const root=process.argv[1];let privileged=0,preferences={},seq=0;
const timers=new Map(),closed=[],ran=[];
globalThis.setTimeout=(fn,ms)=>{const id=++seq;timers.set(id,{fn,ms});return id;};
globalThis.clearTimeout=id=>timers.delete(id);
const deny=()=>{privileged++;throw Error('isolated notification reached session authority');};
const probe={getApp:()=>null,sound:{play:()=>{}},perms:{get:()=> 'allow',set:()=>{}},
 settings:{get:()=>preferences,set:patch=>{preferences=typeof patch==='function'?patch(preferences):{...preferences,...patch};}},
 locale:{get:deny},nativeAppRequest:deny,syncNativeCatalog:deny};
globalThis.__notificationProbe=probe;
const names={'windows.js':['getApp'],'sound.js':['sound'],'privacy.js':['perms','permsRev'],
 'settings.js':['settings'],'i18n.js':['locale'],'nativeappmodel.js':['nativeAppRequest'],'nativecatalog.js':['syncNativeCatalog']};
const result=await build({entryPoints:[path.join(root,'sdk/notify.js')],bundle:true,write:false,format:'esm',
 define:{__AXIA_APP_HOST__:'true'},plugins:[{name:'local-notifications',setup(build){
 build.onResolve({filter:/.*/},args=>{const name=path.basename(args.path);if(names[name]&&args.kind!=='entry-point')return{path:name,namespace:'probe'};});
 build.onLoad({filter:/.*/,namespace:'probe'},args=>({contents:names[args.path].map(name=>`export const ${name}=globalThis.__notificationProbe.${name};`).join('\n')}));
 }}]});
const sdk=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
const first=sdk.notify({label:'Group',title:'Old',sticky:true,sound:false});
const second=sdk.notify({label:'Group',title:'New',sticky:true,sound:false,actions:[{id:'go',run:()=>ran.push('go')}]});
const grouped=sdk.grouped(sdk.notifications.get()),group=grouped.length===1&&grouped[0].more===1;
const n=sdk.notifications.get()[0];sdk.runAction(n,n.actions[0]);
const action=ran[0]==='go'&&sdk.notifications.get()[0].id===first;
sdk.dismissNotification(first);
const timed=sdk.notify({title:'Timed',timeout:1,sound:false,onClose:reason=>closed.push(reason)});
const timer=[...timers.values()][0],deadline=timer.ms===1500;timer.fn();
const expired=sdk.notifications.get().length===0&&closed.join(',')==='closed';
preferences.notifications={dnd:true};const hidden=sdk.notify({title:'DND',sound:false});
const dnd=sdk.notifications.get().length===0&&sdk.notifyHistory.get().some(n=>n.id===hidden);
sdk.setBannerPlacement('bottom');const bottom=sdk.bannerPlacement()==='bottom';
sdk.clearNotifyHistory();const cleared=sdk.notifyHistory.get().length===0;
console.log(JSON.stringify({group,action,deadline,expired,dnd,bottom,cleared,privileged}));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for flag in [
        "group", "action", "deadline", "expired", "dnd", "bottom", "cleared",
    ] {
        assert_eq!(report[flag], true, "{report}");
    }
    assert_eq!(report["privileged"], 0);
}

#[test]
#[ignore = "requires Node; explicitly required by desktop CI after npm ci"]
fn isolated_and_transitional_loaders_preserve_the_versioned_module_contract() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("runtime/lpk-api.js");
    // Only drive and serialize the actual module here; assertions stay below.
    let fixture = r#"
import fs from 'node:fs';
const { createLpkApi, LPK_API_VERSION } = await import('data:text/javascript;base64,' + fs.readFileSync(process.argv[1]).toString('base64'));
const react = {}, jsx = {}, runtime = {}, sdk = { state: 1 }, native = {};
const modules = { react, 'react/jsx-runtime': jsx, '@lordos/runtime': runtime, '@lordos/sdk': sdk };
const legacy = createLpkApi(modules);
const isolated = createLpkApi({ ...modules, '@lordos/nativeapps': native });
const names = Object.keys(modules);
const identities = names.map(name => legacy.require(name) === modules[name] && isolated.require(name) === modules[name]);
sdk.state = 2;
modules['@lordos/sdk'] = {};
const sharedState = [legacy.require('@lordos/sdk').state, isolated.require('@lordos/sdk').state];
const rejected = [];
for (const name of ['constructor', '__proto__', 'toString', 'missing', '', null, 1]) {
  try { isolated.require(name); rejected.push(false); } catch { rejected.push(true); }
}
let legacyNativeRejected = false;
try { legacy.require('@lordos/nativeapps'); } catch { legacyNativeRejected = true; }
try { isolated.apiVersion = 99; } catch {}
let deleted = false;
try { deleted = delete isolated.apiVersion; } catch {}
console.log(JSON.stringify({ version: LPK_API_VERSION, legacyVersion: legacy.apiVersion, isolatedVersion: isolated.apiVersion,
  identities, sharedState, rejected, legacyNativeRejected, nativeIdentity: isolated.require('@lordos/nativeapps') === native, deleted }));
"#;
    let result = Command::new("node")
        .args(["--input-type=module", "-e", fixture])
        .arg(source)
        .output()
        .expect("Node is required for the LPK API contract check");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["legacyVersion"], report["version"]);
    assert_eq!(report["isolatedVersion"], report["version"]);
    assert_eq!(report["identities"], json!([true, true, true, true]));
    assert_eq!(report["sharedState"], json!([2, 2]));
    assert_eq!(
        report["rejected"],
        json!([true, true, true, true, true, true, true])
    );
    assert_eq!(report["legacyNativeRejected"], true);
    assert_eq!(report["nativeIdentity"], true);
    assert_eq!(report["deleted"], false);
}

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn shell_excludes_react_surfaces_while_lpk_hosts_retain_them() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import { build } from 'esbuild';
const root = process.argv[1], reports = [];
for (const appHost of [false, true]) {
  const result = await build({ entryPoints: [path.join(root, 'sdk/index.js'),path.join(root, 'themes/default/components/Window.jsx'),path.join(root,'sdk/nativeapps.js')], outdir: '.lpk-contract', bundle: true, write: false,
    minify: true, format: 'esm', jsx: 'automatic', metafile: true,
    external: ['react', 'react/jsx-runtime', '@lordos/runtime'],
    alias: { '@lordos/sdk': path.join(root, 'sdk/index.js') },
    define: { 'process.env.NODE_ENV': '"production"', __LORD_STANDALONE__: 'true', __AXIA_APP_HOST__: String(appHost) } });
  const inputs = Object.values(result.metafile.outputs).flatMap(output => Object.entries(output.inputs));
  const bytes = suffix => inputs.filter(([name]) => name.replaceAll('\\', '/').endsWith(suffix))
    .reduce((total, [, input]) => total + input.bytesInOutput, 0);
  reports.push({ appHost, localAppControl: bytes('runtime/lpk/appcontrol.js'), reactMenu: bytes('runtime/lpk/menu.jsx'), nativeMenu: bytes('sdk/nativemenu.jsx'),
    reactBanner: bytes('runtime/lpk/notifications.js'), nativeBanner: bytes('sdk/nativebanner.js'),
    runtimeFlag: result.outputFiles.some(file => ['menuNativeView','notificationsNativeView','dockNativeView','spotlightNativeView','launcherNativeView','titleBarNativeView','filesNativeView'].some(flag => file.text.includes(flag))) });
}
console.log(JSON.stringify(reports));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .expect("Node and esbuild are required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports[0]["localAppControl"], 0,
        "local app-control compatibility leaked into shell"
    );
    assert!(reports[1]["localAppControl"].as_u64().unwrap() > 0);
    assert_eq!(reports[0]["reactMenu"], 0, "React menu leaked into shell");
    assert!(reports[0]["nativeMenu"].as_u64().unwrap() > 0);
    assert!(reports[1]["reactMenu"].as_u64().unwrap() > 0);
    assert_eq!(
        reports[0]["reactBanner"], 0,
        "React banner leaked into shell"
    );
    assert!(reports[0]["nativeBanner"].as_u64().unwrap() > 0);
    assert!(reports[1]["reactBanner"].as_u64().unwrap() > 0);
    for report in reports {
        assert_eq!(report["runtimeFlag"], false);
    }
}

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn isolated_app_control_keeps_local_windows_without_session_authority() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import {build} from 'esbuild';
const root=process.argv[1],a={id:'notes#a',app:'notes',desktop:1},b={id:'notes#b',app:'notes',desktop:1},other={id:'other',app:'other',desktop:1},panel={id:'panel',app:'panel',desktop:1};
let list=[panel,other,a,b],privileged=0,quits=[],settingsCalls=0;
const wm={get:id=>list.find(w=>w.id===id)||null,focused:()=>[...list].reverse().find(w=>!w.minimized)?.id,
 minimize:id=>{wm.get(id).minimized=true;},raise:id=>{const w=wm.get(id);w.minimized=false;list=list.filter(x=>x!==w).concat(w);},
 focus:id=>wm.raise(id),toBack:id=>{const w=wm.get(id);list=[w,...list.filter(x=>x!==w)];},open:()=>{}};
const windows={get:()=>list},getApp=id=>({id,name:id==='notes'?'Notes':id,builtin:id==='panel'});
const launcherIdOf=w=>w.app,dockIdOfWindow=launcherIdOf,quitApp=wins=>quits.push(wins.map(w=>w.id));
const deny=()=>{privileged++;throw Error('session authority used by isolated app');};
globalThis.__probe={windows,wm,getApp,onActiveDesktop:w=>w.desktop===1,activeDesktop:{get:()=>1},
 appMenus:{get:()=>({'notes#a':[{items:[{hint:'❖,',onClick:()=>settingsCalls++}]}]})},applyNativeWindowState:deny,
 launcherIdOf,dockIdOfWindow,quitApp,linuxAppFor:()=>null,shortAppName:app=>app.name,windowAppName:w=>w.app,
 t:key=>key,appName:app=>app.name,syncNativeCatalog:deny,sameFacts:()=>false,openFileWithApps:()=>{},
 nativeAppRequest:deny,nativeAppEffects:deny,executeNativeEffect:deny};
const names={ 'windows.js':['windows','wm','getApp','onActiveDesktop','activeDesktop','appMenus','applyNativeWindowState','windowModelFacts'],
 'dock.js':['launcherIdOf','dockIdOfWindow','quitApp'],'apps.js':['linuxAppFor','shortAppName','windowAppName'],
 'i18n.js':['t','appName'],'nativecatalog.js':['syncNativeCatalog'],'nativeviewfacts.js':['sameFacts'],
 'sysopen.js':['openFileWithApps'],'nativeappmodel.js':['nativeAppRequest','nativeAppEffects','executeNativeEffect']};
const result=await build({entryPoints:[path.join(root,'sdk/appcontrol.js')],bundle:true,write:false,format:'esm',
 define:{__AXIA_APP_HOST__:'true'},plugins:[{name:'local-host-observation',setup(build){
 build.onResolve({filter:/\.js$/},args=>{const resolved=path.resolve(args.resolveDir,args.path),name=path.basename(resolved);
 if(path.dirname(resolved)===path.join(root,'sdk')&&names[name])return{path:name,namespace:'probe'};
 if(resolved===path.join(root,'sdk/fs.js'))return{path:'fs',namespace:'probe'};});
 build.onLoad({filter:/.*/,namespace:'probe'},args=>({contents:args.path==='fs'?'export const recent=()=>[],basename=x=>x;':names[args.path].map(name=>`export const ${name}=globalThis.__probe.${name};`).join('\n')}));
 }}]});
const sdk=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
const group=sdk.appGroup(a.id),groupIdentity=group.key==='notes'&&group.name==='Notes'&&group.windows[0]===a&&group.windows[1]===b;
sdk.hideApp(null);sdk.hideOthers('');const emptyIdentity=!list.some(w=>w.minimized);
sdk.cycleAppWindows();const forward=wm.focused()===a.id;sdk.cycleAppWindows(true);const back=wm.focused()===b.id;
sdk.hideOthers(a.id);const othersHidden=other.minimized===true&&!panel.minimized&&!a.minimized;
sdk.hideApp(a.id);const appHidden=a.minimized&&b.minimized;sdk.showAllApps();
sdk.toggleShowDesktop();const desktopHidden=a.minimized&&b.minimized&&other.minimized&&!panel.minimized;sdk.toggleShowDesktop();
const restored=!a.minimized&&!b.minimized&&!other.minimized;
const menu=sdk.appMenuItems(a.id);menu.find(item=>item?.hint==='❖,').onClick();menu.find(item=>item?.hint==='❖Q').onClick();
sdk.quitAppGroup({key:'notes'});
console.log(JSON.stringify({groupIdentity,emptyIdentity,forward,back,othersHidden,appHidden,desktopHidden,restored,settingsCalls,quits,privileged}));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for flag in [
        "groupIdentity",
        "emptyIdentity",
        "forward",
        "back",
        "othersHidden",
        "appHidden",
        "desktopHidden",
        "restored",
    ] {
        assert_eq!(report[flag], true, "{report}");
    }
    assert_eq!(report["settingsCalls"], 1);
    assert_eq!(report["privileged"], 0);
    assert_eq!(
        report["quits"],
        json!([["notes#a", "notes#b"], ["notes#a", "notes#b"]])
    );
}

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn isolated_desktop_commands_keep_local_state_without_session_authority() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import {build} from 'esbuild';
const root=process.argv[1];let list=[1,2,3],active=2,privileged=0;
const store=get=>({get,subscribe:()=>()=>{}}),deny=()=>{privileged++;throw Error('isolated desktop reached session authority');};
const probe={windows:store(()=>[]),wm:{},activeDesktop:store(()=>active),desktopList:store(()=>list),
 setActiveDesktop:id=>{active=id;},setDesktops:(ids,id)=>{list=ids;active=id;},
 createDesktop:()=>{const id=Math.max(...list)+1;list.push(id);return id;},
 closeDesktop:id=>{if(list.length<2)return false;const i=list.indexOf(id),next=list[i+1]??list[i-1];list=list.filter(x=>x!==id);if(active===id)active=next;return true;},
 fxNodes:new Map(),overviewShowing:store(()=>false),visibleWindows:()=>[],applyNativeWindowState:deny,
 nativeAppRequest:deny,neighbor:(ids,id,step)=>ids[ids.indexOf(id)+step]??null,
 arrowStep:()=>1,swipeStep:()=>1,cleanCount:n=>n,cleanIndex:n=>n,
 settings:{get:()=>({}),set:()=>{}},t:key=>key,direction:()=> 'ltr',
 NativeView:()=>null,Layer:()=>null,MAX_DESKTOPS:9,cycleWindows:()=>{},nativeAnim:()=>null,whenAnimDone:()=>{},
 toggleOverview:()=>{},toggleAppWindows:()=>{},mainDisplay:()=>({x:0,y:0,w:640,h:700}),onScreenChange:()=>()=>{}};
globalThis.__desktopProbe=probe;
const names={
 'windows.js':['windows','wm','activeDesktop','desktopList','setActiveDesktop','setDesktops','createDesktop','closeDesktop','fxNodes','overviewShowing','visibleWindows','applyNativeWindowState'],
 'nativeappmodel.js':['nativeAppRequest'],'nativeview.js':['NativeView'],'i18n.js':['t','direction'],
 'settings.js':['settings'],'layers.js':['Layer'],'desktopmodel.js':['MAX_DESKTOPS'],
 'winmodel.js':['neighbor','arrowStep','swipeStep','cleanCount','cleanIndex'],'trackpad.js':['cycleWindows'],
 'anim.js':['nativeAnim','whenAnimDone'],'overview.jsx':['toggleOverview','toggleAppWindows'],
 runtime:['mainDisplay','onScreenChange']};
const result=await build({entryPoints:[path.join(root,'sdk/desktops.js')],bundle:true,write:false,format:'esm',
 define:{__AXIA_APP_HOST__:'true'},plugins:[{name:'local-desktops',setup(build){
 build.onResolve({filter:/.*/},args=>{if(args.path==='@lordos/runtime')return{path:'runtime',namespace:'probe'};
 const name=path.basename(args.path);if(names[name]&&args.kind!=='entry-point')return{path:name,namespace:'probe'};});
 build.onLoad({filter:/.*/,namespace:'probe'},args=>({contents:names[args.path].map(name=>`export const ${name}=globalThis.__desktopProbe.${name};`).join('\n')}));
 }}]});
const sdk=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
sdk.nextDesktopBy(1);const next=active===3;sdk.nextDesktopBy(1);const boundary=active===3;
sdk.nextDesktopBy(-1);const previous=active===2;
const added=sdk.addDesktopAndShow(),add=added===4&&active===4;
const close=sdk.closeActiveDesktop()===true&&active===3&&!list.includes(4);
console.log(JSON.stringify({next,boundary,previous,add,close,privileged}));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for flag in ["next", "boundary", "previous", "add", "close"] {
        assert_eq!(report[flag], true, "{report}");
    }
    assert_eq!(report["privileged"], 0);
}
