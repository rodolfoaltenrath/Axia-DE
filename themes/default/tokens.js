// Shell appearance is resolved in Rust; the wallpaper catalog stays shared with apps.
// Rust owns the metadata; the shell adapter obtains one compatibility snapshot.
export const wallpapers = lord.appearance.request('{"op":"shellCatalog"}');

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
