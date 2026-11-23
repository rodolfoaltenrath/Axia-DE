import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { sysinfoServices } from '../../runtime/native_sysinfo_services.js';

let resolveAdmin;
const protectedStates=[], files=new Map();
globalThis.lord={sysinfo:{},apps:{launchWith:()=>true}};
const services=sysinfoServices({systemInfo:()=>({admin:true}),codeStatus:()=>({}),
  admin:{available:()=>true,run:(_,__,log)=>new Promise(resolve=>{resolveAdmin=()=>{log('serial system PRIVATE');resolve();};})},
  session:{canPower:()=>false},fs:{mkdir(){},stat:path=>files.has(path),write:(path,text)=>files.set(path,text)},
  userDirs:()=>({documents:'/home/test/Documents'}),wm:{setProtected:(_,value)=>protectedStates.push(value)},
  bluetooth:{get:()=>null}});
const bridge=createNativeAsync(services);
const flush=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
bridge.open(1,Object.keys(services));
const stale=bridge.start(1,'sysinfo.serials',['sysinfo']);await flush();
bridge.start(1,'sysinfo.hideSerials');await flush();resolveAdmin();await flush();
assert.deepEqual(protectedStates,[],'hiding while admin is pending must prevent late protection/publication');
assert.equal(bridge.take(1,[stale])[0].value,null);
const live=bridge.start(1,'sysinfo.serials',['sysinfo']);await flush();resolveAdmin();await flush();
assert.deepEqual(protectedStates,[true],'protect before a completed result can be taken');
assert.deepEqual(bridge.take(1,[live])[0].value,['serial system PRIVATE']);
bridge.close(1);assert.deepEqual(protectedStates,[true,false]);
bridge.open(2,Object.keys(services));
const token=bridge.start(2,'sysinfo.serials',['sysinfo']);await flush();bridge.close(2);resolveAdmin();await flush();
assert.deepEqual(bridge.take(2,[token]),[]);assert.deepEqual(protectedStates,[true,false]);
const save=services['sysinfo.save'].run;
lord.sysinfo={report:()=>({date:'2026-11-23'}),markdown:()=>'',facts:()=>{throw Error('optional unavailable');}};
assert.deepEqual(services['sysinfo.snapshot'].run().report,{date:'2026-11-23'});
assert.equal(services['sysinfo.snapshot'].run().facts,null,'optional collectors must not hide the hardware report');
assert.deepEqual(save([[{name:'lordos-report-2026-11-23.md',text:'preview\n'}]]).names,['lordos-report-2026-11-23.md']);
assert.throws(()=>save([[{name:'lordos-report-2026-11-23.md',text:'overwrite'}]]),/sysinfo.exportExists/);
assert.equal(files.get('/home/test/Documents/lordos-report-2026-11-23.md'),'preview\n');
assert.throws(()=>save([[{name:'../../unsafe.md',text:'x'}]]),/app.invalidRequest/);
console.log('native Sysinfo transport: private-query lifetimes, protection order, export collisions and path bounds passed');
