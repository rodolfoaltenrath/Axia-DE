// Files without a screen: what an address means and how columns move. node server/tests/files_model_test.mjs
import { clampSide, SIDE_MIN, SIDE_MAX, viewFor, viewKey, folderViewsWith } from '../../apps/files/viewprefs.js';
import { parseAddress, sshLine, remoteUri, hostAddress } from '../../apps/files/connect.js';
import { normalColumns, visible, toggleColumn, moveColumn, resize, dropBefore, totalWidth, MIN_W, MAX_W } from '../../apps/files/columns.js';
import { resolveUserDirs } from '../../apps/files/userdirs.js';

let ok = 0;
const eq = (name, a, b) => { if (JSON.stringify(a) !== JSON.stringify(b)) { console.log(`FAIL ${name}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`); process.exitCode = 1; } else ok++; };
const p = (s) => parseAddress(s);
// Fedora accounts without user-dirs.dirs, translated names, and custom XDG
// paths must all produce usable sidebar places. An explicit HOME disables one.
const home = '/home/ana';
const dirs = (config, paths) => resolveUserDirs(home, config, (path) => paths.includes(path));
eq('standard folders without XDG', dirs('', [home + '/Downloads', home + '/Documents']).map((d) => d.label), ['downloads', 'documents']);
eq('Portuguese folder names', dirs('', [home + '/Área de trabalho', home + '/Documentos', home + '/Imagens']).map((d) => d.label), ['documents', 'desktop', 'pictures']);
eq('nested custom folder', dirs('XDG_DOCUMENTS_DIR="$HOME/Work/Documents"', [home + '/Work/Documents']), [{ label: 'documents', path: home + '/Work/Documents' }]);
eq('absolute custom folder', dirs('XDG_DOWNLOAD_DIR="/run/media/ana/Data/Downloads"', ['/run/media/ana/Data/Downloads']), [{ label: 'downloads', path: '/run/media/ana/Data/Downloads' }]);
eq('HOME disables the shortcut', dirs('XDG_DOCUMENTS_DIR="$HOME"', [home, home + '/Documents']), []);
eq('missing configured folder is not guessed', dirs('XDG_DOCUMENTS_DIR="$HOME/Removed"', [home + '/Documents']), []);
eq('configured file is not a directory', dirs('XDG_DOCUMENTS_DIR="$HOME/file.txt"', []), []);
eq('spacing and comments', dirs('  XDG_DOCUMENTS_DIR = "$HOME/My Docs" # custom', [home + '/My Docs']), [{ label: 'documents', path: home + '/My Docs' }]);
eq('quoted folder name', dirs('XDG_DOCUMENTS_DIR="$HOME/My \\"Docs\\""', [home + '/My "Docs"']), [{ label: 'documents', path: home + '/My "Docs"' }]);
eq('absolute path within home', dirs('XDG_DOCUMENTS_DIR="/home/ana/Documentos"', [home + '/Documentos']), [{ label: 'documents', path: home + '/Documentos' }]);
eq('relative configured path refused', dirs('XDG_DOCUMENTS_DIR="Documents"', [home + '/Documents']), []);
eq('configuration comment ignored', dirs('# XDG_DOCUMENTS_DIR="$HOME"', [home + '/Documents']), [{ label: 'documents', path: home + '/Documents' }]);
eq('shell text remains a literal path', dirs('XDG_DOCUMENTS_DIR="$HOME/$(not-executed)"', [home + '/$(not-executed)']), [{ label: 'documents', path: home + '/$(not-executed)' }]);
eq('default smb', [p('192.168.1.5').scheme, p('192.168.1.5').kind, p('192.168.1.5').share], ['smb', 'mount', '']);
eq('share', p('smb://nas/Public/sub').share, 'Public');
eq('nfs path', p('nfs://10.0.0.1/srv/data').path, '/srv/data');
eq('vnc', [p('vnc://127.0.0.1:5969').kind, p('vnc://127.0.0.1:5969').port], ['remote', 5969]);
eq('rdp uri', remoteUri(p('rdp://ana@host:3390')), 'rdp://ana@host:3390');
eq('ssh line', sshLine(p('ssh://ana@host:2222')), 'ssh -p 2222 ana@host');
eq('sftp line', sshLine(p('sftp://host')), 'sftp host');
eq('ftp folder', p('ftp://host').kind, 'folder');
eq('ssh folder', p('ssh://ana@h').kind, 'folder');
eq('web', p('https://host/a').kind, 'web');
for (const bad of ['ssh://-oProxyCommand=x', 'ssh://a;rm@host', 'vnc://$(x)', 'smb://a b', 'nfs://h/../etc', 'ssh://host:99999', 'ssh://-x@host', 'smb://h/a\\b', 'x`y', 'smb://h/a|b']) eq(`refused ${bad}`, !!p(bad).error, true);
eq('unknown scheme', p('gopher://h').error, 'scheme');
eq('empty', p('  ').error, 'empty');
const hosts = [{ addr: '10.0.0.2', name: 'NAS', services: [] }, { addr: '10.0.0.3', name: '', services: [{ name: 'Office' }] }];
eq('names', [hostAddress(hosts, 'nas.local'), hostAddress(hosts, 'office'), hostAddress(hosts, 'x'), hostAddress(hosts, '10.1.1.1')], ['10.0.0.2', '10.0.0.3', null, '10.1.1.1']);

let c = normalColumns(null);
eq('default', visible(c), ['date', 'size', 'kind']);
c = toggleColumn(c, 'owner');
eq('added', visible(c), ['date', 'size', 'kind', 'owner']);
c = moveColumn(c, 'kind', 'date');
eq('moved', visible(c), ['kind', 'date', 'size', 'owner']);
c = moveColumn(c, 'kind', null);
eq('to the end', visible(c).pop(), 'kind');
eq('clamped', [resize(c, 'size', 1).width.size, resize(c, 'size', 1e6).width.size], [MIN_W, MAX_W]);
eq('stored junk', visible(normalColumns({ order: ['x', 'size', 'size'], shown: ['nope', 'size'], width: { size: 'a' } })), ['size']);
const d = normalColumns(null);
eq('drop', [dropBefore(d, 10), dropBefore(d, 200), dropBefore(d, 900)], ['date', 'kind', null]);
eq('total', totalWidth(d), 350);
// each place keeps its view and sort; a place with none takes the default; odd values don't count
{
  const eq = (a, b) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${JSON.stringify(a)} != ${JSON.stringify(b)}`); };
  const l = { view: 'list', sort: { by: 'date', desc: true } };
  const p = { folderViews: folderViewsWith({}, '/a', l), defaultView: { view: 'columns', sort: { by: 'name', desc: false } } };
  eq(viewFor(p, { type: 'dir', path: '/a' }), l);
  eq(viewFor(p, { type: 'dir', path: '/b' }).view, 'columns');
  eq(viewFor({ folderViews: {}, defaultView: null }, { type: 'dir', path: '/b' }).view, 'icons');
  eq(viewFor({ folderViews: { '/a': { view: 'evil', sort: {} } }, defaultView: null }, { type: 'dir', path: '/a' }).view, 'icons');
  eq(viewKey({ type: 'network' }), ':network'); eq(viewKey({ type: 'search' }), null);
  let m = {};
  for (let i = 0; i < 405; i++) m = folderViewsWith(m, `/f${i}`, l);
  eq(Object.keys(m).length, 400); eq(!!m['/f0'], false);
  eq([clampSide(10), clampSide(9999), clampSide(220), clampSide('x')], [SIDE_MIN, SIDE_MAX, 220, 200]);
}
console.log(`files-model-test: ${ok} ok`);
