// The native notification view reads the context-owned service directly.
import { createElement as h } from 'react';
import { screen } from '@lordos/runtime';
import { useStore } from './store.js';
import { useLocale } from './i18n.js';
import { workArea } from './windows.js';
import { notificationPlacement } from './notify.js';
import { AppNotificationBanner } from '../runtime/lpk/notifications.js';
import { NativeView } from './nativeview.js';

const MANIFEST = { id: 'shell:notifications', shell: true, fullTheme: true };
function NativeBanners() {
  const { dir } = useLocale();
  const placement = useStore(notificationPlacement);
  const area = workArea() || { x: 0, y: 0, w: screen.width, h: screen.height };
  const left = dir === 'rtl' ? (area.x || 0) + 12 : (area.x || 0) + (area.w || screen.width) - 352;
  const at = placement === 'bottom'
    ? { bottom: Math.max(0, screen.height - ((area.y || 0) + (area.h || screen.height))) + 12 }
    : { top: (area.y || 0) + 10 };
  return h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList,
    style: { position: 'absolute', left, width: 340, ...at } });
}
export function NotificationBanner() {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return h(AppNotificationBanner);
  return h(NativeBanners);
}
