// Real Rust -> compositor -> image -> React contract, in a private headless session.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { render, View, Text } from '@lordos/runtime';
import { configureWindows, windows, wm } from '../../sdk/windows.js';
import { useDockPreviews } from '../../sdk/dockpreviews.jsx';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { Dock } from '../../themes/default/components/Dock.jsx';
import { dockPrefs } from '../../sdk/dock.js';
import { color } from '../../runtime/style.js';
const check = (value, message) => { if (!value) throw new Error('dock preview smoke: ' + message); };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const theme = { ...DEFAULT_TOKENS, dockBg: 'rgba(32,34,48,0.55)', dockBorder: 'rgba(255,255,255,0.2)', blur: 24 };
let captures = 0, calls = 0, lastSource = 0, switchHover, latest, source;
let hoverStarted = 0, firstImageMs = null, pendingRequests = 0;
const nodes = new Map();
for (const method of ['createNode', 'appendChild', 'insertBefore', 'setStyle', 'destroy']) {
  const original = lord[method];
  lord[method] = (...args) => {
    const result = original(...args);
    if (method === 'createNode') nodes.set(result, { id: result, style: {} });
    if (method === 'appendChild' || method === 'insertBefore') nodes.get(args[1]).parent = args[0];
    if (method === 'setStyle') nodes.get(args[0]).style[args[1]] = args[2];
    if (method === 'destroy') nodes.delete(args[0]);
    return result;
  };
}
let opens = 0, changeDock;
const DockIcon = ({ size }) => h(View, { style: { width: size, height: size, backgroundColor: '#5c946e' } });
const originalPreview = lord.wm.preview;
lord.wm.preview = (...args) => { const out = originalPreview(...args); if (hoverStarted && firstImageMs == null && out?.pending) pendingRequests++; return out; };
const original = lord.wm.thumbnail;
lord.wm.thumbnail = (...args) => { calls++; const ok = original(...args); if (ok) { captures++; lastSource = args[1]; if (firstImageMs == null) firstImageMs = lord.now() - hoverStarted; } return ok; };
configureWindows({ apps: [{ id: 'preview-test', name: 'Live preview' }], workArea: { x: 0, y: 0, w: 1000, h: 700 } });
windows.set([{ id: 'preview-test', app: 'preview-test', title: 'Live preview', x: 50, y: 40, w: 600, h: 300 }]);
function Harness() {
  const [hover, setHover] = useState(false), [frame, setFrame] = useState(0);
  const [dock, setDock] = useState({ position: 'bottom', mode: 'dark' });
  changeDock = setDock;
  const root = useRef(null); switchHover = setHover;
  const p = useDockPreviews({ hit: hover ? { id: 'app:preview-test' } : null, anchor: { x: 500, y: 580 },
    area: { x: 0, y: 0, w: 1000, h: 700 }, position: 'bottom', disabled: false, theme });
  latest = p;
  useEffect(() => { const timer = setInterval(() => setFrame((n) => n + 1), 170); return () => clearInterval(timer); }, []);
  useEffect(() => { source = root.current; setTimeout(() => run().catch((e) => console.error(e)), 120); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(View, { ref: (inst) => { root.current = inst; wm.frameRef('preview-test')(inst); }, style: { position: 'absolute', left: 50, top: 40, width: 600, height: 300, backgroundColor: frame % 2 ? '#bc4535' : '#255bad' } },
      h(Text, { style: { color: '#ffffff', fontSize: 26, padding: 30 } }, `Live content ${frame}`)), p.panel,
    h(Dock, { t: { ...theme, mode: dock.mode, effects: dock.position !== 'left' }, items: [{ id: 'app:preview-test', name: 'Test', Icon: DockIcon }],
      running: [], onOpen: () => { opens++; }, position: dock.position, size: 54,
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
async function run() {
  check(source?.id && lord.wm.preview, 'native source and bridge exist');
  check(calls === 0, 'no capture work at rest');
  hoverStarted = lord.now();
  switchHover(true); await sleep(80); check(!latest.open, 'hover delay prevents accidental popup');
  await sleep(600); check(latest.open && captures > 0, 'hover opens and captures real source');
  console.log('dock preview timing: ' + JSON.stringify({ firstImageMs, pendingRequests }));
  check(pendingRequests <= 3, 'hover waits for its deadline without repeated polling');
  const before = captures; await sleep(600); check(captures > before, 'changed content produces live frames');
  check(lastSource === source.id, 'captures original window, never the panel');
  check(captures < 25, 'aggregate refresh remains bounded');
  const layout = lord.getLayout(source.id); check(layout[2] === 600 && layout[3] === 300, 'source size is preserved');
  switchHover(false); await sleep(500); check(!latest.open, 'leaving dismisses after grace');
  const stopped = calls; await sleep(350); check(calls === stopped, 'closed panel stops capture polling');
  switchHover(true); await sleep(650); check(latest.open, 'preview reopens');
  globalThis.__lordEvent('down', 0, 4, 4, 0); await sleep(120); check(!latest.open, 'outside press dismisses');
  console.log('dock preview integration: passed');
  await checkFeedback();
}
render(h(Harness));
