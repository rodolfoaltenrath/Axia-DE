use axia_app_files::FilesView;
use crate::{native_view::Host, runtime::native as scene};
use serde_json::{json, Value};
use std::time::Duration;

#[test]
fn late_preview_results_release_resources_without_a_live_window() {
    super::install_services();
    super::RELEASED_PREVIEWS.with(|r| r.borrow_mut().clear());
    for value in [json!({"kind":"mediaPreview","result":78}),
                  json!({"kind":"documentPreview","result":{"id":90}})] {
        let reply = std::ffi::CString::new(value.to_string()).unwrap();
        unsafe { super::axia_view_result(u32::MAX, reply.as_ptr()); }
    }
    super::RELEASED_PREVIEWS.with(|r| assert_eq!(&*r.borrow(), &[(true, 78), (false, 90)]));
}
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Home {
    root: std::path::PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}
impl Home {
    fn new() -> Self {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("axia-files-host-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        axia_app_files::policy::init(&root, "axia-tests");
        Self { root, _guard: guard }
    }
    fn path(&self, path: &str) -> std::path::PathBuf { self.root.join(path) }
    fn file(&self, path: &str, content: &str) { std::fs::write(self.path(path), content).unwrap(); }
}
impl Drop for Home { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }
    fn facts(path: &std::path::Path) -> Value {
        json!({"args":{"path":path},"settings":{"files":{}},"catalog":[],"registry":[],"lang":"en","focused":true,
            "context":{"theme":{"mode":"dark","text":"#ffffff","textDim":"#aaaaaa","accent":"#0a84ff","control":"#333333","separator":"#444444"}}})
    }
    /// The listing comes from a worker: tick the host until `text` is on screen.
    fn wait_for(host: &mut Host, text: &str) {
        for _ in 0..300 {
            host.tick(1e18);
            if host.text().contains(text) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{text:?} never showed: {}", host.text());
    }


#[test]
    fn a_files_window_lists_a_folder_and_acts_on_clicks_with_no_react() {
        let home = Home::new();
        home.file("report.txt", "x");
        std::fs::create_dir_all(home.path("Folder")).unwrap();
        home.file("Folder/inner.txt", "y");
        let mount = scene::create(scene::VIEW);
        let mut host = Host::new(mount, Box::new(FilesView::default()), "", None);
        host.facts(facts(&home.path("")));
        wait_for(&mut host, "report.txt");
        assert!(host.text().contains("Folder"), "{}", host.text());
        // double-clicking a folder opens it: its own file is listed and the old ones are gone
        let folder = host.find_text("Folder");
        assert_ne!(folder, 0, "the folder's name is on screen");
        for kind in ["move", "down", "up", "down", "up"] {
            host.pointer(kind, folder, 5., 5., 0);
        }
        wait_for(&mut host, "inner.txt");
        assert!(!host.text().contains("report.txt"), "the listing changed: {}", host.text());
        drop(host);
        scene::destroy(mount);
    }
#[test]
    fn native_folder_receives_external_copies_and_explicit_internal_moves() {
        let home=Home::new();
        home.file("source.txt","copied bytes");home.file("move.txt","moved bytes");
        std::fs::create_dir_all(home.path("Destination")).unwrap();
        let mount=scene::create(scene::VIEW);
        let mut host=Host::new(mount,Box::new(FilesView::default()),"",None);
        host.facts(facts(&home.path("Destination")));
        wait_for(&mut host,"Destination");
        // The destination is only a drop target once the worker has verified it, and again after every
        // reload (a job's result), under a new token: ask for it each time, as the compositor does.
        let token_of=|host:&mut Host|{
            for _ in 0..300 {
                host.tick(1e18);
                let title=host.find_text("Destination");
                let token=unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(title))};
                if token!=0 {return token;}
                std::thread::sleep(Duration::from_millis(20));
            }
            0
        };
        let token=token_of(&mut host);
        assert_ne!(token,0);
        assert!(host.file_drop(token,&json!([home.path("source.txt")]),false));
        wait_for(&mut host,"source.txt");
        assert_eq!(std::fs::read_to_string(home.path("Destination/source.txt")).unwrap(),"copied bytes");
        assert!(home.path("source.txt").exists());
        let token=token_of(&mut host);
        assert_ne!(token,0,"the destination is a drop target again after the reload");
        assert!(host.file_drop(token,&json!([home.path("move.txt")]),true));
        wait_for(&mut host,"move.txt");
        assert_eq!(std::fs::read_to_string(home.path("Destination/move.txt")).unwrap(),"moved bytes");
        assert!(!home.path("move.txt").exists());
        drop(host);scene::destroy(mount);
    }
