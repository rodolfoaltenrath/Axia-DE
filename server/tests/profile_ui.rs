//! Manual release-profile comparison using the existing engine workloads.
//! Not installed. No JS fixture, physical input or interaction with the session.
#[path = "../src/bench/host.rs"]
mod host;
use axia_bench::{lb_shape, lb_shape_check, lb_ui, lb_ui_check, lb_ui_checksum};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn measure(mut run: impl FnMut(f64) -> f64) -> Result<Value, String> {
    let warmup = run(0.1);
    if !warmup.is_finite() || warmup <= 0. {
        return Err("workload warmup failed".into());
    }
    let mut samples = vec![];
    for _ in 0..3 {
        let value = run(1.0);
        if !value.is_finite() || value <= 0. {
            return Err("invalid workload sample".into());
        }
        samples.push(value);
    }
    let mut sorted = samples.clone();
    sorted.sort_by(f64::total_cmp);
    Ok(json!({"samples":samples,"median":sorted[1],"sample_seconds":1.0,"warmup_seconds":0.1}))
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || args[0] != "--output" {
        return Err("usage: axia-profile-ui --output <report.json>".into());
    }
    let output = PathBuf::from(&args[1]);
    let (mut nodes, mut unknown) = (0, -1);
    if unsafe { lb_ui_check(&mut nodes, &mut unknown) } != 0 || nodes != 529 || unknown != 0 {
        return Err(format!(
            "fixed scene failed: {nodes} nodes, {unknown} unknown properties"
        ));
    }
    let checksum = lb_ui_checksum(1).ok_or("serial scene checksum failed")?;
    if checksum == 0 || lb_ui_checksum(4) != Some(checksum) {
        return Err("serial/threaded pixels differ".into());
    }
    let mut shaping = vec![];
    for script in 0..5 {
        let (mut glyphs, mut unknown, mut advance) = (0, -1, 0);
        if unsafe { lb_shape_check(script, &mut glyphs, &mut unknown, &mut advance) } != 0
            || glyphs <= 80
            || unknown != 0
            || advance <= 0
        {
            return Err(format!("embedded script {script} failed"));
        }
        shaping.push(json!({"script":script,"glyphs":glyphs,"advance":advance,"unknown":unknown}));
    }
    let serial = measure(|seconds| unsafe { lb_ui(1, seconds) })?;
    let parallel = measure(|seconds| unsafe { lb_ui(4, seconds) })?;
    let shape = measure(|seconds| lb_shape(1, seconds))?;
    let report = json!({"status":"passed","scope":"offscreen engine component throughput; not physical frame/presentation latency",
        "resolution":[1920,1080],"scene_nodes":nodes,"scene_checksum":checksum,"shaping":shaping,
        "layout_paint_serial_fps":serial,"layout_paint_four_threads_fps":parallel,"shaping_one_thread_runs_per_second":shape});
    fs::write(
        &output,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("{}: {e}", output.display()))?;
    println!("{report}");
    Ok(())
}
