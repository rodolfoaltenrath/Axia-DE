// The PDF transport owns document sessions and raster handles for one window lease:
// opens map to the three outcomes Rust understands, a raster is released exactly once
// (by Rust, by a discard or by closing), searches stream as events, and nothing outlives
// the window.
import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { pdfServices } from '../../runtime/native_pdf_services.js';

const released = [], closedDocs = [], printed = [];
let nextImage = 500, openError = null, renderGate = null, crash = null;
const finders = [];
function fakeDoc(name) {
  return {
    info: { pages: 3, title: name, sizes: [[612, 792]] },
    onCrash(fn) { crash = fn; },
    close() { closedDocs.push(name); },
    sizes: async () => ({ sizes: [[612, 792], [612, 792], [300, 400]], forms: false }),
    render: async (page, width) => { if (renderGate) await renderGate; return { image: ++nextImage, w: Math.round(width), h: Math.round(width * 1.3) }; },
    outline: async () => ({ items: [] }),
    pageInfo: async () => ({ links: [], fields: [], notes: [] }),
    select: async () => ({ text: 'x', rects: [] }),
    find: (query, options, onPage) => new Promise((resolve, reject) => finders.push({ query, onPage, resolve, reject })),
    annot: async () => null,
    setField: async () => null,
    undo: async () => null,
    save: async path => { if (path.endsWith('/bad.pdf')) throw new Error('disk full'); return null; },
  };
}
const fs = { home: () => '/home/test', search: () => [{ name: 'a.PDF', path: '/home/test/a.PDF' }, { name: 'b.txt', path: '/home/test/b.txt' }] };
let bridge;
const methods = pdfServices({
  pdf: { open: async path => { if (openError) throw openError; return fakeDoc(path); } },
  releaseImage: image => released.push(image),
  fs, print: { open: options => printed.push(options) },
  events: id => bridge.events(id),
});
bridge = createNativeAsync(methods);
const tick = async () => { for (let i = 0; i < 6; i++) await new Promise(resolve => setImmediate(resolve)); };
async function call(owner, method, args) {
  const token = bridge.start(owner, method, args);
  await tick();
  const [packet] = bridge.take(owner, [token]);
  assert.ok(packet, `${method} produced a result`);
  if (packet.error) throw new Error(packet.error);
  return packet.value;
}
bridge.open(1, Object.keys(methods));

// Opening: a session, or one of the three failures Rust tells apart.
const opened = await call(1, 'pdf.open', ['/home/test/a.pdf', '']);
assert.deepEqual(opened, { doc: 1, pages: 3, title: '/home/test/a.pdf', sizes: [[612, 792]] });
openError = Object.assign(new Error('x'), { code: 'password' });
await assert.rejects(() => call(1, 'pdf.open', ['/p.pdf', '']), /^Error: password$/);
openError = Object.assign(new Error('x'), { code: 'invalid' });
await assert.rejects(() => call(1, 'pdf.open', ['/p.pdf', '']), /^Error: invalid$/);
openError = new Error('helper exploded');
await assert.rejects(() => call(1, 'pdf.open', ['/p.pdf', '']), /helper exploded/);
openError = null;
assert.deepEqual((await call(1, 'pdf.sizes', [1])).sizes.length, 3);
await assert.rejects(() => call(1, 'pdf.sizes', [99]), /pdf.closed/);

// A raster is released once, whoever asks first.
const render = await call(1, 'pdf.render', [1, 0, 600, 0, false, 4]);
assert.equal(render.image, 501);
await call(1, 'pdf.release', [render.image]);
await call(1, 'pdf.release', [render.image]);
assert.deepEqual(released, [501], 'a second release is ignored');

// A cancelled render is freed by the bridge's discard path.
const late = bridge.start(1, 'pdf.render', [1, 1, 400, 0, false, 2]);
await tick();
bridge.take(1, [], [late]);
assert.deepEqual(released, [501, 502], 'the discarded raster is released');

// A render finishing after its window or document is gone never keeps its raster.
let open; renderGate = new Promise(resolve => { open = resolve; });
const gated = bridge.start(1, 'pdf.render', [1, 2, 300, 0, false, 2]);
await tick();
await call(1, 'pdf.close', [1]);
open(); renderGate = null; await tick();
assert.deepEqual(bridge.take(1, [gated]).map(p => p.error), ['pdf.closed']);
assert.ok(released.includes(503), 'the raster of a render that outlived its document is released');
assert.deepEqual(closedDocs, ['/home/test/a.pdf']);

// Closing a document frees what it still holds.
const second = await call(1, 'pdf.open', ['/home/test/b.pdf', '']);
const held = await call(1, 'pdf.render', [second.doc, 0, 500, 0, false, 4]);
await call(1, 'pdf.close', [second.doc]);
assert.ok(released.includes(held.image));
await assert.rejects(() => call(1, 'pdf.pageInfo', [second.doc, 0]), /pdf.closed/);

// Searching answers at once and streams: pages, then the end; the window's queue carries them.
const third = await call(1, 'pdf.open', ['/home/test/c.pdf', '']);
assert.equal(await call(1, 'pdf.find', [third.doc, 'word', 77]), null);
finders[0].onPage(2, [[1, 2, 3, 4]]);
finders[0].resolve({ total: 1 });
await tick();
assert.deepEqual(await call(1, 'pdf.events', [third.doc]), [
  { kind: 'find', token: 77, page: 2, rects: [[1, 2, 3, 4]] }, { kind: 'findDone', token: 77 }]);
crash();
assert.deepEqual(await call(1, 'pdf.events', [third.doc]), [{ kind: 'crash', doc: third.doc }]);

// Edits report the helper's own failure message; print passes the document along.
await call(1, 'pdf.annot', [third.doc, { type: 'highlight' }]);
await assert.rejects(() => call(1, 'pdf.save', [third.doc, '/home/test/bad.pdf']), /disk full/);
await call(1, 'pdf.print', [third.doc, 'c.pdf']);
assert.equal(printed[0].title, 'c.pdf');

// The welcome list only keeps PDFs.
assert.deepEqual(await call(1, 'pdf.recent', []), [{ name: 'a.PDF', path: '/home/test/a.PDF' }]);

// Closing the window closes its documents and rasters.
const fourth = await call(1, 'pdf.open', ['/home/test/d.pdf', '']);
const last = await call(1, 'pdf.render', [fourth.doc, 0, 500, 0, false, 4]);
bridge.close(1);
assert.ok(released.includes(last.image), 'the lease frees every raster');
assert.ok(closedDocs.includes('/home/test/d.pdf') && closedDocs.includes('/home/test/c.pdf'));
console.log('native-pdf-services: open outcomes, single raster release, discard, close and lease cleanup, streamed search and edit failures passed');
