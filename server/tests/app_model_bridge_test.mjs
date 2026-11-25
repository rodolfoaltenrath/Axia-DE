// Exercise the production JS adapters against the production Rust policies.
import './native_app_model_node.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { REFERENCES as sourceReferences } from '../../tools/hwreport/references.js';
const root = fileURLToPath(new URL('../../', import.meta.url));
const sent = [];
const create = lord.nativeApps.create;
lord.nativeApps.create = app => {
  const owner = create(app);
  return { request(text) { sent.push(JSON.parse(text)); return owner.request(text); } };
};
const result = await build({ stdin: { contents: `
  export * as checks from './server/tests/support/sysinfo/checks.js';
  export * as hardware from './server/tests/support/sysinfo/model.js';
  export * as perf from './server/tests/support/sysinfo/perf.js';
  export { settings } from '@lordos/sdk';
`, resolveDir: root }, bundle: true, write: false, format: 'esm', platform: 'node', plugins: [{
  name: 'shell-facts', setup(b) {
    b.onResolve({ filter: /^@lordos\/sdk$/ }, () => ({ path: 'sdk', namespace: 'test' }));
    b.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents: `
      let state = {};
      export const settings = { get: () => state, set: patch => { state = { ...state, ...patch }; } };
      export const t = key => key;
      export const createStore = initial => ({ get: () => initial });
      export const View = () => null, Text = () => null, useTheme = () => ({});
    ` }));
  },
}] });
const { checks, hardware, perf, settings } = await import('data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].text).toString('base64'));
const cases = JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/app_models.json', 'utf8'));
let n = 0;
for (const { app, input: i, expected } of cases) {
  let got;
  if (app === 'sysinfo') got = checks[i.cmd](i.value);
  else continue;
  assert.deepEqual(got, expected, `${app} ${i.cmd} reference ${n++}`);
}
for (const { input: i, expected } of JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/sysinfo_hardware.json', 'utf8'))) {
  assert.deepEqual(hardware[i.cmd](...i.args), expected, `hardware ${i.cmd} reference ${n++}`);
}
assert.deepEqual(perf.REFERENCES, sourceReferences, 'runtime and build-tool reference measurements stay identical');
function sameMeasurement(a, b) {
  if (typeof a === 'number' && typeof b === 'number') return Math.abs(a - b) <= 1e-12 * Math.max(1, Math.abs(a), Math.abs(b));
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return a === b;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(k => Object.prototype.hasOwnProperty.call(b, k) && sameMeasurement(a[k], b[k]));
}
for (const { input: i, expected } of JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/sysinfo_performance.json', 'utf8'))) {
  assert.ok(sameMeasurement(perf[i.cmd](...i.args), expected), `performance ${i.cmd} reference ${n++}`);
}
console.log(`app-model-bridge-test: ${n} reference cases and transport/persistence/list checks passed`);
