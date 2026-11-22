// Thin session adapters. Rust owns snapshots, deadlines, planning, claims and the prompt.
import { createElement as h, useEffect, useSyncExternalStore } from 'react';
import { screen, mainDisplay, onScreenChange } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { settings } from './settings.js';
import { settingsBridge } from './nativesettings.js';
import { wm, getApp, isLiveSystem, setLinuxPlacement, windowModelFacts, applyNativeWindowState } from './windows.js';
import { launchDockItem } from './dock.js';
import { Layer } from './layers.js';
import { NativeView } from './nativeview.js';
import { clearTextFocus } from './components.jsx';
import { sessionRestoreRequest } from './appcontrol.js';
import { restoreBridge } from './nativerestore.js';
import { admin } from './admin.js';
import { clipboard } from './clipboard.js';
import { RESTORE_MODES, RESTORE_DEFAULT } from './sessionmodel.js';

const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
export const restoreMode = (s = appHost ? settings.get() : null) => s ? (RESTORE_MODES.includes(s.sessionRestore) ? s.sessionRestore : RESTORE_DEFAULT) : command('preferences').result?.mode || RESTORE_DEFAULT;
export const restoreBrowsers = (s = appHost ? settings.get() : null) => s ? s.sessionBrowsers !== false : command('preferences').result?.browsers !== false;
const allowed = () => typeof lord !== 'undefined' && typeof lord.appId !== 'string'
  && typeof lord.greeter === 'undefined' && !isLiveSystem();
const visible = createStore(false); // Presentation gate for the shell's keyboard layer only.
const MANIFEST = { id: 'shell:restore', shell: true, keyboard: true, facts: 'appcontrol', fullTheme: true };

function command(action, extra = {}) {
  if (!allowed() && !['preferences', 'setMode', 'setBrowsers'].includes(action)) return { result: null };
  const reply = sessionRestoreRequest({ action, ...(action === 'preferences' ? {} : { model: windowModelFacts() }), ...extra });
  for (const effect of reply.effects || []) restoreBridge.perform?.(effect);
  return reply;
}
restoreBridge.perform = effect => {
  if (effect.kind === 'settingsState') { settingsBridge.perform?.(effect); return; }
  if (effect.kind !== 'sessionRestore') return;
  const value = effect.value;
  switch (effect.op) {
    case 'visible': if (value) clearTextFocus(); visible.set(!!value); break;
    case 'state': applyNativeWindowState(value); break;
    case 'launch': try { launchDockItem(value); } catch (_) { /* removed app */ } break;
    case 'open': {
      let id = null;
      try {
        const app = getApp(value.app);
        if (app && !app.builtin && !app.hidden && !app.live && !app.isolated && !app.package)
          id = wm.open(value.app, { new: value.new, args: value.args || undefined, frame: value.frame, desktop: value.desktop });
      } catch (_) { /* removed app */ }
      command('opened', { ticket: value.ticket, id });
      break;
    }
    case 'browser':
      if (!lord.admin?.available()) { command('browserFailed', { policy: value }); break; }
      admin.run('browser-restore', value, undefined, { quiet: true })
        .catch(() => command('browserFailed', { policy: value }));
      break;
  }
};

export function setRestoreMode(mode) {
  if (!RESTORE_MODES.includes(mode)) return;
  if (appHost) settings.set({ sessionRestore: mode, ...(mode === 'off' ? { sessionSaved: null } : null) });
  else command('setMode', { mode });
}
export function setRestoreBrowsers(on) { if (appHost) settings.set({ sessionBrowsers: !!on }); else command('setBrowsers', { on: !!on }); }
export function flushSession() { command('flush'); }
export function resumeSession() { command('resume'); }
export function markThemeSwitch() { command('theme'); }
export function logOut() { flushSession(); clipboard.beforeLogout(); lord.quit(); }
globalThis.__lordBeforeExit = () => { clipboard.beforeLogout(); command('exit'); };

setLinuxPlacement(id => {
  const claim = command('claim', { id }).result;
  return claim ? { frame: claim.frame, desktop: claim.desktop,
    then: id => command('opened', { ticket: claim.ticket, id }) } : null;
});
// Compatibility for the Dock's Open at Login check; membership lives in Rust.
export const restoredIds = Object.freeze({ has: id => command('restored', { id }).result === true });
export function startSessionRestore() { command('start'); }

export function RestorePrompt() {
  const shown = useStore(visible);
  // Work areas still come from the theme; unrelated profile edits do not rerender this adapter.
  useStore(settings, s => JSON.stringify([s.dockPosition, s.dockSize, s.dockAutohide, s.menuBarAllDisplays]));
  useEffect(() => { if (shown) clearTextFocus(); }, [shown]);
  const display = useSyncExternalStore(onScreenChange, mainDisplay);
  if (!allowed()) return null;
  const view = h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList,
    args: { model: windowModelFacts(), display: { x: display.x, y: display.y, w: display.w, h: display.h } },
    style: { position: 'absolute', left: 0, top: 0, width: shown ? screen.width : 1, height: shown ? screen.height : 1,
      pointerEvents: shown ? 'box-none' : 'none' } });
  return shown ? h(Layer, { name: 'modal' }, view) : view;
}
