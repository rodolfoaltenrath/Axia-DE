// Shell mount for the existing Rust file dialog. Native apps supply the title
// and filter and receive the chosen path; this module owns no browser policy.
import { createElement as h } from 'react';
import { createStore, useStore } from '../sdk/store.js';
import { RustFileDialog } from '../sdk/rustfiles.js';
const pending = createStore([]);
let sequence = 0;
export function pickFile(options, owner) {
  return new Promise(resolve => {
    const id = ++sequence;
    const done = path => { pending.set(rows => rows.filter(row => row.id !== id)); resolve(path); };
    owner.cleanup(() => done(null));
    pending.set(rows => [...rows, { id, owner: owner.id, options, done }]);
  });
}
export function NativeFilePicker({ owner }) {
  const rows = useStore(pending), request = rows.find(row => row.owner === owner);
  if (!request) return null;
  const { extensions, ...props } = request.options;
  const filter = extensions ? new RegExp('\\.(' + extensions.map(extension => extension.replace(/[^A-Za-z0-9]/g, '')).join('|') + ')$', 'i') : null;
  return h(RustFileDialog, { ...props, filter, onPick: request.done, onClose: () => request.done(null) });
}
