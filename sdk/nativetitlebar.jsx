// A window's title bar drawn by the Rust native view host (native_view/titlebar.rs, the
// "titleBarNativeView" setting) in place of the React header of a theme's Window. Lean on purpose:
// it only mounts the view, sends it the title and the colors when they change, and hands the
// window manager's actions (drag, zoom, a light) to the theme's callbacks.
import { createElement as h, useEffect, useLayoutEffect, useRef } from 'react';
import { View } from '@lordos/runtime';
import { addRunner } from './nativeview.js';

// controls: [{ key, color, glyph }]; tokens: { titleBg, text, textDim, lightOff }; chrome: the
// window's appearance plan or null. onDrag(x, y): the pointer in screen coordinates.
export function NativeTitleBar({ id, title, focused, height, controls, tokens, chrome, onDrag, onMax, onAct }) {
  const ref = useRef(null), state = useRef({ handle: 0, sent: '' }), calls = useRef(null);
  calls.current = { onDrag, onMax, onAct };
  const json = JSON.stringify({ focused: true, args: { id, title, focused, height, controls, tokens, chrome } });
  useLayoutEffect(() => {
    const s = state.current;
    s.handle = ref.current ? lord.nativeViews.mount(ref.current.id, 'shell:titlebar', json) : 0;
    s.sent = json;
    if (!s.handle) return undefined;
    const off = addRunner(s.handle, (effect) => {
      const c = calls.current;
      try {
        if (effect.kind === 'winDrag') { const [ox, oy] = lord.getLayout(ref.current.id); c.onDrag(ox + (effect.x || 0), oy + (effect.y || 0)); }
        else if (effect.kind === 'winMax') c.onMax();
        else if (effect.kind === 'winAct') c.onAct(effect.key);
      } catch (e) { console.error('title bar:', e); }
    });
    globalThis.__lordNativeView();
    return () => { off(); lord.nativeViews.unmount(s.handle); s.handle = 0; };
  }, []);
  useEffect(() => {
    const s = state.current;
    if (!s.handle || s.sent === json) return;
    s.sent = json;
    lord.nativeViews.facts(s.handle, json);
    globalThis.__lordNativeView();
  });
  return h(View, { ref, style: { height } });
}
