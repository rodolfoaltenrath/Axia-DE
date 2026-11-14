// Window operations selected by the native application, using the shared WM.
export function windowServices(wm) {
  return { 'windows.open': { run: ([app, options]) => wm.open(app, options) } };
}
