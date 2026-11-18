// Snapshot/revision utilities shared by native-view mounts. Store values and
// fact blocks are immutable snapshots: unchanged sources never prepare or
// serialize another copy. No React or native runtime is needed by this module.
export const emptyFactsStore = Object.freeze({ get: () => null, subscribe: () => () => {} });
export function sameFacts(a, b) {
  if (Object.is(a, b)) return true;
  if (!a || !b || typeof a !== 'object' || typeof b !== 'object' || Array.isArray(a) !== Array.isArray(b)) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(k => Object.hasOwn(b, k) && sameFacts(a[k], b[k]));
}
export function sameRecord(a, b) {
  if (a === b) return true;
  if (!a || !b) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(k => Object.hasOwn(b, k) && Object.is(a[k], b[k]));
}
export function createFactsStore(sources, inputs, prepare) {
  let references, signature, value = null, ready = false, stops = [], queued = false, published;
  const listeners = new Set();
  const get = () => {
    const next = sources.map(source => source.get());
    if (references && next.every((v, i) => v === references[i])) return value;
    references = next;
    const selected = inputs();
    if (ready && sameFacts(signature, selected)) return value;
    signature = selected;
    const prepared = prepare();
    if (!ready || !sameFacts(value, prepared)) value = prepared;
    ready = true;
    return value;
  };
  const changed = () => {
    if (queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      if (!listeners.size) return;
      const next = get();
      if (next !== published) { published = next; for (const listener of listeners) listener(); }
    });
  };
  return { get, subscribe(fn) {
    listeners.add(fn);
    if (listeners.size === 1) { published = get(); stops = sources.map(source => source.subscribe(changed)); }
    return () => {
      listeners.delete(fn);
      if (!listeners.size) {
        for (const stop of stops) stop();
        stops = []; references = signature = published = undefined; value = null; ready = false;
      }
    };
  } };
}
// Shared catalog/service snapshots are encoded once across all their consumers.
const encoded = new WeakMap();
function stringify(value) {
  if (!value || typeof value !== 'object') return JSON.stringify(value);
  let text = encoded.get(value);
  if (text === undefined) { text = JSON.stringify(value); encoded.set(value, text); }
  return text;
}
export function createFactsEncoder() {
  const blocks = new Map();
  return {
    block(name, value) {
      const previous = blocks.get(name);
      if (previous && sameFacts(previous.value, value)) return previous.text;
      const block = { value, text: stringify(value), revision: (previous?.revision || 0) + 1 };
      blocks.set(name, block);
      return block.text;
    },
    revision(name) { return blocks.get(name)?.revision || 0; },
  };
}
const BASIC_THEME = ['text', 'textDim', 'accent', 'separator', 'mode', 'nativeStyle', 'windowBg', 'inputBg', 'control', 'controlRadius', 'controlRadiusSmall', 'buttonBorder', 'sidebarBg', 'hover', 'selection'];
const themes = new WeakMap();
export function nativeTheme(source, manifest) {
  const kind = manifest.themeCatalogs ? 'catalogs' : manifest.fullTheme ? 'full' : 'basic';
  let cache = themes.get(source);
  if (!cache) { cache = {}; themes.set(source, cache); }
  if (!cache[kind]) cache[kind] = kind === 'catalogs' ? { ...source } : kind === 'full'
    ? Object.fromEntries(Object.entries(source).filter(([key]) => key !== 'wallpapers' && key !== 'accents'))
    : Object.fromEntries(BASIC_THEME.map(key => [key, source[key]]));
  return cache[kind];
}
