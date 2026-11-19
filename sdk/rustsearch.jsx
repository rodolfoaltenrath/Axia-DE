// Native Spotlight adapter. Rust draws the tree and handles input; search.js
// still carries session state, providers and shell effects during the migration.
import { createElement as h, useEffect, useState } from 'react';
import { mainBox, screen } from '@lordos/runtime';
import { useTheme } from './theme.js';
import { dockGlass } from './glass.js';
import { useStore } from './store.js';
import { useLauncherApps, refreshApps } from './apps.js';
import { locale } from './i18n.js';
import { createSearchSession, spotlightOpen } from './search.js';
import { NativeView } from './nativeview.js';
import { NATIVE_TREE_APPS, treeOwners } from './nativetree.js';
function NativePanel() {
  useState(refreshApps);
  const theme = useTheme(), apps = useLauncherApps(), language = useStore(locale);
  const context = { theme, glass: dockGlass(theme), area: mainBox() };
  const [session] = useState(() => createSearchSession(true, context));
  const snapshot = useStore(session.store);
  useEffect(() => { session.context(context); }, [theme, language, apps, screen.width, screen.height]);
  useEffect(() => () => { session.close(); treeOwners.delete('spotlight'); }, []);
  treeOwners.set('spotlight', ({ action, extra }) => { session.event(action, action[0] === 'key' ? { reveal: !!extra?.meta } : {}); });
  return h(NativeView, { manifest: NATIVE_TREE_APPS, args: { tree: snapshot.tree, owner: 'spotlight' },
    style: { position: 'absolute', left: 0, top: 0, width: screen.width, height: screen.height } });
}
export function SpotlightPanel() {
  return useStore(spotlightOpen) ? h(NativePanel) : null;
}
