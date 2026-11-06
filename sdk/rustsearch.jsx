// Widget/host adapter. Search policies, selection, geometry and previews belong to Rust.
import { createElement as h, useEffect, useState } from 'react';
import { View, Text, Image, mainBox, screen } from '@lordos/runtime';
import { TextInput } from './components.jsx';
import { SearchGlyph } from './searchglyph.jsx';
import { useTheme } from './theme.js';
import { dockGlass } from './glass.js';
import { useStore } from './store.js';
import { useLauncherApps, launcherApp } from './apps.js';
import { locale } from './i18n.js';
import { createSearchSession, searchCaption, searchSourceIcon, spotlightOpen } from './search.js';
function Widget({ node, session }) {
  if (!node) return null;
  const props = { style: node.style };
  for (const [name, action] of Object.entries(node.events || {})) props[name] = e => { if (action[0] !== 'close') e?.stopPropagation?.(); return session.event(action); };
  switch (node.kind) {
    case 'view': return h(View, props, (node.children || []).map((child, i) => h(Widget, { key: child.key ?? i, node: child, session })));
    case 'text': return h(Text, props, searchCaption(node.label));
    case 'image': return h(Image, { ...props, src: node.src });
    case 'icon': { const Icon = launcherApp(node.app)?.Icon; return Icon ? h(Icon, { size: node.size }) : null; }
    case 'providerIcon': { const Icon = searchSourceIcon(node.source, node.callback); return Icon ? h(Icon, { size: node.size }) : null; }
    case 'searchGlyph': return h(SearchGlyph, { size: node.size, color: node.color });
    case 'input': return h(TextInput, { ...props, value: node.value, size: node.size, autoFocus: node.autoFocus, placeholder: searchCaption(node.placeholder),
      onChange: value => session.event(['query', value]), onKeyDown: e => session.event(['key', e.key], { reveal: !!e.meta }), onCancel: () => session.event(['close']) });
    default: return null;
  }
}
function Panel() {
  const theme = useTheme(), apps = useLauncherApps(), language = useStore(locale);
  const context = { theme, glass: dockGlass(theme), area: mainBox() };
  const [session] = useState(() => createSearchSession(true, context));
  const snapshot = useStore(session.store);
  useEffect(() => { session.context(context); }, [theme, language, apps, screen.width, screen.height]);
  useEffect(() => () => session.close(), []);
  return h(Widget, { node: snapshot.tree, session });
}
export function SpotlightPanel() { return useStore(spotlightOpen) ? h(Panel) : null; }
