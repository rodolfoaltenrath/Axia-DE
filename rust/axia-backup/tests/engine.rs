//! The engine against real folders (this replaces server/tests/backup_test.c): retention, names,
//! encryption, hard-linked snapshots, an interrupted run that resumes, restore and its refusals.
use axia_backup::crypt::{self, Key, CHUNK, HDR, KEY};
use axia_backup::engine::{self, Outcome, Progress, RestoreStats, Run, DIRNAME};
use std::fs;
use std::io::Write;
use std::os::fd::IntoRawFd;
use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

/// The folder policy is global to the process: the tests that restore take turns.
static POLICY: Mutex<()> = Mutex::new(());

fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("axia-backup-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

fn cleanup(p: &Path) {
    // folders restored with mode 0500 could not be emptied
    let _ = std::process::Command::new("chmod").arg("-R").arg("u+w").arg(p).status();
    let _ = fs::remove_dir_all(p);
}

fn ino(p: &Path) -> u64 {
    fs::symlink_metadata(p).map(|m| m.ino()).unwrap_or(0)
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn put(home: &Path, rel: &str, text: &str) {
    fs::write(home.join(rel), text).unwrap();
}

fn put_big(home: &Path, rel: &str, mb: usize) {
    let mut f = fs::File::create(home.join(rel)).unwrap();
    for i in 0..mb {
        f.write_all(&vec![b'a' + (i % 26) as u8; 1 << 20]).unwrap();
    }
}

// ---- retention ----

#[test]
fn retention_keeps_a_day_a_week_and_the_newest() {
    // one per hour for 90 days, ending now
    const N: usize = 90 * 24;
    let now: i64 = 1_792_000_000; // 2026
    let t: Vec<i64> = (0..N).map(|i| now - (N - 1 - i) as i64 * 3600).collect();
    let keep = engine::retention(&t, now, 0);
    let (mut recent, mut total) = (0, 0);
    for i in 0..N {
        total += keep[i] as usize;
        if now - t[i] < 86400 {
            recent += 1;
            assert!(keep[i], "everything from the last 24 h stays");
        }
    }
    assert_eq!(recent, 24);
    assert!(keep[N - 1], "the newest stays");
    // between 1 and 30 days: one per calendar day
    let mut days = [0; 40];
    for i in 0..N {
        let age = now - t[i];
        if keep[i] && (86400..30 * 86400).contains(&age) {
            days[((t[i] / 86400) % 40) as usize] += 1;
        }
    }
    assert!(days.iter().all(|d| *d <= 1), "at most one per day {days:?}");
    // after 30 days: one per week
    let mut weeks: Vec<i64> = vec![];
    for i in 0..N {
        if !keep[i] || now - t[i] < 30 * 86400 {
            continue;
        }
        let w = (t[i] - 4 * 86400) / (7 * 86400);
        assert!(!weeks.contains(&w), "two from the same week");
        weeks.push(w);
    }
    assert!((8..=10).contains(&weeks.len()), "weekly between 30 and 90 days ({})", weeks.len());
    assert!(total < 24 + 31 + 11, "reasonable total ({total})");
    // small cases
    assert_eq!(engine::retention(&[now], now, 0), [true]);
    assert!(engine::retention(&[], now, 0).is_empty());
    let old = [now - 100 * 86400, now - 99 * 86400, now - 98 * 86400];
    assert!(engine::retention(&old, now, 0)[2], "the newest stays even if old");
    // time zone: two on the same local day but on different UTC days
    let tz = [1_791_900_000 - 3600 * 2, 1_791_900_000 + 3600 * 2];
    let off = -(1_791_900_000 % 86400);
    assert!(engine::retention(&tz, tz[1] + 5 * 86400, off + 86400 / 2)[1], "time zone: the newest stays");
}

#[test]
fn names_and_relative_paths_are_validated() {
    assert!(engine::rel_valid(b""));
    assert!(engine::rel_valid(b"a/b c/d.txt"));
    for bad in [&b"/etc"[..], b"a/../b", b"..", b"a//b", b"a/./b", b"a/", b"a\nb", b"a\x7fb"] {
        assert!(!engine::rel_valid(bad), "{}", String::from_utf8_lossy(bad));
    }
    assert!(engine::id_valid(b"2026-10-17-235959"));
    assert!(!engine::id_valid(b"2026-10-17-23595"));
    assert!(!engine::id_valid(b"../10-17-235959x"));
    let now: i64 = 1_792_000_123;
    let id = engine::id_make(now);
    assert!(engine::id_valid(id.as_bytes()));
    assert_eq!(engine::id_time(id.as_bytes()), Some(now), "id round trip");
    assert_eq!(engine::id_time(b"nonsense"), None);
}

// ---- encryption ----

fn encrypt_file(plain: &Path, out: &Path, key: &Key) -> i32 {
    let (i, o) = (fs::File::open(plain).unwrap().into_raw_fd(), fs::File::create(out).unwrap().into_raw_fd());
    let r = crypt::encrypt_fd(i, o, key, &mut |_| false);
    unsafe {
        libc::close(i);
        libc::close(o);
    }
    r
}

fn decrypt_file(enc: &Path, out: &Path, key: &Key) -> i32 {
    let (i, o) = (fs::File::open(enc).unwrap().into_raw_fd(), fs::File::create(out).unwrap().into_raw_fd());
    let r = crypt::decrypt_fd(i, o, key, &mut |_| false);
    unsafe {
        libc::close(i);
        libc::close(o);
    }
    r
}

#[test]
fn encryption_sizes_round_trips_and_tampering() {
    let sizes: [u64; 10] = [0, 1, 15, 16, 65535, 65536, 65537, 131072, 200_000, 1 << 20];
    for s in sizes {
        assert_eq!(crypt::plain_size(crypt::enc_size(s)), Some(s), "size {s} round trip");
    }
    assert_eq!(crypt::plain_size(HDR as u64 + 5), None, "impossible size rejected");
    let t = temp("crypt");
    let machine = t.join("encrypted").join(DIRNAME).join("host-user");
    fs::create_dir_all(&machine).unwrap();
    let key = crypt::setup(&machine, b"a long test passphrase").expect("set up encryption");
    assert_eq!(crypt::unlock(&machine, b"a long test passphrase"), Ok(key), "unlock with the passphrase");
    assert_eq!(crypt::unlock(&machine, b"another passphrase"), Err(-1), "wrong passphrase");
    assert_eq!(crypt::unlock(&t, b"x"), Err(-2), "a destination without encryption");
    for (i, s) in sizes.iter().enumerate() {
        let (plain, enc, back) = (t.join("plain.bin"), t.join("encrypted.bin"), t.join("back.bin"));
        fs::write(&plain, (0..*s).map(|k| ((k * 7 + i as u64) & 0xff) as u8).collect::<Vec<_>>()).unwrap();
        assert_eq!(encrypt_file(&plain, &enc, &key), 0);
        assert_eq!(fs::metadata(&enc).unwrap().len(), crypt::enc_size(*s), "encrypt {s}");
        assert_eq!(decrypt_file(&enc, &back, &key), 0);
        assert_eq!(fs::read(&plain).unwrap(), fs::read(&back).unwrap(), "decrypt {s} identical");
        if *s == 200_000 {
            // one changed byte, a missing chunk and the wrong key: fails without returning bad data
            let mut data = fs::read(&enc).unwrap();
            data[70000] ^= 1;
            fs::write(&enc, &data).unwrap();
            assert_eq!(decrypt_file(&enc, &back, &key), -3, "tampered byte detected");
            data[70000] ^= 1;
            data.truncate(crypt::enc_size(2 * CHUNK as u64) as usize);
            fs::write(&enc, &data).unwrap();
            assert_eq!(decrypt_file(&enc, &back, &key), -3, "missing chunks detected");
            let mut bad = key;
            bad[0] ^= 1;
            assert_eq!(decrypt_file(&enc, &back, &bad), -3, "wrong key detected");
        }
    }
    // a file that is not ours, and one cut inside the header
    fs::write(t.join("junk"), b"not a backup file").unwrap();
    assert_eq!(decrypt_file(&t.join("junk"), &t.join("o"), &key), -3);
    cleanup(&t);
}

#[test]
fn a_file_written_by_the_c_implementation_still_decrypts() {
    // rust/axia-crypto/tests/fixtures/legacy.lbk, made by the old C code: the key comes from this
    // public test passphrase and salt (Argon2id), as in the vault/backup format tests
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../axia-crypto/tests/fixtures/legacy.lbk");
    let t = temp("legacy");
    let out = t.join("out");
    let mut key = [0u8; KEY];
    assert!(crypt::derive(b"Axia compatibility fixture - public test password", &[0x33; crypt::SALT], &mut key));
    assert_eq!(decrypt_file(&fixture, &out, &key), 0);
    let expected: Vec<u8> = (0..65537).map(|i| (i % 251) as u8).collect();
    assert_eq!(fs::read(&out).unwrap(), expected);
    cleanup(&t);
}

// ---- snapshots and restore ----

struct World {
    home: PathBuf,
    machine: PathBuf,
    skip: PathBuf,
}

impl World {
    fn snap(&self, id: &str, rel: &str) -> PathBuf {
        self.machine.join(id).join(rel)
    }

    /// A snapshot at `now` with the usual exclusions; `cut_after`: stop after that many bytes.
    fn snapshot(&self, now: i64, cut_after: u64, machine: Option<&Path>, key: Option<&Key>, extra_excl: &[&str]) -> (Outcome, Progress) {
        let stats = Progress::new();
        let stats_ref = &stats;
        let mut checkpoint = move || cut_after != 0 && stats_ref.bytes.load(Relaxed) >= cut_after;
        let mut excl: Vec<String> = extra_excl.iter().map(|s| s.to_string()).collect();
        if extra_excl.is_empty() {
            excl = vec![".cache".into(), "Excluded".into()];
        }
        let mut run = Run {
            home: &self.home,
            machine: machine.unwrap_or(&self.machine),
            excl,
            skip_real: Some(self.skip.clone()),
            cancel: None,
            checkpoint: Some(&mut checkpoint),
            progress: None,
            min_free: 1,
            key,
            stats: &stats,
        };
        let outcome = engine::snapshot(&mut run, now);
        (outcome, stats)
    }

    fn restore(&self, id: &str, rel: &str, base: &Path, replace: bool, suffix: &[u8], key: Option<&Key>) -> (i32, RestoreStats) {
        let mut out = RestoreStats::default();
        let r = engine::restore(&self.machine, id, rel, base, replace, suffix, None, key, &mut out);
        (r, out)
    }
}

#[test]
fn snapshots_resume_restore_and_refuse() {
    let _turn = POLICY.lock().unwrap_or_else(|e| e.into_inner());
    let t = temp("engine");
    let home = t.join("home");
    fs::create_dir(&home).unwrap();
    let home = home.canonicalize().unwrap();
    // the destination INSIDE the home folder
    let dest = home.join("dest");
    let machine = dest.join(DIRNAME).join("host-user");
    axia_fs::policy::init(&home, "test-user");

    put(&home, "a.txt", "version 1\n");
    fs::create_dir(home.join("dir")).unwrap();
    put_big(&home, "dir/b.bin", 1);
    fs::create_dir_all(home.join("sub/deep")).unwrap();
    put(&home, "sub/deep/c.txt", "c\n");
    fs::create_dir(home.join(".cache")).unwrap();
    put(&home, ".cache/junk", "x");
    fs::create_dir(home.join("Excluded")).unwrap();
    put(&home, "Excluded/x", "x");
    fs::create_dir_all(home.join(".config/axia")).unwrap();
    put(&home, ".config/axia/settings.json", "{\"language\":\"es\"}\n");
    symlink("/etc/passwd", home.join("escape")).unwrap();
    symlink("a.txt", home.join("relative")).unwrap();
    assert_eq!(unsafe { libc::mkfifo(home.join("fifo").to_str().map(|s| std::ffi::CString::new(s).unwrap()).unwrap().as_ptr(), 0o600) }, 0);

    let t0 = 1_792_000_000 - 7200;
    fs::create_dir_all(dest.join(DIRNAME)).unwrap();
    let w = World { skip: dest.join(DIRNAME).canonicalize().unwrap(), home: home.clone(), machine: machine.clone() };

    // the destination inside home: skipped by dev/ino
    let (o1, s1) = w.snapshot(t0, 0, None, None, &[]);
    assert_eq!(o1.code, 0, "first snapshot: {}", String::from_utf8_lossy(&o1.error));
    let id1 = o1.id.clone();
    assert_eq!(fs::read_to_string(w.snap(&id1, "a.txt")).unwrap(), "version 1\n", "a.txt copied");
    assert!(exists(&w.snap(&id1, "sub/deep/c.txt")), "deep folders");
    assert!(!exists(&w.snap(&id1, ".cache")), ".cache excluded");
    assert!(!exists(&w.snap(&id1, "Excluded")), "user exclusion");
    assert!(!exists(&w.snap(&id1, "fifo")), "fifo skipped");
    assert!(!exists(&w.snap(&id1, &format!("dest/{DIRNAME}"))), "the destination is not copied into itself");
    assert!(fs::symlink_metadata(w.snap(&id1, "escape")).unwrap().file_type().is_symlink(), "symlink stored as a symlink");
    assert_eq!(fs::read_link(w.snap(&id1, "escape")).unwrap(), Path::new("/etc/passwd"), "with the same target text");
    assert!(Progress::get(&s1.skipped) >= 1, "special files counted");
    assert!(fs::symlink_metadata(home.join("fifo")).unwrap().file_type().is_fifo());

    // second: a.txt changes, the rest is linked
    put(&home, "a.txt", "version 2, a bit longer\n");
    let (o2, s2) = w.snapshot(t0 + 3600, 0, None, None, &[]);
    assert_eq!(o2.code, 0, "second snapshot");
    let id2 = o2.id.clone();
    assert!(id1 < id2, "ids in order");
    assert!(ino(&w.snap(&id1, "dir/b.bin")) != 0 && ino(&w.snap(&id1, "dir/b.bin")) == ino(&w.snap(&id2, "dir/b.bin")), "dedup: same inode");
    assert_ne!(ino(&w.snap(&id1, "a.txt")), ino(&w.snap(&id2, "a.txt")), "changed: new copy");
    assert_eq!(Progress::get(&s2.copied), 1, "only one was copied");
    assert!(Progress::get(&s2.linked) >= 3, "linked");

    // interrupted and resumed run
    put(&home, "aaa-new.txt", "new before the interruption\n");
    put_big(&home, "zz-big.bin", 40);
    let (o3, _) = w.snapshot(t0 + 5400, 12 << 20, None, None, &[]);
    assert_eq!(o3.code, 1, "interrupted");
    let stamp = engine::id_make(t0 + 5400);
    let inprog = machine.join(format!(".inprogress-{stamp}"));
    assert!(exists(&inprog), ".inprogress is left");
    assert_eq!(engine::list(&machine, 16).len(), 2, "the interrupted run is not listed");
    assert!(!exists(&inprog.join("zz-big.bin")), "the half-copied file has no final name");
    let copied_before = ino(&inprog.join("aaa-new.txt"));
    assert_ne!(copied_before, 0, "what was copied before the interruption is there");
    let (o4, s4) = w.snapshot(t0 + 5500, 0, None, None, &[]);
    assert_eq!(o4.code, 0, "resumed: {}", String::from_utf8_lossy(&o4.error));
    let id3 = o4.id.clone();
    assert_eq!(ino(&w.snap(&id3, "aaa-new.txt")), copied_before, "resumed: already copied files are linked (not copied again)");
    assert_eq!(Progress::get(&s4.copied), 1, "resumed: only the big one");
    assert_eq!(fs::metadata(w.snap(&id3, "zz-big.bin")).unwrap().len(), 40 << 20, "the big file is complete");
    assert!(!exists(&inprog), "no .inprogress");
    let junk = fs::read_dir(&machine).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.')).count();
    assert_eq!(junk, 0, "no leftovers of the interrupted run");

    // restore
    let suffix = b" (restored)";
    fs::remove_file(home.join("a.txt")).unwrap();
    let (r, o) = w.restore(&id1, "a.txt", &home, false, suffix, None);
    assert!(r == 0 && o.files == 1, "restore a deleted file ({r})");
    assert_eq!(fs::read_to_string(home.join("a.txt")).unwrap(), "version 1\n", "with the content it had then");
    put(&home, "a.txt", "version 3 current\n");
    let (r, o) = w.restore(&id1, "a.txt", &home, false, suffix, None);
    assert!(r == 0 && o.kept_both == 1, "keep both");
    assert_eq!(fs::read_to_string(home.join("a (restored).txt")).unwrap(), "version 1\n");
    assert_eq!(fs::read_to_string(home.join("a.txt")).unwrap(), "version 3 current\n", "the current file is not overwritten");
    let (r, _) = w.restore(&id1, "a.txt", &home, false, suffix, None);
    assert!(r == 0 && exists(&home.join("a (restored) 2.txt")), "second duplicate is numbered");
    let (r, o) = w.restore(&id1, "a.txt", &home, true, suffix, None);
    assert!(r == 0 && o.replaced == 1, "replace");
    assert_eq!(fs::read_to_string(home.join("a.txt")).unwrap(), "version 1\n");
    // a folder that is gone
    fs::remove_file(home.join("sub/deep/c.txt")).unwrap();
    fs::remove_dir(home.join("sub/deep")).unwrap();
    let (r, _) = w.restore(&id2, "sub/deep/c.txt", &home, false, suffix, None);
    assert!(r == 0 && exists(&home.join("sub/deep/c.txt")), "restore creates missing folders");

    // protected: a .desktop planted in the backup and the settings are not written
    let plant = w.snap(&id3, ".local/share/applications");
    fs::set_permissions(w.snap(&id3, ""), fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir_all(&plant).unwrap();
    fs::write(plant.join("evil.desktop"), "[Desktop Entry]\nExec=rm -rf ~\n").unwrap();
    put(&home, ".config/axia/settings.json", "{\"language\":\"pt\"}\n");
    let (r, o) = w.restore(&id3, "", &home, true, suffix, None);
    assert_eq!(r, 0, "restore everything ({})", String::from_utf8_lossy(&o.error));
    assert!(!exists(&home.join(".local/share/applications/evil.desktop")), "does not write .desktop");
    assert!(fs::read_to_string(home.join(".config/axia/settings.json")).unwrap().contains("pt"), "does not overwrite settings.json");
    assert!(o.skipped >= 2, "protected paths skipped ({})", o.skipped);

    // links: the link is restored, not its target; never descend through a link in the backup
    fs::remove_file(home.join("escape")).unwrap();
    let (r, _) = w.restore(&id1, "escape", &home, false, suffix, None);
    assert!(r == 0 && fs::symlink_metadata(home.join("escape")).unwrap().file_type().is_symlink(), "symlink restored as a symlink");
    symlink("/etc", w.snap(&id1, "outside")).unwrap();
    let (r, _) = w.restore(&id1, "outside/passwd", &home, false, suffix, None);
    assert!(r != 0 && !exists(&home.join("outside")), "no escape through a link in the backup");
    assert_ne!(w.restore(&id1, "../../../etc/passwd", &home, false, suffix, None).0, 0, ".. rejected");
    assert_ne!(w.restore(&id1, "a.txt", Path::new("/etc"), false, suffix, None).0, 0, "destination outside the roots rejected");
    assert_ne!(w.restore(&id1, "a.txt", &home, false, b"/x", None).0, 0, "suffix with a slash rejected");
    assert_ne!(w.restore(&id1, "a.txt", &home, false, b"", None).0, 0, "empty suffix rejected");
    assert_ne!(w.restore("2020-01-01-000000", "a.txt", &home, false, suffix, None).0, 0, "no such snapshot");
    // a link in the home folder pointing to a protected folder
    let apps = home.join(".local/share/applications");
    fs::create_dir_all(&apps).unwrap();
    symlink(&apps, home.join("shortcut")).unwrap();
    let _ = fs::remove_file(w.snap(&id1, "shortcut"));
    fs::create_dir(w.snap(&id1, "shortcut")).unwrap();
    fs::write(w.snap(&id1, "shortcut/x.desktop"), "x").unwrap();
    let (r, _) = w.restore(&id1, "shortcut/x.desktop", &home, false, suffix, None);
    assert!(!exists(&apps.join("x.desktop")), "does not write into a protected folder through a link ({r})");

    // pruning: out of space, only the newest stays
    let before = engine::list(&machine, 16);
    engine::prune(&machine, 1_792_000_000, u64::MAX);
    let after = engine::list(&machine, 16);
    assert!(before.len() == 3 && after == [id3.clone()], "pruning for space: the newest stays ({} → {})", before.len(), after.len());
    assert!(!engine::delete(&machine, "../../x"), "delete with an invalid name");
    cleanup(&t);
}

#[test]
fn an_encrypted_snapshot_has_nothing_in_clear_links_and_restores() {
    let _turn = POLICY.lock().unwrap_or_else(|e| e.into_inner());
    let t = temp("encsnap");
    let home = t.join("home");
    fs::create_dir(&home).unwrap();
    let home = home.canonicalize().unwrap();
    axia_fs::policy::init(&home, "test-user");
    let machine = t.join("encrypted").join(DIRNAME).join("host-user");
    fs::create_dir_all(&machine).unwrap();
    let key = crypt::setup(&machine, b"a long test passphrase").unwrap();
    let w = World { home: home.clone(), machine: machine.clone(), skip: t.join("encrypted") };
    put(&home, "secret.txt", "very secret text 1234\n");
    fs::write(home.join("empty.txt"), "").unwrap();
    fs::write(home.join("big.bin"), vec![7u8; 3 * CHUNK + 5]).unwrap();
    let now = 1_792_000_000;
    let (o, _) = w.snapshot(now - 600, 0, None, Some(&key), &["encrypted"]);
    assert_eq!(o.code, 0, "encrypted snapshot ({})", String::from_utf8_lossy(&o.error));
    let id1 = o.id.clone();
    let raw = fs::read(w.snap(&id1, "secret.txt")).unwrap();
    assert!(&raw[..4] == b"LBK1" && !String::from_utf8_lossy(&raw).contains("secret text 1234"), "the backup file is encrypted");
    let (o, s) = w.snapshot(now - 300, 0, None, Some(&key), &["encrypted"]);
    assert!(o.code == 0 && Progress::get(&s.copied) == 0 && Progress::get(&s.linked) > 2, "encrypted: unchanged files are linked");
    assert_eq!(ino(&w.snap(&id1, "secret.txt")), ino(&w.snap(&o.id, "secret.txt")), "encrypted: same inode");
    fs::remove_file(home.join("secret.txt")).unwrap();
    let (r, o) = w.restore(&id1, "secret.txt", &home, false, b" (restored)", Some(&key));
    assert!(r == 0 && o.files == 1, "restore decrypts");
    assert_eq!(fs::read_to_string(home.join("secret.txt")).unwrap(), "very secret text 1234\n");
    let (r, o) = w.restore(&id1, "secret.txt", &home, false, b" (restored)", Some(&key));
    assert!(r == 0 && o.kept_both == 0 && o.skipped == 1, "encrypted: identical to the current file, not duplicated");
    let (r, _) = w.restore(&id1, "big.bin", &home, true, b" (restored)", Some(&key));
    assert_eq!(r, 0);
    assert_eq!(fs::read(home.join("big.bin")).unwrap(), vec![7u8; 3 * CHUNK + 5]);
    // tampered backup: nothing is written
    let mut data = fs::read(w.snap(&id1, "secret.txt")).unwrap();
    data[HDR + 2] ^= 0x40;
    let p = w.snap(&id1, "secret.txt");
    // it is a hard link shared with the second snapshot: write through the same inode
    fs::OpenOptions::new().write(true).open(&p).unwrap().write_all(&data).unwrap();
    fs::remove_file(home.join("secret.txt")).unwrap();
    let (_, o) = w.restore(&id1, "secret.txt", &home, false, b" (restored)", Some(&key));
    assert!(o.errors == 1 && !exists(&home.join("secret.txt")), "tampered backup: error and nothing written ({})", String::from_utf8_lossy(&o.error));
    assert_eq!(o.error, b"backup.verifyFailed");
    // a cancelled restore stops
    let cancel = AtomicBool::new(true);
    let mut out = RestoreStats::default();
    assert_eq!(engine::restore(&machine, &id1, "", &home, true, b" (restored)", Some(&cancel), Some(&key), &mut out), 1);
    cleanup(&t);
}

#[test]
fn a_missing_source_or_an_unwritable_destination_is_an_error_code() {
    let t = temp("errors");
    let w = World { home: t.join("nope"), machine: t.join("m"), skip: t.clone() };
    let (o, _) = w.snapshot(1_792_000_000, 0, None, None, &[]);
    assert_eq!((o.code, o.error.as_slice()), (-1, &b"backup.sourceUnreadable"[..]));
    fs::create_dir(t.join("home")).unwrap();
    fs::write(t.join("file"), "x").unwrap();
    let w = World { home: t.join("home"), machine: t.join("file/under"), skip: t.clone() };
    let (o, _) = w.snapshot(1_792_000_000, 0, None, None, &[]);
    assert_eq!(o.code, -1);
    assert!(o.error.starts_with(b"backup.createFailed path="), "{}", String::from_utf8_lossy(&o.error));
    cleanup(&t);
}
