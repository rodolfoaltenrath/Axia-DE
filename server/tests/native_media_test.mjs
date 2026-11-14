import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { mediaServices } from '../../runtime/native_media_services.js';
import { settingsServices } from '../../runtime/native_settings_services.js';
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

// Camera: the session belongs to the window; a recording (and its microphone
// indicator) is stopped before the camera closes, also when the window goes.
const cam={calls:[],closed:[],listeners:new Map(),thumbSubs:new Set(),allowCamera:null};
const camSdk={
  on(id,fn){cam.listeners.set(id,fn);return()=>cam.listeners.delete(id);},
  close(id){cam.closed.push(id);},
  cameras(){return [{device:'/dev/video0',name:'Desk'}];},
  camera(...args){cam.calls.push(['camera',args]);return new Promise(resolve=>{cam.allowCamera=resolve;});},
  view(...args){cam.calls.push(['view',args]);return true;},
  photo(...args){cam.calls.push(['photo',args]);return true;},
  cameraRecord(...args){cam.calls.push(['cameraRecord',args]);return Promise.resolve(true);},
  thumb(...args){cam.calls.push(['thumb',args]);return 'thumb:'+args[0];},
};
const camBridge=createNativeAsync(mediaServices({media:camSdk,userDirs:()=>({pictures:'/Pictures'}),soundState:{get:()=>({}),subscribe:()=>()=>{}},
  thumbRev:{subscribe(fn){cam.thumbSubs.add(fn);return()=>cam.thumbSubs.delete(fn);}},events:owner=>camBridge.events(owner)}));
const camMethods=['media.cameras','media.camera','media.view','media.photo','media.cameraRecord','media.thumbs','media.thumb','media.close','media.events'];
async function camInvoke(owner,method,args=[]){const token=camBridge.start(owner,method,args);await flush();return camBridge.take(owner,[token])[0]?.value;}
camBridge.open(1,camMethods);camBridge.open(2,camMethods);
assert.deepEqual(await camInvoke(1,'media.cameras'),[{device:'/dev/video0',name:'Desk'}]);
const opening=camBridge.start(1,'media.camera',['camera','/dev/video0',{width:1280,height:720}]);await flush();
cam.allowCamera(21);await flush();assert.equal(camBridge.take(1,[opening])[0].value,21);
assert.equal(await camInvoke(2,'media.view',[21,{mirror:1}]),false,'another window cannot drive the viewfinder');
assert.equal(await camInvoke(2,'media.photo',[21,'/x.jpg',true]),false);
assert.equal(await camInvoke(2,'media.cameraRecord',[21,'camera','/x.mp4',{}]),false);
assert.equal(await camInvoke(1,'media.view',[21,{mirror:1}]),true);
assert.equal(await camInvoke(1,'media.photo',[21,'/Pictures/Camera/a.jpg',true]),true);
assert.equal(await camInvoke(1,'media.thumb',['/Pictures/Camera/a.jpg',128]),'thumb:/Pictures/Camera/a.jpg');
await camInvoke(1,'media.thumbs');assert.equal(cam.thumbSubs.size,1);
cam.thumbSubs.forEach(fn=>fn());assert.deepEqual(await camInvoke(1,'media.events'),[{kind:'thumb'}]);
assert.equal(await camInvoke(1,'media.cameraRecord',[21,'camera','/Pictures/Camera/v.mp4',{mic:true}]),true);
cam.listeners.get(21)('video',{ok:true,path:'/Pictures/Camera/v.mp4'});
assert.deepEqual(await camInvoke(1,'media.events'),[{kind:'media',id:21,type:'video',data:{ok:true,path:'/Pictures/Camera/v.mp4'}}]);
assert.deepEqual(await camInvoke(2,'media.events'),[]);
const before=cam.calls.length;
camBridge.close(1);await flush();
assert.deepEqual(cam.calls.slice(before),[['cameraRecord',[21,'camera',null]]],'closing a recording window stops its recording first');
assert.deepEqual(cam.closed,[21]);assert.equal(cam.listeners.size,0);assert.equal(cam.thumbSubs.size,0);
// a camera granted after its window closed is closed at once
camBridge.open(1,camMethods);
camBridge.start(1,'media.camera',['camera','/dev/video0',{}]);await flush();camBridge.close(1);
cam.allowCamera(22);await flush();assert.deepEqual(cam.closed,[21,22]);assert.equal(cam.listeners.size,0);
camBridge.close(2);

// settings.patch: the shared settings transport writes the patch and returns the result
const saved=[];let current={camera:{mirror:true}};
const prefs=createNativeAsync(settingsServices({set(p){saved.push(p);current={...current,...p};},get:()=>current}));
prefs.open(5,['settings.patch']);
const patched=prefs.start(5,'settings.patch',[{camera:{mirror:false,device:'/dev/video1'}}]);await flush();
assert.deepEqual(prefs.take(5,[patched])[0].value,{camera:{mirror:false,device:'/dev/video1'}});
assert.deepEqual(saved,[{camera:{mirror:false,device:'/dev/video1'}}]);prefs.close(5);

console.log('native-media-test: permissions, session ownership, events, export, camera, settings and close passed');
