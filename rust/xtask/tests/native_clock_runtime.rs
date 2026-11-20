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
                "opened",
                "hidden",
                "alarm",
                "noJsTimers",
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
import {ClockSession,startAlarms,clockTimer,arm} from '@ROOT@/sdk/alarms.js';
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
let scheduler=0,status=0,bound=true,noPayload=true,remount,noJsTimers=true,completions=0;
const mount=lord.nativeViews.mount,send=lord.nativeViews.facts;
function inspect(text){const facts=JSON.parse(text);for(const key of ['clock','timer','stopwatch','nextAlarm','config'])noPayload&&=!(key in facts)&&!(key in(facts.args||{}));}
lord.nativeViews.mount=(node,id,text,...sources)=>{
 if(id==='shell:clock'||id==='shell:clock-status'){inspect(text);bound&&=sources[0]===lord.windowList;const handle=mount(node,id,text,...sources);if(id==='shell:clock')scheduler=handle;else status=handle;return handle;}
 return mount(node,id,text,...sources);
};
lord.nativeViews.facts=(handle,text)=>{if(handle===scheduler||handle===status)inspect(text);return send(handle,text);};
locale.set({lang:'en',region:'',dir:'ltr'});
settings.set({clock:{},notifications:{dnd:false,apps:{clock:{sound:false}}},mode:'@MODE@'});
configureWindows({apps:[{id:'clock',name:'Clock',Component:()=>null}]});
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[rev,setRev]=useState(0);remount=()=>setRev(n=>n+1);return h(ThemeProvider,{tokens:theme},
 h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},h(ClockSession,{key:'clock-'+rev}),
 h(View,{style:{position:'absolute',left:30,top:30,height:24,flexDirection:'row'}},h(StatusItems,{key:'status-'+rev,color:theme.text,height:24})),h(NotificationBanner)));}
render(h(App));
const text=()=>status?lord.nativeViews.text(status)||'':'';
const direct=(cmd,extra={})=>lord.clock.request({cmd,now:Date.now(),...extra});
const click=node=>{const[x,y,w,height]=lord.getLayout(node);for(const type of ['down','up']){const target=lord.nativeViews.pointer(type,node,x+w/2,y+height/2,0);__lordEvent(type,target,x+w/2,y+height/2,0);}__lordNativeView();};
async function run(){
 globalThis.setTimeout=(fn,...args)=>{if(fn===arm){noJsTimers=false;throw Error('clock armed a JS timeout');}return timeout(fn,...args);};
 globalThis.setInterval=()=>{noJsTimers=false;throw Error('clock armed a JS interval');};
 try{
  startAlarms();await until(()=>scheduler&&status);
  const perform=clockBridge.perform;clockBridge.perform=()=>{};
  direct('timerStart',{seconds:3});
  const nativeStatus=await until(()=>text().includes('0:03'));
  const mirrorStale=clockTimer.get().state==='idle';
  const countedDown=await until(()=>text().includes('0:02'));
  clockBridge.perform=effect=>{for(const e of effect.reply?.effects||[])if(e.kind==='timer')completions++;perform(effect);};
  const previous=scheduler;remount();const remounted=await until(()=>scheduler!==previous&&text().includes(':'));
  const completed=await until(()=>notifyHistory.get().some(n=>n.app==='clock'))&&clockTimer.get().state==='idle';
  for(const n of notifications.get())dismissNotification(n.id);
  direct('timerStart',{seconds:60});direct('timerPause');await until(()=>text().includes('1:00'));
  click(lord.nativeViews.findText(status,'1:00'));
  const opened=await until(()=>wm.get('clock')?.args?.tab==='timer');
  wm.close('clock');direct('timerReset');const hidden=await until(()=>text().trim()==='');
  settings.set({clock:{alarms:[{id:'probe',on:true,snooze:Date.now()+600000}],hour24:'24'}});
  const alarm=await until(()=>text().includes(':'));
  const source=lord.media.screenshot('screenshots'),saved=await media.saveImage(source,@IMAGE@);media.close(source);
  console.log('native_clock-observation '+JSON.stringify({bound,noPayload,mirrorStale,nativeStatus,countedDown,remounted,completed,opened,hidden,alarm,noJsTimers,completions,saved:!!saved?.ok}));
 }finally{globalThis.setTimeout=timeout;globalThis.setInterval=interval;}
}
timeout(()=>run().catch(error=>console.error('native_clock driver: '+(error.stack||error))),150);
"#;
