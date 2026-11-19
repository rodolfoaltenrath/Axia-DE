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
// a theme without a menu bar can offer About in each window's Help menu. About <app> and Force Quit are system windows
// of their own (apps/appabout, apps/forcequit), the same in every theme.
import { windows, wm, appMenus, applyNativeWindowState, windowModelFacts } from './windows.js';
import { nativeCatalogFacts } from './nativecatalog.js';
import { sameFacts } from './nativeviewfacts.js';
import { openFileWithApps } from './sysopen.js';
import * as fs from './fs.js';
import { nativeAppRequest, nativeAppEffects, executeNativeEffect } from './nativeappmodel.js';
import { t } from './i18n.js';
import * as localControl from '../runtime/lpk/appcontrol.js';

// Compatibility API for SDK consumers that still expect mirror objects.
export function appGroup(winId) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.appGroup(winId);
  if (typeof winId !== 'string' || !winId) return null;
  const group = controlRequest({ cmd: 'group', id: winId }).group;
  return group ? { ...group, windows: group.windows.map(id => wm.get(id)).filter(Boolean) } : null;
}
export function quitAppGroup(identity = {}) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.quitAppGroup(identity);
  const owner = lord.nativeApps;
  let reply = controlRequest({ cmd: 'quitApp', ...identity });
  while (reply.effect) {
    const result = executeNativeEffect(reply.effect);
    reply = nativeAppRequest(owner, { cmd: 'quitAppNext', ticket: reply.ticket, result });
  }
  return reply.error || '';
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
export function hideApp(id) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.hideApp(id);
  windowCommand({ action: 'hide', id });
}
export function hideOthers(id) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.hideOthers(id);
  windowCommand({ action: 'hideOthers', id });
}
export function showAllApps() {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.showAllApps();
  windowCommand({ action: 'showAll' });
}
export function cycleAppWindows(back = false) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.cycleAppWindows(back);
  windowCommand({ action: 'cycle', back });
}
// The app's own Settings… item, if it has one (❖,).
export const appSettingsItem = settingsItemOf;

// Show desktop: everything visible on this desktop goes away, and the next time what it took away
// comes back (each desktop remembers its own).
export function toggleShowDesktop() {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.toggleShowDesktop();
  windowCommand({ action: 'desktop' });
}

export function appMenuItems(winId) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return localControl.appMenuItems(winId, settingsItemOf);
  if (typeof winId !== 'string' || !winId) return [];
  const g = controlRequest({ cmd: 'groupInfo', id: winId }).group;
  if (!g) return [];
  const settingsItem = settingsItemOf(winId);
  return [
    { label: t('shell.app.about', { app: g.name }), onClick: () => wm.open('appabout', { new: true, args: { key: g.key, win: winId } }) },
    '-',
    { label: t('shell.app.settings'), hint: '❖,', disabled: !settingsItem, onClick: () => settingsItem?.onClick() },
    '-',
    { label: t('shell.app.hide', { app: g.name }), hint: '❖H', onClick: () => hideApp(winId) },
    { label: t('shell.app.hideOthers'), hint: '❖⌥H', onClick: () => hideOthers(winId) },
    { label: t('shell.app.showAll'), onClick: showAllApps },
    '-',
    { label: t('shell.app.quit', { app: g.name }), hint: '❖Q', onClick: () => quitAppGroup({ id: winId }) },
  ];
}

let metadataOwner, sentMetadata;
function controlRequest(request) {
  const owner = lord.nativeApps, metadata = nativeCatalogFacts(true).get();
  if (metadataOwner !== owner || (sentMetadata !== metadata && !sameFacts(sentMetadata, metadata))) {
    const reply = nativeAppRequest(owner, { cmd: 'metadata', ...metadata });
    if (!reply.ok) throw new Error(reply.error || 'app.invalidRequest');
  }
  metadataOwner = owner; sentMetadata = metadata;
  return nativeAppRequest(owner, request);
}
function windowCommand(command) {
  const reply = controlRequest({ cmd: 'windowCommand', ...command });
  if (reply.error) throw new Error(reply.error);
  applyNativeWindowState(reply);
}
export function arrangeWindowMenu(id, layout) { windowCommand({ action: 'arrange', id, layout, facts: windowModelFacts() }); }
export function bringAllWindows(id) { windowCommand({ action: 'bringAll', id }); }
export function runningApps() {
  const snapshot = windows.get(), byId = new Map(snapshot.map(w => [String(w.id), w]));
  // Legacy JS callers still consume mirror objects. Native consumers use IDs
  // directly; omit entries whose mirror has not arrived yet, rather than undefined windows.
  return controlRequest({ cmd: 'groups' }).groups
    .map(group => ({ ...group, windows: group.windows.map(id => byId.get(id)).filter(Boolean) }))
    .filter(group => group.windows.length);
}
// Rust owns retry, deduplication and error aggregation. The compositor checks client credentials.
export function forceQuit(key) {
  return nativeAppEffects(lord.nativeApps, controlRequest({ cmd: 'quit', key }));
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
