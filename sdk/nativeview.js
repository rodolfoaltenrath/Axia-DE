// Shell side of native views: compiled apps that Rust mounts straight into the scene
// (rust/axia-shell/src/native_view, docs/RUST.md "Native views"). Rust draws the app, handles its
// pointer, wheel and text input, translates its captions and refreshes it on its own timer. This
// module only gives the view a mount node inside the window manager's frame, sends it the facts it
// reads (settings, theme, services, catalog, focus) and runs the effects that belong to the shell:
// opening apps, saving settings, firewall calls, the clipboard and the window title.
import { createElement as h, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { View } from '@lordos/runtime';
import { useAppWindow } from './components.jsx';
import { useTheme } from './theme.js';
import { useLocale, formatNumber } from './i18n.js';
import { useWindows, wm, appCatalogSource } from './windows.js';
import { useLauncherApps, launchApp } from './apps.js';
import { refreshPackages } from './packages.js';
import { useSettings, settings } from './settings.js';
import { appName } from './i18n.js';

// Serializable facts of an app; localized names and callable icons belong to the host.
export const appFacts = app => ({ id: app.id, name: app.name, kind: app.kind, version: app.version,
  comment: app.comment, description: app.description, genericName: app.genericName,
  package: !!app.package, system: !!app.system, isolated: !!app.isolated, icon: !!app.Icon,
  category: typeof app.category === 'string' ? app.category : undefined });

const runners = new Map();          // handle → runs one effect
globalThis.__lordNativeView = () => {
  for (const [handle, run] of runners) {
    for (let list = lord.nativeViews.take(handle), n = 0; list && n < 64; list = lord.nativeViews.take(handle), n++)
      for (const effect of list) run(effect);
  }
};

function runner(handle, manifest, winId) {
  return (effect) => {
    if (effect.host) {                 // the host's own: no result goes back to the app
      try {
        if (effect.kind === 'title' && winId) wm.setTitle(winId, effect.title);
        else if (effect.kind === 'paste') lord.nativeViews.paste(handle, String(lord.clipboard.get() ?? ''));
        else if (effect.kind === 'copy') lord.clipboard.set(String(effect.text ?? ''));
        else if (effect.kind === 'launchApp') launchApp(String(effect.app));
        else if (effect.kind === 'refreshPackages') refreshPackages();
      } catch (e) { console.error('native view:', e); }
      return;
    }
    let result = '';
    try {
      switch (effect.kind) {
        case 'settings': settings.set(effect.patch); break;
        case 'openApp': wm.open(effect.app, { args: effect.args || {} }); break;
        case 'nativeCall':
          if (!manifest.calls?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
          result = lord[effect.service][effect.method](...(effect.args || []));
          break;
        case 'nativeRead':
          if (!manifest.reads?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
          result = { method: effect.method, value: lord[effect.service][effect.method](...(effect.args || [])) };
          break;
        default: result = 'app.invalidRequest';
      }
    } catch (error) {
      const code = error?.code || error?.message || 'app.invalidRequest';
      result = effect.kind === 'nativeCall' || effect.kind === 'nativeRead' ? { error: code } : code;
    }
    lord.nativeViews.result(handle, result);
  };
}

function services(manifest) {
  const value = {};
  for (const [service, methods] of Object.entries(manifest.services || {})) {
    value[service] = {};
    for (const method of methods) {
      try { value[service][method] = lord[service]?.[method]?.() ?? null; } catch (_) { value[service][method] = null; }
    }
  }
  return value;
}

// args: the window's (or a Settings section's); icon: a dock/launcher icon of `icon` points.
export function NativeView({ manifest, args, icon, style }) {
  const theme = useTheme(), win = useAppWindow() || {}, catalog = useLauncherApps();
  const { lang } = useLocale();
  useWindows();
  const [preferences] = useSettings();
  const ref = useRef(null), mounted = useRef({ handle: 0, sent: '' });
  const [polled, setPolled] = useState(() => (manifest.services && !icon ? services(manifest) : {}));
  useEffect(() => {
    if (!manifest.services || icon) return undefined;
    const id = setInterval(() => setPolled(services(manifest)), 1000);
    return () => clearInterval(id);
  }, [manifest, icon]);
  const { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius } = theme;
  const facts = icon ? { icon } : {
    args: { ...(args || {}) }, lang, focused: win.focused !== false, refreshMs: manifest.refreshMs || 0,
    width: manifest.size?.[0] || 1000,
    settings: manifest.settings ? preferences : undefined,
    services: polled,
    ...(manifest.facts === 'apps' ? { catalog: catalog.map(appFacts),
      registry: appCatalogSource().map(app => appFacts({ ...app, name: appName(app) })) } : {}),
    context: { theme: { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius },
      decimal: formatNumber(1.5, 1)[1] },
  };
  const json = JSON.stringify(facts);
  useLayoutEffect(() => {
    const m = mounted.current;
    m.handle = ref.current ? lord.nativeViews.mount(ref.current.id, manifest.id, json) : 0;
    m.sent = json;
    if (!m.handle) return undefined;
    runners.set(m.handle, runner(m.handle, manifest, win.id));
    globalThis.__lordNativeView();
    return () => { runners.delete(m.handle); lord.nativeViews.unmount(m.handle); m.handle = 0; };
  }, [manifest.id, icon]);
  useEffect(() => {
    const m = mounted.current;
    if (!m.handle || m.sent === json) return;
    m.sent = json;
    lord.nativeViews.facts(m.handle, json);
    globalThis.__lordNativeView();
  });
  return h(View, { ref, style: icon ? { width: icon, height: icon } : [{ flex: 1 }, style] });
}
