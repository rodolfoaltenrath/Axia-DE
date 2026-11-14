// Passwords — the LordOS password manager. Its window is drawn by Rust (apps/passwords/native/, a
// native view), in this sandboxed process (axia-server --app passwords): this file
// only lends the window its services. Everything decrypted is handled by axia-vault, sandboxed and
// without network; the window is protected (black in screenshots and recordings). Files are
// descriptors the system dialog opened, held here and named to Rust by an id; a vault request
// takes them (they are used up), closing a window closes the ones left. See docs/PASSWORDS.md.
import { createElement as h, useEffect, useLayoutEffect, useState } from 'react';
import {
  View, NativeView, useAppWindow, useStore, locale, wm, vaultAvailable, vaultRequest, onVaultEvent, requestPermission, clipboard,
} from '@lordos/sdk';
import { createNativeAsync } from '../../runtime/native_async.js';
import { windowServices } from '../../runtime/native_window_services.js';

export const APP_ID = 'passwords';

const MANIFEST = {
  id: APP_ID, size: [1040, 660], fullTheme: true,
  calls: { nativeAsync: ['start'] }, reads: { nativeAsync: ['take'] },
};

// Picked files: id → { fd, owner }.
const files = new Map();
let fileSeq = 0;
const closeFd = (fd) => { try { lord.broker.fdClose(fd); } catch (e) { /* already closed */ } };

// FIDO2: the USB permission, then the key is lent to axia-vault for this request.
async function fido() {
  if (!(await requestPermission(APP_ID, 'usb'))) throw new Error('fido-denied');
  const a = lord.vault.fidoAttach(APP_ID);
  if (!a.ok) throw new Error(a.error || 'fido-denied');
}

function pick(save, suggest) {
  let r = null;
  try { r = lord.broker.pick(save ? 1 : 0, suggest || ''); } catch (e) { /* cancelled or denied */ }
  return r && r.fd >= 0 ? r : null;
}

const methods = {
  ...windowServices(wm),
  // axia-vault's events (locking, changes, the key's touch) into the window's queue, until it closes
  'vault.watch': { run(args, owner) {
    owner.cleanup(onVaultEvent((ev) => owner.emit({ kind: 'vault', ...ev })));
    return null;
  } },
  // the window's queue: the vault's events and its menus' commands
  'vault.events': { run: (args, owner) => bridge.events(owner.id) },
  'vault.request': { async run([op, args, ids, needsFido], owner) {
    const fds = [];
    for (const id of ids || []) {
      const f = files.get(id);
      if (!f || f.owner !== owner.id) throw new Error('file');
      files.delete(id);            // the request takes the descriptor
      fds.push(f.fd);
    }
    try {
      if (needsFido) await fido();
      return await vaultRequest(String(op), args || {}, fds.length ? { fds } : undefined);
    } catch (e) {
      throw new Error(e?.code || e?.message || 'other');
    }
  } },
  'file.pick': { run([save, suggest], owner) {
    const r = pick(save, suggest);
    if (!r) return null;
    const id = ++fileSeq;
    files.set(id, { fd: r.fd, owner: owner.id });
    owner.cleanup(() => { const f = files.get(id); if (f) { files.delete(id); closeFd(f.fd); } });
    return { id, name: String(r.name || '') };
  } },
  // Save a text (the recovery key): the dialog, then the file. null when cancelled.
  'file.save': { run([suggest, text]) {
    const r = pick(true, suggest);
    if (!r) return null;
    const ok = lord.broker.fdWrite(r.fd, String(text));
    closeFd(r.fd);
    if (!ok) throw new Error('file');
    return true;
  } },
  // A secret: axia-server marks it sensitive and clears it after `secs` if unchanged.
  'clipboard.secret': { run([value, secs]) { lord.clipboard.set(String(value), { secure: true, clearAfter: Number(secs) || 30 }); return null; } },
  'clipboard.text': { run([value]) { clipboard.set(String(value), { origin: APP_ID }); return null; } },
};
const bridge = createNativeAsync(methods);
lord.nativeAsync = {
  start: (...args) => bridge.start(...args),
  take: (...args) => bridge.take(...args),
  events: (...args) => bridge.events(...args),
};

let ownerSeq = 0;
export default function Passwords() {
  const win = useAppWindow() || {};
  const { region } = useStore(locale);
  // the WM's ids are strings: Rust gets an opaque number of its own for this window
  const [owner] = useState(() => ++ownerSeq);
  const [ready, setReady] = useState(false);
  // protected: black in screenshots, recordings and thumbnails
  useEffect(() => { if (win.id) wm.setProtected(win.id, true); }, [win.id]);
  useLayoutEffect(() => {
    bridge.open(owner, Object.keys(methods));
    setReady(true);
    return () => bridge.close(owner);
  }, [owner]);
  if (!ready) return h(View, { style: { flex: 1 } });
  return h(NativeView, { manifest: MANIFEST,
    args: { __nativeOwner: owner, __nativeWindow: win.id, available: vaultAvailable(), region: region || '' } });
}

// The app icon (the Dock, the launcher): drawn by Rust too.
export function Icon({ size }) {
  return h(NativeView, { manifest: MANIFEST, icon: size });
}
