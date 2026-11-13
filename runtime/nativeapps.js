// Common mount for compiled apps that use asynchronous SDK services. No app UI
// is implemented here. Lease a window before mounting Rust; release it on close.
import { createElement as h, useLayoutEffect, useState } from 'react';
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
import { media, userDirs } from '../sdk/media.js';
import { soundState } from '../sdk/sound.js';

let followers = 0;
const watches = new Map();
function stopWatch(id) {
  const stop = watches.get(id);
  if (!stop) return;
  watches.delete(id); stop();
  if (--followers === 0) logs.follow(false);
}
const methods = {
  ...fontServices({ fontInfo, fontSample, fontGrid, releaseImage, list: options => lord.fonts.list(options) }),
  ...documentServices({ decodeImage, convertImage, pdfThumb, releaseImage }),
  ...fileServices(fs),
  ...scanServices(print),
  ...mediaServices({ media, userDirs, soundState, events: owner => bridge.events(owner) }),
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
  const app = baseNativeApp(manifest);
  if (!manifest.asyncServices?.length) return app;
  return { ...app, Component: function NativeAsyncApp() {
    const { args } = useAppWindow();
    // WM IDs are strings ("fonts#2"). Rust receives an opaque numeric lease;
    // each mounted window gets a fresh one even when the WM reuses its name.
    const [owner] = useState(() => ++ownerSequence);
    const [ready, setReady] = useState(false);
    useLayoutEffect(() => {
      bridge.open(owner, manifest.asyncServices);
      setReady(true);
      return () => bridge.close(owner);
    }, [owner]);
    return ready ? h(NativeView, { manifest, args: { ...args, __nativeOwner: owner } }) : h(View, { style: { flex: 1 } });
  } };
}
