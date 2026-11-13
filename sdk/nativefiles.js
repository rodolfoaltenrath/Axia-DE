// What the shell carries out for a Files window drawn as a native view (apps/files/native_app.rs,
// sdk/nativeapps.js, the "filesNativeView" setting): opening files, mounting disks, archives,
// previews, the window's title, menus and frame, and the drag out. The window itself, its listing and
// its input are the app's, in Rust; this is the part that needs the shell, the same effects the
// React host (rustfiles.js) carries out for its own session.
import { t, locale } from './i18n.js';
import { errorText } from './errors.js';
import { wm, getApp } from './windows.js';
import { launchApp } from './apps.js';
import { defaultOpenerId, nativeOf } from './defaults.js';
import { clipboard } from './clipboard.js';
import { startFileDrag } from './filedrag.js';
import { hostEffect } from './rustfiles.js';

const caption = (value) => {
  if (value == null) return '';
  if (typeof value !== 'object') return String(value);
  if (value.error) return errorText(value.error);
  return (value.prefix || '') + t(value.key, value.params);
};

// The effects of a window, one runner per window (handle): `act(action, extra)` sends the app an action.
export function filesEffect(effect, { handle, winId, node, act }) {
  const result = (fields) => lord.nativeViews.result(handle, { kind: effect.kind, generation: effect.generation, ...fields });
  switch (effect.kind) {
    case 'menus': {
      if (!winId) return;
      // the window's menu bar: its items run the app's action
      wm.setMenus(winId, (effect.value || []).map((menu) => ({ label: caption(menu.label), items: menu.items.map((item) =>
        typeof item === 'string' ? item : { ...item, label: caption(item.label), onClick: () => act(item.action) }) })));
      return;
    }
    case 'chrome': if (winId) wm.setArgs(winId, { nativeChrome: effect.value || null }); return;
    case 'title': if (winId) wm.setTitle(winId, caption(effect.title)); return;
    case 'close': case 'cancel': if (winId) wm.close(winId); return;
    case 'dragFiles': {
      if (startFileDrag(effect.paths, node(), !effect.copy)) act(['dragEnd']);
      return;
    }
    case 'transferWindow': {
      const frame = winId ? wm.get(winId) : null;
      const child = wm.open('files', { instance: `transfers-${effect.owner}`,
        frame: { x: (frame?.x ?? 40) + 80, y: (frame?.y ?? 40) + 60, w: 540, h: 460 },
        args: { transferOwner: effect.owner, parent: winId || null, dialog: true } });
      if (!child) lord.files.request(JSON.stringify({ op: 'transferClose', id: effect.owner }));
      return;
    }
    case 'newWindow': wm.open('files', { new: true, args: { path: effect.path } }); return;
    case 'launchApp': launchApp(effect.id); return;
    case 'manageDisk': wm.open('disks', { args: { disk: effect.disk } }); return;
    case 'terminal': wm.open('terminal', { new: true, args: { cwd: effect.path } }); return;
    case 'clipboardText': clipboard.set(effect.text); return;
    case 'openPath': {
      const mime = lord.mime?.guess(effect.path) || 'application/octet-stream';
      const desktop = effect.app || defaultOpenerId(mime) || effect.fallback;
      const native = nativeOf(desktop);
      if (native && getApp(native)) wm.open(native, { new: true, args: { path: effect.path } });
      else if (!lord.apps.launchWith((desktop || '').replace(/\.desktop$/, ''), effect.path)) result({ error: 'files.openFailed' });
      return;
    }
    case 'openUri': lord.apps.launchUri(effect.app || '', effect.uri); return;
    default: {
      // the asynchronous ones (disks, archives, previews, thumbnails): their result goes back to the app
      // Native Files owns source lifetimes in Rust. Bypass the React player's
      // state/listener tracking, which cannot observe Rust's direct release.
      const work = effect.kind === 'mediaPreview'
        ? Promise.resolve().then(() => ({ result: lord.media?.play('files', effect.path, { audio: 1 }) || null }))
        : hostEffect(effect);
      work.then((done) => {
        if (done) result({ ...done });
      }).catch((error) => result({ error: error.message || String(error) }));
    }
  }
}
