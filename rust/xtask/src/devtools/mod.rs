//! The development tools that used to live in `tools/` (Node and Python scripts), as xtask
//! subcommands. They run on the development or build machine only.
//!
//!   cargo xtask code-manifest [--root DIR] [--check]
//!   cargo xtask brand-check
//!   cargo xtask err-check
//!   cargo xtask i18n-check [LANG | lordos-lang-xx.json]
//!   cargo xtask gen-controls-font
//!   cargo xtask gen-lbfonts [--fonts DIR] [--ucd DIR]
//!   cargo xtask gen-monitor-data GEOJSON CSV
//!   cargo xtask gen-unicode UCD-FOLDER
//!   cargo xtask gen-emoji FOLDER
//!   cargo xtask gen-brand [--svg-only]
//!   cargo xtask copr-publish [--packages DIR] [--report FILE] [--build-id N]
//!   cargo xtask desktop-check [--require-gpu] | clipboard-x11-check | dock-bench
//!   cargo xtask gen-wallpapers SOURCE-DIR
//!   cargo xtask build-theme [THEME | app-host | sysapps ...]
//!   cargo xtask lpk FOLDER | keygen | sign | verify | catalog
//!   cargo xtask sign-code [ROOT [KEY]] | sign-release DIR [KEY]
//!   cargo xtask i18n-pack new ID NAME ENGLISH [PLURAL [REGION]] | export ID

mod b64;
mod brand;
mod brand_gen;
mod controls_font;
mod copr;
mod desktop_check;
mod emoji;
mod errcheck;
mod i18n;
mod lbfonts;
mod lpk;
mod manifest;
mod monitor_data;
mod ojson;
mod release;
mod theme;
mod unicode;
mod wallpapers;

use std::path::Path;
use std::process::ExitCode;

/// Runs a tool command; `None` when `command` is not one of them.
pub fn dispatch(repo: &Path, command: &str, args: &[String]) -> Option<crate::Result<ExitCode>> {
    Some(match command {
        "code-manifest" => manifest::execute(repo, args),
        "brand-check" => brand::execute(repo, args),
        "err-check" => errcheck::execute(repo, args),
        "i18n-check" => i18n::check(repo, args),
        "i18n-pack" => i18n::pack(repo, args),
        "gen-controls-font" => controls_font::execute(repo, args),
        "gen-lbfonts" => lbfonts::execute(repo, args),
        "gen-monitor-data" => monitor_data::execute(repo, args),
        "gen-unicode" => unicode::execute(repo, args),
        "gen-emoji" => emoji::execute(repo, args),
        "gen-brand" => brand_gen::execute(repo, args),
        "copr-publish" => copr::execute(repo, args),
        "desktop-check" => desktop_check::execute(repo, args),
        "dock-bench" => desktop_check::dock_bench(repo, args),
        "clipboard-x11-check" => desktop_check::x11_check(repo, args),
        "gen-wallpapers" => wallpapers::execute(repo, args),
        "build-theme" => theme::execute(repo, args),
        "lpk" => lpk::execute(repo, args),
        "sign-code" => release::sign_code(repo, args),
        "sign-release" => release::sign_release(repo, args),
        _ => return None,
    })
}

/// The entries of a directory sorted by name in byte order, as Node lists them.
pub fn entries(dir: &Path) -> Vec<std::fs::DirEntry> {
    let mut list: Vec<_> = match std::fs::read_dir(dir) {
        Ok(read) => read.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };
    list.sort_by_key(|e| e.file_name());
    list
}

/// ASCII word character, as in a JavaScript `\w`.
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 1-based line of a byte offset.
pub fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() + 1
}
