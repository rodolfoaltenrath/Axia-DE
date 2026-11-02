// widgets.js — the desktop widget layer (for example, the Notes app's notes).
//
// An app registers a type and publishes its widgets; the theme draws the layer with <DesktopWidgets />:
// once between the wallpaper and the windows (layer="below") and again above the windows for those
// pinned to the front (layer="above").
//
//   registerWidgetType('note', NoteCard)              // NoteCard({ widget }) draws one
//   setWidgets('note', [{ id, x, y, w, h, pinned }])  // replaces those of that type
import { createElement as h } from 'react';
import { View } from '@lordos/runtime';
import { createStore, useStore } from './store.js';

const types = new Map();
export const desktopWidgets = createStore({});        // type → [widget]

export function registerWidgetType(type, Component) {
  types.set(type, Component);
  desktopWidgets.set((s) => ({ ...s }));
}

export function setWidgets(type, list) {
  desktopWidgets.set((s) => ({ ...s, [type]: list }));
}

export function DesktopWidgets({ layer = 'below' }) {
  const all = useStore(desktopWidgets);
  const items = [];
  for (const [type, list] of Object.entries(all)) {
    const C = types.get(type);
    if (!C || !list) continue;
    for (const w of list) if (!!w.pinned === (layer === 'above')) items.push(h(C, { key: `${type}:${w.id}`, widget: w }));
  }
  if (!items.length) return null;
  return h(View, { style: { position: 'absolute', left: 0, top: 0, width: 0, height: 0 } }, ...items);
}
