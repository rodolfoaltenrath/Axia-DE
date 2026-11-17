//! Terminal driver for the same coordinator used by the desktop.
use axia_bench::{lbrun_poll, lbrun_start};
use std::{
    ffi::{CStr, CString},
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let Some(mode) = args.get(1).and_then(|s| CString::new(s.as_str()).ok()) else {
        eprintln!("usage: lordbench-cli quick|full [sustained_s]");
        std::process::exit(2)
    };
    let sustained = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    if unsafe { lbrun_start(mode.as_ptr(), sustained, c"{\"cli\":true}".as_ptr()) } != 0 {
        eprintln!("refused");
        std::process::exit(1)
    }
    let deadline = Instant::now() + Duration::from_secs(2500);
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let p = lbrun_poll();
        if p.is_null() {
            std::process::exit(1)
        }
        let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { libc::free(p.cast()) };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let step = v["step"].as_str().unwrap_or("");
        if last != step {
            eprintln!("{step}");
            last = step.into()
        }
        if !matches!(v["state"].as_str(), Some("waiting" | "running")) {
            println!("{text}");
            std::process::exit(if v["state"] == "done" { 0 } else { 1 })
        }
        if Instant::now() > deadline {
            axia_bench::lbrun_cancel();
            eprintln!("timeout");
            std::process::exit(1)
        }
    }
}
