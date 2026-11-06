// Search host transport. Rust owns matching, calculations, lifetime and result state.
import { launcherRequest, launcherApp, launchApp } from './apps.js';
import { createStore } from './store.js';
import { locale, t, tOptional, formatNumber, formatDate } from './i18n.js';
import { clipboard, clipboardHistory } from './clipboard.js';
import { wm, getApp, appRegistry } from './windows.js';
import { settings } from './settings.js';
import { logOut } from './sessionrestore.jsx';
import { focusToken, restoreFocus } from './components.jsx';

const request = args => launcherRequest({ ...args, op: 'search' });
const sessions = new Map(), sources = new Map();
const version = createStore(0);
let timer = 0, dirty = true, queued = false, previousFocus, panelSession;
export const spotlightOpen = createStore(false);
export const spotlight = {
  open() { if (!spotlightOpen.get()) { previousFocus = focusToken(); spotlightOpen.set(true); } },
  close() { if (spotlightOpen.get()) { panelSession?.close(); panelSession = null; spotlightOpen.set(false); setTimeout(() => restoreFocus(previousFocus), 0); } },
  toggle() { spotlightOpen.get() ? spotlight.close() : spotlight.open(); },
};
export const calc = text => request({ cmd: 'calc', text: String(text) }).value;
export const convert = text => request({ cmd: 'convert', text: String(text) }).value;

function number(value, max) {
  const n = Number(Number(value).toFixed(max));
  if (Math.abs(n) >= 1e15 || (n !== 0 && Math.abs(n) < 1e-9)) return Number(value).toPrecision(8);
  const [int, frac = ''] = n.toFixed(max).split('.');
  const tail = frac.replace(/0+$/, '');
  return formatNumber(Number(int), 0) + (tail ? (formatNumber(0.5, 1)[1] || '.') + tail : '');
}
export function searchCaption(value) {
  if (value == null) return '';
  if (typeof value !== 'object') return String(value);
  if (value.key) return t(value.key, value.params);
  if (value.parts) return value.parts.map(searchCaption).join('');
  if (value.upper) return searchCaption(value.upper).toUpperCase();
  if ('number' in value) return number(value.number, value.digits);
  if ('date' in value) return formatDate(new Date(value.date), 'short');
  if ('size' in value) { const n = value.size; return n < 1024 ? `${n} B` : n < 1048576 ? `${formatNumber(n / 1024, 1)} KB` : n < 1073741824 ? `${formatNumber(n / 1048576, 1)} MB` : `${formatNumber(n / 1073741824, 1)} GB`; }
  return '';
}

function syncFacts() {
  if (!dirty) return; dirty = false;
  const ids = request({ cmd: 'settings' }).ids;
  // Bound the wire copy before JSON serialization; native storage has its own byte budget.
  let budget = 1024 * 1024;
  const clips = [];
  for (const it of clipboardHistory.get()) {
    if (!it.text || budget <= 0) continue;
    const text = it.text.slice(0, budget).replace(/[\uD800-\uDBFF]$/, ''); budget -= text.length;
    clips.push({ id: it.id, time: it.time, text });
    if (clips.length >= 500) break;
  }
  request({ cmd: 'facts', lang: locale.get().lang.split('-')[0], notes: !!getApp('notes'), help: !!getApp('help'),
    shell: typeof lord.locale?.catalog === 'function' ? lord.locale.catalog(locale.get().lang, 'shell') || '' : '',
    fallbackShell: typeof lord.locale?.catalog === 'function' ? lord.locale.catalog('en', 'shell') || '' : '',
    settings: ids.map(id => ({ id, title: tOptional(`settings.section.${id}`) || '', keywords: tOptional(`settings.keywords.${id}`) || '' })),
    actions: [{ id: 'dark', title: t('shell.spotlight.action.dark') }, { id: 'logout', title: t('shell.menu.logout') }], clipboard: clips });
}
function changed() {
  dirty = true;
  if (queued || !sessions.size) return; queued = true;
  queueMicrotask(() => { queued = false; if (!sessions.size) return; syncFacts(); for (const session of sessions.values()) session.query(session.snapshot.query); version.set(v => v + 1); });
}
locale.subscribe(changed); clipboardHistory.subscribe(changed); settings.subscribe(changed); appRegistry.subscribe(changed);

function arm() {
  if (timer || ![...sessions.values()].some(s => s.snapshot.pending)) return;
  timer = setTimeout(() => {
    timer = 0; if (!sessions.size) return;
    const reply = request({ cmd: 'poll' });
    for (const update of reply.updates) sessions.get(update.id)?.accept(update);
    if (reply.updates.length) version.set(v => v + 1);
    arm();
  }, 16);
}
function execute(action, reveal) {
  if (!action) return;
  const external = action.type === 'external' ? sources.get(action.source)?.callbacks.get(action.key) : null;
  setTimeout(() => {
    switch (action.type) {
      case 'app': launchApp(action.id); break;
      case 'settings': wm.open('settings', { args: { section: action.id } }); break;
      case 'action': if (action.id === 'dark') settings.set({ mode: settings.get().mode === 'dark' ? 'light' : 'dark' }); else if (action.id === 'logout') logOut(); break;
      case 'help': wm.open('help', { args: { article: action.id } }); break;
      case 'note': wm.open('notes', { args: { slug: action.id } }); break;
      case 'copyNumber': clipboard.set(String(+(action.significant ? Number(action.value).toPrecision(action.significant) : Number(action.value).toFixed(action.decimals)))); break;
      case 'clipboard': { const it = clipboardHistory.get().find(it => it.id === action.id && it.time === action.time); if (it) clipboard.use(it); break; }
      case 'file': {
        const f = action.file;
        if (!reveal && f.dir) wm.open('files', { new: true, args: { path: f.path } });
        else if (!reveal && action.text && getApp('editor')) wm.open('editor', { args: { path: f.path } });
        else wm.open('files', { new: true, args: { path: f.path.split('/').slice(0, -1).join('/'), select: f.name } });
        break;
      }
      case 'external': (reveal && external?.reveal ? external.reveal : external?.run)?.(); break;
    }
  }, 0);
}
function scheduleSources(session) {
  const signature = `${session.snapshot.query}|${session.snapshot.revision}`;
  if (signature === session.sourceSignature) return;
  session.sourceSignature = signature; clearTimeout(session.sourceTimer);
  for (const source of sources.values()) for (const key of source.callbacks.keys()) if (key.startsWith(`${session.id}:`)) source.callbacks.delete(key);
  if (!sources.size || !session.snapshot.query?.trim()) return;
  session.sourceTimer = setTimeout(() => {
    session.sourceTimer = 0; if (session.closed || signature !== session.sourceSignature) return;
    const query = session.snapshot.query, q = request({ cmd: 'fold', text: query.trim() }).value;
    const helpers = { fold: text => request({ cmd: 'fold', text }).value, best: (query, ...texts) => request({ cmd: 'best', query, texts: texts.map(v => String(v || '')) }).value };
    const sections = [];
    for (const [id, source] of sources) {
      let items = []; try { items = (source.fn(q, query, helpers) || []).slice(0, 6); } catch (error) { console.error(`spotlight ${id}:`, error); }
      sections.push({ id, items: items.map(it => {
        const key = `${session.id}:${it.key}`; source.callbacks.set(key, it);
        const preview = it.preview?.kind === 'app' ? { kind: 'app', id: it.preview.app.id, name: it.preview.app.name, comment: it.preview.app.comment, appKind: it.preview.app.kind } : it.preview;
        return { key: it.key, score: it.score, title: it.title, subtitle: it.subtitle, glyph: it.glyph, image: it.image, big: it.big, hint: it.hint, preview, providerIcon: !!it.Icon,
          action: { type: 'external', source: id, key } };
      }) });
    }
    session.accept(request({ cmd: 'external', id: session.id, query, sections })); version.set(v => v + 1);
  }, 0);
}
export const searchSourceIcon = (source, key) => sources.get(source)?.callbacks.get(key)?.Icon;
export function createSearchSession(panel = false, context = {}) {
  syncFacts();
  const initial = request({ cmd: 'open', panel, context });
  const session = { id: initial.id, snapshot: initial, store: createStore(initial), closed: false,
    accept(reply) { if (this.closed) return; this.snapshot = reply; this.store.set(reply); scheduleSources(this); arm(); },
    query(query) { if (this.closed) return this.snapshot; syncFacts(); const reply = request({ cmd: 'query', id: this.id, query }); this.accept(reply); return reply; },
    context(context) { if (!this.closed) this.accept(request({ cmd: 'context', id: this.id, context })); },
    event(action, extra = {}) {
      if (this.closed) return false;
      const reply = request({ cmd: 'event', id: this.id, action, ...extra }); this.accept(reply);
      for (const effect of reply.effects || []) if (effect.type === 'execute') execute(effect.action, effect.reveal);
      if (reply.effects?.some(effect => effect.type === 'close')) spotlight.close();
      return !!reply.handled;
    },
    close() { if (this.closed) return; this.closed = true; clearTimeout(this.sourceTimer); sessions.delete(this.id); request({ cmd: 'close', id: this.id }); for (const source of sources.values()) for (const key of source.callbacks.keys()) if (key.startsWith(`${this.id}:`)) source.callbacks.delete(key); this.snapshot = { id: this.id, sections: [], tree: null, pending: false }; this.store.set(this.snapshot); if (!sessions.size) { clearTimeout(timer); timer = 0; dirty = true; } },
  };
  sessions.set(session.id, session); if (panel) panelSession = session;
  return session;
}
function bind(section, session) {
  return { id: section.id, items: section.items.map(it => ({ ...it, title: searchCaption(it.title), subtitle: searchCaption(it.subtitle), hint: searchCaption(it.hint),
    Icon: it.app ? launcherApp(it.app)?.Icon : searchSourceIcon(it.action?.source, it.action?.key),
    preview: it.preview?.kind === 'app' ? { kind: 'app', app: launcherApp(it.preview.id) } : it.preview,
    __session: session, run: () => session.event(['run', it.key]), reveal: it.action?.type === 'file' ? () => session.event(['run', it.key], { reveal: true }) : undefined })) };
}
export const searchEverything = {
  prepare: () => createSearchSession(),
  results(query, apps, session) { if (!session || session.closed) return []; return session.query(query).sections.map(section => bind(section, session)); },
  version: () => version,
  run(item, reveal = false) { if (item?.__session) item.__session.event(['run', item.key], { reveal }); else if (item) (reveal && item.reveal ? item.reveal : item.run)?.(); },
};
export function registerSpotlightSource(id, fn) { sources.set(id, { fn, callbacks: new Map() }); changed(); return () => { sources.delete(id); changed(); }; }
