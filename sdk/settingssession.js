import { createElement as h } from 'react';
import { NativeView } from './nativeview.js';
const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
const MANIFEST = { id: 'shell:settings', shell: true };
export function SettingsSession() {
  if (appHost || typeof lord === 'undefined' || typeof lord.appId === 'string' || typeof lord.greeter !== 'undefined') return null;
  return h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList,
    style: { position: 'absolute', left: 0, top: 0, width: 1, height: 1, pointerEvents: 'none' } });
}
