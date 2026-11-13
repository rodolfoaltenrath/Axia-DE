// Node-only transport for the production Rust SMART model. The desktop uses
// lord.launcher directly; no JS policy fallback is installed in production.
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('../../', import.meta.url));
const target = path.join(root, 'build/sdk-rust');
const binary = process.env.AXIA_SMART_MODEL_BIN || path.join(target, 'debug/examples/smart-model');
if (!globalThis.lord?.launcher) {
  if (!process.env.AXIA_SMART_MODEL_BIN) execFileSync(process.env.CARGO || 'cargo', [
    'build', '--manifest-path', path.join(root, 'rust/Cargo.toml'), '--example', 'smart-model',
    '--target-dir', target, '--locked', '--offline',
  ], { cwd: root, stdio: ['ignore', 'ignore', 'inherit'] });
  globalThis.lord = { ...globalThis.lord, launcher: { request(input) {
    const result = spawnSync(binary, [], { input: input + '\n', encoding: 'utf8', maxBuffer: 8 << 20 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr || `SMART model exited ${result.status}`);
    return JSON.parse(result.stdout);
  } } };
}
