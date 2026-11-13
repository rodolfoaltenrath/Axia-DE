// Production JS adapters + production Rust source, without a compositor build.
import { requests } from './pdf_model_node.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { cacheScenario } from './pdf_cache_scenario.mjs';
const root = fileURLToPath(new URL('../../', import.meta.url));
const result = await build({ stdin: { contents: `
  export * as geometry from './apps/pdf/geometry.js';
  export { PageCache } from './apps/pdf/cache.js';
  export { ViewerModel } from './apps/pdf/viewer.js';
  export { SidebarModel } from './apps/pdf/sidebar.js';
`, resolveDir: root }, bundle: true, write: false, format: 'esm', plugins: [{
  name: 'image-release', setup(b) {
    b.onResolve({ filter: /^@lordos\/sdk$/ }, () => ({ path: 'sdk', namespace: 'test' }));
    b.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents:
      'export const releaseImage = image => globalThis.__pdfRelease(image);' }));
  },
}] });
const { geometry: G, PageCache, ViewerModel, SidebarModel } = await import('data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].text).toString('base64'));
let checks = 0;
function near(actual, expected, label) {
  if (expected === null) { assert.equal(actual, null, label); }
  else if (typeof expected === 'number') {
    assert.ok(typeof actual === 'number' && Math.abs(actual - expected) <= 1e-10 * Math.max(1, Math.abs(expected)), `${label}: ${actual} != ${expected}`);
  } else if (Array.isArray(expected)) {
    assert.equal(actual.length, expected.length, label);
    expected.forEach((value, i) => near(actual[i], value, label));
  } else {
    assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), label);
    for (const key of Object.keys(expected)) near(actual[key], expected[key], label);
  }
}
const cases = JSON.parse(fs.readFileSync(path.join(root, 'rust/axia-shell/tests/fixtures/pdf_geometry.json'), 'utf8'));
for (const { input: i, expected } of cases) {
  let value;
  switch (i.cmd) {
    case 'shown': value = G.shown(i.size, i.rot); break;
    case 'toView': value = G.toView(...i.point, i.size, i.rot, i.scale); break;
    case 'toPage': value = G.toPage(...i.point, i.size, i.rot, i.scale); break;
    case 'rect': value = G.rectToView(i.rect, i.size, i.rot, i.scale); break;
    case 'layout': value = G.layoutPages(i.sizes, i.rot, i.scale, i.gap); break;
    case 'visible': value = G.visibleRange(i.tops, [], 0, 1, i.y0, i.y1); break;
    default: throw new Error(i.cmd);
  }
  near(value, expected, `reference ${checks++}: ${i.cmd}`);
}
const a = G.layoutPages([[100, 200], [100, 200]], 0, 1, 10);
const b = G.layoutPages([[200, 500]], 1, 2, 20);
assert.deepEqual(G.visibleRange(a.tops, [], 0, 1, 220, 300), [1, 1]); checks++;
assert.deepEqual(G.visibleRange(b.tops, [], 0, 1, 220, 300), [0, 0]); checks++;
assert.ok(!Object.hasOwn(requests.at(-1), 'tops'), 'normal scrolling retains native positions'); checks++;
assert.deepEqual(G.shownPages([[100, 200], [300, 400]], 1), [[200, 100], [400, 300]]); checks++;
assert.throws(() => G.toPage(1, 1, [100, 200], 0, 0), /pdf.invalidGeometry/); checks++;
assert.throws(() => G.layoutPages([[100, -1]], 0, 1, 10), /pdf.invalidGeometry/); checks++;
assert.throws(() => G.toView(1e308, 1, [100, 200], 0, 1e308), /pdf.invalidGeometry/); checks++;

const expected = JSON.parse(fs.readFileSync(path.join(root, 'rust/axia-shell/tests/fixtures/pdf_cache_trace.json'), 'utf8'));
const trace = await cacheScenario(PageCache);
trace.forEach((snapshot, i) => { assert.deepEqual(snapshot, expected[i], snapshot.label); checks++; });
assert.equal(trace.length, expected.length); checks++;

// Host failure paths and resource ownership are checked independently of the
// frozen successful-render scenario.
const released = [];
globalThis.__pdfRelease = image => released.push(image);
let pending = [], calls = 0, changed = 0, throwRender = true;
const doc = {
  render() {
    calls++;
    if (throwRender) throw new Error('synchronous render failure');
    return new Promise((resolve, reject) => pending.push({ resolve, reject }));
  }, cancel() {},
};
const cache = new PageCache(doc, () => changed++);
assert.throws(() => cache.want(0, 32, 0, false, 1), /synchronous render failure/); checks++;
throwRender = false;
cache.want(0, 32, 0, false, 1);
pending.shift().resolve({ image: 'invalid-image', w: -1, h: 100 });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
await flush();
assert.deepEqual(released, ['invalid-image']); checks++;
assert.equal(cache.bytes, 0); checks++;
assert.equal(cache.pending.size, 0); checks++;
cache.want(0, 32, 0, false, 1);
pending.shift().resolve({ image: 'ready-image', w: 32, h: 100 });
await flush();
const entry = cache.get(0, 32, 0, false);
assert.equal(cache.get(0, 32, 0, false), entry, 'get preserves rendered-entry identity'); checks++;
assert.equal(changed, 1); checks++;
cache.close(); cache.close();
assert.deepEqual(released, ['invalid-image', 'ready-image']); checks++;
const before = calls;
cache.want(1, 32, 0, false, 1);
assert.equal(calls, before, 'closed owners admit no new helper requests'); checks++;
assert.equal(cache.bytes, 0); checks++;
const other = new PageCache(doc, () => {});
assert.equal(other.key(0, 32, 0, false), '0|32|0|0|0'); checks++;
other.invalidate(0);
assert.equal(other.key(0, 32, 0, false), '0|32|0|0|1'); checks++;
assert.equal(cache.key(0, 32, 0, false), '0|32|0|0|0'); checks++;
other.close();

const viewer = new ViewerModel();
for (const { input, expected } of JSON.parse(fs.readFileSync(path.join(root, 'rust/axia-shell/tests/fixtures/pdf_viewer.json'), 'utf8'))) {
  const { op, cmd, ...facts } = input;
  near(viewer.call(op, facts), expected, `viewer reference ${checks++}`);
}
for (const c of JSON.parse(fs.readFileSync(path.join(root, 'rust/axia-shell/tests/fixtures/pdf_navigation.json'), 'utf8'))) {
  const l = c.layout, layout = G.layoutPages(l.sizes, l.rot, l.scale, l.gap);
  near(G.viewport(layout, c.viewport), c.expected, `viewport reference ${checks++}`);
  near(G.navigate(layout, c.goto.page, c.goto.rect), c.goto.expected, `navigation reference ${checks++}`);
  near(G.hitPoint(layout, c.hit.x, c.hit.y), c.hit.expected, `hit reference ${checks++}`);
  near(G.frameFor(layout, c.expected.first), c.expected.frames[0], `frame reference ${checks++}`);
  assert.ok(!Object.hasOwn(requests.at(-1), 'sizes') && !Object.hasOwn(requests.at(-1), 'tops')); checks++;
}

const searches = [], selections = [], published = [], navigated = [], selected = [];
const helper = {
  find(query, options, onPart) {
    return new Promise((resolve, reject) => searches.push({ query, onPart, resolve, reject }));
  },
  select(...args) {
    return new Promise((resolve, reject) => selections.push({ args, resolve, reject }));
  },
};
const hits = value => published.push(value), go = (...value) => navigated.push(value);
viewer.bind(helper);
const oldFind = viewer.find(helper, 'same', hits, go);
const latestFind = viewer.find(helper, 'same', hits, go);
searches[0].onPart(0, [[1, 2, 3, 4]]); searches[0].resolve(); await oldFind;
assert.equal(published.at(-1).list.length, 0, 'old identical-query result is rejected'); checks++;
searches[1].onPart(0, []);
assert.equal(navigated.length, 0, 'empty helper parts do not navigate'); checks++;
searches[1].onPart(1, [[1, 2, 3, 4]]);
assert.deepEqual(navigated, [[1, [1, 2, 3, 4]]]); checks++;
searches[1].onPart(2, [[5, 6, 7, 8]]); searches[1].resolve(); await latestFind;
assert.equal(published.at(-1).done, true); checks++;
assert.equal(viewer.call('findNext', { delta: -1 }).go.page, 2); checks++;
assert.equal(viewer.call('findNext', { delta: 1 }).go.page, 1); checks++;
await viewer.find(helper, ' \uFEFF ', hits, go);
assert.equal(published.at(-1), null); checks++;
const count = published.length;
searches[1].onPart(0, [[0, 0, 1, 1]]);
assert.equal(published.length, count, 'cleared searches stay clear'); checks++;
const switched = viewer.find(helper, '\uFEFF next \uFEFF', hits, go);
assert.equal(searches[2].query, 'next', 'helper query uses JS-compatible trimming'); checks++;
viewer.reset();
const afterReset = published.length;
searches[2].onPart(0, [[1, 2, 3, 4]]); searches[2].resolve(); await switched;
assert.equal(published.length, afterReset, 'document changes reject pending search results'); checks++;
await viewer.find(helper, 'stale-start', hits, go);
assert.equal(searches.length, 3, 'old render events cannot restart a closed document search'); checks++;

const drag = { page: 1, x0: 8, y0: 9, x1: 2, y1: 3 };
viewer.bind(helper);
const oldSelect = viewer.select(helper, drag, 0, value => selected.push(value));
const latestSelect = viewer.select(helper, drag, 1, value => selected.push(value));
assert.deepEqual(selections[0].args, [1, 8, 9, 2, 3, 0], 'drag order is retained'); checks++;
const selection = { text: 'selected', rects: [[1, 2, 3, 4]] };
selections[0].resolve(selection); await oldSelect;
assert.equal(selected.length, 0); checks++;
selections[1].resolve(selection); await latestSelect;
assert.deepEqual(selected, [{ page: 1, ...selection }]); checks++;
const cleared = viewer.select(helper, drag, 0, value => selected.push(value));
assert.equal(viewer.clearSelection(), null); checks++;
selections[2].resolve(selection); await cleared;
assert.equal(selected.length, 1, 'cleared selection cannot reappear'); checks++;
const closed = viewer.select(helper, drag, 0, value => selected.push(value));
viewer.reset(); selections[3].resolve(selection); await closed;
assert.equal(selected.length, 1, 'old document selection cannot reappear'); checks++;
await viewer.select(helper, drag, 0, value => selected.push(value));
assert.equal(selections.length, 4, 'old render events cannot restart a closed document selection'); checks++;
assert.equal(viewer.key({ ctrl: true, alt: true, base: 'g' }, false, true), 'goto'); checks++;
assert.equal(viewer.key({ ctrl: true, shift: true, base: 'g' }, false, true), 'previousHit'); checks++;
assert.equal(viewer.key({ key: 'Escape' }, true, true), null); checks++;
assert.equal(viewer.call('keyScroll', { action: 'down', value: 490, max: 500, height: 600 }), 500); checks++;
// Production mutation dispatch: deferred helpers expose ordering and stale-doc
// races without rebuilding or starting the compositor.
const operations = [], editPackets = [], infoRequests = [];
const deferredOperation = (kind, payload) => new Promise((resolve, reject) => operations.push({ kind, payload, resolve, reject }));
const editDoc = {
  info: { pages: 4 },
  annot: payload => deferredOperation('annot', payload),
  undo: page => deferredOperation('undo', page),
  setField: (id, value) => deferredOperation('field', { id, value }),
  save: path => deferredOperation('save', path),
  pageInfo: page => new Promise(resolve => infoRequests.push({ page, resolve })),
};
const editing = new ViewerModel();
editing.onEdit = packet => editPackets.push(packet);
editing.bind(editDoc);
const annot = page => ({ type: 'highlight', page, rects: [[1, 2, 3, 4]], color: '#ffd60a' });
editing.edit('annotate', { annotation: annot(1) });
editing.edit('annotate', { annotation: annot(2) });
editing.edit('undo');
editing.edit('save', { path: '/home/u/copy.pdf', name: 'copy.pdf' });
assert.equal(operations.length, 1, 'only one helper mutation is active'); checks++;
operations[0].resolve(); await flush();
assert.equal(operations[1].kind, 'annot'); checks++;
operations[1].resolve(); await flush();
assert.deepEqual([operations[2].kind, operations[2].payload], ['undo', 2]); checks++;
operations[2].resolve(); await flush();
assert.deepEqual([operations[3].kind, operations[3].payload], ['save', '/home/u/copy.pdf']); checks++;
operations[3].resolve(); await flush();
assert.deepEqual(editPackets.at(-1).state.history, [1]); checks++;
assert.equal(editPackets.at(-1).saved, 'copy.pdf'); checks++;
assert.deepEqual(editPackets.filter(p => p.invalidate).map(p => p.invalidate.page), [1, 2, 2]); checks++;
assert.equal(editPackets.at(-1).state.dirty, true, 'saving a copy does not change the original dirty state'); checks++;
const fm = { r: [1, 2, 3, 4], f: { id: 12, ro: false, type: 'check', value: false } };
const staleInfo = editing.pageInfo(editDoc, 0, value => published.push(value));
editing.edit('fieldOpen', { page: 0, field: fm });
editing.edit('fieldOpen', { page: 0, field: fm });
assert.deepEqual(operations[4].payload, { id: 12, value: true }); checks++;
operations[4].resolve(); await flush();
assert.deepEqual(operations[5].payload, { id: 12, value: false }); checks++;
operations[5].resolve(); await flush();
const publishCount = published.length;
infoRequests[0].resolve({ links: [], fields: [fm], notes: [] }); await staleInfo;
assert.equal(published.length, publishCount, 'page facts from before an edit are discarded'); checks++;
const newInfo = editing.pageInfo(editDoc, 0, value => published.push(value));
infoRequests[1].resolve({ links: [], fields: [fm], notes: [] }); await newInfo;
assert.equal(published.length, publishCount + 1); checks++;
assert.equal(editPackets.at(-1).state.dirty, true); checks++;
editing.edit('annotate', { annotation: annot(0) });
editing.edit('annotate', { annotation: annot(3) });
operations[6].reject(new Error('annotation failed')); await flush();
assert.equal(editPackets.at(-1).notice.key, 'pdf.annotError'); checks++;
assert.equal(operations[7].payload.page, 3, 'a failed mutation drains the next queued edit'); checks++;
operations[7].resolve(); await flush();
editing.edit('annotate', { annotation: annot(0) });
editing.reset();
const replacementDoc = { ...editDoc };
editing.bind(replacementDoc);
editing.edit('annotate', { annotation: annot(2) });
operations[8].resolve(); await flush();
assert.deepEqual(editPackets.at(-1).state.history, []); checks++;
operations[9].resolve(); await flush();
assert.deepEqual(editPackets.at(-1).state.history, [2]); checks++;
const oldRequestCount = operations.length;
editing.edit('annotate', { annotation: annot(1) }, editDoc);
assert.equal(operations.length, oldRequestCount, 'events bound to the previous document cannot edit its replacement'); checks++;
editing.edit('noteOpen', { point: { page: 1, x: 5, y: 6 } });
editing.edit('noteChange', { value: ' \uFEFF note \uFEFF ' });
editing.edit('noteCommit');
assert.deepEqual(operations[10].payload, { page: 1, type: 'note', r: [5, 6, 22, 22], text: 'note', color: '#ffd60a' }); checks++;
operations[10].resolve(); await flush();
editing.edit('inkStart', { point: { page: 0, x: 10, y: 10 } });
const beforePoint = editPackets.length;
editing.edit('inkAppend', { point: { page: 0, x: 10.1, y: 10 }, scale: 96 / 72 });
assert.equal(editPackets.length, beforePoint, 'sub-threshold motion does not republish the ink draft'); checks++;
editing.edit('inkAppend', { point: { page: 0, x: 12, y: 10 }, scale: 96 / 72 });
assert.deepEqual(editPackets.at(-1).ink, { kind: 'point', point: [12, 10] }); checks++;
editing.edit('inkFinish', { color: '#30d158' });
assert.deepEqual(operations[11].payload, { page: 0, type: 'ink', strokes: [[10, 10, 12, 10]], color: '#30d158', width: 2.2 }); checks++;
operations[11].resolve(); await flush();
const highlightSelections = [];
replacementDoc.select = (...args) => new Promise(resolve => highlightSelections.push({ args, resolve }));
const move = editing.select(replacementDoc, drag, 0, value => selected.push(value));
const highlight = editing.select(replacementDoc, drag, 0, value => selected.push(value), '#ffd60a');
highlightSelections[0].resolve(selection); await move;
assert.equal(operations.length, 12, 'an obsolete move selection never creates a highlight'); checks++;
highlightSelections[1].resolve(selection); await highlight;
assert.equal(operations[12].payload.type, 'highlight'); checks++;
assert.deepEqual(operations[12].payload.rects, selection.rects); checks++;
assert.equal(editPackets.at(-1).clearSelection, true); checks++;
operations[12].resolve(); await flush();
const oversized = editing.select(replacementDoc, drag, 0, value => selected.push(value), '#ffd60a');
highlightSelections[2].resolve({ text: 'large selection', rects: Array.from({ length: 2001 }, () => [1, 2, 3, 4]) });
await oversized;
assert.equal(editPackets.at(-1).notice.code, 'bad-request', 'oversized highlights preserve the existing localized error path'); checks++;
assert.equal(operations.length, 13, 'invalid highlights do not reach the helper'); checks++;
editing.reset();

for (const c of JSON.parse(fs.readFileSync(path.join(root, 'rust/axia-shell/tests/fixtures/pdf_sidebar.json'), 'utf8'))) {
  const sidebar = new SidebarModel();
  sidebar.configure(c.sizes, c.rot);
  near(sidebar.viewport(c.height, c.page), c.first, `sidebar follow ${checks++}`);
  near(sidebar.scroll(c.delta), c.scrolled, `sidebar scroll ${checks++}`);
  near(sidebar.viewport(c.height, c.nextPage), c.next, `sidebar jump ${checks++}`);
  assert.ok(!Object.hasOwn(requests.at(-1), 'sizes'), 'sidebar scrolling sends scalar facts'); checks++;
}
const sidebar = new SidebarModel(), otherSidebar = new SidebarModel();
const bookmarks = [{ title: 'Chapter', page: 0, open: true, kids: [{ title: 'Section', page: 3 }] }];
assert.deepEqual(sidebar.outline(bookmarks).rows.map(r => r.key), ['0', '0.0']); checks++;
assert.deepEqual(sidebar.toggle('0').rows.map(r => r.key), ['0']); checks++;
assert.deepEqual(sidebar.toggle('0').rows.map(r => r.key), ['0', '0.0']); checks++;
assert.equal(sidebar.pick('0.0'), 3); checks++;
assert.throws(() => otherSidebar.pick('0.0'), /pdf.invalidSidebar/); checks++;
assert.throws(() => sidebar.outline([{ title: 'Broken', kids: true }]), /pdf.invalidSidebar/); checks++;
assert.equal(sidebar.pick('0.0'), 3, 'invalid outline updates preserve the prior document'); checks++;
assert.deepEqual(sidebar.outline([]).rows, []); checks++;
assert.throws(() => sidebar.pick('0.0'), /pdf.invalidSidebar/); checks++;
console.log(`pdf-model-test: ${checks} checks passed (frozen references + native adapters + helper lifecycle)`);
