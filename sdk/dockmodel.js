// Dock policy lives in rust/axia-shell/src/launcher/dock_model.rs.
// This adapter carries callback results and preserves JS object/array identity.
const request = (cmd, input = {}) => {
  const reply = lord.launcher.request(JSON.stringify({ op: 'dock', cmd, ...input }));
  if (reply?.error) throw new Error(`dock: ${reply.error}`);
  return reply;
};
const constants = request('constants');
export const DOCK_KINDS = constants.kinds;
export const LAUNCHER = constants.launcher;
export const FIRST = constants.first;
export const LOCKED = constants.locked;
export const parseDockId = id => {
  // Preserve the public object's field order as well as its values.
  const value = request('parse', { id: typeof id === 'string' ? id : null });
  return value ? { kind: value.kind, key: value.key } : null;
};
export const dockId = (kind, key) => request('id', {
  kind: typeof kind === 'string' ? kind : null, key: typeof key === 'string' ? key : null,
});
export const isDockId = id => parseDockId(id) !== null;

// Only callbacks run here. Already typed (or malformed namespaced) IDs never
// invoke classify, matching the old API's callback contract.
const classification = (id, classify) => {
  const kind = typeof id === 'string' && id && !id.includes(':') ? classify(id) || 'app' : null;
  return typeof kind === 'string' ? kind : null;
};
export const toTypedId = (id, classify = () => null) => request('typed', {
  id: typeof id === 'string' ? id : null, kind: classification(id, classify),
});
export const normalizePinned = (list, classify = () => null) => {
  const value = request('normalize', {
    list: Array.isArray(list) ? list.map(id => typeof id === 'string' ? id : null) : list == null ? null : false,
    kinds: Array.isArray(list) ? list.map(id => classification(id, classify)) : [],
  });
  return { list: value.list, changed: value.changed };
};
const keepIdentity = (list, next) => next.length === list.length && next.every((id, i) => id === list[i]) ? list : next;
const indexValue = index => { const n = +index; return index !== null && Number.isFinite(n) ? n : null; };
function place(cmd, list, id, index, isShown) {
  // Early exits keep callbacks lazy; policy remains validated by the Rust call.
  const exists = list.includes(id);
  const movable = request('movable', { id: typeof id === 'string' ? id : null });
  const valid = cmd !== 'pin' || isDockId(id);
  const active = valid && (cmd === 'pin' ? !exists || movable && index != null : exists && movable);
  if (!active) return list;
  const remaining = active ? list.filter(x => x !== id) : [];
  const shown = active ? remaining.filter(isShown) : [];
  // Unary + is a JS coercion boundary (strings/bools/NaN); rounding is Rust's.
  const value = active ? indexValue(index) : null;
  return keepIdentity(list, request(cmd, { list, id, index: value, omitted: index == null, shown }));
}
export const moveItem = (list, id, index, isShown = () => true) => place('move', list, id, index, isShown);
export const pinItem = (list, id, index, isShown = () => true) => place('pin', list, id, index, isShown);
export const isLocked = (id, locked = LOCKED) => request('locked', { id, locked });
export const isMovable = id => request('movable', { id: typeof id === 'string' ? id : null });
export const unpinItem = (list, id, locked = LOCKED) => keepIdentity(list, request('unpin', { list, id, locked }));
export const ensureItems = (list, required = LOCKED, defaults = []) => keepIdentity(list, request('ensure', { list, required, defaults }));

// JSON drops undefined/functions and turns NaN/Infinity into null. Send explicit
// invalid markers for unsupported values; retain accepted original values in JS.
function actionArgs(value) {
  if (value == null) return {};
  if (typeof value !== 'object' || Array.isArray(value)) return false;
  return Object.fromEntries(Object.entries(value).map(([key, v]) => [key,
    v === null || ['string', 'number', 'boolean'].includes(typeof v) ? v : { invalid: true }]));
}
export function cleanActions(list, max = 40) {
  if (!Array.isArray(list)) return [];
  const identities = new Map();
  const candidates = list.map((a, i) => {
    if (!a || typeof a !== 'object') return {};
    const id = a.id;
    // Strict equality distinguishes object IDs; NaN never equals itself.
    if (!identities.has(id) || Number.isNaN(id)) identities.set(id, i);
    return { id: String(id ?? ''), identity: identities.get(id), label: typeof a.label === 'string' ? a.label : '', args: actionArgs(a.args) };
  });
  const numericMax = +max;
  return request('actions', { candidates, max: numericMax === -Infinity ? -1 : Number.isFinite(numericMax) ? numericMax : null }).map(i => {
    const a = list[i];
    const args = {};
    for (const [key, value] of Object.entries(a.args ?? {})) args[key] = value;
    return { id: a.id, label: a.label, args, new: !!a.new };
  });
}
