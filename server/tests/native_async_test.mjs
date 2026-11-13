import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
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
console.log('native-async-test: ownership, cancellation, errors, capabilities and event delivery passed');
