// Runs inside a real axia-server/QuickJS process against the Rust bridge.
import { createElement as h, useEffect } from 'react';
import { render, View } from '@lordos/runtime';
import { configureWindows, wm } from '../../sdk/windows.js';
import { openSwitcher, switcherKey, switcherState, cancelSwitcher, AppSwitcher } from '../../sdk/appswitcher.jsx';
import { useShortcuts, shortcutList } from '../../sdk/shortcuts.js';
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
