// Exercise the production JS adapters against the production Rust policies.
import './native_app_model_node.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
const root = fileURLToPath(new URL('../../', import.meta.url));
const sent = [];
const create = lord.nativeApps.create;
lord.nativeApps.create = app => {
  const owner = create(app);
  return { request(text) { sent.push(JSON.parse(text)); return owner.request(text); } };
};
const result = await build({ stdin: { contents: `
  export * as terminal from './apps/terminal/profiles.js';
  export * as passwords from './apps/passwords/templates.js';
  export * as checks from './apps/sysinfo/checks.js';
  export * as hardware from './apps/sysinfo/model.js';
  export * as keys from './apps/terminal/keys.js';
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
const { terminal, passwords, checks, hardware, keys, settings } = await import('data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].text).toString('base64'));
const cases = JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/app_models.json', 'utf8'));
let n = 0;
for (const { app, input: i, expected } of cases) {
  let got;
  if (app === 'terminal') {
    settings.set({ terminal: i.settings || {} });
    if (i.cmd === 'catalog') got = { presets: Object.entries(terminal.PRESETS), profile: terminal.DEFAULT_PROFILE };
    else if (i.cmd === 'settings') got = terminal.terminalSettings();
    else if (i.cmd === 'profile') got = terminal.getProfile(i.id);
    else if (i.cmd === 'scheme') got = terminal.schemeOf(i.profile);
    else if (i.cmd === 'config') got = terminal.toConfig(i.profile, i.extra);
  } else if (app === 'passwords') {
    if (i.cmd === 'catalog') got = { builtin: passwords.BUILTIN, system: passwords.SYSTEM_CATS, types: passwords.FIELD_TYPES, colors: passwords.COLORS };
    else if (i.cmd === 'categories') got = passwords.allCategories(i.users);
    else if (i.cmd === 'fields') got = passwords.templateFields(i.category, i.categories);
    else if (i.cmd === 'builtin') got = passwords.builtinById(i.id) ?? null;
  } else if (app === 'sysinfo') got = checks[i.cmd](i.value);
  else continue;
  assert.deepEqual(got, expected, `${app} ${i.cmd} reference ${n++}`);
}
const entries = [{ id: 'a', cat: 'wifi', secret: 'must not enter the model' }, { id: 'b', cat: 'wifi', arch: true, fav: true, tags: ['work', 'work'] }, { id: 'c', trash: true }];
const cats = passwords.allCategories();
assert.deepEqual(passwords.filterEntries(entries, 'all', cats), [entries[0]]);
assert.deepEqual(passwords.filterEntries(entries, 'fav', cats), [entries[1]]);
assert.deepEqual(passwords.filterEntries(entries, 'trash', cats), [entries[2]]);
const counts = passwords.summarizeEntries(entries, cats);
assert.equal(counts.categories.wifi, 2);
assert.equal(counts.tags.work, 1);
assert.equal(counts.all, 1);
assert.ok(!JSON.stringify(sent).includes('must not enter the model'));
settings.set({ terminal: { profiles: [{ id: 'custom', fontSize: 18 }], defaultProfile: 'custom' } });
terminal.saveTerminalSettings({ hotkey: 'Super+`' });
assert.equal(terminal.getProfile('missing').fontSize, 18);
assert.equal(terminal.terminalSettings().hotkey, 'Super+`');
settings.set({ terminal: { schemes: { zFirst: terminal.PRESETS.lordos, aSecond: terminal.PRESETS.light } } });
terminal.saveTerminalSettings({ hotkey: 'F12' });
assert.deepEqual(Object.keys(terminal.allSchemes()).slice(-2), ['zFirst', 'aSecond']);
for (const { input: i, expected } of JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/terminal_keys.json', 'utf8'))) {
  settings.set({ terminal: { shortcuts: Object.fromEntries(i.shortcuts || []), keyMap: i.keyMap || [] } });
  let got;
  if (i.cmd === 'keyCatalog') got = { bindings: Object.entries(keys.DEFAULT_BINDINGS) };
  else if (i.cmd === 'bindings') got = Object.entries(keys.bindings());
  else if (i.cmd === 'combo') got = keys.comboOf(i.event);
  else if (i.cmd === 'action') got = keys.actionOf(i.event);
  else if (i.cmd === 'keyMapping') got = keys.keyMappingOf(i.event, i.profile);
  else if (i.cmd === 'comboLabel') got = keys.comboLabel(i.value);
  else if (i.cmd === 'mappingBytes') {
    const value = keys.mappingBytes(i.mapping);
    got = value === null ? null : value instanceof ArrayBuffer ? { kind: 'hex', bytes: Array.from(new Uint8Array(value)) }
      : { kind: 'text', units: Array.from({ length: value.length }, (_, j) => value.charCodeAt(j)) };
  }
  assert.deepEqual(got, expected, `keys ${i.cmd} reference ${n++}`);
}
assert.throws(() => keys.mappingBytes({ action: 'text', param: '\\u{110000}' }), RangeError);
for (const { input: i, expected } of JSON.parse(fs.readFileSync(root + 'rust/axia-shell/tests/fixtures/sysinfo_hardware.json', 'utf8'))) {
  assert.deepEqual(hardware[i.cmd](...i.args), expected, `hardware ${i.cmd} reference ${n++}`);
}
console.log(`app-model-bridge-test: ${n} reference cases and transport/persistence/list checks passed`);
