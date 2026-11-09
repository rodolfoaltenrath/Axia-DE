// Shared firewall transport for shell status items; the Monitor application is Rust-owned.
import { createStore } from './store.js';
import { codeError } from './errors.js';

// ---- per-app firewall (lord.fw, docs/FIREWALL.md) ----
// axia-netperm (root) applies the rules in the kernel and counts each app's new connections;
// axia-server passes them to JS. firewallState: { status, counts: { app: [allowed, denied, rx, tx,
// rxRate, txRate, [[addr, rx, tx, rxRate, txRate, blocked], ...], blocked] } (the bytes when
// status.bytes; blocked: bytes of open connections dropped because a rule now denies them),
// events: [the last 200 new connections], rev }. rev goes up when a rule changes.
const fwApi = () => (typeof lord !== 'undefined' && lord.fw) || null;
export const firewallState = createStore({ status: null, counts: {}, events: [], rev: 0 });
let fwLast = 0;
function fwPoll(force) {
  const api = fwApi();
  if (!api) return;
  const now = Date.now();
  if (!force && now - fwLast < 1000) return;
  fwLast = now;
  try {
    const p = api.poll();
    const status = api.status();
    const at = Date.now();
    firewallState.set((s) => ({ ...s, status, counts: p.counts || {}, events: p.events.length ? [...s.events, ...p.events.map((e) => ({ ...e, at }))].slice(-200) : s.events }));
  } catch (e) { /* no lord.fw */ }
}
const fwBump = (r) => { firewallState.set((s) => ({ ...s, rev: s.rev + 1 })); fwPoll(true); return r; };
const fwCall = (fn, dflt) => { try { const api = fwApi(); return api ? fn(api) : dflt; } catch (e) { return dflt; } };
// A rule can only be applied to an app with its own scope: LordOS, a .desktop app, an .lpk or (0.8.5)
// one of LordOS's own components with its own scope (lordos:mail, lordos:store, lordos:print,
// lordos:terminal, lordos:lists). The essential system services (system:dns, system:time,
// system:network) are shown but never take a rule.
const fwEnforceable = (app) => app === 'lordos' || /^(desktop|lpk|lordos):/.test(app || '');
const fwCallThrow = (fn) => { const api = fwApi(); if (!api) throw codeError('fw.list.unavailable'); return fn(api); };
export const firewall = {
  available: () => !!fwApi(),
  enforceable: fwEnforceable,
  status: () => fwCall((f) => f.status(), null),
  state: () => fwCall((f) => f.state(), { mode: 'silent', trusted: [], restricted: {} }),
  appRule: (app) => (fwEnforceable(app) ? fwCall((f) => f.appRule(app), null) : null),
  setAppRule: (app, v) => fwBump(fwCall((f) => f.setAppRule(app, v), false)),
  setTrusted: (app, on) => fwBump(fwCall((f) => f.setTrusted(app, !!on), false)),
  setRestricted: (app, flags) => fwBump(fwCall((f) => f.setRestricted(app, flags), false)),
  setMode: (m) => fwBump(fwCall((f) => f.setMode(m), false)),
  blocks: (app, flag) => fwCall((f) => f.blocks(app, flag), false),
  rules: (app) => fwCall((f) => f.rules(app), []),
  addRule: (r) => fwBump(fwCall((f) => f.addRule(r), false)),
  removeRule: (id) => fwBump(fwCall((f) => f.removeRule(id), false)),
  updateRule: (id, r) => fwBump(fwCall((f) => f.updateRule(id, r), false)),
  ruleApps: () => fwCall((f) => (f.ruleApps ? f.ruleApps() : []), []),
  // (0.8.5) subscribed blocklists (docs/FIREWALL.md, "Blocklists"): lists() → { lists: [{ id, url, format,
  // custom, on, updated, tried, domains, nets, truncated, error }], busy, domains, nets } | null
  lists: () => fwCall((f) => (f.lists ? f.lists() : null), null),
  setList: (id, on) => fwBump(fwCallThrow((f) => f.setList(id, !!on))),
  addList: (url, format) => fwBump(fwCallThrow((f) => f.addList(url, format))),
  removeList: (id) => fwBump(fwCallThrow((f) => f.removeList(id))),
  updateLists: (force) => fwCall((f) => (f.updateLists ? f.updateLists(!!force) : 1), 1),
  listMatch: (names, addr) => fwCall((f) => (f.listMatch ? f.listMatch(names || [], addr || '') : null), null),
  // the app whose rules Settings → Privacy shows open (the Monitor's "Rules for this app")
  focus: createStore(null),
  refresh: () => fwPoll(true),
  RESTRICTIONS: ['no-internet', 'no-lan', 'no-network', 'no-incoming', 'no-camera', 'no-mic', 'no-screen', 'no-usb', 'no-bluetooth', 'no-files', 'no-clipboard', 'no-notifications'],
};
