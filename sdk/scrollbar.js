// scrollbar.js — the model of the overlay scroll bar drawn by ScrollView (sdk/components.jsx).
// Pure functions, no engine calls, so tests/scrollbar_model_test.mjs checks them under node.
// See docs/DESIGN.md, "Scroll bars".
//
// The bar is drawn over the content at the very edge (the end edge: right, or left in RTL), never
// reserving a strip: content keeps its normal padding and the thin bar sits in it. It is hidden
// until the view scrolls (wheel, touchpad, keyboard, dragging the thumb) or, in 'auto', until the
// pointer comes near the edge; it fades out a second after the last scroll.

export const SCROLLBAR = {
  thin: 6,          // width of the thumb at rest
  wide: 10,         // width while the pointer is over it or it is being dragged
  inset: 2,         // from the edge of the view (and from its top and bottom)
  hit: 14,          // width of the strip that takes the pointer while the bar is shown
  edge: 16,         // pointer this close to the edge shows the bar ('auto')
  minThumb: 24,     // the thumb never gets shorter
  hideAfter: 1000,  // ms after the last scroll before it starts fading
  fade: 200,        // ms of the fade out (none with reduce motion)
};

// Settings → Appearance → Show scroll bars
export const SCROLLBAR_MODES = ['auto', 'scrolling', 'always'];
export const scrollbarMode = (m) => (SCROLLBAR_MODES.includes(m) ? m : 'auto');

// Thumb geometry for a view `view` px high showing `content` px of content scrolled by `offset`.
// null when everything fits (no bar at all).
export function thumbGeometry(view, content, offset) {
  if (!(view > 0) || !(content > view)) return null;
  const max = content - view;
  const track = Math.max(0, view - SCROLLBAR.inset * 2);
  const size = Math.min(track, Math.max(SCROLLBAR.minThumb, (track * view) / content));
  const y = Math.max(0, Math.min(max, offset));
  const top = SCROLLBAR.inset + (track - size) * (y / max);
  return { max, track, size, top };
}

// Dragging: the offset that puts the thumb's top at `thumbTop` (view coordinates).
export function offsetForThumb(view, content, thumbTop) {
  const g = thumbGeometry(view, content, 0);
  if (!g) return 0;
  const room = g.track - g.size;
  if (room <= 0) return 0;
  const f = Math.max(0, Math.min(1, (thumbTop - SCROLLBAR.inset) / room));
  return f * g.max;
}

// Distance of a point (x relative to the view's left edge) from the bar's edge.
const fromEdge = (x, width, rtl) => (rtl ? x : width - x);

// Is the pointer near the edge where the bar lives?
export const nearEdge = (x, width, rtl) => {
  const d = fromEdge(x, width, rtl);
  return d >= 0 && d <= SCROLLBAR.edge;
};

// Does a point (view coordinates) fall on the thumb's hit strip?
export function hitThumb(x, y, width, geom, rtl) {
  if (!geom) return false;
  const d = fromEdge(x, width, rtl);
  return d >= 0 && d <= SCROLLBAR.hit && y >= geom.top && y <= geom.top + geom.size;
}

// Whether the bar is shown at time `now` (ms). s = { mode, lastScroll, near, hover, dragging }.
export function barShown(s, now) {
  const mode = scrollbarMode(s.mode);
  if (mode === 'always' || s.dragging) return true;
  if (s.lastScroll != null && now - s.lastScroll < SCROLLBAR.hideAfter) return true;
  return mode === 'auto' && !!(s.near || s.hover);
}

// Opacity at `now`: 1 while shown, then a linear fade over SCROLLBAR.fade ms after the moment it
// stopped being shown (`hiddenAt`). With reduce motion it just goes from 1 to 0.
export function barOpacity(s, now, hiddenAt, reduceMotion) {
  if (barShown(s, now)) return 1;
  if (reduceMotion || hiddenAt == null) return 0;
  const k = (now - hiddenAt) / SCROLLBAR.fade;
  return k >= 1 ? 0 : Math.max(0, 1 - k);
}

// When the bar stops being shown if nothing else happens (ms), or null if it stays.
export function hideTime(s) {
  const mode = scrollbarMode(s.mode);
  if (mode === 'always' || s.dragging) return null;
  if (mode === 'auto' && (s.near || s.hover)) return null;
  return s.lastScroll == null ? 0 : s.lastScroll + SCROLLBAR.hideAfter;
}

// Thumb width: wide under the pointer or while dragging.
export const barWidth = (s) => (s.hover || s.dragging ? SCROLLBAR.wide : SCROLLBAR.thin);
