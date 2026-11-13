import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { mediaServices } from '../../runtime/native_media_services.js';
const calls=[],closed=[],listeners=new Map(),soundListeners=new Set();
let allowRecording;
const sdk={
  on(id,fn){listeners.set(id,fn);return()=>listeners.delete(id);},
  close(id){closed.push(id);},
  audioRecord(...args){calls.push(['record',args]);return new Promise(resolve=>{allowRecording=resolve;});},
  play(...args){calls.push(['play',args]);return 9;},
  stop(...args){calls.push(['stop',args]);return true;},
  control(...args){calls.push(['control',args]);return true;},
  waveform(...args){calls.push(['waveform',args]);return {duration:1000,peaks:[1,2]};},
  transcode(...args){calls.push(['transcode',args]);return {ok:true,path:args[2]};},
};
const bridge=createNativeAsync(mediaServices({media:sdk,userDirs:()=>({music:'/Music'}),
  soundState:{get:()=>({sources:[{name:'mic'}]}),subscribe(fn){soundListeners.add(fn);return()=>soundListeners.delete(fn);}},
  events:owner=>bridge.events(owner),
}));
const methods=['media.dirs','media.sound','media.events','media.record','media.play','media.close','media.control','media.stop','media.waveform','media.transcode'];
const flush=async()=>{for(let i=0;i<16;i++)await Promise.resolve();};
async function invoke(owner,method,args=[]){const token=bridge.start(owner,method,args);await flush();return bridge.take(owner,[token])[0]?.value;}
bridge.open(1,methods);bridge.open(2,methods);
assert.deepEqual(await invoke(1,'media.dirs'),{music:'/Music'});
assert.deepEqual(await invoke(1,'media.sound'),{sources:[{name:'mic'}]});
assert.equal(soundListeners.size,1);soundListeners.forEach(fn=>fn());
assert.deepEqual(await invoke(1,'media.events'),[{kind:'sound',data:{sources:[{name:'mic'}]}}]);
const recording=bridge.start(1,'media.record',['recorder','/Music/a.wav',{source:'mic',appName:'Recorder'}]);await flush();
allowRecording(7);await flush();assert.equal(bridge.take(1,[recording])[0].value,7);
assert.deepEqual(calls.at(-1),['record',['recorder','/Music/a.wav',{source:'mic',appName:'Recorder'}]]);
listeners.get(7)('level',{level:100,ms:200});
assert.deepEqual(await invoke(1,'media.events'),[{kind:'media',id:7,type:'level',data:{level:100,ms:200}}]);
assert.deepEqual(await invoke(2,'media.events'),[]);
assert.equal(await invoke(2,'media.control',[7,'pause']),false,'another window cannot control a session');
assert.equal(await invoke(2,'media.stop',[7]),false);
assert.equal(await invoke(1,'media.control',[7,'pause']),true);
assert.equal(await invoke(1,'media.stop',[7]),true);
assert.equal(await invoke(1,'media.play',['recorder','/Music/a.wav',{music:true}]),9);
assert.deepEqual(await invoke(1,'media.waveform',['/Music/a.wav',400]),{duration:1000,peaks:[1,2]});
await invoke(1,'media.transcode',['recorder','/Music/a.wav','/Music/a.opus',{format:'opus',from:100,to:800}]);
assert.deepEqual(calls.at(-1),['transcode',['recorder','/Music/a.wav','/Music/a.opus',{format:'opus',from:100,to:800}]]);
await invoke(1,'media.close',[9]);assert.deepEqual(closed,[9]);assert.ok(!listeners.has(9));
bridge.close(1);assert.deepEqual(closed,[9,7]);assert.equal(soundListeners.size,0);assert.equal(listeners.size,0);
bridge.open(1,methods);
bridge.start(1,'media.record',['recorder','/Music/late.wav',{}]);await flush();bridge.close(1);
allowRecording(10);await flush();assert.deepEqual(closed,[9,7,10],'permission resolving after close cannot leak a microphone');
assert.equal(listeners.size,0);
bridge.open(1,methods);
const discarded=bridge.start(1,'media.record',['recorder','/Music/discard.wav',{}]);await flush();
bridge.take(1,[],[discarded]);allowRecording(11);await flush();assert.deepEqual(closed,[9,7,10,11]);assert.equal(listeners.size,0);
bridge.close(1);bridge.close(2);
console.log('native-media-test: permissions, session ownership, events, export and close passed');
