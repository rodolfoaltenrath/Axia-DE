// Command/effect channel. Selection, window membership and the scene stay in Rust.
let pending = [];
export const switcherBridge = {
  perform: null,
  handle: {
    current: 0,
    attach(handle) {
      for (const action of pending.splice(0)) lord.nativeViews.action(handle, { action });
    },
  },
  reset() { pending = []; this.handle.current = 0; },
  send(action) {
    if (this.handle.current) {
      lord.nativeViews.action(this.handle.current, { action });
      globalThis.__lordNativeView();
    } else pending.push(action);
  },
};
