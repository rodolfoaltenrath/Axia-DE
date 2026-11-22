// The native notification view reads the context-owned service directly.
import { createElement as h } from 'react';
import { screen } from '@lordos/runtime';
import { workArea } from './windows.js';
import { AppNotificationBanner } from '../runtime/lpk/notifications.js';
import { NativeView } from './nativeview.js';

const MANIFEST = { id: 'shell:notifications', shell: true, appearanceSource: 'native', fullTheme: true };
function NativeBanners() {
  const area = workArea() || { x: 0, y: 0, w: screen.width, h: screen.height };
  return h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList, args: { area },
    style: { position: 'absolute', left: 0, top: 0, right: 0, bottom: 0, pointerEvents: 'box-none' } });
}
export function NotificationBanner() {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return h(AppNotificationBanner);
  return h(NativeBanners);
}
