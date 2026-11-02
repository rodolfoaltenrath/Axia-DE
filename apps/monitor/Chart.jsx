// Chart.jsx — gráfico de serie temporal del Monitor (como Chart.tsx de TangoOS): guías,
// eje Y, cruz y globo al pasar el mouse, y pausa mientras el mouse está encima.
// Se dibuja en píxeles (<Image pixels>) al ancho real de la caja; siempre de izquierda a
// derecha (el tiempo no se espeja en RTL).
import { View, Text, Image, useTheme, useLocale } from '@lordos/sdk';
import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { canvas, drawArea, niceMax } from './draw.js';

export const Chart = memo(function Chart({ series, height = 120, max, format, times, capacity = 240, compact, bg }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const box = useRef(null);
  const [w, setW] = useState(0);
  const [frozen, setFrozen] = useState(null);
  const [hover, setHover] = useState(null);
  // ancho real: se mide después de cada dibujo (barato: lee la caja del motor)
  useEffect(() => {
    if (!box.current) return;
    const nw = Math.round(lord.getLayout(box.current.id)[2]);
    if (nw > 0 && Math.abs(nw - w) > 1) setW(nw);
  });
  const data = frozen || { series, times };
  const len = data.series[0]?.values.length || 0;
  let peak = 1e-9;
  for (const s of data.series) for (const v of s.values) if (v > peak) peak = v;
  const top = max || niceMax(peak);
  const pixels = useMemo(() => (w > 0 ? drawArea(canvas(w, height), data.series, top, capacity) : null), [w, height, data, top, capacity]);
  const off = capacity - len;
  const onMove = (e) => {
    if (!w) return;
    const { x } = e.local(e.currentTarget);
    const i = Math.round((x / w) * (capacity - 1)) - off;
    setHover(i >= 0 && i < len ? i : null);
  };
  const hx = hover === null ? 0 : ((off + hover) / (capacity - 1)) * w;
  const ago = hover === null ? 0 : Math.round((Date.now() - data.times[hover]) / 1000);
  const card = bg || t.windowBg;
  return (
    <View ref={box} style={{ height, direction: 'ltr' }}
      onMouseEnter={() => setFrozen({ series, times })} onMouseMove={onMove}
      onMouseLeave={() => { setFrozen(null); setHover(null); }}>
      {[0.25, 0.5, 0.75].map((g) => (
        <View key={g} style={{ position: 'absolute', left: 0, right: 0, top: Math.round(height * g), height: 1, backgroundColor: t.separator }} />
      ))}
      {pixels && <Image pixels={pixels} style={{ position: 'absolute', left: 0, top: 0, width: w, height }} />}
      {!compact && (
        <>
          <Text style={{ position: 'absolute', right: 2, top: -2, fontSize: 10, color: t.textDim, backgroundColor: card, paddingHorizontal: 3, borderRadius: 3 }}>{format(top)}</Text>
          <Text style={{ position: 'absolute', right: 2, top: Math.round(height / 2) - 7, fontSize: 10, color: t.textDim, backgroundColor: card, paddingHorizontal: 3, borderRadius: 3 }}>{format(top / 2)}</Text>
        </>
      )}
      {frozen && <Text style={{ position: 'absolute', left: 4, top: -2, fontSize: 10, fontWeight: 600, color: t.textDim }}>{tr('monitor.paused')}</Text>}
      {hover !== null && (
        <>
          <View style={{ position: 'absolute', left: Math.round(hx), top: 0, bottom: 0, width: 1, backgroundColor: t.textDim, pointerEvents: 'none' }} />
          {data.series.map((s) => (
            <View key={s.label} style={{ position: 'absolute', left: hx - 3.5, top: height - 1 - (Math.min(s.values[hover], top) / top) * (height - 3) - 3.5, width: 7, height: 7,
              borderRadius: 4, backgroundColor: s.color, border: `2px solid ${card}`, pointerEvents: 'none' }} />
          ))}
          <View style={{ position: 'absolute', top: 4, ...(hx > w * 0.6 ? { left: Math.max(0, hx - 160) } : { left: hx + 10 }), width: 150, padding: 8, borderRadius: 8,
            backgroundColor: t.mode === 'dark' ? 'rgba(40,42,48,0.96)' : 'rgba(255,255,255,0.97)', boxShadow: '0 8px 24px rgba(0,0,0,0.35)', gap: 2, pointerEvents: 'none', zIndex: 2 }}>
            <Text style={{ fontSize: 10, color: t.textDim, marginBottom: 2 }}>{ago <= 0 ? tr('monitor.now') : tr('monitor.agoSeconds', { n: ago })}</Text>
            {data.series.map((s) => (
              <View key={s.label} style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
                <View style={{ width: 7, height: 7, borderRadius: 2, backgroundColor: s.color }} />
                <Text style={{ flex: 1, fontSize: 11, color: t.text, whiteSpace: 'nowrap' }}>{s.label}</Text>
                <Text style={{ fontSize: 11, fontWeight: 600, color: t.text, whiteSpace: 'nowrap' }}>{format(s.values[hover])}</Text>
              </View>
            ))}
          </View>
        </>
      )}
    </View>
  );
});
