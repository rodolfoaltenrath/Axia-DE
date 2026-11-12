// Transitional transport for the Rust-owned category catalog and field templates.
import { t } from '@lordos/sdk';
import { GLYPH_KINDS } from './glyphs.jsx';
const model = lord.nativeApps.create('passwords');
const request = value => model.request(JSON.stringify(value));
const catalog = request({ cmd: 'catalog' });
export const BUILTIN = catalog.builtin;
export const SYSTEM_CATS = catalog.system;
export const FIELD_TYPES = catalog.types;
export const ICONS = GLYPH_KINDS;
export const COLORS = catalog.colors;
export const builtinById = id => BUILTIN.find(c => c.id === id) || SYSTEM_CATS.find(c => c.id === id);

// Visible name of a category (built-in or the user's)
export function catName(cat) {
  if (!cat) return '';
  if (cat.builtin || builtinById(cat.id) && !cat.name) return t(`passwords.cat.${cat.id}`);
  return cat.name || cat.id;
}

// A field's label: its own (added fields) or the template's
export function fieldLabel(f) {
  if (f.l) return f.l;
  const k = String(f.k || '');
  return /^c\d+$/.test(k) ? t('passwords.field.custom') : t(`passwords.field.${k}`);
}

// Category membership and template inheritance are evaluated by Rust.
export const allCategories = (users = []) => request({ cmd: 'categories', users });
export const templateFields = (category, categories) => request({ cmd: 'fields', category, categories });
const metadataOf = entries => entries.map(({ cat, trash, arch, fav, totp, tags }) => ({ cat, trash, arch, fav, totp, tags }));
export const summarizeEntries = (entries, categories) => request({ cmd: 'summary', entries: metadataOf(entries), categories });
export const filterEntries = (entries, selection, categories) => {
  // Only the lightweight list metadata reaches this model; values never do.
  const metadata = metadataOf(entries);
  return request({ cmd: 'filter', entries: metadata, selection, categories }).map(i => entries[i]);
};
