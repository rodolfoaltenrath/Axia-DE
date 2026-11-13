// Test-only transport. A model history is replayed by the same production Rust
// source in a subprocess; there is no fallback implementation of PDF policy.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('../../', import.meta.url));
const target = path.join(root, 'build/sdk-rust');
const binary = process.env.AXIA_PDF_MODEL_BIN || path.join(target, 'debug/examples/pdf-model');
if (!process.env.AXIA_PDF_MODEL_BIN) execFileSync(process.env.CARGO || 'cargo', [
  'build', '--manifest-path', path.join(root, 'rust/Cargo.toml'), '-p', 'axia-shell',
  '--example', 'pdf-model', '--target-dir', target, '--locked', '--offline',
], { cwd: root, stdio: ['ignore', 'ignore', 'inherit'] });
export const requests = [];
globalThis.lord = { ...globalThis.lord, nativeApps: { create(id) {
  assert.equal(id, 'pdf');
  const history = [];
  return { request(text) {
    const value = JSON.parse(text);
    requests.push(value);
    const result = spawnSync(binary, [], { input: JSON.stringify([...history, value]) + '\n', encoding: 'utf8', maxBuffer: 8 << 20 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr || `PDF model exited ${result.status}`);
    if (['cache', 'layout', 'view', 'viewer', 'edit', 'sidebar'].includes(value.cmd)) history.push(value);
    return JSON.parse(result.stdout);
  } };
} } };
