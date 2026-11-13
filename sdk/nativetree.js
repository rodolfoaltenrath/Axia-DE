// A widget tree the shell already has, drawn by the native view host (native_view/tree.rs): the
// Spotlight panel (the "spotlightNativeView" setting). The session stays in JS; the owner of the
// view publishes here what to do with an event, and sdk/nativeview.js hands the events over.
export const NATIVE_TREE = { id: 'shell:tree', shell: true, fullTheme: true };

// perform({ action: [cmd, key], extra }): what the user did in the view.
export const treeBridge = { perform: null };
