// Transport and existing-renderer binding only. Rust owns network I/O, policy,
// credentials, the panel tree and the vector status icon.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, screen } from '@lordos/runtime';
import { ScrollView } from './components.jsx';
import { wm } from './windows.js';
import { dockGlass } from './glass.js';
import { Overlay } from './layers.js';
import { RustWidget } from './rustfiles.jsx';
import { useTheme } from './theme.js';
const colors = (t) => Object.fromEntries(['text', 'textDim', 'accent', 'hover', 'selection', 'panelBg', 'windowBorder', 'blur'].map((k) => [k, t[k]]));
const request = (value) => lord.net?.host ? lord.net.host(JSON.stringify(value)) : null;
export function RustNetworkPanel({ compact = false, onSettings }) {
  const theme = useTheme(), current = useRef(theme);
  current.current = theme;
  const id = useRef(null), [tree, setTree] = useState(null);
  const update = (op = 'poll', extra = {}) => {
    if (id.current == null) return;
    const result = request({ op, id: id.current, theme: colors(current.current), ...extra });
    if (result && !result.error) setTree(result);
  };
  useEffect(() => {
    id.current = request({ op: 'create', compact })?.id;
    update();
    const timer = setInterval(update, 500);
    return () => { clearInterval(timer); request({ op: 'close', id: id.current }); id.current = null; };
  }, []);
  useEffect(() => update(), [theme]);
  return tree ? h(RustWidget, { node: tree, send: (action, event, data) => { event?.stopPropagation?.(); if (action?.[0] === 'settings') { onSettings?.(); wm.open('settings', { args: { section: 'network' } }); return; } update('action', { action, ...data }); } }) : null;
}
export function NetworkConnectionIcon({ color }) {
  const [tree, setTree] = useState(null), previous = useRef('');
  useEffect(() => {
    const poll = () => { const result = request({ op: 'icon', color }); const encoded = JSON.stringify(result); if (encoded !== previous.current) { previous.current = encoded; setTree(result); } };
    poll(); const timer = setInterval(poll, 1000); return () => clearInterval(timer);
  }, [color]);
  return tree && !tree.error ? h(RustWidget, { node: tree, send: () => {} }) : null;
}

// A separate, compact network popover; scanning starts only while it is mounted.
export function NetworkPopover({ anchor, onClose, area }) {
  const theme = useTheme();
  const width = Math.min(320, (area?.w || screen.width) - 16);
  const rect = anchor?.current ? lord.getLayout(anchor.current.id) : [area?.x || 0, 0, 0, 0];
  const left = Math.max((area?.x || 0) + 8, Math.min(rect[0] + rect[2] - width,
    (area?.x || 0) + (area?.w || screen.width) - width - 8)) - rect[0];
  return h(Overlay, { anchor, onClose }, h(View, {
    style: { position: 'absolute', left, top: 28, width,
      padding: 12, borderRadius: 16, ...dockGlass(theme), boxShadow: '0 8px 24px rgba(0,0,0,0.3)' },
  }, h(ScrollView, { style: { flex: 0, flexShrink: 1, maxHeight: Math.min(440, (area?.h || screen.height) - 80) } },
    h(RustNetworkPanel, { compact: true, onSettings: onClose }))));
}
