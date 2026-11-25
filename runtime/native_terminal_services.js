// Transport for the compiled Terminal. No UI and no policy: Rust (apps/terminal) decides what every
// tab, pane, key, click and profile does; this runs the session calls it names (lord.term) and keeps
// what must outlive a window's view. A Terminal window is unmounted when it is minimized and its
// sessions go on, so unlike the other apps the sessions are not tied to the lease: they close when
// their window leaves the window manager, or when Rust closes them.
//
// Jobs (`terminal.methods`) answer calls; effects (`terminal.effect`) are the input Rust sends to
// sessions at once (keys, mouse, wheel…), run by the mount before the next frame. Session events
// are kept while no window is mounted and handed to whichever window asks next (`term.events`).
// See docs/RUST-TERMINAL.md.
const EVENTS_MAX = 4000;
const HOTKEY_IDS = ['F12', 'Meta+`'];

export function terminalRuntime({ term, clipboard, clipboardHistory, wm, windows, workArea, settings, addShortcuts, events }) {
  const buffer = [];                    // events of the sessions, newest last
  const pending = new Map();            // window → events for that window alone
  const owners = new Map();             // session → the window that made it
  const guards = new Map();             // window → { busy, stop }
  const closed = new Set();             // windows gone since a window last started
  let known = new Set(windows.get().map(w => w.id));

  const push = event => { if (buffer.length >= EVENTS_MAX) buffer.shift(); buffer.push(event); };
  term.onAny((sid, type, data) => push({ sid, type, data }));

  // A window that leaves the window manager takes its sessions with it.
  windows.subscribe(() => {
    const now = new Set(windows.get().map(w => w.id));
    for (const id of known) {
      if (now.has(id)) continue;
      for (const [sid, window] of [...owners]) if (window === id) { owners.delete(sid); try { term.close(sid); } catch (e) { /* already closed */ } }
      guards.get(id)?.stop();
      guards.delete(id);
      pending.delete(id);
      closed.add(id);
      push({ kind: 'windowGone', id });
    }
    known = now;
  });

  // The drop-down window (F12 or ❖`): a Terminal window of its own that comes and goes.
  const hotkey = e => {
    const hk = settings.get().terminal?.hotkey ?? 'F12';
    return (hk === 'F12' && e.code === 88 && !e.meta && !e.ctrl && !e.alt && !e.shift) || (hk === 'Super+`' && e.meta && e.code === 41);
  };
  function toggleHotkeyWindow() {
    const id = 'terminal#hotkey';
    const w = wm.get(id);
    if (w && !w.minimized && wm.focused() === id) { wm.minimize(id); return; }
    const a = workArea();
    if (w) wm.moveToDesktop(id, wm.desktop());
    wm.open('terminal', { instance: 'hotkey', frame: { x: a.x + 40, y: a.y, w: a.w - 80, h: Math.round(a.h * 0.45) } });
    wm.focus(id);
  }
  const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
  if (!appHost) addShortcuts([{ id: 'terminal.hotkey', keys: HOTKEY_IDS, when: hotkey, run: toggleHotkeyWindow }]);

  const text = (sid, what) => term.text(sid, what) || '';
  const units = list => { const parts = []; for (let i = 0; i < list.length; i += 4096) parts.push(String.fromCharCode(...list.slice(i, i + 4096))); return parts.join(''); };

  const methods = {
    'term.create': { run([options, window]) {
      const sid = term.create(options || {});
      if (sid == null || sid < 0) return null;
      if (window && !windows.get().some(w => w.id === window)) { term.close(sid); return null; }
      if (window) owners.set(sid, window);
      return sid;
    } },
    'term.configure': { run([sid, config]) { term.configure(sid, config); return null; } },
    'term.close': { run([sid]) { owners.delete(sid); term.close(sid); return null; } },
    'term.infos': { run: ([sids]) => (sids || []).map(sid => ({ sid, info: term.info(sid) || null })) },
    'term.events': { run([window], owner) {
      const own = pending.get(window) || [];
      pending.delete(window);
      return [...buffer.splice(0), ...own, ...events(owner.id)];
    } },
    'term.copy': { run: ([sid]) => term.copy(sid) || '' },
    'term.selectOutput': { run: ([sid]) => (term.select(sid, 'lastOutput') ? term.copy(sid) || '' : null) },
    'term.search': { run: ([sid, query, caseSensitive]) => term.search(sid, String(query), !!caseSensitive) },
    'term.searchMove': { run: ([sid, delta]) => term.searchMove(sid, delta) },
    'term.logicalText': { run: ([sid]) => term.logicalText(sid) || '' },
    'term.setMatches': { run: ([sid, triples]) => term.setMatches(sid, triples) },
    'term.cursorWord': { run: ([sid]) => term.cursorWord(sid) || '' },
    'term.words': { run: ([sid, prefix]) => term.words(sid, String(prefix)) || '' },
    'term.replay': { run: ([sid, what]) => term.replay(sid, what) },
    'term.copyKey': { run: ([sid, key, ctrl]) => term.copyKey(sid, String(key), !!ctrl) },
    'term.urlAt': { run: ([sid, x, y]) => term.urlAt(sid, x, y) || '' },
    'term.log': { run: ([sid, on]) => term.log(sid, !!on) || null },
    'term.export': { run: ([sid]) => term.export(sid) || null },
    'term.screenText': { run: ([sid]) => text(sid, 'screen') },
    'term.highlight': { run([sid, match, fg, bg]) { term.highlight(sid, String(match), fg, bg); return null; } },
    'term.tmuxPane': { run: ([gateway, pane]) => term.tmuxPane(gateway, pane) ?? null },
    'clipboard.history': { run: () => clipboardHistory.get().filter(item => item.text != null).map(item => item.text) },
    'clipboard.remember': { run([value]) { clipboard.remember(String(value)); return null; } },
    // The window may not close while a program runs: asking happens in Rust, which hears of it
    // as an event, so the window is brought back if it was minimized.
    'windows.guard': { run([window, busy]) {
      const state = guards.get(window);
      if (state) { state.busy = !!busy; return null; }
      const guard = { busy: !!busy };
      guard.stop = wm.setCloseGuard(window, () => {
        if (!guard.busy) return true;
        const list = pending.get(window) || [];
        list.push({ kind: 'closeRequested' });
        pending.set(window, list);
        wm.focus(window);
        return false;
      });
      guards.set(window, guard);
      return null;
    } },
  };

  // The input of a window's sessions: run at once, with no answer.
  function effect(e) {
    if (e.kind === 'focusWindow') { wm.focus(e.id); return true; }
    if (e.kind === 'toggleMaximize') { wm.toggleMaximize(e.id); return true; }
    if (e.kind !== 'term') return false;
    const sid = e.sid;
    switch (e.op) {
      case 'write':
        term.write(sid, e.bytes ? new Uint8Array(e.bytes).buffer : e.units ? units(e.units) : String(e.text ?? ''));
        break;
      case 'key': term.key(sid, e.key || '', e.char || '', e.base || '', e.mods & 7); break;
      case 'paste': term.paste(sid, String(e.text ?? '')); break;
      case 'mouse': {
        const result = term.mouse(sid, e.type, e.x, e.y, e.button, e.mods);
        if (e.rememberCopy && result === 3) clipboard.remember(lord.clipboard.get());
        break;
      }
      case 'wheel': term.wheel(sid, e.notches, e.x, e.y, e.mods); break;
      case 'focus': term.focus(sid, !!e.on); break;
      case 'scroll': term.scroll(sid, e.what); break;
      case 'clear': term.clear(sid, e.what); break;
      case 'select': term.select(sid, e.what); break;
      case 'mark': term.mark(sid, e.what); break;
      case 'copyMode': term.copyMode(sid, !!e.on); break;
      case 'feed': term.feed(sid, String(e.text ?? '')); break;
      case 'tmuxReady': term.tmuxReady(sid); break;
      case 'tmuxCommand': term.tmuxCommand(sid, String(e.cmd ?? '')); break;
      default: break;
    }
    return true;
  }
  // Windows gone since the last window started: the new one must not find their tabs.
  const takeClosed = () => { const list = [...closed]; closed.clear(); return list; };

  return { methods, effect, takeClosed, toggleHotkeyWindow };
}
