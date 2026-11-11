// Transitional shell transport. Rust owns defaults, schemes, profile fallback and native configuration.
import { settings, t } from '@lordos/sdk';
const model = lord.nativeApps.create('terminal');
const request = value => model.request(JSON.stringify(value));
const catalog = request({ cmd: 'catalog' });
// Entries preserve the original order shown in Preferences.
export const PRESETS = Object.fromEntries(catalog.presets);
export const DEFAULT_PROFILE = catalog.profile;
const raw = () => settings.get().terminal || {};
export const terminalSettings = () => {
  const source = raw();
  const value = request({ cmd: 'settings', settings: source });
  // Keep host dictionary identity/insertion order for imported color schemes
  // and custom shortcuts; JSON object key order is not carried by the model.
  for (const key of ['schemes', 'shortcuts']) if (Object.prototype.hasOwnProperty.call(source, key)) value[key] = source[key];
  return value;
};
export function saveTerminalSettings(patch) {
  settings.set({ terminal: { ...terminalSettings(), ...patch } });
}
export const getProfile = id => request({ cmd: 'profile', settings: raw(), id });
export const allSchemes = () => ({ ...PRESETS, ...(terminalSettings().schemes || {}) });
export const schemeOf = profile => request({ cmd: 'scheme', settings: raw(), profile });
export const toConfig = (profile, extra = {}) => request({ cmd: 'config', settings: raw(), profile, extra });

export function newProfileId() {
  return `p${Date.now().toString(36)}`;
}

// Nombre visible: el perfil de fábrica se traduce (también el de ajustes viejos, "Predeterminado").
export const profileName = (p) => (!p?.name || (p.id === 'default' && p.name === 'Predeterminado') ? t('terminal.profile.default') : p.name);
