// The Terminal transport keeps what must outlive a window's view: session events are buffered while no
// window is mounted, sessions close with the window that made them (not with its lease), the close
// guard asks Rust, and the effects Rust sends become calls on the sessions.
import assert from 'node:assert/strict';
import { createNativeAsync } from '../../runtime/native_async.js';
import { terminalRuntime } from '../../runtime/native_terminal_services.js';

const calls = [];
let anyListener = null;
const term = new Proxy({}, { get: (_, name) => {
  if (name === 'onAny') return fn => { anyListener = fn; };
  if (name === 'create') return options => { calls.push(['create', options]); return 40 + calls.length; };
  if (name === 'info') return sid => ({ sid, cols: 80, rows: 24 });
  if (name === 'text') return (sid, what) => `${what} of ${sid}`;
  if (name === 'select') return () => true;
  if (name === 'copy') return () => 'copied';
  if (name === 'mouse') return () => 3;
  return (...args) => { calls.push([name, ...args]); return null; };
} });
const remembered = [];
const clipboard = { remember: text => remembered.push(text) };
const clipboardHistory = { get: () => [{ text: 'a' }, { image: 1 }, { text: 'b' }] };
globalThis.lord = { clipboard: { get: () => 'selected text' } };
let list = [{ id: 'terminal' }, { id: 'terminal#2' }];
const subscribers = [];
const windows = { get: () => list, subscribe: fn => { subscribers.push(fn); return () => {}; } };
const guards = new Map();
const focused = [];
const wm = {
  setCloseGuard: (id, fn) => { guards.set(id, fn); return () => guards.delete(id); },
  get: id => (id === 'terminal' ? { x: 1, y: 2, w: 3, h: 4, minimized: false } : undefined),
  focus: id => focused.push(id), toggleMaximize: id => calls.push(['maximize', id]),
};
const shortcuts = [];
let bridge;
const runtime = terminalRuntime({ term, clipboard, clipboardHistory, wm, windows, workArea: () => ({ x: 0, y: 0, w: 1000, h: 600 }),
  settings: { get: () => ({}) }, addShortcuts: entries => shortcuts.push(...entries), events: id => bridge.events(id) });
bridge = createNativeAsync(runtime.methods);
const tick = async () => { for (let i = 0; i < 6; i++) await new Promise(resolve => setImmediate(resolve)); };
async function call(owner, method, args) {
  const token = bridge.start(owner, method, args);
  await tick();
  const [packet] = bridge.take(owner, [token]);
  assert.ok(packet, `${method} produced a result`);
  if (packet.error) throw new Error(packet.error);
  if (packet.part === undefined) return packet.value;
  // a large answer arrives in parts
  let text = packet.part, last = packet;
  while (!last.done) { [last] = bridge.take(owner, [token]); text += last.part; }
  return JSON.parse(text);
}
bridge.open(1, Object.keys(runtime.methods));

// the drop-down hotkey is registered once
assert.equal(shortcuts.length, 1);
assert.equal(shortcuts[0].when({ code: 88, meta: false, ctrl: false, alt: false, shift: false }), true, 'F12 is the default');
assert.equal(shortcuts[0].when({ code: 88, meta: true }), false);

// events are kept while nobody asks, and come out once, oldest first
anyListener(41, 'title', 'vim');
anyListener(41, 'bell', null);
assert.deepEqual(await call(1, 'term.events', ['terminal']), [{ sid: 41, type: 'title', data: 'vim' }, { sid: 41, type: 'bell', data: null }]);
assert.deepEqual(await call(1, 'term.events', ['terminal']), []);
for (let i = 0; i < 5000; i++) anyListener(41, 'output', i);
const kept = await call(1, 'term.events', ['terminal']);
assert.ok(kept.length <= 4000 && kept[kept.length - 1].data === 4999, 'the buffer is bounded and keeps the newest');

// sessions: made for a window, closed with it, and not with the lease
const sid = await call(1, 'term.create', [{ cwd: '/tmp', scrollback: 100 }, 'terminal#2']);
assert.equal(typeof sid, 'number');
const other = await call(1, 'term.create', [{}, 'terminal']);
assert.equal(await call(1, 'term.create', [{}, 'gone-already']), null, 'a window that no longer exists gets no session');
const closesBefore = calls.filter(c => c[0] === 'close').length;
bridge.close(1);
assert.equal(calls.filter(c => c[0] === 'close').length, closesBefore, 'closing the lease (minimizing) leaves the sessions alone');
bridge.open(2, Object.keys(runtime.methods));
list = [{ id: 'terminal' }];
subscribers.forEach(fn => fn());
const closes = calls.filter(c => c[0] === 'close').map(c => c[1]);
assert.ok(closes.includes(sid) && !closes.includes(other), 'the session of the window that left is closed, the other stays');
assert.deepEqual(await call(2, 'term.events', ['terminal']), [{ kind: 'windowGone', id: 'terminal#2' }]);
assert.deepEqual(runtime.takeClosed(), ['terminal#2']);
assert.deepEqual(runtime.takeClosed(), [], 'a window learns of the closed ones once');
await call(2, 'term.close', [other]);
assert.ok(calls.some(c => c[0] === 'close' && c[1] === other));

// plain calls
assert.deepEqual(await call(2, 'term.infos', [[41, 42]]), [{ sid: 41, info: { sid: 41, cols: 80, rows: 24 } }, { sid: 42, info: { sid: 42, cols: 80, rows: 24 } }]);
assert.equal(await call(2, 'term.screenText', [41]), 'screen of 41');
assert.equal(await call(2, 'term.selectOutput', [41]), 'copied');
assert.deepEqual(await call(2, 'clipboard.history', []), ['a', 'b'], 'only text entries');
await call(2, 'clipboard.remember', ['kept']);
assert.deepEqual(remembered, ['kept']);

assert.deepEqual(await call(2, 'windows.frames', [['terminal', 'nowhere']]), { terminal: { x: 1, y: 2, w: 3, h: 4 }, nowhere: null }, 'frames of the windows the manager has');
assert.deepEqual(await call(2, 'term.list', []), [], 'no shells listed means an empty list');

// the close guard: free while idle, otherwise it refuses and asks the window through its events
await call(2, 'windows.guard', ['terminal', false]);
assert.equal(guards.get('terminal')(), true);
await call(2, 'windows.guard', ['terminal', true]);
assert.equal(guards.get('terminal')(), false);
assert.deepEqual(focused, ['terminal'], 'the window comes forward to ask');
assert.deepEqual(await call(2, 'term.events', ['terminal']), [{ kind: 'closeRequested' }]);
assert.deepEqual(await call(2, 'term.events', ['terminal2']), [], 'only that window hears of it');

// effects
const send = effect => runtime.effect({ kind: 'term', sid: 7, ...effect });
assert.equal(send({ op: 'write', text: 'ls\r' }), true);
send({ op: 'write', bytes: [27, 91, 65] });
send({ op: 'write', units: [104, 105] });
send({ op: 'key', key: 'ArrowUp', char: '', base: '', mods: 9 });
send({ op: 'mouse', type: 'up', x: 1, y: 2, button: 0, mods: 0, rememberCopy: true });
send({ op: 'mouse', type: 'move', x: 1, y: 2, button: 0, mods: 0 });
send({ op: 'tmuxCommand', cmd: 'new-window' });
assert.deepEqual(calls.filter(c => c[0] === 'write').map(c => (typeof c[2] === 'string' ? c[2] : Array.from(new Uint8Array(c[2])))), ['ls\r', [27, 91, 65], 'hi']);
assert.deepEqual(calls.find(c => c[0] === 'key'), ['key', 7, 'ArrowUp', '', '', 1], 'only the three modifier bits go to the engine');
assert.deepEqual(remembered, ['kept', 'selected text'], 'a mouse release that copied remembers the clipboard once');
assert.deepEqual(calls.find(c => c[0] === 'tmuxCommand'), ['tmuxCommand', 7, 'new-window']);
assert.equal(runtime.effect({ kind: 'focusWindow', id: 'terminal' }), true);
assert.equal(runtime.effect({ kind: 'toggleMaximize', id: 'terminal' }), true);
assert.equal(runtime.effect({ kind: 'closeWindow', id: 'terminal' }), false, 'every other effect is the host\'s');
assert.deepEqual(focused, ['terminal', 'terminal']);

// a method that was not declared cannot be started
assert.throws(() => bridge.start(2, 'term.nothing', []), /app.invalidRequest/);
console.log('native-terminal-services-test: events, session lifetime, close guard and effects passed');
