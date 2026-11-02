// buffer.js — the editor document: lines, multiple cursors, edits and undo.
//
// A position is { l, c }: line and column in JS units (UTF-16), like String.slice.
// A cursor is { l, c, al, ac, goal }: the head (l, c), the selection anchor (al, ac) and the
// desired visual column for moving up and down (goal). Without a selection, anchor = head.
//
// Every change goes through apply(edits): sorted, non-overlapping replacements, applied
// back to front, that return where each one ended up. Each transaction stores its inverse,
// so undo and redo are just another transaction. Consecutive keystrokes are grouped into a single step.

export const cmp = (a, b) => (a.l - b.l) || (a.c - b.c);
export const head = (k) => ({ l: k.l, c: k.c });
export const anchor = (k) => ({ l: k.al, c: k.ac });
export const hasSel = (k) => k.l !== k.al || k.c !== k.ac;
export const selStart = (k) => (cmp(head(k), anchor(k)) <= 0 ? head(k) : anchor(k));
export const selEnd = (k) => (cmp(head(k), anchor(k)) <= 0 ? anchor(k) : head(k));
export const cursorAt = (p, goal = -1) => ({ l: p.l, c: p.c, al: p.l, ac: p.c, goal });

const GROUP_MS = 900;

export class Doc {
  constructor(text = '') {
    this.eol = /\r\n/.test(text) ? '\r\n' : '\n';
    this.lines = text.split(/\r?\n/);
    this.cursors = [cursorAt({ l: 0, c: 0 })];
    this.undoStack = [];
    this.redoStack = [];
    this.version = 0;          // changes with every edit
    this.serial = 0;           // state number: undo and redo go back to earlier states
    this.savedSerial = 0;      // state saved to disk (for "modified")
    this.nextSerial = 1;
    this.firstChanged = 0;     // first line touched since the last query (highlighting)
    this.sealed = true;        // the next change starts a new undo step
  }

  get dirty() { return this.serial !== this.savedSerial; }
  markSaved() { this.savedSerial = this.serial; }
  touch() { this.serial = this.nextSerial++; this.version++; }   // change without history (line ending)
  text() { return this.lines.join(this.eol); }
  line(l) { return this.lines[l] ?? ''; }
  get lineCount() { return this.lines.length; }

  clamp(p) {
    const l = Math.max(0, Math.min(this.lines.length - 1, p.l));
    return { l, c: Math.max(0, Math.min(this.lines[l].length, p.c)) };
  }

  textIn(a, b) {
    if (a.l === b.l) return this.lines[a.l].slice(a.c, b.c);
    const out = [this.lines[a.l].slice(a.c)];
    for (let l = a.l + 1; l < b.l; l++) out.push(this.lines[l]);
    out.push(this.lines[b.l].slice(0, b.c));
    return out.join('\n');
  }

  // Sorts the cursors and merges the ones that touch.
  normalize() {
    const ks = this.cursors.map((k) => ({ ...k })).sort((a, b) => cmp(selStart(a), selStart(b)));
    const out = [];
    for (const k of ks) {
      const prev = out[out.length - 1];
      if (prev && cmp(selStart(k), selEnd(prev)) <= 0 && (hasSel(k) || hasSel(prev) || cmp(head(k), head(prev)) === 0)) {
        const s = cmp(selStart(prev), selStart(k)) <= 0 ? selStart(prev) : selStart(k);
        const e = cmp(selEnd(prev), selEnd(k)) >= 0 ? selEnd(prev) : selEnd(k);
        const forward = cmp(head(prev), anchor(prev)) >= 0;
        out[out.length - 1] = forward ? { l: e.l, c: e.c, al: s.l, ac: s.c, goal: -1 } : { l: s.l, c: s.c, al: e.l, ac: e.c, goal: -1 };
      } else out.push(k);
    }
    this.cursors = out;
  }

  // edits: [{ from, to, text }] sorted, non-overlapping. Returns [{ start, end }] in
  // new coordinates, one per edit, and the inverse (for undo).
  rawApply(edits) {
    const removed = edits.map((e) => this.textIn(e.from, e.to));
    for (let i = edits.length - 1; i >= 0; i--) {
      const { from, to, text } = edits[i];
      const before = this.lines[from.l].slice(0, from.c), after = this.lines[to.l].slice(to.c);
      const parts = text.split('\n');
      parts[0] = before + parts[0];
      parts[parts.length - 1] += after;
      this.lines.splice(from.l, to.l - from.l + 1, ...parts);
    }
    const results = [];
    let dl = 0, dc = 0, lastLine = -1;
    for (const e of edits) {
      const sl = e.from.l + dl, sc = e.from.c + (e.from.l === lastLine ? dc : 0);
      const parts = e.text.split('\n');
      const el = sl + parts.length - 1;
      const ec = parts.length === 1 ? sc + parts[0].length : parts[parts.length - 1].length;
      dl += parts.length - 1 - (e.to.l - e.from.l);
      dc = ec - e.to.c;
      lastLine = e.to.l;
      results.push({ start: { l: sl, c: sc }, end: { l: el, c: ec } });
    }
    const inverse = results.map((r, i) => ({ from: r.start, to: r.end, text: removed[i] }));
    this.version++;
    this.firstChanged = Math.min(this.firstChanged, edits.length ? edits[0].from.l : this.firstChanged);
    return { results, inverse };
  }

  // Maps a position in the old document to the new one through the edits.
  static mapPos(p, edits, results) {
    let dl = 0, dc = 0, lastLine = -1;
    for (let i = 0; i < edits.length; i++) {
      const e = edits[i];
      if (cmp(p, e.from) < 0) break;
      if (cmp(p, e.to) <= 0) return { ...results[i].end };
      dl = results[i].end.l - e.to.l;
      dc = results[i].end.c - e.to.c;
      lastLine = e.to.l;
    }
    return { l: p.l + dl, c: p.c + (p.l === lastLine ? dc : 0) };
  }

  // A transaction: applies, stores the inverse and leaves the cursors that place(results) says.
  transact(edits, place, kind = 'edit') {
    edits = edits.filter((e) => e.text !== '' || cmp(e.from, e.to) !== 0);
    if (!edits.length) return false;
    const before = this.cursors.map((k) => ({ ...k }));
    const serialBefore = this.serial;
    const { results, inverse } = this.rawApply(edits);
    this.serial = this.nextSerial++;
    this.cursors = place(results, edits);
    this.normalize();
    const tx = { edits: inverse, before, after: this.cursors.map((k) => ({ ...k })), serialBefore, serialAfter: this.serial };
    const now = Date.now();
    const last = this.undoStack[this.undoStack.length - 1];
    if (!this.sealed && last && last.kind === kind && kind !== 'edit' && now - last.time < GROUP_MS) {
      // the group becomes a single state
      tx.serialBefore = last.txs[0].serialBefore;
      last.txs.push(tx);
      last.time = now;
    } else this.undoStack.push({ kind, time: now, txs: [tx] });
    if (this.undoStack.length > 500) this.undoStack.shift();
    this.redoStack = [];
    this.sealed = kind === 'edit';
    return true;
  }

  seal() { this.sealed = true; }

  undo() { return this.swap(this.undoStack, this.redoStack); }
  redo() { return this.swap(this.redoStack, this.undoStack); }

  swap(from, to) {
    const group = from.pop();
    if (!group) return false;
    const back = { kind: group.kind, time: 0, txs: [] };
    for (let i = group.txs.length - 1; i >= 0; i--) {
      const tx = group.txs[i];
      const { inverse } = this.rawApply(tx.edits);
      back.txs.unshift({ edits: inverse, before: tx.after, after: tx.before, serialBefore: tx.serialAfter, serialAfter: tx.serialBefore });
      this.cursors = tx.before.map((k) => ({ ...k }));
    }
    this.serial = group.txs[0].serialBefore;
    to.push(back);
    this.sealed = true;
    return true;
  }

  // ---- high-level edits (for every cursor) ----

  // Replaces each selection with text (or with textFor(cursor)); the cursor ends up at the end.
  insert(text, kind = 'type') {
    this.normalize();
    const edits = this.cursors.map((k) => ({ from: selStart(k), to: selEnd(k), text: typeof text === 'function' ? text(k) : text }));
    return this.transact(edits, (rs) => rs.map((r) => cursorAt(r.end)), kind);
  }

  // Deletes the selection, or from the cursor to to(cursor) if there is none.
  deleteBy(to, kind = 'delete') {
    this.normalize();
    const edits = this.cursors.map((k) => {
      if (hasSel(k)) return { from: selStart(k), to: selEnd(k), text: '' };
      const t = this.clamp(to(head(k)));
      return cmp(t, head(k)) < 0 ? { from: t, to: head(k), text: '' } : { from: head(k), to: t, text: '' };
    });
    // two cursors deleting the same thing: without overlapping
    const clean = [];
    for (const e of edits) {
      const prev = clean[clean.length - 1];
      if (prev && cmp(e.from, prev.to) < 0) { if (cmp(e.to, prev.to) > 0) prev.to = e.to; } else clean.push(e);
    }
    return this.transact(clean, (rs) => rs.map((r) => cursorAt(r.start)), kind);
  }

  // Lines touched by the cursors (no repeats, sorted).
  cursorLines() {
    const set = new Set();
    for (const k of this.cursors) {
      const s = selStart(k), e = selEnd(k);
      const last = e.c === 0 && e.l > s.l ? e.l - 1 : e.l;
      for (let l = s.l; l <= last; l++) set.add(l);
    }
    return [...set].sort((a, b) => a - b);
  }

  // Changes whole lines (indentation, comment) keeping the selections.
  mapLines(fn, kind = 'edit') {
    const edits = [];
    for (const l of this.cursorLines()) {
      const old = this.lines[l], nu = fn(old, l);
      if (nu !== old) edits.push({ from: { l, c: 0 }, to: { l, c: old.length }, text: nu });
    }
    if (!edits.length) return false;
    const before = this.cursors;
    return this.transact(edits, (rs, es) => before.map((k) => {
      const shift = (p) => {
        const i = es.findIndex((e) => e.from.l === p.l);
        if (i < 0) return p;
        const delta = rs[i].end.c - es[i].to.c;
        return { l: p.l, c: Math.max(0, Math.min(rs[i].end.c, p.c === 0 && delta < 0 ? 0 : p.c + delta)) };
      };
      const h = shift(head(k)), a = shift(anchor(k));
      return { l: h.l, c: h.c, al: a.l, ac: a.c, goal: -1 };
    }), kind);
  }
}
