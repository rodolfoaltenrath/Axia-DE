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
            let report = ui::run_ui(
                repo,
                "notifications",
                DRIVER,
                mode,
                width,
                "notifications.png",
            );
            assert_eq!(report["ran"], json!(["second", "body"]), "{report}");
            assert_eq!(report["grouped"], true, "{report}");
            assert_eq!(report["oldRevealed"], true, "{report}");
            assert_eq!(report["dndSuppressed"], true, "{report}");
            assert_eq!(report["timerGone"], true, "{report}");
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
import { createElement as h } from 'react';
import { render, View } from '@lordos/runtime';
import { NotificationBanner } from '@ROOT@/sdk/nativebanner.js';
import { notify, notifications, notifyHistory, setBannerPlacement } from '@ROOT@/sdk/notify.js';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { makeTheme } from '@ROOT@/themes/default/tokens.js';
import { settings } from '@ROOT@/sdk/settings.js';
import { locale } from '@ROOT@/sdk/i18n.js';
import { configureWindows } from '@ROOT@/sdk/windows.js';
import { media } from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
async function until(fn){for(let i=0;i<100&&!fn();i++)await sleep(20);return!!fn();}
const ran=[], reasons=[];
const push=(label,title,options={})=>notify({label,title,body:'Notification body',sticky:true,sound:false,onClose:reason=>reasons.push(reason),...options});
const press=node=>{const[x,y,w,hh]=lord.getLayout(node);for(const type of ['down','up']){const mount=lord.nativeViews.pointer(type,node,x+w/2,y+hh/2,0);__lordEvent(type,mount,x+w/2,y+hh/2,0);}globalThis.__lordNativeView();};
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
settings.set({language:'en',mode:'@MODE@',notificationsNativeView:false,notifications:{grouping:'app',dnd:false}});
configureWindows({apps:[],workArea:{x:20,y:28,w:@WIDTH@-40,h:644}});
render(h(ThemeProvider,{tokens:theme},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:theme.windowBg}},h(NotificationBanner))));
async function run(){
 push('Grouped','Grouped old');push('Grouped','Grouped latest');
 await until(()=>find('Grouped latest'));const grouped=!!find('Grouped · +1')&&!find('Grouped old');
 push('Actions','Action primary',{actions:[{id:'first',label:'First',run:()=>ran.push('first')},{id:'second',label:'Second',run:()=>ran.push('second')}]});
 await until(()=>find('Second'));press(find('Second'));await until(()=>!find('Action primary'));
 push('Body','Body action',{actions:[{id:'first',label:'Default action',run:()=>ran.push('body')}]});
 await until(()=>find('Body action'));press(find('Body action'));await until(()=>!find('Body action'));
 press(find('×'));await until(()=>find('Grouped old'));const oldRevealed=!!find('Grouped old')&&!find('Grouped latest');
 press(find('×'));await until(()=>notifications.get().length===0);
 settings.set(s=>({...s,notifications:{...s.notifications,dnd:true}}));
 push('DND','Suppressed');await sleep(50);
 const dndSuppressed=notifications.get().length===0&&notifyHistory.get().some(n=>n.title==='Suppressed');
 settings.set(s=>({...s,notifications:{...s.notifications,dnd:false}}));
 push('Timer','Timed banner',{sticky:false,timeout:1500});await until(()=>find('Timed banner'));await sleep(1700);
 const timerGone=notifications.get().length===0&&!find('Timed banner');
 setBannerPlacement('bottom');locale.set({lang:'en',region:'',dir:'rtl'});
 push('Capture','Final banner',{actions:[{id:'open',label:'Open',primary:true}]});await until(()=>find('Final banner'));await sleep(200);
 const finalBounds=lord.getLayout(find('Final banner'));
 const handle=lord.media.screenshot('screenshots');const saved=await media.saveImage(handle,@IMAGE@);media.close(handle);
 console.log('notifications-observation '+JSON.stringify({ran,reasons,grouped,oldRevealed,dndSuppressed,timerGone,finalBounds,saved:!!saved?.ok}));
}
setTimeout(()=>run().catch(e=>console.error('notifications driver: '+(e.stack||e))),200);
"#;
