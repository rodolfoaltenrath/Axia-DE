//! Packages and signs Axia apps (`.lpk` = tar.gz; see docs/NATIVE-APPS.md and docs/SIGNING.md).
//!
//!   cargo xtask lpk FOLDER [OUTPUT.lpk] [--key key.json | --unsigned]
//!       packages the app; signs it with --key, with $LPK_KEY or with ~/.config/axia/lpk-key.json
//!       (if it exists). Without a key the result is «Unsigned» (still with contenido.json, to
//!       detect damage).
//!   cargo xtask lpk keygen [--name "Name"] [--out file]
//!       creates an Ed25519 key pair (default ~/.config/axia/lpk-key.json, mode 600).
//!   cargo xtask lpk sign PACKAGE.lpk --key key.json [OUTPUT.lpk]
//!       re-signs a package (with a new contenido.json).
//!   cargo xtask lpk verify PACKAGE.lpk
//!       shows the signature, the key and whether the files match.
//!   cargo xtask lpk catalog SOURCE.json --key official.json [FOLDER]
//!       writes a signed catalog.json + catalog.sig (only valid with an official Axia key).
//!
//! An app folder has manifest.json and index.jsx like the ones in apps/, and optionally icon.png,
//! locales/<language>.json and assets/. React, @lordos/sdk and @lordos/runtime are NOT included: the
//! bundle takes them from the app host at run time. The JSX is bundled by esbuild (node_modules/.bin).

use super::ojson::{self, Json};
use crate::{at, sha256, Result};
use axia_crypto::ed25519;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const KNOWN_PERMISSIONS: [&str; 11] =
    ["files", "clipboard", "notifications", "network", "camera", "microphone", "screen", "listen", "usb", "bluetooth", "apps"];
const SHARED: [&str; 4] = ["react", "react/jsx-runtime", "@lordos/sdk", "@lordos/runtime"];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (text.len() % 2 == 0).then_some(())?;
    (0..text.len() / 2).map(|i| u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()).collect()
}

// ---- keys ----

struct Key {
    name: String,
    public: String,
    seed: [u8; 32],
}

fn default_key_path() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".config/axia/lpk-key.json")
}

fn load_key(file: &Path) -> Result<Key> {
    let text = at(file, fs::read_to_string(file))?;
    let k = ojson::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let secret = k.get("secret").and_then(Json::as_str).unwrap_or("");
    let seed: Option<[u8; 32]> = (secret.len() == 64 && secret.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| unhex(secret))
        .flatten()
        .and_then(|v| v.try_into().ok());
    let (Some(seed), Some("ed25519")) = (seed, k.get("alg").and_then(Json::as_str)) else {
        return Err(format!("{} is not an lpk key", file.display()));
    };
    let public = hex(&ed25519::public_key(&seed));
    if k.get("public").and_then(Json::as_str) != Some(public.as_str()) {
        return Err(format!("{}: the public key does not match the private one", file.display()));
    }
    Ok(Key { name: k.get("name").and_then(Json::as_str).unwrap_or("").to_string(), public, seed })
}

// ---- minimal ustar tar: regular files only ----

fn tar_header(name: &str, size: usize, mtime: u64) -> Result<[u8; 512]> {
    let mut h = [0u8; 512];
    let (mut prefix, mut name) = ("", name);
    if name.len() > 100 {
        let cut = name.as_bytes()[..156.min(name.len())].iter().rposition(|&b| b == b'/').ok_or("path too long for the package")?;
        (prefix, name) = (&name[..cut], &name[cut + 1..]);
    }
    let put = |h: &mut [u8; 512], at: usize, text: &str| h[at..at + text.len()].copy_from_slice(text.as_bytes());
    put(&mut h, 0, name);
    put(&mut h, 100, "0000644\0");
    put(&mut h, 108, "0000000\0");
    put(&mut h, 116, "0000000\0");
    put(&mut h, 124, &format!("{size:011o}\0"));
    put(&mut h, 136, &format!("{mtime:011o}\0"));
    put(&mut h, 148, "        ");
    put(&mut h, 156, "0");
    put(&mut h, 257, "ustar\0");
    put(&mut h, 263, "00");
    put(&mut h, 345, prefix);
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    put(&mut h, 148, &format!("{sum:06o}\0 "));
    Ok(h)
}

type Files = Vec<(String, Vec<u8>)>;

fn tar(files: &Files, mtime: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for (name, data) in files {
        out.extend_from_slice(&tar_header(name, data.len(), mtime)?);
        out.extend_from_slice(data);
        out.resize(out.len() + (512 - data.len() % 512) % 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

fn untar(mut buf: Vec<u8>) -> Result<Files> {
    if buf.starts_with(&[0x1f, 0x8b]) {
        let mut plain = Vec::new();
        GzDecoder::new(&buf[..]).read_to_end(&mut plain).map_err(|e| format!("not a gzip file: {e}"))?;
        buf = plain;
    }
    let (mut out, mut off) = (Files::new(), 0usize);
    while off + 512 <= buf.len() {
        let h = &buf[off..off + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let text = |a: usize, n: usize| {
            let field = &h[a..a + n];
            let end = field.iter().position(|&b| b == 0).unwrap_or(n);
            field[..end].iter().map(|&b| b as char).collect::<String>()
        };
        let size = usize::from_str_radix(text(124, 12).trim(), 8).unwrap_or(0);
        let prefix = if text(257, 6) == "ustar" { text(345, 155) } else { String::new() };
        let name = format!("{}{}{}", prefix, if prefix.is_empty() { "" } else { "/" }, text(0, 100));
        if matches!(text(156, 1).as_str(), "0" | "") {
            let data = buf.get(off + 512..off + 512 + size).ok_or("the package is cut short")?;
            out.push((name.strip_prefix("./").unwrap_or(&name).to_string(), data.to_vec()));
        }
        off += 512 + size.div_ceil(512) * 512;
    }
    Ok(out)
}

fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    enc.write_all(data).and_then(|()| enc.finish()).map_err(|e| e.to_string())
}

// ---- contenido.json + firma.json (see server/src/sec/sign.c): the signature is over the bytes of contenido.json ----

fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn seal(files: &Files, manifest: &Json, key: Option<&Key>) -> Files {
    let list: Files = files.iter().filter(|(p, _)| p != "contenido.json" && p != "firma.json").cloned().collect();
    let mut fields = vec![
        ("format", Json::num(1)),
        ("id", manifest.get("id").cloned().unwrap_or(Json::Null)),
        ("version", Json::Str(plain_text(manifest.get("version")))),
    ];
    if let Some(k) = key {
        fields.push(("signer", object(vec![("name", Json::str(&k.name)), ("key", Json::str(&k.public))])));
    }
    let entries = list
        .iter()
        .map(|(p, d)| object(vec![("path", Json::str(p)), ("size", Json::num(d.len())), ("sha256", Json::Str(sha256::hex(d)))]))
        .collect();
    fields.push(("files", Json::Array(entries)));
    let text = format!("{}\n", ojson::pretty(&object(fields))).into_bytes();
    let mut out = list;
    out.push(("contenido.json".into(), text.clone()));
    if let Some(k) = key {
        let sig = hex(&ed25519::sign(&k.seed, &text));
        let firma = object(vec![("format", Json::num(1)), ("alg", Json::str("ed25519")), ("key", Json::str(&k.public)), ("sig", Json::Str(sig))]);
        out.push(("firma.json".into(), format!("{}\n", ojson::pretty(&firma)).into_bytes()));
    }
    out
}

/// `String(value)` for the manifest's version: a string as it is, a number as written.
fn plain_text(value: Option<&Json>) -> String {
    match value {
        Some(Json::Str(s) | Json::Number(s)) => s.clone(),
        _ => String::new(),
    }
}

fn trust_line(key: Option<&Key>) -> String {
    match key {
        Some(k) => format!("signed by «{}» ({}…)", k.name, &k.public[..16]),
        None => "unsigned".into(),
    }
}

/// `(n / 1024).toFixed(1)`, rounding halves up as JavaScript does.
fn kb(n: usize) -> String {
    let tenths = (n * 10 + 512) / 1024;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Command line: positional arguments after removing `--flag value` and boolean flags.
struct Args {
    rest: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> Option<String> {
        let i = self.rest.iter().position(|a| a == name)?;
        let value = self.rest.get(i + 1).cloned();
        self.rest.drain(i..(i + 2).min(self.rest.len()));
        value
    }
    fn bool(&mut self, name: &str) -> bool {
        match self.rest.iter().position(|a| a == name) {
            Some(i) => {
                self.rest.remove(i);
                true
            }
            None => false,
        }
    }
    fn pick_key(&mut self) -> Result<Option<Key>> {
        if self.bool("--unsigned") {
            return Ok(None);
        }
        let file = self
            .flag("--key")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("LPK_KEY").filter(|v| !v.is_empty()).map(PathBuf::from))
            .or_else(|| Some(default_key_path()).filter(|p| p.exists()));
        file.map(|f| load_key(&f)).transpose()
    }
}

fn die<T>(message: &str) -> Result<T> {
    Err(format!("lpk: {message}"))
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=40).contains(&b.len()) && b[0].is_ascii_lowercase() && b[1..].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn valid_version(v: &str) -> bool {
    let b = v.as_bytes();
    (1..=32).contains(&b.len()) && b[0].is_ascii_digit() && b[1..].iter().all(|&c| c.is_ascii_alphanumeric() || b".+~-".contains(&c))
}

fn valid_path(path: &str) -> bool {
    path.split('/').all(|part| {
        let b = part.as_bytes();
        !b.is_empty() && (b[0].is_ascii_alphanumeric() || b[0] == b'_' || b[0] == b'-') && b[1..].iter().all(|&c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    })
}

/// Bundles the app's index.jsx with esbuild, React and the SDK coming from the app host at run time.
fn bundle(repo: &Path, dir: &Path) -> Result<Vec<u8>> {
    let esbuild = std::env::var_os("ESBUILD").map(PathBuf::from).unwrap_or_else(|| repo.join("node_modules/.bin/esbuild"));
    let shim = std::env::temp_dir().join(format!("axia-lpk-shared-{}", std::process::id()));
    let _ = fs::remove_dir_all(&shim);
    at(&shim, fs::create_dir_all(&shim))?;
    let mut command = Command::new(&esbuild);
    command.arg(dir.join("index.jsx"));
    for (i, name) in SHARED.iter().enumerate() {
        let file = shim.join(format!("shared{i}.js"));
        at(&file, fs::write(&file, format!("module.exports = globalThis.__lordos.require({});", ojson::pretty(&Json::str(name)))))?;
        command.arg(format!("--alias:{name}={}", file.display()));
    }
    command
        .args(["--bundle", "--format=iife", "--global-name=__lordosApp", "--target=es2020", "--minify", "--jsx=automatic", "--log-level=warning"])
        .arg("--define:process.env.NODE_ENV=\"production\"");
    let output = command.output().map_err(|e| format!("{}: {e}", esbuild.display()));
    let _ = fs::remove_dir_all(&shim);
    let output = output?;
    if !output.status.success() {
        return Err(format!("esbuild failed:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(output.stdout)
}

fn walk(base: &Path, rel: &str, files: &mut Files) -> Result<()> {
    for entry in super::entries(&base.join(rel)) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            walk(base, &r, files)?;
        } else if kind.is_file() {
            let path = base.join(&r);
            files.push((r, at(&path, fs::read(&path))?));
        }
    }
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn pack(repo: &Path, mut a: Args) -> Result<ExitCode> {
    let key = a.pick_key()?;
    let Some(dir) = a.rest.first().map(PathBuf::from) else {
        return die("usage: cargo xtask lpk <app folder> [output.lpk] [--key key.json | --unsigned]");
    };
    let manifest_path = dir.join("manifest.json");
    let manifest = ojson::parse(&at(&manifest_path, fs::read_to_string(&manifest_path))?).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let id = manifest.get("id").and_then(Json::as_str).unwrap_or("");
    if !valid_id(id) {
        return die("invalid manifest.id: a-z, 0-9 and -, 2 to 40 long");
    }
    let version = plain_text(manifest.get("version"));
    if !valid_version(&version) {
        return die("invalid manifest.version");
    }
    if manifest.get("name").is_none_or(|n| matches!(n, Json::Str(s) if s.is_empty()) || matches!(n, Json::Null | Json::Bool(false))) {
        return die("missing manifest.name");
    }
    let icon = manifest.get("icon").and_then(Json::as_str).unwrap_or("");
    if icon.is_empty() || !icon.ends_with(".png") || !dir.join(icon).exists() {
        return die("missing own icon: manifest.icon must point to a .png in the app (128×128 or larger recommended)");
    }
    for p in manifest.get("permissions").map_or(&[][..], Json::items) {
        let name = p.as_str().unwrap_or("");
        if !KNOWN_PERMISSIONS.contains(&name) {
            return die(&format!("unknown permission in manifest.permissions: {name} (valid: {})", KNOWN_PERMISSIONS.join(", ")));
        }
    }
    let app_js = bundle(repo, &dir)?;
    let mut files: Files = vec![
        ("manifest.json".into(), format!("{}\n", ojson::pretty(&manifest)).into_bytes()),
        ("app.js".into(), app_js.clone()),
        (icon.to_string(), at(&dir.join(icon), fs::read(dir.join(icon)))?),
    ];
    for sub in ["locales", "assets"] {
        if dir.join(sub).exists() {
            walk(&dir, sub, &mut files)?;
        }
    }
    for (p, _) in &files {
        if !valid_path(p) {
            return die(&format!("invalid path in the package: {p}"));
        }
    }
    let out = a.rest.get(1).cloned().unwrap_or_else(|| format!("{id}-{version}.lpk"));
    let sealed = seal(&files, &manifest, key.as_ref());
    write_package(Path::new(&out), &sealed)?;
    let size = at(Path::new(&out), fs::metadata(&out))?.len() as usize;
    println!("{out}: {} files, {} KB (app.js {} KB), {}", sealed.len(), kb(size), kb(app_js.len()), trust_line(key.as_ref()));
    Ok(ExitCode::SUCCESS)
}

fn write_package(out: &Path, files: &Files) -> Result<()> {
    at(out, fs::write(out, gzip(&tar(files, now())?)?))
}

fn random_seed() -> Result<[u8; 32]> {
    let mut seed = [0u8; 32];
    let mut random = at(Path::new("/dev/urandom"), fs::File::open("/dev/urandom"))?;
    random.read_exact(&mut seed).map_err(|e| e.to_string())?;
    Ok(seed)
}

fn user_name() -> String {
    ["USER", "LOGNAME"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty())).unwrap_or_else(|| "user".into())
}

fn keygen(mut a: Args) -> Result<ExitCode> {
    let name = a.flag("--name").unwrap_or_else(user_name);
    let out = a.flag("--out").map(PathBuf::from).unwrap_or_else(default_key_path);
    if out.exists() {
        return die(&format!("{} already exists (a key is never overwritten)", out.display()));
    }
    let seed = random_seed()?;
    let public = hex(&ed25519::public_key(&seed));
    write_key(&out, &name, &public, &seed)?;
    println!("{}: key of «{name}»\npublic: {public}\n(the private key is never shared or committed to the repository)", out.display());
    Ok(ExitCode::SUCCESS)
}

fn write_key(out: &Path, name: &str, public: &str, seed: &[u8; 32]) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        at(parent, fs::DirBuilder::new().recursive(true).mode(0o700).create(parent))?;
    }
    let key = object(vec![("format", Json::num(1)), ("alg", Json::str("ed25519")), ("name", Json::str(name)), ("public", Json::str(public)), ("secret", Json::Str(hex(seed)))]);
    let mut file = at(out, fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(out))?;
    at(out, file.write_all(format!("{}\n", ojson::pretty(&key)).as_bytes()))
}

fn sign(mut a: Args) -> Result<ExitCode> {
    let key = a.pick_key()?;
    let (input, out) = (a.rest.get(1).cloned(), a.rest.get(2).cloned());
    let (Some(input), Some(key)) = (input, key) else {
        return die("usage: cargo xtask lpk sign <package.lpk> --key key.json [output.lpk]");
    };
    let files = untar(at(Path::new(&input), fs::read(&input))?)?;
    let Some((_, manifest)) = files.iter().find(|(p, _)| p == "manifest.json") else {
        return die("not an Axia package");
    };
    let manifest = ojson::parse(&String::from_utf8_lossy(manifest)).map_err(|e| format!("manifest.json: {e}"))?;
    let out = out.unwrap_or_else(|| input.clone());
    write_package(Path::new(&out), &seal(&files, &manifest, Some(&key)))?;
    println!("{out}: {}", trust_line(Some(&key)));
    Ok(ExitCode::SUCCESS)
}

fn verify(a: Args) -> Result<ExitCode> {
    let Some(input) = a.rest.get(1) else { return die("usage: cargo xtask lpk verify <package.lpk>") };
    let files = untar(at(Path::new(input), fs::read(input))?)?;
    let get = |p: &str| files.iter().find(|(n, _)| n == p).map(|(_, d)| d);
    let Some(contents_bytes) = get("contenido.json") else {
        println!("no contenido.json: «Unsigned» and no hashes");
        return Ok(ExitCode::SUCCESS);
    };
    let contents = ojson::parse(&String::from_utf8_lossy(contents_bytes)).map_err(|e| format!("contenido.json: {e}"))?;
    let mut ok = true;
    let listed = contents.get("files").map_or(&[][..], Json::items);
    for f in listed {
        let path = f.get("path").and_then(Json::as_str).unwrap_or("");
        let size = f.get("size").map(|s| plain_text(Some(s))).unwrap_or_default();
        match get(path) {
            Some(d) if d.len().to_string() == size && Some(sha256::hex(d).as_str()) == f.get("sha256").and_then(Json::as_str) => {}
            _ => {
                println!("ALTERED: {path}");
                ok = false;
            }
        }
    }
    for (p, _) in &files {
        if p != "contenido.json" && p != "firma.json" && !listed.iter().any(|f| f.get("path").and_then(Json::as_str) == Some(p)) {
            println!("EXTRA: {p}");
            ok = false;
        }
    }
    if let Some(firma) = get("firma.json") {
        let sig = ojson::parse(&String::from_utf8_lossy(firma)).map_err(|e| format!("firma.json: {e}"))?;
        let key = sig.get("key").and_then(Json::as_str).unwrap_or("");
        let public: Option<[u8; 32]> = unhex(key).and_then(|v| v.try_into().ok());
        let bytes: Option<[u8; 64]> = sig.get("sig").and_then(Json::as_str).and_then(unhex).and_then(|v| v.try_into().ok());
        let good = matches!((public, bytes), (Some(p), Some(s)) if ed25519::verify(&p, &s, contents_bytes));
        let signer = contents.get("signer").and_then(|s| s.get("name")).and_then(Json::as_str).unwrap_or("?");
        println!("signature {} from «{signer}», key {key}", if good { "valid" } else { "INVALID" });
        ok = ok && good;
    } else {
        println!("unsigned (hashes only)");
    }
    println!("{}", if ok { "the files match" } else { "the package does NOT match its contenido.json" });
    Ok(ExitCode::from(u8::from(!ok)))
}

/// Today's date, UTC, as `YYYY-MM-DD`.
fn today() -> String {
    let days = (now() / 86400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn catalog(mut a: Args) -> Result<ExitCode> {
    let key = a.pick_key()?;
    let (src, out_dir) = (a.rest.get(1).cloned(), a.rest.get(2).cloned().unwrap_or_else(|| ".".into()));
    let (Some(src), Some(key)) = (src, key) else {
        return die("usage: cargo xtask lpk catalog <source.json> --key official.json [folder]");
    };
    let src = PathBuf::from(src);
    let mut cat = ojson::parse(&at(&src, fs::read_to_string(&src))?).map_err(|e| format!("{}: {e}", src.display()))?;
    cat.set("format", Json::num(1));
    let date = match cat.get("date") {
        Some(Json::Str(s)) if !s.is_empty() => Json::Str(s.clone()),
        _ => Json::Str(today()),
    };
    cat.set("date", date);
    // size and hash of each package (if its file is given with "file")
    if let Some(Json::Array(packages)) = cat.get_mut("packages") {
        for p in packages {
            let Some(file) = p.get("file").and_then(Json::as_str).filter(|f| !f.is_empty()).map(str::to_string) else { continue };
            let path = src.parent().unwrap_or(Path::new(".")).join(&file);
            let bytes = at(&path, fs::read(&path))?;
            p.set("size", Json::num(bytes.len()));
            p.set("sha256", Json::Str(sha256::hex(&bytes)));
            p.remove("file");
        }
    }
    let text = format!("{}\n", ojson::pretty(&cat)).into_bytes();
    let sig = hex(&ed25519::sign(&key.seed, &text));
    let out_dir = PathBuf::from(out_dir);
    at(&out_dir, fs::create_dir_all(&out_dir))?;
    at(&out_dir, fs::write(out_dir.join("catalog.json"), &text))?;
    let firma = object(vec![("format", Json::num(1)), ("alg", Json::str("ed25519")), ("key", Json::str(&key.public)), ("sig", Json::Str(sig))]);
    at(&out_dir, fs::write(out_dir.join("catalog.sig"), format!("{}\n", ojson::pretty(&firma))))?;
    let count = |name: &str| cat.get(name).map_or(0, |v| v.items().len());
    let revoked = cat.get("revoked").map_or(0, |r| r.get("keys").map_or(0, |v| v.items().len()) + r.get("packages").map_or(0, |v| v.items().len()));
    let serial = cat.get("serial").map_or("undefined".to_string(), |s| plain_text(Some(s)));
    println!(
        "{}/catalog.json (serial {serial}, {} packages, {} authors, {revoked} revoked), signed by «{}»",
        out_dir.display(),
        count("packages"),
        count("authors"),
        key.name
    );
    Ok(ExitCode::SUCCESS)
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let a = Args { rest: args.to_vec() };
    match a.rest.first().map(String::as_str) {
        Some("keygen") => keygen(a),
        Some("sign") => sign(a),
        Some("verify") => verify(a),
        Some("catalog") => catalog(a),
        _ => pack(repo, a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    fn key() -> Key {
        let seed = [9u8; 32];
        Key { name: "Tester".into(), public: hex(&ed25519::public_key(&seed)), seed }
    }

    fn sample() -> (Files, Json) {
        let manifest = ojson::parse("{\"id\":\"demo\",\"version\":\"1.0.0\",\"name\":\"Demo\"}").unwrap();
        let files = vec![("manifest.json".to_string(), b"{}".to_vec()), ("app.js".to_string(), b"console.log(1)".to_vec())];
        (files, manifest)
    }

    #[test]
    fn tar_round_trips_with_long_paths_and_padding() {
        let long = format!("assets/{}/{}.bin", "d".repeat(80), "f".repeat(30));
        let files: Files = vec![("a.txt".into(), vec![1; 700]), (long.clone(), vec![2; 512]), ("empty".into(), vec![])];
        let archive = tar(&files, 1_700_000_000).unwrap();
        assert_eq!(archive.len() % 512, 0);
        assert_eq!(untar(archive.clone()).unwrap(), files);
        assert_eq!(untar(gzip(&archive).unwrap()).unwrap(), files);
        assert!(long.len() > 100);
    }

    #[test]
    fn the_tar_header_has_a_valid_checksum_and_the_ustar_marks() {
        let h = tar_header("x/y.txt", 5, 0o17).unwrap();
        assert_eq!(&h[257..263], b"ustar\0");
        let mut copy = h;
        copy[148..156].copy_from_slice(b"        ");
        let sum: u32 = copy.iter().map(|&b| u32::from(b)).sum();
        assert_eq!(&h[148..156], format!("{sum:06o}\0 ").as_bytes());
        assert!(tar_header(&"p".repeat(300), 1, 0).is_err());
    }

    #[test]
    fn a_sealed_package_lists_its_files_and_the_signature_covers_the_list() {
        let (files, manifest) = sample();
        let signed = seal(&files, &manifest, Some(&key()));
        let names: Vec<_> = signed.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["manifest.json", "app.js", "contenido.json", "firma.json"]);
        let contents = &signed[2].1;
        let text = String::from_utf8(contents.clone()).unwrap();
        assert!(text.starts_with("{\n  \"format\": 1,\n  \"id\": \"demo\",\n  \"version\": \"1.0.0\",\n  \"signer\": {\n    \"name\": \"Tester\""), "{text}");
        assert!(text.ends_with("  ]\n}\n"));
        let firma = ojson::parse(&String::from_utf8_lossy(&signed[3].1)).unwrap();
        let sig: [u8; 64] = unhex(firma.get("sig").and_then(Json::as_str).unwrap()).unwrap().try_into().unwrap();
        assert!(ed25519::verify(&ed25519::public_key(&key().seed), &sig, contents));
        let unsigned = seal(&files, &manifest, None);
        assert_eq!(unsigned.len(), 3);
        assert!(!String::from_utf8_lossy(&unsigned[2].1).contains("signer"));
        // sealing again drops the old seal instead of listing it
        assert_eq!(seal(&signed, &manifest, Some(&key())), signed);
    }

    #[test]
    fn names_and_text_helpers_follow_the_old_rules() {
        assert!(valid_id("my-app2") && !valid_id("a") && !valid_id("My") && !valid_id("1app") && !valid_id(&"a".repeat(41)));
        assert!(valid_version("0.9.1-beta+x~y") && !valid_version("v1") && !valid_version(""));
        assert!(valid_path("locales/pt.json") && valid_path("_a/b-c.d") && !valid_path("../x") && !valid_path("a//b") && !valid_path(".hid") && !valid_path("a b"));
        assert_eq!((kb(0), kb(1024), kb(256), kb(1536), kb(12345)), ("0.0".into(), "1.0".into(), "0.3".into(), "1.5".into(), "12.1".into()));
        assert_eq!(today().len(), 10);
        assert_eq!(trust_line(None), "unsigned");
        assert_eq!(trust_line(Some(&key())), format!("signed by «Tester» ({}…)", &key().public[..16]));
    }

    #[test]
    fn keys_are_written_private_read_back_and_never_overwritten() {
        let dir = TestDir::new("lpk-key");
        let file = dir.0.join("sub/key.json");
        let k = key();
        write_key(&file, &k.name, &k.public, &k.seed).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        let back = load_key(&file).unwrap();
        assert_eq!((back.name.as_str(), back.public.as_str(), back.seed), ("Tester", k.public.as_str(), k.seed));
        assert!(write_key(&file, "x", &k.public, &k.seed).is_err());
        let bad = dir.0.join("bad.json");
        fs::write(&bad, "{\"alg\":\"ed25519\",\"secret\":\"00\"}").unwrap();
        assert!(load_key(&bad).err().unwrap().contains("is not an lpk key"));
        let wrong = dir.0.join("wrong.json");
        fs::write(&wrong, format!("{{\"alg\":\"ed25519\",\"public\":\"{}\",\"secret\":\"{}\"}}", "0".repeat(64), hex(&k.seed))).unwrap();
        assert!(load_key(&wrong).err().unwrap().contains("does not match"));
    }

    #[test]
    fn re_signing_and_verifying_a_package_on_disk() {
        let dir = TestDir::new("lpk-verify");
        let (files, manifest) = sample();
        let package = dir.0.join("demo.lpk");
        write_package(&package, &seal(&files, &manifest, None)).unwrap();
        let args = |v: &[&str]| Args { rest: v.iter().map(|s| s.to_string()).collect() };
        assert_eq!(verify(args(&["verify", &package.display().to_string()])).unwrap(), ExitCode::SUCCESS);
        let keyfile = dir.0.join("k.json");
        let k = key();
        write_key(&keyfile, &k.name, &k.public, &k.seed).unwrap();
        let signed = dir.0.join("signed.lpk");
        let argv = ["sign", &package.display().to_string(), "--key", &keyfile.display().to_string(), &signed.display().to_string()];
        assert_eq!(sign(args(&argv)).unwrap(), ExitCode::SUCCESS);
        assert_eq!(verify(args(&["verify", &signed.display().to_string()])).unwrap(), ExitCode::SUCCESS);
        // a changed file is caught
        let mut tampered = untar(fs::read(&signed).unwrap()).unwrap();
        tampered[1].1.push(b'!');
        let bad = dir.0.join("bad.lpk");
        write_package(&bad, &tampered).unwrap();
        assert_eq!(verify(args(&["verify", &bad.display().to_string()])).unwrap(), ExitCode::from(1));
    }
}
