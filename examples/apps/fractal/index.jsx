// Fractal — installable sample app (.lpk): the Mandelbrot set computed by a WebAssembly
// module (src/mandel.c → assets/mandel.wasm) and drawn with <Image pixels>.
import { View, Text, Image, Button, useTheme, t, loadWasm, formatNumber } from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';

const W = 640, H = 420, MAX = 256;
const START = { cx: -0.6, cy: 0, scale: 3.2 / W };

export default function Fractal() {
  const th = useTheme();
  const wasm = useRef(null);
  const img = useRef(null);
  const [view, setView] = useState(START);
  const [frame, setFrame] = useState(null);
  const [stats, setStats] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    loadWasm('fractal', 'assets/mandel.wasm').then(({ instance }) => { wasm.current = instance.exports; setView({ ...START }); })
      .catch((e) => setError(String(e?.message || e)));
  }, []);

  useEffect(() => {
    const ex = wasm.current;
    if (!ex) return;
    const t0 = Date.now();
    const iter = ex.render(W, H, view.cx, view.cy, view.scale, MAX);
    const ms = Date.now() - t0;
    const data = new Uint8Array(ex.memory.buffer, ex.pixels(), W * H * 4);
    setFrame({ width: W, height: H, data });
    setStats({ iter: Number(iter), ms });
  }, [view]);

  const zoom = (e, f) => {
    const [lx, ly] = lord.getLayout(img.current.id);    // position inside the image
    const x = e.x - lx, y = e.y - ly;
    setView((v) => ({ cx: v.cx + (x - W / 2) * v.scale, cy: v.cy + (y - H / 2) * v.scale, scale: v.scale * f }));
  };

  // Trackpad (the app runs isolated: gestures reach it through zwp_pointer_gestures, docs/TRACKPAD.md):
  // pinching zooms keeping the point under the pointer fixed; two fingers pan the view.
  const pinchK = useRef(1);
  const onPinch = (e) => {
    if (e.phase === 'begin') { pinchK.current = 1; return; }
    if (e.phase !== 'update' || !(e.scale > 0)) return;
    const f = pinchK.current / e.scale;
    pinchK.current = e.scale;
    const [lx, ly] = lord.getLayout(img.current.id);
    const x = e.x - lx, y = e.y - ly;
    setView((v) => ({ cx: v.cx + (x - W / 2) * v.scale * (1 - f), cy: v.cy + (y - H / 2) * v.scale * (1 - f), scale: v.scale * f }));
  };
  const onWheel = (e) => {
    if (!e.precise) return;
    setView((v) => ({ ...v, cx: v.cx + e.deltaX * v.scale, cy: v.cy + e.deltaY * v.scale }));
  };

  return (
    <View style={{ flex: 1, padding: 10, gap: 8, backgroundColor: th.windowBg, direction: 'ltr' }}>
      {error ? <Text style={{ color: '#ff453a', fontSize: 13 }}>{t('fractal.error', { error })}</Text> : (
        <Image ref={img} pixels={frame} onClick={(e) => zoom(e, 0.4)} onContextMenu={(e) => zoom(e, 2.5)}
          onPinch={onPinch} onWheel={onWheel}
          style={{ width: W, height: H, borderRadius: 6, backgroundColor: '#000' }} />
      )}
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
        <Button label={t('fractal.reset')} onClick={() => setView({ ...START })} />
        <Text style={{ fontSize: 12, color: th.textDim, flex: 1 }}>{t('fractal.hint')}</Text>
        {stats && <Text style={{ fontSize: 12, color: th.textDim }}>
          {t('fractal.stats', { w: W, h: H, iter: formatNumber(stats.iter), ms: stats.ms })}</Text>}
      </View>
    </View>
  );
}
