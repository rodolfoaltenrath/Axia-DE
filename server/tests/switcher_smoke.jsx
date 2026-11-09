// Runs inside a real axia-server/QuickJS process against the Rust bridge.
import { createElement as h, useEffect } from 'react';
import { render, View } from '@lordos/runtime';
import { configureWindows, registerBuiltinApp, wm } from '../../sdk/windows.js';
import { openSwitcher, switcherKey, switcherState, cancelSwitcher, AppSwitcher } from '../../sdk/appswitcher.jsx';
import { useShortcuts, shortcutList, resetShortcuts } from '../../sdk/shortcuts.js';
import { findWallpaper } from '../../sdk/wallpapers.js';
import { settings } from '../../sdk/settings.js';
import { defaultOpenerId } from '../../sdk/defaults.js';
import { askPower, canPower, powerAsk, PowerDialog } from '../../sdk/power.jsx';

const check = (ok, message) => { if (!ok) throw new Error('switcher smoke: ' + message); };
function Harness() {
  useShortcuts({ grabs: 'flow', onSuperTap: () => { superTaps++; } });
  useEffect(() => { const timer = setTimeout(runTests, 100); return () => clearTimeout(timer); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#487383' } }, h(AppSwitcher), h(PowerDialog));
}
let superTaps = 0;
resetShortcuts();                       // the dispatcher, as startShell installs it in a session
render(h(Harness));
function runTests() {
  check(!!lord.switcher, 'Rust bridge exists');
  check(typeof lord.session.hostPower === 'function' && !lord.session.hostPower(),
    'headless tests cannot request host power');
  const nativeSession = lord.session;
  try {
    // Test only the UI contract. No native power method is ever called.
    lord.session = { hostPower: () => true, managed: () => false, power: () => { throw new Error('unexpected power request'); } };
    check(canPower(), 'Fedora host power is offered without the LordOS session daemon');
    askPower('reboot');
    check(powerAsk.get()?.host && powerAsk.get()?.state === 'ask', 'restart requires confirmation');
    for (const why of ['denied', 'inhibited', 'unavailable']) {
      powerAsk.set({ ...powerAsk.get(), state: 'sent' });
      globalThis.__lordHw('session-power', { ok: false, why });
      check(powerAsk.get()?.state === why, 'host refusal reaches the dialog: ' + why);
    }
    // The LordOS session daemon's channel is gone with the remote desktop: only the host (logind) powers off.
    lord.session = { managed: () => true, onMonitor: () => true, power: () => false };
    check(!canPower(), 'without host power there is no restart or shut down');
  } finally {
    lord.session = nativeSession;
    powerAsk.set(null);
  }
  console.log('session power integration: passed');
  // Render confirmation and host failure states. The real native API stays
  // headless and the deadline is far away; no power action can be sent here.
  const dialogStates = ['ask', 'denied', 'inhibited', 'unavailable'];
  const showDialog = () => {
    const state = dialogStates.shift();
    powerAsk.set(state ? { action: 'reboot', host: true, until: Date.now() + 3600000, state } : null);
    if (state) setTimeout(showDialog, 80);
    else console.log('host power dialog rendering: passed');
  };
  showDialog();
  const source = lord.fs.home() + '/wallpaper input.png';
  const image = lord.settings.importWallpaper(source);
  check(image && image.src !== source && image.thumb !== image.src, 'wallpaper has a managed copy and thumbnail');
  check(lord.fs.stat(image.src) && lord.fs.stat(image.thumb), 'wallpaper files exist');
  check(lord.settings.importWallpaper(source).id === image.id, 'importing twice reuses the file');
  check(lord.settings.importWallpaper(lord.fs.home() + '/bad image.png') === null, 'corrupt image rejected');
  check(lord.settings.importWallpaper(lord.fs.home() + '/huge image.png') === null, 'oversized image rejected');
  check(lord.settings.importWallpaper(source + '\0extra') === null, 'embedded NUL rejected');
  check(findWallpaper([], image.id, [image]) === image, 'personal wallpaper survives a theme without a collection');
  check(lord.settings.glassMaterial(true, false).backgroundColor.endsWith(',1.0000)'), 'effects off is opaque');
  settings.set({ wallpaperUser: [image], wallpaper: image.id });
  check(lord.settings.save(JSON.stringify(settings.get()), true), 'wallpaper preferences are durable');
  console.log('wallpaper integration: passed');
  configureWindows({ apps: ['files', 'terminal', 'calculator'].map((id) => ({ id, name: id })) });
  wm.open('files'); wm.open('terminal'); wm.open('calculator');
  const extra = wm.open('terminal', { new: true });
  wm.focus('calculator');
  openSwitcher(8);
  let first = switcherState.get();
  check(first.items[first.index].win === extra, 'group picks the latest window');
  wm.close(extra);
  first = switcherState.get();
  check(first.items[first.index].win === 'terminal', 'another window keeps the app in the selector');
  cancelSwitcher();
  openSwitcher(8);
  let s = switcherState.get();
  check(s.items[s.index].win === 'terminal', 'most recently used app selected');
  // Keep the overlay open for a frame to exercise its actual glass styles.
  setTimeout(() => {
    switcherKey({ mods: 8, down: true, key: 'Tab', shift: false });
    s = switcherState.get();
    check(s.items[s.index].win === 'files', 'next choice comes from Rust');
    switcherKey({ mods: 8, down: true, key: 'Escape' });
    check(wm.focused() === 'calculator', 'cancel preserves focus');
    openSwitcher(8);
    settings.set({ shortcutProfile: 'linux' });
    setTimeout(() => {
      check(!switcherState.get(), 'changing profile cancels an open selector');
      const row = shortcutList().flatMap((g) => g.items).find((it) => it.id === 'switchApps');
      check(row.keys.includes('Alt+Tab'), 'Linux profile is applied live');
      check(row.keys.includes('Super+Tab'), 'Super+Tab remains available in the Linux profile');
      __lordKey(15, 1, 'Tab', '', '', 16);
      check(switcherState.get()?.hold === 8, 'Super+Tab dispatcher holds Super in the Linux profile');
      __lordKey(1, 1, 'Escape', '', '', 16);
      __lordKey(1, 0, '', '', '', 16);
      __lordKey(15, 0, '', '', '', 16);
      __lordKey(125, 0, '', '', '', 0);
      check(!switcherState.get(), 'dispatcher cancellation clears Super selector');
      openSwitcher(2);
      switcherKey({ mods: 0, down: false, key: 'Alt' });
      check(wm.focused() === 'terminal', 'releasing Alt commits focus');
      openSwitcher(2);
      s = switcherState.get();
      wm.close(s.items[s.index].win);
      s = switcherState.get();
      check(s && s.index >= 0 && s.index < s.items.length, 'closing a choice updates the Rust selection');
      check(s.items.every((it) => wm.get(it.win)), 'closed app removed from the overlay');
      settings.set({ shortcutProfile: 'macos' });
      setTimeout(() => {
        check(!switcherState.get(), 'second profile change cancels selector');
        const row = shortcutList().flatMap((g) => g.items).find((it) => it.id === 'switchApps');
        check(row.keys.includes('❖Tab'), 'macOS profile is applied live');
        let rejected = false;
        try { lord.switcher.begin([null], '', false); } catch { rejected = true; }
        check(rejected, 'invalid keys rejected at the bridge');
        cancelSwitcher();
        fullscreenTests();
        mimeTests();
        console.log('switcher integration: passed');
      }, 100);
    }, 100);
  }, 100);
}

function mimeTests() {
  const nativeMime = lord.mime;
  registerBuiltinApp({ id: 'pdf', name: 'PDF', builtin: true });
  registerBuiltinApp({ id: 'player', name: 'Player', builtin: true });
  const handlers = {
    'application/x-rpm': { id: 'package-installer.desktop', source: 'declared' },
    'application/vnd.example.document': { id: 'office.desktop', source: 'declared' },
  };
  lord.mime = { ...nativeMime, get: () => null, resolve: (mime) => handlers[mime] || null };
  try {
    check(defaultOpenerId('application/x-rpm') === 'package-installer.desktop', 'RPM uses its installer, not the PDF category');
    check(defaultOpenerId('application/vnd.example.document') === 'office.desktop', 'other application types keep their own handlers');
    check(defaultOpenerId('application/octet-stream') === null, 'unknown binary is not classified as PDF');
    check(defaultOpenerId('application/pdf') === 'lordos-pdf.desktop', 'PDF keeps its native fallback');
    check(defaultOpenerId('audio/example') === 'lordos-player.desktop', 'media keeps its family fallback');
    handlers['application/x-rpm'] = { id: 'chosen-installer.desktop', source: 'user', explicit: true };
    check(defaultOpenerId('application/x-rpm') === 'chosen-installer.desktop', 'explicit package handler takes priority');
    console.log('MIME category integration: passed');
  } finally { lord.mime = nativeMime; }
}

// Keep the real Rust window list, selector and shortcut dispatcher. Only the native fullscreen
// owner is simulated here; the compositor's surface-id query is checked before replacing it.
function fullscreenTests() {
  const nativeWayland = lord.wayland;
  check(nativeWayland.fullscreenWindow() === 0, 'headless bridge has no fullscreen owner');
  registerBuiltinApp({ id: 'linux', name: 'Linux', builtin: true });
  const game = wm.open('linux', { instance: '777', args: { surface: 777, appId: 'fullscreen-test' } });
  check(game === 'linux#777', 'fullscreen surface maps to its managed window');
  lord.wayland = {
    ...nativeWayland,
    fullscreen: () => !!wm.get(game) && !wm.get(game).minimized,
    fullscreenWindow: () => 777,
  };
  try {
    const before = superTaps;
    __lordKey(125, 1, 'Meta', '', '', 16);
    __lordKey(125, 0, 'Meta', '', '', 0);
    check(superTaps === before, 'Super alone cannot open a hidden launcher in fullscreen');
    openSwitcher(2);
    check(wm.get(game).minimized && switcherState.get(), 'fullscreen yields to the selector');
    switcherKey({ mods: 2, down: true, key: 'Escape' });
    check(!wm.get(game).minimized && wm.focused() === game, 'Escape restores the fullscreen origin');
    openSwitcher(2, true);
    const other = switcherState.get().items[switcherState.get().index].win;
    check(other !== game, 'backwards selection leaves fullscreen');
    switcherKey({ mods: 0, down: false, key: 'Alt' });
    check(wm.focused() === other && wm.get(game).minimized, 'Alt release commits the chosen app');
    __lordKey(125, 1, 'Meta', '', '', 16);
    __lordKey(125, 0, 'Meta', '', '', 0);
    check(superTaps === before + 1, 'Super alone works again outside fullscreen');
    openSwitcher(2);
    const current = switcherState.get();
    const target = current.items.findIndex((it) => it.win === game);
    check(target >= 0, 'minimized fullscreen game stays in the selector');
    lord.switcher.select(target);
    switcherKey({ mods: 0, down: false, key: 'Alt' });
    check(wm.focused() === game && !wm.get(game).minimized, 'selecting the game restores its window');
    openSwitcher(2);
    wm.close(game, true);
    cancelSwitcher();
    check(!switcherState.get(), 'closing fullscreen during selection can be cancelled safely');
    console.log('fullscreen switcher integration: passed');
  } finally {
    cancelSwitcher();
    lord.wayland = nativeWayland;
    if (wm.get(game)) wm.close(game, true);
  }
}
