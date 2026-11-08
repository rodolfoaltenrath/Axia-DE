// Existing renderer binding; Rust supplies the tray button, glass panel,
// application rows, menu tree and every action.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, addListener } from '@lordos/runtime';
import { RustWidget } from './rustfiles.js';
import { useTheme } from './theme.js';
import { Overlay } from './layers.js';
const request = (value) => lord.net?.tray ? lord.net.tray(JSON.stringify(value)) : null;
const colors = (t) => Object.fromEntries(['text', 'textDim', 'accent', 'hover', 'panelBg', 'windowBorder', 'blur'].map((k) => [k, t[k]]));
export function BackgroundApps({ color, onOpen }) {
  const theme = useTheme(), current = useRef({ theme, color });
  current.current = { theme, color };
  const id = useRef(null), button = useRef(null), panel = useRef(null);
  const [state, setState] = useState(null), latest = useRef(null);
  latest.current = state;
  const update = (op = 'poll', extra = {}) => {
    if (id.current == null) return;
    const result = request({ op, id: id.current, theme: colors(current.current.theme), color: current.current.color, ...extra });
    if (result && !result.error) setState(result);
  };
  const send = (action, event) => {
    event?.stopPropagation?.();
    if (action?.[0] === 'toggle' && !latest.current?.open) onOpen?.();
    update('action', { action, screenX: event?.x || 0, screenY: event?.y || 0 });
  };
  useEffect(() => {
    id.current = request({ op: 'create' })?.id; update();
    const timer = setInterval(update, 500);
    return () => { clearInterval(timer); request({ op: 'close', id: id.current }); id.current = null; };
  }, []);
  useEffect(() => update(), [theme, color]);
  useEffect(() => {
    if (!state?.open) return;
    const inside = (n, ref) => { for (; n; n = n.parent) if (n === ref.current) return true; return false; };
    const off = addListener('pointerdown', (e) => { if (!latest.current?.context && !inside(e.target, panel) && !inside(e.target, button)) send(['dismiss']); });
    const keys = addListener('keydown', (e) => { if (e.key === 'Escape') send(['dismiss']); });
    return () => { off(); keys(); };
  }, [state?.open]);
  if (!state?.button) return null;
  return h(View, { ref: button }, h(RustWidget, { node: state.button, send }),
    state.panel ? h(Overlay, { anchor: button }, h(View, { ref: panel }, h(RustWidget, { node: state.panel, send }))) : null);
}
