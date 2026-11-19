// Compatibility presentation for isolated LPK apps; never included in the session bundle.
import { createElement as h, useEffect, useState } from 'react';
import { View, Text, screen } from '@lordos/runtime';
import { useStore } from '../../sdk/store.js';
import { useTheme } from '../../sdk/theme.js';
import { direction, formatTime, appName } from '../../sdk/i18n.js';
import { getApp, workArea } from '../../sdk/windows.js';
import { settings } from '../../sdk/settings.js';
import { useNotifications, dismissNotification, runAction, grouped, bannerPlacement } from '../../sdk/notify.js';

function Banner({ n, th }) {
  const [hover, setHover] = useState(false);
  const app = n.app ? getApp(n.app) : null;
  const first = n.actions[0];
  return h(View, {
    onMouseEnter: () => setHover(true), onMouseLeave: () => setHover(false),
    onClick: () => (first ? runAction(n, first) : dismissNotification(n.id)),
    style: { width: 340, padding: 12, gap: 8, borderRadius: 14, backgroundColor: th.windowBg, border: `1px solid ${th.separator}`,
      boxShadow: '0 10px 30px rgba(0,0,0,0.4)' },
  },
  h(View, { style: { flexDirection: 'row', gap: 10, alignItems: 'flex-start' } },
    app?.Icon ? h(app.Icon, { size: 34 }) : n.Icon ? h(n.Icon, { size: 34 }) : h(View, { style: { width: 34, height: 34, borderRadius: 8, backgroundColor: th.accent } }),
    h(View, { style: { flex: 1, gap: 2 } },
      h(View, { style: { flexDirection: 'row', gap: 6 } },
        h(Text, { style: { flex: 1, fontSize: 11, color: th.textDim, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' } },
          `${app ? appName(app) : n.label}${n.more ? ` · +${n.more}` : ''}`),
        h(Text, { style: { fontSize: 11, color: th.textDim } }, formatTime(new Date(n.time)))),
      h(Text, { style: { fontSize: 13, fontWeight: 700, color: th.text, textAlign: 'start' } }, n.title),
      n.body ? h(Text, { style: { fontSize: 12, color: th.text, lineHeight: 1.35, textAlign: 'start' } }, n.body) : null),
    h(View, {
      onClick: (e) => { e.stopPropagation(); dismissNotification(n.id); },
      style: { width: 18, height: 18, borderRadius: 9, alignItems: 'center', justifyContent: 'center', backgroundColor: hover ? th.control : 'transparent' },
    }, h(Text, { style: { fontSize: 11, color: th.textDim } }, '×'))),
  n.actions.length ? h(View, { style: { flexDirection: 'row', gap: 8, justifyContent: 'flex-end' } },
    ...n.actions.map((a, i) => h(View, {
      key: a.id || i, onClick: (e) => { e.stopPropagation(); runAction(n, a); },
      style: { height: 26, paddingHorizontal: 12, borderRadius: 7, justifyContent: 'center', backgroundColor: a.primary ? th.accent : th.control },
    }, h(Text, { style: { fontSize: 12, fontWeight: 600, color: a.primary ? '#fff' : th.text } }, a.label)))) : null);
}

export function AppNotificationBanner() {
  const list = useNotifications();
  // drawn outside the theme's provider: follow the user's light/dark setting
  const base = useTheme(), mode = useStore(settings, (x) => x.mode);
  const th = mode === 'light' ? { ...base, mode: 'light', windowBg: '#f6f6f8', text: '#1d1d1f', textDim: 'rgba(60,60,67,0.62)',
    separator: 'rgba(0,0,0,0.12)', control: '#e2e2e7' } : base;
  const [, force] = useState(0);
  useEffect(() => { if (list.length) force((x) => x + 1); }, [list.length]);
  if (!list.length) return null;
  const area = workArea() || { x: 0, y: 0, w: screen.width, h: screen.height };
  const rtl = direction() === 'rtl';
  const left = rtl ? (area.x || 0) + 12 : (area.x || 0) + (area.w || screen.width) - 352;
  const at = bannerPlacement() === 'bottom'
    ? { bottom: Math.max(0, screen.height - ((area.y || 0) + (area.h || screen.height))) + 12 }
    : { top: (area.y || 0) + 10 };
  const shown = grouped(list);
  if (bannerPlacement() === 'bottom') shown.reverse();          // the newest one next to the taskbar
  return h(View, { style: { position: 'absolute', left, width: 340, gap: 10, ...at } },
    ...shown.map((n) => h(Banner, { key: n.id, n, th })));
}
