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
  View, Text, MediaView, NativeView, Layer, Button, Toggle, Segmented, TextInput, Toolbar, ToolbarButton, ToolbarGroup, ToolbarDivider, ToolbarSpacer, GroupBox, FormRow,
  useTheme, useAppWindow, useKeyboard, useStore, wm, windows, typeScale,
  t as tr, media, userDirs, fs, addListener, addShortcuts, screenshotLauncher, launchApp, createStore, windowTitle, settings, useSettings, notify, toast, errorText, dndActive, appNotifyPrefs,
} from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';
import { screen, onScreenChange, displayFor } from '@lordos/runtime';
import { clearTextFocus, focusToken, restoreFocus } from '../../sdk/components.jsx';

const APP = 'screenshots';
// Transitional presentation only: selection and image coordinates belong to Rust.
let geometryOwner;
const geometry = input => (geometryOwner ||= lord.nativeApps.create(APP)).request(JSON.stringify(input));
const point = ({ x, y }) => ({ x, y });
const recording = createStore(null);   // { id, path, start }
const recent = createStore([]);       // [{ path, time }]
const countdowns = new Map();        // pending transport callbacks, scoped to their window

export function Icon({ size }) { return <NativeView manifest={OVERLAY_MANIFEST} icon={size} />; }

// ---- files ----

const pad = (n) => String(n).padStart(2, '0');
function stamp() {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}.${pad(d.getMinutes())}.${pad(d.getSeconds())}`;
}
function captureDir() {
  const dir = `${userDirs().pictures}/${tr('screenshots.folder')}`;
  try { fs.mkdir(userDirs().pictures); } catch (e) { /* already there */ }
  try { fs.mkdir(dir); } catch (e) { /* already there */ }
  return dir;
}
function freeName(dir, base, ext) {
  for (let n = 1; n < 1000; n++) {
    const p = `${dir}/${base}${n > 1 ? ` (${n})` : ''}.${ext}`;
    try { fs.stat(p); } catch (e) { return p; }
  }
  return `${dir}/${base} ${Date.now()}.${ext}`;
}
const opts = () => ({ timer: 0, clipboard: false, mic: false, ...(settings.get().screenshots || {}) });

// ---- capture ----

async function finish(handle, thumbnail) {
  if (!handle) return;
  const o = opts();
  let path = null;
  if (o.clipboard) await media.copyImage(handle);
  else {
    const r = await media.saveImage(handle, freeName(captureDir(), tr('screenshots.fileName', { date: stamp() }), 'png'));
    if (r.ok) { path = r.path; recent.set((l) => [{ path, time: Date.now() }, ...l].slice(0, 20)); }
  }
  showThumb(handle, path, thumbnail);
}

function showThumb(handle, path, thumbnail) {
  const id = wm.open(APP, { instance: 'thumb', args: { handle, path } });
  const w = 240, h = 150;
  wm.setPip(id, thumbnail || { x: lord.screen.width - w - 20, y: lord.screen.height - h - 90, w, h });
}

// takes the screen once a frame without our windows was drawn (it closes them first)
function afterHide(fn, delay = 160) {
  for (const w of windows.get()) if (w.app === APP && w.id !== `${APP}#edit` && !String(w.id).startsWith(`${APP}#edit`)) wm.close(w.id, true);
  setTimeout(fn, delay);
}

function withTimer(fn) {
  const secs = opts().timer;
  if (!secs) { fn(); return; }
  const start = Date.now();
  const id = wm.open(APP, { instance: 'count', args: { secs, start, returnWindow: wm.focused() } });
  countdowns.set(id, { start, run: fn });
  wm.setPip(id, { x: Math.round(lord.screen.width / 2 - 60), y: Math.round(lord.screen.height / 2 - 60), w: 120, h: 120 });
}

// The screen permission is denied (no answer here means that) or the app is restricted (an exception):
// nothing is captured, and the person who pressed the key is told why. Do Not Disturb or this app's
// notification settings would hide a banner, so then it is a toast.
function refused(err) {
  const body = errorText(err || { code: 'media.denied.screen', params: { app: tr('apps.screenshots') } });
  const banner = notify({ app: APP, label: tr('apps.screenshots'), title: tr('screenshots.refused'), body,
    actions: [{ id: 'privacy', label: tr('screenshots.refused.open'), run: () => wm.open('settings', { args: { section: 'privacy', nonce: Date.now() } }) }] });
  if (!banner || dndActive() || appNotifyPrefs(APP).style === 'none') toast.set(body);
}
async function allowed(ask) {
  try {
    const got = await ask();
    if (!got) refused();
    return got;
  } catch (e) { refused(e); return null; }
}
const grab = () => allowed(() => media.screenshot(APP));

export const capture = {
  full: () => afterHide(() => withTimer(async () => finish(await grab()))),
  area: (mode = 'area') => afterHide(() => withTimer(async () => {
    const signature = displaySignature();
    const h = await allowed(() => media.screenshot(APP, { x: 0, y: 0, w: screen.width, h: screen.height }));
    if (!h) return;
    const info = media.info(h);
    if (displaySignature() !== signature || info?.width !== screen.width || info?.height !== screen.height) {
      media.close(h); toast.set(tr('screenshots.selector.changed')); return;
    }
    const id = wm.open(APP, { instance: 'overlay', args: { handle: h, mode, selectionFacts: selectionFacts() } });
    wm.setFullscreen(id, true);
  })),
  async record() {
    const cur = recording.get();
    if (cur) { media.stop(cur.id); return; }
    afterHide(() => withTimer(async () => {
      const path = freeName(captureDir(), tr('screenshots.recName', { date: stamp() }), 'mp4');
      const id = await allowed(() => media.screenRecord(APP, path, { mic: opts().mic, appName: tr('apps.screenshots') }));
      if (!id) return;
      recording.set({ id, path, start: Date.now() });
      const off = media.on(id, (type, d) => {
        if (type === 'done' || type === 'closed') {
          off();
          recording.set(null);
          wm.close(`${APP}#rec`, true);
          if (type === 'done' && d.ok) recent.set((l) => [{ path: d.path, time: Date.now(), video: true }, ...l].slice(0, 20));
          media.close(id);
        }
      });
      const rid = wm.open(APP, { instance: 'rec' });
      wm.setPip(rid, { x: lord.screen.width - 210, y: 40, w: 190, h: 44 });
    }), 220);
  },
};

// global shortcuts
const hasMedia = () => typeof lord !== 'undefined' && !!lord.media;
// the screenshot app chosen in Settings → Default Apps, when it isn't this one: the keys open it
const other = () => screenshotLauncher();
const usable = () => !!other() || hasMedia();
const orOther = (fn) => () => { const id = other(); if (id) launchApp(id); else fn(); };
const captureFull = orOther(() => capture.full());
const captureArea = orOther(() => capture.area('area'));
// a recording in progress is this app's, whatever is chosen now: the key still stops it
const openPanelOrStop = () => { if (recording.get()) capture.record(); else orOther(() => wm.open(APP))(); };
addShortcuts([
  { id: 'capture.full', keys: ['Meta+Shift+3'], when: usable, run: captureFull, label: 'shell.shortcuts.captureFull', group: 'system' },
  { id: 'capture.selector', keys: ['PrintScreen'], when: usable, run: captureArea, label: 'shell.shortcuts.captureSelector', group: 'system' },
  { id: 'capture.area', keys: ['Meta+Shift+4'], when: usable, run: captureArea, label: 'shell.shortcuts.captureArea', group: 'system' },
  { id: 'capture.panel', keys: ['Meta+Shift+5'], when: usable, run: openPanelOrStop, label: 'shell.shortcuts.capturePanel', group: 'system' },
]);

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
  return <NativeView manifest={AUXILIARY_MANIFEST} handleRef={native} args={{ view: 'thumbnail', handle, path }} onEffect={effect => {
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
  const entry = countdowns.get(win.id);
  const returnFocus = useRef(focusToken());
  useEffect(() => {
    clearTextFocus();
    return () => {
      if (countdowns.get(win.id) === entry) countdowns.delete(win.id);
      if (wm.focused() === win.args.returnWindow) restoreFocus(returnFocus.current);
    };
  }, [entry]);
  return <NativeView manifest={OVERLAY_MANIFEST} args={{ view: 'countdown', secs, start }} onEffect={effect => {
    if (effect.kind !== 'captureAuxiliary') return false;
    if (effect.view !== 'countdown' || effect.source !== start) return true;
    const pending = countdowns.get(win.id);
    countdowns.delete(win.id); wm.close(win.id, true);
    if (effect.op === 'complete' && pending?.start === start) setTimeout(pending.run, 180);
    return true;
  }} />;
}

function RecBadge() {
  const rec = useStore(recording);
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

const COLORS = ['#ff3b30', '#ffcc00', '#34c759', '#0a84ff', '#ffffff', '#1c1c1e'];

function Editor({ handle, path }) {
  const th = useTheme();
  const win = useAppWindow();
  const info = media.info(handle) || { width: 1, height: 1 };
  const owner = useRef(null);
  if (!owner.current) owner.current = lord.nativeApps.create(APP);
  const request = input => owner.current.request(JSON.stringify({ cmd: 'markup', ...input }));
  const [state, setState] = useState(() => request({ op: 'open', image: info }));
  const latest = useRef(state);
  const { tool, color, ops, draft, text } = state;
  const send = (op, facts = {}) => {
    const reply = request({ op, ...facts });
    if (reply.revision !== latest.current.revision) { latest.current = reply; setState(reply); }
  };
  const [preview, setPreview] = useState(0);
  const [msg, setMsg] = useState(null);
  const box = useRef(null);
  useEffect(() => () => { media.close(handle); }, []);
  useEffect(() => () => { if (preview) media.close(preview); }, [preview]);
  useEffect(() => {
    const all = draft ? [...ops, draft] : ops;
    const id = media.annotate(handle, all, preview);
    if (id && id !== preview) setPreview(id);
    if (id) media.view(id, { fit: 'contain', background: 0 });
  }, [state.drawingRevision, handle]);
  const imageGeometry = (cmd, point) => {
    const [x, y, w, h] = box.current ? lord.getLayout(box.current.id) : [0, 0, 1, 1];
    return geometry({ cmd, point, viewport: { x, y, w, h }, image: info });
  };
  const pointerFacts = e => {
    const [x, y, w, h] = box.current ? lord.getLayout(box.current.id) : [0, 0, 1, 1];
    return { point: point(e), viewport: { x, y, w, h } };
  };
  useEffect(() => {
    const offMove = addListener('pointermove', (e) => {
      if (latest.current.dragging) send('move', pointerFacts(e));
    });
    const offUp = addListener('pointerup', () => {
      if (latest.current.dragging) send('up');
    });
    return () => { offMove(); offUp(); };
  });
  const down = e => send('down', pointerFacts(e));
  const addText = () => send('textSubmit');
  useKeyboard((e) => { if ((e.ctrl || e.meta) && String(e.base).toLowerCase() === 'z') send('undo'); });
  const save = async () => {
    const base = path ? path.split('/').pop().replace(/\.png$/i, '') : tr('screenshots.fileName', { date: stamp() });
    const r = await media.saveImage(preview || handle, freeName(captureDir(), `${base} (${tr('screenshots.annotated')})`, 'png'));
    setMsg(r.ok ? tr('screenshots.savedTo', { folder: tr('screenshots.folder') }) : tr('screenshots.saveError'));
    setTimeout(() => setMsg(null), 2500);
  };
  const copy = async () => { const r = await media.copyImage(preview || handle); setMsg(r.ok ? tr('screenshots.copied') : tr('screenshots.saveError')); setTimeout(() => setMsg(null), 2500); };
  const tools = [['arrow', '↗', 'screenshots.tool.arrow'], ['box', '▢', 'screenshots.tool.box'], ['text', 'T', 'screenshots.tool.text'],
    ['blur', '▒', 'screenshots.tool.blur'], ['line', '╱', 'screenshots.tool.line']];
  const tp = text ? imageGeometry('anchor', text) : null;
  return (
    <View style={{ flex: 1 }}>
      <Toolbar>
        <ToolbarGroup>
          {tools.map(([k, g, key]) => <ToolbarButton key={k} icon={g} title={tr(key)} active={tool === k} onClick={() => send('tool', { value: k })} />)}
        </ToolbarGroup>
        <ToolbarDivider />
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          {COLORS.map((c) => (
            <View key={c} onClick={() => send('color', { value: c })} style={{ width: 22, height: 22, borderRadius: 11, alignItems: 'center', justifyContent: 'center',
              border: `2px solid ${color === c ? th.accent : 'transparent'}` }}>
              <View style={{ width: 16, height: 16, borderRadius: 8, backgroundColor: c, border: `1px solid ${th.separator}` }} />
            </View>
          ))}
        </View>
        <ToolbarSpacer />
        <ToolbarButton icon="⟲" title={tr('screenshots.undo')} onClick={() => send('undo')} />
        <ToolbarButton icon="⧉" title={tr('screenshots.copy')} onClick={copy} />
        <Button label={tr('screenshots.done')} onClick={() => wm.close(win.id)} />
        <Button primary label={tr('screenshots.save')} onClick={save} />
      </Toolbar>
      <View ref={box} style={{ flex: 1, backgroundColor: th.mode === 'dark' ? '#111' : '#ddd', overflow: 'hidden', direction: 'ltr' }} onMouseDown={down}>
        <MediaView media={preview || handle} style={{ position: 'absolute', left: 0, top: 0, right: 0, bottom: 0 }} />
        {text && tp && (
          <View style={{ position: 'absolute', left: tp.left, top: tp.top, width: 260 }}>
            <TextInput value={text.value} autoFocus onChange={(value) => send('text', { value })} onSubmit={addText} onCancel={() => send('textCancel')}
              placeholder={tr('screenshots.textHint')} />
          </View>
        )}
        {msg && <View style={{ position: 'absolute', left: 0, right: 0, bottom: 20, alignItems: 'center' }}>
          <View style={{ paddingHorizontal: 14, height: 30, justifyContent: 'center', borderRadius: 15, backgroundColor: 'rgba(0,0,0,0.75)' }}><Text style={{ fontSize: 12, color: '#fff' }}>{msg}</Text></View>
        </View>}
      </View>
    </View>
  );
}

// ---- the panel (main window) ----

function Panel() {
  const th = useTheme();
  const win = useAppWindow();
  const [s, update] = useSettings();
  const o = { ...opts(), ...(s.screenshots || {}) };
  const set = (p) => update({ screenshots: { ...o, ...p } });
  const rec = useStore(recording);
  const list = useStore(recent);
  useEffect(() => {
    wm.setMenus(win.id, [{ label: tr('screenshots.menu.window'), items: [
      { label: tr('screenshots.menu.protect'), onClick: () => {
        const others = windows.get().filter((w) => w.app !== APP && !w.minimized && w.desktop === wm.desktop());
        const front = others[others.length - 1];
        if (front) wm.setProtected(front.id, !front.protected);
      } },
    ] }]);
  }, []);
  const Mode = ({ glyph, label, onClick, danger }) => (
    <View onClick={onClick} style={{ flex: 1, minWidth: 0, height: 82, borderRadius: 10, alignItems: 'center', gap: 6, paddingHorizontal: 4, paddingTop: 16,
      backgroundColor: danger ? 'rgba(255,59,48,0.14)' : th.mode === 'dark' ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.04)', border: `1px solid ${th.separator}` }}>
      <Text style={{ fontSize: 22, color: danger ? '#ff3b30' : th.text }}>{glyph}</Text>
      <Text style={{ fontSize: typeScale.caption, fontWeight: 500, color: th.text, textAlign: 'center', lineHeight: 1.2 }}>{label}</Text>
    </View>
  );
  const prot = windows.get().filter((w) => w.protected).length;
  return (
    <View style={{ flex: 1, padding: 20, gap: 16 }}>
      <View style={{ flexDirection: 'row', gap: 8 }}>
        <Mode glyph="▣" label={tr('screenshots.mode.full')} onClick={() => capture.full()} />
        <Mode glyph="⧈" label={tr('screenshots.mode.window')} onClick={() => capture.area('window')} />
        <Mode glyph="⬚" label={tr('screenshots.mode.area')} onClick={() => capture.area('area')} />
        <Mode glyph={rec ? '■' : '●'} danger label={tr(rec ? 'screenshots.mode.stop' : 'screenshots.mode.record')} onClick={() => capture.record()} />
      </View>
      <GroupBox footer={null}>
        <FormRow label={tr('screenshots.timer')}>
          <Segmented small value={o.timer} onChange={(v) => set({ timer: v })} options={[[0, tr('screenshots.timerNone')], [5, '5 s'], [10, '10 s']]} />
        </FormRow>
        <FormRow label={tr('screenshots.toClipboard')}><Toggle on={!!o.clipboard} onChange={(v) => set({ clipboard: v })} /></FormRow>
        <FormRow label={tr('screenshots.mic')}><Toggle on={!!o.mic} onChange={(v) => set({ mic: v })} /></FormRow>
      </GroupBox>
      <Text style={{ fontSize: typeScale.caption, color: th.textDim, lineHeight: 1.45, paddingHorizontal: 8 }}>{tr('screenshots.shortcuts')}</Text>
      {prot > 0 && <Text style={{ fontSize: typeScale.caption, color: th.textDim, paddingHorizontal: 8 }}>{tr('screenshots.protectedNote', { count: prot })}</Text>}
      {list.length > 0 && (
        <GroupBox title={tr('screenshots.recent')}>
          {list.slice(0, 5).map((r) => (
            <View key={r.path} onClick={() => wm.open(r.video ? 'player' : 'photos', { new: true, args: { path: r.path } })}
              style={{ minHeight: 30, flexDirection: 'row', alignItems: 'center', gap: 8 }}>
              <Text style={{ width: 16, fontSize: 12, color: th.accent, textAlign: 'center' }}>{r.video ? '▶' : '◫'}</Text>
              <View style={{ flex: 1, minWidth: 0, overflow: 'hidden' }}>
                <Text style={{ fontSize: typeScale.callout, color: th.text, whiteSpace: 'nowrap' }}>{r.path.split('/').pop()}</Text>
              </View>
            </View>
          ))}
        </GroupBox>
      )}
    </View>
  );
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
