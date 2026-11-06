//! Builds a source RPM with locked frontend bundles and a growing snapshot release.

use crate::{at, dist, run, Result};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The value of the first `Name:  value` line that has exactly one word, and
/// where that line sits in the text.
fn field<'a>(text: &'a str, name: &str) -> Option<(&'a str, usize, usize)> {
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some(rest) = body.strip_prefix(name) {
            if rest.starts_with([' ', '\t']) {
                let value = rest.trim_matches([' ', '\t']);
                if !value.is_empty() && !value.contains(char::is_whitespace) {
                    return Some((value, start, start + body.len()));
                }
            }
        }
        start += line.len();
    }
    None
}

/// Writes the release into the spec stored in the SRPM. Command-line RPM macros
/// alone would disappear when Copr rebuilds that SRPM on another machine.
pub fn snapshot_spec(
    text: &str,
    version: &str,
    stamp: &str,
    commit: &str,
) -> Result<(String, String)> {
    if field(text, "Version:").map(|(value, ..)| value) != Some(version) {
        return Err("package.json and the RPM spec must have the same version".into());
    }
    let release = field(text, "Release:").and_then(|(value, start, end)| {
        let number = value.strip_suffix("%{?dist}")?;
        let numeric = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
        numeric.then_some((number, start, end))
    });
    let (number, start, end) =
        release.ok_or("expected a numeric RPM Release followed by %{?dist}")?;
    let valid_stamp = stamp.len() == 20 && stamp.bytes().all(|b| b.is_ascii_digit());
    let valid_commit = commit.len() == 12
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !valid_stamp || !valid_commit {
        return Err("invalid snapshot timestamp or commit".into());
    }
    let release = format!("{number}.{stamp}.git{commit}");
    let spec = format!(
        "{}Release:        {release}%{{?dist}}{}",
        &text[..start],
        &text[end..]
    );
    Ok((spec, release))
}

/// Year, month and day of a count of days since 1970-01-01, in the proleptic
/// Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// A UTC time as twenty digits: `YYYYMMDDhhmmss` and the microseconds.
fn utc_stamp(seconds: u64, micros: u32) -> String {
    let (year, month, day) = civil_from_days(seconds / 86_400);
    let time = seconds % 86_400;
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}{micros:06}",
        time / 3_600,
        time / 60 % 60,
        time % 60
    )
}

/// A private temporary directory that is removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(micros: u128) -> Result<Self> {
        let name = format!("axia-srpm-{}-{micros}", std::process::id());
        let path = std::env::temp_dir().join(name);
        at(&path, DirBuilder::new().mode(0o700).create(&path))?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn srpm(repo: &Path, outdir: &Path) -> Result<()> {
    at(outdir, fs::create_dir_all(outdir))?;
    let out = at(outdir, outdir.canonicalize())?;
    let version = dist::version(repo)?;
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(repo)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git rev-parse failed: {}", output.status));
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    let stamp = utc_stamp(now.as_secs(), now.subsec_micros());
    let path = repo.join("packaging/rpm/axia-de.spec");
    let text = at(&path, fs::read_to_string(&path))?;
    let (spec, release) = snapshot_spec(&text, &version, &stamp, &commit)?;
    dist::dist(repo)?;

    let top = TempDir::new(now.as_micros())?;
    let specs = top.0.join("SPECS");
    at(&specs, fs::create_dir(&specs))?;
    let specfile = specs.join("axia-de.spec");
    at(&specfile, fs::write(&specfile, spec))?;
    run(Command::new("rpmbuild")
        .arg("-bs")
        .arg("--define")
        .arg(format!("_topdir {}", top.0.display()))
        .arg("--define")
        .arg(format!("_sourcedir {}", repo.join("dist").display()))
        .arg("--define")
        .arg(format!("_srcrpmdir {}", out.display()))
        .arg(&specfile)
        .current_dir(repo))?;
    println!("Source RPM ready: axia-de {version}-{release} ({commit})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "20261102210000123456";
    const COMMIT: &str = "abcdef012345";

    #[test]
    fn release_is_embedded_and_other_sections_survive() {
        let source = "Name: axia-de\nVersion: 0.8.7\nRelease: 3%{?dist}\n\nSummary: Axia\n";
        let (spec, release) = snapshot_spec(source, "0.8.7", STAMP, COMMIT).unwrap();
        assert_eq!(release, "3.20261102210000123456.gitabcdef012345");
        assert_eq!(
            spec,
            format!("Name: axia-de\nVersion: 0.8.7\nRelease:        {release}%{{?dist}}\n\nSummary: Axia\n")
        );
    }

    #[test]
    fn version_mismatch_is_refused() {
        let source = "Version: 0.8.6\nRelease: 3%{?dist}\n";
        assert!(snapshot_spec(source, "0.8.7", STAMP, COMMIT).is_err());
    }

    #[test]
    fn snapshot_release_cannot_be_injected() {
        let source = "Version: 0.8.7\nRelease: 3%{?dist}\n";
        for (stamp, commit) in [("1\n%post\necho bad", COMMIT), (STAMP, "abc\n%post")] {
            assert!(snapshot_spec(source, "0.8.7", stamp, commit).is_err());
        }
    }

    #[test]
    fn release_must_be_a_number_followed_by_dist() {
        for release in [
            "Release: 3",
            "Release: x%{?dist}",
            "Release: %{?dist}",
            "Release: 3%{?dist} x",
        ] {
            let source = format!("Version: 0.8.7\n{release}\n");
            assert!(
                snapshot_spec(&source, "0.8.7", STAMP, COMMIT).is_err(),
                "{release}"
            );
        }
    }

    #[test]
    fn stamp_is_utc_with_twenty_digits() {
        assert_eq!(utc_stamp(0, 0), "19700101000000000000");
        // Leap day, and the first second after a year's end.
        assert_eq!(utc_stamp(1_709_164_800, 7), "20240229000000000007");
        assert_eq!(utc_stamp(1_735_689_599, 999_999), "20241231235959999999");
        assert_eq!(utc_stamp(1_735_689_600, 0), "20250101000000000000");
    }
}
