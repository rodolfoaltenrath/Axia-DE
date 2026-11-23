// Compare production Rust with the geometry used before the JSX migration
// (apps/screenshots/index.jsx at baeecaf4). Batch requests share one native process.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const binary = process.env.AXIA_APP_MODEL_BIN || fileURLToPath(new URL('../../rust/target/debug/examples/app-model', import.meta.url));
const cases = [];
for (const [width, height] of [[1920, 1080], [1080, 1920], [640, 480], [1, 1]]) {
  for (const [lx, ly, bw, bh] of [[0, 0, 1000, 660], [53, 91, 760, 430], [-30, 70, 200, 800]]) {
    const s = Math.min(bw / width, bh / height);
    const ox = lx + (bw - width * s) / 2, oy = ly + (bh - height * s) / 2;
    for (const [x, y] of [[0, 0], [100, 190], [700, 580], [-90, 840]]) {
      cases.push({ input: { cmd: 'map', viewport: { x: lx, y: ly, w: bw, h: bh }, image: { width, height }, point: { x, y } },
        expected: { x: (x - ox) / s, y: (y - oy) / s, s } });
      cases.push({ input: { cmd: 'anchor', viewport: { x: lx, y: ly, w: bw, h: bh }, image: { width, height }, point: { x, y } },
        expected: { left: (bw - width * s) / 2 + x * s, top: (bh - height * s) / 2 + y * s } });
    }
  }
}
for (const a of [{ x: 0, y: 0 }, { x: 300, y: 200 }, { x: -80, y: 53 }]) {
  for (const b of [{ x: 0, y: 0 }, { x: 100, y: 120 }, { x: 600, y: -50 }]) {
    cases.push({ input: { cmd: 'selection', start: a, end: b }, expected:
      { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) } });
  }
}
const result = spawnSync(binary, [], { input: cases.map(({ input }) => JSON.stringify({ app: 'screenshots', input })).join('\n') + '\n', encoding: 'utf8' });
if (result.error) throw result.error;
assert.equal(result.status, 0, result.stderr);
const answers = result.stdout.trim().split('\n').map(JSON.parse);
assert.equal(answers.length, cases.length);
for (let i = 0; i < cases.length; i++) {
  assert.deepEqual(Object.keys(answers[i]).sort(), Object.keys(cases[i].expected).sort());
  for (const [key, value] of Object.entries(cases[i].expected)) {
    assert.ok(Math.abs(answers[i][key] - value) <= 1e-10 * Math.max(1, Math.abs(value)), `${i}/${key}: ${answers[i][key]} != ${value}`);
  }
}
console.log(`screenshots-geometry: ${cases.length} historical selection, letterbox and text-anchor cases passed`);
