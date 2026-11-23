// Shell side of native views: compiled apps that Rust mounts straight into the scene
// (rust/axia-shell/src/native_view, docs/RUST.md "Native views"). Rust draws the app, handles its
// pointer, wheel and text input, translates its captions and refreshes it on its own timer. This
// module only gives the view a mount node inside the window manager's frame, sends it the facts it
// reads (settings, theme, services, catalog), binds its window state and runs shell effects:
// opening apps, saving settings, firewall calls, the clipboard and the window title.
import { version as reactVersion, createElement as h, useEffect, useLayoutEffect, useRef, useState, useMemo, memo } from 'react';
import { View, screen, onScreenChange, shellHasKeyboard, onShellKeyboard, refreshShellKeyboard, layerContainer } from '@lordos/runtime';
import { LAYERS, shellLayers } from './layers.js';
import { filesEffect } from './nativefiles.js';
import { barEffect } from './nativemenubar.js';
import { launcherBridge } from './nativelauncher.js';
import { treeOwners } from './nativetree.js';
import { notificationBridge } from './nativenotifications.js';
import { clockBridge } from './nativeclock.js';
import { settingsBridge } from './nativesettings.js';
import { localeBridge } from './nativelocale.js';
import { textFieldActive, onTextField } from './textfocus.js';
import { useAppWindow } from './components.jsx';
import { typeScale } from './controls.jsx';
import { useTheme } from './theme.js';
import { useLocale, formatNumber } from './i18n.js';
import { wm, applyNativeWindowState } from './windows.js';
import { launchApp } from './apps.js';
import { nativeCatalogFacts, syncNativeCatalog } from './nativecatalog.js';
import { restoreBridge } from './nativerestore.js';
export { appFacts } from './nativecatalog.js';
import { refreshPackages } from './packages.js';
import { settings, settingsRevision } from './settings.js';
import { arm, clockState } from './alarms.js';
import { useStore } from './store.js';
import { forceQuit } from './appcontrol.js';
import { executeNativeEffect, gestureBridge } from './nativeappmodel.js';
import { disks } from './disks.js';
import { createFactsStore, createFactsEncoder, emptyFactsStore, sameFacts, sameRecord, nativeTheme } from './nativeviewfacts.js';
// The nodes of the shell layers a native view may put its menus and popovers in ("layer": "popover").
// Only when the shell has layers; otherwise the views stay in their window.
const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
const LAYER_NAMES = ['popover', 'modal', 'tooltip'];
function layerFacts() {
  if (!shellLayers()) return undefined;
  const out = {};
  for (const name of LAYER_NAMES) { const id = layerContainer(LAYERS[name])?.id; if (id) out[name] = id; }
  return out;
}
const screenFacts = {
  get: () => `${screen.width}:${screen.height}`,
  subscribe: fn => onScreenChange(fn),
};

// Only consumers of these facts subscribe. Geometry/focus changes do not
// prepare Force Quit's app metadata; catalogs are shared across all mounts.
const preferenceFacts = createFactsStore([settings], () => settings.get(), () => settings.get());

const runners = new Map();          // handle → runs one effect
// A shell component that runs the effects of its own view (the title bar): returns the way to stop.
export function addRunner(handle, run) { runners.set(handle, run); return () => runners.delete(handle); }
let drainingServices = false;
globalThis.__lordNativeView = () => {
  if (!appHost && !drainingServices) {
    drainingServices = true;
    try {
      for (let n = 0, list; n < 64 && (list = lord.nativeApps?.takeServices?.()); n++)
        for (const effect of list) {
          if (effect.kind === 'settingsState') settingsBridge.perform?.(effect);
          else if (effect.kind === 'localeState') localeBridge.perform?.(effect);
          else if (effect.kind === 'sessionRestore') restoreBridge.perform?.(effect);
          else if (effect.kind === 'clockReply') clockBridge.perform?.(effect);
          else if (effect.kind === 'shellKeyboard') refreshShellKeyboard();
          else if (effect.kind === 'switcher') { if (effect.reply) applyNativeWindowState(effect.reply); }
          else if (effect.kind === 'windowModel') applyNativeWindowState(effect.reply);
          else if (effect.kind === 'shellGesture') gestureBridge.perform?.(effect);
          else if (effect.kind === 'shellLaunch') executeNativeEffect({ kind: 'openApp', app: effect.app });
          else notificationBridge.perform?.(effect);
        }
    } finally { drainingServices = false; }
  }
  for (const [handle, run] of runners) {
    for (let list = lord.nativeViews.take(handle), n = 0; list && n < 64; list = lord.nativeViews.take(handle), n++)
      for (const effect of list) run(effect);
  }
};

function runner(handle, manifest, winId, node = () => 0, onEffect) {
  const act = (action, extra = {}) => lord.nativeViews.action(handle, { action, extra });
  return (effect) => {
    // a Files window: its own effects (apps/files/native_app.rs), host ones too
    if (manifest.id === 'files' && !effect.host) { try { filesEffect(effect, { handle, winId, node, act }); } catch (e) { console.error('native files:', e); } return; }
    // a tree the shell carries (the Spotlight panel): its events go back to whoever owns it
    if (manifest.id === 'shell:tree' && !effect.host) {
      try { treeOwners.get(effect.owner)?.(effect); } catch (e) { console.error('native tree:', e); }
      return;
    }
    // the launcher: its effects are run by rustlauncher.jsx (sdk/nativelauncher.js)
    if (manifest.id === 'shell:launcher' && !effect.host) {
      try { launcherBridge.perform?.(effect.effect, { handle, node, act }); } catch (e) { console.error('native launcher:', e); }
      return;
    }
    if (manifest.id === 'shell:notifications' && !effect.host) { notificationBridge.perform?.(effect); return; }
    if (manifest.id === 'shell:restore' && !effect.host) { restoreBridge.perform?.(effect); return; }
    // the menu bar: building a menu's items and running one (sdk/nativemenubar.js)
    if (manifest.id === 'shell:menubar' && !effect.host) { barEffect(effect, handle); return; }
    if (manifest.id === 'shell:window-context' && !effect.host) {
      if (effect.kind === 'windowModel') applyNativeWindowState(effect.reply);
      else if (effect.kind === 'windowContext' && effect.command) barEffect({ kind: 'windowMenu', command: effect.command }, handle);
      return;
    }
    if (effect.host) {                 // the host's own: no result goes back to the app
      try {
        if (effect.kind === 'title' && winId) wm.setTitle(winId, effect.title);
        else if (effect.kind === 'windowState' && winId) wm.setArgs(winId, { ...(wm.get(winId)?.args || {}), __nativeState: effect.state });
        else if (effect.kind === 'fullscreen' && winId) wm.setFullscreen(winId, !!effect.on);
        else if (effect.kind === 'paste') lord.nativeViews.paste(handle, String(lord.clipboard.get() ?? ''));
        else if (effect.kind === 'copy') lord.clipboard.set(String(effect.text ?? ''));
        else if (effect.kind === 'mailImage') lord.mail.bind(effect.node, effect.image);
        else if (effect.kind === 'launchApp') launchApp(String(effect.app));
        else if (effect.kind === 'refreshPackages') refreshPackages();
        else if (effect.kind === 'clock') arm();
      } catch (e) { console.error('native view:', e); }
      return;
    }
    let result = '';
    try {
      if (onEffect?.(effect) === true) return;
      switch (effect.kind) {
        case 'settings': settings.set(effect.patch); break;
        case 'settingsState': settingsBridge.perform?.(effect); return;
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
  return sameFacts(previous, value) ? previous : value;
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
function NativeApp(props) {
  return !appHost && props.manifest.appearanceSource === 'native'
    ? h(NativeInputs, props) : h(ThemedNativeApp, props);
}
function ThemedNativeApp(props) {
  const source = useTheme();
  const { lang, region, dir, revision: localeRevision = 0 } = appHost ? useLocale() : {};
  const theme = nativeTheme(source, props.manifest);
  return h(NativeInputs, { ...props, theme, lang, region, dir, localeRevision });
}
function NativeInputs(props) {
  const win = useAppWindow() || {}, previous = useRef(null), previousArgs = useRef(null);
  if (!sameRecord(previous.current, props.theme)) previous.current = props.theme;
  const args = props.args || null;
  if (!sameRecord(previousArgs.current, args)) previousArgs.current = args;
  const windowSource = win.id && !props.manifest.shell ? lord.windowList : null;
  return h(NativeBridge, { ...props, args: previousArgs.current, theme: previous.current,
    region: props.region || '', winId: win.id, windowSource,
    focused: windowSource ? undefined : win.focused !== false, windowMode: win.mode });
}
const NativeBridge = memo(function NativeBridge({ manifest, args, style, mountRef, handleRef, stateSource, theme, lang, region, dir, localeRevision, winId, windowSource, focused, windowMode, onEffect }) {
  const effectHandler = useRef(onEffect);
  effectHandler.current = onEffect;
  const preferences = useStore(appHost && manifest.settings ? preferenceFacts : emptyFactsStore);
  const nativeCatalog = !appHost && manifest.catalogSource === 'native';
  const apps = useStore(nativeCatalog ? emptyFactsStore : manifest.id === 'forcequit' || manifest.facts === 'appcontrol' ? nativeCatalogFacts(true) : manifest.facts === 'apps' ? nativeCatalogFacts() : emptyFactsStore);
  const clock = useStore(manifest.facts === 'clock' ? clockState : emptyFactsStore);
  useStore(manifest.facts === 'system' ? screenFacts : emptyFactsStore);
  const ref = useRef(null), mounted = useRef({ handle: 0, sent: '' });
  const [encoder] = useState(createFactsEncoder);
  const owner = useRef(null);
  useLayoutEffect(() => { if (mountRef) mountRef.current = ref.current; });
  owner.current = { focused: !!manifest.shell || focused, shell: !!manifest.shell };
  // Bound views read window focus in Rust. JS supplies only the shell/text input gate.
  const ownsKeyboard = () => (windowSource || owner.current.focused) && !textFieldActive()
    && (owner.current.shell || !shellHasKeyboard());
  const [sampled] = useState(() => ({}));
  const [polled, setPolled] = useState(() => (manifest.services ? services(manifest, {}, sampled) : {}));
  useEffect(() => {
    if (!manifest.services) return undefined;
    const id = setInterval(() => setPolled(previous => services(manifest, previous, sampled)), 1000);
    return () => clearInterval(id);
  }, [manifest]);
  const windowArgs = useMemo(() => ({ ...(args || {}), ...(manifest.defaultArgs || {}) }), [args, manifest.defaultArgs]);
  const liveServices = useMemo(() => ({ ...polled, ...(clock ? { $clock: clock } : {}) }), [polled, clock]);
  const formats = useMemo(() => appHost ? ({ decimal: formatNumber(1.5, 1)[1], group: formatNumber(1000).slice(1, -3) }) : null, [lang, region, localeRevision]);
  // Each block has its own revision/encoding. Focusing a window reuses the
  // theme/settings/catalog strings rather than stringifying the complete facts.
  const block = (name, value) => encoder.block(name, value);
  const contextFields = { theme: block('theme', theme), windowMode: block('windowMode', windowMode),
    ...(appHost ? { scale: block('scale', typeScale) } : {}),
    ...(appHost && manifest.settings ? { preferenceRevision: block('preferenceRevision', settingsRevision()) } : {}),
    ...(appHost ? { region: block('region', region), direction: block('direction', dir), localeRevision: block('localeRevision', localeRevision),
      decimal: block('decimal', formats.decimal), group: block('group', formats.group) } : {}),
    ...(manifest.facts === 'system' ? { screen: block('screen', { width: screen.width, height: screen.height }),
      reactVersion: block('reactVersion', reactVersion), version: block('version', lord.version) } : {}) };
  const context = `{${Object.entries(contextFields).filter(([, value]) => value !== undefined).map(([key, value]) => `"${key}":${value}`).join(',')}}`;
  const parts = [
    `"args":${block('args', windowArgs)}`, `"keyboard":${block('keyboard', !!manifest.keyboard)}`,
    ...(appHost ? [`"lang":${block('lang', lang)}`] : []),
    ...(windowSource ? [`"windowId":${block('windowId', winId)}`, `"inputAllowed":${block('inputAllowed', !!ownsKeyboard())}`]
      : [`"focused":${block('focused', !!ownsKeyboard())}`]),
    `"refreshMs":${block('refreshMs', manifest.refreshMs || 0)}`, `"width":${block('width', manifest.size?.[0] || 1000)}`,
    `"services":${block('services', liveServices)}`, `"context":${context}`,
  ];
  const layers = block('layers', layerFacts());
  if (layers !== undefined) parts.push(`"layers":${layers}`);
  if (appHost && manifest.settings) parts.push(`"settings":${block('settings', preferences)}`);
  if (apps) parts.push(`"catalog":${block('catalog', apps.catalog)}`, `"registry":${block('registry', apps.registry)}`);
  const json = `{${parts.join(',')}}`;
  useLayoutEffect(() => {
    const m = mounted.current;
    if (!appHost && manifest.catalogSource === 'native') syncNativeCatalog();
    const source = stateSource || windowSource || (appHost ? null : lord.windowList);
    m.handle = ref.current ? (source ? lord.nativeViews.mount(ref.current.id, manifest.id, json, source)
      : lord.nativeViews.mount(ref.current.id, manifest.id, json)) : 0;
    m.sent = json;
    if (!m.handle) return undefined;
    runners.set(m.handle, runner(m.handle, manifest, winId, () => ref.current?.id || 0, effect => effectHandler.current?.(effect)));
    const sendDisks = () => lord.nativeViews.action(m.handle, { action: ['disks'], extra: { disks: disks.list() } });
    const stopDisks = manifest.id === 'files' ? disks.watch(sendDisks) : null;
    if (stopDisks) sendDisks();
    const syncFocus = () => lord.nativeViews.focus(m.handle, !!ownsKeyboard());
    const stopText = onTextField(syncFocus), stopShell = onShellKeyboard(syncFocus);
    syncFocus();
    if (handleRef) { handleRef.current = m.handle; handleRef.attach?.(m.handle); }
    globalThis.__lordNativeView();
    return () => { if (handleRef?.current === m.handle) handleRef.current = 0; stopDisks?.(); stopText(); stopShell(); runners.delete(m.handle); lord.nativeViews.unmount(m.handle); m.handle = 0; };
  }, [manifest.id, windowSource, stateSource, winId]);
  useEffect(() => {
    const m = mounted.current;
    if (!m.handle || m.sent === json) return;
    m.sent = json;
    lord.nativeViews.facts(m.handle, json);
    globalThis.__lordNativeView();
  });
  return h(View, { ref, style: [{ flex: 1 }, style] });
});
