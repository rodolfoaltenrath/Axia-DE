// Common mount for compiled apps that use asynchronous SDK services. No app UI
// is implemented here. Lease a window before mounting Rust; release it on close.
import { createElement as h, Fragment, useEffect, useLayoutEffect, useState } from 'react';
import { pickFile, NativeFilePicker } from './native_file_picker.js';
import { View } from '@lordos/runtime';
import { nativeApp as baseNativeApp } from '../sdk/nativeapps.js';
import { NativeView } from '../sdk/nativeview.js';
import { useAppWindow } from '../sdk/components.jsx';
import { logs } from '../sdk/logs.js';
import * as fs from '../sdk/fs.js';
import { createNativeAsync } from './native_async.js';
import { fontServices } from './native_font_services.js';
import { fontInfo, fontSample, fontGrid, releaseImage, decodeImage, convertImage, pdfThumb } from '../sdk/doc.js';
import { print } from '../sdk/print.js';
import { documentServices } from './native_doc_services.js';
import { scanServices } from './native_scan_services.js';
import { fileServices } from './native_fs_services.js';
import { mediaServices } from './native_media_services.js';
import { media, userDirs, thumbRev } from '../sdk/media.js';
import { soundState } from '../sdk/sound.js';
import { backup, backupStatus } from '../sdk/backup.js';
import { backupServices } from './native_backup_services.js';
import { windowServices } from './native_window_services.js';
import { wm } from '../sdk/windows.js';
import { settings } from '../sdk/settings.js';
import { settingsServices } from './native_settings_services.js';
import { systemServices } from './native_system_services.js';
import { clipboardServices } from './native_clipboard_services.js';
import { inputServices } from './native_input_services.js';
import { startNativeSession } from './native_session_widgets.js';
import { isolatedApp } from '../sdk/packages.js';
import { mailRequest, onMailEvent } from '../sdk/mail.js';
import { pimServices } from './native_pim_services.js';
import { windows } from '../sdk/windows.js';
import { workArea } from '../sdk/windows.js';
import { sysinfoServices } from './native_sysinfo_services.js';
import { systemInfo } from '../sdk/system.js';
import { admin } from '../sdk/admin.js';
import { canPower, askPower } from '../sdk/power.jsx';
import { bluetooth } from '../sdk/hardware.jsx';
import { useStore } from '../sdk/store.js';
import { launchApp } from '../sdk/apps.js';
import { codeStatus } from '../sdk/codesig.js';
import { disks } from '../sdk/disks.js';
import { smart, smartSummaries } from '../sdk/smart.js';
import { diskServices } from './native_disk_services.js';
import { captureApp } from './native_capture.js';
import { vaultServices } from './native_vault_services.js';
import { vaultAvailable, vaultRequest, onVaultEvent } from '../sdk/vault.js';
import { requestPermission } from '../sdk/privacy.js';
import { clipboard } from '../sdk/clipboard.js';
import { locale } from '../sdk/i18n.js';

// Opaque per-window snapshots chosen by Rust, with the lifetime of useWindowState.
const nativeWindowStates = new Map();
windows.subscribe(() => {
  const live = new Set(windows.get().map(w => w.id));
  for (const id of nativeWindowStates.keys()) if (!live.has(id)) nativeWindowStates.delete(id);
});

let followers = 0;
const watches = new Map();
function stopWatch(id) {
  const stop = watches.get(id);
  if (!stop) return;
  watches.delete(id); stop();
  if (--followers === 0) logs.follow(false);
}
const methods = {
  ...diskServices({ disks, admin, fs, systemInfo, smart, smartSummaries, events: owner => bridge.events(owner) }),
  ...vaultServices({ vaultRequest, onVaultEvent, requestPermission, clipboard, events: owner => bridge.events(owner) }),
  ...sysinfoServices({ systemInfo, codeStatus, admin, session: { canPower, restart: () => askPower('reboot') }, fs, userDirs, wm, workArea, launchApp, bluetooth }),
  'files.pick': { run: ([options], owner) => pickFile(options, owner) },
  ...pimServices({ request: mailRequest, onEvent: onMailEvent, pick: () => lord.broker?.pick(false, ''),
    close: fd => lord.broker.fdClose(fd), releaseImage: image => lord.mail.release(image),
    events: owner => bridge.events(owner) }),
  'windows.state': { run([window, state]) { nativeWindowStates.set(window, state); return null; } },
  ...fontServices({ fontInfo, fontSample, fontGrid, releaseImage, list: options => lord.fonts.list(options) }),
  ...documentServices({ decodeImage, convertImage, pdfThumb, releaseImage }),
  ...fileServices(fs),
  ...scanServices(print),
  ...mediaServices({ media, userDirs, soundState, thumbRev, events: owner => bridge.events(owner) }),
  ...settingsServices(settings),
  ...systemServices(owner => bridge.events(owner)),
  ...backupServices({ backup, backupStatus, events: owner => bridge.events(owner) }),
  ...windowServices(wm, owner => bridge.events(owner)),
  ...inputServices(),
  ...clipboardServices({ clipboard: lord.clipboard || {}, fs }),
  'logs.sources': { run: () => logs.sources() },
  'logs.files': { run: () => logs.files() },
  'logs.query': { run: ([query]) => logs.query(query) },
  'logs.system': { run: ([query]) => logs.system(query) },
  'logs.tail': { run: ([file, bytes]) => logs.tail(file, bytes) },
  'logs.write': { run: ([path, text]) => fs.write(path, text) },
  'logs.follow': { run([on], owner) {
    if (!on) { stopWatch(owner.id); return null; }
    if (watches.has(owner.id)) return null;
    if (followers === 0) logs.follow(true);
    const stop = logs.onChange((kind, name) => owner.emit({ kind, name }));
    followers++;
    watches.set(owner.id, stop);
    owner.cleanup(() => stopWatch(owner.id));
    return null;
  } },
};
const bridge = createNativeAsync(methods);
let ownerSequence = 0;
lord.nativeAsync = {
  start: (...args) => bridge.start(...args),
  take: (...args) => bridge.take(...args),
  events: (...args) => bridge.events(...args),
};
export function nativeApp(manifest) {
  const app = localNativeApp(manifest);
  return manifest.process === 'isolated' && lord.appId !== manifest.id
    ? { ...app, isolated: true, Component: isolatedApp(manifest.id, app.Component) } : app;
}
function localNativeApp(manifest) {
  const app = baseNativeApp(manifest);
  startNativeSession(manifest, NativeAsyncMount);
  if (manifest.id === 'screenshots') return captureApp(app);
  if (!manifest.asyncServices?.length) return app;
  return { ...app, Component: function NativeAsyncApp() {
    const { args, id } = useAppWindow() || {};
    return h(NativeAsyncMount, { manifest, args, window: id });
  } };
}
const noHardwareFacts = { get: () => null, subscribe: () => () => {} };
let diskSnapshot=null;
const nativeDiskFacts={
  get:()=>diskSnapshot??(diskSnapshot=disks.list()),
  subscribe(fn) {
    diskSnapshot=disks.list();
    return disks.watch(()=>{diskSnapshot=disks.list();fn();});
  },
};
function NativeAsyncMount({ manifest, args, window }) {
  const bt = useStore(manifest.id === 'sysinfo' ? bluetooth : noHardwareFacts);
  const diskList = useStore(manifest.id === 'disks' ? nativeDiskFacts : noHardwareFacts);
  const language = useStore(manifest.id === 'passwords' ? locale : noHardwareFacts);
  // A protected app's window is black in screenshots, recordings and thumbnails.
  useEffect(() => { if (manifest.protected && window) wm.setProtected(window, true); }, [window]);
  // WM IDs are strings ("fonts#2"). Rust receives an opaque numeric lease;
  // each mounted window gets a fresh one even when the WM reuses its name.
  const [owner] = useState(() => ++ownerSequence);
  const [ready, setReady] = useState(false);
  useLayoutEffect(() => {
    bridge.open(owner, manifest.asyncServices);
    setReady(true);
    return () => bridge.close(owner);
  }, [owner]);
  return ready ? h(Fragment, null, h(NativeView, { manifest, args: { ...args, __nativeOwner: owner, __nativeWindow: window,
    ...(manifest.id === 'sysinfo' ? { __nativeBluetooth: bt } : {}),
    ...(manifest.id === 'disks' ? { __nativeDisks: diskList } : {}),
    ...(manifest.id === 'passwords' ? { available: vaultAvailable(), region: language?.region || '' } : {}),
    ...(manifest.windowState ? { __nativeState: nativeWindowStates.get(window) || {} } : {}) } }),
    manifest.asyncServices.includes('files.pick') ? h(NativeFilePicker, { owner }) : null) : h(View, { style: { flex: 1 } });
}
