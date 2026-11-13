// smartparse.js — smartctl's JSON (smartmontools, `smartctl -j`) turned into one model the Disks
// app and the Monitor can show (see docs/SYSTEM.md, "SMART with smartmontools"). The parsing is Rust
// (rust/axia-shell/src/launcher/smart_model.rs, reached through the launcher bridge); this adapter
// keeps the public API. No React; tests/smart_parse_test.mjs runs it under node with real smartctl output.
//
//   const m = parseSmartctl(json, exitStatus)
//   m.health            'ok' | 'warning' | 'failing' | 'unknown'   (the UI says OK / Check / Failing)
//   m.reasons           why it is not 'ok': ['reallocated', 'pending', 'smartFailed', 'nvmeSpare', …]
//   m.attrs             ATA attributes [{ id, name, key, value, worst, thresh, raw, prefail, failed }]
//   m.nvme              NVMe health log fields (or null)
//   m.selfTest          { supported, running, remaining, polling: { short, long } } (minutes)
//   m.selfTests         past results [{ type, result, passed, hours }]
//   m.errors            { count, list: [{ n, hours, text }] }
//   m.tempHistory       { interval (minutes), values: [°C | null] } (ATA SCT; null if none)
//
// Everything a drive sends is outside data: numbers are checked, strings are only shown as text.

const request = (cmd, input = {}) => {
  const reply = lord.launcher.request(JSON.stringify({ op: 'smart', cmd, ...input }));
  if (reply?.error) throw new Error(`smart: ${reply.error}`);
  return reply;
};

// The translation keys of smartctl's attribute names (disks.attr.<key>). This module is also loaded
// by processes without the launcher bridge (an app's), where the list is empty.
export const ATTR_KEYS = typeof lord !== 'undefined' && lord.launcher ? request('constants').attrKeys : [];

// "Unknown_SSD_Attribute" → "Unknown SSD Attribute"
export const prettyAttrName = (name) => request('pretty', { name });

// Objects come back with their fields in the order the API always gave them.
const wearOf = (w) => w && { used: w.used, source: w.source };
const summary = (s) => s && { health: s.health, reasons: s.reasons, temp: s.temp, wear: s.wear, hours: s.hours, standby: s.standby, supported: s.supported, t: s.t };

export function parseSmartctl(json, exit = 0) {
  const m = request('parse', { json, exit });
  m.wear = wearOf(m.wear);
  return m;
}

// health from what the drive says about itself; never guessed when there is no data
export const judge = (model) => request('judge', { model });

// The little summary the Monitor keeps: { health, reasons, temp, wear, hours, t }
export const summarize = (model, t = Date.now()) => summary(request('summarize', { model, t }));

// Summary from LordOS's own SMART reader (disk-smart: smart.c), used when smartmontools is missing.
export const summaryFromOwn = (own, t = Date.now()) => summary(request('summaryFromOwn', { own, t }));

// health order, to notice when a disk gets worse
export const HEALTH_RANK = { unknown: 0, ok: 1, warning: 2, failing: 3 };
