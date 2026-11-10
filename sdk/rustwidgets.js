// Host vocabulary for Rust-owned panel trees drawn through React (Calculator, Force Quit,
// About). Layout and interaction policies stay in Rust; this adapter binds the existing renderer,
// input and locale APIs. Monitor is a native view mounted by Rust (sdk/nativeview.js).
import { createElement as h } from 'react';
import { View, Text, Image } from '@lordos/runtime';
import { TextInput, Button, ScrollView } from './components.jsx';
import { t, formatDate } from './i18n.js';
import { errorText } from './errors.js';
export function caption(value) {
  if (value == null) return '';
  if (typeof value !== 'object') return String(value);
  if (value.error) return errorText(value.error);
  if ('date' in value) return formatDate(new Date(value.date), 'short');
  if (value.key) return t(value.key, Object.fromEntries(Object.entries(value.params || {}).map(([k, v]) => [k, typeof v === 'object' && v !== null ? caption(v) : v])));
  return '';
}
export function RustWidget({ node, send, icon }) {
  if (!node) return null;
  const props = { style: node.style };
  for (const [name, action] of Object.entries(node.events || {})) props[name] = event => { if (action[0] !== 'close') event?.stopPropagation?.(); return send(action); };
  switch (node.kind) {
    case 'view': return h(View, props, (node.children || []).map((child, i) => h(RustWidget, { key: child.key ?? i, node: child, send, icon })));
    case 'scroll': return h(ScrollView, props, (node.children || []).map((child, i) => h(RustWidget, { key: child.key ?? i, node: child, send, icon })));
    case 'button': return h(Button, { ...props, label: caption(node.label), disabled: node.disabled, destructive: node.destructive, onClick: node.disabled ? undefined : () => send(node.action, node.value === undefined ? {} : { value: node.value }) });
    case 'appIcon': return icon?.(node);
    case 'windowIcon': return icon?.(node) || h(View, { style: { width: node.size } });
    case 'text': return h(Text, props, caption(node.label));
    case 'image': return h(Image, { ...props, src: node.src });
    case 'input': return h(TextInput, { ...props, value: node.value, autoFocus: node.autoFocus, placeholder: caption(node.placeholder),
      onChange: value => node.change ? send(node.change, { value }) : send(['query', value]), onKeyDown: event => send(['key'], { event: event.key, char: event.char || '' }), onCancel: () => send(['close']) });
    default: return null;
  }
}
