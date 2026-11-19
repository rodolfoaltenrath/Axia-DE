// notify.js — session notifications (like GNOME notifications).
//
//   const id = notify({ app: 'backup', title: 'Backup done', body: '1234 files',
//                       actions: [{ id: 'open', label: 'Open', run: () => wm.open('backup') }],
//                       sound: true, sticky: false, timeout: 6000 })
//   dismissNotification(id)
//   useNotifications() → the banners on screen; notifyHistory (store) → this session's notifications
//
// <NotificationBanner /> is mounted by startShell, so it shows with any theme: at the top, on the
// end side of the line (right; left in Arabic or Hebrew), below the theme's bar (workArea). A
// notification goes away after `timeout` ms, except `sticky` ones (alarms), which wait until an
// action or the × is pressed. Clicking the banner runs the first action.
// The app passes texts already translated (t()). The history lives in memory only.
import { createStore, useStore } from './store.js';
import { getApp } from './windows.js';
import { sound } from './sound.js';
import { settings } from './settings.js';
import { perms } from './privacy.js';

const MAX_SHOWN = 4, HISTORY = 50;
export const notifications = createStore([]);        // on screen, newest first
export const notifyHistory = createStore([]);
let seq = 1;
const timers = new Map();

// ---- Settings → Notifications (docs/SETTINGS.md): per-app style, Do Not Disturb, grouping ----
//
// settings.notifications = { dnd: false | until (ms), schedule: { on, from, to } (local minutes),
//   grouping: 'app' | 'off', apps: { [app]: { allow, style: 'banner' | 'alert' | 'none', sound, lock } },
//   seen: [app ids that have notified] }
// app is an app id ('backup'), a package id, or 'linux:<desktop id or name>' for Linux apps
// (org.freedesktop.Notifications, server/src/hw/notifyd.c). For .lpk packages "allow" is the
// sandbox's "notifications" permission (the same switch as in Privacy & Security).
export const NOTIFY_DEFAULTS = { dnd: false, schedule: { on: false, from: 22 * 60, to: 7 * 60 }, grouping: 'app', apps: {}, seen: [] };
export const notifyPrefs = (s = settings.get()) => ({ ...NOTIFY_DEFAULTS, ...(s.notifications || {}) });
const isPackage = (app) => { try { return !!getApp(app)?.package; } catch (e) { return false; } };
export function appNotifyPrefs(app, s = settings.get()) {
  const p = { allow: true, style: 'banner', sound: true, lock: true, ...(notifyPrefs(s).apps[app] || {}) };
  if (isPackage(app)) p.allow = perms.get(app, 'notifications') !== 'deny';
  return p;
}
export function setAppNotifyPrefs(app, patch) {
  if (isPackage(app) && 'allow' in patch) perms.set(app, 'notifications', patch.allow ? 'allow' : 'deny');
  settings.set((s) => {
    const p = notifyPrefs(s);
    return { ...s, notifications: { ...p, apps: { ...p.apps, [app]: { ...(p.apps[app] || {}), ...patch } } } };
  });
}
export function dndActive(now = new Date(), s = settings.get()) {
  const p = notifyPrefs(s);
  if (p.dnd === true || (typeof p.dnd === 'number' && now.getTime() < p.dnd)) return true;
  if (!p.schedule?.on) return false;
  const m = now.getHours() * 60 + now.getMinutes(), len = (((p.schedule.to - p.schedule.from) % 1440) + 1440) % 1440;
  return len > 0 && (((m - p.schedule.from) % 1440) + 1440) % 1440 < len;
}
function remember(app) {
  const p = notifyPrefs();
  if (!app || p.seen.includes(app)) return;
  settings.set((s) => ({ ...s, notifications: { ...notifyPrefs(s), seen: [app, ...notifyPrefs(s).seen].slice(0, 64) } }));
}

// Firewall restriction "no notifications" (Settings → Privacy → Apps' network, docs/FIREWALL.md):
// checked in axia-server's rule store before anything is shown, above the per-app setting. A
// built-in app is "lordos", an isolated one (.lpk, Passwords) "lpk:<id>". Settings' own system
// warnings (battery) are exempt; Linux apps ('linux:…') are checked where their D-Bus call arrives.
function fwNotifyBlocked(app) {
  try {
    if (typeof lord === 'undefined' || !lord.fw || typeof lord.fw.blocks !== 'function') return false;
    if (!app || app === 'settings' || app.startsWith('linux:')) return false;
    const a = getApp(app);
    return !!lord.fw.blocks(a && (a.package || a.isolated) ? `lpk:${app}` : 'lordos', 'no-notifications');
  } catch (e) { return false; }
}

export function notify({ app = '', title = '', body = '', actions = [], sound: withSound = true, sticky = false, timeout = 6000, onClose, label = '', icon = null } = {}) {
  app = String(app);
  const ap = appNotifyPrefs(app);
  remember(app);
  if (!ap.allow || fwNotifyBlocked(app)) return 0;          // not delivered at all
  const id = seq++;
  const n = { id, app, label: String(label), Icon: typeof icon === 'function' ? icon : null, title: String(title), body: String(body), actions: Array.isArray(actions) ? actions.slice(0, 3) : [],
    sticky: !!sticky || ap.style === 'alert', time: Date.now(), onClose };
  notifyHistory.set((l) => [{ id, app: n.app, label: n.label, title: n.title, body: n.body, time: n.time, actions: n.actions }, ...l].slice(0, HISTORY));
  // Do Not Disturb or "no banner": kept in the history only, without a sound
  if (dndActive() || ap.style === 'none') return id;
  notifications.set((l) => [n, ...l].slice(0, MAX_SHOWN));
  if (withSound && ap.sound) sound.play('notify');
  if (!n.sticky) timers.set(id, setTimeout(() => dismissNotification(id), Math.max(1500, timeout)));
  return id;
}

export function dismissNotification(id, reason = 'closed') {
  clearTimeout(timers.get(id));
  timers.delete(id);
  const n = notifications.get().find((x) => x.id === id);
  if (!n) return;
  notifications.set((l) => l.filter((x) => x.id !== id));
  try { n.onClose?.(reason); } catch (e) { console.error('notify onClose:', e); }
}

export const useNotifications = () => useStore(notifications);

// Where the banners go: 'top' (default: below a menu bar) or 'bottom' (above a taskbar, for a theme
// that has one). Always on the end side of the line.
let placement = 'top';
export function setBannerPlacement(p) { placement = p === 'bottom' ? 'bottom' : 'top'; }
export const bannerPlacement = () => placement;
export const clearNotifyHistory = () => notifyHistory.set([]);

export function runAction(n, a) {
  dismissNotification(n.id, a.id || 'action');
  try { a.run?.(); } catch (e) { console.error('notify:', e); }
}

// Grouping by app (Settings → Notifications): the newest banner of each app, with "+N".
export function grouped(list) {
  if (notifyPrefs().grouping === 'off') return list;
  const out = [], seen = new Map();
  for (const n of list) {
    const k = n.app || n.label;
    if (seen.has(k)) { seen.get(k).more++; continue; }
    const g = { ...n, more: 0 };
    seen.set(k, g);
    out.push(g);
  }
  return out;
}
