// Transitional transport. Rust resolves physical shortcuts, precedence, menu symbols and PTY bytes.
import { createStore, t, settings } from '@lordos/sdk';
const model = lord.nativeApps.create('terminal');
const request = value => model.request(JSON.stringify(value));
export const DEFAULT_BINDINGS = Object.fromEntries(request({ cmd: 'keyCatalog' }).bindings);
const shortcuts = () => Object.entries(settings.get().terminal?.shortcuts || {});
const eventFacts = ({ code, key, meta, ctrl, alt, shift }) => ({ code, key, meta, ctrl, alt, shift });
export const comboOf = event => request({ cmd: 'combo', event: eventFacts(event) });
export const bindings = () => Object.fromEntries(request({ cmd: 'bindings', shortcuts: shortcuts() }));
export const actionOf = event => request({ cmd: 'action', event: eventFacts(event), shortcuts: shortcuts() });
export const keyMappingOf = (event, profile) => request({ cmd: 'keyMapping', event: eventFacts(event),
  profile: { keyMap: profile?.keyMap || [] }, keyMap: settings.get().terminal?.keyMap || [] });
export function mappingBytes(mapping) {
  const reply = request({ cmd: 'mappingBytes', mapping });
  if (!reply) return null;
  if (reply.error) throw new RangeError('Invalid code point');
  if (reply.kind === 'hex') return new Uint8Array(reply.bytes).buffer;
  // Build UTF-16 in bounded chunks; mappings may intentionally contain a lone surrogate.
  const parts = [];
  for (let i = 0; i < reply.units.length; i += 4096) parts.push(String.fromCharCode(...reply.units.slice(i, i + 4096)));
  return parts.join('');
}
export const keyCapture = createStore(null);
export const actionLabel = id => t(`terminal.action.${id}`);
export const ACTION_IDS = Object.keys(DEFAULT_BINDINGS);
export const comboLabel = value => request({ cmd: 'comboLabel', value });
export const hint = action => comboLabel(bindings()[action]?.[0] || '');
