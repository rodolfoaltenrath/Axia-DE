// Real React/runtime lifecycle with the WM's string IDs. Only SDK I/O and the
// native scene endpoint are substituted; the production async mount is bundled.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
const root=fileURLToPath(new URL('../../',import.meta.url));
let node=0;
globalThis.lord={screen:{width:960,height:620},version:'test',now:()=>0,stats:()=>({}),getLayout:()=>[0,0,960,620],
  createNode:()=>++node,setStyle(){},setProp(){},setText(){},appendChild(){},insertBefore(){},removeChild(){},destroy(){},setRoot(){},
  css:text=>JSON.parse(text),fonts:{list:()=>[]}};
globalThis.__nativeMountRecords=[];
globalThis.__nativePickerRecords=[];
globalThis.__nativeBluetoothRecords=[];
const btListeners=new Set();let btState={devices:[]};
globalThis.__btFixture={get:()=>btState,subscribe:fn=>{btListeners.add(fn);return()=>btListeners.delete(fn);}};
const diskListeners=new Set();let diskState=[{name:'sdb',model:'First USB'}],diskReads=0;
globalThis.__nativeDiskRecords=[];
globalThis.__diskFixture={available:()=>true,list:()=>{diskReads++;return diskState;},tools:()=>({}),watch:fn=>{diskListeners.add(fn);return()=>diskListeners.delete(fn);}};
const stubs={
  'rustfiles.js':'export function RustFileDialog(props){globalThis.__nativePickerRecords.push(props);return null;}',
  'passwords-sdk':`export const View=()=>null,NativeView=()=>null,useAppWindow=()=>({}),useStore=()=>({}),locale={},wm={},vaultAvailable=()=>false,vaultRequest=()=>null,onVaultEvent=()=>()=>{},requestPermission=()=>false,clipboard={};`,
  'widgets.js':'export const registerWidgetType=()=>{};export const setWidgets=()=>{};',
  'ime.js':'export const imeState={get:()=>({}),subscribe:()=>()=>{}};',
  'textfocus.js':'export const textFieldActive=()=>false;',
  'nativeapps.js':'export const nativeApp = manifest => ({...manifest});',
  'components.jsx':`import {useContext} from 'react'; export const useAppWindow=()=>useContext(globalThis.__nativeWindowContext);`,
  'nativeview.js':`import React from 'react'; export function NativeView({args}) {
    const win=React.useContext(globalThis.__nativeWindowContext);
    React.useLayoutEffect(()=>{if(args.__nativeBluetooth)globalThis.__nativeBluetoothRecords.push(args.__nativeBluetooth);},[args.__nativeBluetooth]);
    React.useLayoutEffect(()=>{if(args.__nativeDisks)globalThis.__nativeDiskRecords.push(args.__nativeDisks);},[args.__nativeDisks]);
    React.useLayoutEffect(()=>{const token=lord.nativeAsync.start(args.__nativeOwner,'logs.sources');
      globalThis.__nativeMountRecords.push({id:win.id,owner:args.__nativeOwner,token});},[]);
    return React.createElement('view');
  }`,
  'logs.js':`export const logs={sources:()=>({user:1,system:false}),files:()=>[],query:()=>[],system:()=>[],tail:()=>'',follow(){},onChange:()=>()=>{}};`,
  'fs.js':['write','stat','mkdir','copy','jobs','forgetJob','trash','list','rename','volumes','home'].map(name=>`export const ${name}=()=>null;`).join('\n'),
  'doc.js':['fontInfo','fontSample','fontGrid','releaseImage','decodeImage','convertImage','pdfThumb'].map(name=>`export const ${name}=()=>null;`).join('\n'),
  'print.js':`export const print={discover:()=>({scanners:[]}),scanCaps:()=>({}),scan:()=>({}),open:()=>null};`,
  'media.js':`export const media={};export const userDirs=()=>({music:'/home/test/Music',pictures:'/home/test/Pictures'});export const thumbRev={subscribe:()=>()=>{}};`,
  'settings.js':`export const settings={get:()=>({}),set(){}};export const useSettings=()=>[{},()=>{}];`,
  'sound.js':`export const soundState={get:()=>({sources:[]}),subscribe:()=>()=>{}};`,
  'backup.js':`export const backup={};export const backupStatus={get:()=>null,subscribe:()=>()=>{}};`,
  'windows.js':`export const wm={open:()=>null};export const workArea=()=>({x:0,y:0,w:960,h:620});export const windows={get:()=>[],subscribe:()=>()=>{}};`,
  'system.js':`export const systemInfo=()=>({admin:false});`,
  'codesig.js':`export const codeStatus=()=>({});`,
  'admin.js':`export const admin={available:()=>false};`,
  'power.jsx':`export const canPower=()=>false,askPower=()=>false;`,
  'hardware.jsx':`export const bluetooth=globalThis.__btFixture;`,
  'apps.js':`export const launchApp=()=>false;`,
  'disks.js':`export const disks=globalThis.__diskFixture;`,
  'smart.js':`export const smart={},smartSummaries={set(){}};`,
  'packages.js':`export const isolatedApp=(id,App)=>App;`,
  'mail.js':`export const mailRequest=()=>null;export const onMailEvent=()=>()=>{};`,
  'native_system_services.js':`export const systemServices=()=>({});`,
  'native_capture.js':`export const captureApp=app=>app;`,
  'pdf.js':`export const pdf={open:async()=>null};`,
  'vault.js':`export const vaultAvailable=()=>false,vaultRequest=()=>null,onVaultEvent=()=>()=>{};`,
  'privacy.js':`export const requestPermission=()=>false;`,
  'clipboard.js':`export const clipboard={set(){}};`,
  'i18n.js':`export const locale={get:()=>({region:'BR'}),subscribe:()=>()=>{}};`,
};
const result=await build({stdin:{resolveDir:root,contents:`
  import React from 'react'; import {render} from '@lordos/runtime';
  import {nativeApp} from './runtime/nativeapps.js';
  const Context=React.createContext(null);globalThis.__nativeWindowContext=Context;
  const App=nativeApp({id:'logs',asyncServices:['logs.sources','files.pick']}).Component;
  const Sysinfo=nativeApp({id:'sysinfo',asyncServices:['logs.sources']}).Component;
  const Disks=nativeApp({id:'disks',asyncServices:['logs.sources']}).Component;
  let update;
  function Root(){const [ids,set]=React.useState([]);update=set;return React.createElement(React.Fragment,null,...ids.map(id=>React.createElement(Context.Provider,{key:id,value:{id,args:{}}},React.createElement(id==='sysinfo'?Sysinfo:id==='disks'?Disks:App))));}
  render(React.createElement(Root));
  const flush=async()=>{for(let i=0;i<20;i++)await new Promise(resolve=>setImmediate(resolve));};
  export async function windows(ids){await flush();update(ids);await flush();}
`},bundle:true,write:false,format:'esm',platform:'node',define:{'process.env.NODE_ENV':'"production"'},alias:{'@lordos/runtime':path.join(root,'runtime/index.js')},plugins:[{
  name:'native-endpoints',setup(b){
    b.onResolve({filter:/^@lordos\/sdk$/},()=>({path:'passwords-sdk',namespace:'fake-sdk'}));
    b.onResolve({filter:/native_system_services\.js$/},()=>({path:'native_system_services.js',namespace:'fake-sdk'}));
    b.onResolve({filter:/native_capture\.js$/},()=>({path:'native_capture.js',namespace:'fake-sdk'}));
    b.onResolve({filter:/sdk\/(disks\.js|smart\.js|system\.js|codesig\.js|admin\.js|power\.jsx|hardware\.jsx|apps\.js|packages\.js|mail\.js|rustfiles\.js|widgets\.js|ime\.js|textfocus\.js|nativeapps\.js|nativeview\.js|components\.jsx|logs\.js|fs\.js|doc\.js|print\.js|media\.js|sound\.js|settings\.js|backup\.js|windows\.js|pdf\.js|vault\.js|privacy\.js|clipboard\.js|i18n\.js)$/},a=>({path:path.basename(a.path),namespace:'fake-sdk'}));
    b.onLoad({filter:/.*/,namespace:'fake-sdk'},a=>({contents:stubs[a.path],resolveDir:root,loader:'js'}));
  },
}]});
const {windows}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
await windows(['logs','logs#2']);
const [first,second]=globalThis.__nativeMountRecords;
assert.equal(btListeners.size,0,'unrelated windows do not subscribe to Bluetooth');
assert.equal(diskReads,0,'unrelated windows do not collect disks');
assert.equal(first.id,'logs');assert.equal(second.id,'logs#2');
assert.ok(Number.isSafeInteger(first.owner)&&first.owner>0);assert.notEqual(first.owner,second.owner);
assert.equal(lord.nativeAsync.take(first.owner,[first.token])[0].value.user,1);
const options={title:'Choose wallpaper',extensions:['png','jpg','jpeg']};
const picked=lord.nativeAsync.start(first.owner,'files.pick',[options]);
await windows(['logs','logs#2']);
const picker=globalThis.__nativePickerRecords.at(-1);
assert.ok(picker,'files.pick must mount a dialog for its window');
assert.equal(picker.mode,'open','wallpapers need a picker, not the browser default');
for(const name of ['photo.png','photo.JPG','photo.jpeg']) assert.ok(picker.filter.test(name),name);
for(const name of ['photo.txt','photoXpng','photo.png.exe']) assert.ok(!picker.filter.test(name),name);
picker.onPick('/home/test/Pictures/photo.png');
await windows(['logs','logs#2']);
assert.equal(lord.nativeAsync.take(first.owner,[picked])[0].value,'/home/test/Pictures/photo.png');
const cancelled=lord.nativeAsync.start(first.owner,'files.pick',[options]);
await windows(['logs','logs#2']);
globalThis.__nativePickerRecords.at(-1).onClose();
await windows(['logs','logs#2']);
assert.equal(lord.nativeAsync.take(first.owner,[cancelled])[0].value,null);

await windows(['logs#2']);
assert.throws(()=>lord.nativeAsync.start(first.owner,'logs.sources'),/invalidRequest/);
const alive=lord.nativeAsync.start(second.owner,'logs.sources');
await windows([]);
assert.deepEqual(lord.nativeAsync.take(second.owner,[alive]),[]);
await windows(['logs']);
const reopened=globalThis.__nativeMountRecords.at(-1);assert.notEqual(reopened.owner,first.owner);
await windows([]);
await windows(['sysinfo']);assert.equal(btListeners.size,1);
btState={devices:[{name:'live Bluetooth update'}]};for(const fn of btListeners)fn();
await windows(['sysinfo']);assert.equal(globalThis.__nativeBluetoothRecords.at(-1).devices[0].name,'live Bluetooth update');
await windows([]);assert.equal(btListeners.size,0,'closing Sysinfo releases its hardware subscription');
await windows(['disks']);assert.equal(diskListeners.size,1);
diskState=[{name:'sdb',model:'Replacement USB'}];for(const fn of diskListeners)fn();
await windows(['disks']);assert.equal(globalThis.__nativeDiskRecords.at(-1)[0].model,'Replacement USB');
await windows([]);assert.equal(diskListeners.size,0,'closing Disks releases its event subscription');
console.log('native-async-mount-test: picker selection/cancel, leases and window lifecycle passed');
