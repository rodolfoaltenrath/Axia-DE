// Top menu bar: the L menu, the active app and its menus, Window and Help (drawn in Rust by the native
// view host, sdk/nativemenubar.js), the keyboard layout, status items, network, Control Center and clock.
import { View, BackgroundApps, NetworkConnectionIcon, NetworkPopover, useClock, wm, getApp, session, useAppMenus, t as tr, appName, keyboard, useSettings, screen, windowAppName, spotlight, PrivacyIndicators, StatusItems, help, menuClock, appMenuItems, recentItems, storeUpdates, windowMenuItems, NativeView, NATIVE_MENUBAR, barBridge, NATIVE_TREE, treeOwners } from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';

// Date and time with the region's format (sdk/i18n.js),
// with the format chosen in Settings → Date & Time (sdk/hardware.jsx)
const format = (d) => menuClock(d);

function BarItem({ t, children, onClick, active, anchorRef }) {
  const [hover, setHover] = useState(false);
  return (
    <View ref={anchorRef} onClick={onClick} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{ height: 22, paddingHorizontal: 9, borderRadius: 5, justifyContent: 'center', flexDirection: 'row', alignItems: 'center',
        backgroundColor: active || hover ? t.menuHover : 'transparent' }}>
      {children}
    </View>
  );
}

// The bar's menus, each one a getter (see below), for the native bar (sdk/nativemenubar.js).
function barMenus({ app, focused, extra, onLaunchpad }) {
  // Only the open menu is built (each one a getter): the bar renders on every change of focus, of the
  // app's menus, of a setting and of the clock, and the logo menu walks the home folder for the recent
  // files (up to 60 000 entries) and asks for the pending package updates.
  const menus = {
    get logo() { return { items: [
      { label: tr('shell.menu.about'), onClick: () => wm.open('about') },
      '-',
      { label: tr('shell.menu.settings'), onClick: () => wm.open('settings') },
      ...(getApp('store') ? [(() => { const n = storeUpdates(); return { label: tr('shell.menu.store'), hint: n ? tr('shell.menu.storeUpdates', { count: n }) : undefined,
        onClick: () => wm.open('store', n ? { args: { page: 'updates' } } : undefined) }; })()] : []),
      { label: tr('shell.menu.apps'), hint: 'F4', onClick: onLaunchpad },
      '-',
      { label: tr('shell.menu.recent'), submenu: recentItems(10) },
      '-',
      ...(getApp('forcequit') ? [{ label: tr('shell.menu.forceQuit'), hint: '❖⌥Esc', onClick: () => wm.open('forcequit') }, '-'] : []),
      ...(session.managed() ? [
        { label: tr('shell.menu.lock'), hint: '❖L', onClick: () => session.lock() },
        { label: tr('shell.menu.switchUser'), onClick: () => session.switchUser() },
      ] : []),
      { label: tr('shell.menu.logout'), onClick: () => session.quit() },
      ...(session.canPower() ? [
        '-',
        { label: tr('shell.menu.restart'), onClick: () => session.restart() },
        { label: tr('shell.menu.shutdown'), onClick: () => session.shutdown() },
      ] : []),
    ] }; },
    // the app menu under the app's name (About, Settings…, Hide, Quit; sdk/appcontrol.js)
    get app() { return { items: app ? appMenuItems(focused) : [{ label: tr('shell.menu.about'), onClick: () => wm.open('about') }] }; },
    // Minimize, Zoom, Fill, Center, Move & Resize, Move to <display>, the open windows (sdk/windowmenu.js)
    get window() { return { items: windowMenuItems(app ? focused : null) }; },
    get help() { return { items: [
      ...(getApp('help') ? [{ label: tr('shell.help.open'), onClick: () => wm.open('help') }, { label: tr('shell.help.tour'), onClick: () => help.startTour() }, '-'] : []),
      { label: tr('shell.menu.about'), onClick: () => wm.open('about') },
      { label: tr('shell.help.theme'), disabled: true },
    ] }; },
  };
  extra.forEach((m, i) => { menus[`app${i}`] = { items: m.items }; });
  return menus;
}

// A bar item as a tree for the native host (shell:tree): the same box as BarItem.
const barNode = (key, t, children, { active, action } = {}) => ({ kind: 'view', key,
  style: { height: 22, paddingHorizontal: 9, borderRadius: 5, justifyContent: 'center', flexDirection: 'row', alignItems: 'center',
    backgroundColor: active ? t.menuHover : 'transparent' },
  hoverStyle: { backgroundColor: t.menuHover }, events: action ? { onClick: [action] } : undefined, children });
const barLabel = (t, label) => ({ kind: 'text', label, style: { fontSize: 13, fontWeight: 500, color: t.menuText, whiteSpace: 'nowrap' } });
const controlGlyph = (t) => {
  const bar = (key, top, knob) => ({ kind: 'view', key, style: { position: 'absolute', left: 0, top, width: 16, height: 6, borderRadius: 3, border: `1.5px solid ${t.menuText}` },
    children: [{ kind: 'view', key: 'k', style: { position: 'absolute', left: knob, top: 0.5, width: 2, height: 2, borderRadius: 1, backgroundColor: t.menuText } }] });
  return { kind: 'view', key: 'cc', style: { width: 16, height: 14 }, children: [bar('a', 0, 9), bar('b', 8, 2)] };
};
// A group of items of the bar drawn by Rust: what a click does is `onAction(name)`.
function NativeBarGroup({ owner, tree, onAction }) {
  treeOwners.set(owner, ({ action }) => onAction(action[0]));
  useEffect(() => () => treeOwners.delete(owner), [owner]);
  return <NativeView manifest={NATIVE_TREE} args={{ tree, owner }} style={{ flexGrow: 0, flexShrink: 0, flexBasis: 'auto' }} />;
}
// The keyboard layout, the search and the control and clock items: no service of theirs is React
function NativeBarTail({ t, controlCenter, onControlCenter, close }) {
  const clock = useClock(format, 1000);
  return <NativeBarGroup owner="bar-tail" onAction={() => { close(); spotlight.close(); onControlCenter(); }}
    tree={{ kind: 'view', key: 'tail', style: { flexDirection: 'row', alignItems: 'center' }, children: [
      barNode('cc', t, [controlGlyph(t)], { active: controlCenter, action: 'cc' }), barNode('clock', t, [barLabel(t, clock)])] }} />;
}

// The menus on the left are a native view (sdk/nativemenubar.js, native_view/menubar.rs); the
// items that other services drive stay React on the right.
function NativeMenuBar({ t, height, focused, onLaunchpad, onControlCenter, controlCenter, area }) {
  useSettings();         // the active keyboard layout lives in the settings
  const [network, setNetwork] = useState(false);
  const networkRef = useRef(null);
  const app = focused ? getApp(focused) : null;
  const extra = useAppMenus(focused) || [];
  barBridge.menus = barMenus({ app, focused, extra, onLaunchpad });
  barBridge.prepare = () => { if (controlCenter) onControlCenter(); spotlight.close(); setNetwork(false); };
  const close = () => setNetwork(false);
  const items = [
    { id: 'logo', glyph: 'logo' },
    { id: 'app', bold: true, label: app ? (app.builtin ? windowAppName(wm.get(focused)) : appName(app)) : 'Axia DE' },
    ...extra.map((m, i) => ({ id: `app${i}`, label: m.label })),
    { id: 'window', label: tr('shell.menu.window') },
    { id: 'help', label: tr('shell.menu.help') },
  ];
  return (
    <View style={{ position: 'absolute', left: 0, top: 0, width: 0, height: 0 }}>
      <View style={{
        position: 'absolute', left: area.x, top: area.y, width: area.w, height, flexDirection: 'row', alignItems: 'center',
        paddingHorizontal: 8, backgroundColor: t.menuBg, backdropFilter: `blur(${t.blur}px)`,
      }}>
        <NativeView manifest={NATIVE_MENUBAR} args={{ bar: { items, height } }} style={{ height }} />
        <NativeBarGroup owner="bar-keyboard" onAction={() => keyboard.next()}
          tree={barNode('kb', t, [barLabel(t, keyboard.label(keyboard.active()))], { action: 'kb' })} />
        <StatusItems color={t.menuText} />
        <BackgroundApps color={t.menuText} onOpen={() => { close(); if (controlCenter) onControlCenter(); spotlight.close(); }} />
        <PrivacyIndicators />
        <NativeBarGroup owner="bar-search" onAction={() => { close(); if (controlCenter) onControlCenter(); spotlight.toggle(); }}
          tree={barNode('search', t, [{ kind: 'searchGlyph', size: 18, color: t.menuText }], { action: 'search' })} />
        <BarItem t={t} active={network} anchorRef={networkRef} onClick={() => { if (controlCenter) onControlCenter(); spotlight.close(); setNetwork((o) => !o); }}><NetworkConnectionIcon color={t.menuText} /></BarItem>
        <NativeBarTail t={t} controlCenter={controlCenter} onControlCenter={onControlCenter} close={close} />
      </View>
      {network && <NetworkPopover anchor={networkRef} onClose={close} area={area} />}
    </View>
  );
}

// area: the display it is on (global coordinates; the main one, or each one with "menu bar on all
// displays"). The bar's menus are always drawn in Rust; the React bar is gone (the
// "menuBarNativeView" setting no longer chooses anything).
export function MenuBar(props) {
  return <NativeMenuBar {...props} area={props.area || { x: 0, y: 0, w: screen.width, h: screen.height }} />;
}
