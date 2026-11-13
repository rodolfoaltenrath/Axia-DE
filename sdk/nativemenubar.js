// Shell side of the native menu bar (rust/axia-shell/src/native_view/menubar.rs; the
// "menuBarNativeView" setting). Rust lays the bar out, lights its items and keeps the open menu's
// rows; this only knows what Rust can't: the items of a menu (built when it opens, with their
// callbacks) and running one. MenuBar.jsx publishes the menus here on each render.
export const NATIVE_MENUBAR = { id: 'shell:menubar', shell: true, keyboard: true, fullTheme: true };

// menus: { name: { items } } as getters, so only the opened one is built; prepare: what opening a
// menu closes first (the Control Center, Spotlight).
export const barBridge = { menus: null, prepare: null };

let calls = new Map();          // id → the callback of an item of the open menu

// Items with their callbacks → what Rust can hold: an action `["run", id]` per item.
function describe(items) {
  return (items || []).map((item) => {
    if (typeof item === 'string' || !item) return item;
    const out = {};
    for (const key of ['label', 'hint', 'header', 'disabled', 'checked', 'danger']) if (item[key] != null) out[key] = item[key];
    if (item.submenu) out.submenu = describe(item.submenu);
    else if (typeof item.onClick === 'function') { const id = calls.size + 1; calls.set(id, item.onClick); out.action = ['run', id]; }
    return out;
  });
}

// An effect of the bar (a native view effect that is not the host's): `act` is not needed, the
// answer to barOpen goes back as the effect's result.
export function barEffect(effect, handle) {
  if (effect.kind === 'barOpen') {
    calls = new Map();
    let items = [];
    try {
      barBridge.prepare?.();
      items = describe(barBridge.menus?.[effect.name]?.items);
    } catch (e) { console.error('menu bar:', e); }
    lord.nativeViews.result(handle, { open: { name: effect.name, items } });
  } else if (effect.kind === 'barRun') {
    const run = calls.get(effect.id);
    calls = new Map();
    try { run?.(); } catch (e) { console.error('menu bar:', e); }
  }
}
