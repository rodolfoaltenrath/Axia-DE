// A widget tree the shell already has (or builds), drawn by the native view host
// (native_view/tree.rs): the Spotlight panel ("spotlightNativeView") and the notification banners
// ("notificationsNativeView"). The owner of a view registers what to do with its events under the
// name it passes in args.owner; sdk/nativeview.js hands each event to its owner.
export const NATIVE_TREE = { id: 'shell:tree', shell: true, fullTheme: true };

// owner name → perform({ action: [cmd, key], extra }): what the user did in the view.
export const treeOwners = new Map();
