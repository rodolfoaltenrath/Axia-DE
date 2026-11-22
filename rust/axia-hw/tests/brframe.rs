//! Settings → Appearance → "Browsers: tabs in the title bar": Chromium's Preferences edit (malformed
//! JSON, missing keys, theme extensions, idempotence, the rest of the file kept byte for byte),
//! Firefox's user.js line and its tidy-up, running browsers, stale locks, missing and hostile profile
//! folders, over fake homes in a temporary folder.
mod support;

use axia_hw::brframe::*;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use support::*;

fn changed(e: Edit) -> String {
    match e {
        Edit::Changed(v) => String::from_utf8(v).unwrap(),
        other => panic!("expected a change, got {other:?}"),
    }
}

/// browser.custom_chrome_frame (-1 missing) and extensions.theme.system_theme (-1 missing).
fn chrome_values(text: &str) -> (i64, i64) {
    let v: Value = serde_json::from_str(text).unwrap();
    let frame = v["browser"]["custom_chrome_frame"].as_bool().map_or(-1, i64::from);
    let theme = v["extensions"]["theme"]["system_theme"].as_i64().unwrap_or(-1);
    (frame, theme)
}

fn get(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn exists(root: &Path, rel: &str) -> bool {
    fs::symlink_metadata(root.join(rel)).is_ok()
}

#[test]
fn chromium_preferences_are_edited_surgically() {
    let o = changed(chrome_edit(b"{}", true));
    assert_eq!(chrome_values(&o), (1, 1), "empty on");
    assert_eq!(chrome_edit(o.as_bytes(), true), Edit::Same, "second time is the same");
    assert_eq!(chrome_values(&changed(chrome_edit(b"{}", false))), (0, -1), "off writes only the frame");

    // the rest of the file is kept byte for byte
    let real = r#"{"a":"}\"{","browser":{"has_seen_welcome_page":true,"custom_chrome_frame":false,"window_placement":{"x":1}},"extensions":{"theme":{"system_theme":0}},"z":[1,2,{"q":null}]}"#;
    let o = changed(chrome_edit(real.as_bytes(), true));
    assert_eq!(o, r#"{"a":"}\"{","browser":{"has_seen_welcome_page":true,"custom_chrome_frame":true,"window_placement":{"x":1}},"extensions":{"theme":{"system_theme":1}},"z":[1,2,{"q":null}]}"#);
    assert_eq!(chrome_edit(o.as_bytes(), true), Edit::Same, "idempotent");
    assert_eq!(chrome_values(&changed(chrome_edit(o.as_bytes(), false))), (0, 1), "off keeps the GTK theme");

    // pretty-printed, members missing in an existing object
    let o = changed(chrome_edit(b"{\n  \"browser\": {\n    \"x\": 1\n  },\n  \"extensions\": { }\n}\n", true));
    assert_eq!(chrome_values(&o), (1, 1), "pretty");

    // a theme extension the user installed: the frame changes, the theme doesn't
    let o = changed(chrome_edit(br#"{"extensions":{"theme":{"id":"abcdefghijklmnopabcdefghijklmnop","system_theme":0}}}"#, true));
    assert_eq!(chrome_values(&o), (1, 0), "theme extension kept");
    assert_eq!(chrome_edit(br#"{"browser":{"custom_chrome_frame":true},"extensions":{"theme":{"id":"x"}}}"#, true), Edit::Same, "theme ext, already on");

    // malformed or unexpected: refused, never guessed
    let bad: [&str; 12] = [
        "", "{", r#"{"browser":{"custom_chrome_frame":tru}}"#, "{} x", "[1,2]", "\"str\"", "null",
        r#"{"browser":5}"#, r#"{"browser":{"custom_chrome_frame":true},"extensions":{"theme":7}}"#,
        r#"{"browser":{},"browser":{}}"#, // duplicate keys
        r#"{"\u0062rowser":{"x":1}}"#, // the same key, escaped: inserting would duplicate it
        r#"{"a":"\u0000"}"#,
    ];
    for (i, b) in bad.iter().enumerate() {
        assert_eq!(chrome_edit(b.as_bytes(), true), Edit::Bad, "bad[{i}] {b}");
    }
}

#[test]
fn firefox_user_js_lines() {
    let (e, own) = userjs_edit(b"", 0);
    let o = changed(e);
    assert!(!own);
    assert_eq!((userjs_ours(o.as_bytes()), prefs_value(o.as_bytes())), (0, 0), "line with 0: {o}");
    assert_eq!(userjs_edit(o.as_bytes(), 0).0, Edit::Same, "same value again");
    let o2 = changed(userjs_edit(o.as_bytes(), 1).0);
    assert_eq!(userjs_ours(o2.as_bytes()), 1, "replaced by 1");
    assert_eq!(o2.matches("inTitlebar").count(), 1, "only one line: {o2}");
    assert_eq!(changed(userjs_edit(o2.as_bytes(), -1).0), "", "removed: empty");

    // other lines are kept, with or without a final newline
    let o = changed(userjs_edit(b"user_pref(\"a.b\", true);\n// note", 1).0);
    assert!(o.starts_with("user_pref(\"a.b\", true);\n// note\nuser_pref(\"browser.tabs.inTitlebar\", 1);"), "appended: {o}");
    assert_eq!(changed(userjs_edit(o.as_bytes(), -1).0), "user_pref(\"a.b\", true);\n// note\n", "removed again");
    assert_eq!(userjs_edit(b"user_pref(\"a.b\", true);\n", -1).0, Edit::Same, "nothing of ours to remove");

    // the user's own line (either quotes): theirs, untouched
    assert_eq!(userjs_edit(b"user_pref(\"browser.tabs.inTitlebar\", 0);\n", 1), (Edit::Same, true), "own line kept");
    assert_eq!(userjs_edit(b"  user_pref('browser.tabs.inTitlebar', 1);\n", 0), (Edit::Same, true), "own line, single quotes");
    let (e, own) = userjs_edit(b"// user_pref(\"browser.tabs.inTitlebar\", 0);\n", 1);
    assert!(matches!(e, Edit::Changed(_)) && !own, "a comment isn't a line");
    let (e, own) = userjs_edit(b"user_pref(\"browser.tabs.inTitlebarX\", 0);\n", 1);
    assert!(matches!(e, Edit::Changed(_)) && !own, "another pref name");
    assert_eq!(userjs_edit(b"a\0b", 1).0, Edit::Bad, "NUL bytes refused");

    // prefs.js values
    let pj = "// Mozilla User Preferences\nuser_pref(\"browser.startup.page\", 3);\nuser_pref(\"browser.tabs.inTitlebar\", 0);\n";
    assert_eq!(prefs_value(pj.as_bytes()), 0);
    assert_eq!(prefs_value(b""), -1, "unset");
    assert_eq!(prefs_value(b"user_pref(\"browser.tabs.inTitlebar\", true);"), 1, "true");
}

// ---- on disk ----

fn child_named(name: &str) -> Child {
    // a copy of `sleep` under the name the browser has: the process name is the file name
    let dir = temp_dir("proc");
    let exe = dir.join(name);
    fs::copy("/bin/sleep", &exe).or_else(|_| fs::copy("/usr/bin/sleep", &exe)).unwrap();
    let c = Command::new(&exe).arg("1000").spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(80));
    c
}

fn stop(mut c: Child) {
    let _ = c.kill();
    let _ = c.wait();
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn hostname() -> String {
    let mut b = [0u8; 256];
    unsafe { libc::gethostname(b.as_mut_ptr().cast(), 255) };
    let n = b.iter().position(|&c| c == 0).unwrap();
    String::from_utf8_lossy(&b[..n]).into_owned()
}

#[test]
fn chromium_profiles_on_disk() {
    let root = temp_dir("chromium");
    let cfg_p: PathBuf = root.join("h1/.config");
    fs::create_dir_all(&cfg_p).unwrap();
    let cfg = s(&cfg_p);
    let r = chromium_apply(cfg, true);
    assert!(r.found == 0 && r.profiles == 0 && r.changed == 0, "no chromium folder");
    assert!(!chromium_running(cfg), "no folder: not running");

    let orig = r#"{"browser":{"custom_chrome_frame":false},"zz":1}"#;
    put(&root, "h1/.config/chromium/Default/Preferences", orig);
    put(&root, "h1/.config/chromium/Profile 1/Preferences", "{}");
    put(&root, "h1/.config/chromium/Profile 2/Preferences", "{\"browser\":"); // truncated
    put(&root, "h1/.config/chromium/System Profile/Preferences", "{}"); // not a user profile
    put(&root, "h1/.config/chromium/Profile x/Preferences", "{}");
    put(&root, "h1/.config/chromium/Profile 3/.keep", ""); // never started
    put(&root, "h1/target.json", "{}");
    fs::create_dir_all(root.join("h1/.config/chromium/Profile 4")).unwrap();
    lnk(&root, "h1/.config/chromium/Profile 4/Preferences", "../../../target.json"); // a link: refused
    fs::set_permissions(root.join("h1/.config/chromium/Default/Preferences"), fs::Permissions::from_mode(0o600)).unwrap();
    let before2 = get(&root, "h1/.config/chromium/Profile 2/Preferences");
    assert_eq!(chromium_state(cfg), (0, 1), "state before: unset and malformed not counted");

    // running: a live "chromium" process of ours holds the lock → nothing is written
    let host = hostname();
    let ch = child_named("chromium");
    lnk(&root, "h1/.config/chromium/SingletonLock", &format!("{host}-{}", ch.id()));
    assert!(chromium_running(cfg), "running");
    let r = chromium_apply(cfg, true);
    assert!(r.found == 1 && r.running == 1 && r.changed == 0 && get(&root, "h1/.config/chromium/Default/Preferences") == orig, "running: untouched");
    let dead = ch.id();
    stop(ch);

    // a stale lock (dead pid, or a pid that isn't chromium) doesn't stop it; another host does
    lnk(&root, "h1/.config/chromium/SingletonLock", &format!("{host}-{}", std::process::id()));
    assert!(!chromium_running(cfg), "our own pid isn't chromium");
    lnk(&root, "h1/.config/chromium/SingletonLock", "some-other-host-12");
    assert!(chromium_running(cfg), "another computer counts as running");
    lnk(&root, "h1/.config/chromium/SingletonLock", &format!("{host}-{dead}"));
    assert!(!chromium_running(cfg), "dead pid: stale");

    let r = chromium_apply(cfg, true);
    assert!(r.found == 1 && r.running == 0 && r.profiles == 4 && r.changed == 2 && r.failed == 2 && r.unchanged == 0, "apply: {r:?}");
    let d = get(&root, "h1/.config/chromium/Default/Preferences");
    assert_eq!(chrome_values(&d), (1, 1), "Default on");
    assert!(d.contains("\"zz\":1"), "other keys kept");
    assert_eq!(get(&root, "h1/.config/chromium/Default/Preferences.lordos-backup"), orig, "backup is the original");
    assert_eq!(chrome_values(&get(&root, "h1/.config/chromium/Profile 1/Preferences")), (1, 1), "Profile 1 on");
    assert!(get(&root, "h1/.config/chromium/Profile 2/Preferences") == before2 && !exists(&root, "h1/.config/chromium/Profile 2/Preferences.lordos-backup"), "malformed profile untouched, no backup");
    assert_eq!(get(&root, "h1/target.json"), "{}", "a linked Preferences is never followed");
    assert!(get(&root, "h1/.config/chromium/System Profile/Preferences") == "{}" && get(&root, "h1/.config/chromium/Profile x/Preferences") == "{}", "other folders untouched");
    assert!(!exists(&root, "h1/.config/chromium/Default/Preferences.lordos-tmp"), "no temporary file left");
    let mode = fs::metadata(root.join("h1/.config/chromium/Default/Preferences")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "mode kept: {mode:o}");

    assert_eq!(chromium_state(cfg), (2, 0), "state after on");
    let r = chromium_apply(cfg, true);
    assert!(r.changed == 0 && r.unchanged == 2 && r.failed == 2, "idempotent: {r:?}");
    let r = chromium_apply(cfg, false);
    assert_eq!(r.changed, 2, "off");
    assert_eq!(chrome_values(&get(&root, "h1/.config/chromium/Default/Preferences")), (0, 1), "Default off");
    assert_eq!(chromium_state(cfg), (0, 2), "state after off");
}

fn touch_later(root: &Path, rel: &str, secs: i64) {
    let c = std::ffi::CString::new(s(&root.join(rel))).unwrap();
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
    now.tv_sec += secs;
    let ts = [now, now];
    let r = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), ts.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    assert_eq!(r, 0, "utimensat {rel}");
}

#[test]
fn firefox_profiles_on_disk() {
    let root = temp_dir("firefox");
    let home_p = root.join("h2");
    let cfg_p = root.join("h2/.config");
    fs::create_dir_all(&cfg_p).unwrap();
    let (cfg, home) = (s(&cfg_p), s(&home_p));
    let r = firefox_apply(cfg, home, true);
    assert!(r.found == 0 && r.profiles == 0, "no Firefox folders");

    put(&root, "h2/.config/mozilla/firefox/profiles.ini",
        "[Profile1]\nName=default\nIsRelative=1\nPath=aaaa.default\nDefault=1\n\n\
[Profile0]\r\nName=default-esr\r\nIsRelative=1\r\nPath=bbbb.default-esr\r\n\r\n\
[Profile2]\nName=abs\nIsRelative=0\nPath=/tmp\n\n\
[Profile3]\nName=evil\nIsRelative=1\nPath=../../etc\n\n\
[Profile4]\nName=missing\nIsRelative=1\nPath=nothere\n\n\
[Install3B6073811A6ABF12]\nDefault=bbbb.default-esr\nLocked=1\n\n\
[General]\nStartWithLastProfile=1\nVersion=2"); // no final newline
    put(&root, "h2/.config/mozilla/firefox/aaaa.default/prefs.js", "user_pref(\"browser.tabs.inTitlebar\", 1);\n");
    put(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/prefs.js", "user_pref(\"x\", 1);\n");
    put(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/user.js", "user_pref(\"x.y\", 2);");
    // legacy folder with a profile whose user wrote the line themselves
    put(&root, "h2/.mozilla/firefox/profiles.ini", "[Profile0]\nIsRelative=1\nPath=cccc.mine\n");
    put(&root, "h2/.mozilla/firefox/cccc.mine/user.js", "user_pref(\"browser.tabs.inTitlebar\", 0);\n");

    let r = firefox_apply(cfg, home, true);
    assert!(r.found == 1 && r.profiles == 3 && r.changed == 1 && r.unchanged == 1 && r.kept == 1 && r.failed == 0, "apply on: {r:?}");
    assert!(!exists(&root, "h2/.config/mozilla/firefox/aaaa.default/user.js"), "prefs.js already 1: no line needed");
    let u = get(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/user.js");
    assert!(u.starts_with("user_pref(\"x.y\", 2);\nuser_pref(\"browser.tabs.inTitlebar\", 1);"), "line appended: {u}");
    assert_eq!(get(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/user.js.lordos-backup"), "user_pref(\"x.y\", 2);", "user.js backup");
    assert_eq!(get(&root, "h2/.mozilla/firefox/cccc.mine/user.js"), "user_pref(\"browser.tabs.inTitlebar\", 0);\n", "own line untouched");

    let r = firefox_apply(cfg, home, true);
    assert!(r.changed == 0 && r.unchanged == 2 && r.kept == 1, "idempotent: {r:?}");

    // off: both profiles get the line (aaaa's prefs.js says 1)
    let r = firefox_apply(cfg, home, false);
    assert_eq!(r.changed, 2, "off: {r:?}");
    let a = get(&root, "h2/.config/mozilla/firefox/aaaa.default/user.js");
    assert_eq!(userjs_ours(a.as_bytes()), 0, "aaaa 0");
    let mode = fs::metadata(root.join("h2/.config/mozilla/firefox/aaaa.default/user.js")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "new user.js is 0600");

    // tidy: not before Firefox has started with the line. prefs.js written after it is no proof
    // (Firefox writes it when it quits, also after a line written while it ran)
    touch_later(&root, "h2/.config/mozilla/firefox/aaaa.default/prefs.js", 100);
    touch_later(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/prefs.js", 100);
    assert_eq!(firefox_tidy(cfg, home), 0, "no start since the line: nothing to tidy");
    put(&root, "h2/.config/mozilla/firefox/aaaa.default/.parentlock", "");
    touch_later(&root, "h2/.config/mozilla/firefox/aaaa.default/.parentlock", -100);
    lnk(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/lock", "127.0.1.1:+1");
    touch_later(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/lock", -100);
    assert_eq!(firefox_tidy(cfg, home), 0, "last start before the line: nothing to tidy");

    // running: a process holds .parentlock → the line is written anyway (read at the next start),
    // counted as running, and tidy waits
    put(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/.parentlock", "");
    let lockfile = root.join("h2/.config/mozilla/firefox/bbbb.default-esr/.parentlock");
    let ff = hold_lock(&lockfile);
    let r = firefox_apply(cfg, home, true);
    assert!(r.changed == 2 && r.running == 1, "running profile: {r:?}");
    // both profiles "started" after the line; only the closed one is tidied
    touch_later(&root, "h2/.config/mozilla/firefox/aaaa.default/.parentlock", 100);
    touch_later(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/.parentlock", 100);
    assert_eq!(firefox_tidy(cfg, home), 1, "tidy only the closed profile");
    assert!(!exists(&root, "h2/.config/mozilla/firefox/aaaa.default/user.js"), "a user.js that held only our line is removed");
    stop_pid(ff);

    // the "lock" link of a live firefox counts too; a stale one doesn't
    let fx = child_named("firefox-esr");
    lnk(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/lock", &format!("127.0.1.1:+{}", fx.id()));
    touch_later(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/lock", 100);
    assert_eq!(firefox_tidy(cfg, home), 0, "lock link of a live firefox: wait");
    stop(fx);
    assert_eq!(firefox_tidy(cfg, home), 1, "stale lock link: tidied");
    assert_eq!(get(&root, "h2/.config/mozilla/firefox/bbbb.default-esr/user.js"), "user_pref(\"x.y\", 2);\n", "other lines kept");
    assert_eq!(firefox_tidy(cfg, home), 0, "tidy is idempotent");
    assert_eq!(get(&root, "h2/.mozilla/firefox/cccc.mine/user.js"), "user_pref(\"browser.tabs.inTitlebar\", 0);\n", "own line still untouched");

    // a user.js that is a link is refused
    lnk(&root, "h2/.config/mozilla/firefox/aaaa.default/user.js", "../../../../target");
    put(&root, "h2/target", "keep");
    let r = firefox_apply(cfg, home, false);
    assert!(r.failed == 1 && get(&root, "h2/target") == "keep", "linked user.js refused: {r:?}");
}

/// A forked process holding an fcntl write lock on `file` (as Firefox does on .parentlock); returns once
/// the lock is held. Only async-signal-safe calls run in the child.
fn hold_lock(file: &Path) -> i32 {
    let path = std::ffi::CString::new(s(file)).unwrap();
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_RDWR);
            let mut fl: libc::flock = std::mem::zeroed();
            fl.l_type = libc::F_WRLCK as libc::c_short;
            fl.l_whence = libc::SEEK_SET as libc::c_short;
            libc::fcntl(fd, libc::F_SETLK, &fl);
            libc::write(fds[1], b"k".as_ptr().cast(), 1);
            loop {
                libc::pause();
            }
        }
    }
    let mut k = [0u8; 1];
    let n = unsafe { libc::read(fds[0], k.as_mut_ptr().cast(), 1) };
    assert_eq!(n, 1);
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    pid
}

fn stop_pid(pid: i32) {
    unsafe {
        libc::kill(pid, libc::SIGKILL);
        libc::waitpid(pid, std::ptr::null_mut(), 0);
    }
}

#[test]
fn librewolf_profiles_on_disk() {
    let root = temp_dir("librewolf");
    let (cfg, home) = (s(&root.join("h3/.config")).to_string(), s(&root.join("h3")).to_string());
    put(&root, "h3/.config/librewolf/librewolf/profiles.ini", "[Profile0]\nIsRelative=1\nPath=abcd.default\n");
    put(&root, "h3/.config/librewolf/librewolf/abcd.default/user.js", "// keep my preferences\nuser_pref(\"other.option\", true);\n");
    let r = firefox_apply(&cfg, &home, true);
    assert!(r.found == 1 && r.profiles == 1 && r.changed == 1 && r.failed == 0, "LibreWolf Fedora profile discovered once: {r:?}");
    assert!(get(&root, "h3/.config/librewolf/librewolf/abcd.default/user.js").contains("other.option"), "other preferences kept");
    let r = firefox_apply(&cfg, &home, false);
    let u = get(&root, "h3/.config/librewolf/librewolf/abcd.default/user.js");
    assert!(r.changed == 1 && userjs_ours(u.as_bytes()) == 0, "frame choice reversible");
}

#[test]
fn the_result_is_json() {
    let a = BrfResult { found: 1, profiles: 2, changed: 1, unchanged: 1, running: 0, kept: 0, failed: 0 };
    let j = result_json(&a, &BrfResult::default());
    let v: Value = serde_json::from_str(&j).unwrap_or_else(|e| panic!("{e}: {j}"));
    assert!(v.is_object(), "{j}");
}
