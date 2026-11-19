// windowmemory.js — each app reopens where it was (docs/WINDOWS.md, "Remembered places"): size, place, maximized and which
// display, per app, in settings.windowPlaces. Session restore's frames come first; an assignment in the Dock chooses the display.
// A Linux app's own request doesn't override this, except a dialog or a window of fixed size (sdk/wayland.js).
import { createElement as h, useSyncExternalStore } from 'react';
import { screen, mainDisplay, onScreenChange } from '@lordos/runtime';
import { settings } from './settings.js';
import { NativeView } from './nativeview.js';
import { workAreaOf, getApp, setWindowMemory, assignedDisplayOf, isLiveSystem } from './windows.js';
import { dockIdOfWindow } from './dock.js';
import { rememberWindowFrame } from './appcontrol.js';

const MANIFEST = { id: 'shell:window-memory', shell: true, facts: 'appcontrol' };
const allowed = () => typeof lord !== 'undefined' && typeof lord.appId !== 'string'
  && typeof lord.greeter === 'undefined' && !isLiveSystem();
const displayFacts = d => ({ id: d.id, name: d.name, connector: d.connector,
  x: d.x, y: d.y, w: d.w, h: d.h, work: workAreaOf(d) });
const displaySnapshot = () => screen.displays;

const keyOf = (w) => dockIdOfWindow(w) || w.app;

export function WindowMemory() {
  const displays = useSyncExternalStore(onScreenChange, displaySnapshot);
  return h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList,
    args: { enabled: allowed(), displays: displays.map(displayFacts) },
    style: { position: 'absolute', left: 0, top: 0, width: 1, height: 1, pointerEvents: 'none' } });
}

export function rememberedFrame(key, opts = {}) {
  const places = Object.entries(settings.get().windowPlaces || {});
  if (!places.some(([id]) => id === key)) return null;
  const displays = [...screen.displays];
  const index = display => {
    if (!display) return null;
    const at = displays.indexOf(display);
    return at >= 0 ? at : displays.push(display) - 1;
  };
  const { main = mainDisplay(), assigned = assignedDisplayOf(key), ...rest } = opts;
  const options = { ...rest, main: index(main), assigned: index(assigned) };
  return rememberWindowFrame({ key, places, displays: displays.map(displayFacts), options });
}

export function startWindowMemory() {
  if (!allowed()) return;
  setWindowMemory((appId) => {
    const a = getApp(appId);
    return a && !a.builtin ? rememberedFrame(keyOf({ id: appId, app: appId }), { fixed: a.fixed === true, size: a.size }) : null;
  });
}
