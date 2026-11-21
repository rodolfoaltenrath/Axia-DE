// Real Rust -> compositor -> image -> React contract, in a private headless session.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { render, View, Text } from '@lordos/runtime';
import { configureWindows, wm } from '../../sdk/windows.js';
import { useDockPreviews } from '../../sdk/dockpreviews.jsx';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { Dock } from '../../themes/default/components/Dock.jsx';
import { dockPrefs, dockPoof } from '../../sdk/dock.js';
import { settings } from '../../sdk/settings.js';
// a CSS colour as the engine stores it (Rust, rust/axia-shell/src/css_style.rs)
const color = (c) => lord.css(JSON.stringify({ color: c })).color;
const check = (value, message) => { if (!value) throw new Error('dock preview smoke: ' + message); };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// Waits until `ok()` holds, up to `ms` (a slow CI runner can hold the loop for hundreds of ms):
// the states below are checked when they arrive, not after a fixed delay.
async function until(ok, ms = 2000) {
  for (const end = Date.now() + ms; !ok() && Date.now() < end;) await sleep(20);
  return ok();
}
const theme = { ...DEFAULT_TOKENS, dockBg: 'rgba(32,34,48,0.55)', dockBorder: 'rgba(255,255,255,0.2)', blur: 24 };
let captures = 0, calls = 0, lastSource = 0, switchHover, latest, source, resizeSource, showSecond, blockPreview;
let activatedPreview = 0, focusCalls = 0;
const previewStates = new Map(), capturedSizes = new Map();
let hoverStarted = 0, firstImageMs = null, pendingRequests = 0;
const nodes = new Map();
for (const method of ['createNode', 'appendChild', 'insertBefore', 'setStyle', 'setText', 'destroy']) {
  const original = lord[method];
  lord[method] = (...args) => {
    const result = original(...args);
    if (method === 'createNode') nodes.set(result, { id: result, style: {} });
    if (method === 'appendChild' || method === 'insertBefore') nodes.get(args[1]).parent = args[0];
    if (method === 'setStyle') nodes.get(args[0]).style[args[1]] = args[2];
    if (method === 'setText') nodes.get(args[0]).text = args[1];
    if (method === 'destroy') nodes.delete(args[0]);
    return result;
  };
}
let opens = 0, changeDock, iconRenders = 0, pauseSource = false;
const visibility = [];
const originalVisibility = lord.anim.dockVisibility;
lord.anim.dockVisibility = (...args) => { visibility.push(args); return originalVisibility(...args); };
const DockIcon = ({ size }) => { iconRenders++; return h(View, { style: { width: size, height: size, backgroundColor: '#5c946e' } }); };
const originalPreview = lord.wm.preview;
let slowSchedule = false;
const hoverKeys = new Map();
lord.wm.preview = (...args) => {
  const input = JSON.parse(args[0]), out = originalPreview(...args);
  if (input.op === 'activate') activatedPreview = input.id;
  if (input.id && input.op !== 'schedule') previewStates.set(input.id, out);
  if (input.key != null) hoverKeys.set(input.id, input.key);
  if (hoverStarted && firstImageMs == null && out?.pending) pendingRequests++;
  // Model a long capture cooldown; pointer departure still needs an earlier timer.
  if (input.op === 'schedule' && slowSchedule && hoverKeys.get(input.id)) return { waitMs: 1000 };
  return out;
};
const original = lord.wm.thumbnail;
lord.wm.thumbnail = (...args) => { calls++; const ok = original(...args); if (ok) { captures++; lastSource = args[1]; capturedSizes.set(args[1], args.slice(3)); if (firstImageMs == null) firstImageMs = lord.now() - hoverStarted; } return ok; };
const originalFocus = wm.focus;
wm.focus = (...args) => { focusCalls++; return originalFocus(...args); };
configureWindows({ apps: [{ id: 'preview-test', name: 'Live preview', size: [600, 300] }], workArea: { x: 0, y: 0, w: 1000, h: 700 } });
const previewId = wm.open('preview-test', { frame: { x: 50, y: 40, w: 600, h: 300 } });
wm.setTitle(previewId, 'Live preview');
check(previewId === 'preview-test' && wm.get(previewId)?.app === 'preview-test', 'source window registered through the Rust window manager');
function Harness() {
  const [hover, setHover] = useState(false), [frame, setFrame] = useState(0);
  const [dock, setDock] = useState({ position: 'bottom', mode: 'dark' });
  const [dimensions, setDimensions] = useState([600, 300]), [second, setSecond] = useState(null);
  const [previewBlocked, setPreviewBlocked] = useState(false); blockPreview = setPreviewBlocked;
  resizeSource = setDimensions; showSecond = setSecond;
  changeDock = setDock;
  const root = useRef(null); switchHover = setHover;
  const p = useDockPreviews({ hit: hover ? { id: 'app:preview-test' } : null, anchor: { x: 500, y: 580 },
    area: { x: 0, y: 0, w: 1000, h: 700 }, position: 'bottom', disabled: previewBlocked, theme });
  latest = p;
  useEffect(() => { const timer = setInterval(() => { if (!pauseSource) setFrame((n) => n + 1); }, 16); return () => clearInterval(timer); }, []);
  useEffect(() => { source = root.current; setTimeout(() => run().catch((e) => console.error(e)), 120); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(View, { ref: (inst) => { root.current = inst; wm.frameRef('preview-test')(inst); }, style: { position: 'absolute', left: 50, top: 40, width: dimensions[0], height: dimensions[1], backgroundColor: frame % 2 ? '#bc4535' : '#255bad' } },
      h(Text, { style: { color: '#ffffff', fontSize: 26, padding: 30 } }, `Live content ${frame}`)), p.panel,
    second ? h(View, { ref: wm.frameRef(second), style: { position: 'absolute', left: 680, top: 40, width: 300, height: 300, backgroundColor: '#438b64' } }) : null,
    h(Dock, { t: { ...theme, mode: dock.mode, effects: dock.position !== 'left' }, items: [{ id: 'app:preview-test', name: 'Test', Icon: DockIcon }],
      running: [], onOpen: () => { opens++; }, position: dock.position, size: 54, autohide: !!dock.autohide,
      area: { x: 0, y: 0, w: 1000, h: 700 } }));
}
async function checkFeedback() {
  check(dockPrefs({}).dockMagnify === false, 'magnification is off by default');
  check(dockPrefs({ dockMagnify: true }).dockMagnify === true, 'explicit magnification preference is preserved');
  for (const position of ['bottom', 'left', 'right']) {
    const mode = position === 'left' ? 'light' : 'dark';
    changeDock({ position, mode }); await sleep(40);
    const image = [...nodes.values()].find(n => n.style.backgroundColor === color('#5c946e'));
    check(!!image, 'Dock icon is mounted');
    const icon = nodes.get(image.parent), before = lord.getLayout(icon.id);
    const [x, y, w, height] = before;
    const event = (type, id = icon.id, button = 0) => __lordEvent(type, id, x + w / 2, y + height / 2, button);
    const plate = () => [...nodes.values()].find(n => n.parent === icon.id && n.style.pointerEvents === 'none');
    const style = lord.appearance.request(JSON.stringify({ mode })).dockFeedback;
    event('move'); await sleep(40);
    check(plate()?.style.backgroundColor === color(style.hover.backgroundColor), 'hover appears with fixed-size icons');
    event('down'); await sleep(40);
    check(icon.style.opacity === style.pressed.opacity, 'left press dims the icon');
    check(plate()?.style.backgroundColor === color(style.pressed.backgroundColor), 'press differs from hover');
    event('up'); await sleep(40);
    check(icon.style.opacity === 1 && opens === 1, 'release restores the icon and activates once');
    const captureCount = calls;
    await sleep(240);
    check(activatedPreview && !previewStates.get(activatedPreview)?.open, 'activation keeps preview closed while pointer stays on icon');
    check(calls === captureCount, 'activation suppression performs no captures');
    event('move', 0); await sleep(40); event('move');
    check(await until(() => previewStates.get(activatedPreview)?.open), 'leaving and re-entering restores the normal hover delay');
    event('move', 0); await sleep(350);
    event('move');
    event('down'); await sleep(30); event('up', 0); await sleep(30);
    check(icon.style.opacity === 1 && opens === 1, 'release outside clears press without activating');
    event('down', icon.id, 2); await sleep(30);
    check(icon.style.opacity === 1, 'right click does not show left-press feedback');
    event('up', icon.id, 2); event('move', 0); await sleep(30);
    check(!plate(), 'leaving removes hover');
    const after = lord.getLayout(icon.id);
    check(after[2] === before[2] && after[3] === before[3], 'feedback never enlarges the icon');
    opens = 0;
  }
  console.log('dock feedback integration: passed');
}
function clickText(label, header = 0) {
  const beneath = (node, ancestor) => { for (let n = nodes.get(node); n; n = nodes.get(n.parent)) if (n.id === ancestor) return true; return false; };
  const text = [...nodes.values()].find(n => n.text === label && (!header || beneath(n.id, header)));
  check(!!text, 'control exists: ' + label);
  const target = text.parent, box = lord.getLayout(target);
  __lordEvent('move', target, box[0] + box[2] / 2, box[1] + box[3] / 2, 0);
  for (const type of ['down', 'up']) __lordEvent(type, target, box[0] + box[2] / 2, box[1] + box[3] / 2, 0);
}
async function checkDisabledHover() {
  latest.activate('app:preview-test');
  blockPreview(true); await sleep(40);
  blockPreview(false); await sleep(200);
  check(!latest.open, 'disabling preview does not rearm a stationary pointer');
  blockPreview(true); switchHover(false); await sleep(40);
  switchHover(true); await sleep(40);
  const stopped = calls; await sleep(180);
  check(!latest.open && calls === stopped, 'disabled preview never captures after pointer re-entry');
  blockPreview(false);
  check(await until(() => latest.open), 'leaving and returning while disabled rearms preview when enabled');
  slowSchedule = true; await sleep(100);
  const left = lord.now();
  switchHover(false); await sleep(350);
  check(!latest.open && lord.now() - left < 750, 'pointer departure shortens a long capture cooldown');
  slowSchedule = false;
  console.log('dock disabled hover integration: passed');
}
async function checkPreviewSizingAndClose() {
  switchHover(false); await sleep(40);
  switchHover(true); check(await until(() => latest.open), 'preview opens for sizing checks');
  for (const [width, height] of [[600, 300], [300, 300], [300, 600]]) {
    capturedSizes.delete(source.id); resizeSource([width, height]);
    check(await until(() => {
      const size = capturedSizes.get(source.id);
      return size && Math.abs(size[0] / size[1] - width / height) < 0.02;
    }), 'capture follows source aspect ratio without letterbox: ' + width + 'x' + height);
  }
  const second = wm.open('preview-test', { new: true, frame: { x: 680, y: 40, w: 300, h: 300 } });
  wm.setTitle(second, 'Second preview'); showSecond(second);
  // The Wayland bridge always returns false until the client unmaps, even
  // when it accepts the request. Reproduce that contract without forcing close.
  wm.setArgs(second, { surface: 999999 });
  let requestedClose = 0;
  const releaseSecond = wm.setCloseGuard(second, () => { requestedClose++; return false; });
  check(await until(() => [...nodes.values()].some(n => n.text === 'Second preview')), 'second window has its own card');
  await sleep(40); // The rendered caption must reach native layout before pointer input.
  const title = [...nodes.values()].find(n => n.text === 'Second preview');
  const header = nodes.get(title.parent).parent;
  const focused = focusCalls;
  clickText('×', header);
  await sleep(40);
  check(requestedClose === 1 && !!wm.get(second) && latest.open, 'async close keeps remaining previews visible while awaiting the client');
  wm.close(second, true); releaseSecond(); // The client's eventual unmap event.
  check(await until(() => !wm.get(second)), 'close button closes only its target window');
  check(await until(() => !!wm.get(previewId) && latest.open), 'other window stays open with its preview');
  check(focusCalls === focused, 'close never bubbles into card activation');
  wm.minimize(previewId);
  check(!!wm.get(previewId).minimized, 'guarded close starts with a minimized window');
  const release = wm.setCloseGuard(previewId, () => false);
  clickText('×');
  check(await until(() => !latest.open), 'guarded close dismisses preview so the app can ask about unsaved changes');
  check(!!wm.get(previewId), 'close respects the app guard');
  check(await until(() => !wm.get(previewId).minimized), 'guarded close restores the window so confirmation is visible');
  check(focusCalls === focused + 1, 'guarded close reveals the minimized native window once'); release();
  switchHover(false); await sleep(40); switchHover(true);
  check(await until(() => latest.open), 'guarded window can be previewed again');
  clickText('×');
  check(await until(() => !wm.get(previewId) && !latest.open), 'closing the last window removes the preview');
  const stopped = calls; await sleep(240);
  check(calls === stopped, 'last-window close stops capture polling');
  switchHover(false);
  console.log('dock preview sizing and close integration: passed');
}
async function checkNativeDockAnimations() {
  pauseSource = true;
  settings.set({ reduceMotion: false });
  for (const position of ['bottom', 'left', 'right']) {
    changeDock({ position, mode: 'dark', autohide: true });
    const beyond = (l) => position === 'bottom' ? l[1] >= 700 : position === 'left' ? l[0] + l[2] <= 0 : l[0] >= 1000;
    const last = () => visibility[visibility.length - 1];
    await until(() => last()?.[3] === false && beyond(lord.getLayout(last()[0])));
    const hidden = visibility[visibility.length - 1];
    check(hidden?.[3] === false, 'auto-hide starts hidden: ' + position);
    const node = hidden[0], layout = lord.getLayout(node);
    check(beyond(layout), 'hidden Dock moves beyond its display edge: ' + position);
    const [x, y] = position === 'bottom' ? [500, 699] : position === 'left' ? [0, 350] : [999, 350];
    __lordEvent('move', 0, x, y, 0);
    check(await until(() => last()?.[3] === true), 'edge dwell shows Dock: ' + position);
    const renders = iconRenders;
    const inside = (l) => position === 'bottom' ? l[1] < 700 : position === 'left' ? l[0] >= 0 : l[0] < 1000;
    await until(() => inside(lord.getLayout(node)));
    check(iconRenders === renders, 'native reveal never renders the icon per frame');
    const shown = lord.getLayout(node);
    check(position === 'bottom' ? shown[1] < 700 : position === 'left' ? shown[0] >= 0 : shown[0] < 1000,
      'shown Dock returns to display: ' + position);
    __lordEvent('move', 0, 500, 100, 0);
    check(await until(() => last()?.[3] === false, 3000), 'leaving hides after grace: ' + position);
  }
  settings.set({ reduceMotion: true });
  await until(() => visibility[visibility.length - 1]?.[4] === true);
  check(visibility[visibility.length - 1]?.[4] === true, 'reduced motion applies visibility immediately');
  changeDock({ position: 'bottom', mode: 'dark', autohide: false });
  await sleep(80);
  const baseline = lord.stats().nodes;
  dockPoof.set({ x: 300, y: 300, Icon: DockIcon, t0: Date.now() });
  await sleep(60);
  check(lord.stats().nodes === baseline, 'reduced motion suppresses the removal effect');
  settings.set({ reduceMotion: false });
  await sleep(60);
  check(lord.stats().nodes === baseline, 'turning animations on never replays an old removal event');
  // A fresh effect replaces the old one; no stale end handler may remove it.
  dockPoof.set({ x: 300, y: 300, Icon: DockIcon, t0: Date.now() + 1 });
  await sleep(80);
  check(lord.stats().nodes > baseline + 5, 'native removal mounts the icon and five puffs');
  const count = iconRenders;
  await sleep(140);
  check(iconRenders === count, 'native removal never renders its icon per frame');
  dockPoof.set({ x: 400, y: 300, Icon: DockIcon, t0: Date.now() + 2 });
  await sleep(180);
  check(lord.stats().nodes > baseline + 5, 'replacement effect survives the old completion deadline');
  await sleep(250);
  check(lord.stats().nodes === baseline, 'completion removes every temporary effect node');
  dockPoof.set(null);
  console.log('dock native animation integration: passed');
}

async function checkScheduler() {
  let count = 0, frame = 0;
  const step = () => { count++; frame = requestAnimationFrame(step); };
  frame = requestAnimationFrame(step);
  await sleep(350); cancelAnimationFrame(frame);
  check(count >= 5 && count <= 30, 'headless RAF follows software cadence: ' + count);
  const began = lord.now();
  await new Promise((resolve) => requestAnimationFrame(() => setImmediate(resolve)));
  check(lord.now() - began < 120, 'JS work queued while painting wakes without an unrelated timer');
  __lordClearTimers();
  check(lord.timers.next(lord.now()) === -1 && !lord.timers.hasFrames(), 'context reset releases timers and frames');
  console.log('scheduler compositor integration: passed');
}

async function run() {
  check(source?.id && lord.wm.preview, 'native source and bridge exist');
  check(calls === 0, 'no capture work at rest');
  hoverStarted = lord.now();
  switchHover(true); await sleep(80); check(!latest.open, 'hover delay prevents accidental popup');
  await sleep(600); check(latest.open && captures > 0, 'hover opens and captures real source');
  console.log('dock preview timing: ' + JSON.stringify({ firstImageMs, pendingRequests }));
  check(pendingRequests <= 3, 'hover waits for its deadline without repeated polling');
  const before = captures, liveStart = lord.now(); await sleep(600);
  const liveMs = lord.now() - liveStart, liveFrames = captures - before;
  check(liveFrames > 0, 'changed content produces live frames');
  check(liveFrames <= Math.ceil(liveMs * 20 / 1000) + 1, '20 fps budget stays aggregate');
  console.log('dock preview cadence: ' + JSON.stringify({ fps: liveFrames * 1000 / liveMs, frames: liveFrames, ms: liveMs }));
  check(lastSource === source.id, 'captures original window, never the panel');
  check(captures < 30, 'aggregate refresh remains bounded');
  const layout = lord.getLayout(source.id); check(layout[2] === 600 && layout[3] === 300, 'source size is preserved');
  switchHover(false); await sleep(500); check(!latest.open, 'leaving dismisses after grace');
  const stopped = calls; await sleep(350); check(calls === stopped, 'closed panel stops capture polling');
  switchHover(true); await sleep(650); check(latest.open, 'preview reopens');
  globalThis.__lordEvent('down', 0, 4, 4, 0); await sleep(120); check(!latest.open, 'outside press dismisses');
  console.log('dock preview integration: passed');
  await checkFeedback();
  await checkDisabledHover();
  await checkPreviewSizingAndClose();
  await checkNativeDockAnimations();
  await checkScheduler();
}
render(h(Harness));
