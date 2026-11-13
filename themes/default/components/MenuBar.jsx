// Top menu bar: L menu, active app, Window menu, keyboard layout,
// Control Center and clock.
import { View, Text, BackgroundApps, NetworkConnectionIcon, NetworkPopover, SearchGlyph, useClock, wm, getApp, session, useAppMenus, t as tr, appName, formatDate, formatTime, keyboard, useSettings, direction, screen, windowAppName, AxiaGlyph, spotlight, PrivacyIndicators, StatusItems, help, menuClock, appMenuItems, recentItems, storeUpdates, windowMenuItems, NativeView, NATIVE_MENUBAR, barBridge, NATIVE_TREE, treeOwners } from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';
import { Menu } from './Menu.jsx';

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

const Label = ({ t, children, bold }) => (
  <Text style={{ fontSize: 13, fontWeight: bold ? 700 : 500, color: t.menuText, whiteSpace: 'nowrap' }}>{children}</Text>
);

function ControlGlyph({ t }) {
  const bar = (top, knob) => (
    <View style={{ position: 'absolute', left: 0, top, width: 16, height: 6, borderRadius: 3, border: `1.5px solid ${t.menuText}` }}>
      <View style={{ position: 'absolute', left: knob, top: 0.5, width: 2, height: 2, borderRadius: 1, backgroundColor: t.menuText }} />
    </View>
  );
  return <View style={{ width: 16, height: 14 }}>{bar(0, 9)}{bar(8, 2)}</View>;
}

// The bar's menus, each one a getter (see below), shared by the React bar and the native one.
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

// area: the display it is on (global coordinates; the main one, or each one with "menu bar on all displays")
function ClassicMenuBar({ t, height, focused, onLaunchpad, onControlCenter, controlCenter, area = { x: 0, y: 0, w: screen.width, h: screen.height } }) {
  const clock = useClock(format, 1000);
  useSettings();         // the active keyboard layout lives in the settings
  const [open, setOpen] = useState(null);
  const refs = { logo: useRef(null), app: useRef(null), window: useRef(null), help: useRef(null), network: useRef(null) };
  const appRefs = useRef({});
  const app = focused ? getApp(focused) : null;
  const extra = useAppMenus(focused) || [];
  // Menu position: below the item that opened it (from its right edge in Arabic/Hebrew).
  const anchorX = (ref) => {
    if (!ref?.current) return direction() === 'rtl' ? area.x + area.w - 6 : area.x + 6;
    const [x, , w] = lord.getLayout(ref.current.id);
    return direction() === 'rtl' ? x + w : x;
  };
  const toggle = (name, ref) => { if (controlCenter) onControlCenter(); spotlight.close(); setOpen((o) => (o?.name === name ? null : { name, x: anchorX(ref) })); };
  const close = () => setOpen(null);

  const menus = barMenus({ app, focused, extra, onLaunchpad });

  const cur = open && menus[open.name];
  const anchor = open && (refs[open.name] || { current: appRefs.current[open.name] });
  return (
    <View style={{ position: 'absolute', left: 0, top: 0, width: 0, height: 0 }}>
      <View style={{
        position: 'absolute', left: area.x, top: area.y, width: area.w, height, flexDirection: 'row', alignItems: 'center',
        paddingHorizontal: 8, backgroundColor: t.menuBg, backdropFilter: `blur(${t.blur}px)`,
      }}>
        <BarItem t={t} active={open?.name === 'logo'} anchorRef={refs.logo} onClick={() => toggle('logo', refs.logo)}>
          <AxiaGlyph size={16} color={t.menuText} />
        </BarItem>
        <BarItem t={t} active={open?.name === 'app'} anchorRef={refs.app} onClick={() => toggle('app', refs.app)}><Label t={t} bold>{app ? (app.builtin ? windowAppName(wm.get(focused)) : appName(app)) : 'Axia DE'}</Label></BarItem>
        {extra.map((m, i) => {
          const key = `app${i}`;
          const ref = { get current() { return appRefs.current[key]; } };
          return (
            <BarItem key={key} t={t} active={open?.name === key} anchorRef={(inst) => { appRefs.current[key] = inst; }}
              onClick={() => toggle(key, ref)}><Label t={t}>{m.label}</Label></BarItem>
          );
        })}
        <BarItem t={t} active={open?.name === 'window'} anchorRef={refs.window} onClick={() => toggle('window', refs.window)}><Label t={t}>{tr('shell.menu.window')}</Label></BarItem>
        <BarItem t={t} active={open?.name === 'help'} anchorRef={refs.help} onClick={() => toggle('help', refs.help)}><Label t={t}>{tr('shell.menu.help')}</Label></BarItem>
        <View style={{ flex: 1 }} />
        <BarItem t={t} onClick={() => keyboard.next()}>
          <Label t={t}>{keyboard.label(keyboard.active())}</Label>
        </BarItem>
        <StatusItems color={t.menuText} />
        <BackgroundApps color={t.menuText} onOpen={() => { close(); if (controlCenter) onControlCenter(); spotlight.close(); }} />
        <PrivacyIndicators />
        <BarItem t={t} onClick={() => { close(); if (controlCenter) onControlCenter(); spotlight.toggle(); }}><SearchGlyph size={18} color={t.menuText} /></BarItem>
        <BarItem t={t} active={open?.name === 'network'} anchorRef={refs.network} onClick={() => toggle('network', refs.network)}><NetworkConnectionIcon color={t.menuText} /></BarItem>
        <BarItem t={t} active={controlCenter} onClick={() => { close(); spotlight.close(); onControlCenter(); }}><ControlGlyph t={t} /></BarItem>
        <BarItem t={t}><Label t={t}>{clock}</Label></BarItem>
      </View>
      {open?.name === 'network' && <NetworkPopover anchor={refs.network} onClose={close} area={area} />}
      {cur && <Menu x={open.x} y={area.y + height + 2} items={cur.items} onClose={close} anchor={anchor} width={260} />}
    </View>
  );
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

// The clock has its own component: a tick re-renders this, not the bar.
function BarClock({ t }) {
  const clock = useClock(format, 1000);
  return <BarItem t={t}><Label t={t}>{clock}</Label></BarItem>;
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

// "menuBarNativeView": true in settings.json draws the menus in Rust (off by default for now).
export function MenuBar(props) {
  const [prefs] = useSettings();
  const Bar = prefs.menuBarNativeView === true ? NativeMenuBar : ClassicMenuBar;
  return <Bar {...props} area={props.area || { x: 0, y: 0, w: screen.width, h: screen.height }} />;
}
