// windowmemory.js — each app reopens where it was (docs/WINDOWS.md, "Remembered places"): size, place, maximized and which
// display, per app, in settings.windowPlaces. Session restore's frames come first; an assignment in the Dock chooses the display.
// A Linux app's own request doesn't override this, except a dialog or a window of fixed size (sdk/wayland.js).
import { screen, mainDisplay } from '@lordos/runtime';
import { settings } from './settings.js';
import { workAreaOf, getApp, setWindowMemory, assignedDisplayOf, isLiveSystem } from './windows.js';
import { dockIdOfWindow } from './dock.js';
import { rememberWindowFrame } from './appcontrol.js';
import { syncNativeCatalog } from './nativecatalog.js';
import { nativeAppRequest } from './nativeappmodel.js';

const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
const allowed = () => typeof lord !== 'undefined' && typeof lord.appId !== 'string'
  && typeof lord.greeter === 'undefined' && !isLiveSystem();
const displayFacts = d => ({ id: d.id, name: d.name, connector: d.connector,
  x: d.x, y: d.y, w: d.w, h: d.h, work: workAreaOf(d) });

const keyOf = (w) => dockIdOfWindow(w) || w.app;

export function rememberedFrame(key, opts = {}) {
  const places = appHost ? Object.entries(settings.get().windowPlaces || {}) : null;
  if (appHost && !places.some(([id]) => id === key)) return null;
  const displays = [...screen.displays];
  const index = display => {
    if (!display) return null;
    const at = displays.indexOf(display);
    return at >= 0 ? at : displays.push(display) - 1;
  };
  const { main = mainDisplay(), assigned = assignedDisplayOf(key), ...rest } = opts;
  const options = { ...rest, main: index(main), assigned: index(assigned) };
  return rememberWindowFrame({ key, ...(appHost ? { places } : {}), displays: displays.map(displayFacts), options });
}

let started = false;
export function startWindowMemory() {
  if (appHost || !allowed() || started) return;
  started = true;
  nativeAppRequest(syncNativeCatalog(), { cmd: 'memoryStart', enabled: true });
  setWindowMemory((appId) => {
    const a = getApp(appId);
    return a && !a.builtin ? rememberedFrame(keyOf({ id: appId, app: appId }), { fixed: a.fixed === true, size: a.size }) : null;
  });
}
