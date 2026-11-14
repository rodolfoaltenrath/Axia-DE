// Generic native transports used by Notes; all note decisions and UI stay in Rust.
import assert from 'node:assert/strict';
import { build } from 'esbuild';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root=fileURLToPath(new URL('../../',import.meta.url));
const timers=[];
const originalInterval=globalThis.setInterval;
const widgets=new Map(), published=[];
globalThis.__nativeWidgets={widgets,published};
globalThis.lord={nativeApps:{create(id){assert.equal(id,'notes');return {request(encoded){const q=JSON.parse(encoded);assert.equal(q.home,'/home/test');return q.revision===3 ? {same:true} : {revision:3,widgets:[{id:'note-1',pinned:true,rect:{left:80,top:90,width:260,height:240}}]};}};}}};
const bundled=await build({stdin:{resolveDir:root,contents:`
export {createNativeAsync} from './runtime/native_async.js';
export {clipboardServices} from './runtime/native_clipboard_services.js';
export {windowServices} from './runtime/native_window_services.js';
export {inputServices} from './runtime/native_input_services.js';
export {startNativeSession} from './runtime/native_session_widgets.js';
`},bundle:true,write:false,format:'esm',platform:'node',plugins:[{name:'sdk-io',setup(b){
  const stubs={
    '@lordos/runtime':`export const View='view';export const screen={width:800,height:600};`,
    'widgets.js':`export const registerWidgetType=(type,component)=>globalThis.__nativeWidgets.widgets.set(type,component);export const setWidgets=(type,items)=>globalThis.__nativeWidgets.published.push({type,items});`,
    'fs.js':`export const home=()=>'/home/test';`,
    'ime.js':`export const imeState={get:()=>({})};`,
    'textfocus.js':`export const textFieldActive=()=>false;`,
  };
  b.onResolve({filter:/^@lordos\/runtime$|sdk\/(widgets|fs|ime|textfocus)\.js$/},a=>({path:a.path.startsWith('@')?a.path:path.basename(a.path),namespace:'io'}));
  b.onLoad({filter:/.*/,namespace:'io'},a=>({contents:stubs[a.path],loader:'js'}));
}}]});
const {createNativeAsync,clipboardServices,windowServices,inputServices,startNativeSession}=await import('data:text/javascript;base64,'+Buffer.from(bundled.outputFiles[0].text).toString('base64'));
const writes=[],contexts=[],menus=[];
let sdkField=false;
const bridge=createNativeAsync({
  ...clipboardServices({clipboard:{state:()=>({kind:'image'}),get:()=>'',imageData:()=>new Uint8Array([1,2,3])},fs:{write:(...args)=>writes.push(args)}}),
  ...windowServices({open:(...args)=>args,setMenus:(...args)=>menus.push(args)},id=>bridge.events(id)),
  ...inputServices({engine:{setContext:on=>contexts.push(on)},state:{get:()=>({preedit:'にほん',candidates:['日本']})},fieldActive:()=>sdkField}),
});
const allowed=['clipboard.read','clipboard.image','windows.menus','windows.events','inputMethod.context','inputMethod.state'];
const flush=async()=>{for(let i=0;i<4;i++)await Promise.resolve();};
async function call(id,method,args=[]){const token=bridge.start(id,method,args);await flush();const [result]=bridge.take(id,[token]);assert.ok(result);assert.equal(result.error,undefined);return result.value;}
bridge.open(1,allowed);bridge.open(2,allowed);
assert.equal((await call(1,'clipboard.read')).kind,'image');
assert.equal(await call(1,'clipboard.image',['/home/test/Notes/n/image-1.png']),true);
assert.equal(writes[0][0],'/home/test/Notes/n/image-1.png');
await call(1,'windows.menus',['notes#2',[{label:'File',items:[{label:'New',action:['new']}]}]]);
assert.equal(menus[0][0],'notes#2');menus[0][1][0].items[0].onClick();
assert.deepEqual(await call(2,'windows.events'),[]);
assert.deepEqual(await call(1,'windows.events'),[{kind:'action',action:['new']}]);
await call(1,'inputMethod.context',[true]);assert.equal((await call(1,'inputMethod.state')).preedit,'にほん');
await call(2,'inputMethod.context',[true]);bridge.close(1);assert.deepEqual(contexts,[true,true]);
sdkField=true;bridge.close(2);assert.deepEqual(contexts,[true,true]);
bridge.open(3,allowed);sdkField=false;await call(3,'inputMethod.context',[true]);bridge.close(3);assert.equal(contexts.at(-1),false);
assert.throws(()=>bridge.start(3,'clipboard.read'),/invalidRequest/);
// Desktop notes must be restored at registration, before any window is mounted.
globalThis.setInterval=(fn,ms)=>{assert.equal(ms,200);timers.push(fn);return timers.length;};
try {
  function Mount(){}
  startNativeSession({id:'notes',sessionWidgets:'note',asyncServices:allowed},Mount);
  await flush();
  assert.equal(timers.length,1);assert.equal(published.length,1);assert.equal(published[0].items[0].pinned,true);
  const Widget=widgets.get('note');const tree=Widget({widget:published[0].items[0]});
  assert.equal(tree.props.children.type,Mount);assert.equal(tree.props.children.props.args.widget,'note-1');assert.equal(tree.props.children.props.manifest.shell,true);
  timers[0]();assert.equal(published.length,1);
  startNativeSession({id:'notes',sessionWidgets:'note'},Mount);assert.equal(timers.length,1);
} finally {globalThis.setInterval=originalInterval;}
console.log('native-notes-test: clipboard I/O, menu routing, input ownership/cleanup and desktop startup passed');
