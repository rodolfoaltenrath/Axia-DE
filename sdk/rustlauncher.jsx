// Rust owns the launcher tree, input, layout and policy. This adapter still
// binds app opening and the Dock effects while shell services migrate to Rust.
import { createElement as h, useLayoutEffect, useRef, useState } from 'react';
import { useLauncherApps, launcherApp, launchApp, refreshApps } from './apps.js';
import { useSettings } from './settings.js';
import { clearTextFocus } from './components.jsx';
import { dock, dockIdOf, beginDockDrag, dockDragJustEnded } from './dock.js';
import { NativeView } from './nativeview.js';
import { NATIVE_LAUNCHER, launcherBridge } from './nativelauncher.js';
export function RustLauncher({ area, onOpen = launchApp, onClose }) {
  useLayoutEffect(clearTextFocus, []);
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
      case 'open': if (!dockDragJustEnded()) latest.current.onOpen(effect.id); break;
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
