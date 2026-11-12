// Transitional transport: Rust derives the hardware sections, GPU layers, compatibility and device metadata.
const model = lord.nativeApps.create('sysinfo');
const request = (cmd, args) => model.request(JSON.stringify({ cmd, args }));
export const compositionReason = (...args) => request('compositionReason', args);
export const cardLayers = (...args) => request('cardLayers', args);
export const sessionLayers = (...args) => request('sessionLayers', args);
export const compatibility = (...args) => request('compatibility', args);
export const hardwareSections = (...args) => request('hardwareSections', args);
export const piBoard = (...args) => request('piBoard', args);
export const throttleFlags = (...args) => request('throttleFlags', args);
export const piStatus = (...args) => request('piStatus', args);
export const machineName = (...args) => request('machineName', args);
export const parseMemoryLines = (...args) => request('parseMemoryLines', args);
export const parseSerialLines = (...args) => request('parseSerialLines', args);
export const peripherals = (...args) => request('peripherals', args);

// A firmware date as a Date: DMI's mm/dd/yyyy or the report's yyyy-mm-dd; null for anything else.
export function firmwareDate(s) {
  const us = /^(\d{2})\/(\d{2})\/(\d{4})$/.exec(s || ''), iso = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s || '');
  if (!us && !iso) return null;
  const [y, m, d] = us ? [+us[3], +us[1], +us[2]] : [+iso[1], +iso[2], +iso[3]];
  const dt = new Date(y, m - 1, d, 12);
  return dt.getMonth() === m - 1 && dt.getDate() === d ? dt : null;
}


// The Report tab's file name: lordos-report-<date>.<ext>, or -2, -3… when one with that name exists.
export function reportFileName(date, ext, exists) {
  const base = `lordos-report-${/^\d{4}-\d{2}-\d{2}$/.test(date || '') ? date : 'today'}`;
  for (let i = 1; i < 100; i++) {
    const name = `${base}${i > 1 ? `-${i}` : ''}.${ext}`;
    if (!exists(name)) return name;
  }
  return null;
}

// The JSON a report is saved as: the collector's fields as they are, two-space indent.
export const reportJson = (r) => `${JSON.stringify(r, null, 2)}\n`;
