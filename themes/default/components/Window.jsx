// Window frame: draggable title bar, traffic lights, shadow. The content is the app
// (the SDK's AppHost); the theme only decides how the frame looks. Moving and resizing (invisible
// borders on all 4 sides and 4 corners, <WindowShell>) is done by axia-server in C
// (docs/WINDOWS.md), a maximized or snapped window coming loose at its own size as soon as it moves;
// the JS drag below is only a fallback for when the C one can't start.
import { View, addListener, wm, AppHost, appName, WindowShell, moveToDesktopItems, t as tr, solidOver, appAppearance, NativeTitleBar } from '@lordos/sdk';
import { useEffect, useRef } from 'react';
import { Menu } from './Menu.jsx';

const TITLE_H = 40;
const LIGHTS = [
  { key: 'close', color: '#ff5f57', glyph: '×', act: (id) => wm.close(id) },
  { key: 'min', color: '#febc2e', glyph: '−', act: (id) => wm.minimize(id) },
  { key: 'max', color: '#28c840', glyph: '+', act: (id) => wm.toggleMaximize(id) },
];

// leaving: the minimize effect is drawing it on its way to the Dock (natively, see index.jsx).

export function Window({ win, app, t, focused, leaving = false }) {
  const drag = useRef(null);
  const shared = appAppearance(t);
  const plan = win.app === 'files' && win.args?.nativeChrome ? win.args.nativeChrome : shared.chrome;
  const chrome = plan?.schema === 1 && Array.isArray(plan.buttons) ? plan : null;
  t = chrome ? { ...t, ...chrome.tokens } : t;
  const controls = chrome ? chrome.buttons.map((b) => ({ ...b, act: LIGHTS.find((l) => l.key === b.key)?.act })).filter((b) => b.act) : LIGHTS;
  const geo = wm.frame(win);
  const fxs = leaving ? { pointerEvents: 'none' } : null;

  useEffect(() => {
    const offMove = addListener('pointermove', (e) => {
      if (drag.current) wm.move(win.id, e.x - drag.current.dx, e.y - drag.current.dy);
    });
    const offUp = addListener('pointerup', () => { drag.current = null; });
    return () => { offMove(); offUp(); };
  }, [win.id]);

  // Linux app that draws its own frame (GTK, Chromium and Firefox with their own bar): only the
  // content, with the theme's shadow. A right click on its bar asks for the window menu
  // (w.windowMenu): the same items as the menu bar's Window menu.
  if (win.args?.csd || app.ownFrame) {
    const wmenu = win.windowMenu;
    return (
      <WindowShell win={win} leaving={leaving} onMouseDown={() => wm.focus(win.id)}
        style={{ ...(app.ownFrame ? { borderRadius: win.maximized ? 0 : 12, overflow: 'hidden' } : null), boxShadow: focused ? '0 24px 56px rgba(0,0,0,0.5)' : '0 10px 28px rgba(0,0,0,0.3)', ...fxs }}>
        <AppHost id={win.id} focused={focused} />
        {wmenu && (
          <Menu x={wmenu.x} y={wmenu.y} onClose={() => wm.closeWindowMenu(win.id)} items={[
            { label: tr('shell.window.minimize'), onClick: () => wm.minimize(win.id) },
            { label: tr('shell.window.maximize'), checked: !!win.maximized, onClick: () => wm.toggleMaximize(win.id) },
            { label: tr('shell.desktops.moveTo'), submenu: moveToDesktopItems(win.id) },
            '-',
            { label: tr('shell.window.close'), onClick: () => wm.close(win.id) },
          ]} />
        )}
      </WindowShell>
    );
  }

  return (
    <WindowShell win={win} leaving={leaving} onMouseDown={() => wm.focus(win.id)} style={{
      borderRadius: win.maximized ? 0 : chrome?.radius ?? 12, backgroundColor: app.transparent ? 'transparent' : t.windowBg, overflow: 'hidden',
      // a transparent app (the Terminal) has nothing behind its border: the border is drawn opaque
      border: `1px solid ${app.transparent ? solidOver(t.windowBorder, t.titleBg) : t.windowBorder}`,
      boxShadow: focused ? chrome?.shadowFocused || '0 24px 56px rgba(0,0,0,0.55)' : chrome?.shadowInactive || '0 10px 28px rgba(0,0,0,0.32)',
      ...fxs,
    }}>
      <NativeTitleBar id={win.id} fallbackTitle={appName(app) || ''} height={chrome?.header?.height ?? TITLE_H} chrome={chrome}
        controls={controls.map(({ key, color, glyph }) => ({ key, color, glyph }))}
        tokens={{ titleBg: t.titleBg, text: t.text, textDim: t.textDim, lightOff: t.lightOff }}
        onDrag={(x, y) => { if (!wm.beginMove(win.id)) drag.current = { dx: x - geo.x, dy: y - geo.y }; }}
        onMax={() => wm.toggleMaximize(win.id)} onAct={(key) => controls.find((c) => c.key === key)?.act(win.id)} />
      <View style={{ height: 1, backgroundColor: app.transparent ? solidOver(t.separator, t.titleBg) : t.separator }} />
      <View style={{ flex: 1 }}><AppHost id={win.id} focused={focused} /></View>
    </WindowShell>
  );
}
