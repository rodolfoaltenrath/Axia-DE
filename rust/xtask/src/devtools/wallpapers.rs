//! Builds the wallpaper collection (assets/wallpapers/) from the original PNGs.
//!
//!   cargo xtask gen-wallpapers SOURCE-DIR
//!     SOURCE-DIR/<category>/lordos-<name>.png              (category: horizons, cosmos)
//!     SOURCE-DIR/<category>/lordos-<name>@<W>x<H>.png      optional higher-resolution variant
//!
//! For each original it writes, with ffmpeg (decode, Lanczos scaling) and cjpeg (libjpeg-turbo):
//!   assets/wallpapers/<category>/lordos-<name>.jpg            same size and name as the original
//!   assets/wallpapers/<category>/lordos-<name>@<W>x<H>.jpg    each variant, at its own size
//!   assets/wallpapers/thumbs/<category>/lordos-<name>.jpg     320×180 (cover crop) for Settings
//! and assets/wallpapers/index.json, which the SDK bundles (sdk/wallpapers.js) to pick, for each
//! screen, the smallest variant that covers it (or the largest one). To add a 4K version later, drop
//! lordos-<name>@3840x2160.png next to the original and run this again.
//!
//! JPEG at quality 90 with 4:4:4 chroma (the art has fine violet and gold detail that 4:2:0 blurs),
//! baseline + optimized Huffman tables. The four gradients come from `gen-wallpaper`.

use super::ojson::{self, Json};
use crate::{at, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const CATEGORIES: [&str; 2] = ["horizons", "cosmos"];
const THUMB: (u32, u32) = (320, 180);

fn run(program: &str, args: &[String]) -> Result<String> {
    let output = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn size(file: &Path) -> Result<(u32, u32)> {
    let text = run(
        "ffprobe",
        &["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", &file.display().to_string()].map(String::from),
    )?;
    let (w, h) = text.trim().split_once(',').ok_or("ffprobe gave no size")?;
    Ok((w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?))
}

/// ffmpeg → PPM (with an optional filter) → cjpeg
fn jpeg(scratch: &Path, input: &Path, output: &Path, filter: Option<&str>, quality: u32, sample: &str) -> Result<()> {
    let ppm = scratch.join("frame.ppm");
    let mut args: Vec<String> = ["-loglevel", "error", "-y", "-i"].map(String::from).to_vec();
    args.push(input.display().to_string());
    if let Some(vf) = filter {
        args.extend(["-vf".to_string(), vf.to_string()]);
    }
    args.extend(["-pix_fmt".to_string(), "rgb24".to_string(), ppm.display().to_string()]);
    run("ffmpeg", &args)?;
    if let Some(parent) = output.parent() {
        at(parent, fs::create_dir_all(parent))?;
    }
    let args = ["-quality".to_string(), quality.to_string(), "-sample".into(), sample.into(), "-optimize".into(), "-outfile".into(), output.display().to_string(), ppm.display().to_string()];
    run("cjpeg", &args).map(|_| ())
}

/// `lordos-<name>`: lowercase letters, digits and dashes.
fn valid_base(base: &str) -> bool {
    base.strip_prefix("lordos-").is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}

/// `...@<W>x<H>.png`
fn is_variant_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else { return false };
    let Some((_, dims)) = stem.rsplit_once('@') else { return false };
    dims.split_once('x').is_some_and(|(w, h)| !w.is_empty() && !h.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) && h.bytes().all(|b| b.is_ascii_digit()))
}

fn entry_json(id: &str, category: &str, thumb: &str, variants: &[(u32, u32, String)]) -> Json {
    let list = variants
        .iter()
        .map(|(w, h, file)| Json::Object(vec![("w".into(), Json::num(*w as usize)), ("h".into(), Json::num(*h as usize)), ("file".into(), Json::str(file))]))
        .collect();
    Json::Object(vec![
        ("id".into(), Json::str(id)),
        ("category".into(), Json::str(category)),
        ("thumb".into(), Json::str(thumb)),
        ("variants".into(), Json::Array(list)),
        ("file".into(), Json::str(&variants[0].2)),
    ])
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let Some(src) = args.first().map(PathBuf::from).filter(|p| p.is_dir()) else {
        eprintln!("usage: cargo xtask gen-wallpapers <source dir with horizons/ cosmos/>");
        return Ok(ExitCode::from(2));
    };
    let out = repo.join("assets/wallpapers");
    let scratch = std::env::temp_dir().join(format!("lordos-wp-{}", std::process::id()));
    at(&scratch, fs::create_dir_all(&scratch))?;
    let result = build(&src, &out, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    result.map(|()| ExitCode::SUCCESS)
}

fn build(src: &Path, out: &Path, scratch: &Path) -> Result<()> {
    let mut index = Vec::new();
    for cat in CATEGORIES {
        let dir = src.join(cat);
        if !dir.exists() {
            continue;
        }
        let mut pngs: Vec<String> = super::entries(&dir).iter().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".png")).collect();
        pngs.sort();
        for name in pngs.iter().filter(|n| !n.contains('@')) {
            let base = name.trim_end_matches(".png");
            if !valid_base(base) {
                return Err(format!("bad wallpaper name: {name}"));
            }
            let id = base.trim_start_matches("lordos-");
            let thumb = format!("thumbs/{cat}/{base}.jpg");
            let mut variants: Vec<(u32, u32, String)> = Vec::new();
            let extra = pngs.iter().filter(|n| n.starts_with(&format!("{base}@")) && is_variant_name(n));
            for v in std::iter::once(name).chain(extra) {
                let file = format!("{cat}/{}.jpg", v.trim_end_matches(".png"));
                jpeg(scratch, &dir.join(v), &out.join(&file), None, 90, "1x1")?;
                let (w, h) = size(&dir.join(v))?;
                let bytes = at(&out.join(&file), fs::metadata(out.join(&file)))?.len() as usize;
                println!("{file}  {w}×{h}  {} KB", (bytes + 512) / 1024);
                variants.push((w, h, file));
            }
            variants.sort_by_key(|v| v.0);
            let filter = format!("scale={0}:{1}:force_original_aspect_ratio=increase:flags=lanczos,crop={0}:{1}", THUMB.0, THUMB.1);
            jpeg(scratch, &dir.join(name), &out.join(&thumb), Some(&filter), 85, "2x2")?;
            index.push(entry_json(id, cat, &thumb, &variants));
        }
    }
    let text = format!("{}\n", ojson::pretty_with(&Json::Array(index.clone()), " "));
    let file = out.join("index.json");
    at(&file, fs::write(&file, text))?;
    println!("{} wallpapers → assets/wallpapers/index.json", index.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_index_is_what_the_tool_writes() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
        let text = fs::read_to_string(repo.join("assets/wallpapers/index.json")).unwrap();
        let value = ojson::parse(&text).unwrap();
        assert_eq!(format!("{}\n", ojson::pretty_with(&value, " ")), text);
        // and rebuilding each entry from its own fields gives the same entry
        for entry in value.items() {
            let variants: Vec<(u32, u32, String)> = entry
                .get("variants")
                .unwrap()
                .items()
                .iter()
                .map(|v| {
                    let n = |k: &str| match v.get(k) { Some(Json::Number(s)) => s.parse().unwrap(), _ => panic!("{k}") };
                    (n("w"), n("h"), v.get("file").and_then(Json::as_str).unwrap().to_string())
                })
                .collect();
            let rebuilt = entry_json(
                entry.get("id").and_then(Json::as_str).unwrap(),
                entry.get("category").and_then(Json::as_str).unwrap(),
                entry.get("thumb").and_then(Json::as_str).unwrap(),
                &variants,
            );
            assert_eq!(&rebuilt, entry);
        }
    }

    #[test]
    fn names_and_variants_follow_the_old_rules() {
        assert!(valid_base("lordos-eternal-dawn") && valid_base("lordos-a1") && !valid_base("lordos-") && !valid_base("eternal") && !valid_base("lordos-Big") && !valid_base("lordos-a_b"));
        assert!(is_variant_name("lordos-x@3840x2160.png") && !is_variant_name("lordos-x@3840.png") && !is_variant_name("lordos-x@ax1.png") && !is_variant_name("lordos-x@1x1.jpg"));
        let variants = vec![(3840, 2160, "c/x@3840x2160.jpg".to_string()), (1672, 941, "c/x.jpg".to_string())];
        let mut sorted = variants.clone();
        sorted.sort_by_key(|v| v.0);
        assert_eq!(entry_json("x", "c", "t.jpg", &sorted).get("file").and_then(Json::as_str), Some("c/x.jpg"));
    }
}
