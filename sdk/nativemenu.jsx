// A drop-down or context menu drawn by the Rust native view host (native_view/tree.rs and
// menu.rs, the "menuNativeView" setting) in place of sdk/menu.jsx. The same props: where it
// opens, the items with their callbacks, what closes it. Rust lays it out in the shell's popover
// layer (clamped to the screen, scrolling when taller, rows lit under the pointer, the submenu
// beside its row, the arrows, Enter and Escape); the callbacks stay here, run by `["run", n]`.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { NativeView } from './nativeview.js';
import { NATIVE_TREE, treeOwners } from './nativetree.js';

let sequence = 0;

// Items with callbacks → what Rust can hold: an action per item, numbered in order (the same items
// give the same numbers, so a re-render does not look like a new menu to Rust).
function describe(items, calls) {
  return (items || []).map((item) => {
    if (typeof item === 'string' || !item) return item;
    const out = {};
    for (const key of ['label', 'hint', 'header', 'disabled', 'checked', 'danger']) if (item[key] != null) out[key] = item[key];
    if (item.submenu) out.submenu = describe(item.submenu, calls);
    else { const id = calls.size + 1; calls.set(id, item.onClick); out.action = ['run', id]; }
    return out;
  });
}

export function NativeMenu({ x, y, items, onClose, width, above, centered, side }) {
  const [owner] = useState(() => `menu${++sequence}`);
  const calls = useRef(new Map()), close = useRef(onClose);
  close.current = onClose;
  calls.current = new Map();
  const tree = { kind: 'menu', key: 'menu', absolute: true, x, y, layer: 'popover', width, above, centered, side,
    items: describe(items, calls.current), events: { onDismiss: ['dismiss'] } };
  treeOwners.set(owner, ({ action }) => {
    if (action[0] === 'dismiss') { close.current(); return; }
    if (action[0] === 'run') { const run = calls.current.get(action[1]); close.current(); try { run?.(); } catch (e) { console.error('menu:', e); } }
  });
  useEffect(() => () => treeOwners.delete(owner), [owner]);
  return h(NativeView, { manifest: NATIVE_TREE, args: { tree, owner }, style: { position: 'absolute', left: 0, top: 0, width: 0, height: 0 } });
}
