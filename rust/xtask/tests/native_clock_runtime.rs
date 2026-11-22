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
            let report = ui::run_ui(
                repo,
                "native_clock",
                DRIVER,
                mode,
                width,
                "native-clock.png",
            );
            for flag in [
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
import {startAlarms,clockTimer,clockRequest,clockAlarms,arm} from '@ROOT@/sdk/alarms.js';
import {StatusItems} from '@ROOT@/sdk/statusitems.js';
import {clockBridge} from '@ROOT@/sdk/nativeclock.js';
import {NotificationBanner} from '@ROOT@/sdk/nativebanner.js';
import {notifyHistory,notifications,dismissNotification} from '@ROOT@/sdk/notify.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {configureWindows,wm} from '@ROOT@/sdk/windows.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const timeout=globalThis.setTimeout,interval=globalThis.setInterval;
const sleep=ms=>new Promise(resolve=>timeout(resolve,ms));
async function until(fn){for(let i=0;i<200&&!fn();i++)await sleep(20);return!!fn();}
let status=0,bound=true,noPayload=true,remount,toggle,noSchedulerView=true,noJsTimers=true,completions=0;
let noConfigTransport=true;
const request=lord.nativeApps.request;
lord.nativeApps.request=text=>{const input=JSON.parse(text);if(input.cmd==='clockCommand')noConfigTransport&&=!('config'in input);const reply=request.call(lord.nativeApps,text);if(input.cmd==='clockCommand')noConfigTransport&&=!(reply.effects||[]).some(e=>e.kind==='settings');return reply;};
const completionIds=new Set();
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
function inspect(text){const facts=JSON.parse(text);for(const key of ['clock','timer','stopwatch','nextAlarm','config'])noPayload&&=!(key in facts)&&!(key in(facts.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{
 if(id==='shell:clock')noSchedulerView=false;
 if(id==='shell:clock-status'){inspect(text);bound&&=sources[0]===lord.windowList;const handle=mount(node,id,text,...sources);status=handle;return handle;}
 return mount(node,id,text,...sources);
};
lord.nativeViews.facts=(handle,text)=>{if(handle===status)inspect(text);return send(handle,text);};
const unmount=lord.nativeViews.unmount;lord.nativeViews.unmount=handle=>{if(handle===status)status=0;return unmount(handle);};
locale.set({lang:'en',region:'',dir:'ltr'});
settings.set({language:'en',clock:{},notifications:{dnd:false,apps:{clock:{sound:false}}},mode:'@MODE@'});
configureWindows({apps:[{id:'clock',name:'Clock',Component:()=>null}]});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[rev,setRev]=useState(0),[on,setOn]=useState(true);remount=()=>setRev(n=>n+1);toggle=setOn;return h(ThemeProvider,{tokens:theme},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},
 h(View,{style:{position:'absolute',left:30,top:30,height:24,flexDirection:'row'}},on?h(StatusItems,{key:'status-'+rev,color:theme.text,height:24}):null),on?h(NotificationBanner):null));}
render(h(App));
const text=()=>status?lord.nativeViews.text(status)||'':'';
const direct=(cmd,extra={})=>lord.clock.request({cmd,now:Date.now(),...extra});
const click=node=>{const[x,y,w,height]=lord.getLayout(node);for(const type of ['down','up']){const target=lord.nativeViews.pointer(type,node,x+w/2,y+height/2,0);__lordEvent(type,target,x+w/2,y+height/2,0);}__lordNativeView();};
async function run(){
 globalThis.setTimeout=(fn,...args)=>{if(fn===arm){noJsTimers=false;throw Error('clock armed a JS timeout');}return timeout(fn,...args);};
 globalThis.setInterval=()=>{noJsTimers=false;throw Error('clock armed a JS interval');};
 try{
  startAlarms();await until(()=>status);
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
  console.log('native_clock-observation '+JSON.stringify({bound,noPayload,mirrorStale,nativeStatus,countedDown,remounted,completed,backgroundCompleted,noSchedulerView,opened,hidden,alarm,noJsTimers,nativeActions,nativeTemplates,liveFirewall,nativeConfig,nativeWrite,noConfigTransport,completions,saved:!!saved?.ok}));
 }finally{globalThis.setTimeout=timeout;globalThis.setInterval=interval;}
}
timeout(()=>run().catch(error=>console.error('native_clock driver: '+(error.stack||error))),150);
"#;
