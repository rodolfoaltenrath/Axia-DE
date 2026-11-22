// Shell side of the native menu bar (rust/axia-shell/src/native_view/menubar.rs).
// Rust lays the bar out, lights its items and keeps the open menu's
// rows; this only knows what Rust can't: the items of a menu (built when it opens, with their
// callbacks) and running one. The Window menu reads native state; the other
// menus are published by MenuBar.jsx here on each render.
import {wm,applyNativeWindowState} from './windows.js';
import {moveWindowToDesktop} from './desktops.js';
import {arrangeWindowMenu,bringAllWindows} from './appcontrol.js';
import {openOverview} from './overview.jsx';
export const NATIVE_MENUBAR = { id: 'shell:menubar', shell: true, appearanceSource: 'native', catalogSource: 'native', keyboard: true, fullTheme: true };

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
  if (effect.kind === 'barPrepare') { calls = new Map();barBridge.prepare?.();return; }
  if (effect.kind === 'windowModel') { applyNativeWindowState(effect.reply);return; }
  if (effect.kind === 'windowMenu') {
    const c=effect.command,id=c.id;
    switch(c.action){
      case 'minimize':wm.minimize(id);break;
      case 'zoom':wm.toggleMaximize(id);break;
      case 'fill':wm.snap(id,'max');break;
      case 'center':wm.center(id);break;
      case 'snap':wm.snap(id,c.zone);break;
      case 'display':wm.moveToDisplay(id,c.display);break;
      case 'desktop':moveWindowToDesktop(id,c.desktop);break;
      case 'newDesktop':moveWindowToDesktop(id);break;
      case 'overview':openOverview();break;
      case 'bringAll':bringAllWindows(id);break;
      case 'arrange':arrangeWindowMenu(id,c.layout);break;
      case 'close':wm.close(id);break;
      case 'focus':wm.focus(id);break;
    }
    return;
  }
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
