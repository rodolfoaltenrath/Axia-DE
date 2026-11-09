// appswitcher.jsx — the app switcher, the same in every theme (docs/SHORTCUTS.md).
//
// A centered strip of large app icons with the selected app's name. Hold the modifier: Tab moves,
// Shift+Tab goes back, releasing it switches, Esc cancels. Most recently used app first, so a quick
// press goes back to the previous app. sdk/shortcuts.js opens it and hands it every key while it is
// open, and the theme mounts <AppSwitcher /> in its overlay layer inside its ThemeProvider.
//
//   openSwitcher(hold, back)  hold: the modifier's bit in e.mods (8 ❖, 2 ⌥); back: start backwards
//   cycleAppWindows(back)     the next window of the app in front (❖`)
import { createElement as h } from 'react';
import { View, Text, mainDisplay } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { useTheme } from './theme.js';
import { Layer } from './layers.js';
import { windows, wm, getApp, activeDesktop, onActiveDesktop } from './windows.js';
import { desktopSuffix } from './desktops.js';
import { dockIdOfWindow } from './dock.js';
import { runningApps } from './appcontrol.js';
import { windowIcon } from './apps.js';
import { dockGlass } from './glass.js';

// { items: [{ key, name, Icon, win }], index, hold } while it is open
export const switcherState = createStore(null);
export const switcherOpen = () => !!switcherState.get();

// Most recently used apps, newest first. A window that comes to the front moves its app up; a
// theme switch starts a new list, ordered by the windows' stacking.
const keyOf = (w) => dockIdOfWindow(w);
function trackRecent() {
  const top = wm.focused();
  const w = top && wm.get(top);
  if (w) lord.switcher?.focus(keyOf(w));
  const s = switcherState.get();
  if (!s) return;
  if (s.items.every((it) => wm.get(it.win))) return;
  const live = new Map(listApps().map((it) => [it.key, it]));
  const items = s.items.map((it) => wm.get(it.win) ? it : live.get(it.key)).filter(Boolean);
  const removed = s.items.filter((it) => !live.has(it.key));
  let index = s.index;
  for (const it of removed) index = lord.switcher.remove(it.key);
  if (!items.length) cancelSwitcher();
  else switcherState.set({ ...s, items, index });
}
windows.subscribe(trackRecent);

// the window an app comes back with: on this desktop if it has one, a visible one before a minimized
// one. The switcher lists the apps of every desktop; choosing one elsewhere jumps there (wm.focus).
function windowOf(group) {
  return wm.get(wm.pick(windows.get().filter((w) => group.windows.some((x) => x.id === w.id)).map((w) => w.id)));
}

function listApps() {
  const groups = new Map(runningApps().map((g) => {
    const win = windowOf(g);
    return [keyOf(win), { key: keyOf(win), name: g.name + desktopSuffix(win), win: win.id, Icon: windowIcon(win, getApp) }];
  }));
  // Initial input order is the compositor's stack. Rust owns MRU ranking,
  // deduplication and selection, including wrapping and removal.
  const items = [];
  for (const w of [...windows.get()].reverse()) {
    const key = keyOf(w), item = groups.get(key);
    if (item) { items.push(item); groups.delete(key); }
  }
  return items.slice(0, 512);
}

export function openSwitcher(hold, back = false) {
  if (typeof lord !== 'undefined' && lord.wayland?.fullscreen?.()) return;    // a fullscreen app is presented above every layer
  const items = listApps();
  if (!items.length) return;
  const top = wm.get(wm.focused());
  const state = lord.switcher.begin(items.map((it) => it.key), top ? keyOf(top) : '', back);
  switcherState.set({ items: state.order.map((i) => items[i]), index: state.index, hold });
}

export function cancelSwitcher() {
  lord.switcher?.finish(false);
  switcherState.set(null);
}
function commit() {
  const s = switcherState.get();
  const index = lord.switcher.finish(true);
  switcherState.set(null);
  const it = s?.items[index];
  if (it && wm.get(it.win)) wm.focus(it.win);
}
function move(step) {
  const index = lord.switcher.step(step);
  switcherState.set((s) => (s ? { ...s, index } : s));
}

// Every key goes here while the switcher is open. True: taken. A key that arrives with the modifier
// already up (it was released while another session had the keyboard) also ends it.
export function switcherKey(e) {
  const s = switcherState.get();
  if (!(e.mods & s.hold)) { commit(); return true; }
  if (!e.down) return true;
  if (e.key === 'Tab' || e.key === 'ArrowRight') move(e.shift && e.key === 'Tab' ? -1 : 1);
  else if (e.key === 'ArrowLeft') move(-1);
  else if (e.key === 'Enter') commit();
  else if (e.key === 'Escape') cancelSwitcher();
  return true;
}

export function cycleAppWindows(back = false) {
  const top = wm.get(wm.focused());
  if (!top) return;
  const key = keyOf(top);
  const mine = windows.get().filter((w) => keyOf(w) === key && !w.minimized && onActiveDesktop(w));
  if (mine.length < 2) return;
  if (!back) { wm.focus(mine[0].id); return; }           // the app's rearmost window comes up
  wm.toBack(top.id);                                     // the front one goes under everything
  wm.focus(mine[mine.length - 2].id);
}

const ICON = 72, CELL = 104;

export function AppSwitcher() {
  const s = useStore(switcherState);
  const th = useTheme();
  if (!s) return null;
  const a = mainDisplay();
  const perRow = Math.max(1, Math.floor((a.w - 96) / CELL));
  const shown = s.items.length > perRow
    ? s.items.slice(Math.max(0, Math.min(s.index - Math.floor(perRow / 2), s.items.length - perRow))).slice(0, perRow)
    : s.items;
  return h(Layer, { name: 'overlay' },
    h(View, { style: { position: 'absolute', left: a.x, top: a.y, width: a.w, height: a.h, alignItems: 'center', justifyContent: 'center', pointerEvents: 'box-none' } },
      h(View, { style: { alignItems: 'center', padding: 16, borderRadius: th.groupRadius ? th.groupRadius + 10 : 20, maxWidth: a.w - 48,
        ...dockGlass(th),
        boxShadow: '0 18px 48px rgba(0,0,0,0.45)' } },
      h(View, { style: { flexDirection: 'row', direction: 'ltr' } }, shown.map((it) => {
        const on = it === s.items[s.index];
        return h(View, { key: it.key, onClick: () => { lord.switcher.select(s.items.indexOf(it)); commit(); },
          style: { width: CELL, height: CELL, borderRadius: 16, alignItems: 'center', justifyContent: 'center', backgroundColor: on ? `${th.accent}55` : 'transparent' } },
        h(it.Icon, { size: ICON }));
      })),
      h(Text, { style: { marginTop: 10, fontSize: 15, fontWeight: 600, color: th.text, textAlign: 'center' } }, s.items[s.index]?.name || ''))));
}
