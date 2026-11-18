use super::*;
#[test]
fn anchored_zoom_and_rotation_crop() {
    let mut v = View {
        width: 800,
        height: 600,
        ..Default::default()
    };
    let p = v.point(2000, 1600, 300., 250.);
    v.zoom_at(2000, 1600, 3., 300., 250.);
    let q = v.point(2000, 1600, 300., 250.);
    assert!((p.0 - q.0).abs() < 0.001);
    assert!((p.1 - q.1).abs() < 0.001);
    v.rotation = 1;
    v.zoom = 0.;
    assert_eq!(v.size(2000, 1600), (1600, 2000));
    assert!(v.crop(2000, 1600, (100., 100.), (500., 500.)).is_some());
}
#[test]
fn transparent_pyramid_has_no_color_halos() {
    let mut rgba = image::RgbaImage::new(3, 3);
    rgba.put_pixel(0, 0, image::Rgba([255, 0, 0, 0]));
    rgba.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
    let f = decode::pyramid(rgba, 1);
    assert_eq!(f.levels[0].pixels[0], 0);
    assert_eq!((f.levels[1].width, f.levels[1].height), (2, 2));
    assert_eq!(f.levels[1].pixels[0] >> 16 & 255, 0);
    assert_eq!(f.delay, 20);
}
#[test]
fn viewport_memory_is_independent_of_zoom() {
    let f = decode::pyramid(
        image::RgbaImage::from_pixel(1000, 800, image::Rgba([50, 100, 200, 255])),
        100,
    );
    let bytes = f.levels.iter().map(|l| l.pixels.len() * 4).sum();
    let p = decode::Photo {
        width: 1000,
        height: 800,
        frames: vec![f],
        bytes,
    };
    let serial = std::sync::atomic::AtomicU64::new(1);
    for zoom in [0., 1., 16., 32.] {
        let r = render::render(
            &p,
            View {
                width: 128,
                height: 100,
                zoom,
                ..Default::default()
            },
            0,
            1,
            &serial,
        )
        .unwrap();
        assert_eq!(r.pixels.len(), 12800);
        assert!(r.pixels.iter().all(|p| p >> 24 == 255));
    }
    assert!(render::render(&p, View::default(), 0, 0, &serial).is_none());
}
#[test]
fn app_icon_never_starts_workers() {
    let mut app = App::default();
    let icon = app.request(&json!({"cmd":"icon","size":48}));
    assert_eq!(icon["kind"], "view");
    assert!(app.worker.is_none());
    assert!(app.library.is_none());
}

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
fn picture(s: &Scratch, name: &str) -> Entry {
    let p = s.0.join(name);
    image::RgbaImage::from_pixel(8, 6, image::Rgba([30, 80, 150, 255]))
        .save(&p)
        .unwrap();
    Entry::inspect(&p).unwrap()
}
#[test]
fn copies_are_exclusive_and_original_is_unchanged() {
    let s = Scratch::new();
    let e = picture(&s, "original.png");
    let original = std::fs::read(&e.path).unwrap();
    let rgba = image::RgbaImage::from_pixel(4, 3, image::Rgba([255, 0, 0, 255]));
    let a = io::save(&e, &s.0, &rgba, "edited").unwrap();
    let b = io::save(&e, &s.0, &rgba, "edited").unwrap();
    assert_ne!(a, b);
    assert_eq!(std::fs::read(&e.path).unwrap(), original);
    assert_eq!(image::open(a).unwrap().width(), 4);
    assert!(!std::fs::read_dir(&s.0).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
}
#[test]
fn source_replacement_is_rejected_and_scans_skip_links() {
    use std::os::unix::fs::symlink;
    let s = Scratch::new();
    let e = picture(&s, "a.png");
    let replacement = picture(&s, "b.png");
    std::fs::rename(replacement.path, &e.path).unwrap();
    assert!(io::read(&e, &s.0).is_err());
    symlink(&s.0, s.0.join("loop")).unwrap();
    symlink(&e.path, s.0.join("linked.png")).unwrap();
    let serial = std::sync::atomic::AtomicU64::new(1);
    let (list, _) = io::scan(&s.0, true, &s.0, 1, &serial).unwrap();
    assert_eq!(list.len(), 1);
    assert!(io::scan(&s.0, true, &s.0, 0, &serial).is_err());
}
#[test]
fn editing_rotation_and_crop_maps_exact_pixels() {
    let rgba =
        image::RgbaImage::from_fn(3, 2, |x, y| image::Rgba([(x + y * 3) as u8, 10, 20, 255]));
    let f = decode::pyramid(rgba, 100);
    let p = decode::Photo {
        width: 3,
        height: 2,
        frames: vec![f],
        bytes: 32,
    };
    let out = decode::edit(
        &p,
        View {
            rotation: 1,
            ..Default::default()
        },
        Some([0, 1, 2, 1]),
    )
    .unwrap();
    assert_eq!(out.dimensions(), (2, 1));
    assert_eq!(out.get_pixel(0, 0).0, [4, 10, 20, 255]);
    assert_eq!(out.get_pixel(1, 0).0, [1, 10, 20, 255]);
    assert!(decode::edit(&p, View::default(), Some([3, 0, 1, 1])).is_err());
}
#[test]
fn animated_gif_delays_loop_and_thumbnail_uses_only_one_frame() {
    let s = Scratch::new();
    let path = s.0.join("animated.gif");
    let f = std::fs::File::create(&path).unwrap();
    let mut encoder = image::codecs::gif::GifEncoder::new(f);
    for (color, delay) in [([255, 0, 0, 255], 40), ([0, 0, 255, 255], 80)] {
        encoder
            .encode_frame(image::Frame::from_parts(
                image::RgbaImage::from_pixel(4, 4, image::Rgba(color)),
                0,
                0,
                image::Delay::from_numer_denom_ms(delay, 1),
            ))
            .unwrap();
    }
    drop(encoder);
    let e = Entry::inspect(&path).unwrap();
    let serial = std::sync::atomic::AtomicU64::new(1);
    let p = decode::load(&e, &s.0, 1, &serial).unwrap();
    assert_eq!(p.frames.len(), 2);
    assert_eq!(
        [
            p.frame(0),
            p.frame(39),
            p.frame(40),
            p.frame(119),
            p.frame(120)
        ],
        [0, 0, 1, 1, 0]
    );
    let thumb = decode::thumbnail(&e, &s.0, 1, &serial).unwrap();
    assert_eq!((thumb.width, thumb.height), (192, 152));
    assert_eq!(thumb.pixels[76 * 192 + 96], 0xffff0000);
}
#[test]
fn direct_open_and_worker_latest_request_do_not_need_js_media() {
    let s = Scratch::new();
    let e = picture(&s, "test.png");
    let w = worker::Worker::default();
    for width in 12..24 {
        w.submit(worker::Task::View {
            entry: e.clone(),
            home: s.0.clone(),
            view: View {
                width,
                height: 10,
                ..Default::default()
            },
            elapsed: 0,
        });
    }
    let start = std::time::Instant::now();
    loop {
        let out = w.take();
        if let Some((id, worker::ResultValue::View(result))) = out.into_iter().last() {
            if id == w.serial.load(std::sync::atomic::Ordering::Relaxed) {
                assert_eq!(result.unwrap().raster.width, 23);
                break;
            }
        }
        assert!(start.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn photos_ui_is_virtual_and_crop_uses_captured_pointer_coordinates() {
    let mut a = App::default();
    a.width = 980.;
    a.height = 640.;
    for i in 0..20000 {
        a.entries.push(Entry {
            path: format!("/tmp/{i}.png").into(),
            name: format!("{i}.png"),
            size: 1,
            mtime: 1,
            stamp: (1, i, 0, 0),
        });
    }
    a.rebuild_layout();
    assert!(a.visible().len() < 40);
    let tree = ui::tree(&a).to_string();
    assert!(tree.len() < 100000, "{}", tree.len());
    a.current = Some(a.entries[0].clone());
    a.image_size = (2000, 1600);
    a.edit = true;
    a.command("pan", &json!({"x":100,"y":148,"button":0}), 1);
    a.command("pointerMove", &json!({"x":400,"y":448}), 1);
    assert!(a.crop.is_some());
    assert!(a.drag.is_some());
    a.command("dragEnd", &Value::Null, 1);
    assert!(a.drag.is_none());
}
#[test]
#[ignore = "manual release performance measurement; no machine-dependent threshold"]
fn photos_render_benchmark() {
    let f = decode::pyramid(
        image::RgbaImage::from_fn(6000, 4000, |x, y| {
            image::Rgba([x as u8, y as u8, (x + y) as u8, 255])
        }),
        100,
    );
    let bytes = f.levels.iter().map(|l| l.pixels.len() * 4).sum();
    let p = decode::Photo {
        width: 6000,
        height: 4000,
        frames: vec![f],
        bytes,
    };
    let serial = std::sync::atomic::AtomicU64::new(1);
    for (width, height, quality, n) in [(1280, 720, true, 30), (3840, 2074, false, 10)] {
        for zoom in [0., 1., 16.] {
            let view = View {
                width,
                height,
                quality,
                zoom,
                ..Default::default()
            };
            let started = std::time::Instant::now();
            for _ in 0..n {
                std::hint::black_box(render::render(&p, view, 0, 1, &serial).unwrap());
            }
            eprintln!("Photos 24 MP, {width}x{height}, filtered={quality}, zoom {zoom}: {:.2} ms/frame; pyramid {:.2} MiB",started.elapsed().as_secs_f64()*1000./n as f64,p.bytes as f64/1048576.);
        }
    }
}



#[test]
fn tiff_all_eight_orientations_match_rust_geometry() {
    let s = Scratch::new();
    let serial = std::sync::atomic::AtomicU64::new(1);
    for orientation in 1..=8u16 {
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        bytes.extend(10u16.to_le_bytes());
        for (tag, kind, count, value) in [
            (256u16, 4u16, 1u32, 2u32),
            (257, 4, 1, 1),
            (258, 3, 3, 134),
            (259, 3, 1, 1),
            (262, 3, 1, 2),
            (273, 4, 1, 140),
            (274, 3, 1, orientation as u32),
            (277, 3, 1, 3),
            (278, 4, 1, 1),
            (279, 4, 1, 6),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(count.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        bytes.extend([8, 0, 8, 0, 8, 0, 255, 0, 0, 0, 0, 255]);
        let path = s.0.join(format!("{orientation}.tiff"));
        std::fs::write(&path, bytes).unwrap();
        let e = Entry::inspect(&path).unwrap();
        let decoded = match decode::load(&e, &s.0, 1, &serial) {
            Err(e) if e == "photos.codecUnavailable" => return,
            other => other.unwrap(),
        };
        let mut expected =
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(2, 1, |x, _| {
                if x == 0 {
                    image::Rgba([255, 0, 0, 255])
                } else {
                    image::Rgba([0, 0, 255, 255])
                }
            }));
        expected
            .apply_orientation(image::metadata::Orientation::from_exif(orientation as u8).unwrap());
        assert_eq!(
            decoded.rgba(),
            expected.to_rgba8(),
            "orientation {orientation}"
        );
    }
}
#[test]
fn sparse_oversized_file_is_rejected_before_decode() {
    let s = Scratch::new();
    let path = s.0.join("large.png");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(io::MAX_FILE + 1)
        .unwrap();
    let e = Entry::inspect(&path).unwrap();
    assert!(io::read(&e, &s.0).is_err());
}

#[test]
fn fullscreen_checkpoint_restores_view_edits_and_slideshow_without_reusing_old_open_with_args() {
    let s = Scratch::new();
    let e = picture(&s, "a.png");
    let mut a = App::default();
    a.home = s.0.clone();
    a.window = "photos".into();
    a.folder = s.0.clone();
    a.args = json!({"path":e.path,"nonce":7});
    a.open(e.clone(), 10);
    a.view.zoom = 3.;
    a.view.rotation = 1;
    a.view.brightness = 25;
    a.edit = true;
    a.crop = Some([1, 1, 2, 2]);
    a.slide = true;
    a.full(true);
    let state = a
        .effects
        .iter()
        .find(|e| e["kind"] == "windowState")
        .unwrap()["state"]
        .clone();
    let mut b = App::default();
    b.home = s.0.clone();
    b.restore(&state, &a.args, 20);
    assert_eq!(b.current.unwrap().path, e.path);
    assert_eq!(b.view.zoom, 3.);
    assert_eq!(b.view.rotation, 1);
    assert_eq!(b.view.brightness, 25);
    assert_eq!(b.crop, Some([1, 1, 2, 2]));
    assert!(b.edit && b.slide);
    let mut c = App::default();
    c.restore(&state, &json!({"path":e.path,"nonce":8}), 20);
    assert!(c.current.is_none());
}
#[test]
fn slideshow_waits_for_fullscreen_effect_and_stops_on_actual_exit() {
    let s = Scratch::new();
    let e = picture(&s, "a.png");
    let mut a = App::default();
    a.resources("", Some(s.0.to_str().unwrap()));
    let mut facts = json!({"cmd":"sync","lang":"en","args":{"path":e.path,"__nativeWindow":"photos"},"context":{"width":800,"height":600,"windowMode":"window"}});
    a.request(&facts);
    a.command("slideshow", &Value::Null, now_ms());
    assert!(a.slide);
    a.request(&facts);
    assert!(
        a.slide,
        "the WM has not executed the queued fullscreen effect yet"
    );
    facts["context"]["windowMode"] = json!("fullscreen");
    a.request(&facts);
    assert!(a.slide);
    facts["context"]["windowMode"] = json!("window");
    a.request(&facts);
    assert!(!a.slide);
}

#[test]
fn slow_viewports_use_cancelable_previews_and_refine_after_interaction() {
    let mut a = App::default();
    a.current = Some(Entry {
        path: "/tmp/test.png".into(),
        name: "test.png".into(),
        size: 0,
        mtime: 0,
        stamp: (0, 0, 0, 0),
    });
    a.image_size = (6000, 4000);
    a.quality_ms = 30.;
    a.command("wheel", &json!({"x":300,"y":200,"deltaY":-48}), 1000);
    assert!(!a.view.quality);
    assert!(a.needs_refine);
    a.poll(1090);
    assert!(!a.view.quality);
    a.poll(1100);
    assert!(a.view.quality);
    assert!(!a.needs_refine);
}
#[test]
fn exiting_viewer_stops_animation_refinement_and_pending_work() {
    let mut a = App::default();
    a.current = Some(Entry {
        path: "/tmp/gif.gif".into(),
        name: "gif.gif".into(),
        size: 0,
        mtime: 0,
        stamp: (0, 0, 0, 0),
    });
    a.animated = true;
    a.needs_refine = true;
    a.busy = true;
    a.command("escape", &Value::Null, 100);
    assert!(a.current.is_none());
    assert!(!a.animated && !a.needs_refine && !a.busy);
}
#[test]
fn one_pixel_wide_images_still_get_filtered_mipmaps() {
    let f = decode::pyramid(
        image::RgbaImage::from_fn(1, 16, |_, y| {
            image::Rgba([if y % 2 == 0 { 0 } else { 255 }, 0, 0, 255])
        }),
        100,
    );
    assert_eq!(f.levels.last().unwrap().height, 1);
    assert_eq!(f.levels[1].pixels[0] >> 16 & 255, 128);
}
#[test]
#[ignore = "manual optional system-codec fixture, supplied through AXIA_PHOTOS_CODEC_FIXTURE"]
fn photos_system_codec_fixture() {
    let path = std::env::var("AXIA_PHOTOS_CODEC_FIXTURE").unwrap();
    let e = Entry::inspect(std::path::Path::new(&path)).unwrap();
    let current = std::sync::atomic::AtomicU64::new(1);
    let p = decode::load(&e, std::path::Path::new("/tmp"), 1, &current).unwrap();
    eprintln!("decoded {}: {} x {}", e.name, p.width, p.height);
    assert!(p.width > 0 && p.height > 0);
}
#[test]
fn large_folder_lists_are_virtual_and_still_reach_the_last_folder() {
    let mut a = App::default();
    for i in 0..20000 {
        a.folders.push(format!("/tmp/folder{i}").into());
    }
    assert!(ui::tree(&a).to_string().len() < 30000);
    a.command("folderWheel", &json!({"deltaY":1e9}), 100);
    let tree = ui::tree(&a).to_string();
    assert!(tree.contains("folder19999"));
    assert!(tree.len() < 30000);
}
#[test]
fn opaque_fast_paths_match_the_general_compositor_for_every_rotation() {
    let f = decode::pyramid(
        image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 9) as u8, (x + y) as u8, 255])
        }),
        100,
    );
    let mut p = decode::Photo {
        width: 32,
        height: 24,
        frames: vec![f],
        bytes: 4096,
    };
    let serial = std::sync::atomic::AtomicU64::new(1);
    for rotation in 0..4 {
        for zoom in [0., 1., 16.] {
            for quality in [false, true] {
                for brightness in [0, 30] {
                    let view = View {
                        width: 40,
                        height: 30,
                        rotation,
                        zoom,
                        quality,
                        brightness,
                        ..Default::default()
                    };
                    p.frames[0].opaque = true;
                    let fast = render::render(&p, view, 0, 1, &serial).unwrap();
                    p.frames[0].opaque = false;
                    let general = render::render(&p, view, 0, 1, &serial).unwrap();
                    assert_eq!(fast.pixels,general.pixels,"rotation {rotation}, zoom {zoom}, quality {quality}, brightness {brightness}");
                }
            }
        }
    }
}
#[test]
fn invalid_empty_photo_cannot_panic_the_render_worker() {
    let current = std::sync::atomic::AtomicU64::new(1);
    let p = decode::Photo {
        width: 0,
        height: 0,
        frames: vec![],
        bytes: 0,
    };
    assert!(render::render(&p, View::default(), 0, 1, &current).is_none());
}
#[test]
fn navigation_hides_previous_pixels_and_ignores_zoom_until_new_image_is_ready() {
    let s = Scratch::new();
    let e = picture(&s, "next.png");
    let mut a = App::default();
    a.current = Some(e.clone());
    a.image_size = (8, 6);
    a.open(e, 10);
    assert!(!ui::tree(&a).to_string().contains("photo-raster"));
    a.command("zoomIn", &Value::Null, 20);
    assert_eq!(a.view.zoom, 0.);
}
