// Cost of moving the mouse over the real Dock (Rust layout, magnification, 24 icons), in a private
// headless session: each move goes through the compositor's event path (__lordEvent) and React renders
// what it causes. Prints the extra time per move over an idle loop of the same yields.
// Not part of `make check`: tools/dock-bench.sh runs it.
import { createElement as h, useEffect, useState } from 'react';
import { render, View } from '@lordos/runtime';
import { configureWindows } from '../../sdk/windows.js';
import { DEFAULT_TOKENS } from '../../sdk/theme.js';
import { Dock } from '../../themes/default/components/Dock.jsx';

const theme = { ...DEFAULT_TOKENS, dockBg: 'rgba(32,34,48,0.55)', dockBorder: 'rgba(255,255,255,0.2)', blur: 24 };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
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
let iconRenders = 0, layoutMs = 0, layoutCalls = 0;
const requestOriginal = lord.appearance.request;
lord.appearance.request = (json) => { const t = lord.now(), out = requestOriginal(json); layoutMs += lord.now() - t; layoutCalls++; return out; };
const COLOR = lord.css(JSON.stringify({ color: '#5c946e' })).color;
const Icon = ({ size }) => { iconRenders++; return h(View, { style: { width: size, height: size, backgroundColor: '#5c946e' } }); };
const items = Array.from({ length: 24 }, (_, i) => ({ id: `app:bench${i}`, name: `Bench ${i}`, Icon, pinned: true }));
const AREA = { x: 0, y: 0, w: 1920, h: 1080 };
configureWindows({ apps: [], workArea: AREA });

let setMagnify;
function Harness() {
  const [magnify, setM] = useState(false);
  setMagnify = setM;
  useEffect(() => { setTimeout(() => run().catch((e) => console.error('dock bench: ' + (e && e.stack || e))), 300); }, []);
  return h(View, { style: { width: AREA.w, height: AREA.h, backgroundColor: '#182036' } },
    h(Dock, { t: { ...theme, mode: 'dark', effects: true }, items, running: ['app:bench3', 'app:bench9'], onOpen() {},
      position: 'bottom', size: 54, magnify, area: AREA }));
}

async function run() {
  await sleep(300);
  const icons = [...nodes.values()].filter((n) => n.style.backgroundColor === COLOR).map((n) => nodes.get(n.parent));
  if (icons.length < 24) { console.error('dock bench: icons not mounted: ' + icons.length); return; }
  const centers = icons.map((n) => { const [x, y, w, hh] = lord.getLayout(n.id); return { id: n.id, x: x + w / 2, y: y + hh / 2 }; });
  const N = 600;
  const yieldN = async (n, move) => { for (let i = 0; i < n; i++) { if (move) move(i); await sleep(0); } };
  // a sweep from the first icon to the last and back, a move at each step
  const move = (i) => {
    const f = (i % 200) / 199, p = i % 400 < 200 ? f : 1 - f, k = Math.min(23, Math.round(p * 23));
    const c = centers[k];
    __lordEvent('move', c.id, c.x + (p * 23 - k) * 54, c.y, 0);
  };
  for (const magnify of [false, true]) {
    setMagnify(magnify);
    await sleep(200);
    await yieldN(100);
    let t = lord.now();
    await yieldN(N);                      // idle: the same yields without events
    const idle = lord.now() - t;
    const r0 = iconRenders, m0 = layoutMs, c0 = layoutCalls;
    t = lord.now();
    await yieldN(N, move);
    const busy = lord.now() - t;
    console.log('dock bench: ' + JSON.stringify({ magnify, moves: N, extraMsPerMove: +((busy - idle) / N).toFixed(3),
      iconRendersPerMove: +((iconRenders - r0) / N).toFixed(2),
      layoutCallsPerMove: +((layoutCalls - c0) / N).toFixed(2), layoutMsPerMove: +((layoutMs - m0) / N).toFixed(3) }));
  }
  console.log('dock bench: done');
}
render(h(Harness));
