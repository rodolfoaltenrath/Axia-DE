import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createFactsStore, createFactsEncoder, nativeTheme } from '../../sdk/nativeviewfacts.js';
import { build } from 'esbuild';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../../', import.meta.url));
function store(value) {
  const listeners = new Set();
  return {get:()=>value, set(next){value=next;for(const fn of listeners)fn();},subscribe(fn){listeners.add(fn);return()=>listeners.delete(fn);},count:()=>listeners.size};
}
const source=store({id:1,x:0}); let prepared=0, notified=0;
const derived=createFactsStore([source],()=>[source.get().id],()=>{prepared++;return {id:source.get().id};});
assert.equal(source.count(),0);
const initial=derived.get(), stop=derived.subscribe(()=>notified++);
source.set({id:1,x:20}); await Promise.resolve();
assert.equal(derived.get(),initial); assert.equal(prepared,1); assert.equal(notified,0);
source.set({id:2,x:20}); source.set({id:3,x:20});
assert.equal(derived.get().id,3); // A React render may read before the notification microtask.
await Promise.resolve(); assert.equal(notified,1); assert.equal(prepared,2);
stop(); assert.equal(source.count(),0);
source.set({id:4}); assert.equal(derived.get().id,4);
const stopAgain=derived.subscribe(()=>notified++);source.set({id:5});await Promise.resolve();assert.equal(notified,2);stopAgain();
const encoder=createFactsEncoder();let serializations=0;
const value={items:[1,2],toJSON(){serializations++;return this.items;}};
assert.equal(encoder.block('catalog',value),'[1,2]');
encoder.block('catalog',value);createFactsEncoder().block('catalog',value);
assert.equal(serializations,1); assert.equal(encoder.revision('catalog'),1);
encoder.block('focus',true);encoder.block('focus',false);
assert.equal(encoder.revision('focus'),2);assert.equal(encoder.revision('catalog'),1);
const theme={text:'#fff',wallpapers:['large'],accents:['red'],extra:'token'};
assert.equal(nativeTheme(theme,{}),nativeTheme(theme,{}));
assert.equal(nativeTheme(theme,{}).wallpapers,undefined);
assert.equal(nativeTheme(theme,{fullTheme:true}).extra,'token');
assert.deepEqual(nativeTheme(theme,{themeCatalogs:true}).wallpapers,['large']);

// Bundle the production bridge and the real React renderer. Only SDK data/I/O
// is substituted. Count fact-block preparation independently from IPC sends.
const F=globalThis.__factsFixture={blocks:0, prepared:0, mounts:[], sends:[], focus:[], unmounts:[], timers:new Map(), service:1,
  windows:store([{id:'a',app:'notes',title:'A',x:0,args:{}}]), registry:store(1),
  catalog:store({apps:[{id:'notes',name:'Notes'}]}), locale:store({lang:'en',region:'US',dir:'ltr',revision:0}),
  settings:store({fontSize:13}), clock:store({now:1})};
let node=0, handle=0;
globalThis.lord={screen:{width:960,height:620},version:'test',now:()=>0,stats:()=>({}),getLayout:()=>[0,0,960,620],
  createNode:()=>++node,setStyle(){},setProp(){},setText(){},appendChild(){},insertBefore(){},removeChild(){},destroy(){},setRoot(){},
  css:text=>JSON.parse(text),fonts:{list:()=>[]},probe:{read:()=>({value:F.service})},nativeViews:{
    mount(node,id,text){const h=++handle;F.mounts.push({h,id,facts:JSON.parse(text)});return h;},
    facts(h,text){F.sends.push({h,facts:JSON.parse(text)});},take:()=>null,action(){},result(){},focus(h,on){F.focus.push({h,on});},unmount(h){F.unmounts.push(h);}}};
const stubs={
  'layers.js':`export const LAYERS={};export const shellLayers=()=>false;`,
  'nativefiles.js':`export const filesEffect=()=>{};`, 'nativemenubar.js':`export const barEffect=()=>{};`,
  'nativelauncher.js':`export const launcherBridge={};`, 'nativetree.js':`export const treeOwners=new Map();`,
  'textfocus.js':`export const textFieldActive=()=>false;export const onTextField=()=>()=>{};`,
  'components.jsx':`import {useContext} from 'react';export const useAppWindow=()=>useContext(globalThis.__factsFixture.Window);`,
  'controls.jsx':`export const typeScale=1;`,
  'theme.js':`import {useContext} from 'react';export const useTheme=()=>useContext(globalThis.__factsFixture.Theme);`,
  'i18n.js':`import {useSyncExternalStore} from 'react';const F=globalThis.__factsFixture;export const locale=F.locale;export const useLocale=()=>useSyncExternalStore(locale.subscribe,locale.get);export const formatNumber=n=>n===1.5?'1.5':'1,000';export const appName=a=>a.name;`,
  'windows.js':`const F=globalThis.__factsFixture;export const windows=F.windows, appRegistry=F.registry, wm={};export const appCatalogSource=()=>F.catalog.get().apps;`,
  'apps.js':`const F=globalThis.__factsFixture;export const launcherCatalog=F.catalog;export const launchApp=()=>{};export const appIconFacts=a=>({id:a.id});`,
  'packages.js':`export const refreshPackages=()=>{};`,
  'settings.js':`export const settings=globalThis.__factsFixture.settings;`,
  'alarms.js':`export const clockState=globalThis.__factsFixture.clock;export const arm=()=>{};`,
  'appcontrol.js':`const F=globalThis.__factsFixture;export function appControlFacts(){F.prepared++;return F.windows.get().map(w=>({id:w.id,title:w.title}));}export const forceQuit=()=>{};`,
  'nativeappmodel.js':`export const executeNativeEffect=()=>{};`,
  'disks.js':`export const disks={};`,
};
const result=await build({stdin:{resolveDir:root,contents:`
 import React from 'react';import {render} from '@lordos/runtime';import {NativeView} from './sdk/nativeview.js';
 const F=globalThis.__factsFixture;F.Window=React.createContext(null);F.Theme=React.createContext({text:'#fff',mode:'dark'});
 const manifests=[{id:'plain'},{id:'catalog',facts:'apps'},{id:'windows',facts:'windows'},{id:'settings',settings:true},{id:'service',services:{probe:['read']},serviceIntervals:{probe:0}},{id:'system',facts:'system'}];
 let update; function Root(){const [state,set]=React.useState({});update=set;return React.createElement(F.Theme.Provider,{value:state.theme||{text:'#fff',mode:'dark'}},state.closed?null:manifests.map(m=>React.createElement(F.Window.Provider,{key:m.id,value:{id:m.id,focused:state.focusId?state.focusId===m.id:state.focus!==false,mode:state.mode||'window'}},React.createElement(NativeView,{manifest:m,args:{value:state.arg||1}}))));}
 render(React.createElement(Root));export const flush=async()=>{for(let i=0;i<20;i++)await new Promise(resolve=>setImmediate(resolve));};
 export async function change(s){await flush();update(s);await flush();}
`},bundle:true,write:false,format:'esm',platform:'node',define:{'process.env.NODE_ENV':'"production"'},alias:{'@lordos/runtime':path.join(root,'runtime/index.js')},plugins:[{name:'facts-fixture',setup(b){
 b.onResolve({filter:/.*/},a=>{const name=path.basename(a.path);if(stubs[name]&&a.importer.endsWith('/sdk/nativeview.js'))return {path:name,namespace:'fake-sdk'};});
 b.onLoad({filter:/.*/,namespace:'fake-sdk'},a=>({contents:stubs[a.path],resolveDir:root,loader:'js'}));
 b.onLoad({filter:/sdk\/nativeviewfacts\.js$/},async a=>({contents:(await readFile(a.path,'utf8')).replace('block(name, value) {','block(name, value) { globalThis.__factsFixture.blocks++;'),loader:'js'}));
}}]});
const originalInterval=globalThis.setInterval,originalClear=globalThis.clearInterval;
globalThis.setInterval=fn=>{const id=Symbol();F.timers.set(id,fn);return id;};globalThis.clearInterval=id=>F.timers.delete(id);
try {
 const {change,flush}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
 await flush();assert.equal(F.mounts.length,6);
 const h=id=>F.mounts.find(m=>m.id===id).h;
 assert.equal(F.windows.count(),1);assert.equal(F.settings.count(),1);assert.equal(F.clock.count(),0);
 let blocks=F.blocks, prepares=F.prepared;
 await change({}); // Fresh contexts, args and theme with equivalent fields.
 assert.equal(F.blocks,blocks);assert.equal(F.sends.length,0);
 F.windows.set([{...F.windows.get()[0],x:200}]);await flush();
 assert.equal(F.prepared,prepares);assert.equal(F.blocks,blocks);
 F.windows.set([...F.windows.get(),{id:'b',app:'files',title:'B',args:{}}]);await flush();
 assert.deepEqual(F.sends.map(x=>x.h),[h('windows')]);F.sends=[];
 F.settings.set({fontSize:14});await flush();assert.deepEqual(F.sends.map(x=>x.h),[h('settings')]);F.sends=[];
 blocks=F.blocks;F.settings.set({fontSize:14});await flush();assert.equal(F.blocks,blocks);assert.equal(F.sends.length,0);
 F.catalog.set({apps:[{id:'notes',name:'New name'}]});await flush();
 assert.deepEqual(F.sends.map(x=>x.h),[h('catalog')]);F.sends=[];
 blocks=F.blocks;for(const fn of F.timers.values())fn();await flush();assert.equal(F.blocks,blocks);assert.equal(F.sends.length,0);
 F.service=2;for(const fn of F.timers.values())fn();await flush();assert.deepEqual(F.sends.map(x=>x.h),[h('service')]);F.sends=[];
 await change({focus:false,arg:2,mode:'fullscreen'});
 assert.equal(F.sends.length,6);assert.ok(F.sends.every(x=>x.facts.focused===false&&x.facts.args.value===2&&x.facts.context.windowMode==='fullscreen'));F.sends=[];
 await change({focusId:'plain',arg:2,mode:'fullscreen'});
 assert.deepEqual(F.sends.map(x=>x.h),[h('plain')]);F.sends=[];
 await change({focus:true,arg:2,theme:{text:'#111',mode:'light'}});
 assert.equal(F.sends.length,6);assert.ok(F.sends.every(x=>x.facts.focused===true&&x.facts.context.theme.text==='#111'));F.sends=[];
 F.locale.set({lang:'pt',region:'BR',dir:'rtl',revision:1});await flush();
 assert.equal(F.sends.length,6);assert.ok(F.sends.every(x=>x.facts.lang==='pt'&&x.facts.context.direction==='rtl'));F.sends=[];
 lord.screen.width=1100;globalThis.__lordScreen();await flush();assert.deepEqual(F.sends.map(x=>x.h),[h('system')]);F.sends=[];
 await change({closed:true});assert.equal(F.unmounts.length,6);assert.equal(F.windows.count(),0);assert.equal(F.settings.count(),0);assert.equal(F.timers.size,0);assert.equal(F.catalog.count(),0);
 await change({});assert.equal(F.mounts.length,12);assert.equal(F.windows.count(),1);await change({closed:true});
} finally {globalThis.setInterval=originalInterval;globalThis.clearInterval=originalClear;}
console.log('native-view-facts-test: scoped subscriptions, coalescing, serialization, real React lifecycle and cleanup passed');
