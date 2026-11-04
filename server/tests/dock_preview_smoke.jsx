// Real Rust -> compositor -> image -> React contract, in a private headless session.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { render, View, Text } from '@lordos/runtime';
import { configureWindows, windows, wm } from '../../sdk/windows.js';
import { useDockPreviews } from '../../sdk/dockpreviews.jsx';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
const check = (value, message) => { if (!value) throw new Error('dock preview smoke: ' + message); };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const theme = { ...DEFAULT_TOKENS, dockBg: 'rgba(32,34,48,0.55)', dockBorder: 'rgba(255,255,255,0.2)', blur: 24 };
let captures = 0, calls = 0, lastSource = 0, switchHover, latest, source;
let hoverStarted = 0, firstImageMs = null, pendingRequests = 0;
const originalPreview = lord.wm.preview;
lord.wm.preview = (...args) => { const out = originalPreview(...args); if (hoverStarted && firstImageMs == null && out?.pending) pendingRequests++; return out; };
const original = lord.wm.thumbnail;
lord.wm.thumbnail = (...args) => { calls++; const ok = original(...args); if (ok) { captures++; lastSource = args[1]; if (firstImageMs == null) firstImageMs = lord.now() - hoverStarted; } return ok; };
configureWindows({ apps: [{ id: 'preview-test', name: 'Live preview' }], workArea: { x: 0, y: 0, w: 1000, h: 700 } });
windows.set([{ id: 'preview-test', app: 'preview-test', title: 'Live preview', x: 50, y: 40, w: 600, h: 300 }]);
function Harness() {
  const [hover, setHover] = useState(false), [frame, setFrame] = useState(0);
  const root = useRef(null); switchHover = setHover;
  const p = useDockPreviews({ hit: hover ? { id: 'app:preview-test' } : null, anchor: { x: 500, y: 580 },
    area: { x: 0, y: 0, w: 1000, h: 700 }, position: 'bottom', disabled: false, theme });
  latest = p;
  useEffect(() => { const timer = setInterval(() => setFrame((n) => n + 1), 170); return () => clearInterval(timer); }, []);
  useEffect(() => { source = root.current; setTimeout(() => run().catch((e) => console.error(e)), 120); }, []);
  return h(View, { style: { width: 1000, height: 700, backgroundColor: '#182036' } },
    h(View, { ref: (inst) => { root.current = inst; wm.frameRef('preview-test')(inst); }, style: { position: 'absolute', left: 50, top: 40, width: 600, height: 300, backgroundColor: frame % 2 ? '#bc4535' : '#255bad' } },
      h(Text, { style: { color: '#ffffff', fontSize: 26, padding: 30 } }, `Live content ${frame}`)), p.panel);
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
}
render(h(Harness));
