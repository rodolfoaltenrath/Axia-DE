// App-switcher lifecycle and shortcut adapter. Rust owns MRU, membership,
// selection, layout, icons and the scene; no window list crosses this adapter.
import { createElement as h } from 'react';
import { createStore, useStore } from './store.js';
import { Layer } from './layers.js';
import { wm, applyNativeWindowState } from './windows.js';
export { cycleAppWindows } from './appcontrol.js';
import { NativeView } from './nativeview.js';
import { switcherBridge } from './nativeswitcher.js';

const MANIFEST = { id: 'shell:switcher', shell: true, appearanceSource: 'native', catalogSource: 'native', fullTheme: true };
// Lifecycle only. Entries and the selected index are private to the Rust view.
export const switcherState = createStore(null);
export const switcherOpen = () => !!switcherState.get();
let nonce = 0;
switcherBridge.perform = effect => {
  if (effect.nonce !== switcherState.get()?.nonce) return;
  switcherState.set(null);
  switcherBridge.reset();
  if (effect.reply) applyNativeWindowState(effect.reply);
  else if (typeof effect.focus === 'string') wm.focus(effect.focus);
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

export function AppSwitcher() {
  const state = useStore(switcherState);
  if (!state) return null;
  return h(Layer, { name: 'overlay' }, h(NativeView, {
    key: state.nonce, manifest: MANIFEST,
    stateSource: lord.windowList, handleRef: switcherBridge.handle,
    args: state,
    style: { position: 'absolute', left: 0, top: 0, width: 0, height: 0, pointerEvents: 'box-none' },
  }));
}
