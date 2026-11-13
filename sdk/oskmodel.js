// oskmodel.js — the on-screen keyboard's logic with nothing on screen (server/tests/osk_model_test.mjs).
// The logic is Rust (rust/axia-shell/src/launcher/osk_model.rs, reached through the launcher bridge);
// this adapter keeps the public API and the order of the fields of the objects it returns.
//
// The layout is not data of our own: rows are lists of Linux key codes, and what each key shows comes
// from the active XKB layout (lord.input.keyLabels, rust/axia-shell/src/keyboard), so the keyboard follows it
// like a physical one. `labels` = { altgr, keys: { code: [plain, shift, altgr, altgr + shift] } }.
// A key here is { code, level, label } ready for lord.input.tap(code, level); level 1 is Shift, 2 AltGr.

const request = (cmd, input = {}) => {
  const reply = lord.launcher.request(JSON.stringify({ op: 'osk', cmd, ...input }));
  if (reply?.error) throw new Error(`osk: ${reply.error}`);
  return reply;
};

export const KEY = { BACKSPACE: 14, TAB: 15, ENTER: 28, ESC: 1, SPACE: 57, UP: 103, LEFT: 105, RIGHT: 106, DOWN: 108, COMMA: 51, DOT: 52 };
export const TOP_GAP = 30;      // docked at the top the panel starts this far down, below the menu bar

// A key with its fields in the order the API always gave them.
const key = (k) => {
  if (!k) return k;
  const out = { code: k.code, level: k.level };
  for (const field of ['label', 'name', 'after']) if (field in k) out[field] = k[field];
  return out;
};
const rows = (list) => list.map((row) => row.map(key));

// Without XKB (no xkb-data installed): the US layout.
export function fallbackLabels() {
  const reply = request('fallbackLabels');
  return { altgr: reply.altgr, keys: reply.keys };
}

// Where the active layout has this symbol. A symbol that exists only as a dead key is that key and
// then Space (`after`), as a person types it.
export const keyFor = (labels, ch) => key(request('keyFor', { labels, ch }));

// Rows of the letters page. shift: 0 off, 1 next key, 2 locked.
export const letterRows = (labels, shift = 0) => rows(request('letterRows', { labels, shift }));

// Page 1: numbers and the usual symbols. Page 2: the rest, then the keys that move the caret. Only
// what this layout can type is offered.
export const symbolRows = (labels, page) => rows(request('symbolRows', { labels, page }));

export const punctuation = (labels) => request('punctuation', { labels }).map(key);

// A display's keyboard: tablet proportions, the whole width up to a limit, four rows.
export function panelSize(box) {
  const r = request('panelSize', { w: box.w, h: box.h });
  return { width: r.width, height: r.height, gap: r.gap, pad: r.pad, keyH: r.keyH };
}

// mode: Settings → Keyboard ('off' | 'auto' | 'always').

// Where a hint from another process puts the field: hint { y, h, fw, fh }, y in the window and fw × fh
// the window's size when it was given. A window that changed size since has moved its contents, and a
// centered form by a lot, so that y is a place the field has left: null, as if it were never known,
// until the app says where it is now. top: from the window's top to its content.
export function caretOnScreen(hint, frame, top = 0) {
  if (!hint || !frame) return null;
  const r = request('caretOnScreen', { hint, frame, top });
  return r && { y: r.y, h: r.h };
}

// What to do so the field stays in view. caret: { y, h } on screen or null when only the window is
// known (a Linux app that doesn't say where its caret is: then the whole window); frame: the window
// with the field or null (a shell surface such as Search); area: its work area; top: the keyboard's
// top edge docked at the bottom; panelBottom: its bottom edge docked at the top. → { dock: 'bottom' |
// 'top', frame?: where the window goes }. It keeps nothing: each call decides from where the field is
// now, so calling it again after the field moved also brings a panel back to the bottom. On a low
// display the two places can overlap: the top is only worth it if it clears the field.
export function planAvoid({ caret, frame, area, top, panelBottom = 0, minHeight = 160, refused = false }) {
  const r = request('planAvoid', { caret, frame, area, top, panelBottom, minHeight, refused });
  const out = { dock: r.dock };
  if (r.frameY !== undefined) out.frame = { ...frame, y: r.frameY, ...(r.frameH !== undefined ? { h: r.frameH } : null) };
  return out;
}
