import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { backupServices } from '../../runtime/native_backup_services.js';
import { windowServices } from '../../runtime/native_window_services.js';
const subscribers=new Set(),calls=[];
let status={state:'idle',dest:'/backup'},config={dest:'/backup',interval:'hourly'};
const record=name=>(...args)=>{calls.push([name,args]);return true;};
const backup={config:()=>config,configure(c){calls.push(['configure',[c]]);config={...c};},
  runNow:record('run'),pause:record('pause'),cancel:record('cancel'),list:()=>[{id:'new',time:100}],
  ls:(id,rel)=>{calls.push(['ls',[id,rel]]);return [{name:'a',size:42}];},
  path:id=>`/backup/${id}/home`,restore:record('restore'),remove:record('remove'),unlock:record('unlock'),lock:record('lock')};
const bridge=createNativeAsync({...backupServices({backup,backupStatus:{get:()=>status,subscribe(fn){subscribers.add(fn);return()=>subscribers.delete(fn);}},events:owner=>bridge.events(owner)}),...windowServices({open:record('open')})});
const methods=['backup.watch','backup.events','backup.config','backup.configure','backup.run','backup.pause','backup.cancel','backup.list','backup.ls','backup.path','backup.restore','backup.remove','backup.unlock','backup.lock','windows.open'];
const flush=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
async function invoke(owner,method,args=[]){const token=bridge.start(owner,method,args);await flush();return bridge.take(owner,[token])[0]?.value;}
bridge.open(1,methods);bridge.open(2,methods);
assert.deepEqual(await invoke(1,'backup.watch'),status);await invoke(2,'backup.watch');assert.equal(subscribers.size,2);
status={state:'running',dest:'/backup',progress:{files:12,totalFiles:50}};subscribers.forEach(fn=>fn());
assert.deepEqual(await invoke(1,'backup.events'),[{kind:'backup',status}]);assert.deepEqual(await invoke(1,'backup.events'),[]);
assert.deepEqual(await invoke(2,'backup.events'),[{kind:'backup',status}]);
assert.deepEqual(await invoke(1,'backup.config'),config);
const next={dest:'/external',interval:'daily',pauseOnBattery:false,exclude:['Videos']};assert.deepEqual(await invoke(1,'backup.configure',[next]),next);
assert.deepEqual(await invoke(1,'backup.list'),[{id:'new',time:100}]);assert.deepEqual(await invoke(1,'backup.ls',['new','Docs']),[{name:'a',size:42}]);
assert.equal(await invoke(1,'backup.path',['new']),'/backup/new/home');
await invoke(1,'backup.restore',['new','Docs/a',{replace:true,suffix:' (restored)'}]);assert.deepEqual(calls.at(-1),['restore',['new','Docs/a',{replace:true,suffix:' (restored)'}]]);
await invoke(1,'backup.unlock',['test passphrase',true]);assert.deepEqual(calls.at(-1),['unlock',['test passphrase',true]]);
await invoke(1,'windows.open',['files',{new:true,args:{path:'/backup/new/home/Docs'}}]);assert.deepEqual(calls.at(-1),['open',['files',{new:true,args:{path:'/backup/new/home/Docs'}}]]);
for(const [method,args] of [['backup.run',[]],['backup.pause',[true]],['backup.cancel',[]],['backup.remove',['new']],['backup.lock',[]]])await invoke(1,method,args);
assert.ok(calls.some(([name,args])=>name==='pause'&&args[0]===true));
bridge.close(1);assert.equal(subscribers.size,1);bridge.close(2);assert.equal(subscribers.size,0);
status={state:'idle'};subscribers.forEach(fn=>fn());assert.deepEqual(bridge.events(1),[]);
console.log('native-backup-test: raw status, subscription ownership, configure, restore, encryption and windows passed');
