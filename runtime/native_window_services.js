// Window operations selected by the native application, using the shared WM.
export function windowServices(wm, events = () => []) {
  return {
    'windows.open': { run: ([app, options]) => wm.open(app, options) },
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
