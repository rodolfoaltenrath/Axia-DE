// Transport and existing-renderer binding only. Rust owns network I/O, policy,
// credentials, the panel tree and the vector status icon.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { RustWidget } from './rustfiles.jsx';
import { useTheme } from './theme.js';
const colors = (t) => Object.fromEntries(['text', 'textDim', 'accent', 'hover', 'selection', 'panelBg', 'windowBorder', 'blur'].map((k) => [k, t[k]]));
const request = (value) => lord.net?.host ? lord.net.host(JSON.stringify(value)) : null;
export function RustNetworkPanel() {
  const theme = useTheme(), current = useRef(theme);
  current.current = theme;
  const id = useRef(null), [tree, setTree] = useState(null);
  const update = (op = 'poll', extra = {}) => {
    if (id.current == null) return;
    const result = request({ op, id: id.current, theme: colors(current.current), ...extra });
    if (result && !result.error) setTree(result);
  };
  useEffect(() => {
    id.current = request({ op: 'create' })?.id;
    update();
    const timer = setInterval(update, 500);
    return () => { clearInterval(timer); request({ op: 'close', id: id.current }); id.current = null; };
  }, []);
  useEffect(() => update(), [theme]);
  return tree ? h(RustWidget, { node: tree, send: (action, event, data) => { event?.stopPropagation?.(); update('action', { action, ...data }); } }) : null;
}
export function NetworkConnectionIcon({ color }) {
  const [tree, setTree] = useState(null), previous = useRef('');
  useEffect(() => {
    const poll = () => { const result = request({ op: 'icon', color }); const encoded = JSON.stringify(result); if (encoded !== previous.current) { previous.current = encoded; setTree(result); } };
    poll(); const timer = setInterval(poll, 1000); return () => clearInterval(timer);
  }, [color]);
  return tree && !tree.error ? h(RustWidget, { node: tree, send: () => {} }) : null;
}
