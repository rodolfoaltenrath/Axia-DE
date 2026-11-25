// Window operations selected by the native application, using the shared WM.
export function windowServices(wm, events = () => [], screen = () => ({ width: 0, height: 0 })) {
  return {
    'windows.open': { run: ([app, options]) => wm.open(app, options) },
    // Picture in picture: Rust chooses the frame from the screen size, the WM only applies it.
    'windows.screen': { run: () => { const { width, height } = screen(); return { width, height }; } },
    'windows.pip': { run([window, frame]) {
      const ok = frame === null || (frame && [frame.x, frame.y, frame.w, frame.h].every(Number.isFinite) && frame.w > 0 && frame.h > 0);
      if (typeof window !== 'string' || !ok) throw new Error('app.invalidRequest');
      wm.setPip(window, frame ? { x: frame.x, y: frame.y, w: frame.w, h: frame.h } : null);
      return null;
    } },
    'windows.events': { run: (args, owner) => events(owner.id) },
    'windows.menus': { run([window, menus], owner) {
      const bind = items => items.map(item => typeof item === 'object' && item !== null ? {
        ...item, ...(item.items ? { items: bind(item.items) } : {}),
        ...(item.action ? { onClick: () => owner.emit({ kind: 'action', action: item.action }) } : {}),
      } : item);
      wm.setMenus(window, bind(menus));
      return null;
    } },
  };
}
