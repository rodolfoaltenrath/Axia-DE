// The window identity binds a context menu to the native window owner.
import { createElement as h } from 'react';
import { NativeView } from './nativeview.js';

const MANIFEST = { id: 'shell:window-context', shell: true, appearanceSource: 'native', fullTheme: true };
export function NativeWindowMenu({ id }) {
  return h(NativeView, { manifest: MANIFEST, stateSource: lord.windowList, args: { id },
    style: { position: 'absolute', left: 0, top: 0, width: 0, height: 0 } });
}
