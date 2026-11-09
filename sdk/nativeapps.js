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
import { useSettings } from './settings.js';
import { registerPrivacySection } from './privacy.js';

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
  const [preferences] = useSettings();
  const [serviceFacts] = useState(() => ({ at: 0, value: {} }));
  // `same`: Rust had nothing new and kept its tree; the rendered element is reused as is.
  const [memo] = useState(() => ({ tree: null, element: null, facts: () => ({}) }));
  useEffect(() => {
    const ms = Math.max(250, Number(manifest.refreshMs) || 0);
    const timer = manifest.refreshMs ? setInterval(refresh, win.focused ? ms : Math.max(2000, ms * 2)) : null;
    return () => { if (timer) clearInterval(timer); };
  }, [manifest.refreshMs, win.focused]);
  useEffect(() => () => nativeAppRequest(owner, { cmd: 'close' }), [owner]);
  const now = Date.now();
  if (manifest.services && now - serviceFacts.at >= 1000) {
    const value = {};
    for (const [service, methods] of Object.entries(manifest.services)) {
      value[service] = {};
      for (const method of methods) {
        try { value[service][method] = lord[service]?.[method]?.() ?? null; } catch (_) { value[service][method] = null; }
      }
    }
    serviceFacts.at = now; serviceFacts.value = value;
  }
  const facts = () => manifest.facts === 'windows' ? { windows: appControlFacts() }
    : manifest.facts === 'apps' ? { catalog: catalog.map(appFacts),
      registry: appCatalogSource().map(app => appFacts({ ...app, name: appName(app) })) } : {};
  const { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius } = theme;
  const reply = nativeAppRequest(owner, { cmd: 'sync', ...facts(), args: { ...(win.args || {}), ...(manifest.defaultArgs || {}) },
    settings: manifest.settings ? preferences : undefined, services: serviceFacts.value,
    context: { theme: { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius },
      scale: typeScale, decimal: formatNumber(1.5, 1)[1], width: wm.get(win.id)?.w || manifest.size?.[0] || 1000 } });
  memo.facts = facts;
  useEffect(() => { if (reply.effect) { nativeAppEffects(owner, reply, manifest); refresh(); } }, [reply.effect]);
  const title = caption(reply.title);
  useEffect(() => { if (win.id && title) wm.setTitle(win.id, title); }, [win.id, title]);
  // Stable callbacks: an unchanged tree keeps the same element and React skips the subtree.
  const [handlers] = useState(() => {
    const dispatch = input => {
      const result = nativeAppRequest(owner, { ...input, ...memo.facts() });
      nativeAppEffects(owner, result, manifest); refresh();
    };
    return {
      dispatch,
      send: ([cmd, key], extra = {}) => dispatch({ cmd, key, ...extra }),
      icon: node => {
        const window = node.window && wm.get(node.window);
        const Icon = window ? windowIcon(window, getApp) : launcherApp(node.app)?.Icon || getApp(node.app)?.Icon;
        return Icon ? h(Icon, { size: node.size }) : null;
      },
      raster: node => owner.pixels(node.image),
    };
  });
  const tree = reply.same ? memo.tree : reply.tree;
  if (!memo.element || tree !== memo.tree) {
    memo.tree = tree;
    memo.element = h(RustWidget, { node: tree, send: handlers.send, icon: handlers.icon, raster: handlers.raster });
  }
  return h(Fragment, null, manifest.keyboard ? h(KeyboardBinding, { send: handlers.dispatch }) : null, memo.element);
}
export function nativeApp(manifest) {
  for (const section of manifest.privacy || []) registerPrivacySection({ id: section.id, order: section.order,
    Component: function NativePrivacySection() { return h(WidgetHost, { manifest: { ...manifest, defaultArgs: { section: section.section } } }); } });
  const Component = manifest.nativeView === 'files'
    ? function FilesHost() { const { args } = useAppWindow(); return h(RustFiles, { mode: manifest.mode, args }); }
    : function CompiledAppHost() { return h(WidgetHost, { manifest }); };
  const Icon = manifest.nativeView === 'files' ? RustFileIcon
    : function CompiledAppIcon({ size }) { return h(RustWidget, { node: nativeAppRequest(lord.nativeApps, { cmd: 'icon', app: manifest.id, size }), send: () => {} }); };
  return { ...manifest, system: true, native: true, Component, Icon };
}
