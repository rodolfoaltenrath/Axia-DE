//! Contract assertions are Rust; Node evaluates the actual JavaScript loader.
use serde_json::{json, Value};
use std::{path::Path, process::Command};

#[test]
#[ignore = "requires Node; explicitly required by desktop CI after npm ci"]
fn isolated_and_transitional_loaders_preserve_the_versioned_module_contract() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("runtime/lpk-api.js");
    // Only drive and serialize the actual module here; assertions stay below.
    let fixture = r#"
import fs from 'node:fs';
const { createLpkApi, LPK_API_VERSION } = await import('data:text/javascript;base64,' + fs.readFileSync(process.argv[1]).toString('base64'));
const react = {}, jsx = {}, runtime = {}, sdk = { state: 1 }, native = {};
const modules = { react, 'react/jsx-runtime': jsx, '@lordos/runtime': runtime, '@lordos/sdk': sdk };
const legacy = createLpkApi(modules);
const isolated = createLpkApi({ ...modules, '@lordos/nativeapps': native });
const names = Object.keys(modules);
const identities = names.map(name => legacy.require(name) === modules[name] && isolated.require(name) === modules[name]);
sdk.state = 2;
modules['@lordos/sdk'] = {};
const sharedState = [legacy.require('@lordos/sdk').state, isolated.require('@lordos/sdk').state];
const rejected = [];
for (const name of ['constructor', '__proto__', 'toString', 'missing', '', null, 1]) {
  try { isolated.require(name); rejected.push(false); } catch { rejected.push(true); }
}
let legacyNativeRejected = false;
try { legacy.require('@lordos/nativeapps'); } catch { legacyNativeRejected = true; }
try { isolated.apiVersion = 99; } catch {}
let deleted = false;
try { deleted = delete isolated.apiVersion; } catch {}
console.log(JSON.stringify({ version: LPK_API_VERSION, legacyVersion: legacy.apiVersion, isolatedVersion: isolated.apiVersion,
  identities, sharedState, rejected, legacyNativeRejected, nativeIdentity: isolated.require('@lordos/nativeapps') === native, deleted }));
"#;
    let result = Command::new("node")
        .args(["--input-type=module", "-e", fixture])
        .arg(source)
        .output()
        .expect("Node is required for the LPK API contract check");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["legacyVersion"], report["version"]);
    assert_eq!(report["isolatedVersion"], report["version"]);
    assert_eq!(report["identities"], json!([true, true, true, true]));
    assert_eq!(report["sharedState"], json!([2, 2]));
    assert_eq!(
        report["rejected"],
        json!([true, true, true, true, true, true, true])
    );
    assert_eq!(report["legacyNativeRejected"], true);
    assert_eq!(report["nativeIdentity"], true);
    assert_eq!(report["deleted"], false);
}

#[test]
#[ignore = "requires Node and npm ci; explicitly required by desktop CI"]
fn shell_excludes_react_surfaces_while_lpk_hosts_retain_them() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let fixture = r#"
import path from 'node:path';
import { build } from 'esbuild';
const root = process.argv[1], reports = [];
for (const appHost of [false, true]) {
  const result = await build({ entryPoints: [path.join(root, 'sdk/index.js')], bundle: true, write: false,
    minify: true, format: 'esm', jsx: 'automatic', metafile: true,
    external: ['react', 'react/jsx-runtime', '@lordos/runtime'],
    alias: { '@lordos/sdk': path.join(root, 'sdk/index.js') },
    define: { 'process.env.NODE_ENV': '"production"', __LORD_STANDALONE__: 'true', __AXIA_APP_HOST__: String(appHost) } });
  const inputs = Object.values(result.metafile.outputs)[0].inputs;
  const bytes = suffix => Object.entries(inputs).filter(([name]) => name.replaceAll('\\', '/').endsWith(suffix))
    .reduce((total, [, input]) => total + input.bytesInOutput, 0);
  reports.push({ appHost, reactMenu: bytes('runtime/lpk/menu.jsx'), nativeMenu: bytes('sdk/nativemenu.jsx'),
    reactBanner: bytes('runtime/lpk/notifications.js'), nativeBanner: bytes('sdk/nativebanner.js'),
    runtimeFlag: ['menuNativeView','notificationsNativeView','dockNativeView','spotlightNativeView'].some(flag => result.outputFiles[0].text.includes(flag)) });
}
console.log(JSON.stringify(reports));
"#;
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "-e", fixture])
        .arg(repo)
        .output()
        .expect("Node and esbuild are required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0]["reactMenu"], 0, "React menu leaked into shell");
    assert!(reports[0]["nativeMenu"].as_u64().unwrap() > 0);
    assert!(reports[1]["reactMenu"].as_u64().unwrap() > 0);
    assert_eq!(
        reports[0]["reactBanner"], 0,
        "React banner leaked into shell"
    );
    assert!(reports[0]["nativeBanner"].as_u64().unwrap() > 0);
    assert!(reports[1]["reactBanner"].as_u64().unwrap() > 0);
    for report in reports {
        assert_eq!(report["runtimeFlag"], false);
    }
}
