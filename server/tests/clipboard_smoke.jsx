// Real C capture -> Rust worker -> eventfd -> QuickJS projection. Private test HOME.
import { createElement as h, useEffect } from 'react';
import { startShell, View, settings } from '@lordos/sdk';
import { clipboard, clipboardHistory, clipboardKind, ClipboardPanel, toggleClipboardPanel, closeClipboardPanel } from '../../sdk/clipboard.js';
const check = (ok, message) => { if (!ok) throw new Error(`clipboard smoke: ${message}`); };
const pause = (ms = 30) => new Promise((resolve) => setTimeout(resolve, ms));
const wait = async (test, message) => {
  const end = Date.now() + 4000;
  while (!test()) { check(Date.now() < end, message); await pause(10); }
};
let lastView = null, historyUpdates = 0;
const receive = globalThis.__axiaClipboardHistory;
globalThis.__axiaClipboardHistory = (reply) => { if (reply.view) lastView = reply.view; receive(reply); };
clipboardHistory.subscribe(() => historyUpdates++);
const find = (text) => clipboardHistory.get().find((i) => i.text === text);
async function copy(text, options) { clipboard.set(text, options); await wait(() => find(text), 'native copy reaches worker'); }
async function exercise() {
  settings.set({ clipboard: { save: true, max: 3, maxMB: 200, expireDays: 30, ignoreApps: ['secret-app'], clearOnLogout: false } });
  await wait(() => find('Olá, histórico antigo 👋'), 'legacy history imported');
  const original = find('Olá, histórico antigo 👋');
  check(original.id === 'legacy-text' && original.pinned, 'existing id, pin and Unicode preserved');
  const image = clipboardHistory.get().find((i) => i.id === 'legacy-image');
  check(image?.image && image.type === 'image', 'legacy image imported');
  check(clipboard.use(image), 'saved PNG re-copies through actual Wayland clipboard');
  await pause();
  await copy('copy directly after image');
  check(clipboardHistory.get().filter((i) => i.image).length === 1, 're-copy does not duplicate image history');
  clipboard.set('private password fixture', { secure: true });
  await pause();
  clipboard.remember('private password fixture'); await pause();
  check(!find('private password fixture'), 'passwords never reach history');
  clipboard.set('ignored fixture', { origin: 'secret-app' }); await pause();
  clipboard.remember('ignored fixture'); await pause();
  check(!find('ignored fixture'), 'ignored app is not stored');
  await copy('normal copy after password');
  await copy('https://axia.example/férias');
  check(find('https://axia.example/férias').type === 'link' && clipboardKind({ text: '#aabbcc' }) === 'color', 'classification uses Rust');
  const link = find('https://axia.example/férias');
  const updatesBeforeView = historyUpdates;
  lord.clipboard.historyRequest(JSON.stringify({ op: 'view', query: 'https://', filter: 'link', limit: 12, generation: 900 }));
  await wait(() => lastView?.generation === 900, 'worker prepares query projection');
  check(lastView.ids.length === 1 && lastView.ids[0] === link.id && lastView.counts.all > 0, 'query, filter and counts come from Rust');
  check(historyUpdates === updatesBeforeView, 'query reply does not republish history');
  check(clipboard.use(link), 're-copy text'); await pause();
  check(find(link.text).id === link.id, 're-copy retains identity');
  clipboard.pin(link); await wait(() => find(link.text)?.pinned, 'pin roundtrip');
  for (const text of ['A', 'B', 'C', 'D']) await copy(text);
  check(find(link.text)?.pinned && find(original.text)?.pinned, 'limits preserve pinned items');
  check(!find('A'), 'old ordinary items evicted');
  clipboard.clear(); await wait(() => clipboardHistory.get().every((i) => i.pinned), 'clear retains pins');
  for (let i = 0; i < 12; i++) { toggleClipboardPanel(i % 2 ? 'full' : 'quick'); await pause(5); closeClipboardPanel(); }
  clipboard.beforeLogout();
  let disk = JSON.parse(lord.fs.read('.local/share/axia/clipboard/history.json'));
  check(disk.version === 1 && disk.items.some((i) => i.id === 'legacy-text'), 'compatible durable format');
  settings.set({ clipboard: { ...settings.get().clipboard, save: false } }); clipboard.beforeLogout(); await pause();
  disk = JSON.parse(lord.fs.read('.local/share/axia/clipboard/history.json'));
  check(disk.items.length === 0 && clipboardHistory.get().length > 0, 'saving off clears disk but keeps session history');
  settings.set({ clipboard: { ...settings.get().clipboard, save: true, clearOnLogout: true } });
  await copy('temporary before logout'); clipboard.beforeLogout();
  disk = JSON.parse(lord.fs.read('.local/share/axia/clipboard/history.json'));
  check(disk.items.length > 0 && disk.items.every((i) => i.pinned), 'logout clears only unpinned history');
  clipboard.clear({ keepPinned: false }); await wait(() => clipboardHistory.get().length === 0, 'clear all');
  console.log('clipboard integration: passed'); lord.quit();
}
function App() {
  useEffect(() => { exercise().catch((error) => { console.error(String(error)); lord.quit(); }); }, []);
  return h(View, { style: { width: '100%', height: '100%' } }, h(ClipboardPanel));
}
startShell(h(App));
