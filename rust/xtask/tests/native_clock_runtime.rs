//! Native engine invalidations, expiry and bar rendering with JS timers disabled.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built desktop and npm ci; required by GPU=1 CI"]
fn clock_deadlines_and_status_read_the_resident_engine() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_capture(
                repo,
                "native_clock",
                DRIVER,
                mode,
                width,
                "native-clock.png",
                |path| {
                    let pixels = image::open(path).unwrap().into_rgb8();
                    let ink = if mode == "light" { [29,29,31] } else { [245,245,247] };
                    let mut matching = 0;
                    for y in 30..54 { for x in 30..250 {
                        // The caption is 12px and the clock uses 1px strokes;
                        // include near-full antialias coverage of the native ink.
                        matching += usize::from(pixels.get_pixel(x,y).0.iter().zip(ink)
                            .all(|(actual, expected)| actual.abs_diff(expected) <= 3));
                    } }
                    assert!(matching >= 5, "current native clock indicator ink: {mode}/{width}");
                },
            );
            for flag in [
                "noCommandTimeFacts",
                "nativeCommandTime",
                "noDefaultStateFacts",
                "nativeDefaultState",
                "noPrematureCompletion",
                "explicitTimeQueries",
                "pureQueries",
                "noAppearanceFacts",
                "withoutAppearanceFacts",
                "staleAppearanceMirror",
                "appearanceRetained",
                "bound",
                "noPayload",
                "mirrorStale",
                "nativeStatus",
                "countedDown",
                "remounted",
                "completed",
                "backgroundCompleted",
                "noSchedulerView",
                "opened",
                "hidden",
                "alarm",
                "noJsTimers",
                "nativeActions",
                "nativeTemplates",
                "liveFirewall",
                "nativeConfig",
                "nativeWrite",
                "noConfigTransport",
                "saved",
            ] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
            assert_eq!(report["completions"], 1, "{report}");
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h,useState} from 'react';
import {render,View} from '@lordos/runtime';
import {startAlarms,clockTimer,clockRequest,clockAlarms,arm,nextFire,timerLeft,swElapsed,stopwatch} from '@ROOT@/sdk/alarms.js';
import {StatusItems} from '@ROOT@/sdk/statusitems.js';
import {clockBridge} from '@ROOT@/sdk/nativeclock.js';
import {NotificationBanner} from '@ROOT@/sdk/nativebanner.js';
import {notifyHistory,notifications,dismissNotification} from '@ROOT@/sdk/notify.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const timeout=globalThis.setTimeout,interval=globalThis.setInterval;
const sleep=ms=>new Promise(resolve=>timeout(resolve,ms));
async function until(fn){for(let i=0;i<200&&!fn();i++)await sleep(20);return!!fn();}
let status=0,bound=true,noPayload=true,remount,toggle,noSchedulerView=true,noJsTimers=true,completions=0;
let noCommandTimeFacts=true,noDefaultStateFacts=true,checkingDefaultState=false;
let noConfigTransport=true,noAppearanceFacts=true,statusFacts=0,lastStatusFacts;
const opposite='@MODE@'==='light'?'dark':'light';
const request=lord.nativeApps.request;
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='clockCommand'){noConfigTransport&&=!('config'in input);if(!['nextFire','timerLeft','swElapsed'].includes(input.action))noCommandTimeFacts&&=!('now'in input);if(checkingDefaultState)noDefaultStateFacts&&=!('state'in input)&&!('now'in input);}const reply=request.call(lord.nativeApps,text);if(input.cmd==='clockCommand')noConfigTransport&&=!(reply.effects||[]).some(e=>e.kind==='settings');return reply;};
const completionIds=new Set();
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
function inspect(text){const facts=JSON.parse(text);lastStatusFacts=facts;noAppearanceFacts&&=!('color'in(facts.args||{}))&&!('theme'in(facts.context||{}))&&!('scale'in(facts.context||{}));for(const key of ['clock','timer','stopwatch','nextAlarm','config'])noPayload&&=!(key in facts)&&!(key in(facts.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{
 if(id==='shell:clock')noSchedulerView=false;
 if(id==='shell:clock-status'){inspect(text);bound&&=sources[0]===lord.windowList;const handle=mount(node,id,text,...sources);status=handle;return handle;}
 return mount(node,id,text,...sources);
};
lord.nativeViews.facts=(handle,text)=>{if(handle===status){inspect(text);statusFacts++;}return send(handle,text);};
const unmount=lord.nativeViews.unmount;lord.nativeViews.unmount=handle=>{if(handle===status)status=0;return unmount(handle);};
locale.set({lang:'en',region:'',dir:'ltr'});
settings.set({language:'en',clock:{},notifications:{dnd:false,apps:{clock:{sound:false}}},mode:opposite,accent:'azul',effects:false,nativeAppearance:'axia'});
configureWindows({apps:[{id:'clock',name:'Clock',Component:()=>null}]});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[rev,setRev]=useState(0),[on,setOn]=useState(true);remount=()=>setRev(n=>n+1);toggle=setOn;return h(ThemeProvider,{tokens:theme},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(View,{style:{position:'absolute',left:30,top:30,height:24,flexDirection:'row'}},on?h(StatusItems,{key:'status-'+rev,color:'#ff0000',height:24}):null),on?h(NotificationBanner):null));}
render(h(App));
const text=()=>status?lord.nativeViews.text(status)||'':'';
const direct=(cmd,extra={})=>lord.clock.request({cmd,now:Date.now(),...extra});
const click=node=>{const[x,y,w,height]=lord.getLayout(node);for(const type of ['down','up']){const target=lord.nativeViews.pointer(type,node,x+w/2,y+height/2,0);__lordEvent(type,target,x+w/2,y+height/2,0);}__lordNativeView();};
async function run(){
 globalThis.setTimeout=(fn,...args)=>{if(fn===arm){noJsTimers=false;throw Error('clock armed a JS timeout');}return timeout(fn,...args);};
 globalThis.setInterval=()=>{noJsTimers=false;throw Error('clock armed a JS interval');};
 try{
  startAlarms();await until(()=>status);
  const sessionClock=(action,extra={})=>request.call(lord.nativeApps,JSON.stringify({cmd:'clockCommand',action,...extra}));
  const beforeStart=Date.now(),fromPast=sessionClock('timerStart',{seconds:60,now:0}),afterStart=Date.now();
  const nativeCommandTime=fromPast.timer.endsAt>=beforeStart+60000&&fromPast.timer.endsAt<=afterStart+60000;
  const fromFuture=sessionClock('arm',{now:Number.MAX_SAFE_INTEGER});
  const noPrematureCompletion=fromFuture.timer.state==='running'&&(fromFuture.notificationEffects||[]).length===0;
  const nativePaused=sessionClock('timerPause',{now:0});
  sessionClock('swStart',{now:0});const nativeStopped=sessionClock('swStop',{now:Number.MAX_SAFE_INTEGER});
  clockTimer.set({state:'paused',duration:1,left:1,endsAt:0});stopwatch.set({running:false,start:0,acc:-999,laps:[]});
  checkingDefaultState=true;const implicitLeft=timerLeft(),implicitElapsed=swElapsed();checkingDefaultState=false;
  const nativeDefaultState=implicitLeft===nativePaused.timer.left&&implicitElapsed===nativeStopped.stopwatch.acc;
  clockRequest('timerReset',{now:0});clockRequest('swReset');
  const engineBefore=clockRequest('snapshot');
  const explicitTimeQueries=nextFire({on:true,snooze:2000},1000)===2000
    &&clockRequest('timerLeft',{state:{state:'running',endsAt:10000},now:7500}).value===2500
    &&clockRequest('swElapsed',{state:{running:true,start:1000,acc:50},now:1500}).value===550;
  const engineAfter=clockRequest('snapshot');
  const pureQueries=JSON.stringify([engineBefore.timer,engineBefore.stopwatch])===JSON.stringify([engineAfter.timer,engineAfter.stopwatch]);

  const perform=clockBridge.perform;clockBridge.perform=()=>{};
  direct('timerStart',{seconds:3});
  const nativeStatus=await until(()=>text().includes('0:03'));
  const mirrorStale=clockTimer.get().state==='idle';
  const countedDown=await until(()=>text().includes('0:02'));
  clockBridge.perform=effect=>{for(const e of effect.reply?.notificationEffects||[])if(e.kind==='notificationState')for(const n of e.state.history)if(n.app==='clock'&&!completionIds.has(n.id)){completionIds.add(n.id);completions++;}perform(effect);};
  const previous=status;remount();const remounted=await until(()=>status&&status!==previous&&text().includes(':'));
  toggle(false);const unmounted=await until(()=>!status);
  const completed=await until(()=>notifyHistory.get().some(n=>n.app==='clock'))&&clockTimer.get().state==='idle';
  const backgroundCompleted=unmounted&&completed&&!status;
  toggle(true);await until(()=>status);
  const notice=notifyHistory.get().find(n=>n.app==='clock');
  const nativeTemplates=notice?.title==='Timer done'&&notice.body==='Time is up.'&&notice.actions[1].id==='again'&&typeof notice.actions[1].run!=='function';
  direct('timerStart',{seconds:9});direct('timerPause');
  clockTimer.set({state:'idle',duration:1000,endsAt:0,left:0});
  const acted=lord.nativeApps.request(JSON.stringify({cmd:'notifications',action:'act',id:notice?.id,index:1}));
  const snapshot=direct('snapshot');
  const nativeActions=snapshot.timer.state==='running'&&snapshot.timer.duration===9000&&!acted.effects.some(e=>e.kind==='notificationAction');
  direct('timerReset');
  for(const n of notifications.get())dismissNotification(n.id);
  direct('timerStart',{seconds:60});direct('timerPause');await until(()=>text().includes('1:00'));
  await sleep(200);const settingsPerform=settingsBridge.perform;settingsBridge.perform=()=>{};
  const count=statusFacts;
  lord.settings.state({cmd:'patch',patch:{mode:'@MODE@',accent:'verde',nativeAppearance:'adwaita'}});await sleep(200);
  const withoutAppearanceFacts=statusFacts===count,staleAppearanceMirror=settings.get().mode===opposite;
  send(status,JSON.stringify({...lastStatusFacts,args:{...lastStatusFacts.args,color:'#ff0000'},context:{...lastStatusFacts.context,theme:{menuText:'#ff0000'},scale:{body:200}}}));
  await sleep(80);const appearanceRetained=text().includes('1:00')&&direct('snapshot').timer.state==='paused';
  settingsBridge.perform=settingsPerform;
  click(lord.nativeViews.findText(status,'1:00'));
  const opened=await until(()=>wm.get('clock')?.args?.tab==='timer');
  wm.close('clock');direct('timerReset');const hidden=await until(()=>text().trim()==='');
  const before=notifyHistory.get().length;
  direct('timerStart',{seconds:1});
  lord.fw.setRestricted('lordos',['no-notifications']);
  const expired=await until(()=>direct('snapshot').timer.state==='idle');
  const liveFirewall=expired&&notifyHistory.get().length===before;
  lord.fw.setRestricted('lordos',[]);
  lord.settings.state({cmd:'patch',patch:{clock:{alarms:[{id:'probe',on:true,snooze:Date.now()+600000}],hour24:'24',future:'keep'}}});
  const staleConfig=!settings.get().clock.alarms;
  const alarm=await until(()=>text().includes(':'));
  const nativeConfig=staleConfig&&alarm&&clockAlarms.list().some(a=>a.id==='probe');
  const beforeClock=settings.get().clock;
  clockRequest('cities',{cities:['UTC'],config:{alarms:[]}});
  const resident=lord.settings.state({cmd:'snapshot'});
  const nativeWrite=resident.state.clock.cities[0]==='UTC'&&resident.state.clock.future==='keep'&&resident.state.clock.alarms[0].id==='probe'
    &&settings.get().clock===beforeClock&&resident.dirty;
  const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
  console.log('native_clock-observation '+JSON.stringify({noCommandTimeFacts,nativeCommandTime,noDefaultStateFacts,nativeDefaultState,noPrematureCompletion,explicitTimeQueries,pureQueries,noAppearanceFacts,withoutAppearanceFacts,staleAppearanceMirror,appearanceRetained,bound,noPayload,mirrorStale,nativeStatus,countedDown,remounted,completed,backgroundCompleted,noSchedulerView,opened,hidden,alarm,noJsTimers,nativeActions,nativeTemplates,liveFirewall,nativeConfig,nativeWrite,noConfigTransport,completions,saved:!!saved?.ok}));
 }finally{globalThis.setTimeout=timeout;globalThis.setInterval=interval;}
}
timeout(()=>run().catch(error=>console.error('native_clock driver: '+(error.stack||error))),150);
"#;
