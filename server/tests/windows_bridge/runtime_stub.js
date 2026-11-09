// The parts of @lordos/runtime sdk/windows.js uses, without axia-server (runtime/index.js's own rules).
export const screen = { width: 1280, height: 800, displays: [{ id: 0, name: 'A', x: 0, y: 0, w: 1280, h: 800, scale: 100, main: true }], mirror: false };
const screenListeners = new Set();
export function onScreenChange(fn) { screenListeners.add(fn); return () => screenListeners.delete(fn); }
export function __screenChanged(displays) { const old = { ...screen, displays: screen.displays }; screen.displays = displays; for (const fn of screenListeners) fn(screen, old); }
export function addListener() { return () => {}; }
export const mainDisplay = () => screen.displays.find((d) => d.main) || screen.displays[0];
export function displayAt(x, y) {
  let best = null, dist = Infinity;
  for (const d of screen.displays) {
    const dx = Math.max(d.x - x, 0, x - (d.x + d.w - 1)), dy = Math.max(d.y - y, 0, y - (d.y + d.h - 1));
    const k = dx * dx + dy * dy;
    if (k < dist) { dist = k; best = d; }
  }
  return best || mainDisplay();
}
export function displayFor(r) {
  let best = null, area = 0;
  for (const d of screen.displays) {
    const w = Math.min(r.x + r.w, d.x + d.w) - Math.max(r.x, d.x), h = Math.min(r.y + r.h, d.y + d.h) - Math.max(r.y, d.y);
    if (w > 0 && h > 0 && w * h > area) { area = w * h; best = d; }
  }
  return best || displayAt(r.x + r.w / 2, r.y + r.h / 2);
}
