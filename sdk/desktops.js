// desktops.js — Desktops, the part with a runtime (docs/DESKTOPS.md). The rules are in desktopmodel.js
// and the stores in windows.js; this module switches, keeps the count, draws the brief indicator and
// runs the switch animation.
//
//   nextDesktopBy(step), addDesktopAndShow(), closeActiveDesktop(), desktopName(id)
//   flowDesktopKeys, linuxDesktopKeys       the shortcut profiles' entries (sdk/shortcuts.js)
//   desktopSwipe(dir)                       a registerGestureActions action
//
// The animation: the windows arriving slide a little and fade in, stepped by the native animator
// (lord.anim.slide, rust/axia-shell/src/anim.rs): no JS and no relayout per frame, and none at all
// with Reduce motion.
import { createElement as h, useSyncExternalStore } from 'react';
import { mainDisplay, onScreenChange } from '@lordos/runtime';
import { t, direction } from './i18n.js';
import { settings } from './settings.js';
import { NativeView } from './nativeview.js';
import { Layer } from './layers.js';
import { windows, wm, activeDesktop, desktopList, setActiveDesktop, setDesktops, createDesktop, closeDesktop, fxNodes, overviewShowing, visibleWindows, applyNativeWindowState } from './windows.js';
import { nativeAppRequest } from './nativeappmodel.js';
import { MAX_DESKTOPS } from './desktopmodel.js';
import { neighbor, arrowStep, swipeStep, cleanCount, cleanIndex } from './winmodel.js';
import { cycleWindows } from './trackpad.js';
import { nativeAnim, whenAnimDone } from './anim.js';
import { toggleOverview, toggleAppWindows } from './overview.jsx';

const NATIVE_DESKTOP_HUD = { id: 'shell:desktop-hud', shell: true, fullTheme: true };
const hudHandle = { current: 0 };

// ---- names and positions ----

const positionOf = (id) => desktopList.get().indexOf(id);
export const desktopName = (id) => t('shell.desktops.name', { n: positionOf(id) + 1 });
// " · Desktop 2" for a window on another desktop, in lists that show every desktop.
export function desktopSuffix(w) {
  const list = desktopList.get();
  if (list.length < 2 || !w || w.pip || w.desktop === activeDesktop.get()) return '';
  return ` · ${desktopName(w.desktop)}`;
}

// ---- switching ----

export function switchDesktop(id) { setActiveDesktop(id); }
function desktopCommand(action, step) {
  const reply = nativeAppRequest(lord.nativeApps, { cmd: 'desktopCommand', action, ...(step === undefined ? {} : { step }) });
  if (reply.error) throw new Error(reply.error);
  applyNativeWindowState(reply);
  return reply.result;
}
export function nextDesktopBy(step) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) {
    const id = neighbor(desktopList.get(), activeDesktop.get(), step);
    if (id != null) switchDesktop(id); else showHud();
    return;
  }
  if (desktopCommand('step', step)?.boundary) showHud();
}
const stepArrow = (dir) => nextDesktopBy(arrowStep(dir, direction() === 'rtl'));

export function addDesktopAndShow() {
  if (!(typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__)) return desktopCommand('add');
  const id = createDesktop();
  if (id != null) switchDesktop(id);
  return id;
}
export const canAddDesktop = () => desktopList.get().length < MAX_DESKTOPS;
export function removeDesktopById(id) { return closeDesktop(id); }
export const closeActiveDesktop = () => typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__
  ? closeDesktop(activeDesktop.get()) : desktopCommand('closeActive');

// The window menu's Move to Desktop.
export function moveToDesktopItems(id) {
  const w = wm.get(id);
  const items = desktopList.get().filter((d) => d !== w?.desktop)
    .map((d) => ({ label: desktopName(d), onClick: () => wm.moveToDesktop(id, d) }));
  if (items.length) items.push('-');
  items.push({ label: t('shell.desktops.new'), disabled: !canAddDesktop(), onClick: () => { const d = createDesktop(); if (d != null) wm.moveToDesktop(id, d); } });
  return items;
}

// ---- shortcuts ----

export const ctrlArrowsOn = () => settings.get().desktopArrows !== false;
const arrowDir = (e) => (e.code === 105 ? 'left' : 'right');
const onStep = (e) => stepArrow(arrowDir(e));

// Lord Flow. ⌃ arrows are word movement in a Linux app, so Settings → Keyboard can turn them off
// (then the 'flow' key table keeps them from nobody); ❖⌃ arrows always work.
export const flowDesktopKeys = [
  { id: 'overview', keys: ['Ctrl+ArrowUp'], run: () => toggleOverview(), when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.overview', group: 'desktops' },
  { id: 'appWindows', keys: ['Ctrl+ArrowDown'], run: () => toggleAppWindows(), when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.appWindows', group: 'desktops' },
  { id: 'switchDesktopCtrl', keys: ['Ctrl+ArrowLeft', 'Ctrl+ArrowRight'], show: ['⌃←→'], run: onStep, when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
  { id: 'switchDesktopMeta', keys: ['Meta+Ctrl+ArrowLeft', 'Meta+Ctrl+ArrowRight'], show: ['❖⌃←→'], run: onStep, label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
];
export const flowGrabs = () => (ctrlArrowsOn() ? 'flow-desktops' : 'flow');

// The Linux/Windows-style shortcut profile.
export const linuxDesktopKeys = [
  { id: 'overview', keys: ['Meta+Tab'], run: () => toggleOverview(), label: 'shell.shortcuts.overview', group: 'desktops' },
  { id: 'newDesktop', keys: ['Meta+Ctrl+D'], run: addDesktopAndShow, label: 'shell.shortcuts.newDesktop', group: 'desktops' },
  { id: 'switchDesktop', keys: ['Meta+Ctrl+ArrowLeft', 'Meta+Ctrl+ArrowRight'], show: ['❖⌃←→'], run: onStep, label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
  { id: 'closeDesktop', keys: ['Meta+Ctrl+F4'], run: closeActiveDesktop, label: 'shell.shortcuts.closeDesktop', group: 'desktops' },
];

// Trackpad (registerGestureActions): with one desktop a sideways swipe still switches windows.
export const desktopSwipe = (dir) => () => {
  if (desktopList.get().length > 1) nextDesktopBy(swipeStep(dir, direction() === 'rtl'));
  else cycleWindows(dir);
};

// ---- the count, across sessions and theme switches ----

let surfaceDesktops = null;       // theme switch: Linux surface id → position in the list

// A position in the list, created if it isn't there yet (a restored window comes before its desktop).
export function desktopAt(index) {
  const i = cleanIndex(index);
  while (desktopList.get().length <= i) if (createDesktop() == null) break;
  const list = desktopList.get();
  return list[Math.min(i, list.length - 1)];
}
export const activeIndex = () => Math.max(0, positionOf(activeDesktop.get()));
export const indexOfDesktop = (id) => Math.max(0, positionOf(id));

// A Linux window that stays connected across a theme switch comes back on its desktop.
export function desktopOfSurface(sid) {
  const i = surfaceDesktops?.[sid];
  return Number.isInteger(i) ? desktopAt(i) : undefined;
}
// themes.use: what the next JS context needs, by position (the ids are this context's).
export function desktopsForThemeSwitch() {
  const surfaces = {};
  for (const w of windows.get()) if (w.args?.surface) surfaces[w.args.surface] = indexOfDesktop(w.desktop);
  return { active: activeIndex(), surfaces };
}

let started = false;
// startShell, once, before anything opens a window.
export function startDesktops() {
  const s = settings.get();
  const list = Array.from({ length: cleanCount(s.desktopCount) }, (_, i) => i + 1);
  const was = s.themeDesktops;
  if (was) { surfaceDesktops = was.surfaces || null; settings.set({ themeDesktops: null }); }
  setDesktops(list, list[Math.min(cleanIndex(was?.active), list.length - 1)]);
  started = true;
  shown = activeDesktop.get();
}
desktopList.subscribe(() => {
  if (started && settings.get().desktopCount !== desktopList.get().length) settings.set({ desktopCount: desktopList.get().length });
});

// ---- the brief indicator and the animation ----

function showHud() {
  if (hudHandle.current) lord.nativeViews.action(hudHandle.current, { action: ['show'] });
}

// Switching desktops: the windows that appear slide in, natively (lord.anim.slide,
// rust/axia-shell/src/anim.rs): no JS and no relayout per frame; their own values come back at the end.
const sliding = new Set();                          // the nodes the last switch animates
function animate(step) {
  const anim = nativeAnim();
  for (const n of sliding) anim?.clear(n);
  sliding.clear();
  if (!anim || settings.get().reduceMotion || overviewShowing.get()) return;
  const rtl = direction() === 'rtl';
  for (const n of visibleWindows().map((w) => fxNodes.get(w.id)).filter(Boolean)) {
    const id = anim.slide(n, step, rtl);
    if (!id) continue;
    sliding.add(n);
    whenAnimDone(id, () => { if (sliding.delete(n)) anim.clear(n); });
  }
}

let shown = activeDesktop.get();
activeDesktop.subscribe(() => {
  const to = activeDesktop.get(), from = shown;
  shown = to;
  if (!started || to === from) return;
  const list = desktopList.get();
  animate(list.indexOf(to) >= list.indexOf(from) ? 1 : -1);
});

export function DesktopHud() {
  const d = useSyncExternalStore(onScreenChange, mainDisplay);
  return h(Layer, { name: 'tooltip' },
    h(NativeView, { manifest: NATIVE_DESKTOP_HUD, stateSource: lord.windowList, handleRef: hudHandle,
      args: { display: { x: d.x, y: d.y, w: d.w, h: d.h } },
      style: { position: 'absolute', left: 0, top: 0, right: 0, bottom: 0, pointerEvents: 'none' } }));
}
