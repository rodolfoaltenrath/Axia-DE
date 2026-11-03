// Keyboard conventions are independent of the visual theme.
export function shortcutProfile(value, theme = 'default') {
  return value === 'macos' || value === 'linux' ? value : theme === 'classic' ? 'linux' : 'macos';
}

const MAC = {
  switchApps: ['Meta+Tab', 'Meta+Shift+Tab'],
  cycleWindows: ['Meta+`', 'Meta+Shift+`'],
  minimize: ['Meta+M'], hide: ['Meta+H'], hideOthers: ['Meta+Alt+H'],
  closeWindow: [], showDesktop: ['Meta+F11'], files: [], settings: [], monitor: [], snap: [],
  quick: [], notif: [], taskbar: [], captureArea: [],
  lock: ['Meta+Ctrl+Q'], quit: ['Meta+Q'],
};
const LINUX = {
  switchApps: ['Alt+Tab', 'Alt+Shift+Tab'],
  cycleWindows: ['Alt+`', 'Alt+Shift+`'],
  minimize: ['Meta+H'], hide: [], hideOthers: [],
  closeWindow: ['Alt+F4'], showDesktop: ['Meta+D'],
  files: ['Meta+E'], settings: ['Meta+I'], monitor: ['Ctrl+Shift+Escape'],
  snap: ['Meta+ArrowLeft', 'Meta+ArrowRight', 'Meta+ArrowUp', 'Meta+ArrowDown'],
  dockAutohide: [], forceQuit: [], appSettings: [], captureArea: [],
  'capture.full': ['PrintScreen'], 'capture.area': ['Meta+Shift+S'], 'capture.panel': [],
  searchCtrl: [], lock: ['Meta+L'], quit: [],
};

export function profileEntry(entry, profile) {
  const keys = (profile === 'linux' ? LINUX : MAC)[entry.id];
  return keys ? { ...entry, keys, show: undefined } : entry;
}
