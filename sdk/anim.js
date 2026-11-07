// anim.js — the native animator's end notices (rust/axia-shell/src/anim.rs, server/src/ui/js_anim.c).
// lord.anim.* starts an animation and returns its id; the compositor steps it with no JS per frame
// and calls __lordAnimDone(id) on its last frame. whenAnimDone(id, fn) runs fn then.
const pending = new Map();
globalThis.__lordAnimDone = (id) => { const fn = pending.get(id); pending.delete(id); fn?.(); };
export function whenAnimDone(id, fn) {
  if (!id) return () => {};
  pending.set(id, fn);
  return () => { if (pending.get(id) === fn) pending.delete(id); };
}
export const nativeAnim = () => (typeof lord !== 'undefined' && lord.anim ? lord.anim : null);
