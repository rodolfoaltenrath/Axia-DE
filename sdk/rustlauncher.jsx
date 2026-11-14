// Host adapter for the Rust-owned launcher (the grid of apps of Lord Flow): menu state, widget
// trees, layouts and decisions live in Rust (rust/axia-shell/src/launcher); UI callbacks stay here.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { View, Text } from '@lordos/runtime';
import { TextInput, ScrollView } from './components.jsx';
import { useLauncherApps, launcherRequest, launcherApp, launchApp, refreshApps } from './apps.js';
import { useSettings } from './settings.js';
import { locale, t } from './i18n.js';
import { useStore } from './store.js';
import { Menu as DefaultMenu } from './menu.jsx';
import { dock, dockIdOf, beginDockDrag, dockDragJustEnded } from './dock.js';
import { NativeView } from './nativeview.js';
import { NATIVE_LAUNCHER, launcherBridge } from './nativelauncher.js';

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
    case 'input': return h(TextInput, { ...props, value: node.value, autoFocus: node.autoFocus,
      placeholder: caption(node.placeholder), onChange: (value) => host.send(['query', value]),
      onSubmit: () => host.send(['submit']), onCancel: () => host.send(['cancel', node.value]) });
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

function ReactLauncher({ t: theme, area, onOpen = launchApp, onClose, Menu = DefaultMenu }) {
  useState(refreshApps);                // apps installed since the last look show up at once
  const apps = useLauncherApps();
  const [prefs] = useSettings();
  const language = useStore(locale);
  const root = useRef(null), live = useRef(true), id = useRef(0), popups = useRef(new Map()), latest = useRef(null), themeRef = useRef(theme);
  themeRef.current = theme;
  const { wallpapers, accents, ...tokens } = theme;
  const context = { theme: tokens, area };
  latest.current = { context, onOpen, onClose };
  const [snapshot, setSnapshot] = useState(() => {
    const reply = launcherRequest({ op: 'open', mode: 'flow', context }); id.current = reply.id; return reply;
  });
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
    }
  };
  const accept = (reply) => { if (!live.current) return; setSnapshot(reply); for (const effect of reply.effects || []) perform(effect); };
  useEffect(() => {
    accept(launcherRequest({ op: 'context', id: id.current, context: latest.current.context }));
  }, [apps, prefs, language, theme, area?.x, area?.y, area?.w, area?.h]);
  useEffect(() => () => { live.current = false; launcherRequest({ op: 'close', id: id.current }); }, []);
  return h(Widget, { node: snapshot.tree, host: { send, root, live, id, popups, Menu, theme: themeRef } });
}

// "launcherNativeView": true in settings.json draws the launcher in Rust, with no React.
function NativeLauncher({ area, onOpen = launchApp, onClose }) {
  useState(refreshApps);                // apps installed since the last look show up at once
  const apps = useLauncherApps();
  useSettings();                        // the pins in the Dock live in the settings
  const latest = useRef(null), mount = useRef(null);
  latest.current = { onOpen, onClose };
  // what the Dock says of each app, for its menu: its id there, if it is pinned and if it can leave
  const dockFacts = {};
  for (const app of apps) dockFacts[app.id] = { id: dockIdOf(app), pinned: dock.isPinned(app), removable: dock.canRemove(app) };
  launcherBridge.perform = (effect, ctx) => {
    const { node } = ctx;
    const app = launcherApp(effect.id);
    switch (effect.type) {
      case 'close': latest.current.onClose(); break;
      case 'open': latest.current.onOpen(effect.id); break;
      case 'drag': if (app) {
        const [ox, oy] = lord.getLayout(node());
        beginDockDrag({ id: dockIdOf(app), Icon: app.Icon, x: ox + effect.x, y: oy + effect.y, source: 'launcher', onStart: () => ctx.act(['dragStarted']) });
      } break;
      case 'dock': if (app) (effect.pin ? dock.pin(app) : dock.unpin(app)); break;
    }
  };
  return h(NativeView, { manifest: NATIVE_LAUNCHER, mountRef: mount, args: { mode: 'flow', dock: dockFacts },
    style: { position: 'absolute', left: area?.x ?? 0, top: area?.y ?? 0, width: area?.w ?? 0, height: area?.h ?? 0 } });
}

export function RustLauncher(props) {
  const [prefs] = useSettings();
  return h(prefs.launcherNativeView === true ? NativeLauncher : ReactLauncher, props);
}
