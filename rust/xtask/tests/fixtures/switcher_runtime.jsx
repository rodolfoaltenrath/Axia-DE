// Observation driver only. Rust verifies every reported behavior.
import {createElement as h} from 'react';
import {render,View,screen} from '@lordos/runtime';
import {configureWindows,registerBuiltinApp,wm,setDesktops,setActiveDesktop} from '@ROOT@/sdk/windows.js';
import {openSwitcher,switcherKey,cancelSwitcher,switcherOpen,AppSwitcher} from '@ROOT@/sdk/appswitcher.jsx';
import {useShortcuts,resetShortcuts} from '@ROOT@/sdk/shortcuts.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {settingsBridge} from '@ROOT@/sdk/nativesettings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {enableShellLayers} from '@ROOT@/sdk/layers.js';
import {media} from '@ROOT@/sdk/media.js';
import {nativeCatalogFacts} from '@ROOT@/sdk/nativecatalog.js';
import {switcherBridge} from '@ROOT@/sdk/nativeswitcher.js';
const c=globalThis.__AXIA_UI_TEST__||{mode:'@MODE@',width:Number('@WIDTH@'),image:@IMAGE@};
const kind=c.kind||'switcher_@APPEARANCE@';
const appearance=kind.endsWith('adwaita')?'adwaita':'axia';
const opposite=c.mode==='light'?'dark':'light';
settings.set({mode:opposite,accent:'azul',effects:true,nativeAppearance:'axia',nativeStyle:appearance,shortcutProfile:'macos'});locale.set({...locale.get(),lang:'en'});
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn){for(let i=0;i<80&&!fn();i++)await sleep(20);return!!fn();}
let noAppearanceFacts=true,appearanceFacts=0,lastAppearanceFacts;
function inspectAppearance(raw){const input=JSON.parse(raw);lastAppearanceFacts=input;noAppearanceFacts&&=!('theme'in(input.context||{}))&&!('scale'in(input.context||{}))&&!('surface'in(input.args||{}))&&!('radius'in(input.args||{}));}
async function changeAppearance(){
 const perform=settingsBridge.perform;settingsBridge.perform=()=>{};const before=appearanceFacts;
 lord.settings.state({cmd:'patch',patch:{mode:c.mode,accent:'verde',effects:false,nativeAppearance:'adwaita'}});await sleep(120);
 const withoutAppearanceFacts=appearanceFacts===before,staleAppearanceMirror=settings.get().mode===opposite;
 facts(handle,JSON.stringify({...lastAppearanceFacts,args:{...lastAppearanceFacts.args,surface:{backgroundColor:'#ff0000'},radius:999},context:{...lastAppearanceFacts.context,theme:{mode:opposite,text:'#ff0000',accent:'#ff0000'},scale:{body:200}}}));
 settingsBridge.perform=perform;
 return {withoutAppearanceFacts,staleAppearanceMirror};
}
const Icon=()=>null;Icon.nativeIcon={kind:'app',id:'notes',name:'Notes'};
const app=(id,name)=>({id,name,Component:()=>null,Icon});
configureWindows({apps:[app('files','Files'),app('notes','Notes'),app('calculator','Calculator'),app('help','Help')]});
const originalDisplays=screen.displays;
screen.displays=originalDisplays.map(display=>({...display,x:10000,y:10000,w:20,h:20}));
const staleDisplayMirror=screen.displays[0].x===10000;
let handle=0,mountNode=0,mounts=0,unmounts=0,bound=false,noWindowTransport=true,noCatalogTransport=true,noDisplayTransport=true;
const mount=lord.nativeViews.mount,facts=lord.nativeViews.facts,unmount=lord.nativeViews.unmount;
function inspect(text){inspectAppearance(text);const input=JSON.parse(text);noDisplayTransport&&=!('display'in(input.args||{}))&&!('displays'in(input.args||{}));noCatalogTransport&&=!('registry'in input)&&!('catalog'in input);noWindowTransport&&=!('windows'in input)&&!('items'in (input.args||{}))&&!('index'in(input.args||{}));}
lord.nativeViews.mount=(node,id,text,...source)=>{const value=mount(node,id,text,...source);if(id==='shell:switcher'){handle=value;mountNode=node;mounts++;bound=source[0]===lord.windowList;inspect(text);}return value;};
lord.nativeViews.facts=(id,text)=>{if(id===handle){inspect(text);appearanceFacts++;}return facts(id,text);};
lord.nativeViews.unmount=id=>{if(id===handle)unmounts++;return unmount(id);};
const has=label=>!!lord.nativeViews.findText(handle,label);
const raw=(op,...args)=>lord.windowList.call(op,JSON.stringify([{},...args]));
const key=(key,shift=false)=>switcherKey({mods:8,down:true,key,shift});
const release=()=>switcherKey({mods:0,down:false,key:'Meta'});
let legacyFocusCalls=0;
const finish=switcherBridge.perform;
switcherBridge.perform=effect=>{const focus=wm.focus;wm.focus=()=>{legacyFocusCalls++;};try{return finish(effect);}finally{wm.focus=focus;}};
enableShellLayers();resetShortcuts();
function Harness(){useShortcuts({grabs:'flow',onSuperTap:()=>{}});return h(ThemeProvider,{tokens:makeTheme({mode:c.mode,accent:'#3489bd',effects:{}})},h(View,{style:{width:c.width,height:700,backgroundColor:c.mode==='dark'?'#222':'#eee'}},h(AppSwitcher)));}
render(h(Harness));
async function run(){
 const files=wm.open('files'),a=wm.open('notes',{instance:'a'}),b=wm.open('notes',{instance:'b'}),calc=wm.open('calculator');
 openSwitcher(8);await until(()=>has('Notes'));const initial=has('Notes');
 const focusBeforeStale=wm.focused();let staleRead=false;
 switcherBridge.perform({nonce:-1,get reply(){staleRead=true;throw Error('stale completion read its snapshot');}});
 const staleCompletionIgnored=!staleRead&&switcherOpen()&&wm.focused()===focusBeforeStale;
 const publishNotes=(name,builtin=false)=>{const metadata=nativeCatalogFacts(true).get();return lord.nativeApps.request(JSON.stringify({cmd:'metadata',...metadata,registry:metadata.registry.map(app=>app.id==='notes'?{...app,name,builtin}:app)}));};
 const beforeCatalogFacts=appearanceFacts;publishNotes('Native Notes');
 const nativeCatalogRevision=await until(()=>has('Native Notes'))&&appearanceFacts===beforeCatalogFacts&&nativeCatalogFacts(true).get().registry.find(app=>app.id==='notes').name==='Notes';
 facts(handle,JSON.stringify({...lastAppearanceFacts,registry:[{id:'notes',name:'Forged Notes',builtin:true}],catalog:[]}));__lordNativeView();await sleep(40);
 const staleCatalogFacts=has('Native Notes')&&!has('Forged Notes');
 publishNotes('Notes');await until(()=>has('Notes'));
 raw('close',b);release();const freshCommit=await until(()=>!switcherOpen()&&wm.focused()===a);
 wm.focus(calc);openSwitcher(8);await until(()=>has('Notes'));key('Tab');await until(()=>has('Files'));const forward=has('Files');key('Tab',true);await until(()=>has('Notes'));
 const cycling=forward&&has('Notes');key('Escape');const cancelled=await until(()=>!switcherOpen()&&wm.focused()===calc);
 openSwitcher(8);release();const fastRelease=await until(()=>!switcherOpen()&&wm.focused()===a);
 wm.focus(calc);raw('open',{app:'files',instance:'native'});const stale=!wm.get('files#native');
 openSwitcher(8);await until(()=>handle&&lord.nativeViews.text(handle));
 const rawTexts=[lord.nativeViews.text(handle)];
 for(let i=0;i<3&&!has('Files');i++){key('Tab');await sleep(40);rawTexts.push(lord.nativeViews.text(handle));}release();
 await until(()=>!switcherOpen());
 const rawFocus=JSON.parse(raw('state')).state.views.front;
 const rawTarget=stale&&rawFocus==='files#native';
 setDesktops([1,2],1);const away=wm.open('help',{desktop:2});setActiveDesktop(1);wm.focus(calc);
 openSwitcher(8);await sleep(60);
 for(let i=0;i<4&&!has('Help');i++){key('Tab');await sleep(30);}
 const desktopLabel=has('Help')&&has('Desktop 2');release();await until(()=>!switcherOpen());
 const desktopState=JSON.parse(raw('state')).state;
 const desktop=desktopLabel&&desktopState.active===2&&desktopState.views.front===away;
 wm.close(away);setDesktops([1],1);wm.focus('files#native');
 registerBuiltinApp({...app('linux','Linux'),builtin:true});
 const game=wm.open('linux',{instance:'777',args:{surface:777,appId:'fullscreen-test'}}),fullscreen=lord.wayland.fullscreen,fullscreenWindow=lord.wayland.fullscreenWindow;
 lord.wayland.fullscreen=()=>!!wm.get(game)&&!wm.get(game).minimized;lord.wayland.fullscreenWindow=()=>777;
 openSwitcher(8);await until(()=>switcherOpen()&&handle&&has('Files'));key('Escape');
 const fullscreenCancel=await until(()=>!switcherOpen()&&wm.focused()===game&&!wm.get(game).minimized);
 openSwitcher(8);await until(()=>has('Files'));release();
 const fullscreenCommit=await until(()=>!switcherOpen()&&wm.focused()==='files#native'&&wm.get(game).minimized);
 lord.wayland.fullscreen=fullscreen;lord.wayland.fullscreenWindow=fullscreenWindow;
 openSwitcher(8);await sleep(60);settings.set({shortcutProfile:'linux'});const profileCancel=await until(()=>!switcherOpen());
 __lordKey(15,1,'Tab','','',16);const dispatched=await until(()=>switcherOpen()&&handle&&lord.nativeViews.text(handle));
 __lordKey(1,1,'Escape','','',16);__lordKey(1,0,'','','',16);__lordKey(15,0,'','','',16);__lordKey(125,0,'Meta','','',0);
 const dispatcher=dispatched&&await until(()=>!switcherOpen());
 wm.focus(calc);openSwitcher(8);await until(()=>has('Notes'));
 publishNotes('Notes',true);const removedGroup=await until(()=>!has('Notes'));release();await until(()=>!switcherOpen());
 const nativeBuiltinPolicy=removedGroup&&JSON.parse(raw('state')).state.views.front!==a;
 publishNotes('Notes');
 for(const id of [files,a,calc,'files#native',game])raw('close',id);
 const apps=Array.from({length:18},(_,i)=>app('probe'+i,'Probe '+i));configureWindows({apps});
 const ids=apps.map(item=>wm.open(item.id));openSwitcher(8,true);await until(()=>has('Probe 0'));
 // Backwards opening selects the tail of MRU: Probe 0. The row must scroll.
 await sleep(200);const appearanceUpdate=await changeAppearance();await sleep(60);
 const appearanceRetained=has('Probe 0');
 const label=lord.nativeViews.findText(handle,'Probe 0'),r=label?lord.getLayout(label):null;
 const viewport=!!r&&r[0]>=0&&r[0]+r[2]<=c.width&&r[1]>=0&&r[1]+r[3]<=700;
 const mountRect=lord.getLayout(mountNode);
 const nativeDisplayGeometry=staleDisplayMirror&&mountRect[2]===0&&mountRect[3]===0&&viewport;
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,c.image);media.close(image);
 // Cell center is above the label; native hit testing chooses the actual icon cell.
 if(r){const x=c.width/2+((Math.min(18,Math.floor((c.width-96)/104))-1)*104/2),y=r[1]-62;
 for(const type of ['down','up']){const owner=lord.nativeViews.pointer(type,lord.nativeViews.hitTest(x,y),x,y,0);__lordEvent(type,owner||0,x,y,0);__lordNativeView();}}
 const pointerCommit=await until(()=>!switcherOpen()&&wm.focused()===ids[0]);
 cancelSwitcher();for(const id of ids)raw('close',id);openSwitcher(8);const empty=await until(()=>!switcherOpen());
 await until(()=>mounts===unmounts);
 console.log(kind+'-observation '+JSON.stringify({noDisplayTransport,nativeDisplayGeometry,nativeFocusCommands:legacyFocusCalls===0,staleCompletionIgnored,noCatalogTransport,nativeCatalogRevision,staleCatalogFacts,nativeBuiltinPolicy,noAppearanceFacts,...appearanceUpdate,appearanceRetained,bound,initial,freshCommit,cycling,cancelled,fastRelease,rawTarget,rawFocus,rawTexts,stale,desktop,fullscreenCancel,fullscreenCommit,profileCancel,dispatcher,viewport,pointerCommit,empty,unmounted:mounts===unmounts,noWindowTransport,saved:!!saved?.ok,mounts,unmounts,rect:r,mountRect,text:lord.nativeViews.text(handle)}));
}
setTimeout(()=>run().catch(error=>console.error(kind+' driver: '+(error.stack||error))),200);
