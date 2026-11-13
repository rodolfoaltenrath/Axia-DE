// Real QuickJS -> Rust menu bar view -> compositor, in a private headless session: the bar's
// menus are a native view (menuBarNativeView), a click opens one and an item runs in JS.
import { createElement as h, useEffect, useState } from 'react';
import { render, View } from '@lordos/runtime';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { settings } from '../../sdk/settings.js';
import { configureWindows } from '../../sdk/windows.js';
import { MenuBar } from '../../themes/default/components/MenuBar.jsx';
import { NativeTitleBar } from '../../sdk/nativetitlebar.jsx';
import { RustLauncher } from '../../sdk/rustlauncher.jsx';
import { SpotlightPanel } from '../../sdk/rustsearch.jsx';
import { spotlight } from '../../sdk/search.js';

const dump = () => { const out = []; for (let handle = 1; handle < 256; handle++) { const t = lord.nativeViews.text(handle); if (t) out.push(handle + ': ' + t.replace(/\n/g, ' | ')); } return out.join(' // '); };
const check = (ok, message) => { if (!ok) throw new Error('menu bar smoke: ' + message + ' [' + dump() + ']'); };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(ok, ms = 3000) {
  for (const end = Date.now() + ms; !ok() && Date.now() < end;) await sleep(20);
  return ok();
}
const theme = { ...DEFAULT_TOKENS, menuBg: 'rgba(30,32,44,0.7)', menuText: '#ffffff', menuHover: 'rgba(255,255,255,0.2)', blur: 24 };
const find = (label) => { for (let handle = 1; handle < 256; handle++) { const node = lord.nativeViews.findText(handle, label); if (node) return node; } return 0; };
const press = (node) => {
  const [x, y, w, height] = lord.getLayout(node);
  for (const type of ['down', 'up']) {
    const mount = lord.nativeViews.pointer(type, node, x + w / 2, y + height / 2, 0);
    __lordEvent(type, mount, x + w / 2, y + height / 2, 0);
  }
  globalThis.__lordNativeView();
};
let launchpad = 0, maxes = 0, acts = [], closed = 0, showLauncher = false, setLauncher = () => {};
const opened = [];
const drags = [];
function Harness() {
  const [launcher, setShow] = useState(false);
  showLauncher = launcher; setLauncher = setShow;
  useEffect(() => { setTimeout(() => run().catch((e) => { console.error(e); }), 150); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(MenuBar, { t: theme, height: 28, focused: null, onLaunchpad: () => { launchpad++; }, onControlCenter: () => {}, controlCenter: false,
      area: { x: 0, y: 0, w: 1000, h: 700 } }),
    h(View, { style: { position: 'absolute', left: 100, top: 100, width: 400 } },
      h(NativeTitleBar, { id: 'w1', title: 'Smoke window', focused: true, height: 40, chrome: null,
        controls: [{ key: 'close', color: '#ff5f57', glyph: '×' }],
        tokens: { titleBg: '#222233', text: '#ffffff', textDim: '#888888', lightOff: '#555555' },
        onDrag: (x, y) => { drags.push([x, y]); }, onMax: () => { maxes++; }, onAct: (key) => { acts.push(key); } })),
    h(SpotlightPanel),
    showLauncher && h(RustLauncher, { mode: 'flow', t: theme, area: { x: 0, y: 0, w: 1000, h: 700 }, onOpen: (id) => { opened.push(id); }, onClose: () => { closed++; } }));
}
const lines = () => { const out = []; for (let handle = 1; handle < 256; handle++) { const t = lord.nativeViews.text(handle); if (t) out.push(...t.split('\n')); } return out; };
async function run() {
  check(await until(() => find('Window') && find('Help')), 'the native bar shows Window and Help');
  const bar = lines().length;
  const first = lord.getLayout(find('Window'));
  check(first[0] > 0 && first[2] > 20, 'items have a place in the bar');
  press(find('Axia DE'));                       // the app menu
  check(await until(() => find('About Axia DE')), 'a click opens the menu with its items');
  press(find('About Axia DE'));                 // runs wm.open('about') and closes the menu
  check(await until(() => lines().length === bar), 'running an item closes the menu');
  press(find('Window'));
  check(await until(() => lines().length > bar), 'the Window menu opens');
  check(lord.nativeViews.key('ArrowDown', '', 0) === true, 'the open menu takes the arrow keys');
  check(lord.nativeViews.key('Escape', '', 0) === true, 'and Escape');
  globalThis.__lordNativeView();
  check(await until(() => lines().length === bar), 'Escape closes the menu');
  console.log('menu bar integration: passed');
  // the title bar of a window: a press on it starts the window manager's drag, a double press zooms
  press(find('Smoke window'));
  check(await until(() => drags.length === 1), 'a press on the title bar asks to drag the window');
  check(drags[0][0] > 100 && drags[0][1] >= 100, 'the pointer comes in screen coordinates');
  press(find('Smoke window')); press(find('Smoke window'));
  check(await until(() => maxes >= 1), 'a double press on the title bar zooms');
  console.log('title bar integration: passed');
  // the launcher: the grid of apps drawn by Rust, opened and closed by the shell
  setLauncher(true);
  await sleep(600);
  check(await until(() => find('Smoke App')), 'the launcher lists the apps');
  check(find('Search') > 0, 'with its search field');
  check(lord.nativeViews.key('Escape', '', 0) === true, 'the search field has the focus and takes Escape');
  globalThis.__lordNativeView();
  check(await until(() => closed >= 1), 'Escape closes the launcher');
  press(find('Smoke App'));
  check(await until(() => opened.includes('smoke-app')), 'a click on an app opens it');
  console.log('launcher integration: passed');
  // the Spotlight: the session is JS, its tree is drawn by Rust; the arrows reach it, Escape closes it
  setLauncher(false);
  await sleep(200);
  spotlight.open();
  await sleep(600);
  check(!!find('Search apps, files, settings, math…'), 'the Spotlight shows its search field');
  check(lord.nativeViews.key('ArrowDown', '', 0) === true, 'the field passes the arrows on');
  for (const ch of ['2', '+', '2']) { check(lord.nativeViews.key(ch, ch, 0) === true, 'the field takes ' + ch); globalThis.__lordNativeView(); await sleep(100); }   // the shell drains the effects after each key
  check(await until(() => lines().some((l) => l.includes('4'))), 'a typed sum comes back from the search as a result');
  check(lord.nativeViews.key('Escape', '', 0) === true, 'the field takes Escape');
  globalThis.__lordNativeView();
  check(await until(() => !lines().some((l) => l.includes('Esc close'))), 'and Escape closes the panel');
  console.log('spotlight integration: passed');
}
configureWindows({ apps: [{ id: 'smoke-app', name: 'Smoke App', size: [400, 300] }], workArea: { x: 0, y: 0, w: 1000, h: 700 } });
settings.set({ menuBarNativeView: true, launcherNativeView: true, spotlightNativeView: true });
render(h(Harness));
