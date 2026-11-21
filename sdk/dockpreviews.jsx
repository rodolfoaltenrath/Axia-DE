// Renderer binding: Rust owns hover state, grace time, layout, paging and capture budgets.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, Text, Image, addListener } from '@lordos/runtime';
import { windows, wm, windowTitle, getApp } from './windows.js';
import { dockIdOfWindow } from './dock.js';
import { windowIcon } from './apps.js';
import { useStore } from './store.js';
import { Layer } from './layers.js';
import { dockGlass } from './glass.js';
import { ToolbarButton } from './controls.jsx';
import { t } from './i18n.js';
const request = (v) => lord.wm?.preview?.(JSON.stringify(v));
export function useDockPreviews({ hit, anchor, area, position, disabled, theme }) {
  const all = useStore(windows);
  const [state, setState] = useState(null);
  const control = useRef({ id: null, inside: false, key: '', state: null, cursor: 0, timer: 0, deadline: 0 });
  const images = useRef(new Map()), props = useRef(null), poll = useRef(null);
  props.current = { all, hit, anchor, area, position, disabled };
  const sync = (op = 'poll', extra = {}, capture = false) => {
    const c = control.current, p = props.current;
    if (!c.id) return;
    const blocked = p.disabled || !!lord.wayland?.fullscreen?.();
    const hoverKey = p.hit && !p.hit.trash && !p.hit.launcher ? p.hit.id : '';
    const hovered = blocked ? '' : hoverKey;
    if (hovered) c.key = hovered;
    const mine = p.all.filter((w) => dockIdOfWindow(w) === c.key && !w.pip).slice(-128).reverse();
    const out = request({ op: op === 'activate' ? op : blocked ? 'dismiss' : op, id: c.id, key: hovered, hoverKey, inside: c.inside,
      windows: mine.map((w) => {
        const node = w.protected ? 0 : wm.previewNode(w.id), surface = w.protected ? 0 : (w.args?.surface || 0);
        const [width, height] = lord.wm.previewSize?.(node, surface) || [w.w, w.h];
        return { id: w.id, node, surface, width, height, title: windowTitle(w), protected: !!w.protected };
      }),
      ax: p.area.x, ay: p.area.y, aw: p.area.w, ah: p.area.h, side: p.position,
      x: p.anchor?.x, y: p.anchor?.y, ...extra });
    if (!out) return;
    const firstOpen = out.open && !c.state?.open;
    // Remaining hover time only schedules work; it does not change anything on screen.
    const signature = out.open ? JSON.stringify(out) : 'closed';
    if (signature !== c.signature) { c.signature = signature; c.state = out; setState(out); }
    if (out.open && capture) {
      // Round-robin; at most one capture per tick, with the real budget enforced in Rust.
      for (let j = 0; j < out.cards.length; j++) {
        const i = (c.cursor + j) % out.cards.length, card = out.cards[i];
        const image = images.current.get(card.id);
        const take = card.surface ? lord.wm.thumbnailSurface : lord.wm.thumbnail;
        if (image && take && (card.surface || card.node) && take(c.id, card.surface || card.node, image.id, Math.ceil(card.imageWidth), Math.ceil(card.imageHeight))) {
          c.cursor = (i + 1) % out.cards.length; break;
        }
      }
    }
    if (out.open || out.pending) {
      // Timing stays outside renderer state. A pointer departure can shorten an
      // already scheduled capture backoff, without delaying an earlier timer.
      const delay = Math.max(1, Math.ceil(out.pending ? out.waitMs ?? 30 : firstOpen ? 10 : request({ op: 'schedule', id: c.id })?.waitMs ?? 1000 / 20));
      const deadline = lord.now() + delay;
      if (!c.timer || deadline < c.deadline - 1) {
        clearTimeout(c.timer); c.deadline = deadline;
        c.timer = setTimeout(() => { c.timer = 0; c.deadline = 0; poll.current?.('poll', {}, true); }, delay);
      }
    } else { clearTimeout(c.timer); c.timer = 0; c.deadline = 0; }
  };
  poll.current = sync;
  useEffect(() => {
    const c = control.current; c.id = request({ op: 'create' })?.id; sync();
    return () => { clearTimeout(c.timer); request({ op: 'close', id: c.id }); c.id = null; images.current.clear(); };
  }, []);
  useEffect(() => sync(), [hit?.id, disabled, all, position, area.x, area.y, area.w, area.h, anchor?.x, anchor?.y]);
  useEffect(() => {
    if (!state?.open) return;
    const close = () => { control.current.inside = false; sync('dismiss'); };
    const off = addListener('pointerdown', (e) => {
      const s = control.current.state;
      if (s?.open && (e.x < s.left || e.x >= s.left + s.width || e.y < s.top || e.y >= s.top + s.height)) close();
    });
    const keys = addListener('keydown', (e) => { if (e.key === 'Escape') close(); });
    return () => { off(); keys(); };
  }, [state?.open]);
  const inside = (yes) => { control.current.inside = yes; sync(); };
  const panel = !state?.open ? null : h(Layer, { name: 'popover' }, h(View, {
    onMouseEnter: () => inside(true), onMouseLeave: () => inside(false),
    style: { position: 'absolute', left: state.left, top: state.top, width: state.width, height: state.height,
      padding: 8, borderRadius: 16, ...dockGlass(theme), boxShadow: '0 8px 24px rgba(0,0,0,0.3)', direction: 'ltr' },
  }, h(View, { style: { flexDirection: 'row', gap: 8 } }, state.cards.map((card) => {
    const win = all.find((w) => w.id === card.id), Icon = win ? windowIcon(win, getApp) : null;
    return h(View, { key: card.id, onClick: () => { sync('dismiss'); if (wm.get(card.id)) wm.focus(card.id); },
      style: { width: card.width, borderRadius: 10, overflow: 'hidden', cursor: 'pointer' } },
    h(View, { style: { height: 30, flexDirection: 'row', alignItems: 'center', paddingLeft: 6, gap: 4 } },
      h(Text, { style: { flex: 1, minWidth: 0, fontSize: 12, color: theme.text,
        whiteSpace: 'nowrap', textOverflow: 'ellipsis' } }, card.title),
      h(View, { onClick: (e) => e.stopPropagation(), style: { flexShrink: 0 } },
        h(ToolbarButton, { icon: '×', title: t('shell.window.close'), small: true,
          style: { width: 26, height: 26, borderRadius: 6 },
          onClick: () => {
            const win = wm.get(card.id);
            // Wayland closes asynchronously: false means the request was sent,
            // while an in-process guard may already be asking about unsaved work.
            if (win && !wm.close(card.id) && !win.args?.surface) sync('dismiss');
          } }))),
    h(View, { style: { width: card.width, height: card.imageHeight, alignItems: 'center', justifyContent: 'center' } },
      Icon ? h(Icon, { size: 40 }) : null,
      (card.node || card.surface) ? h(Image, { ref: (inst) => { if (inst) images.current.set(card.id, inst); else images.current.delete(card.id); },
        style: { position: 'absolute', left: (card.width - card.imageWidth) / 2, top: 0, width: card.imageWidth, height: card.imageHeight, objectFit: 'contain', pointerEvents: 'none' } }) : null));
  })), state.pages > 1 ? h(View, { style: { height: 28, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 14 } },
    h(View, { onClick: () => sync('page', { back: true }), style: { width: 28, height: 26, alignItems: 'center', justifyContent: 'center', cursor: 'pointer' } }, h(Text, { style: { color: theme.text } }, '‹')),
    h(Text, { style: { color: theme.textDim, fontSize: 11 } }, `${state.page + 1} / ${state.pages}`),
    h(View, { onClick: () => sync('page'), style: { width: 28, height: 26, alignItems: 'center', justifyContent: 'center', cursor: 'pointer' } }, h(Text, { style: { color: theme.text } }, '›'))) : null));
  return { open: !!state?.open, panel, activate: (key) => sync('activate', { key }) };
}
