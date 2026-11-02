// statusitems.js — status icons in the theme's bar (menu bar extras).
//
//   const off = registerStatusItem('clock', ClockStatus, 10)   // ClockStatus({ color }) draws its own
//   <StatusItems color={t.menuText} />                          // the theme shows them wherever it wants
//
// It is optional: a theme that does not draw <StatusItems /> simply doesn't show them. The default
// and classic themes put them next to the privacy indicators. An item with nothing to show returns
// null.
import { createElement as h } from 'react';
import { View } from '@lordos/runtime';
import { createStore, useStore } from './store.js';

export const statusItems = createStore([]);          // [{ id, Component, order }]

export function registerStatusItem(id, Component, order = 0) {
  statusItems.set((l) => [...l.filter((x) => x.id !== id), { id, Component, order }].sort((a, b) => a.order - b.order));
  return () => statusItems.set((l) => l.filter((x) => x.id !== id));
}

export function StatusItems({ color, height = 22 }) {
  const list = useStore(statusItems);
  if (!list.length) return null;
  return h(View, { style: { flexDirection: 'row', alignItems: 'center', height } },
    ...list.map(({ id, Component }) => h(Component, { key: id, color, height })));
}
