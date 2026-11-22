//! Signs a release with the maintainer's Ed25519 key (docs/SIGNING.md, docs/RELEASING.md).
//!
//!   cargo xtask sign-code [ROOT [KEY.pem]]
//!       writes ROOT/code-manifest.json (see `code-manifest`) and code-manifest.sig, the base64 of the
//!       signature over the exact bytes of the manifest
//!   cargo xtask sign-release RELEASE-DIR [KEY.pem]
//!       writes SHA256SUMS (every release file) and SHA256SUMS.sig the same way
//!
//! Both verify the signature against tools/release/lordos-release.pub.pem (`CODE_PUB` overrides it
//! for sign-code, `RELEASE_PUB` for sign-release). The private key never enters the repository or a build VM; the default is
//! ~/.config/lordos-release/release-ed25519.key.pem (mode 0600) on the maintainer's machine.

use super::{b64, manifest};
use crate::{at, sha256, Result};
use axia_crypto::ed25519;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const PKCS8: [u8; 16] = [0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20];
const SPKI: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

/// The DER inside a PEM block with the given label.
fn pem_der(text: &str, label: &str) -> Option<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let from = text.find(&begin)? + begin.len();
    let to = from + text[from..].find(&end)?;
    b64::decode(&text[from..to])
}

pub fn private_seed(pem: &str) -> Result<[u8; 32]> {
    let der = pem_der(pem, "PRIVATE KEY").ok_or("not a PEM private key")?;
    if der.len() != 48 || der[..16] != PKCS8 {
        return Err("not an Ed25519 private key".into());
    }
    Ok(der[16..].try_into().unwrap())
}

pub fn public_key(pem: &str) -> Result<[u8; 32]> {
    let der = pem_der(pem, "PUBLIC KEY").ok_or("not a PEM public key")?;
    if der.len() != 44 || der[..12] != SPKI {
        return Err("not an Ed25519 public key".into());
    }
    Ok(der[12..].try_into().unwrap())
}

/// The signature line: base64 of the 64 bytes and a newline, as the shell scripts wrote it.
fn sign_line(seed: &[u8; 32], data: &[u8]) -> String {
    format!("{}\n", b64::encode(&ed25519::sign(seed, data)))
}

fn verify_line(public: &[u8; 32], line: &str, data: &[u8]) -> bool {
    b64::decode(line.trim()).and_then(|s| <[u8; 64]>::try_from(s).ok()).is_some_and(|sig| ed25519::verify(public, &sig, data))
}

fn default_key() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/lordos-release/release-ed25519.key.pem")
}

fn load_keys(key: &Path, public: &Path) -> Result<([u8; 32], [u8; 32])> {
    if fs::File::open(key).is_err() {
        return Err(format!("no private key at {}", key.display()));
    }
    let seed = private_seed(&at(key, fs::read_to_string(key))?)?;
    let public = self::public_key(&at(public, fs::read_to_string(public))?)?;
    Ok((seed, public))
}

/// Signs `file` into `sig_file` and checks the result against the public key.
fn sign_file(seed: &[u8; 32], public: &[u8; 32], file: &Path, sig_file: &Path) -> Result<()> {
    let data = at(file, fs::read(file))?;
    let line = sign_line(seed, &data);
    at(sig_file, fs::write(sig_file, &line))?;
    if !verify_line(public, &line, &data) {
        return Err(format!("{}: the signature does not verify against the release public key", sig_file.display()));
    }
    Ok(())
}

pub fn sign_code(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let root = std::path::absolute(args.first().map(PathBuf::from).unwrap_or_else(|| repo.to_path_buf())).map_err(|e| e.to_string())?;
    let key = args.get(1).map(PathBuf::from).unwrap_or_else(default_key);
    let public = std::env::var_os("CODE_PUB").map(PathBuf::from).unwrap_or_else(|| repo.join("tools/release/lordos-release.pub.pem"));
    let (seed, public) = load_keys(&key, &public)?;
    let outcome = manifest::write_manifest(repo, &root, false)?;
    println!("code-manifest.json: {} files{}", outcome.files, if outcome.changed { "" } else { " (unchanged)" });
    let sig = root.join("code-manifest.sig");
    let _ = fs::remove_file(&sig);
    sign_file(&seed, &public, &root.join("code-manifest.json"), &sig)?;
    println!("signed code-manifest.json ({} files) in {}, signature verified", outcome.files, root.display());
    Ok(ExitCode::SUCCESS)
}

/// A release file name that is safe to list: plain characters and no leading dash.
fn plain_name(name: &str) -> bool {
    !name.starts_with('-') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+~-".contains(&b))
}

pub fn sign_release(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let dir = PathBuf::from(args.first().ok_or("usage: sign-release RELEASE-DIR [KEY.pem]")?);
    let key = args.get(1).map(PathBuf::from).unwrap_or_else(default_key);
    let public = std::env::var_os("RELEASE_PUB").map(PathBuf::from).unwrap_or_else(|| repo.join("tools/release/lordos-release.pub.pem"));
    let (seed, public) = load_keys(&key, &public)?;
    let (sums, sig) = (dir.join("SHA256SUMS"), dir.join("SHA256SUMS.sig"));
    let _ = fs::remove_file(&sums);
    let _ = fs::remove_file(&sig);
    let mut names = Vec::new();
    for entry in at(&dir, fs::read_dir(&dir))?.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let regular = fs::symlink_metadata(entry.path()).map(|m| m.is_file()).unwrap_or(false);
        if regular && !name.starts_with("SHA256SUMS") && !name.starts_with('.') {
            names.push(name);
        }
    }
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    if names.is_empty() {
        return Err(format!("no files in {}", dir.display()));
    }
    for name in &names {
        if !plain_name(name) {
            return Err(format!("unexpected file name in {}: {name}", dir.display()));
        }
    }
    let mut text = String::new();
    for name in &names {
        let path = dir.join(name);
        text.push_str(&format!("{}  {name}\n", sha256::hex(&at(&path, fs::read(&path))?)));
    }
    at(&sums, fs::write(&sums, &text))?;
    sign_file(&seed, &public, &sums, &sig)?;
    println!("signed {} files in {} (SHA256SUMS + SHA256SUMS.sig), signature verified", names.len(), dir.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    // A throwaway key made with `openssl genpkey -algorithm ed25519` and its public half.
    const SEED: [u8; 32] = [7; 32];

    fn pems(seed: &[u8; 32]) -> (String, String) {
        let wrap = |label: &str, der: Vec<u8>| {
            let body = b64::encode(&der);
            let lines: Vec<&str> = body.as_bytes().chunks(64).map(|c| std::str::from_utf8(c).unwrap()).collect();
            format!("-----BEGIN {label}-----\n{}\n-----END {label}-----\n", lines.join("\n"))
        };
        let (mut private, mut public) = (PKCS8.to_vec(), SPKI.to_vec());
        private.extend_from_slice(seed);
        public.extend_from_slice(&ed25519::public_key(seed));
        (wrap("PRIVATE KEY", private), wrap("PUBLIC KEY", public))
    }

    #[test]
    fn pem_keys_round_trip_and_wrong_ones_are_refused() {
        let (private, public) = pems(&SEED);
        assert_eq!(private_seed(&private).unwrap(), SEED);
        assert_eq!(self::public_key(&public).unwrap(), ed25519::public_key(&SEED));
        assert!(private_seed(&public).is_err() && self::public_key(&private).is_err());
        assert!(private_seed("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----").is_err());
        assert!(private_seed("nothing").is_err());
    }

    #[test]
    fn a_signature_line_verifies_and_only_for_its_data() {
        let public = ed25519::public_key(&SEED);
        let line = sign_line(&SEED, b"manifest");
        assert!(line.ends_with("==\n") && line.len() == 89);
        assert!(verify_line(&public, &line, b"manifest"));
        assert!(!verify_line(&public, &line, b"manifest2") && !verify_line(&public, "garbage\n", b"manifest"));
    }

    #[test]
    fn a_release_folder_gets_sorted_sums_and_a_valid_signature() {
        let repo = TestDir::new("release-repo");
        fs::create_dir_all(repo.0.join("tools/release")).unwrap();
        let (private, public) = pems(&SEED);
        fs::write(repo.0.join("tools/release/lordos-release.pub.pem"), public).unwrap();
        let key = repo.0.join("key.pem");
        fs::write(&key, private).unwrap();
        let dir = TestDir::new("release-dir");
        for (name, data) in [("b.rpm", "bb"), ("a.iso", "aa"), (".hidden", "h"), ("SHA256SUMS.old", "x")] {
            fs::write(dir.0.join(name), data).unwrap();
        }
        let args = vec![dir.0.display().to_string(), key.display().to_string()];
        sign_release(&repo.0, &args).unwrap();
        let sums = fs::read_to_string(dir.0.join("SHA256SUMS")).unwrap();
        assert_eq!(sums, format!("{}  a.iso\n{}  b.rpm\n", sha256::hex(b"aa"), sha256::hex(b"bb")));
        let sig = fs::read_to_string(dir.0.join("SHA256SUMS.sig")).unwrap();
        assert!(verify_line(&ed25519::public_key(&SEED), &sig, sums.as_bytes()));
        // a second run replaces the sums instead of listing them
        sign_release(&repo.0, &args).unwrap();
        assert_eq!(fs::read_to_string(dir.0.join("SHA256SUMS")).unwrap(), sums);
        fs::write(dir.0.join("bad name"), "x").unwrap();
        assert!(sign_release(&repo.0, &args).unwrap_err().contains("unexpected file name"));
        assert!(sign_release(&repo.0, &[dir.0.display().to_string(), "/nonexistent.pem".into()]).unwrap_err().starts_with("no private key at"));
        let empty = TestDir::new("release-empty");
        assert!(sign_release(&repo.0, &[empty.0.display().to_string(), key.display().to_string()]).unwrap_err().starts_with("no files in"));
    }
}
