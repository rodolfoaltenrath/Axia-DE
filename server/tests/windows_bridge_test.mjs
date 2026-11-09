// windows_bridge_test.mjs — sdk/windows.js as the session runs it: bundled with esbuild (a minimal
// @lordos/runtime, tests/windows_bridge/runtime_stub.js) and run in QuickJS by build/windows-bridge-host,
// with the real bridge (src/ui/js_winmodel.c) and the real Rust window list. It checks what only the
// three together show: the mirror's stores, kept objects, views, desktops, snapping and C drags.
//
//   make -C server build/windows-bridge-host && node tests/windows_bridge_test.mjs   (from server/)
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
let esbuild = null;
try { esbuild = createRequire(path.join(root, 'package.json'))('esbuild'); } catch (e) { console.log('needs node_modules (esbuild)'); process.exit(1); }
const dir = path.join(here, 'windows_bridge');
const out = path.join(here, '../build/windows-bridge.bundle.js');
await esbuild.build({
  entryPoints: [path.join(dir, 'test.js')], bundle: true, format: 'iife', outfile: out, logLevel: 'error', target: 'es2020',
  alias: { '@lordos/runtime': path.join(dir, 'runtime_stub.js'), react: path.join(dir, 'react_stub.js') }, loader: { '.json': 'json' },
});
const host = path.join(here, '../build/windows-bridge-host');
try {
  process.stdout.write(execFileSync(host, [out], { encoding: 'utf8' }).split('\n').filter((l) => /checks ok|FAIL/.test(l)).join('\n') + '\n');
} catch (e) {
  process.stderr.write(String(e.stdout || '') + String(e.stderr || ''));
  process.exit(1);
}
