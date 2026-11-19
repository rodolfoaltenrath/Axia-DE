// App-switcher lifecycle and shortcut adapter. Rust owns MRU, membership,
// selection, layout, icons and the scene; no window list crosses this adapter.
import { createElement as h } from 'react';
import { mainDisplay } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { useTheme } from './theme.js';
import { Layer } from './layers.js';
import { windows, wm, onActiveDesktop } from './windows.js';
import { dockIdOfWindow } from './dock.js';
import { dockGlass } from './glass.js';
import { NativeView } from './nativeview.js';
import { switcherBridge } from './nativeswitcher.js';

const MANIFEST = { id: 'shell:switcher', shell: true, facts: 'appcontrol', fullTheme: true };
// Lifecycle only. Entries and the selected index are private to the Rust view.
export const switcherState = createStore(null);
export const switcherOpen = () => !!switcherState.get();
let nonce = 0;
switcherBridge.perform = effect => {
  if (effect.nonce !== switcherState.get()?.nonce) return;
  switcherState.set(null);
  switcherBridge.reset();
  if (typeof effect.focus === 'string') wm.focus(effect.focus);
};
export function openSwitcher(hold, back = false) {
  const fullscreen = !!lord.wayland?.fullscreen?.();
  const surface = fullscreen ? lord.wayland?.fullscreenWindow?.() : 0;
  if (fullscreen && !surface) return;
  if (switcherOpen()) cancelSwitcher();
  const origin = surface ? `linux#${surface}` : '';
  switcherBridge.reset();
  switcherState.set({ hold, back, origin, nonce: ++nonce });
  if (origin) wm.minimize(origin);
}
export function cancelSwitcher() {
  const state = switcherState.get();
  if (!state) return;
  if (switcherBridge.handle.current) switcherBridge.send(['cancel']);
  else {
    switcherState.set(null);switcherBridge.reset();
    if (state.origin) wm.focus(state.origin);
  }
}
export function switcherKey(e) {
  const state = switcherState.get();
  if (!state) return false;
  if (!(e.mods & state.hold)) { switcherBridge.send(['commit']); return true; }
  if (!e.down) return true;
  if (e.key === 'Tab' || e.key === 'ArrowRight') switcherBridge.send(['move', e.shift && e.key === 'Tab' ? -1 : 1]);
  else if (e.key === 'ArrowLeft') switcherBridge.send(['move', -1]);
  else if (e.key === 'Enter') switcherBridge.send(['commit']);
  else if (e.key === 'Escape') cancelSwitcher();
  return true;
}

// This separate window-cycling command still uses the SDK mirror.
const keyOf = w => dockIdOfWindow(w);
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

export function AppSwitcher() {
  const state = useStore(switcherState), theme = useTheme();
  if (!state) return null;
  const display = mainDisplay();
  return h(Layer, { name: 'overlay' }, h(NativeView, {
    key: state.nonce, manifest: MANIFEST,
    stateSource: lord.windowList, handleRef: switcherBridge.handle,
    args: { ...state, surface: dockGlass(theme), radius: theme.groupRadius ? theme.groupRadius + 10 : 20 },
    style: { position: 'absolute', left: display.x, top: display.y, width: display.w, height: display.h, pointerEvents: 'box-none' },
  }));
}
