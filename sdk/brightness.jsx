// Renderer binding only: Rust discovers backlights and coalesces logind writes off-thread.
import { createElement as h, useEffect, useState } from 'react';
import { View, Text } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { Slider } from './components.jsx';
import { useTheme } from './theme.js';
import { t } from './i18n.js';
const state = createStore({ devices: [], busy: false, error: '' });
let previous = '', pending = new Map();
const request = (v) => lord.display?.brightness?.(JSON.stringify(v));
function refresh() {
  const result = request({ op: 'refresh' });
  if (!result?.devices) return;
  result.busy = result.busy || pending.size > 0;
  const encoded = JSON.stringify(result);
  if (encoded !== previous) { previous = encoded; state.set(result); }
}
function setBrightness(name, percent) {
  if (!state.get().busy || state.get().error) state.set({ ...state.get(), busy: true, error: '' });
  previous = ''; // reconcile optimistic state even if hardware kept the same percentage
  const queued = pending.get(name);
  if (queued) { queued.percent = percent; return; }
  const latest = { percent };
  pending.set(name, latest);
  // Throttle with the latest value: live adjustment during a drag, one call per 70 ms.
  setTimeout(() => {
    pending.delete(name);
    request({ op: 'set', name, percent: latest.percent });
  }, 70);
}

function DeviceSlider({ device, theme, busy }) {
  const [draft, setDraft] = useState(null);
  useEffect(() => { if (!busy) setDraft(null); }, [device.percent, busy]);
  const value = draft ?? device.percent;
  return h(View, { style: { flexDirection: 'row', alignItems: 'center', gap: 8, direction: 'ltr' } },
    h(Text, { style: { color: theme.textDim, fontSize: 18, width: 26, textAlign: 'center' } }, '☀'),
    h(Slider, { value, min: 1, max: 100, width: 196, onChange: (v) => { setDraft(Math.round(v)); setBrightness(device.name, v); } }),
    h(Text, { style: { color: theme.textDim, fontSize: 11, width: 34, textAlign: 'end' } }, `${value} %`));
}
export function BrightnessControl({ all = false }) {
  const theme = useTheme(), data = useStore(state);
  useEffect(() => {
    refresh(); const soon = setTimeout(refresh, 100), timer = setInterval(refresh, 1000);
    return () => { clearTimeout(soon); clearInterval(timer); };
  }, []);
  const list = all ? data.devices : data.devices.slice(0, 1);
  return h(View, { style: { gap: 8 } },
    list.length ? list.map((device) => h(View, { key: device.name, style: { gap: 6 } },
      all && list.length > 1 ? h(Text, { style: { fontSize: 11, color: theme.textDim } }, device.name) : null,
      h(DeviceSlider, { device, theme, busy: data.busy }))) :
      h(View, { style: { opacity: 0.45, pointerEvents: 'none', gap: 6 } },
        h(View, { style: { height: 20, justifyContent: 'center', paddingHorizontal: 8 } },
          h(View, { style: { height: 4, borderRadius: 2, backgroundColor: theme.control } })),
        h(Text, { style: { fontSize: 11, color: theme.textDim } }, t('shell.cc.brightnessUnavailable'))),
    data.error ? h(Text, { style: { fontSize: 12, color: theme.text } }, t('shell.cc.brightnessFailed')) : null);
}
