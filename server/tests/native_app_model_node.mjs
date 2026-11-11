// Node-only adapter for suites importing an app whose policy now lives in Rust.
// Build: cargo build --manifest-path rust/Cargo.toml --locked --offline --example app-model
// Run: node --import ./tests/native_app_model_node.mjs tests/sysinfo_model_test.mjs
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const binary = process.env.AXIA_APP_MODEL_BIN || fileURLToPath(new URL('../../rust/target/debug/examples/app-model', import.meta.url));
globalThis.lord = { nativeApps: { create: app => ({
  request(text) {
    const result = spawnSync(binary, [], { input: JSON.stringify({ app, input: JSON.parse(text) }) + '\n', encoding: 'utf8', maxBuffer: 4 << 20 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr || `Rust model exited ${result.status}`);
    return JSON.parse(result.stdout);
  },
}) } };
