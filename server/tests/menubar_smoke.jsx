// Real QuickJS -> Rust menu bar view -> compositor, in a private headless session: the bar's
// menus are a native view (menuBarNativeView), a click opens one and an item runs in JS.
import { createElement as h, useEffect, useState } from 'react';
import { render, View } from '@lordos/runtime';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { settings } from '../../sdk/settings.js';
import { configureWindows } from '../../sdk/windows.js';
import { enableShellLayers } from '../../sdk/layers.js';
import { Menu } from '../../sdk/menu.jsx';
import { NativeDockTip, NativeDockGhost } from '../../sdk/nativedockparts.jsx';
import { MenuBar } from '../../themes/default/components/MenuBar.jsx';
import { NativeTitleBar } from '../../sdk/nativetitlebar.jsx';
import { RustLauncher } from '../../sdk/rustlauncher.jsx';
import { SpotlightPanel } from '../../sdk/rustsearch.jsx';
import { spotlight } from '../../sdk/search.js';
import { NotificationBanner, notify, notifications, keyboard } from '../../sdk/index.js';

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
let classicOn = false, setClassic = () => {};
let dockParts = false, setDockParts = () => {};
let menuRan = [], menuClosed = 0, showMenu = false, setMenuShown = () => {};
let kbd = 0, ran = 0, launchpad = 0, maxes = 0, acts = [], closed = 0, showLauncher = false, setLauncher = () => {};
const opened = [];
const drags = [];
function Harness() {
  const [launcher, setShow] = useState(false);
  showLauncher = launcher; setLauncher = setShow;
  const [menuOn, setMenuOn] = useState(false), [partsOn, setPartsOn] = useState(false), [classic, setClassicOn] = useState(false);
  classicOn = classic; setClassic = setClassicOn;
  dockParts = partsOn; setDockParts = setPartsOn;
  showMenu = menuOn; setMenuShown = setMenuOn;
  useEffect(() => { setTimeout(() => run().catch((e) => { console.error(e); }), 150); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(MenuBar, { t: theme, height: 28, focused: null, onLaunchpad: () => { launchpad++; }, onControlCenter: () => {}, controlCenter: false,
      area: { x: 0, y: 0, w: 1000, h: 700 } }),
    h(View, { style: { position: 'absolute', left: 100, top: 100, width: 400 } },
      h(NativeTitleBar, { id: 'w1', title: 'Smoke window', focused: true, height: 40, chrome: null,
        controls: [{ key: 'close', color: '#ff5f57', glyph: '×' }],
        tokens: { titleBg: '#222233', text: '#ffffff', textDim: '#888888', lightOff: '#555555' },
        onDrag: (x, y) => { drags.push([x, y]); }, onMax: () => { maxes++; }, onAct: (key) => { acts.push(key); } })),
    h(SpotlightPanel), h(NotificationBanner),
    classicOn && h(RustLauncher, { mode: 'classic', t: theme, box: { left: 50, top: 60, width: 420, height: 560 }, onOpen: (id) => { opened.push(id); }, onClose: () => { closed++; }, isOnBar: () => false }),
    dockParts && h(NativeDockTip, { t: theme, label: 'Smoke tip', style: { position: 'absolute', left: 400, top: 300 } }),
    dockParts && h(NativeDockGhost, { t: theme, base: 54, removeLabel: 'Smoke remove cue', drag: { id: 'smoke-app', x: 600, y: 400, removing: true } }),
    showMenu && h(Menu, { x: 300, y: 200, onClose: () => { menuClosed++; }, items: [
      { label: 'Menu A', onClick: () => { menuRan.push('a'); } }, '-',
      { label: 'Menu B', submenu: [{ label: 'Sub C', onClick: () => { menuRan.push('c'); } }] },
      { label: 'Menu D', onClick: () => { menuRan.push('d'); } }] }),
    showLauncher && h(RustLauncher, { mode: 'flow', t: theme, area: { x: 0, y: 0, w: 1000, h: 700 }, onOpen: (id) => { opened.push(id); }, onClose: () => { closed++; } }));
}
const lines = () => { const out = []; for (let handle = 1; handle < 256; handle++) { const t = lord.nativeViews.text(handle); if (t) out.push(...t.split('\n')); } return out; };
async function run() {
  check(await until(() => find('Window') && find('Help')), 'the native bar shows Window and Help');
  const layout = keyboard.label(keyboard.active());
  check(await until(() => find(layout)), 'the keyboard layout is an item of the native bar');
  const realNext = keyboard.next; keyboard.next = () => { kbd++; };
  press(find(layout));
  check(await until(() => kbd === 1), 'a click on the layout switches it');
  keyboard.next = realNext;
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
  for (const ch of ['s', 'm', 'o']) { check(lord.nativeViews.key(ch, ch, 0) === true, 'the field takes ' + ch); globalThis.__lordNativeView(); await sleep(80); }
  check(await until(() => lines().includes('smo') && find('Smoke App')), 'typing keeps the text in the field and filters the grid');
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
  // a notification banner: built by the shell, drawn by Rust; its action runs and dismisses it
  notify({ title: 'Smoke banner', body: 'Banner body', sticky: true, sound: false, actions: [{ id: 'go', label: 'Do it', run: () => { ran++; } }] });
  check(await until(() => find('Smoke banner') && find('Do it')), 'the banner shows its title and its action');
  press(find('Do it'));
  check(await until(() => ran === 1 && notifications.get().length === 0), 'its action runs and dismisses the banner');
  check(await until(() => !find('Smoke banner')), 'and the banner goes away');
  console.log('banner integration: passed');
  // the shared Menu: drawn by Rust in the popover layer, its callbacks run here
  enableShellLayers();
  setMenuShown(true);
  check(await until(() => find('Menu A') && find('Menu D')), 'the menu shows its rows');
  press(find('Menu A'));
  check(await until(() => menuRan.join() === 'a' && menuClosed >= 1), 'a click runs the row and closes the menu');
  setMenuShown(false); await sleep(100); menuClosed = 0; setMenuShown(true);
  check(await until(() => find('Menu D')), 'it opens again');
  for (let n = 0; n < 3; n++) check(lord.nativeViews.key('ArrowDown', '', 0) === true, 'the arrows move through the rows');   // A, B (the separator is skipped), D
  globalThis.__lordNativeView(); await sleep(100);
  check(lord.nativeViews.key('Enter', '', 0) === true, 'Enter takes the row');
  globalThis.__lordNativeView();
  check(await until(() => menuRan.join() === 'a,d'), 'and runs the third row');
  console.log('menu integration: passed');
  // the Dock's tooltip and the ghost of a dragged icon with its removal cue
  setDockParts(true);
  check(await until(() => find('Smoke tip') && find('Smoke remove cue')), 'the Dock tooltip and the drag ghost show their texts');
  setDockParts(false);
  check(await until(() => !find('Smoke tip') && !find('Smoke remove cue')), 'and go away');
  console.log('dock parts integration: passed');
  // the start menu of the classic theme: a search whose results come from JS, run with Enter
  setClassic(true);
  await sleep(600);
  for (const ch of ['s', 'm', 'o', 'k', 'e']) { check(lord.nativeViews.key(ch, ch, 0) === true, 'the start menu field takes ' + ch); globalThis.__lordNativeView(); await sleep(80); }
  check(await until(() => lines().some((l) => l.includes('Smoke App'))), 'the search lists the app');
  closed = 0;
  check(lord.nativeViews.key('ArrowDown', '', 0) === true, 'the field passes the arrows on');
  check(lord.nativeViews.key('Enter', '', 0) === true, 'and Enter');
  globalThis.__lordNativeView();
  check(await until(() => closed >= 1), 'running a result closes the start menu');
  console.log('classic launcher integration: passed');
}
configureWindows({ apps: [{ id: 'smoke-app', name: 'Smoke App', size: [400, 300] }], workArea: { x: 0, y: 0, w: 1000, h: 700 } });
settings.set({ menuBarNativeView: true, launcherNativeView: true, spotlightNativeView: true, notificationsNativeView: true, menuNativeView: true });
render(h(Harness));
