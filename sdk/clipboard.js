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
import { createElement as h, useEffect, useRef } from 'react';
import { addListener, screen, mainBox } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { useTheme } from './theme.js';
import { t, locale } from './i18n.js';
import { settings } from './settings.js';
import { wm } from './windows.js';
import { focusToken, restoreFocus, pasteIntoFocused } from './components.jsx';
import { RustWidget } from './rustwidgets.jsx';

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

export const clipboardPanel = createStore(false);
export const toast = createStore(null);
const panelProjection = createStore({ tree: null });
let target = null;
function projectPanel(reply) {
  if (!reply?.mode) { panelProjection.set({ tree: null }); return; }
  // Rust chooses the visible IDs and preview bound. Never serialize the entire
  // history or a large copied string back into the panel controller.
  const ids = new Set(reply.visible || []); if (reply.selected) ids.add(reply.selected);
  const rows = {};
  for (const id of ids) {
    const it = mirror.get(id); if (!it) continue;
    rows[id] = { id, type: it.type, image: it.image, origin: it.origin, time: it.time, pinned: it.pinned,
      preview: it.preview, characters: it.characters, color: it.type === 'color' ? boundedText(it.text, reply.textLimit).trim() : undefined,
      text: id === reply.selected && it.text != null ? boundedText(it.text, reply.textLimit) : undefined };
  }
  const rendered = request('panel', { cmd: 'render', rows });
  if (rendered) panelProjection.set(rendered);
}
function panelSend(cmd, args = {}, render = true) {
  const reply = request('panel', { cmd, ...args }); if (!reply) return false;
  if (clipboardPanel.get() !== (reply.mode || false)) clipboardPanel.set(reply.mode || false);
  let displaced = false;
  for (const effect of reply.effects || []) {
    if (effect.type === 'query') request('view', effect);
    else if (effect.type === 'close') { closeClipboardPanel(); displaced = true; }
    else if (effect.type === 'full') { openPanel('full'); displaced = true; }
    else if (effect.type === 'pick') {
      const item = mirror.get(effect.id); if (!item) continue;
      panelSend('close'); displaced = true;
      setTimeout(() => { if (clipboard.paste(item, target) === 'copied') { restoreFocus(target?.text); toast.set(t('shell.clipboard.copiedOnly')); } }, 0);
    } else if (effect.type === 'clear') clipboard.clear();
    else if (effect.type === 'pin' || effect.type === 'remove') { const item = mirror.get(effect.id); if (item) clipboard[effect.type](item); }
  }
  if (render && !displaced) projectPanel(reply);
  return !!reply.handled;
}
function openPanel(mode) {
  if (!clipboardPanel.get()) { const top = wm.focused(); target = { text: focusToken(), linux: !!(top && String(top).startsWith('linux#')) }; }
  panelSend('open', { mode });
}
export const closeClipboardPanel = () => { panelSend('close'); setTimeout(() => restoreFocus(target?.text), 0); };
export function toggleClipboardPanel(mode) { if (clipboardPanel.get() === mode) closeClipboardPanel(); else openPanel(mode); }
addListener('keydown', (event) => { if (event.key === 'Escape' && clipboardPanel.get()) closeClipboardPanel(); });
clipboardHistory.subscribe(() => { if (clipboardPanel.get()) panelSend('refresh'); });
function PanelHost() {
  const th = useTheme(), language = useStore(locale), snapshot = useStore(panelProjection), view = useStore(viewStore), prefs = useStore(settings);
  useEffect(() => { panelSend('context', { context: { theme: th, area: mainBox() }, save: conf().save }); }, [th, language, prefs.clipboard, screen.width, screen.height]);
  useEffect(() => { if (view) panelSend('view', { view }); }, [view]);
  return h(RustWidget, { node: snapshot.tree, send: (action, event = {}) => panelSend('event', { action, ...event }) });
}
export function ClipboardPanel() { return useStore(clipboardPanel) ? h(PanelHost) : null; }

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
  return h(RustWidget, { node: request('panel', { cmd: 'toast', context: { theme: th, area: mainBox() }, message: msg })?.tree, send: () => false });
}
