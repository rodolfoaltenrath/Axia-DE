// Generic host adapter for Rust-owned widget trees and effects. There is no
// filesystem/navigation state here: browser and every picker use lord.files.
import { createElement as h, createContext, useContext, Fragment, memo, useEffect, useMemo, useRef, useState } from 'react';
import { View, Text, Image, MediaView, mainBox, addListener, screen } from '@lordos/runtime';
import { Button, TextInput, ScrollView, useAppWindow, useKeyboard, hasTextFocus } from './components.jsx';
import { useTheme, ThemeScope } from './theme.js';
import { Toolbar, ToolbarButton, ToolbarGroup } from './controls.jsx';
import { t, locale, formatDate, formatTime, useLocale } from './i18n.js';
import { settings } from './settings.js';
import { errorText } from './errors.js';
import { wm, getApp } from './windows.js';
import { Layer } from './layers.js';
import { useLauncherApps, launchApp } from './apps.js';
import { defaultOpenerId, nativeOf } from './defaults.js';
import { clipboard } from './clipboard.js';
import { archive } from './archive.js';
import { DocImage, pdfThumb, fontSample, releaseImage } from './doc.js';
import { media, thumbRev } from './media.js';
import { startFileDrag, useFileDrop } from './filedrag.js';
import { lan } from './lan.js';
import { useDisks, disks } from './disks.js';
import { Menu } from './menu.jsx';
const modalHandles = new Set();
const FilesRootContext = createContext(null);

// Catalogs belong to the shell, not app styling. Keep them out of the request,
// Rust's appearance replies and the frame metadata saved with a window.
function appearanceContext(theme) {
  const { wallpapers, accents, ...tokens } = theme;
  return { ...tokens, nativeAppearance: settings.get().nativeAppearance || 'adwaita' };
}

export function filesRequest(value) {
  const result = lord.files.request(JSON.stringify(value));
  if (result?.error) throw new Error(errorText(result.error));
  return result;
}
const caption = (value) => {
  if (value == null) return '';
  if (typeof value !== 'object') return String(value);
  if (value.error) return errorText(value.error);
  if (value.format === 'date') {
    if (!value.value) return '';
    const date = new Date(value.value);
    return `${formatDate(date)} ${formatTime(date)}`;
  }
  return (value.prefix || '') + t(value.key, value.params);
};
const EVENT_NAMES = ['onClick', 'onDoubleClick', 'onContextMenu', 'onMouseDown', 'onMouseUp', 'onMouseMove', 'onWheel'];

// Shared toolbar buttons use a parameterless callback. Anchor Rust menus to
// the rendered control without changing that API for the other desktop apps.
function ToolWidget({ node, send }) {
  const ref = useRef(null);
  const press = () => {
    const [x, y, width, height] = lord.getLayout(ref.current.id);
    send(node.action, { type: 'click', x: x + width / 2, y: y + height });
  };
  return h(View, { ref, style: { flexShrink: 0 } }, h(ToolbarButton, { small: true, icon: node.icon,
    title: caption(node.title), active: node.active, disabled: node.disabled, style: node.style, onClick: press }));
}

// A file cell repeats the same few vector icons, so the tree only names them.
const iconTrees = new Map();
function iconTree(name, size) {
  const id = `${name}:${size}`;
  let tree = iconTrees.get(id);
  if (!tree) {
    if (iconTrees.size >= 64) iconTrees.clear();
    tree = filesRequest({ op: 'icon', name, size });
    iconTrees.set(id, tree);
  }
  return tree;
}

// Rust sends only what changed: {$: 1} stands for the previous node at that
// place (same key, or same position), so memo() below skips it. Matches delta()
// in apps/files/ffi.rs. Every reply must pass through here, in order.
function expand(prev, node) {
  if (node.$) return prev;
  const children = node.children;
  if (children?.length) {
    const before = prev?.children || [];
    let keyed = null;
    for (const child of before) if (child.key) (keyed ||= new Map()).set(child.key, child);
    for (let i = 0; i < children.length; i++) {
      const child = children[i];
      children[i] = expand(child.key && keyed ? keyed.get(child.key) : before[i], child);
    }
  }
  return node;
}
// Captions are resolved here, so anything that formats them differently must
// render every node again even though Rust's description did not change.
const FORMAT_SAMPLE = new Date(2001, 11, 31, 23, 59);
const captionFormat = () => { const { lang, region } = locale.get(); return `${lang}|${region}|${formatDate(FORMAT_SAMPLE)}|${formatTime(FORMAT_SAMPLE)}`; };

function FilesContextMenu({ node, send }) {
  const root = useContext(FilesRootContext);
  const origin = useRef(null), [position, setPosition] = useState(null);
  useEffect(() => {
    // The app root has already been laid out when a menu opens. A freshly
    // mounted anchor still reports (0,0) during this effect, shifting the menu.
    const [x, y] = node.absolute ? [0, 0] : lord.getLayout((root?.current || origin.current).id);
    setPosition({ x: x + node.x, y: y + node.y });
  }, [node.x, node.y, node.absolute, root]);
  const convert = (item) => item === '-' ? item : ({ ...item, label: caption(item.label),
    submenu: item.submenu?.map(convert), onClick: item.action ? () => send(item.action) : undefined });
  return h(View, { ref: origin, style: { position: 'absolute', left: 0, top: 0, width: 1, height: 1 } },
    position ? h(Menu, { ...position, layer: node.layer || 'popover', items: node.items.map(convert), onClose: () => send(['closeContext']) }) : null);
}

// Full names live outside clipped/virtualized rows; inactive labels own no timers.
function FilenameBubble({ node, tip, full }) {
  const theme = useTheme(), win = useAppWindow();
  return h(Layer, { name: node.tooltipLayer || 'tooltip' },
    h(View, { protected: !!win?.protected, style: { position: 'absolute', left: tip.x, top: tip.y, width: tip.width,
      paddingHorizontal: 10, paddingVertical: 8, borderRadius: 7, pointerEvents: 'none',
      backgroundColor: theme.popupBg || (theme.mode === 'dark' ? '#34343a' : '#fafafa'),
      border: `1px solid ${theme.separator}`, boxShadow: '0 4px 14px rgba(0,0,0,0.25)' } },
      h(Text, { style: { fontSize: 12, lineHeight: 1.4, color: theme.text } }, full)));
}
function Filename({ node, props, children }) {
  const ref = useRef(null), timer = useRef(0);
  useFileDrop(ref, props.onFileDrop, node.dropPath === 'trash');
  const { onFileDrop, ...widgetProps } = props;
  const [tip, setTip] = useState(null);
  const full = caption(node.tooltip ?? node.text);
  const hide = () => { clearTimeout(timer.current); timer.current = 0; setTip(null); };
  useEffect(() => { hide(); return () => clearTimeout(timer.current); }, [full, node.style?.width, node.style?.fontSize]);
  useEffect(() => {
    if (!tip) return;
    const off = addListener('pointerdown', hide), keys = addListener('keydown', hide);
    return () => { off(); keys(); };
  }, [!!tip]);
  const enter = () => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      if (!ref.current || !full) return;
      const [x, y, , h] = lord.getLayout(ref.current.id);
      const measured = lord.measureText(full, 12, 400);
      const width = Math.min(440, screen.width - 16, Math.max(100, measured + 20));
      const lines = Math.max(1, Math.ceil(measured / (width - 20)));
      const height = Math.min(screen.height - 16, lines * 17 + 16);
      setTip({ x: Math.max(8, Math.min(x, screen.width - width - 8)),
        y: y + h + height + 8 < screen.height ? y + h + 6 : Math.max(8, y - height - 6), width });
    }, 550);
  };
  return h(Fragment, null,
    h(node.type === 'fileTile' ? View : Text, { ...widgetProps, ref, onMouseEnter: enter, onMouseLeave: hide,
      onMouseDown: (e) => { hide(); props.onMouseDown?.(e); }, onWheel: (e) => { hide(); props.onWheel?.(e); } },
      node.type === 'fileTile' ? children : caption(node.text)),
    tip ? h(FilenameBubble, { node, tip, full }) : null);

}

export const RustWidget = memo(function WidgetNode({ node, send }) {
  if (!node) return null;
  if (node.type === 'icon') return h(Widget, { node: iconTree(node.name, node.size), send });
  const props = { style: node.style };
  if (node.dropPath) props.onFileDrop = (drop) => send(['dropFiles', node.dropPath], null, { paths: drop.paths, internalMove: drop.internalMove });
  for (const event of EVENT_NAMES) if (node.events?.[event]) props[event] = (e) => send(node.events[event], e);
  const children = (node.children || []).map((child, i) => h(Widget, { key: child.key || i, node: child, send }));
  switch (node.type) {
    case 'contextMenu': return h(FilesContextMenu, { node, send });
    case 'toolbar': return h(Toolbar, props, ...children);
    case 'toolGroup': return h(ToolbarGroup, props, ...children);
    case 'tool': return h(ToolWidget, { node, send });
    case 'fileTile':
    case 'filename': return h(Filename, { node, props, children });
    case 'text': return h(Text, props, caption(node.text));
    case 'button': return h(Button, { ...props, primary: node.primary, disabled: node.disabled, small: true, label: caption(node.label), onClick: node.disabled ? undefined : (e) => send(node.action, e) });
    case 'input': return h(TextInput, { ...props, value: node.value, placeholder: caption(node.placeholder), autoFocus: node.autoFocus, secure: node.secure,
      selectAll: node.selectAll, select: node.select, onChange: (value) => send(node.action, null, { value }), onSubmit: node.submit ? () => send(node.submit) : undefined, onCancel: node.cancel ? () => send(node.cancel) : undefined });
    case 'image': return h(Image, { ...props, src: node.src });
    case 'media': return h(MediaView, { ...props, media: media.view(node.media, { fit: 'contain' }) });
    case 'documentImage': return h(DocImage, { ...props, image: node.image?.id || 0 });
    case 'scroll': return h(ScrollView, { ...props, horizontal: node.horizontal }, ...children);
    default: return h(View, props, ...children);
  }
});

const Widget = RustWidget;

function eventData(e, root) {
  if (!e) return {};
  if (e.type !== 'mousedown') e.stopPropagation?.();
  const box = root?.current ? lord.getLayout(root.current.id) : [0, 0];
  return { x: (e.x || 0) - box[0], y: (e.y || 0) - box[1], deltaY: e.deltaY || 0,
    button: e.button || 0, shift: !!(e.shift ?? e.shiftKey), ctrl: !!(e.ctrl ?? e.ctrlKey), meta: !!(e.meta ?? e.metaKey), alt: !!(e.alt ?? e.altKey), key: e.key || '' };
}

// These are integrations with existing desktop services, selected by Rust.
// All requests/results go back through the owning Rust window handle.
export async function hostEffect(effect) {
  switch (effect.kind) {
    case 'diskMount':
    case 'diskRelease': {
      const { job } = filesRequest({ op: 'volumeStart', action: effect.kind === 'diskMount' ? 'mount' : 'release', device: effect.device });
      for (;;) {
        await new Promise((resolve) => setTimeout(resolve, 80));
        const result = filesRequest({ op: 'volumePoll', job });
        if (!result.done) continue;
        if (result.failure) throw new Error(errorText(result.failure));
        return { result: result.result, disks: disks.list() };
      }
    }
    case 'videoThumbnail': {
      if (!media.available()) return { result: null };
      // The confined worker signals completion. Avoid repeatedly stat'ing
      // videos on slow USB/network mounts from the UI's polling timer.
      return new Promise((resolve) => {
        let timer = 0, off = null;
        const finish = (path) => { clearTimeout(timer); off?.(); resolve({ result: path }); };
        const check = () => { const path = media.thumb(effect.path, 320); if (path) finish(path); };
        off = thumbRev.subscribe(check);
        timer = setTimeout(() => finish(null), 15000);
        check();
      });
    }
    case 'pdfThumbnail': { const result = await pdfThumb(effect.path, { width: 128, height: 176 }); return { result: { id: result.image, w: result.w, h: result.h } }; }
    case 'networkScan': {
      if (!lan.available()) return { result: { state: 'no-connector', hosts: [] } };
      lan.start();
      const deadline = lord.now() + 30000;
      let state = lan.poll();
      while (state.running && lord.now() < deadline) { await new Promise((resolve) => setTimeout(resolve, 500)); state = lan.poll(); }
      return { result: state };
    }
    case 'archiveList': return { result: await archive.list(effect.path) };
    case 'archiveExtract': return { result: await archive.extract(effect.path, effect.destination) };
    case 'archiveCreate': return { result: await archive.create(effect.paths, effect.destination) };
    case 'archivePreview':
    case 'archiveOpen': {
      const destination = `${lord.fs.home()}/.cache/axia/files/extract/${Date.now()}`;
      lord.fs.mkdir('.cache'); lord.fs.mkdir('.cache/axia'); lord.fs.mkdir('.cache/axia/files'); lord.fs.mkdir('.cache/axia/files/extract');
      await archive.extract(effect.path, destination, { only: [effect.entry] });
      return { path: `${destination}/${effect.entry}` };
    }
    case 'documentPreview': { const result = effect.format === 'pdf' ? await pdfThumb(effect.path, { width: 600, height: 500 }) : await fontSample(effect.path, { width: 600, lines: ['Axia', 'Aa Bb Cc 0123456789'] }); return { result: { id: result.image, w: result.w, h: result.h } }; }
    case 'mediaPreview': return { result: media.play('files', effect.path, { audio: 1 }) };
    default: return null;
  }
}

const appMetadata = (apps) => apps.map(({ id, name, desktopId, kind, mimeTypes }) => ({ id, name, kind,
  desktopId: desktopId ? (desktopId.endsWith('.desktop') ? desktopId : `${desktopId}.desktop`) : `lordos-${id}.desktop`,
  mimeTypes: Array.isArray(mimeTypes) ? mimeTypes : String(mimeTypes || '').split(';').filter(Boolean) }));

export function RustFiles({ mode = 'browser', args = {}, title = '', start = '', filter = null, multiple = false, name = '', onPick, onClose }) {
  useLocale(); // Captions must update even when the native poll has no changes.
  const theme = useTheme();
  const win = useAppWindow();
  const apps = useLauncherApps();
  const [volumes] = useDisks();
  const root = useRef(null), live = useRef(true), pending = useRef(new Set()), resources = useRef({ media: new Set(), images: new Set() }), capture = useRef(false), latest = useRef(null);
  const [snapshot, setSnapshot] = useState(() => filesRequest({ op: 'create', mode, args, title, start, name, multiple,
    filter: filter?.source || '', ignoreCase: !!filter?.ignoreCase, preferences: settings.get().files,
    theme: appearanceContext(theme),
    apps: appMetadata(apps) }));
  const handle = useRef(snapshot.id).current;
  // React may batch several snapshots into one render. Preserve the effects of
  // every accepted response, including intermediate snapshots that never paint.
  const received = useRef(snapshot.serial), effectQueue = useRef(null);
  if (effectQueue.current === null) effectQueue.current = [...(snapshot.effects || [])];
  const shown = useRef({ tree: snapshot.tree, menus: snapshot.menus, appearance: snapshot.appearance, chrome: snapshot.chrome, format: captionFormat() }), pace = useRef({ timer: 0, loading: !!snapshot.loading, poll: null });
  // Finished listings are collected promptly; an idle window stays at 10 Hz.
  const armPoll = () => {
    const state = pace.current;
    clearTimeout(state.timer);
    state.timer = setTimeout(state.poll, state.loading ? 16 : 100);
  };
  const accept = (result) => {
    if (result && live.current) {
      if (result.serial > received.current) { received.current = result.serial; effectQueue.current.push(...(result.effects || [])); }
      capture.current = !!result.capture;
      const format = captionFormat();
      result.tree = expand(shown.current.tree, result.tree);
      // New objects render every node again, with the new captions.
      if (format !== shown.current.format) result.tree = JSON.parse(JSON.stringify(result.tree));
      result.menus ??= shown.current.menus;
      result.appearance ??= shown.current.appearance;
      if (!Object.prototype.hasOwnProperty.call(result, 'chrome')) result.chrome = shown.current.chrome;
      shown.current = { tree: result.tree, menus: result.menus, appearance: result.appearance, chrome: result.chrome, format };
      const loading = !!result.loading;
      if (loading && !pace.current.loading && pace.current.poll) { pace.current.loading = true; armPoll(); }
      pace.current.loading = loading;
      setSnapshot(result);
    }
  };
  // Stable identity: memo() compares it, and everything it uses is a ref.
  const send = useRef((action, e, extra = {}) => {
    if (!live.current || !action) return;
    accept(filesRequest({ op: 'event', id: handle, action, ...eventData(e, root), ...extra }));
  }).current;
  useFileDrop(root, mode === 'browser' ? (drop) => send(['dropFiles'], null, { paths: drop.paths, internalMove: drop.internalMove }) : null);
  latest.current = { theme, send, onPick, onClose, win };
  useEffect(() => { send(['disks'], null, { disks: volumes }); }, [volumes]);
  const metadata = useMemo(() => JSON.stringify(appMetadata(apps)), [apps]);
  useEffect(() => { send(['apps'], null, { apps: JSON.parse(metadata) }); }, [metadata]);
  useKeyboard((e) => { if (mode === 'browser' && !modalHandles.size) send(['key'], e); });

  useEffect(() => {
    live.current = true;
    if (mode !== 'browser') modalHandles.add(handle);
    const poll = () => {
      if (!root.current) return;
      const [, , width, height] = lord.getLayout(root.current.id);
      accept(filesRequest({ op: 'poll', id: handle, width, height,
        theme: appearanceContext(latest.current.theme), locale: locale.get() }));
    };
    const move = addListener('pointermove', (e) => { if (capture.current) latest.current.send(['pointerMove'], e); });
    const up = addListener('pointerup', () => { if (capture.current) setTimeout(() => { if (live.current) latest.current.send(['dragEnd']); }, 0); });
    pace.current.poll = () => { try { poll(); } finally { if (live.current) armPoll(); } };
    pace.current.poll();
    const keys = mode !== 'browser' ? addListener('keydown', (e) => { if (!hasTextFocus() && [...modalHandles].at(-1) === handle) latest.current.send(['key'], e); }) : null;
    return () => { live.current = false; clearTimeout(pace.current.timer); pace.current.poll = null; keys?.(); move(); up(); for (const id of resources.current.media) media.close(id); for (const id of resources.current.images) releaseImage(id); modalHandles.delete(handle); filesRequest({ op: 'close', id: handle }); };
  }, [handle]);

  const format = captionFormat();
  useEffect(() => {
    if (shown.current.format === format) return;
    const tree = JSON.parse(JSON.stringify(shown.current.tree));
    shown.current = { ...shown.current, tree, format };
    setSnapshot((previous) => ({ ...previous, tree }));
  }, [format]);
  const menuLocale = locale.get();
  const windowTitle = caption(snapshot.title);
  const chromeKey = JSON.stringify(snapshot.chrome);
  useEffect(() => {
    if (mode === 'browser' && win?.id) wm.setArgs(win.id, { nativeChrome: snapshot.chrome || null });
  }, [chromeKey, win?.id, mode]);
  useEffect(() => {
    if (mode !== 'browser' || !win?.id) return;
    wm.setMenus(win.id, (snapshot.menus || []).map((menu) => ({ label: caption(menu.label), items: menu.items.map((item) =>
      typeof item === 'string' ? item : { ...item, label: caption(item.label), onClick: () => latest.current.send(item.action) }) })));
  }, [snapshot.menus, menuLocale, win?.id, mode]);
  useEffect(() => {
    if (mode === 'browser' && win?.id) wm.setTitle(win.id, windowTitle);
  }, [windowTitle, win?.id, mode]);

  useEffect(() => {
    for (const effect of effectQueue.current.splice(0)) {
      if (effect.kind === 'choose') { latest.current.onPick?.(multiple ? effect.paths : effect.paths[0]); continue; }
      if (effect.kind === 'cancel' || effect.kind === 'close') { if (mode === 'browser') wm.close(latest.current.win.id); else latest.current.onClose?.(); continue; }
      if (effect.kind === 'dragFiles') {
        if (startFileDrag(effect.paths, root.current?.id, !effect.copy)) latest.current.send(['dragEnd']);
        continue;
      }
      if (effect.kind === 'transferWindow') {
        const parent = latest.current.win;
        const frame = parent?.id ? wm.get(parent.id) : null;
        const child = wm.open('files', { instance: `transfers-${effect.owner}`,
          frame: { x: (frame?.x ?? 40) + 80, y: (frame?.y ?? 40) + 60, w: 540, h: 460 },
          args: { transferOwner: effect.owner, parent: parent?.id || null, dialog: true } });
        if (!child) filesRequest({ op: 'transferClose', id: effect.owner });
        continue;
      }
      if (effect.kind === 'newWindow') { wm.open('files', { new: true, args: { path: effect.path } }); continue; }
      if (effect.kind === 'launchApp') { launchApp(effect.id); continue; }
      if (effect.kind === 'manageDisk') { wm.open('disks', { args: { disk: effect.disk } }); continue; }
      if (effect.kind === 'terminal') { wm.open('terminal', { new: true, args: { cwd: effect.path } }); continue; }
      if (effect.kind === 'clipboardText') { clipboard.set(effect.text); continue; }
      if (effect.kind === 'closePreview') { if (effect.media) { media.close(effect.media); resources.current.media.delete(effect.media); } if (effect.image?.id) { releaseImage(effect.image.id); resources.current.images.delete(effect.image.id); } continue; }
      if (effect.kind === 'openPath') {
        const mime = lord.mime?.guess(effect.path) || 'application/octet-stream';
        const desktop = effect.app || defaultOpenerId(mime) || effect.fallback;
        const native = nativeOf(desktop);
        if (native && getApp(native)) wm.open(native, { new: true, args: { path: effect.path } });
        else if (!lord.apps.launchWith((desktop || '').replace(/\.desktop$/, ''), effect.path)) latest.current.send(['service'], null, { kind: 'openPath', error: 'files.openFailed' });
        continue;
      }
      if (effect.kind === 'openUri') { lord.apps.launchUri(effect.app || '', effect.uri); continue; }
      const task = hostEffect(effect).then((result) => {
        if (live.current && result) { if (effect.kind === 'mediaPreview' && result.result) resources.current.media.add(result.result); if (['documentPreview', 'pdfThumbnail'].includes(effect.kind) && result.result?.id) resources.current.images.add(result.result.id); if (result.disks) latest.current.send(['disks'], null, { disks: result.disks }); accept(filesRequest({ op: 'event', id: handle, action: ['service'], kind: effect.kind, generation: effect.generation, ...result })); }
        else if (effect.kind === 'mediaPreview' && result?.result) media.close(result.result);
        else if (['documentPreview', 'pdfThumbnail'].includes(effect.kind) && result?.result?.id) releaseImage(result.result.id);
      }).catch((error) => { if (live.current) accept(filesRequest({ op: 'event', id: handle, action: ['service'], kind: effect.kind, generation: effect.generation, error: error.message || String(error) })); });
      pending.current.add(task); task.finally(() => pending.current.delete(task));
    }
  }, [snapshot, handle, mode, multiple]);

  return h(ThemeScope, { value: snapshot.appearance || theme },
    h(FilesRootContext.Provider, { value: root },
      h(View, { ref: root, style: { flex: 1, overflow: 'hidden' } }, h(Widget, { node: snapshot.tree, send }))));
}

export function RustFileDialog(props) {
  const box = mainBox();
  const win = useAppWindow();
  return h(Layer, { name: 'system' }, h(View, { protected: !!win?.protected, onMouseDown: (e) => e.stopPropagation?.(), style: { position: 'absolute', ...box, alignItems: 'center', justifyContent: 'center', backgroundColor: 'rgba(0,0,0,0.3)' } },
    h(View, { style: { width: Math.min(920, box.width - 40), height: Math.min(620, box.height - 80), borderRadius: 12, overflow: 'hidden', boxShadow: '0 16px 40px rgba(0,0,0,0.45)' } }, h(RustFiles, props))));
}

export function RustFileIcon({ size = 32 }) {
  return h(Widget, { node: iconTree('files', size), send: () => {} });
}

// The secondary window shares the originating Rust session. No second browser,
// filesystem model, or copy worker is created. Its close button only hides it.
export function RustTransfers({ owner }) {
  const theme = useTheme(), win = useAppWindow();
  const language = useLocale();
  const appearance = useRef(theme);
  appearance.current = theme;
  const live = useRef(true), root = useRef(null);
  const [snapshot, setSnapshot] = useState(() => filesRequest({ op: 'transferAttach', id: owner, theme: appearanceContext(theme) }));
  const send = useRef((action) => {
    if (!live.current) return;
    const next = filesRequest({ op: 'transferEvent', id: owner, action });
    if (next) setSnapshot(next);
  }).current;
  useEffect(() => {
    live.current = true;
    const timer = setInterval(() => {
      if (!live.current) return;
      try {
        const next = filesRequest({ op: 'transferPoll', id: owner, theme: appearanceContext(appearance.current) });
        if (next) setSnapshot(next);
      } catch (e) { console.error('Files transfers:', e); wm.close(win.id); }
    }, 200);
    return () => {
      live.current = false;
      clearInterval(timer);
      filesRequest({ op: 'transferClose', id: owner });
    };
  }, [owner]);
  const tree = useMemo(() => snapshot?.tree ? JSON.parse(JSON.stringify(snapshot.tree)) : null, [snapshot?.tree, language.lang, language.region]);
  const title = caption(snapshot?.title);
  const chromeKey = JSON.stringify(snapshot?.chrome);
  useEffect(() => {
    if (win?.id) { wm.setTitle(win.id, title); wm.setArgs(win.id, { nativeChrome: snapshot?.chrome || null }); }
  }, [title, chromeKey, win?.id]);
  return h(ThemeScope, { value: snapshot?.appearance || theme },
    h(FilesRootContext.Provider, { value: root },
      h(View, { ref: root, style: { flex: 1, minHeight: 0 } }, h(Widget, { node: tree, send }))));
}
