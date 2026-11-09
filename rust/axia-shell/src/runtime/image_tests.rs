use super::super::memory::AllocationBudget;
use super::*;

fn solid(w: i32, h: i32, color: u32) -> Pixels {
    let mut p = Pixels::empty(w, h).unwrap();
    p.data.fill(color);
    p.opaque = color >> 24 == 255;
    p
}
#[test]
fn rgba_premultiplication_and_argb_updates_preserve_channels_and_opacity() {
    let mut p = Pixels::empty(4, 1).unwrap();
    assert!(!p.opaque);
    assert_eq!(p.data, [0; 4]);
    p.rgba(&[
        255, 127, 3, 0, 255, 127, 3, 128, 255, 127, 3, 255, 1, 2, 3, 1,
    ]);
    assert_eq!(p.data, [0, 0x80804002, 0xffff7f03, 0x01000000]);
    assert!(!p.opaque);
    p.argb(&[0xff010203; 4]);
    assert!(p.opaque);
    p.argb(&[0x80010203; 4]);
    assert!(!p.opaque);
}
#[test]
fn shared_managed_wallpapers_release_all_buffers_after_the_last_display() {
    let mut cache = Cache::default();
    let path = c"/tmp/axia/wallpapers/picture.png";
    let a = cache.get(path, || Some(solid(16, 16, 0xffffffff)));
    let b = cache.get(path, || panic!("cache hit must not decode"));
    assert!(!a.is_null());
    assert_eq!(a, b);
    unsafe {
        (*a).scaled(24, 24, 1).unwrap();
        (*a).scaled(32, 32, 1).unwrap();
        assert_eq!(cache.managed_bytes(), (16 * 16 + 24 * 24 + 32 * 32) * 4);
        cache.release(a);
        assert_eq!((*b).refs, 1);
        assert_eq!(cache.paths.len(), 1);
        assert!(cache.managed_bytes() > 0);
        cache.release(b);
    }
    assert!(cache.paths.is_empty());
    assert_eq!(cache.managed_bytes(), 0);
}
#[test]
fn generated_tray_and_search_files_are_managed_but_theme_assets_remain_cached() {
    let mut cache = Cache::default();
    for path in [c"/tmp/axia/tray/a.png", c"/tmp/axia/search/a.png"] {
        let p = cache.get(path, || Some(solid(2, 2, 0xff123456)));
        unsafe {
            cache.release(p);
        }
        assert_eq!(cache.managed_bytes(), 0);
        assert!(cache.paths.is_empty());
    }
    let path = c"/theme/icons/a.png";
    let a = cache.get(path, || Some(solid(2, 2, 0xff123456)));
    unsafe {
        cache.release(a);
    }
    assert_eq!(cache.paths.len(), 1);
    let _fail = AllocationBudget::new(0);
    let b = cache.get(path, || panic!("theme asset should stay cached"));
    assert_eq!(a, b);
    assert_eq!(cache.managed_bytes(), 0);
    unsafe {
        cache.release(b);
    }
}
#[test]
fn outstanding_handles_survive_cache_teardown_in_either_tls_drop_order() {
    for path in [c"/theme/a.png", c"/tmp/axia/wallpapers/a.png"] {
        let mut cache = Cache::default();
        let a = cache.get(path, || Some(solid(2, 2, 0xffabcdef)));
        let b = cache.get(path, || unreachable!());
        drop(cache);
        unsafe {
            assert!(!(*a).cached);
            assert_eq!((*a).pixels.data, [0xffabcdef; 4]);
            image_release(a);
            assert_eq!((*b).refs, 1);
            image_release(b);
        }
    }
}
#[test]
fn renderer_handles_remain_valid_across_hash_table_growth_and_other_image_removal() {
    let mut cache = Cache::default();
    let original = cache.get(c"/tmp/axia/wallpapers/keep.png", || {
        Some(solid(1, 1, 0xff112233))
    });
    let n = if cfg!(miri) { 24 } else { 200 };
    let mut images = Vec::new();
    for i in 0..n {
        let path = CString::new(format!("/tmp/axia/search/{i}.png")).unwrap();
        let p = cache.get(&path, || Some(solid(1, 1, 0xff000000 | i)));
        assert!(!p.is_null());
        images.push(p);
    }
    for p in images {
        unsafe {
            cache.release(p);
        }
    }
    unsafe {
        assert_eq!((*original).pixels.data, [0xff112233]);
        assert_eq!(
            cache.get(c"/tmp/axia/wallpapers/keep.png", || unreachable!()),
            original
        );
        cache.release(original);
        cache.release(original);
    }
    assert!(cache.paths.is_empty());
}
#[test]
fn failed_cache_insertions_do_not_publish_partial_images_or_lose_existing_handles() {
    let (mut failures, mut successes) = (0, 0);
    for budget in 0..9 {
        let mut cache = Cache::default();
        let old = cache.get(c"/tmp/axia/wallpapers/old.png", || {
            Some(solid(1, 1, 0xff123456))
        });
        let new = {
            let _fail = AllocationBudget::new(budget);
            cache.get(c"/tmp/axia/search/new.png", || Pixels::empty(2, 2))
        };
        unsafe {
            assert_eq!((*old).pixels.data, [0xff123456]);
            if new.is_null() {
                failures += 1;
                assert_eq!(cache.paths.len(), 1);
            } else {
                successes += 1;
                cache.release(new);
            }
            cache.release(old);
        }
        assert!(cache.paths.is_empty());
    }
    assert!(failures > 1 && successes > 0);
}
#[test]
fn repeated_two_size_draws_reuse_both_cached_versions_without_allocating() {
    let mut image = Image::new(solid(8, 8, 0xff123456));
    let a = image.scaled(12, 12, 1).unwrap().data.as_ptr();
    let b = image.scaled(16, 16, 1).unwrap().data.as_ptr();
    let _fail = AllocationBudget::new(0);
    for _ in 0..5 {
        assert_eq!(image.scaled(12, 12, 1).unwrap().data.as_ptr(), a);
        assert_eq!(image.scaled(16, 16, 1).unwrap().data.as_ptr(), b);
    }
    // A third size can recycle the least recent buffer without allocating.
    assert_eq!(image.scaled(10, 10, 1).unwrap().data.as_ptr(), a);
    assert_eq!(image.scaled(16, 16, 1).unwrap().data.as_ptr(), b);
}
#[test]
fn scaling_failure_retains_the_latest_version_and_shrinking_retires_oversized_buffers() {
    let mut image = Image::new(solid(8, 8, 0xff123456));
    image.scaled(32, 32, 1).unwrap();
    image.scaled(16, 16, 1).unwrap();
    let bytes = image.bytes();
    {
        let _fail = AllocationBudget::new(0);
        assert!(image.scaled(64, 64, 1).is_none());
        assert!(image.scaled(16, 16, 1).is_some());
    }
    image.scaled(2, 2, 1).unwrap();
    image.scaled(3, 3, 1).unwrap();
    assert!(image.bytes() < bytes / 2);
}
#[test]
fn dynamic_updates_reuse_storage_and_resize_failures_keep_the_old_image_alive() {
    unsafe {
        let rgba = [255u8; 4 * 4];
        let old = image_pixels(ptr::null_mut(), 2, 2, rgba.as_ptr());
        assert!(!old.is_null());
        (*old).scaled(4, 4, 1).unwrap();
        {
            let _fail = AllocationBudget::new(0);
            assert_eq!(image_pixels(old, 2, 2, rgba.as_ptr()), old);
            assert!((*old).scales.iter().all(Option::is_none));
        }
        for budget in [0, 1] {
            let _fail = AllocationBudget::new(budget);
            assert!(image_argb(old, 3, 3, [0xff112233; 9].as_ptr()).is_null());
            assert_eq!((*old).pixels.data, [0xffffffff; 4]);
            assert_eq!(((*old).pixels.w, (*old).pixels.h), (2, 2));
        }
        let new = image_argb(old, 3, 3, [0xff112233; 9].as_ptr());
        assert!(!new.is_null());
        assert_eq!((*new).pixels.data, [0xff112233; 9]);
        image_release(new);
    }
}
#[test]
fn replacing_one_cached_reference_does_not_modify_the_other_display() {
    let path = c"/tmp/axia/wallpapers/shared.png";
    let a = IMAGES.with(|cache| {
        cache
            .borrow_mut()
            .get(path, || Some(solid(2, 2, 0xff123456)))
    });
    let b = IMAGES.with(|cache| cache.borrow_mut().get(path, || unreachable!()));
    unsafe {
        {
            let _fail = AllocationBudget::new(0);
            assert!(image_argb(a, 2, 2, [0xffabcdef; 4].as_ptr()).is_null());
        }
        assert_eq!((*b).refs, 2);
        let owned = image_argb(a, 2, 2, [0xffabcdef; 4].as_ptr());
        assert!(!owned.is_null());
        assert_ne!(owned, b);
        assert_eq!((*b).refs, 1);
        assert_eq!((*b).pixels.data, [0xff123456; 4]);
        assert_eq!((*owned).pixels.data, [0xffabcdef; 4]);
        image_release(b);
        image_release(owned);
    }
    assert_eq!(image_managed_bytes(), 0);
}
#[test]
fn image_abi_rejects_invalid_dimensions_before_reading_input_and_initializes_outputs() {
    unsafe {
        for (w, h) in [(0, 1), (-1, 2), (i32::MAX, 2), (32769, 1), (8193, 8193)] {
            assert!(image_pixels(ptr::null_mut(), w, h, ptr::dangling()).is_null());
            assert!(image_argb(ptr::null_mut(), w, h, ptr::dangling()).is_null());
        }
        assert!(image_pixels(ptr::null_mut(), 2, 2, ptr::null()).is_null());
        let mut w = std::mem::MaybeUninit::uninit();
        let mut h = std::mem::MaybeUninit::uninit();
        image_size(ptr::null(), w.as_mut_ptr(), h.as_mut_ptr());
        assert_eq!((w.assume_init(), h.assume_init()), (0, 0));
        image_release(ptr::null_mut());
    }
}
#[test]
fn clipped_and_nonfinite_draws_do_not_need_pixel_buffers_or_allocations() {
    let clip = Clip {
        x0: 0,
        y0: 0,
        x1: 100,
        y1: 100,
    };
    for (x, y, w, h) in [
        (101., 0., 50., 50.),
        (-50., 0., 50., 50.),
        (0., 0., 0., 50.),
        (f32::NAN, 0., 5., 5.),
        (0., 0., f32::INFINITY, 5.),
        (i32::MAX as f32, 0., 5., 5.),
    ] {
        assert!(draw_bounds(clip, x, y, w, h).is_none());
    }
    let draw = draw_bounds(clip, -0.5, 1.5, 8.5, 7.5).unwrap();
    assert_eq!((draw.x, draw.y, draw.w, draw.h), (-1, 2, 9, 8));
}
#[test]
fn simple_fit_modes_and_downscaling_have_fixed_expected_pixels() {
    let mut p = Pixels::empty(2, 1).unwrap();
    p.argb(&[0xffff0000, 0xff0000ff]);
    let small = p.rescale(Pixels::default(), 1, 1, 0).ok().unwrap();
    assert_eq!(small.data, [0xff7f007f]);
    let contain = p.rescale(Pixels::default(), 2, 3, 2).ok().unwrap();
    assert_eq!(contain.data, [0, 0, 0xffff0000, 0xff0000ff, 0, 0]);
    let fill = p.rescale(Pixels::default(), 2, 3, 0).ok().unwrap();
    assert_eq!(
        fill.data,
        [0xffff0000, 0xff0000ff, 0xffff0000, 0xff0000ff, 0xffff0000, 0xff0000ff]
    );
}
#[test]
#[cfg_attr(miri, ignore = "large area accumulator regression; exercised natively")]
fn very_large_downscales_do_not_overflow_color_accumulators() {
    let p = solid(4096, 4113, 0xffffffff);
    let scaled = p.rescale(Pixels::default(), 1, 1, 0).ok().unwrap();
    assert_eq!(scaled.data, [0xffffffff]);
}
