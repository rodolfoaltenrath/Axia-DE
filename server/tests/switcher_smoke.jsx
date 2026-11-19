// Runs inside a real axia-server/QuickJS process against the Rust bridge.
// Legacy power, wallpaper and MIME coverage. The selector is checked by
// rust/xtask/tests/switcher_runtime.rs and the offline switcher-check command.
import { createElement as h, useEffect } from 'react';
import { render, View } from '@lordos/runtime';
import { registerBuiltinApp } from '../../sdk/windows.js';

import { findWallpaper } from '../../sdk/wallpapers.js';
import { settings } from '../../sdk/settings.js';
import { defaultOpenerId } from '../../sdk/defaults.js';
import { askPower, canPower, powerAsk, PowerDialog } from '../../sdk/power.jsx';

const check = (ok, message) => { if (!ok) throw new Error('switcher smoke: ' + message); };
function Harness() {
  useEffect(() => { const timer = setTimeout(runTests, 100); return () => clearTimeout(timer); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#487383' } }, h(PowerDialog));
}

render(h(Harness));
function runTests() {
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
  mimeTests();
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
