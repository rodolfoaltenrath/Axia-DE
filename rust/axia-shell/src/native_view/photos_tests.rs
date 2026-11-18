use axia_app_photos::App;
use serde_json::json;
struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "axia-photos-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn photos_native_host_loads_owned_viewport_and_routes_pinch() {
    use crate::{native_view::Host, runtime::native as scene};
    let s = Scratch::new();
    let path = s.0.join("photo.png");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([30, 80, 150, 255])).save(&path).unwrap();
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = crate::runtime::node_get(mount);
        (*n).w = 800.;
        (*n).h = 480.;
    }
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mut host = Host::new(
        mount,
        Box::new(App::default()),
        assets.to_str().unwrap(),
        Some(s.0.to_str().unwrap()),
    );
    host.facts(json!({"lang":"en","args":{"path":path,"__nativeWindow":"photos"},"context":{"theme":{"text":"#222","textDim":"#666","accent":"#06c","windowBg":"#fff"}}}));
    fn image(root: i32) -> i32 {
        unsafe {
            let n = crate::runtime::node_get(root);
            if n.is_null() {
                return 0;
            }
            if (*n).kind == scene::IMAGE && !(*n).img.is_null() {
                return root;
            }
            let mut child = (*n).first;
            while !child.is_null() {
                let found = image((*child).id);
                if found != 0 {
                    return found;
                }
                child = (*child).next;
            }
            0
        }
    }
    let start = std::time::Instant::now();
    let target = loop {
        host.tick(f64::MAX);
        let target = image(mount);
        if target != 0 {
            break target;
        }
        assert!(start.elapsed().as_secs() < 5, "{}", host.text());
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(scene::image_dimensions(target), (800, 394));
    let before = host.text();
    host.gesture("pinch", "begin", target, 1., 0.);
    host.gesture("pinch", "update", target, 2., 0.);
    assert_ne!(host.text(), before);
    host.pointer("down", target, 100., 100., 0);
    host.pointer("move", target, 130., 110., 0);
    host.pointer("up", target, 130., 110., 0);
    drop(host);
    scene::destroy(mount);
}