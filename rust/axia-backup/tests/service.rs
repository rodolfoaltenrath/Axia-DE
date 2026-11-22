//! The service thread end to end (this replaces what server/src/backup/backup.c did untested):
//! configuration, a manual backup, listing, restore, deletion and the encrypted destination.
//! The service is one per process, so everything runs in a single test.
use axia_backup::service;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn status() -> Value {
    serde_json::from_slice(&service::status_json().expect("running")).expect("the status is JSON")
}

fn until(what: &str, mut ok: impl FnMut(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let s = status();
        if ok(&s) {
            return s;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {s}");
        std::thread::sleep(Duration::from_millis(40));
    }
}

fn err(r: Result<(), Vec<u8>>) -> String {
    String::from_utf8(r.expect_err("an error code line")).unwrap()
}

fn config(dest: &Path, extra: &str) -> String {
    format!("{{\"dest\":\"{}\",\"interval\":\"manual\",\"pauseOnBattery\":false{extra}}}", dest.display())
}

#[test]
fn the_service_runs_backups_restores_and_encrypts() {
    std::env::set_var("LORD_BACKUP_NOPRIO", "1");
    let t: PathBuf = std::env::temp_dir().join(format!("axia-backup-service-{}", std::process::id()));
    let _ = fs::remove_dir_all(&t);
    let home = t.join("home");
    fs::create_dir_all(home.join(".config/axia")).unwrap();
    let home = home.canonicalize().unwrap();
    axia_fs::policy::init(&home, "tester");
    fs::write(home.join("a.txt"), "version 1\n").unwrap();
    fs::create_dir(home.join("skip-me")).unwrap();
    fs::write(home.join("skip-me/x"), "x").unwrap();
    fs::create_dir(home.join("docs")).unwrap();
    fs::write(home.join("docs/b.txt"), "b\n").unwrap();
    let dest = home.join("disk");
    fs::create_dir(&dest).unwrap();

    // not running yet: every call says so, nothing panics
    assert_eq!(err(service::configure(b"{}")), "backup.notRunning");
    assert_eq!(err(service::run_now()), "backup.notRunning");
    assert!(service::status_json().is_none() && service::list_json().is_none() && service::event_fd() < 0);

    assert!(service::start(&home, "te/ster"), "starts");
    assert!(!service::start(&home, "again"), "once");
    assert!(service::event_fd() >= 0);
    let s = until("the idle state", |s| s["state"] == "idle");
    assert_eq!((s["dest"].as_str(), s["locked"].as_bool(), s["destAvailable"].as_bool()), (Some(""), Some(false), Some(false)));
    assert_eq!(err(service::run_now()), "backup.noDestination");

    // configuration: validated before it is written
    assert_eq!(err(service::configure(b"not json")), "backup.badConfig");
    assert_eq!(err(service::configure(b"[]")), "backup.badConfig");
    assert_eq!(err(service::configure(br#"{"interval":"yearly"}"#)), "backup.badSchedule");
    assert_eq!(err(service::configure(br#"{"dest":"relative"}"#)), "backup.destMissing");
    assert_eq!(err(service::configure(br#"{"dest":"/etc"}"#)), "backup.destOutside");
    assert_eq!(err(service::configure(br#"{"exclude":["../x"]}"#)), "backup.badExclusion");
    fs::create_dir_all(home.join(".local/share/applications")).unwrap();
    assert_eq!(err(service::configure(config(&home.join(".local/share/applications"), "").as_bytes())), "backup.destOutside", "a protected folder");
    assert!(!home.join(".config/axia/backup.json").exists(), "a refused configuration is not written");
    service::configure(config(&dest, r#","exclude":["skip-me"]"#).as_bytes()).expect("a good configuration");
    let saved: Value = serde_json::from_slice(&fs::read(home.join(".config/axia/backup.json")).unwrap()).unwrap();
    assert_eq!(saved["interval"], "manual");
    assert_eq!(saved["exclude"][0], "skip-me");
    assert_eq!(saved["pauseOnBattery"], false);
    assert_eq!(serde_json::from_slice::<Value>(&service::config_json().unwrap()).unwrap(), saved);
    let s = status();
    assert_eq!((s["dest"].as_str(), s["destAvailable"].as_bool()), (dest.to_str(), Some(true)));
    assert!(s["destFree"].as_f64().unwrap() > 0.0);

    // a backup on request
    service::run_now().expect("run now");
    let s = until("the first backup", |s| s["last"]["id"].as_str().is_some_and(|i| !i.is_empty()) && s["state"] == "idle");
    let id1 = s["last"]["id"].as_str().unwrap().to_string();
    assert!(s["last"]["files"].as_u64().unwrap() >= 2 && s["last"]["errors"] == 0, "{s}");
    assert!(s["progress"]["totalFiles"].as_u64().unwrap() >= 2);
    let machine = dest.join("LordOS-Backups").join(format!("{}-te_ster", hostname()));
    assert!(machine.join(&id1).join("a.txt").exists() && machine.join(&id1).join("docs/b.txt").exists());
    assert!(!machine.join(&id1).join("skip-me").exists(), "the user's exclusion");
    let list: Value = serde_json::from_slice(&service::list_json().unwrap()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], id1.as_str());
    assert_eq!(service::snapshot_path(&id1).as_deref(), machine.join(&id1).canonicalize().ok().as_deref());
    assert!(service::snapshot_path("../../etc").is_none());
    let ls: Value = serde_json::from_slice(&service::ls_json(&id1, "").unwrap()).unwrap();
    let names: Vec<&str> = ls.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, [".config", ".local", "disk", "docs", "a.txt"], "folders first, by name");
    assert_eq!(ls[4]["size"], 10.0);
    assert_eq!((ls[3]["dir"].as_bool(), ls[4]["dir"].as_bool()), (Some(true), Some(false)));
    assert_eq!(String::from_utf8(service::ls_json(&id1, "../x").unwrap_err()).unwrap(), "backup.badPath");
    assert_eq!(String::from_utf8(service::ls_json("2020-01-01-000000", "").unwrap_err()).unwrap(), "backup.noSuchBackup");

    // a second one links what did not change
    fs::write(home.join("a.txt"), "version 2, longer\n").unwrap();
    service::run_now().unwrap();
    let s = until("the second backup", |s| s["last"]["id"].as_str().is_some_and(|i| !i.is_empty() && i != id1) && s["state"] == "idle");
    assert_eq!(s["last"]["copied"], 1, "{s}");
    assert!(s["last"]["linked"].as_u64().unwrap() >= 1);

    // restore the old version next to the current one, then wait for its result
    assert_eq!(err(service::restore(&id1, "a.txt", false, b"/x", Path::new(""))), "backup.badSuffix");
    assert_eq!(err(service::restore(&id1, "../a", false, b" (r)", Path::new(""))), "backup.badPath");
    assert_eq!(err(service::restore(&id1, "a.txt", false, b" (r)", Path::new("/etc"))), "backup.destNotAllowed");
    service::restore(&id1, "a.txt", false, b" (restored)", Path::new("")).unwrap();
    let s = until("the restore", |s| s["restore"]["done"] == true);
    assert_eq!((s["restore"]["ok"].as_bool(), s["restore"]["keptBoth"].as_u64(), s["restore"]["id"].as_str()), (Some(true), Some(1), Some(id1.as_str())));
    assert_eq!(fs::read_to_string(home.join("a (restored).txt")).unwrap(), "version 1\n");

    // pause is a flag the next backup clears
    service::pause(true);
    assert_eq!(status()["userPause"], true);
    service::pause(false);

    // delete the first snapshot
    service::delete(&id1).unwrap();
    until("the deletion", |_| service::list_json().is_some_and(|l| l.len() < 80 && !String::from_utf8_lossy(&l).contains(&id1)));
    assert_eq!(err(service::delete("not-an-id")), "backup.badId");

    // an encrypted destination: a new, empty one
    let dest2 = home.join("disk2");
    fs::create_dir(&dest2).unwrap();
    service::configure(config(&dest2, r#","encrypt":true"#).as_bytes()).unwrap();
    let s = status();
    assert_eq!((s["encrypted"].as_bool(), s["locked"].as_bool(), s["destEncrypted"].as_bool()), (Some(true), Some(true), Some(false)));
    assert_eq!(err(service::unlock(b"short", true)), "backup.passphraseShort");
    assert_eq!(err(service::unlock(b"", false)), "backup.badPassphrase");
    service::run_now().unwrap();
    until("the locked state", |s| s["state"] == "locked");
    assert!(!dest2.join("LordOS-Backups").exists() || fs::read_dir(dest2.join("LordOS-Backups")).unwrap().flatten().all(|m| fs::read_dir(m.path()).unwrap().count() == 0), "never copied in clear text");
    service::unlock(b"a long test passphrase", false).unwrap();
    let s = until("the failed unlock", |s| s["unlock"]["done"] == true);
    assert_eq!((s["unlock"]["ok"].as_bool(), s["unlock"]["error"].as_str()), (Some(false), Some("backup.notEncrypted")));
    service::unlock(b"a long test passphrase", true).unwrap();
    let s = until("the unlock", |s| s["unlock"]["done"] == true && s["state"] != "unlocking");
    assert_eq!((s["unlock"]["ok"].as_bool(), s["destEncrypted"].as_bool(), s["locked"].as_bool()), (Some(true), Some(true), Some(false)), "{s}");
    // the backup asked for while it was locked runs by itself once the passphrase is in
    let s = until("the encrypted backup", |s| s["last"]["id"].as_str().is_some_and(|i| !i.is_empty() && i != id1) && s["state"] == "idle" && s["destEncrypted"] == true);
    let id = s["last"]["id"].as_str().unwrap().to_string();
    let machine2 = dest2.join("LordOS-Backups").join(format!("{}-te_ster", hostname()));
    assert_eq!(&fs::read(machine2.join(&id).join("docs/b.txt")).unwrap()[..4], b"LBK1");
    let ls: Value = serde_json::from_slice(&service::ls_json(&id, "docs").unwrap()).unwrap();
    assert_eq!(ls[0]["size"], 2.0, "the listing shows the size of the content");
    service::lock();
    assert_eq!(status()["locked"], true);
    // a restore asked while locked is refused by the thread, with a code line
    if let Err(e) = service::restore(&id, "docs/b.txt", true, b" (r)", Path::new("")) {
        panic!("restore refused with {} while the status is {}", String::from_utf8_lossy(&e), status());
    }
    let s = until("the refused restore", |s| s["restore"]["done"] == true);
    assert_eq!((s["restore"]["ok"].as_bool(), s["restore"]["error"].as_str()), (Some(false), Some("backup.locked")), "{s}");

    let _ = fs::remove_dir_all(&t);
}

fn hostname() -> String {
    let mut name = [0u8; 64];
    unsafe { libc::gethostname(name.as_mut_ptr().cast(), 63) };
    let n = name.iter().position(|b| *b == 0).unwrap_or(63);
    String::from_utf8_lossy(&name[..n]).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}
