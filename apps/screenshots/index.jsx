// Screenshots: screen captures and screen recording (a floating capture panel,
// with our own design).
//
// Shortcuts (global, also with a Linux app in front):
//   Print Screen: visual selector       ❖⇧3: direct screen       ❖⇧4: selection
//   ❖⇧5                 the Screenshots panel (and stop a recording in progress)
// With another screenshot app chosen in Settings → Default Apps, these keys open that app instead.
// Protected windows (wm.setProtected) come out black: the server does that, not this app.
// After each capture a floating thumbnail shows up; a click opens the markup editor (arrow, box,
// text, blur). They are saved in ~/Pictures/Screenshots or to the clipboard.
import {
  View, NativeView, Layer,
  useAppWindow, useStore, useWindows, wm, windows,
  t as tr, media, userDirs, fs, addShortcuts, launchApp, createStore, windowTitle, toast, errorText,
} from '@lordos/sdk';
import { useEffect, useRef } from 'react';
import { screen, onScreenChange, displayFor } from '@lordos/runtime';
import { clearTextFocus, focusToken, restoreFocus } from '../../sdk/components.jsx';
import { captureBridge } from '../../sdk/nativecapture.js';
import { notificationBridge } from '../../sdk/nativenotifications.js';
import { syncNativeCatalog } from '../../sdk/nativecatalog.js';

const APP = 'screenshots';
const session = createStore({ revision: 0, recording: null, pendingRecord: false, recent: [] });
function sessionRequest(input) {
  const reply = lord.nativeApps.request(JSON.stringify({ cmd: 'captureSession', ...input }));
  if (reply.state.revision !== session.get().revision) session.set(reply.state);
  for (const effect of reply.effects || []) captureBridge.perform(effect);
  return reply;
}

export function Icon({ size }) { return <NativeView manifest={OVERLAY_MANIFEST} icon={size} />; }

// ---- files ----

function outputRequest(input) {
  const reply = lord.nativeApps.request(JSON.stringify({ cmd: 'captureOutput', ...input }));
  if (reply.capture && reply.capture.revision !== session.get().revision) session.set(reply.capture);
  if (reply.error) throw new Error(reply.error);
  return reply;
}
function outputTarget(kind, source) {
  let reply = outputRequest({ op: 'begin', kind, source });
  const id = reply.id;
  try {
    while (reply.effect) {
      const effect = reply.effect;
      let result;
      try {
        if (effect.op === 'dirs') result = userDirs();
        else if (effect.op === 'mkdir') result = fs.mkdir(effect.path);
        else if (effect.op === 'stat') { fs.stat(effect.path); result = { exists: true }; }
      } catch (error) { result = { error: error?.code || error?.message || 'app.invalidRequest' }; }
      reply = outputRequest({ op: 'next', id, step: reply.step, result });
    }
    return { ...reply.target, id };
  } catch (error) { outputRequest({ op: 'release', id }); throw error; }
}
const releaseOutput = target => outputRequest({ op: 'release', id: target.id });

// ---- capture ----

async function finish(handle, thumbnail) {
  if (!handle) return;
  let target, output;
  try {
    target = outputTarget('image');
    const result = await (target.clipboard ? media.copyImage(handle) : media.saveImage(handle, target.path));
    output = outputRequest({ op: 'complete', id: target.id, result });
  } catch (error) {
    output = target ? outputRequest({ op: 'complete', id: target.id, result: { ok: false } }) : { ok: false };
    toast.set(errorText(error));
  } finally { if (target) releaseOutput(target); }
  showThumb(handle, output.path || null, thumbnail, { ok: output.ok, clipboard: output.clipboard });
}

function showThumb(handle, path, thumbnail, output) {
  const id = wm.open(APP, { instance: 'thumb', args: { handle, path, output } });
  const w = 240, h = 150;
  wm.setPip(id, thumbnail || { x: lord.screen.width - w - 20, y: lord.screen.height - h - 90, w, h });
}

// The resident Rust service owns generations and every capture/countdown deadline.
function captureRequest(input) {
  const reply = lord.nativeApps.request(JSON.stringify({ cmd: 'captureCommand', ...input }));
  if (reply.capture && reply.capture.revision !== session.get().revision) session.set(reply.capture);
  for (const effect of reply.effects || []) captureBridge.perform(effect);
  return reply.result;
}
async function allowed(ask, ticket) {
  try {
    const got = await ask();
    if (!got) { syncNativeCatalog(); captureRequest({ action: 'refused', ticket }); }
    return got;
  } catch (error) {
    const value = error && typeof error === 'object' ? { code: error.code, params: error.params, message: error.message } : error;
    syncNativeCatalog(); captureRequest({ action: 'refused', ticket, error: value });
    return null;
  }
}
export const capture = {
  full: () => captureRequest({ action: 'start', mode: 'full' }),
  area: (mode = 'area') => captureRequest({ action: 'start', mode }),
  record: () => captureRequest({ action: 'start', mode: 'record' }),
};
async function runCaptureWork(effect) {
  const { ticket, mode, recordToken: token } = effect;
  const current = () => !!captureRequest({ action: 'valid', ticket })?.valid;
  if (!current()) return;
  if (mode === 'record') {
    const target = outputTarget('record');
    const path = target.path;
    const id = await allowed(() => media.screenRecord(APP, path, { mic: target.mic, appName: target.appName }), ticket);
    if (!id) { releaseOutput(target); sessionRequest({ op: 'recordFailed', token }); return; }
    if (!sessionRequest({ op: 'recordReady', token, id, path, output: target.id }).accepted) { media.stop(id); media.close(id); releaseOutput(target); return; }
    const off = media.on(id, (type, d) => {
      const result = sessionRequest({ op: 'recordEvent', id, type, data: d });
      if (result.terminal) { off(); media.close(id); }
    });
    const rid = wm.open(APP, { instance: 'rec' });
    wm.setPip(rid, { x: lord.screen.width - 210, y: 40, w: 190, h: 44 });
    return;
  }
  const signature = displaySignature();
  const handle = await allowed(() => media.screenshot(APP, mode === 'full' ? undefined : { x: 0, y: 0, w: screen.width, h: screen.height }), ticket);
  if (!handle) { captureRequest({ action: 'cancel', ticket }); return; }
  if (!captureRequest({ action: 'accept', ticket })?.valid) { media.close(handle); return; }
  if (mode === 'full') {
    try { await finish(handle); } catch (error) { media.close(handle); throw error; }
    return;
  }
  const info = media.info(handle);
  if (displaySignature() !== signature || info?.width !== screen.width || info?.height !== screen.height) {
    media.close(handle); toast.set(tr('screenshots.selector.changed')); return;
  }
  try {
    const id = wm.open(APP, { instance: 'overlay', args: { handle, mode, selectionFacts: selectionFacts() } });
    wm.setFullscreen(id, true);
  } catch (error) { media.close(handle); throw error; }
}
captureBridge.perform = effect => {
  if (effect.kind === 'notificationAction') { captureRequest({ action: 'noticeAction', id: effect.id, index: effect.index }); return; }
  if (effect.op === 'notifications') { for (const item of effect.effects || []) notificationBridge.perform?.(item); }
  else if (effect.op === 'toast') toast.set(effect.text);
  else if (effect.op === 'launch') launchApp(effect.app);
  else if (effect.op === 'open') wm.open(effect.app, { args: effect.args });
  else if (effect.op === 'close') wm.close(effect.id, true);
  else if (effect.op === 'hide') { for (const id of effect.ids) wm.close(id, true); }
  else if (effect.op === 'closeCountdown') {
    if (wm.get(`${APP}#count`)?.args.captureTicket === effect.ticket) wm.close(`${APP}#count`, true);
  } else if (effect.op === 'countdown') {
    const id = wm.open(APP, { instance: 'count', args: { secs: effect.secs, start: effect.start, returnWindow: effect.returnWindow, captureTicket: effect.ticket } });
    wm.setPip(id, effect.frame);
  } else if (effect.op === 'stop') media.stop(effect.id);
  else if (effect.op === 'restoreRecording') {
    if (captureRequest({ action: 'restoreRecording', ticket: effect.ticket, id: effect.id })?.valid && !wm.get(`${APP}#rec`)) {
      const id = wm.open(APP, { instance: 'rec' });
      wm.setPip(id, { x: lord.screen.width - 210, y: 40, w: 190, h: 44 });
    }
  }
  else if (effect.op === 'work') runCaptureWork(effect).catch(error => {
    captureRequest({ action: 'cancel', ticket: effect.ticket }); toast.set(errorText(error));
  });
};

// The SDK transports bindings; Rust owns keys, availability and routing.
if (!(typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__)) addShortcuts(captureRequest({ action: 'bindings' }).map(binding => ({
  ...binding,
  when: () => !!captureRequest({ action: 'shortcutUsable', key: binding.id, media: media.available() })?.enabled,
  run: () => captureRequest({ action: 'shortcut', key: binding.id, media: media.available() }),
})));

// ---- the app's windows ----

const OVERLAY_MANIFEST = { id: APP, nativeView: 'rust', appearanceSource: 'native', keyboard: true, shell: true };
const AUXILIARY_MANIFEST = { id: APP, nativeView: 'rust', appearanceSource: 'native', keyboard: true };
const displaySignature = () => JSON.stringify([screen.width, screen.height, screen.displays]);
function selectionFacts() {
  return { screen: { width: screen.width, height: screen.height }, displays: screen.displays, returnWindow: wm.focused(),
    desktop: wm.desktop(), windows: windows.get().map(w => {
      const { x, y, w: width, h } = w.pip || (w.fullscreen ? displayFor(w) : wm.frame(w));
      return { app: w.app, desktop: w.desktop, minimized: w.minimized, hidden: w.hidden,
        sticky: w.sticky, pip: !!w.pip, title: windowTitle(w), frame: { x, y, w: width, h } };
    }) };
}
// Mount and resource transport only. Rust owns selection, input, controls and the scene.
function Overlay({ handle, mode }) {
  const win = useAppWindow();
  const snapshot = useRef(null);
  if (!snapshot.current || snapshot.current.handle !== handle) snapshot.current = {
    handle, facts: win.args.selectionFacts || selectionFacts(), signature: displaySignature(),
  };
  const { facts, signature } = snapshot.current;
  const returnFocus = useRef(focusToken());
  const lease = useRef(null);
  if (!lease.current || lease.current.handle !== handle) lease.current = { handle, consumed: false };
  const owner = lease.current;
  const done = async (rect, thumbnail) => {
    if (owner.consumed) return;
    owner.consumed = true;
    let cropped = null;
    try { cropped = rect ? media.crop(handle, rect.x, rect.y, rect.w, rect.h) : null; }
    catch (error) { toast.set(errorText(error)); }
    finally { wm.close(win.id, true); media.close(handle); }
    if (cropped) {
      try { await finish(cropped, thumbnail); }
      catch (error) { media.close(cropped); toast.set(errorText(error)); }
    }
  };
  useEffect(() => {
    clearTextFocus();
    media.view(handle, { zoom: 1, cx: 0.5, cy: 0.5, background: 0xff000000 });
    const off = onScreenChange(() => { if (displaySignature() !== signature) { done(null); toast.set(tr('screenshots.selector.changed')); } });
    return () => {
      off(); if (!owner.consumed) { owner.consumed = true; media.close(handle); }
      if (wm.focused() === facts.returnWindow) restoreFocus(returnFocus.current);
    };
  }, [handle]);
  return <Layer name="overlay"><View style={{ position: 'absolute', left: 0, top: 0, width: facts.screen.width, height: facts.screen.height }}>
    <NativeView manifest={OVERLAY_MANIFEST} args={{ ...facts, handle, mode }} onEffect={effect => {
      if (effect.kind !== 'captureSelection') return false;
      if (effect.source !== handle) return true;
      done(effect.rect, effect.thumbnail); return true;
    }} />
  </View></Layer>;
}
function Thumb({ handle, path }) {
  const win = useAppWindow();
  const native = useRef(null);
  useEffect(() => {
    media.view(handle, { fit: 'cover', background: 0xff222222 });
    return () => { if (!windows.get().some(w => w.args?.handle === handle && w.id !== win.id)) media.close(handle); };
  }, [handle]);
  return <NativeView manifest={AUXILIARY_MANIFEST} handleRef={native} args={{ view: 'thumbnail', handle, path, output: win.args.output }} onEffect={effect => {
    if (effect.kind !== 'captureAuxiliary') return false;
    if (effect.view !== 'thumbnail' || effect.source !== handle) return true;
    if (effect.op === 'edit') {
      try {
        const w = Math.min(1000, lord.screen.width - 80), h = Math.min(680, lord.screen.height - 140);
        wm.open(APP, { instance: `edit${handle}`, args: { handle, path }, frame: { x: Math.round((lord.screen.width - w) / 2), y: 60, w, h } });
        setTimeout(() => { if (wm.get(win.id)?.args.handle === handle) wm.close(win.id, true); }, 50);
      } catch (error) {
        if (native.current) lord.nativeViews.action(native.current, { action: ['retry', handle] });
        toast.set(errorText(error));
      }
    } else if (effect.op === 'expire') wm.close(win.id, true);
    return true;
  }} />;
}

function Countdown({ secs, start }) {
  const win = useAppWindow();
  const ticket = win.args.captureTicket;
  const returnFocus = useRef(focusToken());
  useEffect(() => {
    clearTextFocus();
    return () => {
      if (ticket != null) captureRequest({ action: 'countClosed', ticket });
      if (wm.focused() === win.args.returnWindow) restoreFocus(returnFocus.current);
    };
  }, [ticket]);
  return <NativeView manifest={OVERLAY_MANIFEST} args={{ view: 'countdown', secs, start }} onEffect={effect => {
    if (effect.kind !== 'captureAuxiliary') return false;
    if (effect.view !== 'countdown' || effect.source !== start) return true;
    if (ticket == null) wm.close(win.id, true);
    else if (effect.op !== 'complete') captureRequest({ action: 'cancel', ticket });
    return true;
  }} />;
}

function RecBadge() {
  const rec = useStore(session, value => value.recording);
  const native = useRef(null);
  return <NativeView manifest={AUXILIARY_MANIFEST} handleRef={native} args={{ view: 'recording', recording: rec }} onEffect={effect => {
    if (effect.kind !== 'captureAuxiliary') return false;
    if (effect.view === 'recording' && effect.op === 'stop' && rec?.id === effect.source) {
      try {
        if (!media.stop(rec.id) && native.current) lord.nativeViews.action(native.current, { action: ['retry', rec.id] });
      } catch (error) {
        if (native.current) lord.nativeViews.action(native.current, { action: ['retry', rec.id] });
        toast.set(errorText(error));
      }
    }
    return true;
  }} />;
}

// ---- markup editor ----

// Native scene; this wrapper only owns image leases and executes SDK I/O.
function Editor({ handle, path }) {
  const win = useAppWindow(), native = useRef(null), lease = useRef(null);
  if (!lease.current || lease.current.handle !== handle) {
    const info = media.info(handle) || { width: 1, height: 1 };
    lease.current = { handle, info: { width: info.width, height: info.height }, preview: 0, closed: false };
  }
  const owner = lease.current;
  useEffect(() => {
    media.view(handle, { fit: 'contain', background: 0 });
    return () => {
      owner.closed = true;
      if (owner.preview && owner.preview !== handle) media.close(owner.preview);
      media.close(handle);
    };
  }, [handle]);
  const report = result => {
    if (!owner.closed && lease.current === owner && native.current)
      lord.nativeViews.action(native.current, { action: ['editorResult', { ...result, source: handle }] });
  };
  const perform = async effect => {
    try {
      let result;
      if (effect.op === 'copy') result = await media.copyImage(effect.image);
      else {
        const target = outputTarget('annotated', path);
        try {
          const saved = await media.saveImage(effect.image, target.path);
          result = outputRequest({ op: 'complete', id: target.id, result: saved });
        } finally { releaseOutput(target); }
      }
      report({ op: effect.op, token: effect.token, ok: !!result?.ok });
    } catch (error) {
      report({ op: effect.op, token: effect.token, ok: false });
      if (!owner.closed) toast.set(errorText(error));
    }
  };
  return <NativeView manifest={AUXILIARY_MANIFEST} handleRef={native}
    args={{ view: 'editor', handle, path, info: owner.info }} onEffect={effect => {
      if (effect.kind !== 'captureEditor') return false;
      if (effect.source !== handle || owner.closed) return true;
      if (effect.op === 'annotate') {
        try {
          const preview = media.annotate(handle, effect.ops, owner.preview);
          if (preview) {
            if (owner.preview && owner.preview !== preview && owner.preview !== handle) media.close(owner.preview);
            owner.preview = preview;
            media.view(preview, { fit: 'contain', background: 0 });
          }
          report({ op: 'annotate', revision: effect.revision, handle: preview || 0 });
        } catch (error) {
          report({ op: 'annotate', revision: effect.revision, handle: 0 });
          toast.set(errorText(error));
        }
      } else if (effect.op === 'close') wm.close(win.id);
      else if (effect.op === 'save' || effect.op === 'copy') perform(effect);
      return true;
    }} />;
}

// ---- the panel (main window) ----

function panelFacts() {
  return { desktop: wm.desktop(), windows: windows.get().map(w => ({
    id: w.id, app: w.app, desktop: w.desktop, minimized: w.minimized, hidden: w.hidden,
    visible: w.visible, sticky: w.sticky, pip: !!w.pip, protected: !!w.protected,
  })) };
}
function Panel() {
  const win = useAppWindow(), native = useRef(null);
  useWindows();
  const state = useStore(session);
  const protect = () => {
    if (native.current) lord.nativeViews.action(native.current, { action: ['protectFacts'], extra: { facts: panelFacts() } });
  };
  useEffect(() => {
    wm.setMenus(win.id, [{ label: tr('screenshots.menu.window'), items: [
      { label: tr('screenshots.menu.protect'), onClick: protect },
    ] }]);
  }, []);
  return <NativeView manifest={AUXILIARY_MANIFEST} handleRef={native}
    args={{ view: 'panel', session: state, facts: panelFacts() }} onEffect={effect => {
      if (effect.kind !== 'capturePanel') return false;
      if (effect.op === 'capture') {
        if (effect.mode === 'full') capture.full();
        else if (effect.mode === 'record') capture.record();
        else capture.area(effect.mode);
      } else if (effect.op === 'recent') wm.open(effect.app, { new: true, args: { path: effect.path } });
      else if (effect.op === 'protect') wm.setProtected(effect.id, effect.on);
      return true;
    }} />;
}

export default function Screenshots() {
  const win = useAppWindow();
  const inst = String(win.id).split('#')[1] || '';
  const a = win.args || {};
  if (inst === 'overlay' && a.handle) return <Overlay handle={a.handle} mode={a.mode} />;
  if (inst === 'thumb' && a.handle) return <Thumb handle={a.handle} path={a.path} />;
  if (inst === 'count') return <Countdown secs={a.secs} start={a.start} />;
  if (inst === 'rec') return <RecBadge />;
  if (inst.startsWith('edit') && a.handle) return <Editor handle={a.handle} path={a.path} />;
  return <Panel />;
}
