// Compatibility host shared by compiled Rust apps. App layout, metadata and controllers live in Rust.
import { createElement as h, Fragment, useEffect, useReducer, useState } from 'react';
import { useAppWindow, useKeyboard } from './components.jsx';
import { useTheme } from './theme.js';
import { typeScale } from './controls.jsx';
import { useLocale, appName, formatNumber } from './i18n.js';
import { useWindows, wm, getApp, appCatalogSource } from './windows.js';
import { windowIcon, launcherApp, useLauncherApps } from './apps.js';
import { appControlFacts } from './appcontrol.js';
import { nativeAppRequest, nativeAppEffects } from './nativeappmodel.js';
import { RustWidget, caption } from './rustwidgets.js';
import { RustFiles, RustFileIcon } from './rustfiles.js';

// Copy serializable facts; localized names and callable icons belong to the host.
const appFacts = app => ({ id: app.id, name: app.name, kind: app.kind, version: app.version,
  comment: app.comment, description: app.description, genericName: app.genericName,
  package: !!app.package, system: !!app.system, isolated: !!app.isolated, icon: !!app.Icon });
function KeyboardBinding({ send }) {
  useKeyboard(event => send({ cmd: 'key', event: event.key, char: event.char || '' }));
  return null;
}
function WidgetHost({ manifest }) {
  const theme = useTheme(), win = useAppWindow(), catalog = useLauncherApps();
  useLocale(); useWindows();
  const [owner] = useState(() => lord.nativeApps.create(manifest.id));
  const [, refresh] = useReducer(n => n + 1, 0);
  const facts = () => manifest.facts === 'windows' ? { windows: appControlFacts() }
    : manifest.facts === 'apps' ? { catalog: catalog.map(appFacts),
      registry: appCatalogSource().map(app => appFacts({ ...app, name: appName(app) })) } : {};
  const { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius } = theme;
  const reply = nativeAppRequest(owner, { cmd: 'sync', ...facts(), args: win.args || {},
    context: { theme: { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius },
      scale: typeScale, decimal: formatNumber(1.5, 1)[1] } });
  const title = caption(reply.title);
  useEffect(() => { if (win.id && title) wm.setTitle(win.id, title); }, [win.id, title]);
  const dispatch = input => {
    const result = nativeAppRequest(owner, { ...input, ...facts() });
    nativeAppEffects(owner, result); refresh();
  };
  const send = ([cmd, key]) => dispatch({ cmd, key });
  const icon = node => {
    const window = node.window && wm.get(node.window);
    const Icon = window ? windowIcon(window, getApp) : launcherApp(node.app)?.Icon || getApp(node.app)?.Icon;
    return Icon ? h(Icon, { size: node.size }) : null;
  };
  return h(Fragment, null, manifest.keyboard ? h(KeyboardBinding, { send: dispatch }) : null,
    h(RustWidget, { node: reply.tree, send, icon }));
}
export function nativeApp(manifest) {
  const Component = manifest.nativeView === 'files'
    ? function FilesHost() { const { args } = useAppWindow(); return h(RustFiles, { mode: manifest.mode, args }); }
    : function CompiledAppHost() { return h(WidgetHost, { manifest }); };
  const Icon = manifest.nativeView === 'files' ? RustFileIcon
    : function CompiledAppIcon({ size }) { return h(RustWidget, { node: nativeAppRequest(lord.nativeApps, { cmd: 'icon', app: manifest.id, size }), send: () => {} }); };
  return { ...manifest, system: true, native: true, Component, Icon };
}
