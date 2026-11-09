// sdk/windows.js over the real bridge (server/src/ui/js_winmodel.c) and the real Rust window list
// (rust/axia-shell/src/winmodel/session.rs), in QuickJS. Run by server/tests/windows_bridge_test.mjs.
import { wm, windows, activeDesktop, desktopList, windowViews, configureWindows, useWindows, createDesktop, closeDesktop,
  setActiveDesktop, onActiveDesktop, onDesktopOf, visibleWindows, surfaceState, overviewShowing, setSnapAssistHook, snapPreview, windowHidden } from '../../../sdk/windows.js';
import { screen, __screenChanged } from '@lordos/runtime';

let checks = 0;
const ok = (name, v) => { checks++; if (!v) throw new Error(`FAIL ${name}`); };
const eq = (name, got, want) => { checks++; const g = JSON.stringify(got), w = JSON.stringify(want); if (g !== w) throw new Error(`FAIL ${name}: got ${g}, want ${w}`); };
const ids = (l) => l.map((w) => w.id);
let fired = 0;
windows.subscribe(() => { fired++; });

configureWindows({ apps: [{ id: 'files', size: [800, 500], minSize: [400, 300] }, { id: 'notes', size: [500, 400] }],
  workArea: (d) => ({ x: d.x, y: d.y + 28, w: d.w, h: d.h - 28 }), snap: true });

// opening
eq('open', wm.open('files'), 'files');
const w1 = wm.get('files');
eq('placed', [w1.x, w1.y, w1.w, w1.h, w1.desktop, w1.minimized], [180, 124, 800, 500, 1, false]);
eq('new window', wm.open('files', { new: true, args: { path: '/tmp' } }), 'files#2');
ok('the first window object is kept (React keeps it)', wm.get('files') === w1);
eq('args with a nonce', [wm.get('files#2').args.path, typeof wm.get('files#2').args.nonce], ['/tmp', 'number']);
eq('unknown app', wm.open('nothing'), null);
eq('visible', ids(useWindows().visible), ['files', 'files#2']);
eq('front', [wm.focused(), useWindows().focused], ['files#2', 'files#2']);
const before = fired;
wm.setTitle('files#2', 'Doc');
ok('one change, one notice', fired === before + 1);
ok('the other window object is kept', wm.get('files') === w1);
eq('title', wm.get('files#2').title, 'Doc');
wm.setTitle('files#2', 'Doc');
ok('no change, no notice', fired === before + 1);

// focus and stacking
wm.focus('files');
eq('focus raises', ids(windows.get()), ['files#2', 'files']);
wm.toBack('files');
eq('to the back', ids(windows.get()), ['files', 'files#2']);
wm.cycle('left');
eq('cycle', ids(windows.get()), ['files#2', 'files']);
eq('pick: the top one of the list, in stacking order', wm.pick(ids(windows.get())), 'files');

// desktops
eq('create', createDesktop(), 2);
eq('list', desktopList.get(), [1, 2]);
wm.moveToDesktop('files', 2);
ok('moved away', !onActiveDesktop(wm.get('files')) && onDesktopOf(wm.get('files'), 2));
eq('hidden', [windowHidden('files'), windowHidden('files#2')], [true, false]);
eq('parked surface state', surfaceState(wm.get('files')), { maximized: false, parked: true, iconic: true });
setActiveDesktop(2);
eq('desktop 2', [activeDesktop.get(), ids(visibleWindows())], [2, ['files']]);
wm.focus('files#2');
eq('focus brings its desktop', activeDesktop.get(), 1);
ok('close desktop', closeDesktop(2));
eq('its windows come back', [wm.get('files').desktop, desktopList.get()], [1, [1]]);
ok('the last stays', !closeDesktop(1));

// snapping, Snap Assist and the end of a drag in C
let offered = null;
setSnapAssistHook((a) => { offered = a; });
wm.snap('files#2', 'left');
eq('snapped', [wm.get('files#2').snapped, wm.get('files#2').x, wm.get('files#2').w], ['left', 0, 640]);
wm.snapKey('files', 'right');
eq('Super+Right from a free window offers the rest', [wm.get('files').snapped, offered && offered.zone], ['right', 'right']);
wm.snap('files#2', 'restore');
eq('restored', [wm.get('files#2').snapped, wm.get('files#2').w], [null, 800]);
globalThis.lord.wm = { grab: () => true, pointer: () => ({ x: 2, y: 300 }), cancel() {} };
wm.frameRef('files#2', 0)({ id: 77 });
// the button is held for a drag only from a real pointer: beginMove needs lord.wm.grab to accept
ok('drag starts', wm.beginMove('files#2'));
globalThis.__lordWm('zone', { zone: 'left', x: 0, y: 28, w: 640, h: 772 });
eq('preview', snapPreview.get() && snapPreview.get().zone, 'left');
offered = null;
globalThis.__lordWm('end', { node: 77, edges: 0, dx: -300, dy: 0, dw: 0, dh: 0, zone: 'left', links: [] });
eq('dropped on the left edge', [wm.get('files#2').snapped, snapPreview.get(), offered && offered.zone, offered && offered.display === screen.displays[0]], ['left', null, 'left', true]);
// REGRESSION: a snapped or maximized window comes loose at its own size as soon as it moves (grab.c),
// and lands where C left it; it used to keep the snapped size until released, and a maximized one
// couldn't be grabbed in C at all (Flow's fallback threw it away from the pointer)
let grabOpts = null;
globalThis.lord.wm.grab = (node, edges, o) => { grabOpts = o; return true; };
const own = wm.get('files#2').restore;
ok('a snapped window grabs', wm.beginMove('files#2'));
eq('…with its own size to come loose at', grabOpts.detach, { w: own.w, h: own.h });
globalThis.__lordWm('end', { node: 77, edges: 0, dx: 200, dy: 50, dw: own.w - 640, dh: own.h - 772, zone: null, detached: true, links: [] });
eq('dropped free at its own size', [wm.get('files#2').snapped, wm.get('files#2').x, wm.get('files#2').y, wm.get('files#2').w, wm.get('files#2').h],
  [null, 200, 78, own.w, own.h]);
wm.toggleMaximize('files#2');
ok('a maximized window grabs to move…', wm.beginMove('files#2'));
eq('…coming loose at its own size', grabOpts.detach, { w: own.w, h: own.h });
globalThis.__lordWm('end', { node: 77, edges: 0, dx: 0, dy: 0, dw: 0, dh: 0, zone: null, detached: false, links: [] });
ok('a click on its bar keeps it maximized', wm.get('files#2').maximized);
ok('but not to resize', !wm.beginResize('files#2', 8));
wm.toggleMaximize('files#2');
globalThis.lord.wm.grab = () => true;

// maximize, fullscreen, picture-in-picture, the keyboard's lift
wm.toggleMaximize('files');
ok('maximized', wm.get('files').maximized);
wm.setFullscreen('files', true);
ok('fullscreen', wm.get('files').fullscreen && !ids(useWindows().mounted).includes('files'));
ok('escape', wm.exitFullscreen() && !wm.get('files').fullscreen);
wm.setPip('files#2', { x: 10, y: 10, w: 200, h: 120 });
ok('pip on every desktop', onDesktopOf(wm.get('files#2'), 1) && !ids(useWindows().visible).includes('files#2'));
wm.setPip('files#2', null);
wm.liftForKeyboard('files#2', { y: 40 });
wm.dropFromKeyboard('files#2');
eq('back from the keyboard', wm.get('files#2').oskFrame, null);
wm.setArgs('files#2', { csd: true });
eq('args merged', [wm.get('files#2').args.csd, wm.get('files#2').args.path], [true, '/tmp']);
const argsBefore = wm.get('files#2').args, winBefore = wm.get('files#2');
wm.move('files#2', 300, 300);
ok('a moved window is a new object…', wm.get('files#2') !== winBefore);
ok('…but keeps its args object (no effect fires for nothing)', wm.get('files#2').args === argsBefore);
wm.setArgs('files#2', { csd: true });
ok('the same args again change nothing', wm.get('files#2').args === argsBefore);

// the overview's views
overviewShowing.set(true);
ok('overview on', overviewShowing.get() === true);
overviewShowing.set(false);

// REGRESSION: a subscriber that changes the windows while the stores update (the Overview closing
// itself when the desktop changes) wins; the older state must not come back over it
createDesktop();
overviewShowing.set(true);
const off = activeDesktop.subscribe(() => { if (overviewShowing.get()) overviewShowing.set(false); });
setActiveDesktop(2);
off();
eq('the nested change stays', [overviewShowing.get(), activeDesktop.get()], [false, 2]);
ok('the mirror matches the list', windows.get().every((w) => w === wm.get(w.id)));
setActiveDesktop(1);
closeDesktop(2);

// a display unplugged: the window comes back on the one left
__screenChanged([{ id: 0, name: 'A', x: 0, y: 0, w: 1280, h: 800, scale: 100, main: true }, { id: 1, name: 'B', x: 1280, y: 0, w: 1920, h: 1080 }]);
wm.move('files#2', 2000, 100);
eq('on the second display', wm.get('files#2').x, 2000);
__screenChanged([{ id: 0, name: 'A', x: 0, y: 0, w: 1280, h: 800, scale: 100, main: true }]);
ok('back on the first', wm.get('files#2').x + wm.get('files#2').w <= 1280);

// a state that never reached the mirror (lost): the next one finds it out and asks for everything again
JSON.parse(lord.windowList.call('patch', JSON.stringify([{}, 'files#2', { title: 'Lost' }])));
wm.minimize('files');
eq('resynced', [wm.get('files#2').title, wm.get('files').minimized], ['Lost', true]);
ok('the mirror matches the list after a resync', windows.get().every((w) => w === wm.get(w.id)) && windows.get().length === 2);

// the mirror is read-only: a write from outside is refused, not half-applied
const was = windows.get();
windows.set([]);
activeDesktop.set(7);
ok('read-only mirror', windows.get() === was && activeDesktop.get() === 1);

// closing
ok('close', wm.close('files#2'));
eq('closed', [ids(windows.get()), wm.get('files#2')], [['files'], null]);
console.log(`windows bridge: ${checks} checks ok`);
