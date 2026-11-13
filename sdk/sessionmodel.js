// sessionmodel.js — session restore as plain data, with no runtime or React (so it can be unit tested
// with node: server/tests/session_model_test.mjs). The runtime part is sdk/sessionrestore.jsx.
//
// settings.json keeps what was open as a list of entries:
//   { id: <typed Dock id>, f: { x, y, w, h }, max?: 1, min?: 1, snap?: <zone>, desk?: n, args?: {…} }
// (desk: the window's position in the list of desktops, left out for the first).
// Nothing else ever enters it: no Exec line, argv, environment, pid, title or Linux document path.
// args exist only for LordOS apps, only the fields below, and never "nonce" (wm.open adds a new one).
// The file can be edited by any program the user runs, so all of it is checked again when it is read.
// The rules are Rust (rust/axia-shell/src/launcher/session_model.rs, through the launcher bridge); this
// adapter keeps the public API and resolves the callbacks (once, area) into plain data.
const request = (cmd, input = {}) => {
  const reply = lord.launcher.request(JSON.stringify({ op: 'session', cmd, ...input }));
  if (reply?.error) throw new Error(`session: ${reply.error}`);
  return reply;
};
const constants = request('constants');

export const RESTORE_MODES = constants.modes;
export const RESTORE_DEFAULT = constants.default;
export const MAX_WINDOWS = constants.max;      // a session never launches more than this

// Never reopened: tools about another window, one-shot helpers and apps of the live system.
export const NEVER = constants.never;

// Objects come back with sorted keys; these rebuild them in the order the API always gave.
const frame = (f) => f && { x: f.x, y: f.y, w: f.w, h: f.h };
const entry = (e) => {
  if (!e) return null;
  const out = { id: e.id, f: frame(e.f) };
  if (e.max) out.max = e.max;
  if (e.min) out.min = e.min;
  if (e.snap !== undefined) out.snap = e.snap;
  if (e.desk !== undefined) out.desk = e.desk;
  if (e.args) out.args = Object.fromEntries(e.args);
  return out;
};

// Only JSON-able values can be saved, so anything else (functions, undefined) is not an arg at all.
const plain = (v) => { try { return JSON.parse(JSON.stringify(v)) ?? null; } catch { return null; } };

export function cleanArgs(app, args) {
  const pairs = request('cleanArgs', { app: typeof app === 'string' ? app : '', args: plain(args) });
  return pairs && Object.fromEntries(pairs);
}

export const cleanFrame = (f) => frame(request('cleanFrame', { f: plain(f) }));

// One entry as read back from settings.json (null if anything is off). zones: the snap zones.
export const cleanEntry = (e, zones = []) => entry(request('cleanEntry', { e: plain(e), zones: plain(zones) }));
export const cleanList = (list, zones) => request('cleanList', { list: plain(list), zones: plain(zones) || [] }).map(entry);

// What a new session does with what the last one left. An unclean end (power cut, crash) always
// asks first, unless restore is off: the same apps may be why it fell.
export const restoreDecision = (mode, clean, count) => request('restoreDecision', {
  mode: typeof mode === 'string' ? mode : '', clean: !!clean, count: typeof count === 'number' ? count : (count ? 1 : 0),
});

// The launches of a saved list, in order (bottom window first). open: Map dock id → windows already
// open (Open at Login, the Terminal's live sessions): that many entries are skipped, nothing doubles.
// once(id): ids that bring back all their windows from a single launch (isolated apps, .lpk packages,
// a browser reopening its own session); the frames of the others still go to the windows that show up.
export function planRestore(list, open, once = () => false) {
  const ids = list.map((e) => e.id);
  const steps = request('planRestore', {
    ids, open: [...open].map(([id, n]) => [id, n]), once: [...new Set(ids)].filter((id) => once(id)),
  });
  return steps.map((s) => ({ entry: list[s.index], launch: s.launch }));
}

// A saved frame on today's screens: kept on the display it was on, or centered on the main one when
// that display is gone. displays: [{ x, y, w, h }], area(d): that display's work area; main: index.
export function fitFrame(f, displays, area, main = 0) {
  const r = request('fitFrame', {
    f: frame(f), displays, areas: displays.map((d) => area(d)), main,
    fallbackArea: displays.length ? null : area(undefined),
  });
  return frame(r);
}
