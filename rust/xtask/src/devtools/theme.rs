//! Bundles the shell themes, the isolated-app host and the system apps with esbuild, and the
//! integration-test bundles.
//!
//!   cargo xtask build-theme [THEME | app-host | sysapps ...]
//!
//! With no arguments: every theme (themes/*/index.jsx → theme.bundle.js), the app host
//! (runtime/app-host.js → assets/app-host.bundle.js), the system apps with `"process": "isolated"`
//! (assets/sysapps/<id>/), the test bundles in tests/, and finally code-manifest.json (see
//! `code-manifest`). esbuild comes from node_modules/.bin (`npm ci`); `ESBUILD` overrides the path.
//!
//! The virtual module `@lordos/apps` lists the apps in apps/ and the Rust registry (`native-apps`);
//! `react`, `@lordos/sdk`, `@lordos/runtime` and `@lordos/nativeapps` become `__lordos.require(...)`
//! calls in an isolated app's bundle, because the app host provides them at run time.

use super::ojson::{self, Json};
use super::{entries, manifest};
use crate::{at, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

const SHARED: [&str; 5] = ["react", "react/jsx-runtime", "@lordos/sdk", "@lordos/runtime", "@lordos/nativeapps"];

fn quote(text: &str) -> String {
    ojson::compact(&Json::str(text))
}

/// `(n / 1024).toFixed(0)`
fn kb(n: usize) -> usize {
    (n + 512) / 1024
}

struct Build<'a> {
    root: &'a Path,
    scratch: PathBuf,
}

impl Build<'_> {
    fn esbuild(&self, args: &[String], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
        let program = std::env::var_os("ESBUILD").map(PathBuf::from).unwrap_or_else(|| self.root.join("node_modules/.bin/esbuild"));
        let mut command = Command::new(&program);
        command.args(args).arg("--log-level=warning").current_dir(self.root).stdout(Stdio::piped()).stderr(Stdio::piped());
        command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = command.spawn().map_err(|e| format!("{}: {e} (run npm ci)", program.display()))?;
        if let Some(data) = stdin {
            let mut pipe = child.stdin.take().ok_or("no stdin")?;
            pipe.write_all(data).map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            return Err(format!("esbuild failed ({})", output.status));
        }
        Ok(output.stdout)
    }

    fn alias(&self, name: &str, file: &Path) -> String {
        format!("--alias:{name}={}", file.display())
    }

    /// The options every bundle shares: IIFE, ES2020, minified, automatic JSX, the SDK and the runtime.
    fn common(&self, host: bool) -> Vec<String> {
        let mut args: Vec<String> = ["--bundle", "--format=iife", "--target=es2020", "--minify", "--jsx=automatic"].map(String::from).to_vec();
        args.push(self.alias("@lordos/runtime", &self.root.join("runtime/index.js")));
        args.push(self.alias("@lordos/sdk", &self.root.join("sdk/index.js")));
        args.extend(self.defines(host));
        args
    }

    fn defines(&self, host: bool) -> Vec<String> {
        vec![
            "--define:process.env.NODE_ENV=\"production\"".into(),
            "--define:__LORD_STANDALONE__=true".into(),
            format!("--define:__AXIA_APP_HOST__={host}"),
        ]
    }

    /// The virtual module `@lordos/apps`: the apps in apps/ and the compiled Rust registry.
    fn apps_module(&self, native: &[Json]) -> Result<String> {
        let dir = self.root.join("apps");
        let mut ids = Vec::new();
        for entry in entries(&dir) {
            if entry.path().join("manifest.json").exists() {
                ids.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        let mut isolated = Vec::new();
        for id in &ids {
            let path = dir.join(id).join("manifest.json");
            let m = ojson::parse(&at(&path, fs::read_to_string(&path))?).map_err(|e| format!("{}: {e}", path.display()))?;
            // every app carries its own version (semver), shown in the Store
            let version = m.get("version");
            if !matches!(version, Some(Json::Str(v)) if is_semver(v)) {
                let shown = version.map_or("undefined".to_string(), ojson::compact);
                return Err(format!("apps/{id}/manifest.json: \"version\" must be semver (e.g. \"0.8.5\"), got {shown}"));
            }
            isolated.push(m.get("process").and_then(Json::as_str) == Some("isolated"));
        }
        let lines: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let base = dir.join(id);
                format!(
                    "import * as A{i} from {};\nimport M{i} from {};",
                    quote(&base.join("index.jsx").to_string_lossy()),
                    quote(&base.join("manifest.json").to_string_lossy())
                )
            })
            .collect();
        let mut list: Vec<String> = (0..ids.len())
            .map(|i| {
                if isolated[i] {
                    format!("{{ ...M{i}, system: true, isolated: true, Component: isolatedApp(M{i}.id, A{i}.default), Icon: A{i}.Icon }}")
                } else {
                    format!("{{ ...M{i}, system: true, Component: A{i}.default, Icon: A{i}.Icon }}")
                }
            })
            .collect();
        let head = format!("import {{ nativeApp }} from {};\n", quote(&self.root.join("runtime/nativeapps.js").to_string_lossy()));
        let packages = if isolated.iter().any(|&b| b) {
            format!("import {{ isolatedApp }} from {};\n", quote(&self.root.join("sdk/packages.js").to_string_lossy()))
        } else {
            String::new()
        };
        // one list: a comma between two separate ones would leave a hole in the array, which the
        // shell reads as an undefined app, once either of them is empty
        list.extend(native.iter().map(|app| format!("nativeApp({})", ojson::compact(app))));
        Ok(format!("{head}{packages}{}\nexport const apps = [\n  {}\n];\n", lines.join("\n"), list.join(",\n  ")))
    }

    fn theme(&self, name: &str, apps: &Path) -> Result<()> {
        let t0 = Instant::now();
        let dir = self.root.join("themes").join(name);
        let out = dir.join("theme.bundle.js");
        let mut args = vec![dir.join("index.jsx").display().to_string()];
        args.extend(self.common(false));
        args.push(self.alias("@lordos/apps", apps));
        args.push(format!("--outfile={}", out.display()));
        self.esbuild(&args, None)?;
        println!("theme {name}: {} KB in {} ms", kb(at(&out, fs::metadata(&out))?.len() as usize), t0.elapsed().as_millis());
        Ok(())
    }

    /// React + runtime + SDK (broker) + the app loader, for `axia-server --app`.
    fn app_host(&self) -> Result<()> {
        let t0 = Instant::now();
        let out = self.root.join("assets/app-host.bundle.js");
        let mut args = vec![self.root.join("runtime/app-host.js").display().to_string()];
        args.extend(self.common(true));
        args.push(format!("--outfile={}", out.display()));
        self.esbuild(&args, None)?;
        println!("app-host: {} KB in {} ms", kb(at(&out, fs::metadata(&out))?.len() as usize), t0.elapsed().as_millis());
        Ok(())
    }

    /// Built-in apps with `"process": "isolated"`: their app.js for `axia-server --app <id>`, built
    /// like an .lpk (React and the SDK come from the app host), into assets/sysapps/<id>/.
    fn sysapps(&self, native: &[Json]) -> Result<()> {
        let dir = self.root.join("apps");
        let shim = self.scratch.join("shared");
        at(&shim, fs::create_dir_all(&shim))?;
        let mut shared = Vec::new();
        for (i, name) in SHARED.iter().enumerate() {
            let file = shim.join(format!("shared{i}.js"));
            at(&file, fs::write(&file, format!("module.exports = globalThis.__lordos.require({});", quote(name))))?;
            shared.push(self.alias(name, &file));
        }
        let compiled: Vec<&Json> = native.iter().filter(|a| a.get("process").and_then(Json::as_str) == Some("isolated")).collect();
        let compiled_id = |a: &Json| a.get("id").and_then(Json::as_str).unwrap_or("").to_string();
        let mut ids: Vec<String> = entries(&dir).iter().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        ids.extend(compiled.iter().map(|a| compiled_id(a)));
        ids.sort();
        ids.dedup();
        for id in ids {
            let file = dir.join(&id).join("manifest.json");
            let native_app = compiled.iter().find(|a| compiled_id(a) == id).copied();
            if !file.exists() && native_app.is_none() {
                continue;
            }
            let manifest = match native_app {
                Some(app) => app.clone(),
                None => ojson::parse(&at(&file, fs::read_to_string(&file))?).map_err(|e| format!("{}: {e}", file.display()))?,
            };
            if manifest.get("process").and_then(Json::as_str) != Some("isolated") {
                continue;
            }
            let t0 = Instant::now();
            let out = self.root.join("assets/sysapps").join(&id);
            at(&out, fs::create_dir_all(&out))?;
            let mut args: Vec<String> = Vec::new();
            let mut stdin = None;
            if native_app.is_some() {
                let source = format!(
                    "import {{ nativeApp }} from '@lordos/nativeapps'; const app=nativeApp({}); export default app.Component; export const Icon=app.Icon;",
                    ojson::compact(&manifest)
                );
                stdin = Some(source.into_bytes());
                args.push("--loader=js".into());
                args.push(format!("--sourcefile=compiled-{id}.js"));
            } else {
                args.push(dir.join(&id).join("index.jsx").display().to_string());
            }
            args.extend(["--bundle", "--format=iife", "--global-name=__lordosApp", "--target=es2020", "--minify", "--jsx=automatic"].map(String::from));
            args.extend(shared.iter().cloned());
            args.extend(self.defines(false));
            let bundle = self.esbuild(&args, stdin.as_deref())?;
            at(&out, fs::write(out.join("app.js"), &bundle))?;
            at(&out, fs::write(out.join("manifest.json"), format!("{}\n", ojson::pretty(&manifest))))?;
            // the app process reads the texts of its own namespace from the package (lord.locale.catalog)
            for lang in entries(&self.root.join("assets/locales")) {
                let src = lang.path().join(format!("{id}.json"));
                if !src.exists() {
                    continue;
                }
                let target = out.join("locales");
                at(&target, fs::create_dir_all(&target))?;
                let name = lang.file_name().to_string_lossy().into_owned();
                at(&src, fs::copy(&src, target.join(format!("{name}.json"))))?;
            }
            println!("system app {id}: {} KB in {} ms", kb(bundle.len()), t0.elapsed().as_millis());
        }
        Ok(())
    }

    /// The bundles that the integration checks load instead of the theme (tests/*.bundle.js).
    fn test_bundles(&self) -> Result<()> {
        let fixture = self.root.join("rust/xtask/tests/fixtures/switcher_runtime.jsx");
        let source = at(&fixture, fs::read_to_string(&fixture))?
            .replace("@ROOT@", &self.root.to_string_lossy())
            .replace("@MODE@", "dark")
            .replace("@WIDTH@", "1000")
            .replace("@IMAGE@", "\"\"")
            .replace("@APPEARANCE@", "axia");
        let mut args = vec!["--loader=jsx".to_string(), "--sourcefile=switcher-runtime.jsx".into()];
        args.extend(self.common(false));
        args.push(format!("--outfile={}", self.root.join("tests/switcher-native.bundle.js").display()));
        self.esbuild(&args, Some(source.as_bytes()))?;
        for (entry, out) in [
            ("switcher_smoke.jsx", "switcher-smoke"),
            ("dock_preview_smoke.jsx", "dock-preview-smoke"),
            ("dock_bench.jsx", "dock-bench"),
            ("clipboard_smoke.jsx", "clipboard-smoke"),
        ] {
            let mut args = vec![self.root.join("server/tests").join(entry).display().to_string()];
            args.extend(self.common(false));
            args.push(format!("--outfile={}", self.root.join(format!("tests/{out}.bundle.js")).display()));
            self.esbuild(&args, None)?;
        }
        Ok(())
    }
}

fn is_semver(v: &str) -> bool {
    // \d+\.\d+\.\d+(?:[-~][0-9A-Za-z.]+)?
    let (core, pre) = match v.find(['-', '~']) {
        Some(i) => (&v[..i], Some(&v[i + 1..])),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.'))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let scratch = std::env::temp_dir().join(format!("axia-build-theme-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    at(&scratch, fs::create_dir_all(&scratch))?;
    let result = build(repo, args, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    result.map(|()| ExitCode::SUCCESS)
}

fn build(repo: &Path, names: &[String], scratch: &Path) -> Result<()> {
    let build = Build { root: repo, scratch: scratch.to_path_buf() };
    let native = match ojson::parse(&crate::native_apps::manifests().to_string()) {
        Ok(Json::Array(apps)) => apps,
        _ => return Err("native-apps did not give a list".into()),
    };
    let apps = scratch.join("apps.js");
    at(&apps, fs::write(&apps, build.apps_module(&native)?))?;
    let themes: Vec<String> = entries(&repo.join("themes"))
        .iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|d| repo.join("themes").join(d).join("index.jsx").exists())
        .collect();
    let chosen = if names.is_empty() { &themes } else { names };
    for name in chosen.iter().filter(|n| *n != "app-host" && *n != "sysapps") {
        build.theme(name, &apps)?;
    }
    if names.is_empty() || names.iter().any(|n| n == "app-host") {
        build.app_host()?;
    }
    if names.is_empty() || names.iter().any(|n| n == "sysapps") {
        build.sysapps(&native)?;
    }
    if names.is_empty() {
        build.test_bundles()?;
    }
    // the verified-code manifest of what was just built (unsigned: a development build until
    // `cargo xtask sign-code` signs it on the maintainer's machine)
    let outcome = manifest::write_manifest(repo, repo, false)?;
    println!("code-manifest.json: {} files{}", outcome.files, if outcome.changed { "" } else { " (unchanged)" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    #[test]
    fn semver_accepts_the_app_versions_and_refuses_the_rest() {
        for good in ["0.8.5", "1.2.3-beta.1", "10.0.0~rc1", "1.0.0-a.b"] {
            assert!(is_semver(good), "{good}");
        }
        for bad in ["1.2", "1.2.3.4", "v1.2.3", "1.2.3-", "1.2.x", "", "1.2.3-b_1"] {
            assert!(!is_semver(bad), "{bad}");
        }
    }

    #[test]
    fn sizes_round_half_up_like_to_fixed() {
        assert_eq!((kb(0), kb(511), kb(512), kb(1024), kb(1535), kb(1536)), (0, 0, 1, 1, 1, 2));
    }

    #[test]
    fn the_apps_module_lists_apps_in_order_with_their_kind() {
        let repo = TestDir::new("theme-apps");
        for (id, extra) in [("zeta", ""), ("alpha", ",\"process\":\"isolated\"")] {
            let dir = repo.0.join("apps").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("manifest.json"), format!("{{\"id\":\"{id}\",\"version\":\"1.0.0\"{extra}}}")).unwrap();
        }
        fs::create_dir_all(repo.0.join("apps/nomanifest")).unwrap();
        let build = Build { root: &repo.0, scratch: repo.0.join("scratch") };
        let native = [ojson::parse("{\"id\":\"calc\",\"x\":[1,2]}").unwrap()];
        let text = build.apps_module(&native).unwrap();
        let root = repo.0.display();
        assert!(text.starts_with(&format!("import {{ nativeApp }} from \"{root}/runtime/nativeapps.js\";\nimport {{ isolatedApp }} from \"{root}/sdk/packages.js\";\nimport * as A0 from \"{root}/apps/alpha/index.jsx\";")), "{text}");
        assert!(text.contains("{ ...M0, system: true, isolated: true, Component: isolatedApp(M0.id, A0.default), Icon: A0.Icon },\n  { ...M1, system: true, Component: A1.default, Icon: A1.Icon },\n  nativeApp({\"id\":\"calc\",\"x\":[1,2]})\n];"));
        assert!(!text.contains("nomanifest"));
        fs::write(repo.0.join("apps/zeta/manifest.json"), "{\"id\":\"zeta\",\"version\":\"1.0\"}").unwrap();
        assert_eq!(build.apps_module(&native).unwrap_err(), "apps/zeta/manifest.json: \"version\" must be semver (e.g. \"0.8.5\"), got \"1.0\"");
        // with every app compiled, the list starts at the first of them: no hole before it
        for id in ["alpha", "zeta"] {
            fs::remove_file(repo.0.join("apps").join(id).join("manifest.json")).unwrap();
        }
        let text = build.apps_module(&native).unwrap();
        assert!(text.ends_with("export const apps = [\n  nativeApp({\"id\":\"calc\",\"x\":[1,2]})\n];\n"), "{text}");
    }
}
