// appcontrol.js — what a theme offers for the app in front and for every app at once (0.8.5):
//
//   appGroup(winId)        { key, name, windows } of the app that owns a window (grouped as the Dock does)
//   appMenuItems(winId)    the app menu: About <app>, Settings…, Hide <app>, Hide Others, Show All, Quit <app>
//   runningApps()          every app with a window, for Force Quit
//   forceQuit(key)         ends an app that stopped answering (see below)
//   recentItems(n)         the recent files as menu items (Recent Items in the crown menu)
//   storeUpdates()         how many package updates are waiting (the crown menu's Store item)
//
//   hideApp, hideOthers, toggleShowDesktop, appSettingsItem   what the keyboard shortcuts call too
//
// Lord Flow shows the app menu under the app's name in its menu bar and the rest in the crown menu;
// Lord Classic offers About in each window's Help menu. About <app> and Force Quit are system windows
// of their own (apps/appabout, apps/forcequit), the same in every theme.
import { windows, wm, getApp, appMenus, onActiveDesktop, activeDesktop } from './windows.js';
import { launcherIdOf, quitApp } from './dock.js';
import { linuxAppFor, shortAppName, windowAppName, windowIconFacts } from './apps.js';
import { openFileWithApps } from './sysopen.js';
import * as fs from './fs.js';
import { nativeAppRequest, nativeAppEffects } from './nativeappmodel.js';
import { t, appName } from './i18n.js';

// the theme's own panels; a Linux window also runs under a built-in app id, but it is an app
const isShellPanel = (w) => !!getApp(w.id)?.builtin && !String(w.id).startsWith('linux#');

function groupName(key, w) {
  const a = getApp(key);
  if (a) return appName(a);
  const lin = linuxAppFor(String(key).replace(/^desktop:/, ''));
  if (lin) return shortAppName(lin);
  return w ? windowAppName(w) : String(key);
}

export function appGroup(winId) {
  const w = wm.get(winId);
  if (!w) return null;
  const key = launcherIdOf(w);
  const wins = windows.get().filter((x) => launcherIdOf(x) === key && !isShellPanel(x));
  return { key, name: groupName(key, w), windows: wins };
}

// the app's own settings item: by convention the one with the ❖, shortcut (Passwords, Terminal)
function settingsItemOf(winId) {
  for (const g of appMenus.get()[winId] || []) {
    for (const it of g.items || []) {
      if (it && it !== '-' && it.hint === '❖,' && !it.disabled && typeof it.onClick === 'function') return it;
      for (const s of (it && it.submenu) || []) if (s && s !== '-' && s.hint === '❖,' && typeof s.onClick === 'function') return s;
    }
  }
  return null;
}

// Hide <app>: every window of the app goes away; Hide Others: every other app's on this desktop.
export function hideApp(winId) { appGroup(winId)?.windows.forEach((w) => wm.minimize(w.id)); }
export function hideOthers(winId) {
  const g = appGroup(winId);
  if (!g) return;
  const mine = new Set(g.windows.map((w) => w.id));
  windows.get().forEach((w) => { if (!mine.has(w.id) && !isShellPanel(w) && onActiveDesktop(w)) wm.minimize(w.id); });
}
// The app's own Settings… item, if it has one (❖,).
export const appSettingsItem = settingsItemOf;

// Show desktop: everything visible on this desktop goes away, and the next time what it took away
// comes back (each desktop remembers its own).
const hiddenByDesktop = new Map();
export function toggleShowDesktop() {
  const desk = activeDesktop.get();
  const vis = windows.get().filter((w) => !w.minimized && !w.pip && !isShellPanel(w) && onActiveDesktop(w));
  if (vis.length) { hiddenByDesktop.set(desk, vis.map((w) => w.id)); vis.forEach((w) => wm.minimize(w.id)); return; }
  (hiddenByDesktop.get(desk) || []).forEach((id) => { if (wm.get(id)?.desktop === desk) wm.raise(id); });
  hiddenByDesktop.delete(desk);
}

export function appMenuItems(winId) {
  const g = appGroup(winId);
  if (!g) return [];
  const settingsItem = settingsItemOf(winId);
  return [
    { label: t('shell.app.about', { app: g.name }), onClick: () => wm.open('appabout', { new: true, args: { key: g.key, win: winId } }) },
    '-',
    { label: t('shell.app.settings'), hint: '❖,', disabled: !settingsItem, onClick: () => settingsItem?.onClick() },
    '-',
    { label: t('shell.app.hide', { app: g.name }), hint: '❖H', onClick: () => hideApp(winId) },
    { label: t('shell.app.hideOthers'), hint: '❖⌥H', onClick: () => hideOthers(winId) },
    { label: t('shell.app.showAll'), onClick: () => windows.get().forEach((w) => { if (w.minimized) wm.raise(w.id); }) },
    '-',
    { label: t('shell.app.quit', { app: g.name }), hint: '❖Q', onClick: () => quitApp(g.windows) },
  ];
}

// Host facts resolve registry/localized names and window handles. Rust owns filtering and grouping.
export function appControlFacts() {
  return windows.get().map(w => {
    const icon = String(w.id).startsWith('linux#') ? linuxAppFor(w.args?.appId) : getApp(w.id);
    return { id: String(w.id), app: w.app, builtin: !!getApp(w.id)?.builtin,
      key: launcherIdOf(w), name: groupName(launcherIdOf(w), w), surface: w.args?.surface,
      iconApp: icon?.id, iconPath: typeof icon?.icon === 'string' ? icon.icon : '',
      x11: !!w.args?.x11, isolated: !!w.args?.pkg, iconData: windowIconFacts(w) };
  });
}
export function runningApps() {
  const snapshot = windows.get(), byId = new Map(snapshot.map(w => [String(w.id), w]));
  return nativeAppRequest(lord.nativeApps, { cmd: 'groups', windows: appControlFacts() }).groups
    .map(group => ({ ...group, windows: group.windows.map(id => byId.get(id)) }));
}
// Rust owns retry, deduplication and error aggregation. The compositor checks client credentials.
export function forceQuit(key) {
  return nativeAppEffects(lord.nativeApps, nativeAppRequest(lord.nativeApps, { cmd: 'quit', key, windows: appControlFacts() }));
}

export function recentItems(n = 10) {
  let list = [];
  try { list = fs.recent(n, 30) || []; } catch (e) { list = []; }
  if (!list.length) return [{ label: t('shell.recent.none'), disabled: true }];
  return list.slice(0, n).map((f) => ({ label: f.name || fs.basename(f.path), onClick: () => openFileWithApps(f.path, f.name) }));
}

export function storeUpdates() {
  try { return typeof lord !== 'undefined' && lord.pkg && typeof lord.pkg.upgrades === 'function' ? (lord.pkg.upgrades() || []).length : 0; } catch (e) { return 0; }
}
