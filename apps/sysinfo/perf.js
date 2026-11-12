// Transitional transport: Rust interprets benchmark results and owns thresholds and categories.
const model = lord.nativeApps.create('sysinfo');
const request = (cmd, args = []) => model.request(JSON.stringify({ cmd, args }));
const catalog = request('perfCatalog');
export const CATEGORIES = catalog.categories;
export const DESKTOP_CATEGORIES = catalog.desktop;
export const SANDBOX_TRIES = catalog.sandbox;
export const LANDLOCK_TRIES = catalog.landlock;
export const PI_FIRMWARE_EVENTS = catalog.events;
export const ENGINE_IDS = catalog.engine;
export const REFERENCES = catalog.references;
export const MISSED_WARN = catalog.missedWarn;
export const MISSED_FAIL = catalog.missedFail;
export const INPUT_WARN_FRAMES = catalog.inputWarn;
export const ratio = (res, ref, id) => request('ratio', [
  { lordos: res?.lordos, results: { [id]: res?.results?.[id] } },
  { results: { [id]: ref?.results?.[id] } }, id,
]);
export const categoryRatios = (...args) => request('categoryRatios', args);
export const comparableReferences = (...args) => request('comparableReferences', args);
export const piFirmware = (...args) => request('piFirmware', args);
export const stability = (...args) => request('stability', args);
export const compatibilityVerdict = (...args) => request('compatibilityVerdict', args);
export const desktopVerdict = (...args) => request('desktopVerdict', args);
export const conditionDiffs = (...args) => request('conditionDiffs', args);
export const changes = (...args) => request('changes', args);
export const sustainedSeries = (...args) => request('sustainedSeries', args);
export const refreshBudget = (...args) => request('refreshBudget', args);

// Local calendar dates and filesystem collision callbacks belong to the host.
export function fileName(at, taken) {
  const d = new Date((at || 0) * 1000);
  const p = (n) => String(n).padStart(2, '0');
  const base = `lordbench-${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  for (let i = 1; i < 100; i++) {
    const name = i === 1 ? `${base}.json` : `${base}-${i}.json`;
    if (!taken(name)) return name;
  }
  return null;
}
