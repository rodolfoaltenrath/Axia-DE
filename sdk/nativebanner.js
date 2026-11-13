// The notification banners drawn by the Rust view host (native_view/tree.rs, the
// "notificationsNativeView" setting): the shell builds each banner's tree from the notifications
// (sdk/notify.js keeps them, their timers and their actions) and Rust draws and routes the clicks.
import { createElement as h, useEffect } from 'react';
import { screen } from '@lordos/runtime';
import { useStore } from './store.js';
import { useTheme } from './theme.js';
import { direction, formatTime, appName } from './i18n.js';
import { getApp, workArea } from './windows.js';
import { settings, useSettings } from './settings.js';
import { useNotifications, dismissNotification, runAction, grouped, bannerPlacement, ReactNotificationBanner } from './notify.js';
import { NativeView } from './nativeview.js';
import { NATIVE_TREE, treeOwners } from './nativetree.js';

const text = (label, style) => ({ kind: 'text', label, style });
const view = (key, style, children, extra) => ({ kind: 'view', key, style, children: children.filter(Boolean), ...extra });

function bannerTree(n, th) {
  const app = n.app ? getApp(n.app) : null;
  const first = n.actions[0];
  const icon = app ? { kind: 'appIcon', app: n.app, size: 34 }       // an icon drawn by a JS component shows as the accent square
    : view('icon', { width: 34, height: 34, borderRadius: 8, backgroundColor: th.accent }, []);
  return view(String(n.id), { width: 340, padding: 12, gap: 8, borderRadius: 14, backgroundColor: th.windowBg, border: `1px solid ${th.separator}`,
    boxShadow: '0 10px 30px rgba(0,0,0,0.4)' }, [
    view('head', { flexDirection: 'row', gap: 10, alignItems: 'flex-start' }, [
      icon,
      view('body', { flex: 1, gap: 2 }, [
        view('line', { flexDirection: 'row', gap: 6 }, [
          text(`${app ? appName(app) : n.label}${n.more ? ` · +${n.more}` : ''}`, { flex: 1, fontSize: 11, color: th.textDim, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' }),
          text(formatTime(new Date(n.time)), { fontSize: 11, color: th.textDim })]),
        text(n.title, { fontSize: 13, fontWeight: 700, color: th.text, textAlign: 'start' }),
        n.body ? text(n.body, { fontSize: 12, color: th.text, lineHeight: 1.35, textAlign: 'start' }) : null]),
      view('close', { width: 18, height: 18, borderRadius: 9, alignItems: 'center', justifyContent: 'center', backgroundColor: 'transparent' },
        [text('×', { fontSize: 11, color: th.textDim })], { hoverStyle: { backgroundColor: th.control }, events: { onClick: ['dismiss', String(n.id)] } })]),
    n.actions.length ? view('actions', { flexDirection: 'row', gap: 8, justifyContent: 'flex-end' },
      n.actions.map((a, i) => view(`a${i}`, { height: 26, paddingHorizontal: 12, borderRadius: 7, justifyContent: 'center', backgroundColor: a.primary ? th.accent : th.control },
        [text(a.label, { fontSize: 12, fontWeight: 600, color: a.primary ? '#fff' : th.text })], { events: { onClick: ['act', `${n.id}:${i}`] } }))) : null,
  ], { events: { onClick: first ? ['act', `${n.id}:0`] : ['dismiss', String(n.id)] } });
}

function NativeBanners() {
  const list = useNotifications();
  const base = useTheme(), mode = useStore(settings, (x) => x.mode);
  const th = mode === 'light' ? { ...base, mode: 'light', windowBg: '#f6f6f8', text: '#1d1d1f', textDim: 'rgba(60,60,67,0.62)',
    separator: 'rgba(0,0,0,0.12)', control: '#e2e2e7' } : base;
  const shown = grouped(list);
  if (bannerPlacement() === 'bottom') shown.reverse();
  treeOwners.set('banners', ({ action }) => {
    const [cmd, key] = action;
    if (cmd === 'dismiss') { dismissNotification(isNaN(key) ? key : Number(key)); return; }
    const at = String(key).lastIndexOf(':');
    const id = String(key).slice(0, at), n = list.find((x) => String(x.id) === id) || shown.find((x) => String(x.id) === id);
    const a = n?.actions[Number(String(key).slice(at + 1))];
    if (n && a) runAction(n, a);
  });
  useEffect(() => () => treeOwners.delete('banners'), []);
  if (!list.length) return null;
  const area = workArea() || { x: 0, y: 0, w: screen.width, h: screen.height };
  const left = direction() === 'rtl' ? (area.x || 0) + 12 : (area.x || 0) + (area.w || screen.width) - 352;
  const at = bannerPlacement() === 'bottom'
    ? { bottom: Math.max(0, screen.height - ((area.y || 0) + (area.h || screen.height))) + 12 }
    : { top: (area.y || 0) + 10 };
  return h(NativeView, { manifest: NATIVE_TREE, args: { tree: view('banners', { width: 340, gap: 10 }, shown.map((n) => bannerTree(n, th))), owner: 'banners' },
    style: { position: 'absolute', left, width: 340, ...at } });
}

export function NotificationBanner() {
  const [prefs] = useSettings();
  return h(prefs.notificationsNativeView === true ? NativeBanners : ReactNotificationBanner);
}
