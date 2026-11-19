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
