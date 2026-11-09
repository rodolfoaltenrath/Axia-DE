// windows.js — window manager for LordOS apps (independent of the theme).
// The theme decides how they look; the SDK decides which windows exist, their order and geometry.
//
// Each window has its own `id` and the `app` that shows it. An app can have several
// windows (wm.open('terminal', { new: true }) → id 'terminal#2'), a dynamic title
// (wm.setTitle), confirmation on close (wm.setCloseGuard) and its own menus (wm.setMenus)
// that the theme shows in its menu bar.
import { screen, onScreenChange, addListener, mainDisplay, displayAt, displayFor } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { appName } from './i18n.js';
import { FIRST_DESKTOP } from './desktopmodel.js';

// Snapping's geometry is Rust's (lord.snap, server/src/ui/js_snap.c). Isolated apps' processes don't
// have it: there everything about snapping is a no-op.
const snapApi = () => (typeof lord !== 'undefined' && lord.snap) || null;
export const SNAP_ZONES = Object.freeze([...(snapApi()?.zones || [])]);
// The rectangle of `zone` in work area `a`, or null.
export const snapRect = (zone, a) => snapApi()?.rect(zone, a) || null;

let registry = new Map();
let builtins = [];
let themeApps = [];
let packageApps = [];
// Changes when an app is installed or removed (to redraw the app launcher).
export const appRegistry = createStore(0);
const rebuild = () => {
  registry = new Map([...themeApps, ...packageApps, ...builtins].map((a) => [a.id, a]));
  appRegistry.set((v) => v + 1);
};
// Work area of each display (docs/DISPLAYS.md): the theme gives a function display → rect (menu bar
// and Dock margins), or the main display's rect as before; other displays then use all of theirs.
let areaFn = null;
let area = { x: 0, y: 0, w: screen.width, h: screen.height };
let chrome = { w: 0, h: 0 };
// The display under the pointer: new windows open there.
const pointer = { x: -1, y: -1 };
addListener('pointermove', (e) => { pointer.x = e.x; pointer.y = e.y; });
addListener('pointerdown', (e) => { pointer.x = e.x; pointer.y = e.y; });
export function workAreaOf(d) {
  d = d || mainDisplay();
  if (areaFn) return areaFn(d);
  return d.main ? area : { x: d.x, y: d.y, w: d.w, h: d.h };
}
const pointerDisplay = () => (pointer.x >= 0 ? displayAt(pointer.x, pointer.y) : mainDisplay());
// Work area of the display under the pointer (where a new window opens).
export const pointerWorkArea = () => workAreaOf(pointerDisplay());
// The display an app's new windows open on: the one the user assigned it to in the Dock (Options ›
// Assign to, sdk/dock.js sets the hook), else the one under the pointer.
let assignHook = null;
export function setDisplayAssignment(fn) { assignHook = fn; }
export function openingArea(appKey) {
  const id = appKey && assignHook ? assignHook(appKey) : null;
  const d = id != null ? screen.displays.find((x) => x.id === id) : null;
  return workAreaOf(d || pointerDisplay());
}
export function assignedDisplayOf(appKey) {
  const id = appKey && assignHook ? assignHook(appKey) : null;
  return id != null ? screen.displays.find((x) => x.id === id) || null : null;
}
// Where an app was the last time (sdk/windowmemory.js)
let memory = null;
export function setWindowMemory(fn) { memory = fn; }
const areaOfWindow = (w) => workAreaOf(displayFor(w));
// Window id → the node of its frame inside WindowShell (the Desktops animation, sdk/desktops.js).
export const fxNodes = new Map();
// Session restore (sdk/sessionrestore.jsx): a new Linux window of a Dock id may have a saved place.
let linuxPlacement = null;
export function setLinuxPlacement(fn) { linuxPlacement = fn; }
export const placeLinuxWindow = (dockId) => (linuxPlacement ? linuxPlacement(dockId) : null);
const guards = new Map();

// ---- the window list, in Rust (lord.windowList: rust/axia-shell/src/winmodel/session.rs) ----
// Rust keeps the windows (bottom to top: the last one is in front), the desktops and the one showing,
// and every rule that changes them; it also works out what shows, what is mounted and what is in
// front, once per change. These stores are its mirror, for React: read them, change them with wm.*.
// Each window has a revision, and only the objects that changed are replaced, so React keeps the rest.
// Read-only for everyone else: a write here would only part the mirror from Rust's list.
const readOnly = (store, name, how) => ({
  get: store.get, subscribe: store.subscribe,
  set() { console.warn(`${name} is Rust's (winmodel/session.rs): change it with ${how}`); },
});
const windowsStore = createStore([]);
export const windows = readOnly(windowsStore, 'windows', 'wm.*');
// Desktops (docs/DESKTOPS.md): each window has w.desktop, and only the windows of activeDesktop show,
// take the front place and get the keyboard. All displays switch together. desktopList is the order on
// screen; sdk/desktops.js switches, persists and animates.
const activeStore = createStore(FIRST_DESKTOP), listStore = createStore([FIRST_DESKTOP]);
export const activeDesktop = readOnly(activeStore, 'activeDesktop', 'setActiveDesktop');
export const desktopList = readOnly(listStore, 'desktopList', 'createDesktop/closeDesktop/setDesktops');
// The overview shows this desktop's minimized windows too (sdk/overview.jsx turns it on).
export const overviewShowing = { get: () => overviewOn.get(), subscribe: (fn) => overviewOn.subscribe(fn), set: (on) => { run('setOverview', !!on); } };
const overviewOn = createStore(false);
// What the theme draws, from Rust: { visible, mounted, front, top, hidden, parked, placeholder, desk }
// (ids; desk: desktop → the ids on it, picture-in-picture on all of them).
const NO_VIEWS = { visible: [], mounted: [], front: null, top: null, hidden: [], parked: [], placeholder: [], desk: {} };
export const windowViews = createStore({ ...NO_VIEWS, sets: { hidden: new Set(), parked: new Set(), placeholder: new Set(), desk: new Map() } });

const mirror = new Map();           // id → the window object last received
const revs = new Map();             // id → its revision
const rect = (r) => ({ x: r.x, y: r.y, w: r.w, h: r.h });
// What the rules need that only this side knows: the displays with the theme's work areas (in
// lord.screen's order: an index into it names a display), the open apps' sizes, and whether the theme snaps.
function facts() {
  const apps = {};
  for (const w of windows.get()) {
    const a = registry.get(w.app);
    if (a && !apps[w.app]) apps[w.app] = { size: a.size || null, minSize: a.minSize || null };
  }
  const displays = (screen.displays || []).map((d) => ({ id: d.id, x: d.x, y: d.y, w: d.w, h: d.h, main: !!d.main, work: rect(workAreaOf(d)) }));
  return { displays, apps, snap: snapOn };
}
// A changed window comes as a new object, but its parts that didn't change (args, restore, pip…) keep
// the objects they had: an app's AppHost and effects see a new `args` only when its content changed.
function keepParts(old, w) {
  if (!old) return w;
  for (const k of Object.keys(w)) {
    const a = old[k], b = w[k];
    if (a && b && a !== b && typeof a === 'object' && typeof b === 'object' && JSON.stringify(a) === JSON.stringify(b)) w[k] = a;
  }
  return w;
}
// A subscriber may change the windows while this runs (the Overview closing itself when the desktop
// changes): that newer state is applied in full, and this older one must not overwrite what is left.
let applied = 0;
let resyncing = false;
function apply(st) {
  // a window the mirror doesn't have at that revision (it can't happen unless a state was lost): ask
  // for all of them again
  if (!resyncing && st.windows.some((x) => !x.w && revs.get(x.id) !== x.rev)) {
    resyncing = true;
    try { const r = JSON.parse(lord.windowList.call('resync', JSON.stringify([facts()]))); if (r.state) st = r.state; } catch (e) { console.error('windows: resync:', e); } finally { resyncing = false; }
  }
  const gen = ++applied;
  const seen = new Set();
  const next = [];
  for (const x of st.windows) {
    seen.add(x.id);
    if (x.w) { mirror.set(x.id, keepParts(mirror.get(x.id), x.w)); revs.set(x.id, x.rev); }
    const w = mirror.get(x.id);
    if (w) next.push(w);
  }
  for (const id of [...mirror.keys()]) if (!seen.has(id)) { mirror.delete(id); revs.delete(id); }
  const v = st.views || NO_VIEWS;
  windowViews.set({ ...v, sets: {
    hidden: new Set(v.hidden), parked: new Set(v.parked), placeholder: new Set(v.placeholder),
    desk: new Map(Object.entries(v.desk || {}).map(([d, ids]) => [Number(d), new Set(ids)])),
  } });
  const same = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
  if (gen !== applied) return;
  if (!same(listStore.get(), st.desktops)) listStore.set(st.desktops);
  if (gen !== applied) return;
  if (activeStore.get() !== st.active) activeStore.set(st.active);
  if (gen !== applied) return;
  if (overviewOn.get() !== !!st.overview) overviewOn.set(!!st.overview);
  if (gen !== applied) return;
  if (!same(windowsStore.get(), next)) windowsStore.set(next);
}
// Runs a rule on the window list; its result. Without lord.windowList (a test, a tool) nothing happens.
function run(op, ...args) {
  if (typeof lord === 'undefined' || !lord.windowList) return null;
  let r;
  try {
    r = JSON.parse(lord.windowList.call(op, JSON.stringify([facts(), ...args])));
  } catch (e) {
    console.error(`windows: ${op}:`, e && e.message ? e.message : e);
    return null;
  }
  if (r.state) apply(r.state);
  return r.result;
}

// Is window `w` on desktop `d` (the one showing, without d)? Picture-in-picture is on all of them.
export const onDesktopOf = (w, d) => !!w && !!windowViews.get().sets.desk.get(d ?? activeDesktop.get())?.has(w.id);
export const onActiveDesktop = (w) => !!w && !windowViews.get().sets.parked.has(w.id);
export function setActiveDesktop(id) { run('setActiveDesktop', id); }
// A new desktop at the end (null when there are already nine).
export const createDesktop = () => run('createDesktop');
// Closes a desktop: its windows move to the next one. false for the last desktop.
export const closeDesktop = (id) => run('closeDesktop', id) === true;
// Exactly `list`, in that order (a new session, sdk/desktops.js): no window is on any other desktop yet.
export function setDesktops(list, active) { run('setDesktops', list, active); }

// The screen size changed: the work area keeps the theme's margins (menu
// bar, dock) and the windows are rearranged to stay in view (Rust: a window left on a display that is
// gone moves to the main one, the others stay inside their display's work area).
onScreenChange((now, old) => {
  if (!areaFn && (old.displays || []).length <= 1 && screen.displays.length <= 1) {
    area = { ...area, w: Math.max(200, area.w + now.width - old.width), h: Math.max(150, area.h + now.height - old.height) };
  }
  run('screen');
});
// Menus of each window: id → [{ label, items: [{ label, hint, onClick, disabled, checked } | '-'] }]
export const appMenus = createStore({});

// chrome: what the theme's frame adds around the content (borders and title bar);
// used to open Linux app windows at the size they requested.
// snap: true turns on window snapping (dragging a window to an edge of the screen, see "Snapping"
// below); both desktop themes do. snapAssist: false keeps the snapping without Snap Assist.
export function configureWindows({ apps, workArea, chrome: c, snap, snapAssist }) {
  themeApps = apps;
  rebuild();
  if (typeof workArea === 'function') areaFn = workArea;
  else if (workArea) { area = workArea; areaFn = null; }
  if (c) chrome = c;
  if (snap != null) snapOn = !!snap;
  if (snapAssist != null) assistOn = !!snapAssist;
}

// SDK internal apps (they don't appear in the app launcher): 'linux' = Wayland windows.
export function registerBuiltinApp(app) {
  builtins = [...builtins.filter((a) => a.id !== app.id), app];
  registry.set(app.id, app);
}
// Apps installed as a package (packages.js). They don't override the theme's.
export function setPackageApps(list) {
  // a package is never a system app, whatever its manifest says (only apps/ built into the theme)
  packageApps = list.filter((a) => !themeApps.some((t) => t.id === a.id)).map((a) => ({ ...a, system: false }));
  rebuild();
}
export const windowChrome = () => chrome;

export const appIdOf = (id) => String(id ?? '').split('#')[0];
export const getApp = (id) => registry.get(appIdOf(id));
// A system app: built into LordOS (apps/), verified with it and not removable (docs/SIGNING.md,
// "Verified system code"). Packages (.lpk) and Linux apps never are.
export const isSystemApp = (id) => { const a = getApp(id); return !!(a && a.system && !a.package); };
// Live system (ISO): apps with "live": true in their manifest (the installer) only exist there.
// "hidden": true (System Information's Sandbox Probe) keeps an app out of the launcher and search.
let live = null;
export const isLiveSystem = () => {
  if (live === null) { try { live = !!lord.system.info().live; } catch (e) { live = false; } }
  return live;
};
export const listApps = () => [...registry.values()].filter((a) => !a.builtin && !a.hidden && (!a.live || isLiveSystem()));
// Raw registry facts for Rust-owned catalogs; visibility is decided there.
export const appCatalogSource = () => [...registry.values()];
export const workArea = (d) => workAreaOf(d);

// What a new window's place comes from (Rust decides): the frame asked for, the place the app was left
// in (sdk/windowmemory.js), else its size centered and cascaded in the work area where it opens.
function placeFacts(appId, frame) {
  if (frame) return { frame };
  const kept = memory && appId !== 'linux' ? memory(appId) : null;
  if (kept) return { memory: kept };
  return { size: registry.get(appId)?.size || null, area: rect(openingArea(appId)) };   // its assigned display, or where the pointer is
}

// ---- dragging in C (lord.wm, docs/WINDOWS.md) ----
// The theme marks each window's node with ref={wm.frameRef(id, inset)}; inset = how much the
// node extends past the frame on each side (the invisible borders for resizing). Moving or resizing is tracked
// by axia-server with no JS per frame; on release __lordWm('end') arrives and the new geometry is saved.
const frameNodes = new Map();         // window id → { node, inset }
const frameRefs = new Map();
let grab = null;
function frameRef(id, inset = 0) {
  const key = `${id}\u0000${inset}`;
  let fn = frameRefs.get(key);
  if (!fn) {
    fn = (inst) => {
      if (inst && inst.id) frameNodes.set(id, { node: inst.id, inset });
      else if (frameNodes.get(id)?.inset === inset) frameNodes.delete(id);
    };
    frameRefs.set(key, fn);
  }
  return fn;
}
// ---- Snapping (docs/WINDOWS.md, "Snapping") ----
// A window dragged to the left or right edge of a display takes that half of its work area; near the
// top or bottom of that edge, a quarter; to the top edge, the whole area (maximized). With Shift held
// the sides give the outer thirds and the top edge the middle one. The theme turns it on
// (configureWindows({ snap: true })), draws the preview from `snapPreview` and may offer the layouts
// with wm.snap(id, zone). While a window is moved in C, Rust follows the zone under the pointer and
// axia-server announces it only when it changes (__lordWm('zone')): no JS per frame, no polling.
// After a snap, Snap Assist offers the rest of the layout to the other windows (sdk/overview.jsx).
// Snapped windows sharing an edge resize together (grab.c with rust snap/resize.rs).
let snapOn = false;
let assistOn = true;
// { id, zone, rect: { x, y, w, h } } while a dragged window would snap, else null
export const snapPreview = createStore(null);
export const snapEnabled = () => snapOn;
// The zone a window dropped with the pointer at (x, y) takes, or null; thirds: Shift is held.
export function snapZoneAt(x, y, thirds = false) {
  const api = snapApi(), d = displayAt(x, y);
  return api && d ? api.zoneAt(x, y, workAreaOf(d), !!thirds) : null;
}
// Snaps window `id` to `zone` in the work area of its display (or of `display`); 'restore' goes back.
const displayIndex = (d) => (d ? screen.displays.indexOf(d) : -1);
function snapTo(id, zone, display) {
  const i = displayIndex(display);
  run('snap', id, zone, i >= 0 ? i : null);
}
// The windows snapped on display `d` on this desktop, other than `except`, in stacking order.
export function snappedOn(d, except) {
  if (!d) return [];
  return windows.get().filter((w) => w.id !== except && w.snapped && !w.minimized && !w.fullscreen && !w.pip
    && onActiveDesktop(w) && displayFor(w).id === d.id);
}
// Snap Assist: sdk/overview.jsx draws it and sets the hook; ({ id, zone, display, layout }) after a
// snap (layout: the zones of the layout picked in the theme's flyout, if that's where it came from).
let assistHook = null;
export function setSnapAssistHook(fn) { assistHook = fn; }
function offerAssist(id, zone, display, layout) {
  if (!snapOn || !assistOn || !assistHook || !zone || zone === 'max') return;
  const w = windows.get().find((x) => x.id === id);
  if (w && w.snapped === zone) assistHook({ id, zone, display: display || displayFor(w), layout: Array.isArray(layout) ? layout : [] });
}

const grabAreas = () => screen.displays.map((d) => workAreaOf(d)).map((a, i) => ({ x: screen.displays[i].x, w: screen.displays[i].w, y: a.y }));
// Resizing a snapped window: the others snapped on its display, which follow an edge they share with it.
function grabLinks(w) {
  if (!snapOn || !w.snapped) return [];
  return snappedOn(displayFor(w), w.id).filter((o) => frameNodes.has(o.id)).slice(-8).map((o) => {
    const f = frameNodes.get(o.id);
    const [minW, minH] = registry.get(o.app)?.minSize || [160, 100];
    return { id: o.id, node: f.node, inset: f.inset, minW, minH };
  });
}
function beginGrab(id, edges) {
  const w = windows.get().find((x) => x.id === id);
  const f = frameNodes.get(id);
  if (!w || !f || (w.maximized && edges) || w.fullscreen || typeof lord === 'undefined' || !lord.wm) return false;
  const [minW, minH] = registry.get(w.app)?.minSize || [160, 100];
  const opts = { inset: f.inset, minW, minH, minTop: areaOfWindow(w).y, areas: grabAreas() };
  if (!edges && snapOn && snapApi()) opts.snap = screen.displays.map((d) => ({ x: d.x, y: d.y, w: d.w, h: d.h, work: workAreaOf(d) }));
  // a maximized or snapped window comes loose at its own size as soon as it really moves (grab.c)
  const own = w.maximized ? w : w.snapped ? w.restore : null;
  if (!edges && own && own.w > 0 && own.h > 0) opts.detach = { w: own.w + 2 * f.inset, h: own.h + 2 * f.inset };
  const links = edges ? grabLinks(w) : [];
  if (links.length) { opts.links = links; opts.area = areaOfWindow(w); }
  if (!lord.wm.grab(f.node, edges, opts)) return false;
  const on = wm.frame(w);           // where it is on screen (a maximized window: its work area)
  grab = { id, node: f.node, edges, f: { x: on.x, y: on.y, w: on.w, h: on.h }, links: new Map(links.map((l) => [l.node, l.id])) };
  return true;
}
function showZone(d) {
  const cur = snapPreview.get();
  if (!grab || grab.edges || !d.zone) { if (cur) snapPreview.set(null); return; }
  if (cur && cur.zone === d.zone && cur.rect.x === d.x && cur.rect.y === d.y) return;
  snapPreview.set({ id: grab.id, zone: d.zone, rect: { x: d.x, y: d.y, w: d.w, h: d.h } });
}
// The drag ended in C: Rust applies it (a snap where it was dropped, back out of a snapped place
// under the pointer, the windows resized along a shared line, or the plain change).
globalThis.__lordWm = (type, d) => {
  if (type === 'zone') { showZone(d); return; }
  if (type !== 'end' || !grab || grab.node !== d.node) return;
  const { id, f, edges, links } = grab;
  grab = null;
  if (snapPreview.get()) snapPreview.set(null);
  const pointer = typeof lord !== 'undefined' && lord.wm && typeof lord.wm.pointer === 'function' ? lord.wm.pointer() : null;
  const moved = (Array.isArray(d.links) ? d.links : []).filter((l) => links.has(l.node)).map((l) => ({ id: links.get(l.node), dx: l.dx, dy: l.dy, dw: l.dw, dh: l.dh }));
  const r = run('grabEnd', { id, f, edges, dx: d.dx, dy: d.dy, dw: d.dw, dh: d.dh, zone: d.zone || null, detached: !!d.detached, links: moved,
    pointer: pointer ? { x: pointer.x, y: pointer.y } : null });
  if (r && r.assist) offerAssist(id, r.assist, screen.displays[r.display]);
};

// when each window was opened (lord.now()), for the developer timing log (settings "debugTiming")
export const openedAt = new Map();

export const wm = {
  // opts: { new: true } another window of the same app; { instance: 'x' } named window;
  //       { frame: { x, y, w, h } } initial geometry; { args: {...} } data for the app
  //       (e.g. wm.open('files', { args: { path: '/home/lord/Documentos' } }));
  //       { desktop: n } a new window on that desktop, not the showing one (session restore: it
  //       doesn't take the front or the keyboard from the desktop in view).
  // Returns the window id.
  open(appId, opts = {}) {
    if (!registry.has(appId)) {
      // inside an isolated system app (axia-server --app): another system app is opened by the
      // session, through the broker (Mail → Calendar, Contacts → a new message; sec/brokermail.c)
      if (typeof lord !== 'undefined' && typeof lord.appId === 'string' && lord.broker && typeof lord.broker.openApp === 'function') {
        lord.broker.openApp(String(appId), JSON.stringify(opts.args || {}), !!opts.new);
        return null;
      }
      console.warn('unknown app:', appId);
      return null;
    }
    // already open: it comes up (and its desktop into view) with the new args
    const id = run('open', { app: appId, instance: opts.instance || null, new: !!opts.new, args: opts.args || null,
      desktop: opts.desktop ?? null, ...placeFacts(appId, opts.frame) });
    if (id && globalThis.__lordTiming && typeof lord !== 'undefined' && lord.now) openedAt.set(id, lord.now());
    return id;
  },
  // Closes the window; if the app set a guard and it returns false, it doesn't close.
  close(id, force = false) {
    const guard = guards.get(id);
    if (!force && guard && guard() === false) {
      // it stays, and it may be asking something (unsaved changes): on another desktop that must be seen.
      // A Linux window always answers false here: its app closes it, or asks in a window of its own.
      const w = windows.get().find((x) => x.id === id);
      if (w && !w.args?.surface && !onActiveDesktop(w)) wm.focus(id);
      return false;
    }
    guards.delete(id);
    appMenus.set((m) => { if (!(id in m)) return m; const n = { ...m }; delete n[id]; return n; });
    run('close', id);
    return true;
  },
  // To the front of its desktop, and that desktop comes into view.
  focus: (id) => { run('focus', id); },
  // To the front of its own desktop, whichever is showing.
  raise: (id) => { run('raise', id); },
  // Under every other window (the app switcher's backward step).
  toBack: (id) => { run('toBack', id); },
  // The windows of this desktop take turns: 'left' brings the rearmost up, 'right' sends the front one under.
  cycle: (dir) => { run('cycle', dir); },
  // To another desktop: the window leaves, and the desktop in view stays.
  moveToDesktop: (id, desktop) => { run('moveToDesktop', id, desktop); },
  minimize: (id) => { run('minimize', id); },
  toggleMaximize: (id) => { run('toggleMaximize', id); },
  // Snapping (see "Snapping" above): zone is one of SNAP_ZONES; 'restore' goes back to the size
  // and place from before the first snap (or un-maximizes).
  // opts: { assist: true } offers the rest of the layout to the other windows afterwards (Snap
  // Assist), { layout: [zones] } the layout the zone was picked from; { display } snaps on that
  // display instead of the window's own.
  snap: (id, zone, opts) => {
    snapTo(id, zone, opts && opts.display);           // Rust leaves an unknown zone alone
    if (opts && opts.assist) offerAssist(id, zone, opts.display, opts.layout);
  },
  // The Window menu (sdk/windowmenu.js, 0.8.5): centered in its display's work area at its own size.
  center: (id) => { run('center', id); },
  // To another display: the same place relative to the work area (a snapped window keeps its zone, a
  // maximized one stays maximized), its size kept inside the new area.
  moveToDisplay: (id, displayId) => { run('moveToDisplay', id, displayId); },
  // Several windows at once on the display of the first: 'left-right', 'right-left', 'top-bottom',
  // 'bottom-top' (two windows) or 'quarters' (up to four).
  arrange: (ids, layout) => { run('arrange', ids, layout); },
  // Super+arrow (dir: 'left' | 'right' | 'up' | 'down'), the moves in Rust (snap_next). From a free
  // window, Snap Assist follows.
  snapKey(id, dir) {
    const zone = run('snapKey', id, dir);
    if (zone) offerAssist(id, zone);
  },
  move: (id, x, y) => { run('move', id, x, y); },
  resize: (id, w, h) => { run('resize', id, w, h); },
  setFrame: (id, f) => { run('setFrame', id, f); },
  // The on-screen keyboard moves the window with the field out of its way, and puts it back as it was
  // (sdk/osk.js). A window the person moves meanwhile stays where they left it.
  liftForKeyboard: (id, f) => { run('liftForKeyboard', id, f); },
  dropFromKeyboard: (id) => { run('dropFromKeyboard', id); },
  setTitle: (id, title) => {
    const w = mirror.get(id);
    if (w && w.title !== title) run('patch', id, { title });
  },
  setCloseGuard(id, fn) {
    guards.set(id, fn);
    return () => { if (guards.get(id) === fn) guards.delete(id); };
  },
  setMenus: (id, menus) => {
    // In an isolated app (axia-server --app) the menus go to the system menu bar through the
    // broker: they are serialized (no onClick, with an id per item) and clicks come back as an event.
    if (typeof lord !== 'undefined' && typeof lord.appId === 'string') { sendAppMenus(menus); return; }   // only in the app process
    appMenus.set((m) => ({ ...m, [id]: menus }));
  },
  get: (id) => mirror.get(id) || null,
  // The window in front of the showing desktop (never another desktop's, never picture-in-picture).
  focused: () => windowViews.get().front,
  // The window an app comes back with, of `ids`: on this desktop if it has one, a visible one before a
  // minimized one, else the top one of any desktop (focusing that one switches desktop).
  pick: (ids) => run('pick', ids),
  // Protected window (capture protection): it looks normal on screen, but
  // comes out black in screenshots and recordings (docs/MEDIA.md). Used, for
  // example, by a password app.
  setProtected: (id, on = true) => { run('patch', id, { protected: !!on }); },
  // Merges into a window's args (a Linux app switching between the theme's frame and its own, args.csd).
  setArgs: (id, a) => { run('setArgs', id, a); },
  // The window menu at a screen point (a Linux app with its own bar asked for it,
  // xdg_toplevel.show_window_menu): the theme draws it from w.windowMenu; null closes it.
  showWindowMenu: (id, x, y) => { run('patch', id, { windowMenu: { x: Math.round(x), y: Math.round(y) } }); },
  closeWindowMenu: (id) => { run('patch', id, { windowMenu: null }); },
  // Fullscreen for a LordOS app (Player, Photos): the SDK draws it above everything,
  // without the theme's frame. Esc (wm.exitFullscreen) or wm.setFullscreen(id, false) goes back.
  setFullscreen: (id, on = true) => { run('setFullscreen', id, !!on); },
  exitFullscreen: () => run('exitFullscreen') === true,
  // Picture-in-picture: a small frameless window, always on top (frame = { x, y, w, h } or null).
  // It shows on every desktop; back from it, the window stays on the one in view.
  desktop: () => activeDesktop.get(),
  setPip: (id, frame) => { run('setPip', id, frame ? { ...frame } : null); },
  // Dragging in C (see frameRef above). beginMove/beginResize return false if it wasn't possible (no
  // marked node, maximized, or no button held): the theme can continue with its own JS drag.
  // edges as in xdg_toplevel.resize: 1 top, 2 bottom, 4 left, 8 right (5 = top-left…).
  frameRef,
  previewNode: (id) => frameNodes.get(id)?.node || 0,
  beginMove: (id) => beginGrab(id, 0),
  beginResize: (id, edges) => beginGrab(id, edges & 15),
  // LordBench's desktop scene (apps/sysinfo/scene.jsx): axia-server moves the window along a figure of
  // eight (rx × ry px, one turn every periodMs) through the drag's own path, one step per frame, until
  // stopScript() or pointer input. The store keeps the old place: the scene's window is closed after.
  scriptMove(id, { rx, ry, periodMs }) {
    const w = mirror.get(id);
    const f = frameNodes.get(id);
    if (!w || !f || w.maximized || w.fullscreen || typeof lord === 'undefined' || !lord.wm || typeof lord.wm.script !== 'function') return false;
    return !!lord.wm.script(f.node, { rx, ry, periodMs, inset: f.inset, minTop: areaOfWindow(w).y, areas: grabAreas() });
  },
  stopScript: () => { if (typeof lord !== 'undefined' && lord.wm) lord.wm.cancel(); },
  // Effective geometry (a maximized window takes the work area).
  frame: (w) => { if (!w.maximized) return w; const a = areaOfWindow(w); return { x: a.x, y: a.y, w: a.w, h: a.h }; },
};

// windows: all of them, every desktop. visible: what shows on this desktop. focused: its front window.
// mounted: what the theme draws, in stacking order: visible, plus the LordOS apps of hidden desktops
// (WindowShell keeps those display: none so they keep their state) and, in the overview, the minimized.
// fullscreen and picture-in-picture windows are drawn by the SDK (FloatingWindows).
const pickIds = (ids) => ids.map((id) => mirror.get(id)).filter(Boolean);
let viewsMemo = null;
export function useWindows() {
  const list = useStore(windows);
  const desktop = useStore(activeDesktop);
  const v = useStore(windowViews);
  if (!viewsMemo || viewsMemo.v !== v || viewsMemo.list !== list) {
    viewsMemo = { v, list, visible: pickIds(v.visible), mounted: pickIds(v.mounted) };
  }
  return { windows: list, visible: viewsMemo.visible, mounted: viewsMemo.mounted, focused: v.top, desktop };
}
// The windows that show on this desktop, bottom to top (no fullscreen, no picture-in-picture).
export const visibleWindows = () => pickIds(windowViews.get().visible);
// The compositor's view of a Linux window: parked = on a hidden desktop. It loses the keyboard and any
// fullscreen until it comes back. X11 windows go iconic for both, so X's stacking can't catch the pointer.
export function surfaceState(w) {
  const parked = windowViews.get().sets.parked.has(w.id);
  return { maximized: !!w.maximized, parked, iconic: !!w.minimized || parked };
}
// The window is out of sight (minimized or on a hidden desktop): what animates in it should stop.
export const windowHidden = (id) => !mirror.has(id) || windowViews.get().sets.hidden.has(id);
export function useWindowHidden(id) {
  return useStore(windowViews, () => windowHidden(id));
}

// ---- menus of an isolated app: serialize to the broker and receive the clicks ----
const appMenuHandlers = new Map();
let appMenuSeq = 1;
function sendAppMenus(menus) {
  appMenuHandlers.clear();
  const ser = (menus || []).map((g) => ({
    label: g.label,
    items: (g.items || []).map((it) => {
      if (it === '-' || it == null) return '-';
      const mid = `m${appMenuSeq++}`;
      if (typeof it.onClick === 'function') appMenuHandlers.set(mid, it.onClick);
      const sub = it.submenu ? it.submenu.map((s) => (s === '-' ? '-' : (() => { const sid = `m${appMenuSeq++}`; if (s.onClick) appMenuHandlers.set(sid, s.onClick); return { label: s.label, hint: s.hint, disabled: !!s.disabled, checked: !!s.checked, id: sid }; })())) : undefined;
      return { label: it.label, hint: it.hint, disabled: !!it.disabled, checked: !!it.checked, id: mid, submenu: sub };
    }),
  }));
  try { lord.broker.menus(JSON.stringify(ser)); } catch (e) { /* server without broker */ }
}
// Called by the app host when the user picks an item in the system menu bar.
export function appMenuClick(itemId) { const fn = appMenuHandlers.get(itemId); if (fn) try { fn(); } catch (e) { console.error('menu:', e); } }

// Menus of window `id` (for the theme's menu bar).
export const useAppMenus = (id) => useStore(appMenus, (m) => (id ? m[id] : null)) || null;

// Display name of a window: its title or the app's name.
export const windowTitle = (w) => w?.title || appName(getApp(w?.id)) || '';
