// settings.js — user settings, stored in ~/.config/axia/settings.json.
import { createStore, useStore } from './store.js';
import defaults from './settings-defaults.json';
import { settingsBridge } from './nativesettings.js';
import { localeBridge } from './nativelocale.js';
const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
function nativeSettings(input) {
  const reply = lord.settings.state(input);
  if (!reply || reply.error) throw new Error(reply?.error || 'settings.invalidRequest');
  return reply;
}

const DEFAULTS = defaults;

// Per-user settings migrations (docs/UPDATES.md, "Migrations"): when an update changes the shape or
// meaning of a setting, update rust/axia-shell/src/settings.rs and the isolated SDK table below.
// The main session delegates migrations to Rust; this table preserves the isolated contract.
// Each migration runs once per saved schema,
// in order, when the session loads the file; "schema" records the last one applied. The system-wide
// migrations (as root) are the lordos package's (packaging/migrations).
export const SETTINGS_SCHEMA = 3;
const MIGRATIONS = [
  // Remove preferences for retired desktop features; user source files remain untouched.

  [1, (s) => s],          // 0.8.1: the schema number starts being recorded; nothing changes
  [2, (s) => ({ ...s, externalWindowStyle: false, nativeAppearance: s.filesAppearance || 'adwaita' })], // retire the old mac-style adapters once
  [3, (s) => Object.fromEntries(Object.entries(s).filter(([key]) => ![
    'motion', 'motionPaths', 'motionFiles', 'motionUser', 'motionRate', 'remoteDesks', 'loginWallpaper',
  ].includes(key)))],
];
export function migrateSettings(s) {
  if (!appHost) return reuse(s, nativeSettings({ cmd: 'migrate', value: s }).value);
  let out = s;
  for (const [to, fn] of MIGRATIONS) if ((out.schema || 0) < to) out = { ...fn(out), schema: to };
  return out;
}

function load() {
  try {
    const txt = lord.settings.load();
    return txt ? migrateSettings({ ...DEFAULTS, ...JSON.parse(txt) }) : { ...DEFAULTS, schema: SETTINGS_SCHEMA };
  } catch (e) {
    console.warn('unreadable settings, using the defaults:', e);
    return { ...DEFAULTS };
  }
}

function initializeNative() {
  const reply = nativeSettings({ cmd: 'initialize' });
  if (reply.invalid) console.warn('unreadable settings, using the defaults');
  return reply;
}
const initial = appHost ? null : initializeNative();
const mirror = createStore(appHost ? load() : initial.state);
let nativeRevision = initial?.revision ?? 0;
function reuse(previous, next) {
  if (Object.is(previous, next)) return previous;
  if (!previous || !next || typeof previous !== 'object' || typeof next !== 'object'
    || Array.isArray(previous) !== Array.isArray(next)) return next;
  const keys = Object.keys(next);
  let same = keys.length === Object.keys(previous).length;
  for (const key of keys) {
    const own = Object.hasOwn(previous, key);
    if (own) next[key] = reuse(previous[key], next[key]);
    if (!own || !Object.is(previous[key], next[key])) same = false;
  }
  return same ? previous : next;
}
function adopt(reply) {
  if (!reply) return;
  if (reply.revision > nativeRevision) {
    nativeRevision = reply.revision;
    mirror.set(() => reuse(mirror.get(), reply.state));
  }
  if (reply.localeState) localeBridge.perform?.({ kind: 'localeState', reply: reply.localeState });
}
export const settings = appHost ? mirror : {
  get: mirror.get, subscribe: mirror.subscribe,
  set(change) {
    const reply = typeof change === 'function'
      ? nativeSettings({ cmd: 'replace', value: change(nativeSettings({ cmd: 'snapshot' }).state) })
      : nativeSettings({ cmd: 'patch', patch: change });
    adopt(reply);
  },
};
settingsBridge.perform = effect => { if (effect.kind === 'settingsState') adopt(effect.reply); };
let preferenceRevision = 0;
export const settingsRevision = () => appHost ? preferenceRevision : nativeRevision;
settings.subscribe(() => { preferenceRevision++; });
// Developer timing (settings.json "debugTiming": true, no UI): opening an app logs how long its
// first window took (sdk/components.jsx AppHost, sdk/packages.js isolatedApp, docs/PRIVACY.md).
globalThis.__lordTiming = !!settings.get().debugTiming;

let pending = 0;
if (appHost) {
  settings.subscribe(() => {
    clearTimeout(pending);
    pending = setTimeout(() => lord.settings.save(JSON.stringify(settings.get(), null, 2)), 300);
  });
}

// Now, not in 300 ms; durable: on the disk before this returns (session restore's marker must survive
// a power cut). Blocks the frame for that write, so it's only for that.
export function saveSettingsNow(durable = false) {
  if (!appHost) return nativeSettings({ cmd: 'save', durable: !!durable }).saved;
  clearTimeout(pending);
  return lord.settings.save(JSON.stringify(settings.get(), null, 2), durable);
}

export function useSettings() {
  return [useStore(settings), settings.set];
}
