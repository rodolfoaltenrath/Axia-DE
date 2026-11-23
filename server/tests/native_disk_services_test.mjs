import assert from 'node:assert/strict';
import {createNativeAsync} from '../../runtime/native_async.js';
import {diskServices} from '../../runtime/native_disk_services.js';
let pending,summary=null;const calls=[];
const services=diskServices({disks:{available:()=>true,list:()=>[{name:'sdb'}],tools:()=>({smartctl:true})},
  admin:{available:()=>true,run:(action,arg,log)=>{calls.push({action,arg});return new Promise(resolve=>{pending=()=>{log?.('progress done=4 total=8 phase=write');resolve('ok');};});}},
  fs:{home:()=>'/home/test',stat:path=>path==='/home/test/test.iso'?{size:4,dir:false}:null,search:()=>[]},systemInfo:()=>({admin:true}),
  smart:{install:async()=>{}},smartSummaries:{set:fn=>summary=fn({})},events:owner=>bridge.events(owner)});
const bridge=createNativeAsync(services),flush=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
bridge.open(1,Object.keys(services));
const token=bridge.start(1,'disks.run',['erase','sdb\nzero',7]);await flush();pending();await flush();
assert.deepEqual(calls,[{action:'disk-erase',arg:'sdb\nzero'}]);
assert.deepEqual(bridge.events(1),[{kind:'progress',generation:7,line:'progress done=4 total=8 phase=write'}]);
assert.equal(bridge.take(1,[token])[0].value,'ok');
const late=bridge.start(1,'disks.run',['mount','sdb1',8]);await flush();bridge.close(1);pending();await flush();
assert.deepEqual(bridge.take(1,[late]),[]);assert.deepEqual(bridge.events(1),[],'closing a window discards late progress and results');
assert.throws(()=>services['disks.run'].run(['arbitrary','sdb'],{}),/app.invalidRequest/);
assert.throws(()=>services['disks.run'].run(['erase','../sdb\nzero'],{}),/app.invalidRequest/);
assert.throws(()=>services['disks.smartTest'].run(['sdb','arbitrary']),/app.invalidRequest/);
assert.throws(()=>services['disks.smartInfo'].run(['sdb1']),/disk.badDevice/);
services['disks.summary'].run(['sdb',{health:'warning',temp:45}]);assert.equal(summary.sdb.health,'warning');
assert.equal(services['disks.stat'].run(['/home/test/test.iso']).size,4);
console.log('native disk transport: closed actions, request ownership, progress generations and SMART summary passed');
