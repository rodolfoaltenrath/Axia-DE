// clipboard.js — the session clipboard and its (advanced) history.
//
// A single one for the whole session: shared by the terminal, text fields and Linux apps
// (Wayland; the compositor bridges both ways). The history keeps everything that gets
// copied in any of them —text, links, colors, code, paths and images— with its
// origin and time. It is stored in ~/.local/share/axia/clipboard/ (history.json + img-*.png) and
// survives a reboot; it is turned off or limited in Settings → Clipboard.
//
// Never stored: what was copied from a password field (clipboard.set(t, { secure: true })), what
// a Linux app marks as secret (x-kde-passwordManagerHint, like password
// managers do) and what comes from apps the user excluded.
//
//   clipboard.get() / clipboard.set(text, { secure, origin })
//   useStore(clipboardHistory) → [{ id, kind: 'text'|'image', type, text?, image?, origin, time, pinned }]
//   <ClipboardPanel /> (❖V quick, ❖⇧V full) is mounted by startShell: every theme has it.
import { createElement as h, useEffect, useMemo, useRef, useState } from 'react';
import { View, Text, Image, addListener, screen, mainBox } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { useTheme } from './theme.js';
import { t, formatDate } from './i18n.js';
import { settings } from './settings.js';
import { wm } from './windows.js';
import { TextInput, focusToken, restoreFocus, pasteIntoFocused } from './components.jsx';

export const DEFAULTS = { save: true, max: 500, maxMB: 200, expireDays: 30, ignoreApps: [], clearOnLogout: false };
const conf = () => ({ ...DEFAULTS, ...(settings.get().clipboard || {}) });
export const clipboardHistory = createStore([]);

// Thin projection/effect adapter. Rust owns history, classification, limits,
// deduplication, privacy and disk work. No filesystem calls or history timers.
const supported = () => typeof lord.clipboard.historyRequest === 'function';
const request = (op, args = {}) => supported() ? lord.clipboard.historyRequest(JSON.stringify({ op, ...args })) : null;
let lastVersion = 0, lastConfig = '', lastError = '';
const mirror = new Map();
const viewStore = createStore(null);
let viewSequence = 0;
globalThis.__axiaClipboardHistory = (reply) => {
  if (reply.error && reply.error !== lastError) { lastError = reply.error; console.warn('clipboard:', reply.error); }
  if (reply.view) viewStore.set(reply.view);
  if (reply.version <= lastVersion) return;
  lastVersion = reply.version;
  for (const item of reply.upsert || []) {
    const previous = mirror.get(item.id);
    if (previous || item.text != null || item.image) mirror.set(item.id, previous ? { ...previous, ...item } : item);
  }
  const ids = new Set(reply.order || []);
  for (const id of mirror.keys()) if (!ids.has(id)) mirror.delete(id);
  clipboardHistory.set((reply.order || []).map((id) => mirror.get(id)).filter(Boolean));
};
function configure() {
  const config = conf(), encoded = JSON.stringify(config);
  if (encoded === lastConfig) return;
  const reset = !lastConfig; lastConfig = encoded; request('configure', { config, reset });
}
export const clipboardKind = (item) => lord.clipboard.historyKind(item.text || '', !!item.image);
const boundedText = (value, limit) => { const text = value.slice(0, limit), last = text.charCodeAt(text.length - 1); return last >= 0xd800 && last <= 0xdbff ? text.slice(0, -1) : text; };
const remember = (text, origin) => {
  if (!text) return;
  const state = lord.clipboard.state?.(), current = text === lord.clipboard.get();
  if (current && state?.sensitive) return;
  request('remember', { text: boundedText(String(text), 1048576), origin: origin ?? (current ? state?.origin : null) ?? 'lordos' });
};
const load = () => { request('load'); };
export const clipboard = {
  get: () => lord.clipboard.get(),
  set(text, { secure = false, origin = 'lordos' } = {}) {
    lord.clipboard.set(String(text), { secure, origin });
    // The compositor captures the completed copy, including its secure marker.
  },
  use(item) {
    if (item.image) { if (!lord.clipboard.useImage(item.image)) return false; }
    else if (item.text != null) lord.clipboard.set(item.text, { origin: item.origin || 'lordos' });
    else return false;
    lord.clipboard.historySuppress?.();
    request('use', { id: item.id }); return true;
  },
  pin: (item) => request('pin', { id: item.id }),
  remove: (item) => request('remove', { id: item.id }),
  clear: ({ keepPinned = true } = {}) => request('clear', { keepPinned }),
  // Only the explicit session-exit boundary waits (at most two seconds) for
  // worker writes; normal copies and panel operations never wait for disk.
  beforeLogout: () => request('flush', { logout: true }),
  paste(item, target) {
    if (!clipboard.use(item)) return 'copied';
    if (target?.text && (restoreFocus(target.text), pasteIntoFocused())) return 'pasted';
    if (target?.linux && typeof lord.wayland !== 'undefined' && lord.wayland.hasKeyboard() && lord.wayland.paste()) return 'pasted';
    return 'copied';
  },
  remember, load,
};
settings.subscribe(configure);
configure();

// ---- panel (❖V quick, ❖⇧V full) ----

export const clipboardPanel = createStore(false);       // false | 'quick' | 'full'
let target = null;                                       // where to paste on choosing
function openPanel(mode) {
  if (!clipboardPanel.get()) {
    const top = wm.focused();
    target = { text: focusToken(), linux: !!(top && String(top).startsWith('linux#')) };
  }
  clipboardPanel.set(mode);
}
export const closeClipboardPanel = () => { clipboardPanel.set(false); setTimeout(() => restoreFocus(target?.text), 0); };
// ❖V and ❖⇧V: the keys are in sdk/shortcuts.js.
export function toggleClipboardPanel(mode) {
  if (clipboardPanel.get() === mode) clipboardPanel.set(false); else openPanel(mode);
}
addListener('keydown', (e) => { if (e.key === 'Escape' && clipboardPanel.get()) clipboardPanel.set(false); });

const FILTERS = ['all', 'text', 'link', 'image', 'code', 'color', 'file', 'pinned'];
const TYPE_GLYPH = { text: '¶', link: '🔗', image: '🖼', code: '‹›', color: '●', file: '📁' };

function relTime(ms) {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return t('shell.clipboard.now');
  if (s < 3600) return t('shell.clipboard.minutes', { count: Math.floor(s / 60) });
  if (s < 86400) return t('shell.clipboard.hours', { count: Math.floor(s / 3600) });
  return formatDate(new Date(ms), 'short');
}
const originLabel = (o) => (!o || o === 'lordos' ? 'Axia DE' : o === 'terminal' ? t('shell.clipboard.fromTerminal') : o === 'linux' ? 'Linux' : o.replace(/^.*\./, ''));

function Swatch({ color, size = 14 }) {
  return h(View, { style: { width: size, height: size, borderRadius: 3, backgroundColor: color, border: '1px solid rgba(128,128,128,0.5)' } });
}

function Row({ item, active, th, onPick, onHover, compact }) {
  const fg = active ? '#fff' : th.text, dim = active ? 'rgba(255,255,255,0.8)' : th.textDim;
  return h(View, {
    onClick: onPick, onMouseEnter: onHover,
    style: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: 8, paddingVertical: compact ? 5 : 7, borderRadius: 7,
      backgroundColor: active ? th.accent : 'transparent' },
  },
  item.type === 'color' ? h(Swatch, { color: item.text.trim() }) : h(Text, { style: { width: 16, fontSize: 11, color: dim, textAlign: 'center' } }, TYPE_GLYPH[item.type] || '¶'),
  item.image
    ? h(Image, { src: item.image, style: { width: 64, height: 36, objectFit: 'contain' } })
    : h(Text, { style: { flex: 1, fontSize: 12, color: fg, whiteSpace: 'nowrap', overflow: 'hidden', fontFamily: item.type === 'code' ? 'monospace' : undefined } },
      item.preview ?? item.text.slice(0, 160)),
  item.image ? h(View, { style: { flex: 1 } }) : null,
  item.pinned ? h(Text, { style: { fontSize: 11, color: fg } }, '📌') : null,
  h(Text, { style: { fontSize: 10, color: dim } }, relTime(item.time)));
}

export function ClipboardPanel() {
  const mode = useStore(clipboardPanel);
  if (!mode) return null;
  return h(mode === 'full' ? FullPanel : QuickPanel);
}

function pick(item) {
  clipboardPanel.set(false);
  setTimeout(() => {
    const r = clipboard.paste(item, target);
    if (r === 'copied') { restoreFocus(target?.text); toast.set(t('shell.clipboard.copiedOnly')); }
  }, 0);
}
export const toast = createStore(null);

// Quick (❖V): the latest, with search; 1–9 choose.
function QuickPanel() {
  const th = useTheme();
  useEffect(() => load(), []);
  const list = useStore(clipboardHistory);
  const [q, setQ] = useState('');
  const [sel, setSel] = useState(0);
  const close = closeClipboardPanel;
  const { items: shown } = useClipboardView(list, q, 'all', 12);
  const W = 420;
  const onKey = (e) => {
    if (e.key === 'ArrowDown') { setSel((s) => Math.min(shown.length - 1, s + 1)); return true; }
    if (e.key === 'ArrowUp') { setSel((s) => Math.max(0, s - 1)); return true; }
    if (e.key === 'Enter') { if (shown[sel]) pick(shown[sel]); return true; }
    if (/^[1-9]$/.test(e.char || '') && !q) { const it = shown[Number(e.char) - 1]; if (it) pick(it); return true; }
    return false;
  };
  return h(View, { onClick: close, style: { position: 'absolute', ...mainBox() } },
    h(View, {
      onClick: (e) => e.stopPropagation(),
      style: { position: 'absolute', left: Math.round((mainBox().width - W) / 2), top: 70, width: W, padding: 10, gap: 4, borderRadius: 12,
        backgroundColor: th.windowBg, border: `1px solid ${th.separator}`, boxShadow: '0 16px 40px rgba(0,0,0,0.45)' },
    },
    h(View, { style: { flexDirection: 'row', alignItems: 'center', gap: 8, marginBottom: 4 } },
      h(Text, { style: { fontSize: 13, fontWeight: 700, color: th.text } }, t('shell.clipboard.title')),
      h(TextInput, { value: q, onChange: (v) => { setQ(boundedText(v, 128)); setSel(0); }, autoFocus: true, placeholder: t('shell.clipboard.search'), onKeyDown: onKey,
        onCancel: close, style: { flex: 1 } }),
      h(View, { onClick: () => openPanel('full'), style: { paddingHorizontal: 8, paddingVertical: 4, borderRadius: 6, backgroundColor: th.control } },
        h(Text, { style: { fontSize: 11, color: th.text } }, t('shell.clipboard.more')))),
    shown.length === 0 && h(Text, { style: { fontSize: 12, color: th.textDim, padding: 8 } }, t('shell.clipboard.empty')),
    ...shown.map((item, i) => h(Row, { key: item.id || i, item, th, active: i === sel, compact: true, onHover: () => setSel(i), onPick: () => pick(item) })),
    h(Text, { style: { fontSize: 10, color: th.textDim, paddingHorizontal: 4, marginTop: 4 } }, t('shell.clipboard.hint'))));
}

// Queries/filter counts/pinned ordering are prepared by the same Rust worker.
// React only resolves IDs from the current projection; hovering a row reuses it.
function useClipboardView(list, query, filter, limit) {
  const view = useStore(viewStore);
  useEffect(() => { request('view', { query, filter, limit, generation: ++viewSequence }); }, [list, query, filter, limit]);
  return useMemo(() => view && view.query === query && view.filter === filter && view.limit === limit
    ? { items: view.ids.map((id) => mirror.get(id)).filter(Boolean), counts: view.counts }
    : { items: [], counts: {} }, [view, list, query, filter, limit]);
}

// Full (❖⇧V): filters, list, preview and actions.
function FullPanel() {
  const th = useTheme();
  useEffect(() => load(), []);
  const list = useStore(clipboardHistory);
  const [q, setQ] = useState('');
  const [filter, setFilter] = useState('all');
  const [sel, setSel] = useState(0);
  const [confirm, setConfirm] = useState(false);
  const close = closeClipboardPanel;
  const { items: shown, counts } = useClipboardView(list, q, filter, 8192);
  const cur = shown[Math.min(sel, shown.length - 1)] || null;
  const W = Math.min(860, mainBox().width - 40), H = Math.min(540, mainBox().height - 120);
  const onKey = (e) => {
    if (e.key === 'ArrowDown') { setSel((s) => Math.min(shown.length - 1, s + 1)); return true; }
    if (e.key === 'ArrowUp') { setSel((s) => Math.max(0, s - 1)); return true; }
    if (e.key === 'Enter') { if (cur) pick(cur); return true; }
    if (e.key === 'Delete' && cur) { clipboard.remove(cur); return true; }
    return false;
  };
  const btn = (label, onClick, danger) => h(View, { onClick, style: { paddingHorizontal: 10, paddingVertical: 5, borderRadius: 6, backgroundColor: danger ? '#ff453a' : th.control } },
    h(Text, { style: { fontSize: 11, fontWeight: 600, color: danger ? '#fff' : th.text } }, label));
  const count = (f) => counts[f] || 0;
  return h(View, { onClick: close, style: { position: 'absolute', ...mainBox(), backgroundColor: 'rgba(0,0,0,0.25)' } },
    h(View, {
      onClick: (e) => e.stopPropagation(),
      style: { position: 'absolute', left: Math.round((mainBox().width - W) / 2), top: Math.round((mainBox().height - H) / 2), width: W, height: H,
        borderRadius: 14, backgroundColor: th.windowBg, border: `1px solid ${th.separator}`, boxShadow: '0 24px 60px rgba(0,0,0,0.5)', overflow: 'hidden' },
    },
    h(View, { style: { flexDirection: 'row', alignItems: 'center', gap: 10, padding: 12 } },
      h(Text, { style: { fontSize: 15, fontWeight: 700, color: th.text } }, t('shell.clipboard.title')),
      h(TextInput, { value: q, onChange: (v) => { setQ(boundedText(v, 128)); setSel(0); }, autoFocus: true, placeholder: t('shell.clipboard.search'), onKeyDown: onKey, onCancel: close, style: { flex: 1 } }),
      confirm ? btn(t('shell.clipboard.confirmClear'), () => { clipboard.clear({ keepPinned: true }); setConfirm(false); }, true) : btn(t('shell.clipboard.clear'), () => setConfirm(true)),
      btn('✕', close)),
    h(View, { style: { height: 1, backgroundColor: th.separator } }),
    h(View, { style: { flex: 1, flexDirection: 'row' } },
      h(View, { style: { width: 150, padding: 8, gap: 2, backgroundColor: th.sidebarBg } },
        ...FILTERS.map((f) => h(View, { key: f, onClick: () => { setFilter(f); setSel(0); },
          style: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 8, height: 28, borderRadius: 6, backgroundColor: filter === f ? th.accent : 'transparent' } },
        h(Text, { style: { flex: 1, fontSize: 12, color: filter === f ? '#fff' : th.text } }, t(`shell.clipboard.filter.${f}`)),
        h(Text, { style: { fontSize: 10, color: filter === f ? '#fff' : th.textDim } }, String(count(f)))))),
      h(View, { style: { flex: 1, padding: 6, gap: 1, overflow: 'hidden' } },
        shown.length === 0 && h(Text, { style: { fontSize: 12, color: th.textDim, padding: 10 } }, t('shell.clipboard.empty')),
        ...shown.slice(Math.max(0, Math.min(sel - 8, shown.length - 16)), Math.max(16, sel + 8)).map((item) => {
          const i = shown.indexOf(item);
          return h(Row, { key: item.id || i, item, th, active: i === sel, onHover: () => setSel(i), onPick: () => pick(item) });
        })),
      h(View, { style: { width: 1, backgroundColor: th.separator } }),
      h(View, { style: { width: 280, padding: 12, gap: 8 } },
        cur ? [
          cur.image ? h(Image, { key: 'i', src: cur.image, style: { width: 256, height: 200, objectFit: 'contain' } })
            : cur.type === 'color' ? h(View, { key: 'c', style: { flexDirection: 'row', alignItems: 'center', gap: 10 } }, h(Swatch, { color: cur.text.trim(), size: 48 }),
              h(Text, { style: { fontSize: 16, fontWeight: 700, color: th.text, fontFamily: 'monospace' } }, cur.text.trim()))
              : h(Text, { key: 't', style: { fontSize: 11, color: th.text, fontFamily: cur.type === 'code' ? 'monospace' : undefined, maxHeight: 280, overflow: 'hidden', lineHeight: 1.35 } },
                cur.text.slice(0, 3000)),
          h(Text, { key: 'm', style: { fontSize: 11, color: th.textDim, lineHeight: 1.4 } },
            t('shell.clipboard.meta', { type: t(`shell.clipboard.filter.${cur.type}`), origin: originLabel(cur.origin), when: relTime(cur.time),
              size: cur.text ? t('shell.clipboard.chars', { count: cur.characters ?? cur.text.length }) : '' })),
          h(View, { key: 'a', style: { flexDirection: 'row', flexWrap: 'wrap', gap: 6 } },
            btn(t('shell.clipboard.paste'), () => pick(cur)),
            btn(cur.pinned ? t('shell.clipboard.unpin') : t('shell.clipboard.pin'), () => clipboard.pin(cur)),
            btn(t('shell.clipboard.delete'), () => clipboard.remove(cur))),
        ] : h(Text, { style: { fontSize: 12, color: th.textDim } }, t('shell.clipboard.noSelection')))),
    h(View, { style: { height: 1, backgroundColor: th.separator } }),
    h(Text, { style: { fontSize: 10, color: th.textDim, paddingHorizontal: 12, paddingVertical: 6 } },
      conf().save ? t('shell.clipboard.fullHint') : t('shell.clipboard.notSaved'))));
}

// Short notice (for example: "copied; paste with Ctrl+V").
export function ClipboardToast() {
  const msg = useStore(toast);
  const th = useTheme();
  const timer = useRef(null);
  useEffect(() => {
    if (!msg) return undefined;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => toast.set(null), 2500);
    return () => clearTimeout(timer.current);
  }, [msg]);
  if (!msg) return null;
  return h(View, { style: { position: 'absolute', left: Math.round(mainBox().left + mainBox().width / 2 - 170), width: 340, top: mainBox().top + mainBox().height - 150, alignItems: 'center', pointerEvents: 'none' } },
    h(View, { style: { paddingHorizontal: 14, paddingVertical: 8, borderRadius: 10, backgroundColor: th.windowBg, border: `1px solid ${th.separator}`, boxShadow: '0 8px 24px rgba(0,0,0,0.35)' } },
      h(Text, { style: { fontSize: 12, color: th.text } }, msg)));
}
