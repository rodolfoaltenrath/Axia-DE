// Shared shell mounts for compiled Rust apps. Rust owns their widgets, input and
// icons; Files retains its own shared browser/dialog renderer until its next stage.
import { createElement as h } from 'react';
import { useAppWindow } from './components.jsx';
import { RustFiles, RustFileIcon, RustTransfers } from './rustfiles.js';
import { registerPrivacySection } from './privacy.js';
import { NativeView } from './nativeview.js';
import { useSettings } from './settings.js';

// The manifest of a native Files window: its facts (the installed apps, the settings of Files), all the
// theme tokens, the keyboard, and the browser mode.
const NATIVE_FILES = { id: 'files', nativeView: 'rust', facts: 'apps', settings: true, keyboard: true, fullTheme: true, defaultArgs: { mode: 'browser' }, size: [960, 600] };

export function nativeApp(manifest) {
  if (manifest.nativeView === 'files') {
    return { ...manifest, system: true, native: true,
      Component: function FilesHost() {
        const { args } = useAppWindow();
        const [prefs] = useSettings();
        if (args?.transferOwner) return h(RustTransfers, { owner: args.transferOwner });
        // Files drawn by the native view host, with no React (settings.json "filesNativeView": true; off by
        // default while it still lacks the media and PDF previews and dropping files into the window)
        if (prefs.filesNativeView === true && manifest.mode === 'browser') return h(NativeView, { manifest: NATIVE_FILES, args });
        return h(RustFiles, { mode: manifest.mode, args });
      },
      Icon: RustFileIcon };
  }
  if (manifest.nativeView !== 'rust') throw new Error('Unsupported compiled app view: ' + manifest.id);
  for (const section of manifest.privacy || []) registerPrivacySection({ id: section.id, order: section.order,
    Component: function NativePrivacySection() { return h(NativeView, { manifest, args: { section: section.section } }); } });
  return { ...manifest, system: true, native: true,
    Component: function NativeAppHost() { const { args } = useAppWindow(); return h(NativeView, { manifest, args }); },
    Icon: function NativeAppIcon({ size }) { return h(NativeView, { manifest, icon: size || 32 }); } };
}
