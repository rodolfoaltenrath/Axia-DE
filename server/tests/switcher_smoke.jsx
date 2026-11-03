// Runs inside a real axia-server/QuickJS process against the Rust bridge.
import { createElement as h, useEffect } from 'react';
import { render, View } from '@lordos/runtime';
import { configureWindows, wm } from '../../sdk/windows.js';
import { openSwitcher, switcherKey, switcherState, cancelSwitcher, AppSwitcher } from '../../sdk/appswitcher.jsx';
import { useShortcuts, shortcutList } from '../../sdk/shortcuts.js';
import { findWallpaper } from '../../sdk/wallpapers.js';
import { settings } from '../../sdk/settings.js';

const check = (ok, message) => { if (!ok) throw new Error('switcher smoke: ' + message); };
function Harness() {
  useShortcuts({ grabs: 'flow' });
  useEffect(() => { const timer = setTimeout(runTests, 100); return () => clearTimeout(timer); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#487383' } }, h(AppSwitcher));
}
render(h(Harness));
function runTests() {
  check(!!lord.switcher, 'Rust bridge exists');
  const profile = lord.settings.motionProfile(false, false, false);
  check(profile.codecs.join() === 'h264' && profile.width === 1280 && profile.height === 720 && profile.fps === 15 && profile.threads === 2,
    'CPU laptop gets a bounded native playback profile');
  check(!lord.settings.downloadMotion('../invalid catalog entry'), 'invalid catalog selection rejected');
  const motionJob = lord.settings.downloadMotion('testclip 1080.h264');
  check(motionJob > 0, 'native catalog download starts without the LordOS admin helper');
  let polls = 0;
  const downloadTimer = setInterval(() => {
    const result = lord.settings.pollMotion(motionJob);
    check(++polls < 30, 'cached catalog job completed promptly');
    if (!result.status) return;
    clearInterval(downloadTimer);
    check(result.status === 1 && result.path.endsWith('.mp4') && result.percent === 100, 'verified catalog cache reused');
    check(lord.settings.removeMotion('testclip 1080.h264'), 'catalog file removal stays user scoped');
    let removed = false;
    try { removed = !lord.fs.stat(result.path); } catch { removed = true; }
    check(removed, 'managed catalog file removed');
    console.log('motion storage integration: passed');
  }, 50);
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
  settings.set({ wallpaperUser: [image], wallpaper: image.id, motion: {} });
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
        console.log('switcher integration: passed');
      }, 100);
    }, 100);
  }, 100);
}
