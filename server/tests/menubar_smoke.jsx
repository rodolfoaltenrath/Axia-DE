// Real QuickJS -> Rust menu bar view -> compositor, in a private headless session: the bar's
// menus are a native view (menuBarNativeView), a click opens one and an item runs in JS.
import { createElement as h, useEffect } from 'react';
import { render, View } from '@lordos/runtime';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { settings } from '../../sdk/settings.js';
import { MenuBar } from '../../themes/default/components/MenuBar.jsx';

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
let launchpad = 0;
function Harness() {
  useEffect(() => { setTimeout(() => run().catch((e) => { console.error(e); }), 150); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(MenuBar, { t: theme, height: 28, focused: null, onLaunchpad: () => { launchpad++; }, onControlCenter: () => {}, controlCenter: false,
      area: { x: 0, y: 0, w: 1000, h: 700 } }));
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
}
settings.set({ menuBarNativeView: true });
render(h(Harness));
