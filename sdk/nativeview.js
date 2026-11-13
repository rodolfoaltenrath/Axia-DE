// Shell side of native views: compiled apps that Rust mounts straight into the scene
// (rust/axia-shell/src/native_view, docs/RUST.md "Native views"). Rust draws the app, handles its
// pointer, wheel and text input, translates its captions and refreshes it on its own timer. This
// module only gives the view a mount node inside the window manager's frame, sends it the facts it
// reads (settings, theme, services, catalog, focus) and runs the effects that belong to the shell:
// opening apps, saving settings, firewall calls, the clipboard and the window title.
import { version as reactVersion, createElement as h, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { View, screen, shellHasKeyboard, onShellKeyboard, layerContainer } from '@lordos/runtime';
import { LAYERS, shellLayers } from './layers.js';
import { filesEffect } from './nativefiles.js';
import { barEffect } from './nativemenubar.js';
import { textFieldActive, onTextField } from './textfocus.js';
import { useAppWindow } from './components.jsx';
import { typeScale } from './controls.jsx';
import { useTheme } from './theme.js';
import { useLocale, formatNumber } from './i18n.js';
import { useWindows, wm, appCatalogSource } from './windows.js';
import { useLauncherApps, launchApp, appIconFacts } from './apps.js';
import { refreshPackages } from './packages.js';
import { useSettings, settings } from './settings.js';
import { appName } from './i18n.js';
import { arm, clockState } from './alarms.js';
import { useStore } from './store.js';
import { appControlFacts, forceQuit } from './appcontrol.js';
import { executeNativeEffect } from './nativeappmodel.js';
import { disks } from './disks.js';
// The nodes of the shell layers a native view may put its menus and popovers in ("layer": "popover").
// Only when the shell has layers; otherwise the views stay in their window.
const LAYER_NAMES = ['popover', 'modal', 'tooltip'];
function layerFacts() {
  if (!shellLayers()) return undefined;
  const out = {};
  for (const name of LAYER_NAMES) { const id = layerContainer(LAYERS[name])?.id; if (id) out[name] = id; }
  return out;
}
const emptyClock = { get: () => null, subscribe: () => () => {} };

// Serializable facts of an app; localized names and callable icons belong to the host.
export const appFacts = app => ({ id: app.id, name: app.name, kind: app.kind, version: app.version,
  desktopId: app.desktopId, mimeTypes: app.mimeTypes,
  comment: app.comment, description: app.description, genericName: app.genericName,
  iconPath: typeof app.icon === 'string' ? app.icon : '',
  package: !!app.package, system: !!app.system, isolated: !!app.isolated, icon: !!app.Icon,
  iconData: appIconFacts(app),
  category: typeof app.category === 'string' ? app.category : undefined });

const runners = new Map();          // handle → runs one effect
// A shell component that runs the effects of its own view (the title bar): returns the way to stop.
export function addRunner(handle, run) { runners.set(handle, run); return () => runners.delete(handle); }
globalThis.__lordNativeView = () => {
  for (const [handle, run] of runners) {
    for (let list = lord.nativeViews.take(handle), n = 0; list && n < 64; list = lord.nativeViews.take(handle), n++)
      for (const effect of list) run(effect);
  }
};

function runner(handle, manifest, winId, node = () => 0) {
  const act = (action, extra = {}) => lord.nativeViews.action(handle, { action, extra });
  return (effect) => {
    // a Files window: its own effects (apps/files/native_app.rs), host ones too
    if (manifest.id === 'files' && !effect.host) { try { filesEffect(effect, { handle, winId, node, act }); } catch (e) { console.error('native files:', e); } return; }
    // the menu bar: building a menu's items and running one (sdk/nativemenubar.js)
    if (manifest.id === 'shell:menubar' && !effect.host) { barEffect(effect, handle); return; }
    if (effect.host) {                 // the host's own: no result goes back to the app
      try {
        if (effect.kind === 'title' && winId) wm.setTitle(winId, effect.title);
        else if (effect.kind === 'paste') lord.nativeViews.paste(handle, String(lord.clipboard.get() ?? ''));
        else if (effect.kind === 'copy') lord.clipboard.set(String(effect.text ?? ''));
        else if (effect.kind === 'launchApp') launchApp(String(effect.app));
        else if (effect.kind === 'refreshPackages') refreshPackages();
        else if (effect.kind === 'clock') arm();
      } catch (e) { console.error('native view:', e); }
      return;
    }
    let result = '';
    try {
      switch (effect.kind) {
        case 'settings': settings.set(effect.patch); break;
        case 'openApp': wm.open(effect.app, { args: effect.args || {} }); break;
        case 'forceQuit':
          if (manifest.id !== 'forcequit') throw new Error('app.invalidRequest');
          result = forceQuit(effect.key);
          break;
        case 'nativeCall':
          if (!manifest.calls?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
          result = lord[effect.service][effect.method](...(effect.args || []));
          break;
        case 'nativeRead':
          if (!manifest.reads?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
          result = { method: effect.method, value: lord[effect.service][effect.method](...(effect.args || [])) };
          break;
        default: result = executeNativeEffect(effect, manifest);   // kill, closeSurface, closeWindow…
      }
    } catch (error) {
      const code = error?.code || error?.message || 'app.invalidRequest';
      result = effect.kind === 'nativeCall' || effect.kind === 'nativeRead' ? { error: code } : code;
    }
    lord.nativeViews.result(handle, result);
  };
}

function services(manifest, previous = {}, sampled = {}) {
  const now = Date.now();
  const value = {};
  for (const [service, methods] of Object.entries(manifest.services || {})) {
    const interval = manifest.serviceIntervals?.[service] ?? 1000;
    if (sampled[service] && (interval < 0 || now - sampled[service] < interval)) { value[service] = previous[service]; continue; }
    sampled[service] = now;
    value[service] = {};
    const source = service === '$root' ? lord : lord[service];
    for (const method of methods) {
      try { value[service][method] = source?.[method]?.() ?? null; } catch (_) { value[service][method] = null; }
    }
  }
  return value;
}

// An app's icon (`icon` points): all the view needs is its size and its args. It subscribes to nothing
// (the theme, the windows, the catalog and the settings are for a window's view), so a change of
// window no longer renders every icon of the Dock and the launcher.
function NativeIcon({ manifest, args, icon }) {
  const ref = useRef(null), state = useRef({ handle: 0, sent: '' });
  const json = JSON.stringify({ icon, args: args || {} });
  useLayoutEffect(() => {
    const m = state.current;
    m.handle = ref.current ? lord.nativeViews.mount(ref.current.id, manifest.id, json) : 0;
    m.sent = json;
    if (!m.handle) return undefined;
    runners.set(m.handle, runner(m.handle, manifest, 0, () => ref.current?.id || 0));
    globalThis.__lordNativeView();
    return () => { runners.delete(m.handle); lord.nativeViews.unmount(m.handle); m.handle = 0; };
  }, [manifest.id, icon]);
  useEffect(() => {
    const m = state.current;
    if (!m.handle || m.sent === json) return;
    m.sent = json;
    lord.nativeViews.facts(m.handle, json);
    globalThis.__lordNativeView();
  });
  return h(View, { ref, style: { width: icon, height: icon } });
}

export function NativeView(props) { return props.icon ? h(NativeIcon, props) : h(NativeApp, props); }

// args: the window's (or a Settings section's).
function NativeApp({ manifest, args, icon, style }) {
  const theme = useTheme(), win = useAppWindow() || {}, catalog = useLauncherApps();
  const { lang } = useLocale();
  useWindows();
  const [preferences] = useSettings();
  const clock = useStore(manifest.facts === 'clock' && !icon ? clockState : emptyClock);
  const ref = useRef(null), mounted = useRef({ handle: 0, sent: '' });
  const owner = useRef(null);
  // a shell view (the menu bar) is in no window: the context's default says unfocused
  owner.current = { focused: !!manifest.shell || win.focused !== false, shell: !!manifest.shell };
  const ownsKeyboard = () => owner.current.focused && !textFieldActive()
    && (owner.current.shell || !shellHasKeyboard());
  const [sampled] = useState(() => ({}));
  const [polled, setPolled] = useState(() => (manifest.services && !icon ? services(manifest, {}, sampled) : {}));
  useEffect(() => {
    if (!manifest.services || icon) return undefined;
    const id = setInterval(() => setPolled(previous => services(manifest, previous, sampled)), 1000);
    return () => clearInterval(id);
  }, [manifest, icon]);
  const { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius, controlRadiusSmall, buttonBorder, sidebarBg, hover, selection } = theme;
  const facts = icon ? { icon, args: args || {} } : {
    args: { ...(args || {}), ...(manifest.defaultArgs || {}) }, keyboard: !!manifest.keyboard, lang, focused: ownsKeyboard(), layers: layerFacts(), refreshMs: manifest.refreshMs || 0,
    width: manifest.size?.[0] || 1000,
    settings: manifest.settings ? preferences : undefined,
    services: { ...polled, ...(clock ? { $clock: clock } : {}) },
    ...(manifest.facts === 'windows' ? { windows: appControlFacts() } : {}),
    ...(manifest.facts === 'apps' ? { catalog: catalog.map(appFacts),
      registry: appCatalogSource().map(app => appFacts({ ...app, name: appName(app) })) } : {}),
    // Files reads many more tokens than the other native apps: all of them (not the catalogs of wallpapers and accents)
    context: { theme: manifest.fullTheme ? (({ wallpapers, accents, ...tokens }) => tokens)(theme)
        : { text, textDim, accent, separator, mode, nativeStyle, windowBg, inputBg, control, controlRadius, controlRadiusSmall, buttonBorder, sidebarBg, hover, selection },
      scale: typeScale, decimal: formatNumber(1.5, 1)[1], group: formatNumber(1000).slice(1, -3),
      ...(manifest.facts === 'system' ? { screen: { width: screen.width, height: screen.height }, reactVersion, version: lord.version } : {}) },
  };
  const json = JSON.stringify(facts);
  useLayoutEffect(() => {
    const m = mounted.current;
    m.handle = ref.current ? lord.nativeViews.mount(ref.current.id, manifest.id, json) : 0;
    m.sent = json;
    if (!m.handle) return undefined;
    runners.set(m.handle, runner(m.handle, manifest, win.id, () => ref.current?.id || 0));
    // Event bridge only: Rust owns volumes, mount policy and sidebar updates.
    const sendDisks = () => lord.nativeViews.action(m.handle, { action: ['disks'], extra: { disks: disks.list() } });
    const stopDisks = manifest.id === 'files' && !icon ? disks.watch(sendDisks) : null;
    if (stopDisks) sendDisks();
    const syncFocus = () => { if (!icon) lord.nativeViews.focus(m.handle, ownsKeyboard()); };
    const stopText = onTextField(syncFocus), stopShell = onShellKeyboard(syncFocus);
    syncFocus();
    globalThis.__lordNativeView();
    return () => { stopDisks?.(); stopText(); stopShell(); runners.delete(m.handle); lord.nativeViews.unmount(m.handle); m.handle = 0; };
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
