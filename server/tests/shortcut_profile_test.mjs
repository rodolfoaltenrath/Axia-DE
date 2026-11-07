import assert from 'node:assert/strict';
import { shortcutProfile, profileEntry } from '../../sdk/shortcutprofile.js';
import { dockGlass } from '../../sdk/glass.js';
const keys = (id, profile, original = []) => profileEntry({ id, keys: original }, profile).keys;
assert.equal(shortcutProfile(null, 'default'), 'macos');
assert.equal(shortcutProfile(null, 'classic'), 'linux');
assert.equal(shortcutProfile('linux', 'default'), 'linux');
assert.equal(shortcutProfile('macos', 'classic'), 'macos');
assert.equal(shortcutProfile('invalid', 'classic'), 'linux');
assert.deepEqual(keys('switchApps', 'macos'), ['Meta+Tab', 'Meta+Shift+Tab']);
assert.deepEqual(keys('switchApps', 'linux'), ['Meta+Tab', 'Meta+Shift+Tab', 'Alt+Tab', 'Alt+Shift+Tab']);
for (const profile of ['macos', 'linux']) {
  assert.deepEqual(keys('search', profile, ['Meta+Space']), ['Meta+Space']);
}
assert.deepEqual(keys('closeWindow', 'linux'), ['Alt+F4']);
assert.deepEqual(keys('closeWindow', 'macos', ['Alt+F4']), []);
assert.deepEqual(keys('files', 'linux'), ['Meta+E']);
assert.deepEqual(keys('lock', 'macos'), ['Meta+Ctrl+Q']);
assert.deepEqual(keys('lock', 'linux'), ['Meta+L']);
assert.deepEqual(keys('quit', 'linux', ['Meta+Q']), []);
assert.deepEqual(keys('searchCtrl', 'linux', ['Ctrl+Space']), []);
assert.deepEqual(keys('quick', 'macos', ['Meta+A']), []); // leaves select-all available
assert.deepEqual(keys('capture.area', 'linux'), ['Meta+Shift+S']);
assert.deepEqual(keys('terminalDropdown', 'linux', ['F12']), ['F12']);
assert.equal(profileEntry({ id: 'switchApps', keys: [], show: ['old'] }, 'linux').show, undefined);
// Native material policy is tested through the real Rust bridge in switcher_smoke.
// This unit test checks theme overrides and the SDK's handoff to that service.
globalThis.lord = { settings: { glassMaterial: (dark, effects) => ({
  backgroundColor: effects ? 'native-glass' : 'native-opaque', borderColor: 'native-outline',
}) } };
const glass = { mode: 'dark', effects: true, dockBg: 'rgba(46,46,56,0.42)', dockBorder: '#aaa', blur: 24 };
assert.equal(dockGlass(glass).backdropFilter, 'blur(24px)');
assert.equal(dockGlass(glass).backgroundColor, glass.dockBg);
assert.equal(dockGlass({ ...glass, effects: false }).backdropFilter, 'blur(0px)');
assert.equal(dockGlass({ ...glass, effects: false }).backgroundColor, 'native-opaque');
console.log('shortcut profiles and shared glass: passed');
