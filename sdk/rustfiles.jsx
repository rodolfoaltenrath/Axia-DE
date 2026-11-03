// Generic host adapter for Rust-owned widget trees and effects. There is no
// filesystem/navigation state here: browser and every picker use lord.files.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, Text, Image, MediaView, mainBox, addListener } from '@lordos/runtime';
import { Button, TextInput, ScrollView, useAppWindow, useKeyboard, hasTextFocus } from './components.jsx';
import { useTheme } from './theme.js';
import { Toolbar, ToolbarButton, ToolbarGroup } from './controls.jsx';
import { t, locale, formatDate, formatTime } from './i18n.js';
import { settings } from './settings.js';
import { errorText } from './errors.js';
import { wm, getApp } from './windows.js';
import { Layer } from './layers.js';
import { useLauncherApps, launchApp } from './apps.js';
import { defaultOpenerId, nativeOf } from './defaults.js';
import { clipboard } from './clipboard.js';
import { archive } from './archive.js';
import { DocImage, pdfThumb, fontSample, releaseImage } from './doc.js';
import { media } from './media.js';
import { lan } from './lan.js';
import { useDisks, disks } from './disks.js';
const modalHandles = new Set();

export function filesRequest(value) {
  const result = lord.files.request(JSON.stringify(value));
  if (result?.error) throw new Error(errorText(result.error));
  return result;
}
const caption = (value) => {
  if (value == null) return '';
  if (typeof value !== 'object') return String(value);
  if (value.error) return errorText(value.error);
  if (value.format === 'date') return value.value ? `${formatDate(value.value)} ${formatTime(value.value)}` : '';
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
    title: caption(node.title), active: node.active, disabled: node.disabled, onClick: press }));
}

function Widget({ node, send, path = 'root' }) {
  if (!node) return null;
  const props = { style: node.style };
  for (const event of EVENT_NAMES) if (node.events?.[event]) props[event] = (e) => send(node.events[event], e);
  const children = (node.children || []).map((child, i) => h(Widget, { key: child.key || i, node: child, send, path: `${path}/${i}` }));
  switch (node.type) {
    case 'toolbar': return h(Toolbar, props, ...children);
    case 'toolGroup': return h(ToolbarGroup, props, ...children);
    case 'tool': return h(ToolWidget, { node, send });
    case 'text': return h(Text, props, caption(node.text));
    case 'button': return h(Button, { ...props, primary: node.primary, disabled: node.disabled, small: true, label: caption(node.label), onClick: node.disabled ? undefined : (e) => send(node.action, e) });
    case 'input': return h(TextInput, { ...props, value: node.value, placeholder: caption(node.placeholder), autoFocus: node.autoFocus, secure: node.secure,
      selectAll: node.selectAll, onChange: (value) => send(node.action, null, { value }), onSubmit: node.submit ? () => send(node.submit) : undefined, onCancel: node.cancel ? () => send(node.cancel) : undefined });
    case 'image': return h(Image, { ...props, src: node.src });
    case 'media': return h(MediaView, { ...props, media: media.view(node.media, { fit: 'contain' }) });
    case 'documentImage': return h(DocImage, { ...props, image: node.image?.id || 0 });
    case 'scroll': return h(ScrollView, { ...props, horizontal: node.horizontal }, ...children);
    default: return h(View, props, ...children);
  }
}

function eventData(e, root) {
  if (!e) return {};
  if (e.type !== 'mousedown') e.stopPropagation?.();
  const box = root?.current ? lord.getLayout(root.current.id) : [0, 0];
  return { x: (e.x || 0) - box[0], y: (e.y || 0) - box[1], deltaY: e.deltaY || 0,
    button: e.button || 0, shift: !!(e.shift ?? e.shiftKey), ctrl: !!(e.ctrl ?? e.ctrlKey), meta: !!(e.meta ?? e.metaKey), alt: !!(e.alt ?? e.altKey), key: e.key || '' };
}

// These are integrations with existing desktop services, selected by Rust.
// All requests/results go back through the owning Rust window handle.
async function hostEffect(effect) {
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
  const theme = useTheme();
  const win = useAppWindow();
  const apps = useLauncherApps();
  const [volumes] = useDisks();
  const root = useRef(null), live = useRef(true), pending = useRef(new Set()), resources = useRef({ media: new Set(), images: new Set() }), capture = useRef(false), latest = useRef(null);
  const [snapshot, setSnapshot] = useState(() => filesRequest({ op: 'create', mode, args, title, start, name, multiple,
    filter: filter?.source || '', ignoreCase: !!filter?.ignoreCase, preferences: settings.get().files,
    apps: appMetadata(apps) }));
  const handle = useRef(snapshot.id).current;
  // React may batch several snapshots into one render. Preserve the effects of
  // every accepted response, including intermediate snapshots that never paint.
  const received = useRef(snapshot.serial), effectQueue = useRef(null);
  if (effectQueue.current === null) effectQueue.current = [...(snapshot.effects || [])];
  const accept = (result) => {
    if (result && live.current) {
      if (result.serial > received.current) { received.current = result.serial; effectQueue.current.push(...(result.effects || [])); }
      capture.current = !!result.capture; setSnapshot(result);
    }
  };
  const send = (action, e, extra = {}) => {
    if (!live.current || !action) return;
    accept(filesRequest({ op: 'event', id: handle, action, ...eventData(e, root), ...extra }));
  };
  latest.current = { theme, send, onPick, onClose, win };
  useEffect(() => { send(['disks'], null, { disks: volumes }); }, [volumes]);
  const metadata = JSON.stringify(appMetadata(apps));
  useEffect(() => { send(['apps'], null, { apps: JSON.parse(metadata) }); }, [metadata]);
  useKeyboard((e) => { if (mode === 'browser' && !modalHandles.size) send(['key'], e); });

  useEffect(() => {
    live.current = true;
    if (mode !== 'browser') modalHandles.add(handle);
    const poll = () => {
      if (!root.current) return;
      const [, , width, height] = lord.getLayout(root.current.id);
      accept(filesRequest({ op: 'poll', id: handle, width, height, theme: latest.current.theme, locale: locale.get() }));
    };
    const move = addListener('pointermove', (e) => { if (capture.current) latest.current.send(['pointerMove'], e); });
    const up = addListener('pointerup', () => { if (capture.current) setTimeout(() => { if (live.current) latest.current.send(['dragEnd']); }, 0); });
    const timer = setInterval(poll, 100);
    poll();
    const keys = mode !== 'browser' ? addListener('keydown', (e) => { if (!hasTextFocus() && [...modalHandles].at(-1) === handle) latest.current.send(['key'], e); }) : null;
    return () => { live.current = false; clearInterval(timer); keys?.(); move(); up(); for (const id of resources.current.media) media.close(id); for (const id of resources.current.images) releaseImage(id); modalHandles.delete(handle); filesRequest({ op: 'close', id: handle }); };
  }, [handle]);

  useEffect(() => {
    if (mode !== 'browser' || !win?.id) return;
    wm.setMenus(win.id, (snapshot.menus || []).map((menu) => ({ label: caption(menu.label), items: menu.items.map((item) =>
      typeof item === 'string' ? item : { ...item, label: caption(item.label), onClick: () => latest.current.send(item.action) }) })));
    wm.setTitle(win.id, caption(snapshot.title));
  }, [snapshot, win?.id, mode]);

  useEffect(() => {
    for (const effect of effectQueue.current.splice(0)) {
      if (effect.kind === 'choose') { latest.current.onPick?.(multiple ? effect.paths : effect.paths[0]); continue; }
      if (effect.kind === 'cancel' || effect.kind === 'close') { if (mode === 'browser') wm.close(latest.current.win.id); else latest.current.onClose?.(); continue; }
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
      if (effect.kind === 'remoteDesktop') { wm.open('remote-desk', { new: true, args: effect.args }); continue; }
      const task = hostEffect(effect).then((result) => {
        if (live.current && result) { if (effect.kind === 'mediaPreview' && result.result) resources.current.media.add(result.result); if (['documentPreview', 'pdfThumbnail'].includes(effect.kind) && result.result?.id) resources.current.images.add(result.result.id); if (result.disks) latest.current.send(['disks'], null, { disks: result.disks }); accept(filesRequest({ op: 'event', id: handle, action: ['service'], kind: effect.kind, generation: effect.generation, ...result })); }
        else if (effect.kind === 'mediaPreview' && result?.result) media.close(result.result);
        else if (['documentPreview', 'pdfThumbnail'].includes(effect.kind) && result?.result?.id) releaseImage(result.result.id);
      }).catch((error) => { if (live.current) accept(filesRequest({ op: 'event', id: handle, action: ['service'], kind: effect.kind, generation: effect.generation, error: error.message || String(error) })); });
      pending.current.add(task); task.finally(() => pending.current.delete(task));
    }
  }, [snapshot, handle, mode, multiple]);

  return h(View, { ref: root, style: { flex: 1, overflow: 'hidden' } }, h(Widget, { node: snapshot.tree, send }));
}

export function RustFileDialog(props) {
  const box = mainBox();
  const win = useAppWindow();
  return h(Layer, { name: 'system' }, h(View, { protected: !!win?.protected, onMouseDown: (e) => e.stopPropagation?.(), style: { position: 'absolute', ...box, alignItems: 'center', justifyContent: 'center', backgroundColor: 'rgba(0,0,0,0.3)' } },
    h(View, { style: { width: Math.min(920, box.width - 40), height: Math.min(620, box.height - 80), borderRadius: 12, overflow: 'hidden', boxShadow: '0 16px 40px rgba(0,0,0,0.45)' } }, h(RustFiles, props))));
}

export function RustFileIcon({ size = 32 }) {
  const tree = filesRequest({ op: 'icon', name: 'files', size });
  return h(Widget, { node: tree, send: () => {} });
}
