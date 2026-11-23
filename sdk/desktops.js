// Desktops adapters (docs/DESKTOPS.md). The context-owned Rust service initializes and
// remembers desktops and theme hand-offs; the native indicator observes the same owner.
// This module routes shortcuts, persists settings effects and mounts native services.
//
//   nextDesktopBy(step), addDesktopAndShow(), closeActiveDesktop(), desktopName(id)
//   flowDesktopKeys, linuxDesktopKeys       the shortcut profiles' entries (sdk/shortcuts.js)
//   desktopSwipe(dir)                       a registerGestureActions action
//
// The animation: the windows arriving slide a little and fade in, selected and stepped in Rust
// (desktop_animation.rs and anim.rs): no JS and no relayout per frame, and none at all
// with Reduce motion.
import { createElement as h, useSyncExternalStore } from 'react';
import { mainDisplay, onScreenChange } from '@lordos/runtime';
import { t, direction } from './i18n.js';
import { settings } from './settings.js';
import { settingsBridge } from './nativesettings.js';
import { NativeView } from './nativeview.js';
import { Layer } from './layers.js';
import { wm, activeDesktop, desktopList, setActiveDesktop, setDesktops, createDesktop, closeDesktop, fxNodes, overviewShowing, visibleWindows, applyNativeWindowState } from './windows.js';
import { nativeAppRequest } from './nativeappmodel.js';
import { MAX_DESKTOPS } from './desktopmodel.js';
import { neighbor, arrowStep, swipeStep, cleanCount, cleanIndex } from './winmodel.js';
import { cycleWindows } from './trackpad.js';
import { nativeAnim, whenAnimDone } from './anim.js';
import { toggleOverview, toggleAppWindows } from './overview.jsx';

const NATIVE_DESKTOP_HUD = { id: 'shell:desktop-hud', shell: true, appearanceSource: 'native', fullTheme: true };
const hudHandle = { current: 0 };
const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
function sessionCommand(action, extra = {}) {
  const reply = nativeAppRequest(lord.nativeApps, { cmd: 'desktopSession', action, ...extra });
  if (reply.error) throw new Error(reply.error);
  applyNativeWindowState(reply);
  for (const effect of reply.effects || []) if (effect.kind === 'settingsState') settingsBridge.perform?.(effect);
  return reply.result;
}

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
function desktopCommand(action, step, extra = {}) {
  const reply = nativeAppRequest(lord.nativeApps, { cmd: 'desktopCommand', action, ...(step === undefined ? {} : { step }), ...extra });
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
  desktopCommand('step', step);
}
const stepArrow = (dir) => {
  if (appHost) return nextDesktopBy(arrowStep(dir, direction() === 'rtl'));
  sessionCommand('arrow', { direction: dir });
};

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

// A session move validates the current target and creates a new desktop atomically.
export function moveWindowToDesktop(id, desktop) {
  if (!appHost) return sessionCommand(desktop === undefined ? 'moveNew' : 'move', { id, desktop });
  const target = desktop === undefined ? createDesktop() : desktop;
  if (target != null) wm.moveToDesktop(id, target);
  return target;
}

// The window menu's Move to Desktop.
export function moveToDesktopItems(id) {
  const w = wm.get(id);
  const items = desktopList.get().filter((d) => d !== w?.desktop)
    .map((d) => ({ label: desktopName(d), onClick: () => moveWindowToDesktop(id, d) }));
  if (items.length) items.push('-');
  items.push({ label: t('shell.desktops.new'), disabled: !canAddDesktop(), onClick: () => moveWindowToDesktop(id) });
  return items;
}

// ---- shortcuts ----

export const ctrlArrowsOn = () => appHost ? settings.get().desktopArrows !== false : sessionCommand('arrowsEnabled');
const arrowDir = (e) => (e.code === 105 ? 'left' : 'right');
const onStep = (e) => stepArrow(arrowDir(e));

// Lord Flow. ⌃ arrows are word movement in a Linux app, so Settings → Keyboard can turn them off
// (then the 'flow' key table keeps them from nobody); ❖⌃ arrows always work.
export const flowDesktopKeys = [
  { id: 'overview', keys: ['Ctrl+ArrowUp'], run: () => toggleOverview(), when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.overview', group: 'desktops' },
  { id: 'appWindows', keys: ['Ctrl+ArrowDown'], run: () => toggleAppWindows(), when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.appWindows', group: 'desktops' },
  { id: 'switchDesktopCtrl', keys: ['Ctrl+ArrowLeft', 'Ctrl+ArrowRight'], show: ['⌃←→'], ...(appHost ? {run:onStep} : {native:true}), when: ctrlArrowsOn, listed: ctrlArrowsOn, label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
  { id: 'switchDesktopMeta', keys: ['Meta+Ctrl+ArrowLeft', 'Meta+Ctrl+ArrowRight'], show: ['❖⌃←→'], ...(appHost ? {run:onStep} : {native:true}), label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
];
export const flowGrabs = () => (ctrlArrowsOn() ? 'flow-desktops' : 'flow');

// The Linux/Windows-style shortcut profile.
export const linuxDesktopKeys = [
  { id: 'overview', keys: ['Meta+Tab'], run: () => toggleOverview(), label: 'shell.shortcuts.overview', group: 'desktops' },
  { id: 'newDesktop', keys: ['Meta+Ctrl+D'], ...(appHost ? {run:addDesktopAndShow} : {native:true}), label: 'shell.shortcuts.newDesktop', group: 'desktops' },
  { id: 'switchDesktop', keys: ['Meta+Ctrl+ArrowLeft', 'Meta+Ctrl+ArrowRight'], show: ['❖⌃←→'], ...(appHost ? {run:onStep} : {native:true}), label: 'shell.shortcuts.switchDesktop', group: 'desktops' },
  { id: 'closeDesktop', keys: ['Meta+Ctrl+F4'], ...(appHost ? {run:closeActiveDesktop} : {native:true}), label: 'shell.shortcuts.closeDesktop', group: 'desktops' },
];

// Trackpad (registerGestureActions): with one desktop a sideways swipe still switches windows.
export const desktopSwipe = (dir) => () => {
  if (!appHost) {
    sessionCommand('swipe', { direction: dir });
    return;
  }
  if (desktopList.get().length > 1) nextDesktopBy(swipeStep(dir, direction() === 'rtl'));
  else cycleWindows(dir);
};

// ---- the count, across sessions and theme switches ----

// A position in the list, created if it isn't there yet (a restored window comes before its desktop).
export function desktopAt(index) {
  if (!appHost) return sessionCommand('at', { index });
  const i = cleanIndex(index);
  while (desktopList.get().length <= i) if (createDesktop() == null) break;
  const list = desktopList.get();
  return list[Math.min(i, list.length - 1)];
}
export const activeIndex = () => appHost ? Math.max(0, positionOf(activeDesktop.get())) : sessionCommand('active');
export const indexOfDesktop = (id) => appHost ? Math.max(0, positionOf(id)) : sessionCommand('index', { id });

// A Linux window that stays connected across a theme switch comes back on its desktop.
export function desktopOfSurface(sid) {
  return appHost ? undefined : sessionCommand('surface', { surface: sid }) ?? undefined;
}
// themes.use: what the next JS context needs, by position (the ids are this context's).
export function desktopsForThemeSwitch() {
  return appHost ? { active: activeIndex(), surfaces: {} } : sessionCommand('theme');
}

let started = false;
// startShell, once, before anything opens a window.
export function startDesktops() {
  if (!appHost) {
    sessionCommand('start');
    return;
  }
  const s = settings.get();
  const list = Array.from({ length: cleanCount(s.desktopCount) }, (_, i) => i + 1);
  const was = s.themeDesktops;
  if (was) settings.set({ themeDesktops: null });
  setDesktops(list, list[Math.min(cleanIndex(was?.active), list.length - 1)]);
  started = true;
  shown = activeDesktop.get();
}
if (appHost) desktopList.subscribe(() => {
  if (started && settings.get().desktopCount !== desktopList.get().length) settings.set({ desktopCount: desktopList.get().length });
});

// ---- the brief indicator and the animation ----

function showHud() {
  if (hudHandle.current) lord.nativeViews.action(hudHandle.current, { action: ['show'] });
}

// Compatibility for isolated hosts. The main session's selection, bindings and completion
// belong to desktop_animation.rs; it never subscribes to this mirror or calls these JS effects.
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

let shown = appHost ? activeDesktop.get() : 0;
if (appHost) activeDesktop.subscribe(() => {
  const to = activeDesktop.get(), from = shown;
  shown = to;
  if (!started || to === from) return;
  const list = desktopList.get();
  animate(list.indexOf(to) >= list.indexOf(from) ? 1 : -1);
});

function desktopHud(args) {
  return h(Layer, { name: 'tooltip' },
    h(NativeView, { manifest: NATIVE_DESKTOP_HUD, stateSource: lord.windowList, handleRef: hudHandle, args,
      style: { position: 'absolute', left: 0, top: 0, right: 0, bottom: 0, pointerEvents: 'none' } }));
}
function LocalDesktopHud() {
  const d = useSyncExternalStore(onScreenChange, mainDisplay);
  return desktopHud({ display: { x: d.x, y: d.y, w: d.w, h: d.h } });
}
export function DesktopHud() {
  return appHost ? h(LocalDesktopHud) : null;
}
