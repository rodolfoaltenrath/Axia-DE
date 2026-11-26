//! The compiled Mail in the real compositor, in front of a pretend helper in place of
//! `lord.mail`: the list and a sanitized message are drawn, remote images load only when asked,
//! a link shows its address, and a new message is written with the keyboard and the pointer and
//! sent. The captures prove the three panes were painted in both modes.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn mail_reads_loads_images_and_writes_a_message() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [820, 1000] {
            let report = ui::run_ui_with_capture_timeout(repo, "mail", DRIVER, mode, width, "mail.png", 40, |path| {
                let capture = image::open(path).unwrap().into_rgb8();
                assert_eq!((capture.width(), capture.height()), (width, 700));
                // the selected row of the list is painted with the accent color
                let accent = capture
                    .enumerate_pixels()
                    .filter(|(x, y, p)| (200..520).contains(x) && *y > 120 && p.0[2] > 190 && p.0[0] < 90 && p.0[1] > 80 && p.0[1] < 180)
                    .count();
                assert!(accent > 8_000, "{mode}/{width}: the selected message was not painted ({accent})");
                if let Some(output) = std::env::var_os("AXIA_MAIL_OUTPUT") {
                    let out = std::path::PathBuf::from(output).join(format!("{mode}-{width}"));
                    std::fs::create_dir_all(&out).unwrap();
                    std::fs::copy(path, out.join("mail.png")).unwrap();
                }
            });
            for flag in ["compiled", "listed", "opened", "blocked", "images", "link", "seen", "composing", "recipient", "formatted", "sent", "closed"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
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
import {media} from '@ROOT@/sdk/media.js';
const kind='mail',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let handle=0,stage='bootstrap';
const report={};
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',effects:false});
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const result=mount(node,id,raw,...rest);if(id==='mail'&&!JSON.parse(raw).icon)handle=result;return result;};
// ---- the pretend helper ----
const requests=[],replies=[],bound=[],allowed=[];let sequence=0;
const when=Date.now()-60000;
const item=(uid,from,subject)=>({acct:'me',folder:'INBOX',uid,from,subject,preview:'Numbers for the quarter',date:when,seen:uid>1,labels:uid===2?[4]:[]});
const message={acct:'me',folder:'INBOX',uid:1,subject:'Quarterly report',seen:false,date:when,msgid:'<1@example.org>',
 from:[{name:'Ann Lee',email:'ann@example.org'}],to:[{name:'',email:'me@example.org'}],text:'Numbers attached.',
 atts:[{idx:0,name:'report.pdf',size:20480,type:'application/pdf'}],
 html:{blocks:[{t:'p',c:[{t:'x',x:'The numbers are ready. '},{t:'x',x:'Details',l:0}]},{t:'img',src:'r:1',w:120,h:60,alt:'chart'}],
  links:[{h:'https://example.org/report',w:0,txt:'Details'}],remote:['https://img.example.org/chart.png'],stats:{remote:1,trackers:0}}};
function answer(request){switch(request.op){
 case 'folders':return{accounts:[{id:'me',name:'Example',status:'idle',folders:[{name:'INBOX',special:'inbox',display:'Inbox',unread:1,total:3},{name:'Sent',special:'sent',display:'Sent',unread:0,total:2},{name:'Projects',special:'',display:'Projects',unread:0,total:0}]}]};
 case 'list':return{version:1,total:3,items:[item(1,'Ann Lee','Quarterly report'),item(2,'Bo','Lunch on Friday'),item(3,'Carla','Minutes')]};
 case 'open':return message;
 case 'image':return{image:91,w:240,h:120};
 case 'autocomplete':return{items:[]};
 case 'draft':return{uid:5};
 default:return{};}}
const config={accounts:[{id:'me',kind:'mail',name:'Example',email:'me@example.org',user:'me@example.org',signature:'Axia probe',imap:{host:'imap.example.org',port:993},smtp:{host:'smtp.example.org',port:465}}]};
lord.mail={available:()=>true,start:()=>true,config:()=>JSON.stringify(config),saveConfig:()=>true,cacheDir:()=>lord.fs.home()+'/.cache/axia-mail',
 allowImages:list=>{allowed.push(...list);},bind:(node,image)=>{bound.push(image);},release:()=>{},
 request(text){const request=JSON.parse(text);requests.push(request);const id=++sequence;replies.push({type:'ok',id,data:answer(request)});return id;},
 poll:()=>replies.splice(0),pending:()=>replies.length};
const asked=op=>requests.filter(r=>r.op===op);
// ---- the windows: the newest one is shown ----
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='mail');
configureWindows({apps:[nativeApp({...manifest,process:undefined})],workArea:{x:0,y:0,w:@WIDTH@,h:700}});
wm.open('mail',{});
let shown='';
function Harness(){const list=useWindows().windows;shown=list.length?list[list.length-1].id:'';return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},shown?h(AppHost,{key:shown,id:shown,focused:true}):null));}
render(h(Harness));
const text=()=>lord.nativeViews.text(handle)||'';
async function until(fn,label,steps=250){stage=label;for(let i=0;i<steps&&!fn();i++)await sleep(20);if(!fn())throw Error('timeout: '+label+'; '+text()+'; '+JSON.stringify(requests.slice(-6)));return true;}
const key=(name,char='',mods=0)=>{__lordKey(0,1,name,char,char||name,mods);__lordKey(0,0,name,'',char||name,mods);__lordNativeView();};
const type=word=>{for(const ch of word)key(ch,ch);};
function click(label){const node=lord.nativeViews.findText(handle,label);if(!node)throw Error('no text node: '+label+'; '+text());const [x,y,w,h_]=lord.getLayout(node),cx=x+w/2,cy=y+h_/2;
 for(const phase of ['down','up']){const target=lord.nativeViews.pointer(phase,node,cx,cy,0);__lordEvent(phase,target,cx,cy,0);}__lordNativeView();}
async function capture(){await sleep(250);const image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@);media.close(image);return!!result?.ok;}
async function run(){
 report.compiled=manifest?.nativeView==='rust'&&manifest?.process==='isolated';
 report.listed=await until(()=>text().includes('Lunch on Friday')&&text().includes('Projects'),'list and folders');
 click('Quarterly report');
 report.opened=await until(()=>text().includes('ann@example.org')&&text().includes('report.pdf')&&text().includes('Details'),'the message opens');
 report.blocked=await until(()=>text().includes('Load Images'),'remote images wait')&&asked('image').length===0&&allowed.length===0;
 click('Load Images');
 report.images=await until(()=>bound.includes(91),'the image is bound')&&allowed[0]==='https://img.example.org/chart.png';
 click('Details');
 report.link=await until(()=>text().includes('https://example.org/report'),'the link sheet');
 key('Escape');await until(()=>!text().includes('https://example.org/report'),'the sheet closes');
 report.seen=await until(()=>asked('flag').some(r=>r.add?.[0]==='seen'),'marked as read');
 const painted=await capture();
 const first=handle,main=shown;
 key('n','n',16);
 report.composing=await until(()=>shown!==main&&handle!==first&&text().includes('Axia probe'),'a compose window');
 await sleep(300);
 type('bo@example.org,');
 report.recipient=await until(()=>text().includes('bo@example.org'),'a recipient chip');
 await sleep(200);
 click('Axia probe');await sleep(100);
 key('Home','',2);type('Hello bo');
 await until(()=>text().includes('Hello bo'),'typing in the body');
 key('ArrowLeft','',1);key('ArrowLeft','',1);await sleep(100);
 click('B');
 report.formatted=await until(()=>text().includes('Hello **bo**'),'Shift selects and the button formats');
 key('Enter','',2);
 report.sent=await until(()=>asked('send').length===1,'sent')&&asked('send')[0].to[0].email==='bo@example.org'&&asked('send')[0].html.includes('<b>bo</b>')&&asked('send')[0].text.startsWith('Hello *bo*');
 report.closed=await until(()=>shown===main,'the compose window closes');
 console.log(kind+'-observation '+JSON.stringify({...report,painted}));
}
setTimeout(()=>run().catch(async error=>{await capture();console.log(kind+'-observation '+JSON.stringify({...report,error:String(error),stage,text:text()}));}),200);
"#;
