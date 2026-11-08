// Broker permissions/descriptor transport. All picker UI/state belongs to Rust.
import { createElement as h } from 'react';
import { createStore, useStore } from './store.js';
import { useLocale, appName } from './i18n.js';
import { getApp } from './windows.js';
import { RustFileDialog, filesRequest } from './rustfiles.js';
const queue = createStore([]);
let sequence = 0;
globalThis.__lordBrokerPick = (reqid, app, save, suggest) => {
  queue.set((q) => [...q, { key: ++sequence, reqid, app, save: !!save, suggest: suggest || '' }]);
};
globalThis.__axiaPortalPick = (request) => {
  const native = filesRequest({ op: 'externalPrepare', ...request });
  queue.set((q) => [...q, { key: ++sequence, external: true, request, native }]);
};
globalThis.__axiaPortalClose = (id) => queue.set((q) => q.filter((x) => !x.external || x.request.id !== id));
function finish(item, path) {
  queue.set((q) => q.filter((x) => x.key !== item.key));
  if (item.external) {
    const paths = path ? (Array.isArray(path) ? path : [path]) : [];
    try {
      const { uris } = filesRequest({ op: 'externalUris', paths, save: item.request.mode === 'save', files: item.native.files });
      lord.portal.pickResult(item.request.id, uris);
    } catch (error) {
      lord.portal.pickResult(item.request.id, []);
      console.log(`File chooser: ${error.message}`);
    }
  } else lord.broker.pickResult(item.reqid, path || '');
}
export function Powerbox() {
  const q = useStore(queue);
  const { t } = useLocale();
  const item = q[0];
  if (!item) return null;
  if (item.external) {
    const native = item.native;
    return h(RustFileDialog, { key: item.key, ...native, filter: native.filter ? new RegExp(native.filter) : null,
      onPick: (path) => finish(item, path), onClose: () => finish(item, '') });
  }
  return h(RustFileDialog, { key: item.key, mode: item.save ? 'save' : 'open', name: item.suggest,
    title: t(item.save ? 'powerbox.saveTitle' : 'powerbox.openTitle', { app: appName(getApp(item.app)) || item.app }),
    onPick: (path) => finish(item, path), onClose: () => finish(item, '') });
}
const inApp = () => typeof lord !== 'undefined' && lord.broker && typeof lord.broker.pick === 'function';
export async function openFile() {
  if (!inApp()) throw new Error('the file dialog only exists in an isolated app');
  const r = lord.broker.pick(false, '');
  if (!r) return null;
  try { const data = lord.broker.fdRead(r.fd); return { name: r.name, arrayBuffer: () => data, text: () => new TextDecoder().decode(data) }; }
  finally { lord.broker.fdClose(r.fd); }
}
export async function saveFile(suggestedName, data) {
  if (!inApp()) throw new Error('the file dialog only exists in an isolated app');
  const r = lord.broker.pick(true, suggestedName || 'arquivo.txt');
  if (!r) return false;
  try { const buf = typeof data === 'string' ? new TextEncoder().encode(data).buffer : data; return { name: r.name, ok: lord.broker.fdWrite(r.fd, buf) }; }
  finally { lord.broker.fdClose(r.fd); }
}
