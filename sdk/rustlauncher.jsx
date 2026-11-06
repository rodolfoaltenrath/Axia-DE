// Host adapter for Rust-owned menu state and widget trees. UI callbacks stay
// here; catalog preparation, menu decisions, layouts and effects live in Rust.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, Text, addListener, mainDisplay } from '@lordos/runtime';
import { TextInput, ScrollView } from './components.jsx';
import { SearchField } from './controls.jsx';
import { useLauncherApps, launcherRequest, launcherApp, launchApp } from './apps.js';
import { settings, useSettings } from './settings.js';
import { locale, t, tOptional, direction } from './i18n.js';
import { useStore } from './store.js';
import { Menu as DefaultMenu } from './menu.jsx';
import { dock, dockIdOf, beginDockDrag, dockDragJustEnded } from './dock.js';
import { searchEverything } from './search.js';
import { session } from './index.js';
import { wm } from './windows.js';
import { useSystemInfo } from './system.js';

export const launcherPlacement = (position, alignment, display = mainDisplay()) =>
  launcherRequest({ op: 'placement', position, alignment, display, rtl: direction() === 'rtl' });

const caption = (value) => value && typeof value === 'object' ? t(value.key, value.params) : String(value ?? '');
const contains = (target, roots) => {
  for (let node = target; node; node = node.parent) if (roots.includes(node)) return true;
  return false;
};
function Widget({ node, host }) {
  if (!node) return null;
  const props = { key: node.key, style: node.style };
  if (node.root) props.ref = host.root;
  for (const [name, action] of Object.entries(node.events || {})) props[name] = (e) => {
    if (!node.root) e?.stopPropagation?.();
    return host.send(action, { x: e?.x, y: e?.y, button: e?.button,
      insidePopup: contains(e?.target, [...host.popups.current.values()].map((ref) => ref.current)) });
  };
  const children = () => (node.children || []).map((child, i) => h(Widget, { key: child.key ?? i, node: child, host }));
  switch (node.kind) {
    case 'view': return h(View, props, children());
    case 'text': return h(Text, props, caption(node.label));
    case 'icon': { const Icon = launcherApp(node.app)?.Icon; return Icon ? h(Icon, { size: node.size }) : null; }
    case 'resultIcon': { const Icon = host.results.current.get(node.result)?.Icon; return Icon ? h(Icon, { size: node.size }) : null; }
    case 'input': return h(node.search ? SearchField : TextInput, { ...props, value: node.value, autoFocus: node.autoFocus,
      placeholder: caption(node.placeholder), onChange: (value) => host.send(['query', value]),
      onKeyDown: (e) => host.send(['key', e.key]), onSubmit: () => host.send(['submit']),
      onCancel: () => host.send(['cancel', node.value]) });
    case 'scroll': return h(ScrollView, props, ({ offset, height }) => {
      if (!host.live.current) return null;
      const reply = launcherRequest({ op: 'scroll', id: host.id.current, key: node.region, offset, ...(height ? { height } : {}) });
      return h(Widget, { node: reply.tree, host });
    });
    case 'menu': {
      if (!host.popups.current.has(node.key)) host.popups.current.set(node.key, { current: null });
      return h(host.Menu, { ...props, nodeRef: host.popups.current.get(node.key), t: host.theme.current, x: node.x, y: node.y, above: node.above, width: node.width,
      onClose: () => host.send(node.close), items: node.items.map((item) => item === '-' ? item : ({ ...item,
        label: caption(item.label), onClick: () => host.send(item.action) })) });
    }
    default: return null;
  }
}

export function RustLauncher({ mode = 'flow', t: theme, area, box, onOpen = launchApp, onClose, isOnBar = () => false, Menu = DefaultMenu }) {
  const apps = useLauncherApps();
  const [prefs] = useSettings();
  const language = useStore(locale);
  const version = useStore(searchEverything.version());
  const info = useSystemInfo(60000);
  const root = useRef(null), live = useRef(true), id = useRef(0), results = useRef(new Map()), popups = useRef(new Map()), latest = useRef(null), themeRef = useRef(theme);
  const searchLease = useRef(null);
  themeRef.current = theme;
  const { wallpapers, accents, ...tokens } = theme;
  const context = { theme: tokens, area: box || area,
    settings: { startPinned: prefs.startPinned, dockRecentApps: prefs.dockRecentApps }, info: { fullName: info.fullName, user: info.user }, rtl: direction() === 'rtl',
    session: { managed: session.managed(), canPower: session.canPower() } };
  latest.current = { context, onOpen, onClose, isOnBar, apps };
  const [snapshot, setSnapshot] = useState(() => {
    const reply = launcherRequest({ op: 'open', mode, context }); id.current = reply.id; return reply;
  });
  const initial = useRef(snapshot);
  const send = (action, data = {}) => {
    if (!live.current) return false;
    const app = launcherApp(action?.[1]);
    const reply = launcherRequest({ op: 'event', id: id.current, action, ...data,
      dragJustEnded: dockDragJustEnded(), dock: app ? { id: dockIdOf(app), pinned: dock.isPinned(app), removable: dock.canRemove(app) } : null });
    accept(reply); return !!reply.handled;
  };
  const perform = (effect) => {
    const app = launcherApp(effect.id);
    switch (effect.type) {
      case 'close': latest.current.onClose(); break;
      case 'open': latest.current.onOpen(effect.id); break;
      case 'drag': if (app) beginDockDrag({ id: dockIdOf(app), Icon: app.Icon, x: effect.x, y: effect.y,
        source: 'launcher', onStart: () => send(['dragStarted']) }); break;
      case 'dock': if (app) (effect.pin ? dock.pin(app) : dock.unpin(app)); break;
      case 'settings': settings.set(effect.patch); break;
      case 'userSettings': wm.open('settings', { args: { section: 'users' } }); break;
      case 'session': ({ quit: session.quit, lock: session.lock, switchUser: session.switchUser,
        restart: session.restart, shutdown: session.shutdown })[effect.action]?.(); break;
      case 'prepareSearch': searchLease.current = searchEverything.prepare(); break;
      case 'runSearch': searchEverything.run(results.current.get(effect.key)); break;
      case 'search': {
        const sections = searchEverything.results(effect.query, latest.current.apps, searchLease.current);
        results.current = new Map(sections.flatMap((section) => section.items.map((it) => [it.key, it])));
        send(['results'], { query: effect.query, sections: sections.map((section) => ({ id: section.id,
          title: tOptional(`shell.spotlight.section.${section.id}`) || section.id,
          items: section.items.map((it) => ({ key: it.key, title: it.title, subtitle: it.subtitle, glyph: it.glyph,
            hasIcon: !!it.Icon, hasImage: !!it.image, app: it.preview?.kind === 'app' ? it.preview.app.id : null })) })) });
        break;
      }
    }
  };
  const accept = (reply) => { if (!live.current) return; setSnapshot(reply); for (const effect of reply.effects || []) perform(effect); };
  useEffect(() => { for (const effect of initial.current.effects || []) perform(effect); }, []);
  useEffect(() => {
    accept(launcherRequest({ op: 'context', id: id.current, context: latest.current.context }));
  }, [apps, prefs, language, theme, info, area?.x, area?.y, area?.w, area?.h, box?.left, box?.top, box?.width, box?.height]);
  useEffect(() => { send(['searchRefresh']); }, [version, language, apps]);
  useEffect(() => addListener('pointerdown', (e) => {
    const roots = [root.current, ...[...popups.current.values()].map((ref) => ref.current)];
    send(['outside'], { inside: contains(e.target, roots), onBar: latest.current.isOnBar(e.x, e.y) });
  }), []);
  useEffect(() => () => { live.current = false; searchLease.current?.close(); searchLease.current = null; launcherRequest({ op: 'close', id: id.current }); }, []);
  return h(Widget, { node: snapshot.tree, host: { send, root, live, id, results, popups, Menu, theme: themeRef } });
}
