// The launcher drawn by the native view host (rust/axia-shell/src/native_view/launcher.rs, the
// "launcherNativeView" setting). Rust draws it and keeps its state; the shell side is what Rust
// can't do: opening an app, the Dock's drag and pins, settings and the session. rustlauncher.jsx
// publishes how to run an effect here on each render; sdk/nativeview.js hands the effects over.
export const NATIVE_LAUNCHER = { id: 'shell:launcher', shell: true, fullTheme: true, settings: true, facts: 'apps' };

// perform(effect, { handle, node, act }): runs one effect of the launcher.
export const launcherBridge = { perform: null };
