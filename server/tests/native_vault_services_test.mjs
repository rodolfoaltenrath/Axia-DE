// The Passwords transport owns descriptors, not secrets: a vault request takes only the
// files its own window picked, each at most once, and closing the window closes the rest.
import assert from 'node:assert/strict';
import { vaultServices } from '../../runtime/native_vault_services.js';

const closed = [], written = [], secrets = [], texts = [], requests = [], permissions = [], attached = [];
let nextFd = 40, pickReply = null, fidoAllowed = true;
globalThis.lord = {
  broker: {
    pick: (save, suggest) => pickReply ?? { fd: nextFd++, name: `${suggest || 'file'}${save ? '.save' : ''}` },
    fdClose: fd => closed.push(fd),
    fdWrite: (fd, text) => { written.push([fd, text]); return fd !== 99; },
  },
  vault: { fidoAttach: app => { attached.push(app); return { ok: true }; } },
  clipboard: { set: (value, options) => secrets.push([value, options]) },
};
const owners = new Map();
const owner = id => owners.get(id) ?? (owners.set(id, { id, cleanups: [], emitted: [],
  cleanup(fn) { this.cleanups.push(fn); }, emit(event) { this.emitted.push(event); } }), owners.get(id));
const listeners = new Set();
const services = vaultServices({
  vaultRequest: async (op, args, extra) => { requests.push([op, args, extra]); if (op === 'boom') throw Object.assign(new Error('x'), { code: 'vault.locked' }); return { op }; },
  onVaultEvent: fn => { listeners.add(fn); return () => listeners.delete(fn); },
  requestPermission: async (app, perm) => { permissions.push([app, perm]); return fidoAllowed; },
  clipboard: { set: (value, options) => texts.push([value, options]) },
  events: id => ({ queue: id }),
});
const run = (name, args, id) => services[name].run(args, owner(id));

// A request takes the picked file once, and only for the window that picked it.
const first = run('file.pick', [false, 'key'], 1);
assert.deepEqual(first, { id: 1, name: 'key' });
await assert.rejects(() => run('vault.request', ['import', {}, [first.id]], 2), /file/);
assert.deepEqual(await run('vault.request', ['import', { a: 1 }, [first.id]], 1), { op: 'import' });
assert.deepEqual(requests.at(-1), ['import', { a: 1 }, { fds: [40] }]);
await assert.rejects(() => run('vault.request', ['import', {}, [first.id]], 1), /file/);

// A file nobody used is closed with its window; a used one is not closed twice.
const left = run('file.pick', [true, 'backup'], 1);
assert.equal(left.id, 2);
owner(1).cleanups.forEach(fn => fn());
assert.deepEqual(closed, [41], 'only the descriptor still held is closed');

// A cancelled dialog is not an error.
pickReply = { fd: -1 };
assert.equal(run('file.pick', [false, ''], 1), null);
assert.equal(run('file.save', ['recovery', 'secret'], 1), null);
pickReply = null;

// Saving writes the text and always closes the descriptor; a failed write is an error.
assert.equal(run('file.save', ['recovery', 'key text'], 1), true);
assert.deepEqual(written.at(-1), [42, 'key text']);
assert.equal(closed.at(-1), 42);
pickReply = { fd: 99 };
assert.throws(() => run('file.save', ['recovery', 'x'], 1), /file/);
assert.equal(closed.at(-1), 99);
pickReply = null;

// FIDO: the USB permission first, then the key is lent; a refusal never reaches the vault.
await run('vault.request', ['unlock', {}, [], true], 1);
assert.deepEqual(permissions.at(-1), ['passwords', 'usb']);
assert.deepEqual(attached, ['passwords']);
fidoAllowed = false;
const before = requests.length;
await assert.rejects(() => run('vault.request', ['unlock', {}, [], true], 1), /fido-denied/);
assert.equal(requests.length, before);
fidoAllowed = true;

// A vault failure keeps only its code.
await assert.rejects(() => run('vault.request', ['boom'], 1), /vault\.locked/);

// Vault events reach the window's queue until it closes; the queue is the window's own.
run('vault.watch', [], 3);
for (const fn of listeners) fn({ type: 'locked' });
assert.deepEqual(owner(3).emitted, [{ kind: 'vault', type: 'locked' }]);
assert.deepEqual(run('vault.events', [], 3), { queue: 3 });
owner(3).cleanups.forEach(fn => fn());
assert.equal(listeners.size, 0);

// Secrets are marked sensitive and cleared; the plain copy is attributed to the app.
run('clipboard.secret', ['hunter2', 12], 1);
run('clipboard.secret', ['hunter3'], 1);
assert.deepEqual(secrets, [['hunter2', { secure: true, clearAfter: 12 }], ['hunter3', { secure: true, clearAfter: 30 }]]);
run('clipboard.text', ['user@example.test'], 1);
assert.deepEqual(texts, [['user@example.test', { origin: 'passwords' }]]);

console.log('native-vault-services: file ownership, descriptor release, FIDO order, vault events and secret clipboard passed');
