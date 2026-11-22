//! The provisional Axia DE logo (assets/brand/*.svg) and its PNG renders (assets/icons/).
//!
//!   cargo xtask gen-brand [--svg-only]
//!       writes the SVGs and renders the PNGs (needs rsvg-convert: `dnf install librsvg2-tools`)
//!
//! The mark is an "A" whose crossbar runs past both legs, like an axis. The app icon puts it in white
//! on the violet rounded square the desktop already uses; the mono glyph is the mark alone, for a menu
//! bar. Sizes of 32 px and less use heavier strokes so the mark stays readable on the pixel grid.
//! The wordmark is live text in the UI (AxiaWordmark in sdk/logo.js), so no lockup is drawn here.

use crate::{at, Result};
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

const VIOLET_LIGHT: &str = "#B46CFF";
const VIOLET_DARK: &str = "#4F46E5";
const INK_DARK: &str = "#1d1d1f";
const INK_LIGHT: &str = "#ffffff";

/// The mark in a 512 box: two legs meeting at the apex and the long crossbar.
fn mark(fill: &str, small: bool) -> String {
    let (leg, bar) = if small { (76, 56) } else { (58, 38) };
    format!(
        "<g fill=\"none\" stroke=\"{fill}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\
         <path d=\"M150 396L256 118L362 396\" stroke-width=\"{leg}\"/>\
         <path d=\"M108 296H404\" stroke-width=\"{bar}\"/></g>"
    )
}

fn svg(body: &str, defs: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 512 512\" width=\"512\" height=\"512\">\
         <title>Axia DE</title><defs>{defs}</defs>{body}</svg>\n"
    )
}

fn app_icon(small: bool) -> String {
    let defs = format!(
        "<linearGradient id=\"bg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"{VIOLET_LIGHT}\"/>\
         <stop offset=\"1\" stop-color=\"{VIOLET_DARK}\"/></linearGradient>\
         <radialGradient id=\"gl\" cx=\".22\" cy=\".12\" r=\".75\"><stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\".25\"/>\
         <stop offset=\"1\" stop-color=\"#fff\" stop-opacity=\"0\"/></radialGradient>\
         <linearGradient id=\"ink\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"90\" x2=\"0\" y2=\"430\"><stop offset=\"0\" stop-color=\"#fff\"/>\
         <stop offset=\"1\" stop-color=\"#EEEAFF\"/></linearGradient>"
    );
    // user-space gradients: a bounding-box one paints nothing on the flat crossbar
    let body = format!(
        "<rect width=\"512\" height=\"512\" rx=\"112\" fill=\"url(#bg)\"/>\
         <rect width=\"512\" height=\"512\" rx=\"112\" fill=\"url(#gl)\"/>\
         <g transform=\"translate(256 262) scale(.78) translate(-256 -256)\">{}</g>",
        mark("url(#ink)", small)
    );
    svg(&body, &defs)
}

fn color_mark() -> String {
    let defs = format!(
        "<linearGradient id=\"mk\" gradientUnits=\"userSpaceOnUse\" x1=\"90\" y1=\"90\" x2=\"430\" y2=\"430\"><stop offset=\"0\" stop-color=\"{VIOLET_LIGHT}\"/>\
         <stop offset=\"1\" stop-color=\"{VIOLET_DARK}\"/></linearGradient>"
    );
    svg(&mark("url(#mk)", false), &defs)
}

/// The five SVGs: file name and text.
fn svgs() -> [(&'static str, String); 5] {
    [
        ("app-icon.svg", app_icon(false)),
        ("app-icon-small.svg", app_icon(true)),
        ("mark.svg", color_mark()),
        ("mark-mono.svg", svg(&mark("#000", false), "")),
        ("mark-mono-small.svg", svg(&mark("#000", true), "")),
    ]
}

/// Every PNG: destination, source SVG, size and the ink of a mono glyph.
fn renders() -> Vec<(String, &'static str, u32, Option<&'static str>)> {
    let mut list = vec![("axia-mark.png".to_string(), "mark.svg", 256, None), ("axia-mark-512.png".to_string(), "mark.svg", 512, None)];
    for s in [16, 32, 64, 128, 256, 512, 1024] {
        list.push((format!("axia-icon-{s}.png"), if s <= 32 { "app-icon-small.svg" } else { "app-icon.svg" }, s, None));
    }
    for (tone, color) in [("dark", INK_DARK), ("light", INK_LIGHT)] {
        for s in [16, 32] {
            list.push((format!("axia-glyph-{tone}-{s}.png"), if s <= 16 { "mark-mono-small.svg" } else { "mark-mono.svg" }, s, Some(color)));
        }
    }
    list
}

fn render(src: &Path, dst: &Path, size: u32, color: Option<&str>) -> Result<()> {
    let mut command = Command::new("rsvg-convert");
    command.args(["-w", &size.to_string(), "-h", &size.to_string(), "-o"]).arg(dst);
    match color {
        // the mono glyph in the given ink, without touching the SVG on disk
        Some(color) => {
            let text = at(src, fs::read_to_string(src))?.replace("stroke=\"#000\"", &format!("stroke=\"{color}\""));
            command.arg("/dev/stdin").stdin(Stdio::piped());
            let mut child = command.spawn().map_err(|e| format!("rsvg-convert: {e}"))?;
            std::io::Write::write_all(&mut child.stdin.take().ok_or("no stdin")?, text.as_bytes()).map_err(|e| e.to_string())?;
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("rsvg-convert failed ({status})"));
            }
        }
        None => {
            let status = command.arg(src).status().map_err(|e| format!("rsvg-convert: {e}"))?;
            if !status.success() {
                return Err(format!("rsvg-convert failed ({status})"));
            }
        }
    }
    Ok(())
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let svg_only = args.iter().any(|a| a == "--svg-only");
    if let Some(bad) = args.iter().find(|a| *a != "--svg-only") {
        return Err(format!("unexpected argument: {bad}"));
    }
    let (out, icons) = (repo.join("assets/brand"), repo.join("assets/icons"));
    at(&out, fs::create_dir_all(&out))?;
    for (name, text) in svgs() {
        at(&out.join(name), fs::write(out.join(name), text))?;
    }
    if svg_only {
        return Ok(ExitCode::SUCCESS);
    }
    at(&icons, fs::create_dir_all(&icons))?;
    for (dst, src, size, color) in renders() {
        render(&out.join(src), &icons.join(&dst), size, color)?;
        println!("{dst} ({size} px)");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_svgs_are_exactly_the_committed_files() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
        for (name, text) in svgs() {
            let committed = fs::read_to_string(repo.join("assets/brand").join(name)).unwrap();
            assert_eq!(text, committed, "{name}");
        }
    }

    #[test]
    fn every_render_has_a_source_and_the_small_marks_use_the_heavy_strokes() {
        let list = renders();
        assert_eq!(list.len(), 2 + 7 + 4);
        let names: Vec<&str> = svgs().iter().map(|(n, _)| *n).collect();
        for (dst, src, _, _) in &list {
            assert!(names.contains(src), "{dst} uses {src}");
        }
        assert!(mark("#000", true).contains("stroke-width=\"76\"") && mark("#000", false).contains("stroke-width=\"58\""));
        assert_eq!(list.iter().find(|r| r.0 == "axia-icon-32.png").unwrap().1, "app-icon-small.svg");
        assert_eq!(list.iter().find(|r| r.0 == "axia-glyph-light-32.png").unwrap().3, Some("#ffffff"));
    }
}
