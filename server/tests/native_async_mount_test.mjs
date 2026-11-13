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
const stubs={
  'nativeapps.js':'export const nativeApp = manifest => ({...manifest});',
  'components.jsx':`import {useContext} from 'react'; export const useAppWindow=()=>useContext(globalThis.__nativeWindowContext);`,
  'nativeview.js':`import React from 'react'; export function NativeView({args}) {
    const win=React.useContext(globalThis.__nativeWindowContext);
    React.useLayoutEffect(()=>{const token=lord.nativeAsync.start(args.__nativeOwner,'logs.sources');
      globalThis.__nativeMountRecords.push({id:win.id,owner:args.__nativeOwner,token});},[]);
    return React.createElement('view');
  }`,
  'logs.js':`export const logs={sources:()=>({user:1,system:false}),files:()=>[],query:()=>[],system:()=>[],tail:()=>'',follow(){},onChange:()=>()=>{}};`,
  'fs.js':['write','stat','mkdir','copy','jobs','forgetJob','trash'].map(name=>`export const ${name}=()=>null;`).join('\n'),
  'doc.js':['fontInfo','fontSample','fontGrid','releaseImage'].map(name=>`export const ${name}=()=>null;`).join('\n'),
};
const result=await build({stdin:{resolveDir:root,contents:`
  import React from 'react'; import {render} from '@lordos/runtime';
  import {nativeApp} from './runtime/nativeapps.js';
  const Context=React.createContext(null);globalThis.__nativeWindowContext=Context;
  const App=nativeApp({id:'logs',asyncServices:['logs.sources']}).Component;
  let update;
  function Root(){const [ids,set]=React.useState([]);update=set;return React.createElement(React.Fragment,null,...ids.map(id=>React.createElement(Context.Provider,{key:id,value:{id,args:{}}},React.createElement(App))));}
  render(React.createElement(Root));
  const flush=async()=>{for(let i=0;i<20;i++)await new Promise(resolve=>setImmediate(resolve));};
  export async function windows(ids){await flush();update(ids);await flush();}
`},bundle:true,write:false,format:'esm',platform:'node',define:{'process.env.NODE_ENV':'"production"'},alias:{'@lordos/runtime':path.join(root,'runtime/index.js')},plugins:[{
  name:'native-endpoints',setup(b){
    b.onResolve({filter:/sdk\/(nativeapps\.js|nativeview\.js|components\.jsx|logs\.js|fs\.js|doc\.js)$/},a=>({path:path.basename(a.path),namespace:'fake-sdk'}));
    b.onLoad({filter:/.*/,namespace:'fake-sdk'},a=>({contents:stubs[a.path],resolveDir:root,loader:'js'}));
  },
}]});
const {windows}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
await windows(['logs','logs#2']);
const [first,second]=globalThis.__nativeMountRecords;
assert.equal(first.id,'logs');assert.equal(second.id,'logs#2');
assert.ok(Number.isSafeInteger(first.owner)&&first.owner>0);assert.notEqual(first.owner,second.owner);
assert.equal(lord.nativeAsync.take(first.owner,[first.token])[0].value.user,1);
await windows(['logs#2']);
assert.throws(()=>lord.nativeAsync.start(first.owner,'logs.sources'),/invalidRequest/);
const alive=lord.nativeAsync.start(second.owner,'logs.sources');
await windows([]);
assert.deepEqual(lord.nativeAsync.take(second.owner,[alive]),[]);
await windows(['logs']);
const reopened=globalThis.__nativeMountRecords.at(-1);assert.notEqual(reopened.owner,first.owner);
await windows([]);
console.log('native-async-mount-test: string WM IDs, startup, independent leases, close and reopen passed');
