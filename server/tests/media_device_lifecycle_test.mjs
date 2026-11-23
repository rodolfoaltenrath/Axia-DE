import assert from 'node:assert/strict';
import { build } from 'esbuild';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root=fileURLToPath(new URL('../../',import.meta.url));
const fixture=globalThis.__mediaDevices={active:new Map(),events:[]};
let next=0;
globalThis.lord={media:{screenRecord:()=>++next,audioRecord:()=>++next,camera:()=>++next,close(){}}};
const stubs={
  'privacy.js':`export const requestPermission=async()=>true;export function markDeviceUse(app,kind,on){const f=globalThis.__mediaDevices;f.active.set(app+':'+kind,on);f.events.push({app,kind,on});}`,
  'theme.js':`export const useTheme=()=>({});`,
  'i18n.js':`export const t=k=>k;`,
  'fs.js':`export const read=()=>'',home=()=>'/fixture';`,
};
const result=await build({stdin:{resolveDir:root,contents:`export {media} from './sdk/media.js';`},bundle:true,write:false,format:'esm',platform:'node',define:{'process.env.NODE_ENV':'"production"'},plugins:[{name:'media-device-fixture',setup(b){
  b.onResolve({filter:/^@lordos\/runtime$/},()=>({path:'runtime',namespace:'media-fixture'}));
  b.onResolve({filter:/.*/},a=>{const name=path.basename(a.path);if(stubs[name]&&a.importer.endsWith('/sdk/media.js'))return{path:name,namespace:'media-fixture'};});
  b.onLoad({filter:/.*/,namespace:'media-fixture'},a=>({contents:a.path==='runtime'?`export const View=()=>null,Text=()=>null,Image=()=>null,MediaView=()=>null,addListener=()=>()=>{};`:stubs[a.path],resolveDir:root,loader:'js'}));
}}]});
const {media}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
const active=(app,kind)=>fixture.active.get(app+':'+kind)===true;

const old=await media.screenRecord('screenshots','/old.mp4',{mic:true});
const current=await media.screenRecord('screenshots','/current.mp4',{mic:true});
media.close(old);
assert.equal(active('screenshots','microphone'),true,'closing a rejected old recording must keep the accepted recording indicator');
__lordMedia(old,'done',{ok:true});
assert.equal(active('screenshots','microphone'),true,'late completion must not clear the remaining source');
__lordMedia(current,'done',{ok:true});
assert.equal(active('screenshots','microphone'),false);
const changes=fixture.events.length;media.close(current);assert.equal(fixture.events.length,changes,'completion and close release once');

const first=await media.audioRecord('first','/first.wav');
const second=await media.audioRecord('second','/second.wav');
media.close(first);assert.equal(active('first','microphone'),false);assert.equal(active('second','microphone'),true);
__lordMedia(second,'state',{state:'failed'});assert.equal(active('second','microphone'),false);media.close(second);

const camera=await media.camera('mixed','/camera');
const audio=await media.audioRecord('mixed','/audio.wav');
media.close(camera);assert.equal(active('mixed','camera'),false);assert.equal(active('mixed','microphone'),true);
media.close(audio);assert.equal(active('mixed','microphone'),false);
console.log('media-devices: overlapping sources, late completion, single release, app/kind isolation and failed jobs passed');
