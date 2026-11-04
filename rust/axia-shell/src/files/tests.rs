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
                let (cols, _, row) = s.entry_grid();
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

fn enter_text(s: &mut Session, shared: &mut Shared, text: &str) {
    s.event(&json!({"action":["dialogText"],"value":text}), shared);
    event(s, shared, "submit", Value::Null);
    ready(s, shared);
}
#[test]
fn create_rename_and_collisions_preserve_existing_files() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({}));
    event(&mut s, &mut shared, "newFolder", Value::Null);
    enter_text(&mut s, &mut shared, "Pasta com espaços");
    assert!(h.path("Pasta com espaços").is_dir());
    select(&mut s, &mut shared, "Pasta com espaços");
    event(&mut s, &mut shared, "rename", Value::Null);
    enter_text(&mut s, &mut shared, "Renomeada");
    assert!(h.path("Renomeada").is_dir());
    assert!(!h.path("Pasta com espaços").exists());
    h.file("existente.txt", "original");
    h.file("outro.txt", "outro");
    event(&mut s, &mut shared, "refresh", Value::Null);
    ready(&mut s, &shared);
    select(&mut s, &mut shared, "outro.txt");
    event(&mut s, &mut shared, "rename", Value::Null);
    enter_text(&mut s, &mut shared, "existente.txt");
    assert!(s.error.is_some());
    let tree = s.render(&shared);
    let modal = tree["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["style"]["zIndex"] == 200)
        .expect("rename dialog stays open after a collision");
    assert!(
        modal.to_string().contains(s.error.as_ref().unwrap()),
        "rename failure must be shown inside the dialog"
    );
    assert_eq!(
        fs::read_to_string(h.path("existente.txt")).unwrap(),
        "original"
    );
    assert_eq!(fs::read_to_string(h.path("outro.txt")).unwrap(), "outro");
}
#[test]
fn back_forward_up_and_breadcrumb_navigation_keep_history_consistent() {
    let h = Home::new();
    let one = h.dir("one");
    let two = h.dir("one/two");
    let other = h.dir("other");
    let (mut s, mut shared) = h.session(json!({}));
    for path in [&one, &two] {
        event(&mut s, &mut shared, "navigate", json!(path));
        ready(&mut s, &shared);
    }
    event(&mut s, &mut shared, "back", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(one.clone()));
    event(&mut s, &mut shared, "forward", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(two));
    event(&mut s, &mut shared, "up", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(one));
    event(&mut s, &mut shared, "back", Value::Null);
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "navigate", json!(other.clone()));
    ready(&mut s, &shared);
    assert!(s.tab().forward.is_empty());
    event(&mut s, &mut shared, "forward", Value::Null);
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(other));
}
#[test]
fn folder_links_file_urls_relative_addresses_and_failed_paths_are_recoverable() {
    let h = Home::new();
    let dest = h.dir("Folder with spaces");
    h.file("Folder with spaces/inside.txt", "data");
    symlink(&dest, h.path("shortcut")).unwrap();
    let (mut s, mut shared) = h.session(json!({}));
    let index = s
        .tab()
        .entries
        .iter()
        .position(|e| e.name == "shortcut")
        .unwrap();
    event(&mut s, &mut shared, "activate", json!(index));
    ready(&mut s, &shared);
    assert_eq!(s.tab().entries[0].name, "inside.txt");
    event(&mut s, &mut shared, "back", Value::Null);
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "goTo", Value::Null);
    enter_text(
        &mut s,
        &mut shared,
        &format!("file://{}/Folder%20with%20spaces", h.root.display()),
    );
    assert_eq!(s.tab().location, Location::Directory(dest.clone()));
    event(&mut s, &mut shared, "goTo", Value::Null);
    enter_text(&mut s, &mut shared, "..");
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
    event(&mut s, &mut shared, "goTo", Value::Null);
    enter_text(&mut s, &mut shared, "Folder with spaces");
    assert_eq!(s.tab().location, Location::Directory(dest.clone()));
    event(&mut s, &mut shared, "goTo", Value::Null);
    enter_text(&mut s, &mut shared, "does-not-exist");
    assert!(s.error.is_some());
    assert_eq!(s.tab().location, Location::Directory(dest));
}
#[test]
fn shift_selection_excludes_hidden_filtered_files_and_extends_from_cursor() {
    let h = Home::new();
    for name in ["a.txt", "b.txt", "c.txt", "hidden.log"] {
        h.file(name, "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    s.event(&json!({"action":["query"],"value":".txt"}), &mut shared);
    select(&mut s, &mut shared, "a.txt");
    event(&mut s, &mut shared, "view", json!("list"));
    for _ in 0..2 {
        s.event(
            &json!({"action":["key"],"key":"ArrowDown","shift":true}),
            &mut shared,
        );
    }
    assert_eq!(
        s.selected()
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "b.txt", "c.txt"]
    );
    s.event(
        &json!({"action":["key"],"key":"ArrowUp","shift":true}),
        &mut shared,
    );
    assert_eq!(s.selected().len(), 2);
    // Arrange an excluded entry between two visible entries in the backing list.
    let hidden = ops::entry(&h.path("hidden.log")).unwrap();
    s.tab_mut().entries.insert(1, hidden);
    select(&mut s, &mut shared, "a.txt");
    let end = s
        .tab()
        .entries
        .iter()
        .position(|e| e.name == "c.txt")
        .unwrap();
    s.event(&json!({"action":["select",end],"shift":true}), &mut shared);
    assert!(s.selected().iter().all(|e| e.name.ends_with(".txt")));
}
fn disks_fact(path: Option<&Path>) -> Value {
    json!([{"name":"sdb","transport":"usb","removable":true,"system":false,"partitions":[{"name":"sdb1","fstype":"exfat","label":"USB de teste","mountpoint":path.map(|p|p.to_string_lossy()).unwrap_or_default()}]}])
}
#[test]
fn hotplug_mount_and_eject_use_native_actions_and_update_the_sidebar() {
    let h = Home::new();
    let mount = h.dir("USB de teste");
    h.file("USB de teste/note.txt", "usb");
    let (mut s, mut shared) = h.session(json!({}));
    s.event(
        &json!({"action":["disks"],"disks":disks_fact(None)}),
        &mut shared,
    );
    assert!(s.render(&shared).to_string().contains("USB de teste"));
    event(&mut s, &mut shared, "volumeOpen", json!("sdb1"));
    assert!(s
        .effects
        .iter()
        .any(|e| e["kind"] == "diskMount" && e["device"] == "sdb1"));
    s.event(
        &json!({"action":["disks"],"disks":disks_fact(Some(&mount))}),
        &mut shared,
    );
    s.event(
        &json!({"action":["service"],"kind":"diskMount","result":mount.to_str()}),
        &mut shared,
    );
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(mount.clone()));
    assert_eq!(s.tab().entries[0].name, "note.txt");
    event(&mut s, &mut shared, "volumeRelease", json!("sdb1"));
    assert!(s.effects.iter().any(|e| e["kind"] == "diskRelease"
        && e["device"] == "sdb"
        && e["action"] == "disk-eject"));
    s.event(&json!({"action":["disks"],"disks":[]}), &mut shared);
    ready(&mut s, &shared);
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
    assert!(!s.render(&shared).to_string().contains("volumeOpen"));
}
#[test]
fn internal_ssd_is_listed_unmounted_and_failures_leave_files_untouched() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({}));
    s.event(&json!({"action":["disks"],"disks":[{"name":"nvme1n1","transport":"nvme","partitions":[{"name":"nvme1n1p1","fstype":"ext4","label":"SSD","mountpoint":""}]}]}),&mut shared);
    assert_eq!(shared.volumes.len(), 1);
    assert!(!shared.volumes[0].removable);
    event(&mut s, &mut shared, "volumeOpen", json!("nvme1n1p1"));
    s.event(
        &json!({"action":["service"],"kind":"diskMount","error":"disk.internalNeedsAdmin"}),
        &mut shared,
    );
    assert!(s.volume_busy.is_none());
    assert_eq!(s.error.as_deref(), Some("disk.internalNeedsAdmin"));
    assert_eq!(s.tab().location, Location::Directory(h.root.clone()));
}
#[test]
fn unplugging_a_volume_removes_stale_locations_from_every_tab_and_window() {
    let h = Home::new();
    let mount = h.dir("USB");
    let sub = h.dir("USB/sub");
    let (mut a, mut shared) = h.session(json!({}));
    let mut b = Session::new(2, &json!({}), &mut shared).unwrap();
    ready(&mut b, &shared);
    a.event(
        &json!({"action":["disks"],"disks":disks_fact(Some(&mount))}),
        &mut shared,
    );
    a.poll(&json!({}), &shared);
    b.poll(&json!({}), &shared);
    for s in [&mut a, &mut b] {
        event(s, &mut shared, "navigate", json!(sub));
        ready(s, &shared);
        event(s, &mut shared, "newTab", Value::Null);
        ready(s, &shared);
    }
    a.event(&json!({"action":["disks"],"disks":[]}), &mut shared);
    a.poll(&json!({}), &shared);
    b.poll(&json!({}), &shared);
    for s in [&a, &b] {
        for tab in &s.tabs {
            assert_eq!(tab.location, Location::Directory(h.root.clone()));
            assert!(!tab
                .back
                .iter()
                .any(|l| l.path().is_some_and(|p| p.starts_with(&mount))));
        }
    }
}
#[test]
fn mounted_secondary_disks_extend_and_revoke_writable_roots() {
    let h = Home::new();
    let outside = h.root.with_extension("disk");
    fs::create_dir(&outside).unwrap();
    let (mut s, mut shared) = h.session(json!({}));
    assert!(!s.policy.allowed(&outside));
    s.event(
        &json!({"action":["disks"],"disks":disks_fact(Some(&outside))}),
        &mut shared,
    );
    assert!(s.policy.allowed(&outside));
    ops::mkdir(&outside.join("new")).unwrap();
    s.event(&json!({"action":["disks"],"disks":[]}), &mut shared);
    assert!(!s.policy.allowed(&outside));
    assert!(ops::mkdir(&outside.join("denied")).is_err());
    fs::remove_dir_all(outside).unwrap();
}
#[test]
fn resizing_all_views_keeps_the_last_selected_file_visible() {
    let h = Home::new();
    for n in 0..200 {
        h.file(&format!("file-{n:03}.txt"), "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    fn cell<'a>(node: &'a Value, index: usize) -> Option<&'a Value> {
        if node["events"]["onClick"] == json!(["select", index]) {
            return Some(node);
        }
        node["children"]
            .as_array()?
            .iter()
            .find_map(|c| cell(c, index))
    }
    for mode in ["icons", "list", "columns", "gallery"] {
        event(&mut s, &mut shared, "view", json!(mode));
        for (w, h) in [(1920, 1080), (640, 359), (1366, 727), (700, 420)] {
            for side in [150., 340.] {
                s.side_width = side;
                s.poll(&json!({"width":w,"height":h}), &shared);
                s.event(&json!({"action":["key"],"key":"End"}), &mut shared);
                let index = *s.tab().selected.first().unwrap();
                let tree = s.render(&shared);
                let cell = cell(&tree, index).unwrap();
                let top = cell["style"]["top"].as_f64().unwrap() - tree_scroll(&tree);
                let height = cell["style"]["height"].as_f64().unwrap();
                assert!(
                    top >= -0.01 && top + height <= s.entry_height() + 0.01,
                    "{mode} {w} {h} {side}"
                );
            }
        }
    }
}

#[test]
fn broken_links_report_an_error_and_view_shortcuts_match_the_toolbar() {
    let h = Home::new();
    symlink("missing", h.path("broken")).unwrap();
    let (mut s, mut shared) = h.session(json!({}));
    select(&mut s, &mut shared, "broken");
    event(&mut s, &mut shared, "open", Value::Null);
    assert_eq!(s.error.as_deref(), Some("files.brokenLink"));
    assert!(!s.effects.iter().any(|e| e["kind"] == "openPath"));
    for (key, mode) in [
        ("1", "icons"),
        ("2", "list"),
        ("3", "columns"),
        ("4", "gallery"),
    ] {
        s.event(
            &json!({"action":["key"],"key":key,"ctrl":true}),
            &mut shared,
        );
        assert_eq!(s.tab().view, mode);
    }
}

#[test]
fn unformatted_disks_open_management_without_mounting_or_formatting() {
    let h = Home::new();
    let (mut s, mut shared) = h.session(json!({}));
    s.event(&json!({"action":["disks"],"disks":[{"name":"nvme1n1","transport":"nvme","fstype":"","partitions":[]}]}),&mut shared);
    assert_eq!(shared.volumes.len(), 1);
    assert!(!shared.volumes[0].mountable);
    event(&mut s, &mut shared, "volumeOpen", json!("nvme1n1"));
    assert!(s
        .effects
        .iter()
        .any(|e| e["kind"] == "manageDisk" && e["disk"] == "nvme1n1"));
    assert!(!s.effects.iter().any(|e| e["kind"] == "diskMount"));
}

#[test]
fn browser_polling_never_consumes_a_c_callers_copy_result() {
    let h = Home::new();
    let from = h.file("source.txt", "C consumer data");
    let to = h.path("target.txt");
    let (mut s, shared) = h.session(json!({}));
    let id = ops::start(&from, &to, false).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        s.poll(&json!({}), &shared);
        let jobs = ops::jobs();
        let job = jobs
            .iter()
            .find(|j| j.id == id)
            .expect("Files must not forget another caller's result");
        if job.done {
            assert!(!job.failed);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    s.poll(&json!({}), &shared);
    assert!(ops::jobs().iter().any(|j| j.id == id));
    assert_eq!(fs::read_to_string(to).unwrap(), "C consumer data");
    ops::forget(id);
}
#[test]
fn copy_failures_belong_to_the_initiating_window() {
    let h = Home::new();
    let from = h.path("source.pipe");
    let path = std::ffi::CString::new(from.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let destination = h.dir("destination");
    let (mut owner, mut shared) = h.session(json!({}));
    let mut other = Session::new(2, &json!({}), &mut shared).unwrap();
    ready(&mut other, &shared);
    select(&mut owner, &mut shared, "source.pipe");
    event(&mut owner, &mut shared, "copy", Value::Null);
    event(&mut owner, &mut shared, "navigate", json!(destination));
    ready(&mut owner, &shared);
    event(&mut owner, &mut shared, "paste", Value::Null);
    let id = ops::jobs()[0].id;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        other.poll(&json!({}), &shared);
        if ops::jobs().iter().any(|j| j.id == id && j.done) {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(other.job_errors.is_empty());
    owner.poll(&json!({}), &shared);
    assert!(!owner.job_errors.is_empty());
    assert!(!ops::jobs().iter().any(|j| j.id == id));
}
#[test]
fn closing_the_browser_does_not_abort_copies_or_leak_job_slots() {
    let h = Home::new();
    let from = h.file("source.txt", "keep copying after close");
    let destination = h.dir("destination");
    let (mut s, mut shared) = h.session(json!({}));
    select(&mut s, &mut shared, "source.txt");
    event(&mut s, &mut shared, "copy", Value::Null);
    event(&mut s, &mut shared, "navigate", json!(destination.clone()));
    ready(&mut s, &shared);
    event(&mut s, &mut shared, "paste", Value::Null);
    drop(s);
    let until = Instant::now() + Duration::from_secs(3);
    while !ops::jobs().is_empty() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fs::read(destination.join("source.txt")).unwrap(),
        fs::read(from).unwrap()
    );
}

#[test]
fn icon_grid_uses_available_width_and_keeps_scrollbar_clear() {
    let h = Home::new();
    for n in 0..80 {
        h.file(&format!("item-{n:03}.txt"), "");
    }
    for mode in ["browser", "open"] {
        let (mut s, mut shared) = h.session(json!({"mode":mode}));
        event(&mut s, &mut shared, "view", json!("icons"));
        for width in [480, 640, 701, 900, 1366] {
            s.poll(&json!({"width":width,"height":400}), &shared);
            let tree = s.render(&shared);
            fn cells<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
                if v["events"]["onClick"][0] == "select" {
                    out.push(v);
                }
                for c in v["children"].as_array().into_iter().flatten() {
                    cells(c, out);
                }
            }
            let mut visible = Vec::new();
            cells(&tree, &mut visible);
            let top = visible[0]["style"]["top"].as_f64().unwrap();
            let first_row: Vec<_> = visible
                .iter()
                .filter(|c| c["style"]["top"].as_f64() == Some(top))
                .collect();
            let last = first_row.last().unwrap();
            let right =
                last["style"]["left"].as_f64().unwrap() + last["style"]["width"].as_f64().unwrap();
            assert!((right - (s.entry_width() - 12.).max(1.)).abs() < 0.01);
            assert_eq!(visible[0]["style"]["height"], 100.);
            // Vertical keyboard movement must land directly below the first cell.
            s.event(&json!({"action":["key"],"key":"Home"}), &mut shared);
            s.event(&json!({"action":["key"],"key":"ArrowDown"}), &mut shared);
            assert_eq!(s.tab().cursor, Some(s.filtered()[first_row.len()]));
        }
    }
}

#[test]
#[ignore = "manual performance probe; reports timings rather than machine-specific limits"]
fn large_folder_interaction_profile() {
    let h = Home::new();
    h.file("template.txt", "");
    let (mut s, mut shared) = h.session(json!({}));
    let template = s
        .tab()
        .entries
        .iter()
        .find(|e| e.name == "template.txt")
        .unwrap()
        .clone();
    s.tab_mut().entries = (0..20_000)
        .map(|n| {
            let mut e = template.clone();
            e.name = format!("document-{n:05}.txt");
            e.raw = h.path(&e.name);
            e.path = e.raw.to_string_lossy().into_owned();
            e
        })
        .collect();
    s.poll(&json!({"width":900,"height":540}), &shared);
    for query in ["", "document-1"] {
        s.event(&json!({"action":["query"],"value":query}), &mut shared);
        let mut samples = Vec::new();
        let mut phases = [0u128; 4];
        let mut max_bytes = 0;
        for _ in 0..100 {
            let start = Instant::now();
            s.event(&json!({"action":["wheel"],"deltaY":20}), &mut shared);
            let event_done = start.elapsed().as_micros();
            s.poll(&json!({}), &shared);
            let poll_done = start.elapsed().as_micros();
            let tree = s.render(&shared);
            let _ = s.menus(&shared);
            let render_done = start.elapsed().as_micros();
            max_bytes = max_bytes.max(serde_json::to_vec(&tree).unwrap().len());
            let total = start.elapsed().as_micros();
            phases[0] += event_done;
            phases[1] += poll_done - event_done;
            phases[2] += render_done - poll_done;
            phases[3] += total - render_done;
            samples.push(total);
        }
        samples.sort();
        println!(
            "FILES PERF 20000 query={query:?}: median={}us p95={}us max_tree={}bytes; mean event={}us poll={}us render={}us serialize={}us",
            samples[50], samples[95], max_bytes, phases[0] / 100, phases[1] / 100, phases[2] / 100, phases[3] / 100
        );
        assert!(max_bytes < 200_000, "listing must remain virtualized");
    }
}

#[test]
#[ignore = "manual performance probe; reports timings rather than machine-specific limits"]
fn large_folder_sort_profile() {
    let h = Home::new();
    let template = ops::entry(&h.file("template.txt", "")).unwrap();
    let mut entries: Vec<_> = (0..20_000)
        .map(|n| {
            let mut e = template.clone();
            e.name = format!("Document-{:05}.txt", (n * 7919) % 20_000);
            e
        })
        .collect();
    let start = Instant::now();
    super::model::sort_entries(&mut entries, "name", false, true);
    println!("FILES PERF sort 20000: {}us", start.elapsed().as_micros());
    assert_eq!(entries[0].name, "Document-00000.txt");
}

#[test]
fn filtered_listing_tracks_search_hidden_sort_and_equal_sized_reloads() {
    let h = Home::new();
    let first = h.file("apple.txt", "");
    h.file("banana.txt", "");
    h.file(".secret", "");
    let (mut s, mut shared) = h.session(json!({}));
    let names = |s: &Session| {
        s.filtered()
            .iter()
            .map(|i| s.tab().entries[*i].name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&s), vec!["apple.txt", "banana.txt"]);
    s.event(&json!({"action":["query"],"value":"APPLE"}), &mut shared);
    assert_eq!(names(&s), vec!["apple.txt"]);
    for _ in 0..10 {
        s.event(&json!({"action":["wheel"],"deltaY":20}), &mut shared);
    }
    assert_eq!(names(&s), vec!["apple.txt"]);
    fs::rename(first, h.path("apricot.txt")).unwrap();
    event(&mut s, &mut shared, "refresh", Value::Null);
    ready(&mut s, &shared);
    assert!(names(&s).is_empty());
    s.event(&json!({"action":["query"],"value":""}), &mut shared);
    assert_eq!(names(&s), vec!["apricot.txt", "banana.txt"]);
    event(&mut s, &mut shared, "hidden", Value::Null);
    assert!(names(&s).contains(&".secret".into()));
    event(&mut s, &mut shared, "sort", json!("name"));
    assert_eq!(names(&s).last().unwrap(), ".secret");
}

#[test]
fn unchanged_undersized_layout_does_not_keep_redrawing() {
    let h = Home::new();
    let (mut s, shared) = h.session(json!({}));
    s.poll(&json!({"width":200,"height":100}), &shared);
    s.dirty = false;
    s.poll(&json!({"width":200,"height":100}), &shared);
    assert!(!s.dirty);
    assert_eq!((s.width, s.height), (320., 240.));
}

// How far the listing's moving container is scrolled, from the tree itself.
fn tree_scroll(node: &Value) -> f64 {
    let children = node["children"].as_array();
    if children
        .into_iter()
        .flatten()
        .any(|c| c["events"]["onClick"][0] == "select")
    {
        return -node["style"]["top"].as_f64().unwrap();
    }
    children
        .into_iter()
        .flatten()
        .map(tree_scroll)
        .fold(0., f64::max)
}

#[test]
fn scrolling_only_changes_the_moving_container() {
    let h = Home::new();
    for n in 0..200 {
        h.file(&format!("item-{n:03}.txt"), "");
    }
    let (mut s, mut shared) = h.session(json!({}));
    s.poll(&json!({"width":900,"height":540}), &shared);
    fn cells(v: &Value, out: &mut Vec<Value>) {
        if v["events"]["onClick"][0] == "select" {
            out.push(v.clone());
        }
        for c in v["children"].as_array().into_iter().flatten() {
            cells(c, out);
        }
    }
    let (mut before, mut after) = (Vec::new(), Vec::new());
    cells(&s.render(&shared), &mut before);
    s.event(&json!({"action":["wheel"],"deltaY":10}), &mut shared);
    let tree = s.render(&shared);
    cells(&tree, &mut after);
    assert_eq!(
        before, after,
        "a scroll inside a row leaves every cell identical"
    );
    assert_eq!(tree_scroll(&tree), 10.);
    assert!(before[0]["key"].as_str().unwrap().starts_with("entry:"));
}

#[test]
fn wheel_at_the_end_of_the_list_needs_no_redraw() {
    let h = Home::new();
    h.file("only.txt", "");
    let (mut s, mut shared) = h.session(json!({}));
    s.poll(&json!({"width":900,"height":540}), &shared);
    s.dirty = false;
    s.event(&json!({"action":["wheel"],"deltaY":48}), &mut shared);
    assert!(!s.dirty);
    s.error = Some("fs.notFile".into());
    s.event(&json!({"action":["wheel"],"deltaY":48}), &mut shared);
    assert!(s.dirty, "clearing a visible error still draws");
}

#[test]
fn watcher_refresh_without_changes_does_not_redraw() {
    let h = Home::new();
    let (mut s, shared) = h.session(json!({}));
    h.file("new.txt", "");
    for _ in 0..200 {
        s.dirty = false;
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.poll(&json!({}), &shared);
        if s.tab().entries.iter().any(|e| e.name == "new.txt") {
            break;
        }
        assert!(
            !s.visibly_loading(),
            "a background refresh keeps the listing"
        );
    }
    assert!(s.tab().entries.iter().any(|e| e.name == "new.txt"));
    assert!(s.dirty, "a different listing draws");
    // Same permissions: the watcher wakes (IN_ATTRIB), but the listing is unchanged.
    while s.loading {
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.poll(&json!({}), &shared);
    }
    let file = h.path("new.txt");
    let generation = s.generation.load(std::sync::atomic::Ordering::Relaxed);
    fs::set_permissions(&file, fs::metadata(&file).unwrap().permissions()).unwrap();
    s.dirty = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.poll(&json!({}), &shared);
        assert!(!s.dirty, "an identical listing draws nothing");
    }
    assert!(s.generation.load(std::sync::atomic::Ordering::Relaxed) > generation);
    assert!(!s.loading);
}

#[test]
fn sorting_keeps_folders_first_and_reverses_only_within_each_group() {
    let h = Home::new();
    let template = ops::entry(&h.file("template.txt", "")).unwrap();
    let e = |name: &str, dir, size| {
        let mut e = template.clone();
        e.name = name.into();
        e.dir = dir;
        e.size = size;
        e
    };
    let mut entries = vec![
        e("b", false, 1),
        e("a", true, 9),
        e("c10", false, 1),
        e("c2", false, 2),
        e("z", true, 1),
    ];
    let names = |v: &[ops::Entry]| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    super::model::sort_entries(&mut entries, "name", false, true);
    assert_eq!(names(&entries), ["a", "z", "b", "c2", "c10"]);
    super::model::sort_entries(&mut entries, "name", true, true);
    assert_eq!(names(&entries), ["z", "a", "c10", "c2", "b"]);
    super::model::sort_entries(&mut entries, "size", false, false);
    assert_eq!(names(&entries), ["b", "c10", "z", "c2", "a"]);
    super::model::sort_entries(&mut entries, "size", true, false);
    assert_eq!(names(&entries), ["a", "c2", "z", "c10", "b"]);
}

#[test]
fn natural_order_matches_people_and_ignores_case_and_leading_zeros() {
    use std::cmp::Ordering::*;
    for (a, b, order) in [
        ("file2", "file10", Less),
        ("file10", "file2", Greater),
        ("file010", "file10", Equal),
        ("File", "file", Equal),
        ("a", "ab", Less),
        ("a1b", "a1", Greater),
        ("9", "10", Less),
        ("123", "124", Less),
        ("1234", "124", Greater),
        ("007x", "7y", Less),
        ("", "", Equal),
        ("Água", "água", Equal),
    ] {
        assert_eq!(super::model::natural_cmp(a, b), order, "{a} vs {b}");
    }
}

#[test]
fn contextual_menu_only_contains_available_actions_in_compact_groups() {
    let h = Home::new();
    h.file("photo.png", "");
    let (mut s, mut shared) = h.session(json!({}));
    fn menu(v: &Value) -> Option<&Value> {
        if v["type"] == "contextMenu" {
            return Some(v);
        }
        v["children"].as_array()?.iter().find_map(menu)
    }
    event(&mut s, &mut shared, "context", Value::Null);
    let tree = s.render(&shared);
    let items = menu(&tree).unwrap()["items"].as_array().unwrap();
    assert!(items.len() <= 10);
    assert!(!items.iter().any(|i| i["action"][0] == "open"
        || i["action"][0] == "copy"
        || i["action"][0] == "paste"));
    assert!(items.iter().any(|i| i["submenu"].is_array()));
    assert!(items.iter().all(|i| i["disabled"] != true));
    select(&mut s, &mut shared, "photo.png");
    let index = *s.tab().selected.first().unwrap();
    event(&mut s, &mut shared, "context", json!(index));
    let tree = s.render(&shared);
    let items = menu(&tree).unwrap()["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["action"][0] == "open"));
    assert!(!items.iter().any(|i| i["action"][0] == "newFolder"));
    assert!(items.iter().any(|i| i["label"]["key"] == "files.menu.tags"
        && i["submenu"].as_array().is_some_and(|v| v.len() == 7)));
}
