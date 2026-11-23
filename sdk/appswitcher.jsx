// Intent adapter. The Session owns opening state, queued input and the scene.
import { applyNativeWindowState } from './windows.js';
import { nativeAppRequest } from './nativeappmodel.js';
import { syncNativeCatalog } from './nativecatalog.js';
import './nativeview.js';
export { cycleAppWindows } from './appcontrol.js';

export const switcherOpen = () => nativeAppRequest(lord.nativeApps, { cmd: 'switcherStatus' }).opened === true;
export function openSwitcher(hold, back = false) {
  if (switcherOpen()) cancelSwitcher();
  syncNativeCatalog();
  applyNativeWindowState(nativeAppRequest(lord.nativeApps, { cmd: 'switcherPrepare', hold, back }));
  globalThis.__lordNativeView?.();
}
export function cancelSwitcher() {
  applyNativeWindowState(nativeAppRequest(lord.nativeApps, { cmd: 'switcherCancel' }));
  globalThis.__lordNativeView?.();
}
export function switcherKey(e) {
  const reply = nativeAppRequest(lord.nativeApps, {
    cmd: 'switcherKey', key: e.key || '', mods: e.mods, down: !!e.down, shift: !!e.shift,
  });
  globalThis.__lordNativeView?.();
  return reply.consumed === true;
}
// Retained SDK export for older callers. The session root mounts its own view.
export const AppSwitcher = () => null;
