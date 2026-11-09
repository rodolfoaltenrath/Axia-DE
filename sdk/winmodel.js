// winmodel.js — the window manager's pure rules, in Rust (rust/axia-shell/src/winmodel, through
// lord.winmodel: server/src/ui/js_winmodel.c). Remembered places, the list of desktops, the Overview's
// and Snap Assist's layouts and Settings → Displays' arrangement take plain facts and give plain data
// back. The window list, its stores and the per-window visibility rules (desktopmodel.js) stay in JS.
// Only the fields each rule needs cross over, never a whole window (its args can be anything).

function call(op, ...args) {
  if (typeof lord === 'undefined' || typeof lord.winmodel !== 'function') throw new Error(`winmodel: '${op}' runs in axia-server`);
  return JSON.parse(lord.winmodel(op, JSON.stringify(args)));
}
const rect = (r) => ({ x: r.x, y: r.y, w: r.w, h: r.h });

// ---- remembered places (docs/WINDOWS.md, "Remembered places"; sdk/windowmemory.js) ----
const frameFacts = (w) => ({
  x: w.x, y: w.y, w: w.w, h: w.h, maximized: !!w.maximized, snapped: w.snapped || null,
  restore: w.restore ? rect(w.restore) : null, pip: !!w.pip, fullscreen: !!w.fullscreen,
  oskFrame: w.oskFrame ? { ...rect(w.oskFrame), maximized: !!w.oskFrame.maximized } : null,
});
const displayFacts = (d) => ({ name: d.name, connector: d.connector, x: d.x, y: d.y });

// What a window leaves on `display`: { d, x, y, w, h, max }, or null.
export const placeOf = (w, display) => call('place.of', frameFacts(w), displayFacts(display));

// A saved place back on the displays that exist (areaOf: display → its work area), or null.
// opts: { main, assigned } displays, fixed and size (the app's size can't change), shift.
export function restoreFrame(p, displays, areaOf, { main, assigned, fixed, size, shift = 0 } = {}) {
  const list = [...(displays || [])];
  const indexOf = (d) => {
    if (!d) return null;
    const i = list.indexOf(d);
    return i >= 0 ? i : list.push(d) - 1;
  };
  const opts = { main: indexOf(main), assigned: indexOf(assigned), fixed: !!fixed, size: size || null, shift };
  if (!list.length) return null;
  return call('place.restore', p, list.map(displayFacts), list.map((d) => rect(areaOf(d))), opts);
}

// settings.windowPlaces with `key` saved last, at most 200 apps (the oldest go).
export const putPlace = (all, key, p) => Object.fromEntries(call('place.put', Object.entries(all || {}), key, p));

// ---- the list of desktops (docs/DESKTOPS.md) ----
export const addDesktop = (state) => call('desk.add', { list: state.list, active: state.active });
export const removeDesktop = (state, id) => call('desk.remove', { list: state.list, active: state.active }, id);
export const neighbor = (list, active, step) => call('desk.neighbor', list, active, step);
export const arrowStep = (dir, rtl) => call('desk.arrow', dir, !!rtl);
export const swipeStep = (dir, rtl) => call('desk.swipe', dir, !!rtl);
export const cleanCount = (n) => call('desk.count', n);
export const cleanIndex = (i) => call('desk.index', i);

// ---- the Overview's and Snap Assist's layouts ----
// id → { x, y, w, h, scale, ly, lh }: each window as a miniature (frame: the size it is drawn at) and
// its title row (ly, lh) above it. kind: 'overview' (under the strip) or 'assist' (inside one zone).
export function gridLayout(list, a, frame = (w) => w, kind = 'overview') {
  const frames = list.map((w) => { const f = frame(w); return { w: f.w, h: f.h }; });
  const places = frames.length ? call('layout.grid', frames, rect(a), kind) : [];
  const out = new Map();
  list.forEach((w, i) => { if (places[i]) out.set(w.id, places[i]); });
  return out;
}
// The strip of desktops: { cards: [{ id, x, y, w, h }], add, h }. opts: { aspect, canAdd, rtl }.
export const stripLayout = (list, a, opts = {}) => call('layout.strip', list, rect(a), opts);

// ---- Settings → Displays' arrangement (docs/DISPLAYS.md, "Numbers, identify and snapping") ----
// Settings converts its 12 preview px with the preview's scale.
export const SNAP_PREVIEW_PX = 12;
const rects = (rs) => rs.map(rect);
// { [id]: 1, 2, 3… }: the main display first, then the server's order.
export const displayNumbers = (outputs) => call('display.numbers', (outputs || []).map((o) => o && { id: o.id, active: !!o.active, w: o.w, main: !!o.main }));
export const placementOk = (rs, i, x, y) => call('display.ok', rects(rs), i, x, y);
export const snapDisplay = (rs, i, x, y, threshold) => call('display.snap', rects(rs), i, x, y, threshold);
export const resolveDrop = (rs, i, x, y, threshold) => call('display.drop', rects(rs), i, x, y, threshold);
