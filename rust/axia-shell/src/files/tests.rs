//! Regression tests use private temporary homes; none access the running session.
use super::{model::*, operations as ops, policy, trash};
use serde_json::{json, Value};
use std::os::unix::{
    ffi::OsStringExt,
    fs::{symlink, PermissionsExt},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
static SERIAL: Mutex<()> = Mutex::new(());
pub(super) struct Home {
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}
impl Home {
    pub(super) fn new() -> Self {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "axia-files-tests-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        policy::init(&root, "axia-tests");
        Self {
            root,
            _guard: guard,
        }
    }
    pub(super) fn path(&self, p: &str) -> PathBuf {
        self.root.join(p)
    }
    fn dir(&self, p: &str) -> PathBuf {
        let path = self.path(p);
        fs::create_dir_all(&path).unwrap();
        path
    }
    pub(super) fn file(&self, p: &str, content: &str) -> PathBuf {
        let path = self.path(p);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }
    pub(super) fn session(&self, args: Value) -> (Session, Shared) {
        let mut shared = Shared::default();
        let mut session = Session::new(1, &args, &mut shared).unwrap();
        ready(&mut session, &shared);
        (session, shared)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        for job in ops::jobs() {
            ops::cancel(job.id);
        }
        let until = Instant::now() + Duration::from_secs(3);
        while ops::jobs().iter().any(|j| !j.done) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        for job in ops::jobs() {
            ops::forget(job.id);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn ready(session: &mut Session, shared: &Shared) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        session.poll(&json!({}), shared);
        if !session.loading && !session.busy {
            return;
        }
        assert!(
            Instant::now() < until,
            "async operation timed out: {:?}",
            session.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn event(s: &mut Session, shared: &mut Shared, command: &str, arg: Value) {
    s.event(&json!({"action":[command,arg]}), shared);
}
fn select(s: &mut Session, shared: &mut Shared, name: &str) {
    let index = s.tab().entries.iter().position(|e| e.name == name).unwrap();
    event(s, shared, "select", json!(index));
}
fn finish(id: i32) -> ops::Job {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let job = ops::jobs().into_iter().find(|j| j.id == id).unwrap();
        if job.done {
            ops::forget(id);
            return job;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn xdg_directories_preserve_fedora_names_and_explicit_disabling() {
    let h = Home::new();
    h.dir("Documents");
    h.dir("Downloads");
    h.dir("Área de trabalho");
    h.dir("My \"Docs\"");
    let p = policy::current();
    let defaults = user_dirs(&p, "");
    assert!(defaults.iter().any(|p| p.label == "downloads"));
    assert!(defaults.iter().any(|p| p.label == "desktop"));
    let dirs = user_dirs(
        &p,
        "XDG_DOWNLOAD_DIR=\"$HOME\"\n XDG_DOCUMENTS_DIR = \"$HOME/My \\\"Docs\\\"\" # custom",
    );
    assert!(!dirs.iter().any(|p| p.label == "downloads"));
    assert!(dirs
        .iter()
        .any(|p| p.label == "documents" && p.path == h.path("My \"Docs\"")));
    assert!(!user_dirs(&p, "XDG_DOCUMENTS_DIR=\"Documents\"")
        .iter()
        .any(|p| p.label == "documents"));
    assert!(!user_dirs(&p, "XDG_DOCUMENTS_DIR=\"$HOME/missing\"")
        .iter()
        .any(|p| p.label == "documents"));
}
#[test]
fn xdg_never_executes_shell_text() {
    let h = Home::new();
    h.dir("$(not-executed)");
    let dirs = user_dirs(
        &policy::current(),
        "XDG_DOCUMENTS_DIR=\"${HOME}/$(not-executed)\"",
    );
    assert!(dirs.iter().any(|p| p.path == h.path("$(not-executed)")));
}
#[test]
fn browsing_orders_naturally_and_hides_dotfiles() {
    let h = Home::new();
    h.file("file10.txt", "ten");
    h.file("file2.txt", "two");
    h.file(".secret", "secret");
    h.dir("Folder");
    let (s, _) = h.session(json!({}));
    let names: Vec<_> = s
        .filtered()
        .iter()
        .map(|i| s.tab().entries[*i].name.as_str())
        .collect();
    assert_eq!(names, vec!["Folder", "file2.txt", "file10.txt"]);
}
#[test]
fn browser_and_picker_have_independent_history_and_selection() {
    let h = Home::new();
    let folder = h.dir("Downloads");
    h.file("Downloads/note.txt", "note");
    let (mut a, mut shared) = h.session(json!({}));
    let mut b = Session::new(2, &json!({"mode":"open"}), &mut shared).unwrap();
    ready(&mut b, &shared);
    event(&mut a, &mut shared, "navigate", json!(folder));
    ready(&mut a, &shared);
    assert_eq!(a.tab().entries[0].name, "note.txt");
    assert_eq!(b.tab().location, Location::Directory(h.root.clone()));
    assert!(b.tab().selected.is_empty());
    event(&mut a, &mut shared, "back", Value::Null);
    ready(&mut a, &shared);
    assert_eq!(a.tab().location, Location::Directory(h.root.clone()));
}
#[test]
fn tabs_keep_their_own_locations_and_restore_view_preferences() {
    let h = Home::new();
    let folder = h.dir("Docs");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "view", json!("list"));
    event(&mut s, &mut shared, "newTab", Value::Null);
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "navigate", json!(folder));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "view", json!("gallery"));
    event(&mut s, &mut shared, "tab", json!(0));
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
    assert_eq!(s.tab().view, "list");
    event(&mut s, &mut shared, "navigate", json!(folder));
    ready(&mut s, &shared);
    assert_eq!(s.tab().view, "gallery");
    assert!(h.path(".config/axia/files.json").is_file());
}
#[test]
fn opening_picker_selects_only_matching_files() {
    let h = Home::new();
    h.file("picture.png", "image");
    h.file("note.txt", "note");
    h.dir("Folder");
    let (mut s, mut shared) =
        h.session(json!({"mode":"open","filter":"\\.png$","ignoreCase":true}));
    let names: Vec<_> = s
        .filtered()
        .iter()
        .map(|i| s.tab().entries[*i].name.clone())
        .collect();
    assert_eq!(names, vec!["Folder", "picture.png"]);
    select(&mut s, &mut shared, "picture.png");
    event(&mut s, &mut shared, "choose", Value::Null);
    assert_eq!(
        s.effects.last().unwrap()["paths"],
        json!([h.path("picture.png")])
    );
    assert!(s.closed);
}
#[test]
fn single_picker_cannot_return_multiple_files() {
    let h = Home::new();
    h.file("a.txt", "a");
    h.file("b.txt", "b");
    let (mut s, mut shared) = h.session(json!({"mode":"open"}));
    event(&mut s, &mut shared, "selectAll", Value::Null);
    assert_eq!(s.selected().len(), 1);
    event(&mut s, &mut shared, "choose", Value::Null);
    assert_eq!(
        s.effects.last().unwrap()["paths"].as_array().unwrap().len(),
        1
    );
}
#[test]
fn multiple_picker_keeps_multiple_selection() {
    let h = Home::new();
    h.file("a.txt", "a");
    h.file("b.txt", "b");
    let (mut s, mut shared) = h.session(json!({"mode":"open","multiple":true}));
    event(&mut s, &mut shared, "selectAll", Value::Null);
    assert_eq!(s.selected().len(), 2);
    event(&mut s, &mut shared, "choose", Value::Null);
    assert_eq!(
        s.effects.last().unwrap()["paths"].as_array().unwrap().len(),
        2
    );
}
#[test]
fn folder_picker_returns_current_folder_and_cancel_does_not_choose() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({"mode":"folder"}));
    event(&mut s, &mut shared, "choose", Value::Null);
    assert_eq!(s.effects[0]["paths"], json!([h.root]));
    let (mut s, mut shared) = h.session(json!({"mode":"save","name":"new.txt"}));
    event(&mut s, &mut shared, "cancel", Value::Null);
    assert!(s.effects.iter().all(|e| e["kind"] != "choose"));
    assert!(!h.path("new.txt").exists());
}
#[test]
fn saving_requires_confirmation_before_replacing_existing_file() {
    let h = Home::new();
    h.file("existing.txt", "original");
    let (mut s, mut shared) = h.session(json!({"mode":"save","name":"existing.txt"}));
    event(&mut s, &mut shared, "choose", Value::Null);
    assert!(matches!(s.dialog, Some(Dialog::Confirm { .. })));
    assert!(!s.closed);
    assert!(s.effects.is_empty());
    event(&mut s, &mut shared, "cancelDialog", Value::Null);
    assert_eq!(
        fs::read_to_string(h.path("existing.txt")).unwrap(),
        "original"
    );
    event(&mut s, &mut shared, "choose", Value::Null);
    event(&mut s, &mut shared, "confirm", Value::Null);
    assert!(s.closed);
    assert_eq!(
        s.effects.last().unwrap()["paths"],
        json!([h.path("existing.txt")])
    );
}
#[test]
fn picker_refuses_escape_and_protected_save_targets() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({"mode":"save","name":"../outside"}));
    event(&mut s, &mut shared, "choose", Value::Null);
    assert!(s.error.is_some());
    event(&mut s, &mut shared, "navigate", json!("/etc"));
    assert!(s.error.is_some());
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
    event(&mut s, &mut shared, "trash", Value::Null);
    assert!(s.error.is_some());
}
#[test]
fn changing_extensions_has_confirmation_and_preserves_contents() {
    let h = Home::new();
    h.file("note.txt", "original");
    let (mut s, mut shared) = h.session(json!({}));
    select(&mut s, &mut shared, "note.txt");
    event(&mut s, &mut shared, "rename", Value::Null);
    s.event(
        &json!({"action":["dialogText"],"value":"note.md"}),
        &mut shared,
    );
    event(&mut s, &mut shared, "submit", Value::Null);
    assert!(matches!(s.dialog, Some(Dialog::Confirm { .. })));
    assert!(h.path("note.txt").is_file());
    event(&mut s, &mut shared, "confirm", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(fs::read_to_string(h.path("note.md")).unwrap(), "original");
}
#[test]
fn folder_watch_updates_without_manual_refresh() {
    let h = Home::new();
    let (mut s, shared) = h.session(json!({}));
    h.file("created-after-open.txt", "new");
    let until = Instant::now() + Duration::from_secs(3);
    while !s
        .tab()
        .entries
        .iter()
        .any(|e| e.name == "created-after-open.txt")
    {
        s.poll(&json!({}), &shared);
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(15));
    }
}
#[test]
fn recursive_search_and_scope_return_nested_matches() {
    let h = Home::new();
    h.file("Documents/deep/needle.txt", "found");
    let (mut s, mut shared) = h.session(json!({}));
    s.event(&json!({"action":["query"],"value":"needle"}), &mut shared);
    event(&mut s, &mut shared, "search", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(s.tab().entries.len(), 1);
    assert!(s.tab().entries[0].raw.ends_with("deep/needle.txt"));
}
#[test]
fn copying_publishes_complete_files_without_overwriting() {
    let h = Home::new();
    let from = h.file("source/a.txt", "payload");
    let target = h.path("copy.txt");
    let job = finish(ops::start(&from, &target, false).unwrap());
    assert!(!job.failed, "{}", job.error);
    assert_eq!(fs::read_to_string(&target).unwrap(), "payload");
    assert!(ops::start(&from, &target, false).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "payload");
}
#[test]
fn copy_preserves_symlinks_and_never_traverses_them() {
    let h = Home::new();
    let source = h.dir("source");
    symlink("/etc/passwd", source.join("link")).unwrap();
    let dst = h.path("copy");
    assert!(!finish(ops::start(&source, &dst, false).unwrap()).failed);
    assert_eq!(
        fs::read_link(dst.join("link")).unwrap(),
        Path::new("/etc/passwd")
    );
    assert!(fs::symlink_metadata(dst.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
}
#[test]
fn copying_directory_permissions_and_hidden_files_are_preserved() {
    let h = Home::new();
    let source = h.dir("source");
    h.file("source/.hidden", "secret");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
    let dst = h.path("copy");
    assert!(!finish(ops::start(&source, &dst, false).unwrap()).failed);
    assert_eq!(fs::read(dst.join(".hidden")).unwrap(), b"secret");
    assert_eq!(
        fs::metadata(dst).unwrap().permissions().mode() & 0o777,
        0o750
    );
}
#[test]
fn copy_to_descendant_is_refused() {
    let h = Home::new();
    let source = h.dir("source");
    assert!(ops::start(&source, &source.join("nested"), false).is_err());
}
#[test]
fn cancellation_never_removes_existing_destination() {
    let h = Home::new();
    let src = h.file("source", "payload");
    h.file("existing", "keep");
    assert!(ops::start(&src, &h.path("existing"), false).is_err());
    assert_eq!(fs::read_to_string(h.path("existing")).unwrap(), "keep");
}
#[test]
fn non_utf8_filenames_are_copied_without_loss() {
    let h = Home::new();
    let source = h
        .root
        .join(std::ffi::OsString::from_vec(b"name-\xff".to_vec()));
    fs::write(&source, b"bytes").unwrap();
    let target = h
        .root
        .join(std::ffi::OsString::from_vec(b"copy-\xfe".to_vec()));
    assert!(!finish(ops::start(&source, &target, false).unwrap()).failed);
    assert_eq!(fs::read(target).unwrap(), b"bytes");
}
#[test]
fn trash_restore_never_overwrites_a_new_file() {
    let h = Home::new();
    let file = h.file("note.txt", "old");
    let id = trash::put(&file).unwrap();
    assert!(!file.exists());
    h.file("note.txt", "new");
    assert!(trash::restore(Path::new(&id)).is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "new");
    fs::remove_file(&file).unwrap();
    trash::restore(Path::new(&id)).unwrap();
    assert_eq!(fs::read_to_string(file).unwrap(), "old");
}
#[test]
fn trash_removal_never_follows_symlinks() {
    let h = Home::new();
    let keep = h.file("kept/file.txt", "keep");
    let link = h.path("link");
    symlink(keep.parent().unwrap(), &link).unwrap();
    let id = trash::put(&link).unwrap();
    trash::remove(Path::new(&id)).unwrap();
    assert_eq!(fs::read_to_string(keep).unwrap(), "keep");
    assert!(trash::remove(&h.root).is_err());
}
#[test]
fn protected_ancestors_and_symlink_parents_cannot_be_moved() {
    let h = Home::new();
    h.file(".config/axia/settings.json", "{}");
    h.file("other.txt", "other");
    assert!(ops::rename(&h.path(".config"), &h.path("moved")).is_err());
    assert!(ops::rename(
        &h.path("other.txt"),
        &h.path(".config/autostart/run.desktop")
    )
    .unwrap_err()
    .starts_with("fs.protected"));
    symlink("/etc", h.path("outside")).unwrap();
    assert!(ops::mkdir(&h.path("outside/not-created")).is_err());
}
#[test]
fn virtualized_view_keeps_widget_count_bounded_for_large_folders() {
    let h = Home::new();
    for n in 0..1500 {
        h.file(&format!("item-{n:04}.txt"), "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    s.poll(&json!({"width":960,"height":600}), &shared);
    let tree = s.render(&shared);
    fn count(node: &Value) -> usize {
        1 + node["children"]
            .as_array()
            .map(|a| a.iter().map(count).sum::<usize>())
            .unwrap_or(0)
    }
    assert!(count(&tree) < 1500, "all files were materialized");
    event(&mut s, &mut shared, "scroll", json!(100000));
    assert!(count(&s.render(&shared)) < 1500);
}

#[test]
fn a_large_selection_is_one_copy_job() {
    let h = Home::new();
    h.dir("destination");
    let mut items = Vec::new();
    for n in 0..24 {
        let path = h.file(&format!("source/file-{n}.txt"), "data");
        items.push((path, h.path(&format!("destination/file-{n}.txt")), false));
    }
    let job = finish(ops::start_batch(items).unwrap());
    assert!(!job.failed, "{}", job.error);
    for n in 0..24 {
        assert_eq!(
            fs::read_to_string(h.path(&format!("destination/file-{n}.txt"))).unwrap(),
            "data"
        );
    }
}
#[test]
fn cancelling_a_copy_leaves_no_published_or_staging_file() {
    let h = Home::new();
    let source = h.file("source.bin", "");
    fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_len(128 * 1024 * 1024)
        .unwrap();
    let id = ops::start(&source, &h.path("copy.bin"), false).unwrap();
    ops::cancel(id);
    let job = finish(id);
    assert!(job.cancel);
    assert!(!h.path("copy.bin").exists());
    assert_eq!(fs::metadata(&source).unwrap().len(), 128 * 1024 * 1024);
    assert!(!fs::read_dir(&h.root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".axia-copy-")));
}
#[test]
fn paste_in_the_same_folder_creates_a_duplicate() {
    let h = Home::new();
    h.file("note.txt", "keep");
    let (mut s, mut shared) = h.session(json!({}));
    select(&mut s, &mut shared, "note.txt");
    event(&mut s, &mut shared, "copy", Value::Null);
    event(&mut s, &mut shared, "paste", Value::Null);
    assert!(s.error.is_none(), "{:?}", s.error);
    let until = Instant::now() + Duration::from_secs(3);
    while !h.path("note 2.txt").is_file() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fs::read_to_string(h.path("note 2.txt")).unwrap(), "keep");
}
#[test]
fn sorting_keeps_the_selected_file() {
    let h = Home::new();
    h.file("a.txt", "longer");
    h.file("b.txt", "x");
    let (mut s, mut shared) = h.session(json!({}));
    select(&mut s, &mut shared, "a.txt");
    event(&mut s, &mut shared, "sort", json!("size"));
    assert_eq!(s.selected()[0].name, "a.txt");
}
#[test]
fn folder_picker_hides_files() {
    let h = Home::new();
    h.dir("Folder");
    h.file("note.txt", "x");
    let (s, _) = h.session(json!({"mode":"folder"}));
    assert!(s.filtered().iter().all(|i| s.tab().entries[*i].dir));
}
#[test]
fn keyboard_navigation_reveals_offscreen_selection() {
    let h = Home::new();
    for n in 0..100 {
        h.file(&format!("file-{n}.txt"), "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "view", json!("list"));
    s.event(&json!({"action":["key"],"key":"End"}), &mut shared);
    assert_eq!(s.selected()[0].name, "file-99.txt");
    assert!(s.tab().offset > 0.);
    s.event(&json!({"action":["key"],"key":"Home"}), &mut shared);
    assert_eq!(s.tab().offset, 0.);
}
#[test]
fn miller_columns_keep_ancestors_and_drop_them_on_back() {
    let h = Home::new();
    let a = h.dir("one");
    let b = h.dir("one/two");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "view", json!("columns"));
    event(&mut s, &mut shared, "navigate", json!(a));
    ready(&mut s, &shared);
    assert_eq!(s.tab().view, "columns");
    event(&mut s, &mut shared, "navigate", json!(b));
    ready(&mut s, &shared);
    assert_eq!(s.tab().columns.len(), 2);
    event(&mut s, &mut shared, "navigate", json!(h.root));
    ready(&mut s, &shared);
    assert!(s.tab().columns.is_empty());
}
#[test]
fn trash_empty_removes_orphans_without_metadata() {
    let h = Home::new();
    h.file(".local/share/Trash/files/orphan.bin", "old");
    assert_eq!(trash::empty().unwrap(), 1);
    assert!(!h.path(".local/share/Trash/files/orphan.bin").exists());
}
#[test]
fn native_chooser_never_exposes_the_system_directory() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({"mode":"open"}));
    event(&mut s, &mut shared, "place", json!("computer"));
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
    assert_eq!(s.error.as_deref(), Some("fs.outsideHome"));
}
#[test]
fn malformed_preferences_do_not_break_the_browser() {
    let h = Home::new();
    h.file(".config/axia/files.json", "{broken");
    h.file("note.txt", "x");
    let (s, _) = h.session(json!({"preferences":{"defaultView":{"view":"not-a-view"}}}));
    assert_eq!(s.tab().view, "icons");
    assert_eq!(s.filtered().len(), 1);
}

#[test]
fn left_button_drag_moves_the_file_to_the_target_folder() {
    let h = Home::new();
    h.file("note.txt", "data");
    h.dir("Target");
    let (mut s, mut shared) = h.session(json!({}));
    let source = s
        .tab()
        .entries
        .iter()
        .position(|e| e.name == "note.txt")
        .unwrap();
    let target = s
        .tab()
        .entries
        .iter()
        .position(|e| e.name == "Target")
        .unwrap();
    s.event(
        &json!({"action":["dragStart",source],"button":0,"x":10,"y":10}),
        &mut shared,
    );
    assert!(shared.dragging.is_some());
    s.event(
        &json!({"action":["pointerMove"],"x":80,"y":80}),
        &mut shared,
    );
    s.event(&json!({"action":["drop",target]}), &mut shared);
    assert!(s.error.is_none(), "{:?}", s.error);
    let job = ops::jobs().into_iter().next().unwrap();
    assert!(!finish(job.id).failed);
    assert!(!h.path("note.txt").exists());
    assert_eq!(
        fs::read_to_string(h.path("Target/note.txt")).unwrap(),
        "data"
    );
}
#[test]
fn middle_button_does_not_begin_a_file_drag() {
    let h = Home::new();
    h.file("note.txt", "data");
    let (mut s, mut shared) = h.session(json!({}));
    s.event(
        &json!({"action":["dragStart",0],"button":1,"x":10,"y":10}),
        &mut shared,
    );
    assert!(shared.dragging.is_none());
    assert!(s.drag.is_none());
}

#[test]
fn chooser_widgets_do_not_bind_browser_drag_actions() {
    let h = Home::new();
    h.file("note.txt", "data");
    let (s, shared) = h.session(json!({"mode":"open"}));
    fn check(node: &Value) {
        for event in node["events"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
        {
            assert!(
                event[0] != "dragStart" && event[0] != "drop",
                "browser-only action bound in chooser"
            );
        }
        for child in node["children"].as_array().into_iter().flatten() {
            check(child);
        }
    }
    check(&s.render(&shared));
}

#[test]
fn closing_a_tab_to_the_left_preserves_the_active_folder() {
    let h = Home::new();
    let one = h.dir("one");
    let two = h.dir("two");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "newTab", Value::Null);
    event(&mut s, &mut shared, "navigate", json!(one));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "newTab", Value::Null);
    event(&mut s, &mut shared, "navigate", json!(two));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "tab", json!(1));
    event(&mut s, &mut shared, "closeTab", json!(0));
    ready(&mut s, &shared);
    assert_eq!(s.active, 0);
    assert_eq!(s.tab().location, Location::Directory(one));
}

#[test]
fn parent_column_chooser_returns_the_clicked_file_not_the_current_selection() {
    let h = Home::new();
    let target = h.file("parent.txt", "parent");
    let folder = h.dir("child");
    h.file("child/other.txt", "other");
    let (mut s, mut shared) = h.session(json!({"mode":"open","start":folder,"filter":"\\.txt$"}));
    select(&mut s, &mut shared, "other.txt");
    let count = s.tab().entries.len();
    event(&mut s, &mut shared, "openRaw", json!(target));
    let choice = s.effects.iter().find(|e| e["kind"] == "choose").unwrap();
    assert_eq!(choice["paths"], json!([target]));
    assert_eq!(s.tab().entries.len(), count);
}

#[test]
fn scroll_keyboard_and_render_share_geometry_after_resizing() {
    let h = Home::new();
    for n in 0..200 {
        h.file(&format!("item-{n:03}.txt"), "");
    }
    for mode in ["browser", "open"] {
        let (mut s, mut shared) = h.session(json!({"mode":mode}));
        for view in ["icons", "list", "gallery"] {
            event(&mut s, &mut shared, "view", json!(view));
            for (w, h) in [(640, 360), (960, 600), (1366, 768)] {
                s.poll(&json!({"width":w,"height":h}), &shared);
                s.error = Some("fs.copyStartFailed".into());
                s.job_errors = vec!["fs.copyStartFailed".into()];
                s.event(&json!({"action":["key"],"key":"End"}), &mut shared);
                let index = *s.tab().selected.first().unwrap();
                assert_eq!(s.tab().entries[index].name, "item-199.txt");
                let cols = if view == "icons" {
                    (s.entry_width() / 112.).floor().max(1.) as usize
                } else {
                    1
                };
                let row = if view == "icons" { 110. } else { 32. };
                let bottom = (199 / cols + 1) as f64 * row;
                assert!((bottom - s.tab().offset - s.entry_area()).abs() < 0.01);
                event(&mut s, &mut shared, "scroll", json!(1e9));
                assert!((bottom - s.tab().offset - s.entry_area()).abs() < 0.01);
                s.poll(&json!({"height":h+100}), &shared);
                assert!(s.tab().offset <= (bottom - s.entry_area()).max(0.) + 0.01);
            }
        }
    }
}

#[test]
fn ancestor_columns_reach_the_last_file_without_materializing_the_folder() {
    let h = Home::new();
    for n in 0..350 {
        h.file(&format!("parent-{n:03}.txt"), "");
    }
    let child = h.dir("child");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "view", json!("columns"));
    event(&mut s, &mut shared, "navigate", json!(child));
    ready(&mut s, &shared);
    let before = s.render(&shared).to_string();
    assert!(!before.contains("parent-349.txt"));
    s.event(
        &json!({"action":["columnWheel",h.root],"deltaY":1e6}),
        &mut shared,
    );
    let after = s.render(&shared).to_string();
    assert!(after.contains("parent-349.txt"));
    assert!(after.len() < 50000, "ancestor folder lost virtualization");
    assert!(!after.contains("parent-000.txt"));
}

#[test]
fn many_tabs_keep_the_active_tab_visible_and_titles_match_the_location() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({}));
    for _ in 1..32 {
        event(&mut s, &mut shared, "newTab", Value::Null);
    }
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "place", json!("trash"));
    ready(&mut s, &shared);
    s.poll(&json!({"width":640,"height":360}), &shared);
    let tree = s.render(&shared).to_string();
    assert!(tree.contains("files.place.trash"));
    assert!(tree.contains("[\"tab\",31]"));
    assert!(!tree.contains("[\"tab\",0]"));
    shared.preferences["foldersInTabs"] = json!(true);
    let folder = h.dir("next");
    event(&mut s, &mut shared, "place", json!("home"));
    ready(&mut s, &shared);
    let index = s
        .tab()
        .entries
        .iter()
        .position(|e| e.raw == folder)
        .unwrap();
    event(&mut s, &mut shared, "activate", json!(index));
    ready(&mut s, &shared);
    assert_eq!(s.tabs.len(), 32);
    assert_eq!(s.tab().location, Location::Directory(folder));
}

#[test]
fn small_window_previews_fit_inside_the_dialog() {
    let h = Home::new();
    h.file("note.txt", "preview");
    let (mut s, mut shared) = h.session(json!({}));
    s.poll(&json!({"width":640,"height":360}), &shared);
    select(&mut s, &mut shared, "note.txt");
    event(&mut s, &mut shared, "preview", Value::Null);
    if let Some(Dialog::Preview { text, .. }) = &mut s.dialog {
        *text = Some("example".into());
    }
    fn check(node: &Value) {
        if let Some(w) = node["style"]["width"].as_f64() {
            assert!(w <= 640.);
        }
        if node["type"] == "scroll" && node["style"]["width"].is_number() {
            assert!(node["style"]["width"].as_f64().unwrap() <= 540.);
        }
        for child in node["children"].as_array().into_iter().flatten() {
            check(child);
        }
    }
    check(&s.render(&shared));
}

#[test]
fn dragging_the_scrollbar_clamps_and_preserves_selection() {
    let h = Home::new();
    for n in 0..100 {
        h.file(&format!("file-{n:03}.txt"), "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "view", json!("list"));
    select(&mut s, &mut shared, "file-000.txt");
    s.event(
        &json!({"action":["scrollDrag"],"y":100,"button":0}),
        &mut shared,
    );
    s.event(&json!({"action":["pointerMove"],"y":1e6}), &mut shared);
    assert!((s.tab().offset - (3200. - s.entry_area())).abs() < 0.01);
    assert_eq!(s.selected()[0].name, "file-000.txt");
    s.event(&json!({"action":["pointerMove"],"y":-1e6}), &mut shared);
    assert_eq!(s.tab().offset, 0.);
    event(&mut s, &mut shared, "dragEnd", Value::Null);
    assert!(s.drag.is_none());
    assert!(shared.dragging.is_none());
}

#[test]
fn keyboard_cycles_tabs_in_both_directions() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "newTab", Value::Null);
    event(&mut s, &mut shared, "newTab", Value::Null);
    s.event(
        &json!({"action":["key"],"key":"Tab","ctrl":true}),
        &mut shared,
    );
    assert_eq!(s.active, 0);
    s.event(
        &json!({"action":["key"],"key":"Tab","ctrl":true,"shift":true}),
        &mut shared,
    );
    assert_eq!(s.active, 2);
}

#[test]
fn background_menu_never_selects_the_first_file() {
    let h = Home::new();
    h.file("first.txt", "keep");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "context", Value::Null);
    assert!(s.selected().is_empty());
    assert!(s.context.is_some());
    // Also tolerate old UI messages whose out-of-range Number was rounded.
    event(
        &mut s,
        &mut shared,
        "context",
        json!(18446744073709552000_f64),
    );
    assert!(s.selected().is_empty());
    select(&mut s, &mut shared, "first.txt");
    event(&mut s, &mut shared, "context", Value::Null);
    assert_eq!(s.selected()[0].name, "first.txt");
}

#[test]
fn background_drop_targets_the_current_folder_not_the_first_subfolder() {
    let h = Home::new();
    h.dir("aaa");
    let from = h.file("origin/note.txt", "move me");
    let (mut s, mut shared) = h.session(json!({}));
    shared.dragging = Some((s.id, vec![from.clone()], true));
    event(&mut s, &mut shared, "drop", Value::Null);
    let until = Instant::now() + Duration::from_secs(3);
    while !h.path("note.txt").is_file() {
        assert!(
            Instant::now() < until,
            "background drop went to another folder"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!h.path("aaa/note.txt").exists());
    assert!(!from.exists());
    assert_eq!(fs::read_to_string(h.path("note.txt")).unwrap(), "move me");
}

#[test]
fn close_tab_shortcut_closes_the_active_tab() {
    let h = Home::new();
    let one = h.dir("one");
    let two = h.dir("two");
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "newTab", Value::Null);
    event(&mut s, &mut shared, "navigate", json!(one));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "newTab", Value::Null);
    event(&mut s, &mut shared, "navigate", json!(two));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "tab", json!(1));
    s.event(
        &json!({"action":["key"],"ctrl":true,"key":"w"}),
        &mut shared,
    );
    ready(&mut s, &shared);
    assert_eq!(s.tabs.len(), 2);
    assert_eq!(s.tabs[0].location, Location::Directory(h.root.clone()));
    assert_eq!(s.tabs[1].location, Location::Directory(two));
}
