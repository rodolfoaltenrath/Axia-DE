// TrafficChart.jsx — the Network tab's sent / received chart (and the same chart for one app):
// a mirrored bar strip, sent above the baseline and received below, one bar per time bucket on a
// faint full-height track, so it reads as one continuous strip even when nothing moves. Above it,
// the totals for the visible period as two pills; on it, the auto-scaled maximum. Time runs left
// to right in every language (the strip is laid out ltr).
//
//   <TrafficChart buckets={[[rx, tx], ...]} shift={0..1} seconds={60} />
//
// buckets are bytes per second, oldest first; shift is how far the newest bucket has filled (the
// strip slides smoothly between buckets). Drawn in pixels at the box's real width (draw.js).
import { View, Text, Image, useTheme, useLocale, formatBytes } from '@lordos/sdk';
import { useEffect, useMemo, useRef, useState } from 'react';
import { C, fmt } from './index.jsx';
import { canvas, drawMirror, niceMax, parseColor } from './draw.js';

const tint = (c, a) => { const [r, g, b] = parseColor(c); return `rgba(${r},${g},${b},${a})`; };

function Pill({ arrow, value, color, t }) {
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 4, height: 18, paddingHorizontal: 7, borderRadius: 9,
      backgroundColor: tint(color, t.mode === 'dark' ? 0.18 : 0.14) }}>
      <Text style={{ fontSize: 10, fontWeight: 700, color }}>{arrow}</Text>
      <Text style={{ fontSize: 10, fontWeight: 600, color: t.text, whiteSpace: 'nowrap' }}>{value}</Text>
    </View>
  );
}

// totals: [rxBytes, txBytes] over the visible period (shown as pills) or null.
// caption replaces "last 60 s" and axis the [left, right] labels under it (the history's dates).
export function TrafficChart({ buckets, shift = 0, seconds = 60, height = 52, totals, compact, caption, axis }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const box = useRef(null);
  const [w, setW] = useState(0);
  useEffect(() => {
    if (!box.current) return;
    const nw = Math.round(lord.getLayout(box.current.id)[2]);
    if (nw > 0 && Math.abs(nw - w) > 1) setW(nw);
  });
  let peak = 0;
  for (const [a, b] of buckets) peak = Math.max(peak, a, b);
  const top = niceMax(peak || 1024);
  const track = t.mode === 'dark' ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.045)';
  const key = `${w}|${top}|${shift.toFixed(2)}|${t.mode}|${buckets.map((p) => `${Math.round(p[0])},${Math.round(p[1])}`).join(';')}`;
  const pixels = useMemo(() => (w ? drawMirror(canvas(w, height), buckets, { top, rx: C.rx, tx: C.tx, track, shift }) : null), [key]);
  const label = { position: 'absolute', right: 3, fontSize: 9, color: t.textDim, paddingHorizontal: 3, borderRadius: 3,
    backgroundColor: t.mode === 'dark' ? 'rgba(28,29,33,0.75)' : 'rgba(255,255,255,0.8)' };
  return (
    <View style={{ gap: 5 }}>
      {totals ? (
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          <Pill arrow="↑" value={formatBytes(totals[1])} color={C.tx} t={t} />
          <Pill arrow="↓" value={formatBytes(totals[0])} color={C.rx} t={t} />
          <View style={{ flex: 1 }} />
          <Text style={{ fontSize: 9, color: t.textDim, whiteSpace: 'nowrap' }}>{caption || tr('monitor.net.lastSeconds', { n: seconds })}</Text>
        </View>
      ) : null}
      <View ref={box} style={{ height, direction: 'ltr', borderRadius: 4, overflow: 'hidden' }}>
        {pixels && <Image pixels={pixels} style={{ position: 'absolute', left: 0, top: 0, width: w, height }} />}
        <Text style={{ ...label, top: 1 }}>{`↑ ${fmt.rate(tr, top)}`}</Text>
        <Text style={{ ...label, bottom: 1 }}>{`↓ ${fmt.rate(tr, top)}`}</Text>
      </View>
      {!compact ? (
        <View style={{ flexDirection: 'row', direction: 'ltr' }}>
          <Text style={{ flex: 1, fontSize: 9, color: t.textDim }}>{axis ? axis[0] : tr('monitor.agoSeconds', { n: seconds })}</Text>
          <Text style={{ fontSize: 9, color: t.textDim }}>{axis ? axis[1] : tr('monitor.now')}</Text>
        </View>
      ) : null}
    </View>
  );
}

// The machine's traffic (all interfaces but loopback) from the 4 Hz samples: one bucket per second
// (the mean of its samples), the newest one still filling, so the strip moves every sample.
export function systemBuckets(samples, seconds = 60) {
  const by = new Map();
  let rxBytes = 0, txBytes = 0;
  const last = samples[samples.length - 1];
  if (!last) return { buckets: Array.from({ length: seconds + 1 }, () => [0, 0]), shift: 0, totals: [0, 0] };
  const now = Math.floor(last.t / 1000);
  for (const m of samples) {
    const k = Math.floor(m.t / 1000);
    if (k <= now - seconds - 1) continue;
    const b = by.get(k) || [0, 0, 0];
    b[0] += m.net.rx; b[1] += m.net.tx; b[2]++;
    by.set(k, b);
    const dt = (m.dt || 250) / 1000;
    rxBytes += m.net.rx * dt;
    txBytes += m.net.tx * dt;
  }
  const buckets = [];
  for (let k = now - seconds; k <= now; k++) {
    const b = by.get(k);
    buckets.push(b ? [b[0] / b[2], b[1] / b[2]] : [0, 0]);
  }
  return { buckets, shift: (last.t % 1000) / 1000, totals: [rxBytes, txBytes] };
}
