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
import { FIRST_DESKTOP, onDesktop, visibleOn, mountedOn, focusedOn, windowHidden, addDesktop, removeDesktop } from './desktopmodel.js';

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
let seq = 2;
const guards = new Map();

// State: list ordered from bottom to top (the last one is in front).
export const windows = createStore([]);

// Desktops (docs/DESKTOPS.md, the rules are in desktopmodel.js): each window has w.desktop, and only
// the windows of activeDesktop show, take the front place and get the keyboard. All displays switch
// together. desktopList is the order on screen; sdk/desktops.js switches, persists and animates.
export const activeDesktop = createStore(FIRST_DESKTOP);
export const desktopList = createStore([FIRST_DESKTOP]);
export const onActiveDesktop = (w) => onDesktop(w, activeDesktop.get());
const desktopState = () => ({ list: desktopList.get(), active: activeDesktop.get() });
// The overview shows this desktop's minimized windows too (sdk/overview.jsx sets it).
export const overviewShowing = createStore(false);
export function setActiveDesktop(id) {
  if (desktopList.get().includes(id)) activeDesktop.set(id);
}
export function createDesktop() {
  const { state, id } = addDesktop(desktopState());
  if (id != null) desktopList.set(state.list);
  return id;
}
// Closes a desktop: its windows move to the next one. false for the last desktop.
export function closeDesktop(id) {
  const { state, target } = removeDesktop(desktopState(), id);
  if (target == null) return false;
  windows.set((ws) => ws.map((w) => ((w.desktop ?? FIRST_DESKTOP) === id ? { ...w, desktop: target } : w)));
  desktopList.set(state.list);
  activeDesktop.set(state.active);
  return true;
}
// Exactly `list`, in that order (a new session, sdk/desktops.js): no window is on any other desktop yet.
export function setDesktops(list, active) {
  desktopList.set(list);
  activeDesktop.set(active);
}

// The screen size changed: the work area keeps the theme's margins (menu
// bar, dock) and the windows are rearranged to stay in view.
onScreenChange((now, old) => {
  if (!areaFn && (old.displays || []).length <= 1 && screen.displays.length <= 1) {
    area = { ...area, w: Math.max(200, area.w + now.width - old.width), h: Math.max(150, area.h + now.height - old.height) };
  }
  // a window left on a display that is gone (or outside every display) moves to the main one; the
  // others stay on their display, inside its work area
  const onSome = (w) => screen.displays.some((d) => w.x < d.x + d.w - 48 && w.x + w.w > d.x + 48 && w.y < d.y + d.h - 24 && w.y + 24 > d.y);
  windows.set((ws) => ws.map((w) => {
    const a = onSome(w) ? areaOfWindow(w) : workAreaOf(mainDisplay());
    const ww = Math.min(w.w, a.w), hh = Math.min(w.h, a.h);
    const keep = onSome(w) && w.x + ww > a.x && w.x < a.x + a.w;
    return {
      ...w, w: ww, h: hh,
      x: Math.round(keep ? Math.min(Math.max(w.x, a.x - ww + 96), a.x + a.w - 96) : a.x + (a.w - ww) / 2),
      y: Math.round(keep ? Math.min(Math.max(w.y, a.y), a.y + a.h - Math.min(hh, 48)) : a.y + Math.max(0, (a.h - hh) / 2)),
    };
  }));
});
// Menus of each window: id → [{ label, items: [{ label, hint, onClick, disabled, checked } | '-'] }]
export const appMenus = createStore({});

// chrome: what the theme's frame adds around the content (borders and title bar);
// used to open Linux app windows at the size they requested.
// snap: true turns on window snapping (dragging a window to an edge of the screen, see "Snapping"
// below); the Classic theme does. snapAssist: false keeps the snapping without Snap Assist.
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

function place(appId, count, frame) {
  if (frame) return { ...frame, minimized: false, maximized: false };
  const kept = memory && appId !== 'linux' ? memory(appId) : null;
  if (kept) return { ...kept, minimized: false };
  const [sw, sh] = registry.get(appId)?.size || [640, 420];
  const offset = (count % 6) * 30;
  const area = openingArea(appId);                 // its assigned display, or where the pointer is (docs/DISPLAYS.md)
  // the whole window stays inside the work area: a cascaded window must not slide under the
  // Dock, which would cover (and take the clicks of) its bottom edge
  const w = Math.min(sw, area.w), h = Math.min(sh, area.h);
  const x = Math.round(area.x + (area.w - w) / 2 - 60 + offset);
  const y = Math.round(Math.max(area.y + 16, area.y + (area.h - h) / 2 - 40 + offset));
  return {
    w, h, minimized: false, maximized: false,
    x: Math.max(area.x, Math.min(x, area.x + area.w - w)),
    y: Math.max(area.y, Math.min(y, area.y + area.h - h)),
  };
}

const raise = (ws, id) => {
  const w = ws.find((x) => x.id === id);
  return w ? [...ws.filter((x) => x.id !== id), { ...w, minimized: false }] : ws;
};
const raiseUnlessTop = (ws, id) => (ws[ws.length - 1]?.id === id && !ws[ws.length - 1].minimized ? ws : raise(ws, id));
// Bringing a window forward shows its desktop, except picture-in-picture, which is on all of them.
function showDesktopOf(id) {
  const w = windows.get().find((x) => x.id === id);
  if (w && !onActiveDesktop(w)) setActiveDesktop(w.desktop);
}
const patch = (id, p) => windows.set((ws) => ws.map((w) => (w.id === id ? { ...w, ...p } : w)));

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
// Snaps window `id` to `zone` in the work area of its display (or of `display`).
function snapTo(id, zone, display) {
  windows.set((ws) => ws.map((w) => {
    if (w.id !== id) return w;
    const restore = w.snapped ? w.restore : { x: w.x, y: w.y, w: w.w, h: w.h };
    if (zone === 'max') return { ...w, maximized: true, minimized: false, snapped: null, restore: null, ...(w.snapped ? restore : null) };
    const a = workAreaOf(display || displayFor(w));
    const [minW, minH] = registry.get(w.app)?.minSize || [160, 100];
    const r = snapRect(zone, a);
    if (!r) return w;
    return { ...w, ...r, w: Math.max(r.w, Math.min(minW, a.w)), h: Math.max(r.h, Math.min(minH, a.h)), maximized: false, minimized: false, snapped: zone, restore };
  }));
}
function unsnap(id) {
  windows.set((ws) => ws.map((w) => {
    if (w.id !== id) return w;
    if (w.maximized) return { ...w, maximized: false };
    if (!w.snapped) return w;
    return { ...w, ...(w.restore || {}), snapped: null, restore: null };
  }));
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
  if (!w || !f || w.maximized || w.fullscreen || typeof lord === 'undefined' || !lord.wm) return false;
  const [minW, minH] = registry.get(w.app)?.minSize || [160, 100];
  const opts = { inset: f.inset, minW, minH, minTop: areaOfWindow(w).y, areas: grabAreas() };
  if (!edges && snapOn && snapApi()) opts.snap = screen.displays.map((d) => ({ x: d.x, y: d.y, w: d.w, h: d.h, work: workAreaOf(d) }));
  const links = edges ? grabLinks(w) : [];
  if (links.length) { opts.links = links; opts.area = areaOfWindow(w); }
  if (!lord.wm.grab(f.node, edges, opts)) return false;
  grab = { id, node: f.node, edges, f: { x: w.x, y: w.y, w: w.w, h: w.h }, links: new Map(links.map((l) => [l.node, l.id])) };
  return true;
}
function showZone(d) {
  const cur = snapPreview.get();
  if (!grab || grab.edges || !d.zone) { if (cur) snapPreview.set(null); return; }
  if (cur && cur.zone === d.zone && cur.rect.x === d.x && cur.rect.y === d.y) return;
  snapPreview.set({ id: grab.id, zone: d.zone, rect: { x: d.x, y: d.y, w: d.w, h: d.h } });
}
globalThis.__lordWm = (type, d) => {
  if (type === 'zone') { showZone(d); return; }
  if (type !== 'end' || !grab || grab.node !== d.node) return;
  const { id, f, edges, links } = grab;
  grab = null;
  if (snapPreview.get()) snapPreview.set(null);
  if (!d.dx && !d.dy && !d.dw && !d.dh) return;      // it was a click on the bar
  const nx = Math.round(f.x + d.dx), ny = Math.round(f.y + d.dy);
  const moved = Array.isArray(d.links) ? d.links.filter((l) => links.has(l.node)) : [];
  if (edges && moved.length) {
    // a linked resize: the windows along the line keep their snapped places, at their new sizes
    const by = new Map(moved.map((l) => [links.get(l.node), l]));
    windows.set((ws) => ws.map((w) => {
      if (w.id === id) return { ...w, x: nx, y: ny, w: Math.round(f.w + d.dw), h: Math.round(f.h + d.dh) };
      const l = by.get(w.id);
      return l ? { ...w, x: Math.round(w.x + l.dx), y: Math.round(w.y + l.dy), w: Math.round(w.w + l.dw), h: Math.round(w.h + l.dh) } : w;
    }));
    return;
  }
  if (snapOn && !edges && typeof lord !== 'undefined' && lord.wm && typeof lord.wm.pointer === 'function') {
    const p = lord.wm.pointer();
    const w = windows.get().find((x) => x.id === id);
    if (d.zone && w) {
      const display = displayAt(p.x, p.y);
      if (!w.snapped) patch(id, { x: nx, y: ny, w: f.w, h: f.h });   // the size to go back to: from before the first snap
      snapTo(id, d.zone, display);
      offerAssist(id, d.zone, display);
      return;
    }
    if (w && w.snapped && w.restore) {
      // dragged out of a snapped place: back to its size, under the pointer where it was grabbed
      const rel = f.w ? Math.min(1, Math.max(0, (p.x - nx) / f.w)) : 0.5;
      const r = w.restore;
      patch(id, { x: Math.round(p.x - rel * r.w), y: ny, w: r.w, h: r.h, maximized: false, snapped: null, restore: null });
      return;
    }
  }
  patch(id, { x: nx, y: ny, w: Math.round(f.w + d.dw), h: Math.round(f.h + d.dh), maximized: false,
    ...(edges ? { snapped: null, restore: null } : null) });
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
    const id = opts.instance ? `${appId}#${opts.instance}` : opts.new ? `${appId}#${seq++}` : appId;
    const args = opts.args ? { ...opts.args, nonce: seq++ } : null;
    if (globalThis.__lordTiming && typeof lord !== 'undefined' && lord.now) openedAt.set(id, lord.now());
    const desktop = desktopList.get().includes(opts.desktop) ? opts.desktop : activeDesktop.get();
    showDesktopOf(id);                                 // already open: its desktop comes into view
    windows.set((ws) => (ws.some((w) => w.id === id)
      ? raise(ws, id).map((w) => (w.id === id && args ? { ...w, args } : w))
      : [...ws, { id, app: appId, title: null, args, desktop, ...place(appId, ws.length, opts.frame) }]));
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
    windows.set((ws) => ws.filter((w) => w.id !== id));
    return true;
  },
  // To the front of its desktop, and that desktop comes into view.
  focus(id) {
    showDesktopOf(id);
    windows.set((ws) => raiseUnlessTop(ws, id));
  },
  // To the front of its own desktop, whichever is showing.
  raise: (id) => windows.set((ws) => raiseUnlessTop(ws, id)),
  // To another desktop: the window leaves, and the desktop in view stays.
  moveToDesktop(id, desktop) {
    if (desktopList.get().includes(desktop)) patch(id, { desktop });
  },
  minimize: (id) => patch(id, { minimized: true }),
  toggleMaximize: (id) => windows.set((ws) => ws.map((w) => (w.id !== id ? w : w.snapped && w.restore
    ? { ...w, ...w.restore, snapped: null, restore: null, maximized: true }
    : { ...w, maximized: !w.maximized }))),
  // Snapping (see "Snapping" above): zone is one of SNAP_ZONES; 'restore' goes back to the size
  // and place from before the first snap (or un-maximizes).
  // opts: { assist: true } offers the rest of the layout to the other windows afterwards (Snap
  // Assist), { layout: [zones] } the layout the zone was picked from; { display } snaps on that
  // display instead of the window's own.
  snap: (id, zone, opts) => {
    if (zone === 'restore') { unsnap(id); return; }
    if (!SNAP_ZONES.includes(zone)) return;
    snapTo(id, zone, opts && opts.display);
    if (opts && opts.assist) offerAssist(id, zone, opts.display, opts.layout);
  },
  // The Window menu (sdk/windowmenu.js, 0.8.5): centered in its display's work area at its own size.
  center(id) {
    const w = windows.get().find((x) => x.id === id);
    if (!w) return;
    const a = workAreaOf(displayFor(w)), r = w.snapped && w.restore ? w.restore : w;
    const ww = Math.min(r.w, a.w), hh = Math.min(r.h, a.h);
    patch(id, { x: Math.round(a.x + (a.w - ww) / 2), y: Math.round(a.y + (a.h - hh) / 2), w: ww, h: hh, maximized: false, snapped: null, restore: null });
  },
  // To another display: the same place relative to the work area (a snapped window keeps its zone, a
  // maximized one stays maximized), its size kept inside the new area.
  moveToDisplay(id, displayId) {
    const w = windows.get().find((x) => x.id === id);
    const d = screen.displays.find((x) => x.id === displayId);
    if (!w || !d) return;
    if (w.snapped) { snapTo(id, w.snapped, d); return; }
    const from = workAreaOf(displayFor(w)), to = workAreaOf(d);
    const ww = Math.min(w.w, to.w), hh = Math.min(w.h, to.h);
    const fx = from.w > w.w ? (w.x - from.x) / (from.w - w.w) : 0, fy = from.h > w.h ? (w.y - from.y) / (from.h - w.h) : 0;
    const clamp = (v) => Math.max(0, Math.min(1, v));
    patch(id, { x: Math.round(to.x + clamp(fx) * (to.w - ww)), y: Math.round(to.y + clamp(fy) * (to.h - hh)), w: ww, h: hh });
  },
  // Several windows at once on the display of the first: 'left-right', 'right-left', 'top-bottom',
  // 'bottom-top' (two windows) or 'quarters' (up to four).
  arrange(ids, layout) {
    const zones = { 'left-right': ['left', 'right'], 'right-left': ['right', 'left'], 'top-bottom': ['top', 'bottom'], 'bottom-top': ['bottom', 'top'],
      quarters: ['top-left', 'top-right', 'bottom-left', 'bottom-right'] }[layout];
    const first = windows.get().find((x) => x.id === ids[0]);
    if (!zones || !first) return;
    const d = displayFor(first);
    ids.slice(0, zones.length).forEach((id, i) => { snapTo(id, zones[i], d); });
    for (const id of [...ids.slice(0, zones.length)].reverse()) wm.focus(id);
  },
  // Super+arrow (dir: 'left' | 'right' | 'up' | 'down'), the moves in Rust (snap_next). From a free
  // window, Snap Assist follows.
  snapKey(id, dir) {
    const w = windows.get().find((x) => x.id === id);
    const api = snapApi();
    if (!w || w.fullscreen || w.pip || !api) return;
    const to = api.next(w.maximized ? 'max' : w.snapped || null, dir);
    if (!to) return;
    if (to === 'minimize') wm.minimize(id);
    else wm.snap(id, to, { assist: !w.snapped && !w.maximized });
  },
  move: (id, x, y) => patch(id, { x: Math.round(x), y: Math.round(Math.max(workAreaOf(displayAt(x, y)).y, y)), maximized: false, snapped: null, restore: null, oskFrame: null }),
  resize: (id, w, h) => patch(id, { w: Math.round(w), h: Math.round(h) }),
  setFrame: (id, f) => patch(id, { ...f, maximized: false, snapped: null, restore: null, oskFrame: null }),
  // The on-screen keyboard moves the window with the field out of its way, and puts it back as it was
  // (sdk/osk.js). A window the person moves meanwhile stays where they left it.
  liftForKeyboard(id, f) {
    const w = windows.get().find((x) => x.id === id);
    if (!w) return;
    const oskFrame = w.oskFrame || { x: w.x, y: w.y, w: w.w, h: w.h, maximized: w.maximized, snapped: w.snapped, restore: w.restore };
    patch(id, { ...f, maximized: false, snapped: null, restore: null, oskFrame });
  },
  dropFromKeyboard(id) {
    const w = windows.get().find((x) => x.id === id);
    if (w?.oskFrame) patch(id, { ...w.oskFrame, oskFrame: null });
  },
  setTitle: (id, title) => {
    const w = windows.get().find((x) => x.id === id);
    if (w && w.title !== title) patch(id, { title });
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
  get: (id) => windows.get().find((w) => w.id === id) || null,
  // The window in front of the showing desktop.
  focused: () => focusedOn(windows.get(), activeDesktop.get())?.id ?? null,
  // Protected window (capture protection): it looks normal on screen, but
  // comes out black in screenshots and recordings (docs/MEDIA.md). Used, for
  // example, by a password app.
  setProtected: (id, on = true) => patch(id, { protected: !!on }),
  // Merges into a window's args (a Linux app switching between the theme's frame and its own, args.csd).
  setArgs: (id, a) => windows.set((ws) => ws.map((w) => (w.id === id ? { ...w, args: { ...(w.args || {}), ...a } } : w))),
  // The window menu at a screen point (a Linux app with its own bar asked for it,
  // xdg_toplevel.show_window_menu): the theme draws it from w.windowMenu; null closes it.
  showWindowMenu: (id, x, y) => patch(id, { windowMenu: { x: Math.round(x), y: Math.round(y) } }),
  closeWindowMenu: (id) => patch(id, { windowMenu: null }),
  // Fullscreen for a LordOS app (Player, Photos): the SDK draws it above everything,
  // without the theme's frame. Esc or wm.setFullscreen(id, false) goes back.
  setFullscreen: (id, on = true) => windows.set((ws) => ws.map((w) => (w.id === id ? { ...w, fullscreen: !!on, minimized: false } : w))),
  // Picture-in-picture: a small frameless window, always on top (frame = { x, y, w, h } or null).
  // It shows on every desktop; back from it, the window stays on the one in view.
  desktop: () => activeDesktop.get(),
  setPip: (id, frame) => patch(id, { pip: frame ? { ...frame } : null, fullscreen: false, ...(frame ? null : { desktop: activeDesktop.get() }) }),
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
    const w = windows.get().find((x) => x.id === id);
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
export function useWindows() {
  const list = useStore(windows);
  const desktop = useStore(activeDesktop);
  const overview = useStore(overviewShowing);
  // fullscreen and picture-in-picture windows are drawn by the SDK (FloatingWindows)
  const visible = visibleOn(list, desktop);
  const top = visible.length ? visible[visible.length - 1].id : null;
  return { windows: list, visible, mounted: mountedOn(list, desktop, overview), focused: top, desktop };
}
// The window is out of sight (minimized or on a hidden desktop): what animates in it should stop.
export function useWindowHidden(id) {
  const desktop = useStore(activeDesktop);
  return useStore(windows, (ws) => windowHidden(ws.find((w) => w.id === id), desktop));
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
