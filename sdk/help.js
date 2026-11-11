// help.js — articles of the Help app (assets/help/<lang>/<id>.md) and the welcome tour.
//
//   help.list()            → [{ id, title, app, section, order, keywords }] in the interface language
//   help.read(id)          → { id, title, app, section, keywords, body } or null
//   help.open(id)          → opens Help on that article
//   help.search(text)      → [{ id, title, score, snippet }]
//   help.startTour()       → opens "Welcome to LordOS"
//
// English is the default language: every article exists in assets/help/en, and es/pt (or any other
// language) are translations. The list is the English one plus any extra articles of the interface
// language; each article is read in the interface language and falls back to English when that
// language does not have it.
//
// Each article is markdown with a header:
//   ---
//   title: Clock
//   app: clock              (the app it explains, for the "Open" button)
//   section: apps           (sidebar group: start, desktop, apps, system, settings)
//   order: 10
//   keywords: alarm, timer
//   ---
// Special links in the text: [Open Settings → Network](settings:network), [Files](app:files)
// and [another article](help:spotlight). Global search reads articles on its Rust worker.
// Rust reads them through lord.help (server/src/ui/js_help.c), only from Axia's assets/help folder.
import { locale } from './i18n.js';
import { settings } from './settings.js';
import { wm, getApp, isLiveSystem } from './windows.js';

const DEFAULT_LANG = 'en';
const has = () => typeof lord.help?.request === 'function';
const langOf = () => String(locale.get().lang || DEFAULT_LANG).split('-')[0];
// The shell keeps its public API; catalog policy, parsing, fallback, ranking and
// snippets run in Rust. Only window management belongs on this side of the bridge.
function request(cmd, args = {}) {
  if (!has()) return null;
  return lord.help.request({ cmd, lang: langOf(), ...args });
}

export const help = {
  available: has,
  list() { return request('list') || []; },
  read(id) { return request('read', { id: String(id || '') }); },
  open(id) { wm.open('help', { args: { article: id } }); },
  startTour() { wm.open('help', { args: { tour: true } }); },
  search(query) { return request('search', { query: String(query || '') }) || []; },
  // Rust validates internal links; this adapter runs their desktop effects.
  link(url) {
    const target = request('link', { url: String(url || '') });
    if (!target) return false;
    if (target.app === 'settings' || target.app === 'help' || getApp(target.app)) {
      wm.open(target.app, target.args ? { args: target.args } : undefined);
    }
    return true;
  },
};

// Global search keeps its asynchronous Rust worker; the Help API above uses
// the Rust content service on the UI thread, with bounded article caches.

// First login: "Welcome to LordOS", only once (settings.welcomeShown). On the live system the
// installer opens instead.
export function maybeStartTour() {
  if (settings.get().welcomeShown || isLiveSystem() || !getApp('help') || !has()) return;
  settings.set({ welcomeShown: true });
  setTimeout(() => help.startTour(), 1200);
}

// when the language changes, everything is read again
locale.subscribe(() => { request('clear'); });
