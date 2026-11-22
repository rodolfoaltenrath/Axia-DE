//! Real native notification surface, with Rust assertions and private session.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
use serde_json::json;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn native_notifications_preserve_actions_grouping_dnd_timeout_and_placement() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640u32, 1000] {
            let report = ui::run_ui_with_capture_timeout(
                repo,
                "notifications",
                DRIVER,
                mode,
                width,
                "notifications.png",
                24,
                |path| {
                    let pixels = image::open(path).unwrap().into_rgb8();
                    let ink = if mode == "light" { [29,29,31] } else { [245,245,247] };
                    assert!(pixels.pixels().filter(|p| p.0 == ink).count() > 10, "native notice ink: {mode}/{width}");
                    assert!(pixels.pixels().filter(|p| p.0 == [48,209,88]).count() > 50, "native notice accent: {mode}/{width}");
                },
            );
            assert_eq!(report["ran"], json!(["second", "body"]), "{report}");
            assert_eq!(report["grouped"], true, "{report}");
            assert_eq!(report["oldRevealed"], true, "{report}");
            assert_eq!(report["dndSuppressed"], true, "{report}");
            assert_eq!(report["timerGone"], true, "{report}");
            for flag in [
                "noAppearanceFacts",
                "nativePlacement",
                "clickThrough",
                "withoutAppearanceFacts",
                "staleAppearanceMirror",
                "appearanceRetained",
                "bound",
                "noPayload",
                "noJsDeadline",
                "nativeDirect",
                "mirrorStale",
                "remounted",
                "backgroundExpired",
                "ringGuard",
                "nativeRing",
                "nativeSound",
                "nativePreferences",
                "rawPolicy",
                "nativeAccess",
                "noAccessFacts",
                "nativeWrite",
                "revisionOrder",
                "residentPreferences",
                "nativeCatalog",
                "nativePresentation",
                "noPresentationFacts",
                "nativeLocaleSource",
                "noLocaleFacts",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
            assert_eq!(
                report["reasons"],
                json!(["second", "first", "closed", "closed", "closed"]),
                "{report}"
            );
            let bounds = report["finalBounds"].as_array().unwrap();
            assert!(
                bounds[0].as_f64().unwrap() >= 20.
                    && bounds[0].as_f64().unwrap() < (width as f64 / 2.),
                "RTL: {report}"
            );
            assert!(
                bounds[1].as_f64().unwrap() > 350.,
                "bottom placement: {report}"
            );
            assert!(
                bounds[0].as_f64().unwrap() + bounds[2].as_f64().unwrap() <= width as f64 - 20.,
                "{report}"
            );
            assert_eq!(report["saved"], true, "{report}");
        }
    }
}

const DRIVER: &str = r#"
import { createElement as h, useState } from 'react';
import { render, View } from '@lordos/runtime';
import { NotificationBanner } from '@ROOT@/sdk/nativebanner.js';
import { notify, dismissNotification, notifications, notifyHistory, notificationPlacement, setBannerPlacement, NOTIFY_DEFAULTS, notifyPrefs, appNotifyPrefs, setAppNotifyPrefs, dndActive } from '@ROOT@/sdk/notify.js';
import { clockRequest, ringing } from '@ROOT@/sdk/alarms.js';
import { sound } from '@ROOT@/sdk/sound.js';
import { perms, permsRev } from '@ROOT@/sdk/privacy.js';
import { notificationBridge } from '@ROOT@/sdk/nativenotifications.js';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { makeTheme } from '@ROOT@/themes/default/tokens.js';
import { settings } from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import { localeBridge } from '@ROOT@/sdk/nativelocale.js';
import { locale, direction } from '@ROOT@/sdk/i18n.js';
import { configureWindows } from '@ROOT@/sdk/windows.js';
import { media } from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const opposite='@MODE@'==='light'?'dark':'light';
let noAppearanceFacts=true,appearanceFacts=0,lastAppearanceFacts;
function inspectAppearance(raw){const input=JSON.parse(raw);lastAppearanceFacts=input;noAppearanceFacts&&=!('theme'in(input.context||{}))&&!('scale'in(input.context||{}))&&!('surface'in(input.args||{}))&&!('radius'in(input.args||{}));}
async function changeAppearance(){
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};const before=appearanceFacts;
 lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',effects:false,nativeAppearance:'adwaita'}});await sleep(120);
 const withoutAppearanceFacts=appearanceFacts===before,staleAppearanceMirror=settings.get().mode===opposite;
 facts(bannerHandle,JSON.stringify({...lastAppearanceFacts,args:{...lastAppearanceFacts.args,surface:{backgroundColor:'#ff0000'},radius:999},context:{...lastAppearanceFacts.context,theme:{mode:opposite,text:'#ff0000',accent:'#ff0000'},scale:{body:200}}}));
 settingsBridge.perform=perform;
 return {withoutAppearanceFacts,staleAppearanceMirror};
}
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
async function until(fn,tries=100){for(let i=0;i<tries&&!fn();i++)await sleep(20);return!!fn();}
const ran=[], reasons=[];
let bannerHandle=0,bound=false,noPayload=true,toggle;
const background={current:null};
let noSoundEffects=true,rawPolicy=true,noAccessFacts=true,nativeCatalog=false,noPresentationFacts=true,noLocaleFacts=true;
const perform=notificationBridge.perform,request=lord.nativeApps.request;
notificationBridge.perform=effect=>{if(effect.kind==='notificationSound')noSoundEffects=false;perform(effect);};
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='notifications'||input.cmd==='clockCommand')for(const key of ['lang','region','revision','localeRevision'])noLocaleFacts&&=!(key in input);if(input.cmd==='notifications'){rawPolicy&&=!('prefs'in input)&&input.action!=='preferencesPatch';noAccessFacts&&=!('permission'in input)&&!('blocked'in input)&&!('source'in input);if(input.action==='push'){rawPolicy&&=!('appPrefs'in input)&&!('prefs'in input);noPresentationFacts&&=!('wallMs'in input)&&!('minute'in input)&&!('hasApp'in(input.notice||{}))&&!('displayLabel'in(input.notice||{}))&&!('timeLabel'in(input.notice||{}));}}if(input.cmd==='clockCommand')noAccessFacts&&=!('notificationPolicy'in input);if(input.notificationPolicy){rawPolicy&&=!('clock'in input.notificationPolicy)&&!('prefs'in input.notificationPolicy);noAccessFacts&&=!('permission'in input.notificationPolicy)&&!('blocked'in input.notificationPolicy);}if(input.action==='ringPolicy')rawPolicy&&=!('clock'in input);const reply=request.call(lord.nativeApps,text);for(const effect of [...(reply.effects||[]),...(reply.notificationEffects||[])])if(effect.kind==='notificationSound')noSoundEffects=false;return reply;};
const mount=lord.nativeViews.mount,unmount=lord.nativeViews.unmount,facts=lord.nativeViews.facts;
function inspect(text){inspectAppearance(text);const value=JSON.parse(text);noPayload&&=!('tree'in(value.args||{}))&&!('shown'in(value.args||{}))&&!('notifications'in(value.args||{}))&&!('ungrouped'in(value.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{if(id==='shell:notifications'){inspect(text);bound=sources[0]===lord.windowList;bannerHandle=mount(node,id,text,...sources);return bannerHandle;}return mount(node,id,text,...sources);};
lord.nativeViews.facts=(handle,text)=>{if(handle===bannerHandle){inspect(text);appearanceFacts++;}return facts(handle,text);};
lord.nativeViews.unmount=handle=>{if(handle===bannerHandle)bannerHandle=0;return unmount(handle);};
const push=(label,title,options={})=>notify({label,title,body:'Notification body',sticky:true,sound:false,onClose:reason=>reasons.push(reason),...options});
const press=async node=>{await until(()=>{const r=lord.getLayout(node);return r&&r[2]>0&&r[3]>0;});const[x,y,w,hh]=lord.getLayout(node);for(const type of ['down','up']){const mount=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x+w/2,y+hh/2),x+w/2,y+hh/2,0);__lordEvent(type,mount,x+w/2,y+hh/2,0);}globalThis.__lordNativeView();};
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
settings.set({language:'en',mode:opposite,accent:'azul',effects:false,nativeAppearance:'axia',notificationsNativeView:false,notifications:{grouping:'app',dnd:false}});
const probeApps=[{id:'access-probe',name:'Access probe',package:true,Component:()=>null},{id:'isolated-probe',name:'Isolated probe',isolated:true,Component:()=>null},{id:'clock',name:'Clock',Component:()=>null}];
configureWindows({apps:probeApps,workArea:{x:20,y:28,w:@WIDTH@-40,h:644}});
function App(){const[on,setOn]=useState(true);toggle=setOn;return h(ThemeProvider,{tokens:theme},h(View,{ref:background,style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},on?h(NotificationBanner):null));}
render(h(App));
async function run(){
 const grouping=NOTIFY_DEFAULTS.grouping;NOTIFY_DEFAULTS.grouping='off';
 const defaults=notifyPrefs();
 const initialPrefs=settings.get().notifications;
 setAppNotifyPrefs('clock',{sound:false});const options=appNotifyPrefs('clock');
 settings.set({notifications:initialPrefs});
 NOTIFY_DEFAULTS.grouping=grouping;
 const original=settings.get().notifications;
 settings.set({notifications:{...original,extra:'keep',apps:{notes:{lock:false},clock:{sound:false,custom:3}}}});
 setAppNotifyPrefs('clock',{style:'alert'});
 const edited=settings.get().notifications;
 const nativePreferences=defaults.grouping==='app'&&defaults.schedule.from===1320&&options.allow&&options.style==='banner'&&!options.sound
   &&edited.apps.clock.style==='alert'&&edited.apps.clock.custom===3&&!edited.apps.clock.sound&&!edited.apps.notes.lock&&edited.extra==='keep';
 settings.set({notifications:original});
 const localePerform=localeBridge.perform;localeBridge.perform=()=>{};
 const beforePresentation=lord.settings.state({cmd:'snapshot'}).state;
 lord.settings.state({cmd:'patch',patch:{language:'pt',region:'en_US',clock:{hour24:'24'},notifications:{...beforePresentation.notifications,seen:['clock']}}});
 const presented=request.call(lord.nativeApps,JSON.stringify({cmd:'notifications',action:'push',lang:'en',region:'',wallMs:0,minute:0,
   notice:{app:'clock',title:'Native presentation',sound:false,sticky:true,hasApp:false,displayLabel:'Forged caption',timeLabel:'Forged time'}}));
 for(const effect of presented.effects||[])notificationBridge.perform(effect);
 const displayed=notifyHistory.get().find(n=>n.id===presented.result);
 const captionVisible=await until(()=>find('Relógio')&&find('Native presentation'));
 const nativePresentation=captionVisible&&locale.get().lang==='en'&&displayed?.displayLabel==='Relógio'&&displayed.hasApp===true&&/^\d{2}:\d{2}$/.test(displayed.timeLabel)&&displayed.time>0;
 dismissNotification(presented.result);
 const originalLocaleGet=lord.locale.get;let jsLocaleReads=0,nativePackReload=false,nativeBootstrap=false;
 lord.locale.get=()=>{jsLocaleReads++;throw Error('native service queried locale through JS');};
 try{
   const installed=lord.locale.installPack('pt',{apps:JSON.stringify({clock:'Native pack caption'})});
   const packed=notify({app:'clock',title:'Native catalog reload',sound:false,sticky:true});
   const packNotice=notifyHistory.get().find(n=>n.id===packed);
   nativePackReload=installed&&packNotice?.displayLabel==='Native pack caption'&&await until(()=>find('Native pack caption'));
   dismissNotification(packed);
   lord.settings.state({cmd:'patch',patch:{language:null}});
   const configured=lord.locale.set({LANG:'pt_BR.UTF-8'});
   const automatic=notify({app:'clock',title:'Native locale bootstrap',sound:false,sticky:true});
   nativeBootstrap=configured&&notifyHistory.get().find(n=>n.id===automatic)?.displayLabel==='Native pack caption'&&locale.get().lang==='en';
   dismissNotification(automatic);
 }finally{lord.locale.get=originalLocaleGet;localeBridge.perform=localePerform;}
 const nativeLocaleSource=nativePackReload&&nativeBootstrap&&jsLocaleReads===0;
 lord.settings.state({cmd:'replace',value:beforePresentation});
 push('Grouped','Grouped old');push('Grouped','Grouped latest');
 await until(()=>find('Grouped latest'));const grouped=!!find('Grouped · +1')&&!find('Grouped old');
 const nativePrefs=()=>lord.settings.state({cmd:'snapshot'}).state.notifications;
 lord.settings.state({cmd:'patch',patch:{notifications:{...nativePrefs(),grouping:'off'}}});
 const staleGrouping=settings.get().notifications.grouping==='app';
 const ungroupedNative=await until(()=>find('Grouped old')&&!find('Grouped · +1'));
 lord.settings.state({cmd:'patch',patch:{notifications:{...nativePrefs(),grouping:'app'}}});
 const regroupedNative=await until(()=>find('Grouped · +1')&&!find('Grouped old'));
 lord.settings.state({cmd:'patch',patch:{notifications:{...nativePrefs(),dnd:true}}});
 const staleDnd=settings.get().notifications.dnd===false;
 const nativeQueries=dndActive()&&notifyPrefs().dnd===true;
 const hidden=request.call(lord.nativeApps,JSON.stringify({cmd:'notifications',action:'push',prefs:{dnd:false},notice:{title:'Resident DND'},wallMs:Date.now(),minute:0}));
 const residentState=request.call(lord.nativeApps,JSON.stringify({cmd:'notifications',action:'snapshot'})).result;
 const nativeHidden=hidden.result>0&&!residentState.shown.some(n=>n.title==='Resident DND')&&residentState.history.some(n=>n.title==='Resident DND');
 lord.settings.state({cmd:'patch',patch:{notifications:{...nativePrefs(),dnd:false}}});
 const beforeNativeEdit=nativePrefs();
 lord.settings.state({cmd:'patch',patch:{notifications:{...beforeNativeEdit,nativeOnly:'keep',apps:{clock:{custom:7,sound:false}}}}});
 setAppNotifyPrefs('clock',{style:'alert'});
 const committed=nativePrefs();
 const nativeEdit=committed.nativeOnly==='keep'&&committed.apps.clock.custom===7&&committed.apps.clock.sound===false&&committed.apps.clock.style==='alert'&&!appNotifyPrefs('clock').sound;
 lord.settings.state({cmd:'patch',patch:{notifications:beforeNativeEdit}});
 const residentPreferences=staleGrouping&&ungroupedNative&&regroupedNative&&staleDnd&&nativeHidden&&nativeQueries&&nativeEdit;
 push('Actions','Action primary',{actions:[{id:'first',label:'First',run:()=>ran.push('first')},{id:'second',label:'Second',run:()=>ran.push('second')}]});
 await until(()=>find('Second'));await press(find('Second'));await until(()=>!find('Action primary'));
 push('Body','Body action',{actions:[{id:'first',label:'Default action',run:()=>ran.push('body')}]});
 await until(()=>find('Body action'));await press(find('Body action'));await until(()=>!find('Body action'));
 await press(find('×'));await until(()=>find('Grouped old'));const oldRevealed=!!find('Grouped old')&&!find('Grouped latest');
 await press(find('×'));await until(()=>notifications.get().length===0);
 settings.set(s=>({...s,notifications:{...s.notifications,dnd:true}}));
 push('DND','Suppressed');await sleep(50);
 const dndSuppressed=notifications.get().length===0&&notifyHistory.get().some(n=>n.title==='Suppressed');
 settings.set(s=>({...s,notifications:{...s.notifications,dnd:false}}));
 const setTimeoutBefore=globalThis.setTimeout;let noJsDeadline=true;
 globalThis.setTimeout=()=>{noJsDeadline=false;throw Error('notification armed a JS deadline');};
 try{push('Timer','Timed banner',{sticky:false,timeout:1500});}finally{globalThis.setTimeout=setTimeoutBefore;}
 await until(()=>find('Timed banner'));await sleep(1700);
 const timerGone=notifications.get().length===0&&!find('Timed banner');
 // Discard the compatibility snapshot: the native surface must still see the new notice.
 const direct=lord.nativeApps.request(JSON.stringify({cmd:'notifications',action:'push',
   notice:{title:'Direct native',label:'Direct',sticky:true,sound:false}}));
 const nativeDirect=await until(()=>find('Direct native'));
 const mirrorStale=!notifications.get().some(n=>n.title==='Direct native');
 lord.nativeViews.action(bannerHandle,{action:['dismiss',String(direct.result)]});__lordNativeView();await until(()=>!find('Direct native'));
 notificationBridge.perform(direct.effects.find(effect=>effect.kind==='notificationState'));
 const revisionOrder=!notifications.get().some(n=>n.title==='Direct native');
 notify({title:'Survives remount',sticky:false,timeout:1500,sound:false});await until(()=>find('Survives remount'));
 toggle(false);await until(()=>!bannerHandle);await sleep(700);toggle(true);await until(()=>find('Survives remount'));
 const remounted=!!find('Survives remount')&&await until(()=>!find('Survives remount'));
 toggle(false);await until(()=>!bannerHandle);
 let backgroundReason;const backgroundId=notify({title:'Expires without a banner',sticky:false,timeout:1500,sound:false,onClose:r=>{backgroundReason=r;}});
 const backgroundExpired=await until(()=>!notifications.get().some(n=>n.id===backgroundId))&&!bannerHandle&&backgroundReason==='closed';
 toggle(true);await until(()=>bannerHandle);
 const getPermission=perms.get,getBlocked=lord.fw.blocks;let jsAccess=0,nativeAccess=false,accessDetails={},nativeWrite=false,writeDetails={};
 perms.get=()=>{jsAccess++;return'allow';};lord.fw.blocks=()=>{jsAccess++;return false;};
 try{
   lord.perms.set('access-probe','notifications','deny');
   const deniedPrefs=!appNotifyPrefs('access-probe').allow;
   const denied=notify({app:'access-probe',title:'Denied package',sound:false});
   lord.perms.set('access-probe','notifications','allow');
   const granted=notify({app:'access-probe',title:'Allowed package',sound:false,sticky:true});dismissNotification(granted);
   lord.fw.setRestricted('lpk:access-probe',['no-notifications']);
   const blocked=notify({app:'access-probe',title:'Blocked package',sound:false});
   lord.fw.setRestricted('lpk:isolated-probe',['no-notifications']);
   const isolated=notify({app:'isolated-probe',title:'Blocked isolated',sound:false});
   lord.fw.setRestricted('lordos',['no-notifications']);
   const builtin=notify({app:'backup',title:'Blocked builtin',sound:false});
   const exempt=notify({app:'settings',title:'System warning',sound:false,sticky:true});dismissNotification(exempt);
   const linux=notify({app:'linux:probe',title:'Linux namespace',sound:false,sticky:true});dismissNotification(linux);
   lord.fw.setRestricted('lordos',[]);
   configureWindows({apps:[...probeApps,{id:'clock',name:'Clock',package:true,Component:()=>null}]});
   const live=lord.nativeApps.request(JSON.stringify({cmd:'notifications',action:'push',
     notice:{app:'clock',title:'Live permission',ring:true,sticky:true,sound:true}}));
   for(const effect of live.effects||[])notificationBridge.perform(effect);
   const started=ringing.get().includes(live.result);
   await sleep(200);
   lord.perms.set('clock','notifications','deny');
   const cancelled=await until(()=>!ringing.get().includes(live.result),160);
   accessDetails={deniedPrefs,denied,granted,blocked,isolated,builtin,exempt,linux,started,cancelled,jsAccess};
   dismissNotification(live.result);
   configureWindows({apps:[...probeApps,{id:'clock',name:'Clock',Component:()=>null}]});
   await sleep(30); // catalog invalidations are coalesced in a microtask
   const builtinClock=lord.nativeApps.request(JSON.stringify({cmd:'notifications',action:'push',
     notice:{app:'clock',title:'Catalog identity',ring:true,sticky:true,sound:true}}));
   for(const effect of builtinClock.effects||[])notificationBridge.perform(effect);
   const catalogStarted=ringing.get().includes(builtinClock.result);
   configureWindows({apps:[...probeApps,{id:'clock',name:'Clock',package:true,Component:()=>null}]});
   const catalogCancelled=await until(()=>!ringing.get().includes(builtinClock.result));
   nativeCatalog=catalogStarted&&catalogCancelled;
   dismissNotification(builtinClock.result);lord.perms.set('clock','notifications','ask');
   nativeAccess=deniedPrefs&&denied===0&&granted>0&&blocked===0&&isolated===0&&builtin===0&&exempt>0&&linux>0&&started&&cancelled&&jsAccess===0;
   const setPermission=perms.set,setNative=lord.perms.set;let jsWrites=0;
   perms.set=()=>{jsWrites++;return false;};lord.perms.set=()=>{jsWrites++;return false;};
   try{
     const revision=permsRev.get();
     const off=permsRev.subscribe(()=>settings.set(s=>({...s,notifications:{...s.notifications,duringPermissionChange:'kept'}})));
     setAppNotifyPrefs('access-probe',{allow:false});
     off();
     const deniedWrite=lord.perms.get('access-probe','notifications')==='deny'&&permsRev.get()===revision+1&&settings.get().notifications.duringPermissionChange==='kept';
     setAppNotifyPrefs('access-probe',{allow:true});
     const grantedWrite=lord.perms.get('access-probe','notifications')==='allow'&&permsRev.get()===revision+2;
     setAppNotifyPrefs('access-probe',{sound:false});
     const quietWrite=permsRev.get()===revision+2&&settings.get().notifications.apps['access-probe'].sound===false;
     const invalidNative=setNative.call(lord.perms,'INVALID','notifications','deny')===false&&lord.perms.get('INVALID','notifications')==='ask';
     writeDetails={deniedWrite,grantedWrite,quietWrite,invalidNative,jsWrites};
     nativeWrite=deniedWrite&&grantedWrite&&quietWrite&&invalidNative&&jsWrites===0;
   }finally{perms.set=setPermission;lord.perms.set=setNative;}
 }finally{configureWindows({apps:probeApps});perms.get=getPermission;lord.fw.blocks=getBlocked;for(const id of ['lpk:access-probe','lpk:isolated-probe','lordos'])lord.fw.setRestricted(id,[]);}
 const play=sound.play,repeat=globalThis.setInterval;let jsSounds=0;sound.play=()=>{jsSounds++;};
 let ringGuard=false,nativeRing=false,ordinary=0;
 globalThis.setInterval=()=>{throw Error('alarm armed a JS ringing interval');};
 try{
   const fire=async()=>{clockRequest('timerStart',{seconds:1});await until(()=>clockRequest('snapshot').timer.state==='idle');await sleep(60);};
   settings.set(s=>({...s,notifications:{...s.notifications,dnd:true}}));await fire();
   const quietDnd=ringing.get().length===0&&jsSounds===0;
   settings.set(s=>({...s,notifications:{...s.notifications,dnd:false,apps:{clock:{sound:false}}}}));await fire();
   const quietMuted=ringing.get().length===0&&jsSounds===0;
   for(const n of notifications.get())dismissNotification(n.id);
   settings.set(s=>({...s,notifications:{...s.notifications,apps:{}}}));await fire();
   const armed=ringing.get().length===1&&jsSounds===0;
   toggle(false);await until(()=>!bannerHandle);await sleep(700);toggle(true);await until(()=>bannerHandle);
   await sleep(2000);nativeRing=ringing.get().length===1&&jsSounds===0;
   for(let i=0;i<4;i++)notify({title:'Eviction '+i,sticky:true,sound:false});
   await sleep(2700);
   const evicted=ringing.get().length===0&&jsSounds===0;
   await fire();const restarted=ringing.get().length===1&&jsSounds===0;
   settings.set(s=>({...s,notifications:{...s.notifications,apps:{clock:{sound:false}}}}));
   const mutedWhileRinging=await until(()=>ringing.get().length===0)&&jsSounds===0;
   ringGuard=quietDnd&&quietMuted&&armed&&evicted&&restarted&&mutedWhileRinging;
   for(const n of notifications.get())dismissNotification(n.id);
   ordinary=notify({title:'Native audio',sound:true,sticky:true});dismissNotification(ordinary);
 }finally{sound.play=play;globalThis.setInterval=repeat;}
 const nativeSound=ordinary>0&&jsSounds===0&&noSoundEffects;
 // Mutate native placement and direction while their compatibility mirrors stay stale.
 setBannerPlacement('top');locale.set({lang:'en',region:'',dir:'ltr'});
 const route=notificationBridge.perform,localeRoute=localeBridge.perform;
 notificationBridge.perform=effect=>{if(effect.kind!=='notificationState')route(effect);};
 localeBridge.perform=()=>{};
 request.call(lord.nativeApps,JSON.stringify({cmd:'notifications',action:'placement',placement:'bottom'}));
 lord.locale.state({cmd:'patch',value:{lang:'en',region:'',dir:'rtl'}});
 await sleep(80);
 let nativePlacement=notificationPlacement.get()==='top'&&direction()==='ltr';
 request.call(lord.nativeApps,JSON.stringify({cmd:'notifications',action:'push',notice:{label:'Capture',title:'Final banner',body:'Notification body',sticky:true,sound:false,actions:[{id:'open',label:'Open',primary:true}]}}));await until(()=>find('Final banner'));await sleep(200);
 const appearanceUpdate=await changeAppearance();await sleep(60);
 const appearanceRetained=!!find('Final banner')&&!!find('Open');
 const finalBounds=lord.getLayout(find('Final banner'));
 const clickThrough=lord.nativeViews.hitTest(5,350)===background.current?.id;
 const handle=lord.media.screenshot('screenshots');const saved=await media.saveImage(handle,@IMAGE@);media.close(handle);
 nativePlacement&&=notificationPlacement.get()==='top'&&direction()==='ltr';
 notificationBridge.perform=route;localeBridge.perform=localeRoute;
 console.log('notifications-observation '+JSON.stringify({noAppearanceFacts,nativePlacement,clickThrough,...appearanceUpdate,appearanceRetained,ran,reasons,grouped,oldRevealed,dndSuppressed,timerGone,bound,noPayload,noJsDeadline,nativeDirect,mirrorStale,remounted,backgroundExpired,ringGuard,nativeRing,nativeSound,nativePreferences,rawPolicy,nativeAccess,accessDetails,noAccessFacts,nativeWrite,writeDetails,revisionOrder,residentPreferences,nativeCatalog,nativePresentation,noPresentationFacts,nativeLocaleSource,nativePackReload,nativeBootstrap,jsLocaleReads,noLocaleFacts,finalBounds,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(e=>console.error('notifications driver: '+(e.stack||e))),200);
"#;
