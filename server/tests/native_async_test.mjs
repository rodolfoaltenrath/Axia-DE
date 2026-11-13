import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { fontServices } from '../../runtime/native_font_services.js';
import { documentServices } from '../../runtime/native_doc_services.js';
import { scanServices } from '../../runtime/native_scan_services.js';
import { fileServices } from '../../runtime/native_fs_services.js';
const pending = [], disposed = [], stopped = [], events = [];
const bridge = createNativeAsync({
  wait: { run(args, owner) { return new Promise((resolve, reject) => pending.push({ resolve, reject, owner })); }, dispose(value) { disposed.push(value); } },
  fail: { run() { throw new Error('denied'); } },
  watch: { run(args, owner) { events.push(owner); owner.cleanup(() => stopped.push(owner.id)); return 'watching'; } },
});
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
bridge.open(1, ['wait', 'fail', 'watch']); bridge.open(2, ['wait']);
assert.throws(() => bridge.open(1, []), /duplicateWindow/);
assert.throws(() => bridge.start(2, 'watch'), /invalidRequest/);
assert.throws(() => bridge.start(1, 'missing'), /invalidRequest/);
const a = bridge.start(1, 'wait'), b = bridge.start(2, 'wait');
await flush();
assert.deepEqual(bridge.take(1, [a]), []);
pending[1].resolve({ rows: [2] }); await flush();
assert.deepEqual(bridge.take(1, [b]), [], 'one window cannot consume another result');
assert.deepEqual(bridge.take(2, [b]), [{ token: b, value: { rows: [2] } }]);
assert.deepEqual(bridge.take(2, [b]), [], 'results are delivered once');
const failure = bridge.start(1, 'fail'); await flush();
assert.deepEqual(bridge.take(1, [failure]), [{ token: failure, error: 'denied' }]);
const watch = bridge.start(1, 'watch'); await flush(); bridge.take(1, [watch]);
events[0].emit({ kind: 'journal' });
assert.deepEqual(bridge.events(1), [{ kind: 'journal' }]);
assert.deepEqual(bridge.events(1), []);
bridge.close(1); bridge.open(1, ['wait']);
pending[0].resolve({ image: 7 }); await flush();
assert.deepEqual(disposed, [{ image: 7 }], 'closing a window disposes late resources even if its ID is reused');
assert.deepEqual(stopped, [1]);
assert.deepEqual(bridge.take(1, [a]), []);
const completed = bridge.start(1, 'wait'); await flush();
pending[2].resolve({ image: 8 }); await flush(); bridge.close(1);
assert.deepEqual(disposed, [{ image: 7 }, { image: 8 }], 'unconsumed ready resources are disposed');
bridge.close(2);
assert.throws(() => bridge.start(2, 'wait'), /invalidRequest/);
bridge.open(3, ['wait']);
const big = bridge.start(3, 'wait'); await flush();
const large = { rows: [{ m: '😀\\"\n'.repeat(80_000) }], nested: ['末尾'] };
pending.at(-1).resolve(large); await flush();
let encoded = '', fragments = 0;
for (;;) {
  const packets = bridge.take(3, [big]);
  assert.ok(Buffer.byteLength(JSON.stringify(packets)) < 1024 * 1024);
  assert.equal(packets.length, 1);
  encoded += packets[0].part; fragments++;
  if (packets[0].done) break;
}
assert.ok(fragments > 1); assert.deepEqual(JSON.parse(encoded), large);
assert.deepEqual(bridge.take(3, [big]), []);
const discarded = bridge.start(3, 'wait'); await flush();
bridge.take(3, [], [discarded]);
pending.at(-1).resolve({ image: 9 }); await flush();
assert.deepEqual(disposed.at(-1), { image: 9 }, 'discarded pending work disposes its eventual result');
assert.deepEqual(bridge.take(3, [discarded]), []);
for (let i = 0; i < 32; i++) bridge.start(3, 'wait');
assert.throws(() => bridge.start(3, 'wait'), /tooManyRequests/);
bridge.close(3); await flush();
const renders = [], releasedImages = [];
const fontBridge = createNativeAsync(fontServices({
  fs: {}, list: () => [], fontInfo: () => ({}),
  fontSample: () => new Promise(resolve => renders.push(resolve)),
  fontGrid: () => new Promise(resolve => renders.push(resolve)),
  releaseImage: id => releasedImages.push(id),
}));
const capabilities = ['fonts.sample', 'fonts.grid', 'fonts.release'];
fontBridge.open(10, capabilities);
const sample = fontBridge.start(10, 'fonts.sample', []); await flush();
renders.shift()({ image: 101, w: 100, h: 20 }); await flush();
assert.equal(fontBridge.take(10, [sample])[0].value.image, 101);
assert.deepEqual(releasedImages, [], 'delivered images remain owned by their window');
const release = fontBridge.start(10, 'fonts.release', [[101]]); await flush(); fontBridge.take(10, [release]);
assert.deepEqual(releasedImages, [101]);
const grid = fontBridge.start(10, 'fonts.grid', []); await flush();
renders.shift()({ image: 102, w: 640, h: 40 }); await flush();
fontBridge.take(10, [grid]); fontBridge.close(10);
assert.deepEqual(releasedImages, [101, 102], 'closing disposes delivered images too');
fontBridge.open(10, capabilities);
const stale = fontBridge.start(10, 'fonts.sample', []); await flush();
fontBridge.take(10, [], [stale]); renders.shift()({ image: 103, w: 100, h: 20 }); await flush();
assert.deepEqual(releasedImages, [101, 102, 103], 'discarding a pending image releases it on arrival');
const closing = fontBridge.start(10, 'fonts.sample', []); await flush(); fontBridge.close(10);
renders.shift()({ image: 104, w: 100, h: 20 }); await flush();
assert.deepEqual(releasedImages, [101, 102, 103, 104], 'an image arriving after close is released exactly once');
assert.deepEqual(fontBridge.take(10, [closing]), []);
// Production providers forward SDK arguments and retain decoded previews in
// the same owner lease used by the native scanner window.
const scanCalls = [], imageDisposals = [];
let finishImage;
const record = name => (...args) => { scanCalls.push([name, args]); return null; };
const scannerBridge = createNativeAsync({
  ...scanServices({ discover:record('discover'), scanCaps:record('caps'), scan:record('scan'), open:record('print') }),
  ...documentServices({ decodeImage:()=>new Promise(resolve=>{finishImage=resolve;}),
    pdfThumb:record('pdfThumb'), convertImage:record('convert'), releaseImage:id=>imageDisposals.push(id) }),
  ...fileServices({ stat:record('stat'), copy:record('copy'), jobs:record('jobs'), forgetJob:record('forgetJob') }),
});
const scanMethods=['scanner.discover','scanner.caps','scanner.scan','print.open','images.decode','images.pdfThumb','images.convert','images.release','fs.stat','fs.copy','fs.jobs','fs.forgetJob'];
scannerBridge.open(20,scanMethods);
const dev={addr:'192.168.1.2',port:80,rs:'eSCL'}, options={res:300,color:'Grayscale8',source:'Platen',x:0,y:0,w:2550,h:3508};
for(const [method,args] of [
  ['scanner.discover',[]],['scanner.caps',[dev]],['scanner.scan',[dev,options]],
  ['print.open',[{title:'Scan.jpg',image:'/home/test/Scan.jpg'}]],
  ['images.convert',['/tmp/scan.jpg','/home/test/Scan.pdf',{format:'pdf',native:true,dpi:300}]],
  ['fs.copy',['/tmp/scan.jpg','/home/test/Scan.jpg']],['fs.jobs',[]],['fs.forgetJob',[42]],
]) { const token=scannerBridge.start(20,method,args); await flush(); scannerBridge.take(20,[token]); }
assert.deepEqual(scanCalls.find(([name])=>name==='scan'),['scan',[dev,options]]);
assert.deepEqual(scanCalls.find(([name])=>name==='convert'),['convert',['/tmp/scan.jpg','/home/test/Scan.pdf',{format:'pdf',native:true,dpi:300}]]);
assert.deepEqual(scanCalls.find(([name])=>name==='print'),['print',[{title:'Scan.jpg',image:'/home/test/Scan.jpg'}]]);
const preview=scannerBridge.start(20,'images.decode',['/tmp/scan.jpg',{maxw:520,maxh:560}]); await flush();
finishImage({image:201,w:520,h:560}); await flush(); scannerBridge.take(20,[preview]);
assert.deepEqual(imageDisposals,[]);
scannerBridge.close(20); assert.deepEqual(imageDisposals,[201]);
scannerBridge.open(20,scanMethods);
scannerBridge.start(20,'images.decode',['/tmp/late.jpg']); await flush(); scannerBridge.close(20);
finishImage({image:202,w:520,h:560}); await flush();
assert.deepEqual(imageDisposals,[201,202],'late scanner previews are released once');
console.log('native-async-test: ownership, cancellation, errors, capabilities and event delivery passed');
