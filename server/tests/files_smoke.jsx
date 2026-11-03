// Real QuickJS -> Rust -> document-helper integration in a private test home.
// Only disk facts/volume replies are simulated: no host disk operation is sent.
import { createElement as h, useEffect, useState } from 'react';
import { startShell } from '../../sdk/index.js';
import { View } from '@lordos/runtime';
import { ThemeProvider } from '../../sdk/theme.js';
import { RustFiles } from '../../sdk/rustfiles.jsx';
import { makeTextPdf, saveBlob, releaseBlob } from '../../sdk/doc.js';
const check = (ok, message) => { if (!ok) throw new Error(`files smoke: ${message}`); };
const wait = async (test, message) => {
  const until = Date.now() + 5000;
  while (!test()) { check(Date.now() < until, message); await new Promise((r) => setTimeout(r, 20)); }
};
const find = (node, test) => !node ? null : test(node) ? node : (node.children || []).map((n) => find(n, test)).find(Boolean);
let handle = 0, latest = null, action = null, facts = [], show;
const bound = new Set(), released = new Set(), volumeCalls = [];
const original = { request: lord.files.request, list: lord.disks.list, bind: lord.doc.bind, release: lord.doc.release };
lord.files.request = (raw) => {
  let request = JSON.parse(raw);
  if (request.op === 'volumeStart') {
    check((request.action === 'mount' && request.device === 'sdb1') || (request.action === 'release' && request.device === 'sdb'), 'only expected simulated disk requests');
    volumeCalls.push(request.action); facts = request.action === 'mount' ? disk(true) : [];
    return { job: request.action === 'mount' ? 1 : 2 };
  }
  if (request.op === 'volumePoll') return { done: true, result: request.job === 1 ? devicePath : null };
  if (request.op === 'poll' && request.id === handle && action) {
    request = { ...request, op: 'event', action }; action = null;
  }
  const result = original.request(JSON.stringify(request));
  if (request.op === 'create') handle = result.id;
  if (result?.tree) latest = result;
  return result;
};
lord.disks.list = () => facts;
lord.doc.bind = (node, image) => { if (image) { check(typeof image === 'number', 'document image handle is numeric'); bound.add(image); } return original.bind(node, image); };
lord.doc.release = (image) => { released.add(image); return original.release(image); };
const home = lord.fs.home();
const devicePath = `${home}/files-device`;
const disk = (mounted) => [{ name: 'sdb', removable: true, transport: 'usb', system: false,
  partitions: [{ name: 'sdb1', fstype: 'exfat', label: 'USB integration', mountpoint: mounted ? devicePath : '' }] }];
async function send(next) {
  const serial = latest.serial; action = next;
  await wait(() => !action && latest.serial !== serial, `action ${next[0]} reaches Rust`);
}
async function prepare() {
  lord.fs.mkdir('files-integration'); lord.fs.mkdir('files-device');
  lord.fs.write('files-device/note.txt', 'Private simulated USB contents');
  const pdf = await makeTextPdf({ text: 'Axia PDF thumbnail integration', title: 'Preview' });
  try { saveBlob(pdf.blob, `${home}/files-integration/preview.pdf`); } finally { releaseBlob(pdf.blob); }
}
async function exercise() {
  await wait(() => latest?.tree && !latest.loading, 'Files listing loaded');
  const tile = () => find(latest.tree, (n) => n.type === 'documentImage' && n.style.width === 56 && bound.has(n.image?.id));
  await wait(tile, 'first PDF page appears as an actual bound thumbnail');
  const tileId = tile().image.id;
  const cell = find(latest.tree, (n) => n.events?.onClick?.[0] === 'select' && JSON.stringify(n.children).includes('preview.pdf'));
  check(cell, 'PDF file can be selected');
  await send(cell.events.onClick); await send(['preview']);
  const preview = () => find(latest.tree, (n) => n.type === 'documentImage' && n.style.width > 128 && bound.has(n.image?.id));
  await wait(preview, 'PDF Quick Preview binds the rendered page');
  const previewId = preview().image.id;
  await send(['cancelDialog']); await wait(() => released.has(previewId), 'closing PDF preview releases the page');
  facts = disk(false); globalThis.__lordDisks('change');
  await wait(() => find(latest.tree, (n) => n.events?.onClick?.[0] === 'volumeOpen'), 'hotplug updates the sidebar through useDisks');
  await send(['volumeOpen', 'sdb1']);
  await wait(() => latest.title === 'files-device' && !latest.loading, 'mount result navigates to the volume');
  check(volumeCalls.join() === 'mount', 'mount uses the native closed action');
  check(JSON.stringify(latest.tree).includes('note.txt'), 'mounted files are listed');
  await send(['volumeRelease', 'sdb1']);
  await wait(() => latest.title?.key === 'files.place.home', 'eject returns to Home');
  check(volumeCalls.join() === 'mount,release', 'eject uses the whole USB disk');
  check(!find(latest.tree, (n) => n.events?.onClick?.[0] === 'volumeOpen'), 'ejected device leaves the sidebar');
  show(false); await wait(() => released.has(tileId), 'closing Files releases cached PDF images');
  console.log('files integration: passed (PDF tile/preview/release, hotplug/mount/eject)');
}
function Harness() {
  const [ready, setReady] = useState(false); show = setReady;
  useEffect(() => {
    prepare().then(() => { setReady(true); return exercise(); }).catch((e) => console.log(e.message)).finally(() => {
      lord.files.request = original.request; lord.disks.list = original.list;
      lord.doc.bind = original.bind; lord.doc.release = original.release;
    });
  }, []);
  return h(ThemeProvider, null, h(View, { style: { flex: 1 } }, ready ? h(RustFiles, { args: { path: `${home}/files-integration` } }) : null));
}
startShell(h(Harness));
