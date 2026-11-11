// Compiled Rust apps: native scene views, or the Files compatibility host.
// No React widget renderer is used for Calculator, Force Quit, About or Monitor.
import { createElement as h } from 'react';
import { useAppWindow } from './components.jsx';
import { RustFiles, RustFileIcon } from './rustfiles.js';
import { registerPrivacySection } from './privacy.js';
import { NativeView } from './nativeview.js';

export function nativeApp(manifest) {
  if (manifest.nativeView === 'files') {
    return { ...manifest, system: true, native: true,
      Component: function FilesHost() { const { args } = useAppWindow(); return h(RustFiles, { mode: manifest.mode, args }); },
      Icon: RustFileIcon };
  }
  if (manifest.nativeView !== 'rust') throw new Error('Unsupported compiled app view: ' + manifest.id);
  for (const section of manifest.privacy || []) registerPrivacySection({ id: section.id, order: section.order,
    Component: function NativePrivacySection() { return h(NativeView, { manifest, args: { section: section.section } }); } });
  return { ...manifest, system: true, native: true,
    Component: function NativeAppHost() { const { args } = useAppWindow(); return h(NativeView, { manifest, args }); },
    Icon: function NativeAppIcon({ size }) { return h(NativeView, { manifest, icon: size || 32 }); } };
}
