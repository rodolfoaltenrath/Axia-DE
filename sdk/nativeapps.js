// Shared shell mounts for compiled Rust apps. Rust owns their widgets, input and
// icons. Shared Files dialogs and transfers migrate separately from the browser.
import { createElement as h } from 'react';
import { useAppWindow } from './components.jsx';
import { RustTransfers } from './rustfiles.js';
import { registerPrivacySection } from './privacy.js';
import { NativeView } from './nativeview.js';
import { registerDockActions } from './dock.js';
import { standaloneDesktop } from './platform.js';

// The manifest of a native Files window: its facts (the installed apps, the settings of Files), all the
// theme tokens, the keyboard, and the browser mode.
const NATIVE_FILES = { id: 'files', nativeView: 'rust', facts: 'apps', settings: true, keyboard: true, fullTheme: true, defaultArgs: { mode: 'browser' }, size: [960, 600] };

export function nativeApp(manifest) {
  if (manifest.dockActionsModel) {
    const model=lord.nativeApps.create(manifest.id);
    registerDockActions(manifest.id, () => {
      const services={};
      for (const [service, methods] of Object.entries(manifest.dockServices || {})) {
        services[service]={};
        for (const method of methods) { try { services[service][method]=lord[service]?.[method]?.() ?? null; } catch (_) { services[service][method]=null; } }
      }
      return model.request(JSON.stringify({ cmd: manifest.dockActionsModel, services, standalone: standaloneDesktop }));
    });
  }
  if (manifest.nativeView === 'files') {
    return { ...manifest, system: true, native: true,
      Component: function FilesHost() {
        const { args } = useAppWindow();
        if (args?.transferOwner) return h(RustTransfers, { owner: args.transferOwner });
        return h(NativeView, { manifest: NATIVE_FILES, args });
      },
      Icon: function FilesIcon({ size }) { return h(NativeView, { manifest: NATIVE_FILES, icon: size || 32 }); } };
  }
  if (manifest.nativeView !== 'rust') throw new Error('Unsupported compiled app view: ' + manifest.id);
  for (const section of manifest.privacy || []) registerPrivacySection({ id: section.id, order: section.order,
    Component: function NativePrivacySection() { return h(NativeView, { manifest, args: { section: section.section } }); } });
  return { ...manifest, system: true, native: true,
    Component: function NativeAppHost() { const { args } = useAppWindow(); return h(NativeView, { manifest, args }); },
    Icon: function NativeAppIcon({ size }) { return h(NativeView, { manifest, icon: size || 32 }); } };
}
