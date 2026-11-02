// store.js — the notes on disk: ~/Notes/<slug>/nota.md (markdown with a minimal YAML
// header) and its attachments next to it. They save themselves, about 600 ms after the last change.
//
//   ---
//   title: Groceries
//   color: yellow
//   x: 120
//   y: 90
//   w: 260
//   h: 240
//   desktop: true
//   pinned: false
//   created: 2026-10-16T10:12:00.000Z
//   updated: 2026-10-16T10:20:31.000Z
//   ---
//   # Groceries
//   - [ ] yerba mate
import { createStore, fs, screen, registerWidgetType, setWidgets } from '@lordos/sdk';

export const DIR = 'Notes';
const OLD_DIR = 'Notas'; // before 0.7.0: moved to ~/Notes on first run (see ensureDir)
export const COLORS = ['yellow', 'pink', 'green', 'blue', 'purple', 'gray'];
export const notesStore = createStore({ loaded: false, list: [] });

const FIELDS = ['title', 'color', 'x', 'y', 'w', 'h', 'desktop', 'pinned', 'created', 'updated'];

export function parseNote(text) {
  const meta = {};
  let body = text;
  const m = /^---\n([\s\S]*?)\n---\n?/.exec(text.replace(/\r\n/g, '\n'));
  if (m) {
    body = text.replace(/\r\n/g, '\n').slice(m[0].length);
    for (const line of m[1].split('\n')) {
      const kv = /^(\w+):\s*(.*)$/.exec(line);
      if (!kv) continue;
      let v = kv[2].trim();
      if (/^".*"$/.test(v)) v = v.slice(1, -1).replace(/\\"/g, '"');
      else if (v === 'true' || v === 'false') v = v === 'true';
      else if (/^-?\d+(\.\d+)?$/.test(v)) v = Number(v);
      meta[kv[1]] = v;
    }
  }
  return { meta, body };
}

const yaml = (v) => (typeof v === 'string' && (/[:#"'\n]|^\s|\s$/.test(v) || v === '' || v === 'true' || v === 'false' || /^-?\d/.test(v))
  ? `"${v.replace(/"/g, '\\"').replace(/\n/g, ' ')}"` : String(v));

export function serialize(n) {
  const head = FIELDS.filter((k) => n[k] !== undefined && n[k] !== null).map((k) => `${k}: ${yaml(n[k])}`).join('\n');
  return `---\n${head}\n---\n${n.body}`;
}

// Title: the first line with text, without the markdown marks.
export function titleOf(body) {
  const line = body.split('\n').find((l) => l.trim() && !/^(`{3,}|---)/.test(l.trim())) || '';
  return line.replace(/^\s*(#{1,6}\s+|>\s*|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)/, '').replace(/[*_`~]|!\[[^\]]*\]\([^)]*\)/g, '').trim().slice(0, 60);
}

export const folder = (slug) => `${DIR}/${slug}`;
export const absFolder = (slug) => `${fs.home()}/${DIR}/${slug}`;

const exists = (p) => { try { return !!fs.stat(p); } catch (e) { return false; } };

// First run after the rename: ~/Notas becomes ~/Notes. If both exist, ~/Notes wins and ~/Notas is
// left untouched.
let migrated = false;
function migrateOldDir() {
  if (migrated) return;
  migrated = true;
  if (exists(DIR) || !exists(OLD_DIR)) return;
  try { fs.rename(OLD_DIR, DIR); console.log('notes: moved ~/Notas to ~/Notes'); } catch (e) { console.log(`notes: could not move ~/Notas to ~/Notes: ${e.message}`); }
}

function ensureDir() {
  migrateOldDir();
  try { fs.list(DIR); } catch (e) { try { fs.mkdir(DIR); } catch (e2) { /* no home folder */ } }
}

export function loadNotes() {
  ensureDir();
  const list = [];
  let dirs = [];
  try { dirs = fs.list(DIR).filter((e) => e.dir); } catch (e) { dirs = []; }
  for (const d of dirs) {
    let txt = null;
    try { txt = fs.read(`${DIR}/${d.name}/nota.md`); } catch (e) { continue; }
    const { meta, body } = parseNote(txt);
    list.push({
      slug: d.name, body, title: meta.title || titleOf(body), color: COLORS.includes(meta.color) ? meta.color : 'yellow',
      x: meta.x ?? 80, y: meta.y ?? 80, w: meta.w ?? 260, h: meta.h ?? 240, desktop: !!meta.desktop, pinned: !!meta.pinned,
      created: meta.created || new Date(d.mtime).toISOString(), updated: meta.updated || new Date(d.mtime).toISOString(),
    });
  }
  list.sort((a, b) => String(b.updated).localeCompare(String(a.updated)));
  notesStore.set({ loaded: true, list });
}

// ---- saving ----

const timers = new Map();
function writeNow(slug) {
  timers.delete(slug);
  const n = notesStore.get().list.find((x) => x.slug === slug);
  if (!n) return;
  try { fs.write(`${folder(slug)}/nota.md`, serialize(n)); } catch (e) { console.log(`notes: could not save ${slug}: ${e.message}`); }
}
function saveSoon(slug) {
  clearTimeout(timers.get(slug));
  timers.set(slug, setTimeout(() => writeNow(slug), 600));
}
export function flushAll() { for (const slug of [...timers.keys()]) { clearTimeout(timers.get(slug)); writeNow(slug); } }

const patchList = (fn) => notesStore.set((s) => ({ ...s, list: fn(s.list) }));

export function updateNote(slug, patch, { touch = true } = {}) {
  patchList((list) => list.map((n) => {
    if (n.slug !== slug) return n;
    const next = { ...n, ...patch };
    if (patch.body !== undefined) next.title = titleOf(patch.body) || n.title;
    if (touch) next.updated = new Date().toISOString();
    return next;
  }));
  saveSoon(slug);
}

const stamp = () => new Date().toISOString().replace(/[-:]/g, '').replace(/\..*/, '').replace('T', '-');
function newSlug() {
  const taken = new Set(notesStore.get().list.map((n) => n.slug));
  let s = `note-${stamp()}`, i = 2;
  while (taken.has(s)) s = `note-${stamp()}-${i++}`;
  return s;
}

export function createNote({ body = '', color = 'yellow', desktop = false, x, y } = {}) {
  ensureDir();
  const slug = newSlug();
  try { fs.mkdir(folder(slug)); } catch (e) { return null; }
  const now = new Date().toISOString();
  const count = notesStore.get().list.filter((n) => n.desktop).length;
  const n = { slug, body, title: titleOf(body), color, x: x ?? 140 + (count % 6) * 30, y: y ?? 90 + (count % 6) * 30, w: 260, h: 240,
    desktop, pinned: false, created: now, updated: now };
  patchList((list) => [n, ...list]);
  writeNow(slug);
  return slug;
}

export function duplicateNote(slug) {
  const n = notesStore.get().list.find((x) => x.slug === slug);
  if (!n) return null;
  const copy = createNote({ body: n.body, color: n.color, desktop: n.desktop, x: n.x + 30, y: n.y + 30 });
  // attachments: copied next to it (in the background)
  try {
    for (const e of fs.list(folder(slug))) if (e.name !== 'nota.md') fs.copy(`${folder(slug)}/${e.name}`, `${folder(copy)}/${e.name}`);
  } catch (e) { /* no attachments */ }
  return copy;
}

// Delete: to the Trash if it exists; otherwise, to ~/Notes/.trash (can be recovered by hand).
export function deleteNote(slug) {
  clearTimeout(timers.get(slug));
  timers.delete(slug);
  let ok = false;
  try { if (fs.trash) { fs.trash(folder(slug)); ok = true; } } catch (e) { ok = false; }
  if (!ok) {
    try { fs.mkdir(`${DIR}/.trash`); } catch (e) { /* already there */ }
    try { fs.rename(folder(slug), `${DIR}/.trash/${slug}`); ok = true; } catch (e) { ok = false; }
  }
  if (ok) patchList((list) => list.filter((n) => n.slug !== slug));
  return ok;
}

// ---- attachments ----

function freeName(slug, name) {
  let names;
  try { names = new Set(fs.list(folder(slug)).map((e) => e.name)); } catch (e) { names = new Set(); }
  const dot = name.lastIndexOf('.');
  const base = dot > 0 ? name.slice(0, dot) : name, ext = dot > 0 ? name.slice(dot) : '';
  let out = name.replace(/[/\\]/g, '_'), i = 2;
  while (names.has(out)) out = `${base}-${i++}${ext}`;
  return out;
}

const IMG = /\.(png|jpe?g)$/i;
export const isImage = (name) => IMG.test(name);

// Copies a file from the home folder into the note and returns the markdown that shows it.
export function attachFile(slug, path) {
  const rel = path.replace(`${fs.home()}/`, '');
  const name = freeName(slug, rel.split('/').pop());
  const dst = `${folder(slug)}/${name}`;
  try {
    if (fs.copy) fs.copy(rel, dst);
    else fs.write(dst, fs.read(rel, { binary: true }));
  } catch (e) { return null; }
  return isImage(name) ? `![${name}](${name})` : `[📎 ${name}](${name})`;
}

// The clipboard image (copied in any app) as the note's image-N.png.
export function pasteClipboardImage(slug) {
  const data = lord.clipboard.imageData ? lord.clipboard.imageData() : null;
  if (!data) return null;
  let names;
  try { names = new Set(fs.list(folder(slug)).map((e) => e.name)); } catch (e) { names = new Set(); }
  let i = 1;
  while (names.has(`image-${i}.png`)) i++;
  const name = `image-${i}.png`;
  try { fs.write(`${folder(slug)}/${name}`, data); } catch (e) { return null; }
  return `![${name}](${name})`;
}

// ---- notes on the desktop (widgets) ----

export function publishWidgets() {
  const list = notesStore.get().list.filter((n) => n.desktop);
  setWidgets('note', list.map((n) => ({
    id: n.slug, type: 'note', pinned: n.pinned,
    x: Math.max(0, Math.min(n.x, screen.width - 80)), y: Math.max(0, Math.min(n.y, screen.height - 60)), w: n.w, h: n.h,
  })));
}
notesStore.subscribe(publishWidgets);

export function setupWidgets(Card) {
  registerWidgetType('note', Card);
}
