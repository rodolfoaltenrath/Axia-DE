// scrollbar_model_test.mjs — the overlay scroll bar's model (sdk/scrollbar.js) that ScrollView
// (sdk/components.jsx) draws: thumb geometry at the edge, hit-testing and dragging the thumb, when it
// shows and hides in each Settings → Appearance mode, and the fade timing (none with reduce motion).
//
//   node tests/scrollbar_model_test.mjs        (from server/; run_suites.sh runs it when node exists)
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const M = await import(path.join(here, '../../sdk/scrollbar.js'));
const { SCROLLBAR: S, scrollbarMode, thumbGeometry, offsetForThumb, nearEdge, hitThumb, barShown, barOpacity, hideTime, barWidth } = M;

let n = 0, bad = 0;
const ok = (name, cond, info = '') => { n++; if (!cond) { bad++; console.log(`✗ ${name} ${info}`); } };
const near = (a, b, eps = 1e-6) => Math.abs(a - b) <= eps;

// ---- geometry ----
ok('no bar when it fits', thumbGeometry(300, 300, 0) === null && thumbGeometry(300, 120, 0) === null && thumbGeometry(0, 500, 0) === null);
let g = thumbGeometry(400, 1600, 0);
ok('thumb starts at the inset', near(g.top, S.inset), JSON.stringify(g));
ok('thumb size is the visible share of the track', near(g.size, (400 - 2 * S.inset) / 4), JSON.stringify(g));
g = thumbGeometry(400, 1600, 1200);
ok('at the end the thumb touches the bottom inset', near(g.top + g.size, 400 - S.inset), JSON.stringify(g));
g = thumbGeometry(400, 1600, 99999);
ok('offset past the end is clamped', near(g.top + g.size, 400 - S.inset));
g = thumbGeometry(100, 100000, 0);
ok('minimum thumb', g.size === S.minThumb, String(g.size));
ok('the thumb is thin and sits 2 px from the edge', S.thin === 6 && S.wide === 10 && S.inset === 2);

// ---- dragging ----
for (const off of [0, 300, 600, 1200]) {
  const gg = thumbGeometry(400, 1600, off);
  ok(`drag round trip ${off}`, near(offsetForThumb(400, 1600, gg.top), off, 1e-6), String(offsetForThumb(400, 1600, gg.top)));
}
ok('drag above the track = top', offsetForThumb(400, 1600, -50) === 0);
ok('drag below the track = end', offsetForThumb(400, 1600, 5000) === 1200);
ok('drag with nothing to scroll', offsetForThumb(400, 200, 100) === 0);

// ---- hit testing: the end edge (right; left in RTL) ----
g = thumbGeometry(400, 1600, 0);
ok('hit thumb LTR', hitThumb(300 - 4, g.top + 5, 300, g, false));
ok('hit strip LTR is 14 px', hitThumb(300 - S.hit, g.top + 5, 300, g, false) && !hitThumb(300 - S.hit - 1, g.top + 5, 300, g, false));
ok('miss below the thumb', !hitThumb(296, g.top + g.size + 1, 300, g, false));
ok('miss on the start edge LTR', !hitThumb(4, g.top + 5, 300, g, false));
ok('hit thumb RTL (left edge)', hitThumb(4, g.top + 5, 300, g, true) && !hitThumb(296, g.top + 5, 300, g, true));
ok('no thumb, no hit', !hitThumb(296, 10, 300, null, false));
ok('near edge LTR', nearEdge(290, 300, false) && !nearEdge(200, 300, false));
ok('near edge RTL', nearEdge(10, 300, true) && !nearEdge(290, 300, true));

// ---- showing, hiding and the fade ----
const base = { mode: 'auto', lastScroll: null, near: false, hover: false, dragging: false };
ok('auto: hidden at rest', !barShown(base, 5000));
ok('auto: shown near the edge', barShown({ ...base, near: true }, 5000));
ok('auto: shown while scrolling', barShown({ ...base, lastScroll: 5000 }, 5500));
ok('auto: still shown just before 1 s', barShown({ ...base, lastScroll: 5000 }, 5000 + S.hideAfter - 1));
ok('auto: hides 1 s after the last scroll', !barShown({ ...base, lastScroll: 5000 }, 5000 + S.hideAfter));
ok('scrolling: not shown near the edge', !barShown({ ...base, mode: 'scrolling', near: true }, 5000));
ok('scrolling: shown while scrolling', barShown({ ...base, mode: 'scrolling', lastScroll: 4900 }, 5000));
ok('always: shown at rest', barShown({ ...base, mode: 'always' }, 5000));
ok('dragging keeps it', barShown({ ...base, mode: 'scrolling', dragging: true, lastScroll: 0 }, 99999));
ok('unknown mode = auto', scrollbarMode('x') === 'auto' && scrollbarMode('always') === 'always');

const s = { ...base, lastScroll: 1000 };
const hid = 1000 + S.hideAfter;
ok('hide time', hideTime(s) === hid);
ok('hide time: none when always / near in auto', hideTime({ ...s, mode: 'always' }) === null && hideTime({ ...s, near: true }) === null);
ok('hide time: near does not hold it in scrolling', hideTime({ ...s, mode: 'scrolling', near: true }) === hid);
ok('opacity 1 while shown', barOpacity(s, 1500, null, false) === 1);
ok('fade starts at 1', near(barOpacity(s, hid, hid, false), 1));
ok('fade halfway', near(barOpacity(s, hid + S.fade / 2, hid, false), 0.5));
ok('fade ends', barOpacity(s, hid + S.fade, hid, false) === 0 && barOpacity(s, hid + 5000, hid, false) === 0);
ok('fade is short (≤ 250 ms)', S.fade > 0 && S.fade <= 250);
ok('reduce motion: no fade, just hidden', barOpacity(s, hid, hid, true) === 0 && barOpacity(s, hid + 1, hid, true) === 0);
ok('reduce motion: shown is still 1', barOpacity(s, 1500, null, true) === 1);

// ---- width ----
ok('thin at rest, wide under the pointer or dragging', barWidth(base) === 6 && barWidth({ ...base, hover: true }) === 10 && barWidth({ ...base, dragging: true }) === 10);

console.log(`scrollbar-model-test: ${n} checks, ${bad} failures`);
process.exit(bad ? 1 : 0);
