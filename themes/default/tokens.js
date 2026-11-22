// Shell appearance is resolved in Rust; the wallpaper catalog stays shared with apps.
import { lordosWallpapers } from '@lordos/sdk';

// The desktop collection (Horizons, Cosmos; sdk/wallpapers.js) and this theme's own
// gradients (tools/gen-wallpaper.py). name: English fallback; the shown name is the translation of
// nameKey (assets/locales/*/shell.json). Thumbnails: assets/thumbs/ (320×180).
const gradients = [
  { id: 'aurora', category: 'gradients', nameKey: 'shell.wallpaper.aurora', name: 'Aurora', src: 'assets/aurora.png', thumb: 'assets/thumbs/aurora.jpg' },
  { id: 'ocean', category: 'gradients', nameKey: 'shell.wallpaper.ocean', name: 'Ocean', src: 'assets/ocean.png', thumb: 'assets/thumbs/ocean.jpg' },
  { id: 'sunset', category: 'gradients', nameKey: 'shell.wallpaper.sunset', name: 'Sunset', src: 'assets/sunset.png', thumb: 'assets/thumbs/sunset.jpg' },
  { id: 'graphite', category: 'gradients', nameKey: 'shell.wallpaper.graphite', name: 'Graphite', src: 'assets/graphite.png', thumb: 'assets/thumbs/graphite.jpg' },
];
export const wallpapers = [...lordosWallpapers, ...gradients];

// Palette and materials belong to Rust; this adapter attaches the shared catalog.
// Only the three appearance inputs cross the bridge, once per change.
let previous = null, cached = null;
export function makeTheme({ mode, accent, effects }) {
  if (previous && previous.mode === mode && previous.accent === accent && previous.effects === effects) return cached;
  const plan = lord.appearance.request(JSON.stringify({ op: 'shellTheme', mode, accent, effects }));
  if (!plan || !Array.isArray(plan.accents)) throw new Error('Invalid native shell appearance');
  cached = { ...plan, accents: Object.fromEntries(plan.accents), wallpapers };
  previous = { mode, accent, effects };
  return cached;
}
