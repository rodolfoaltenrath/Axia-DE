use super::{
    edges::{self, Edges},
    layout::{Layout, Metrics},
    memory::AllocationBudget,
    scene::Scene,
    types::*,
};
use std::{
    ffi::{c_void, CStr},
    ptr,
};

#[derive(Default)]
struct TestMetrics {
    calls: usize,
    generation: u32,
    unit: f32,
}
impl Metrics for TestMetrics {
    fn generation(&mut self) -> u32 {
        self.generation
    }
    fn text(&mut self, text: &CStr, _: &Style, width: f32) -> (f32, f32) {
        self.calls += 1;
        let len = text.to_bytes().len() as f32 * self.unit.max(1.);
        if width > 0. {
            (len.min(width), (len / width).ceil().max(1.))
        } else {
            (len, 1.)
        }
    }
    fn image(&mut self, _: *mut c_void) -> (i32, i32) {
        (80, 40)
    }
}
fn node(scene: &mut Scene, ui: &mut Ui, kind: i32, parent: usize) -> usize {
    let id = scene.create(kind, ui);
    assert_ne!(id, 0);
    if parent != 0 {
        assert!(scene.insert(parent, id, 0, ui));
    }
    id
}
fn px(v: f32) -> Dim {
    Dim { v, unit: 1 }
}
fn pct(v: f32) -> Dim {
    Dim { v, unit: 2 }
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.0002, "{a} != {b}");
}
fn run(
    layout: &mut Layout,
    scene: &mut Scene,
    ui: &mut Ui,
    root: usize,
    w: f32,
    h: f32,
    m: &mut TestMetrics,
) {
    assert!(layout.run(scene, ui, root, w, h, m));
}
#[test]
fn flex_growth_shrink_justify_and_wrapping_have_fixed_geometry() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let a = node(&mut s, &mut ui, 0, root);
    let b = node(&mut s, &mut ui, 0, root);
    s.node_mut(root).unwrap().s.row = 1;
    for (id, grow) in [(a, 1.), (b, 3.)] {
        let n = s.node_mut(id).unwrap();
        n.s.width = px(20.);
        n.s.height = px(10.);
        n.s.grow = grow;
    }
    run(&mut l, &mut s, &mut ui, root, 120., 40., &mut m);
    close(s.node(a).unwrap().w, 40.);
    close(s.node(b).unwrap().w, 80.);
    close(s.node(b).unwrap().x, 40.);
    for id in [a, b] {
        let n = s.node_mut(id).unwrap();
        n.s.grow = 0.;
        n.s.shrink = 1.;
        n.s.width = px(80.);
    }
    s.invalidate(root, &mut ui);
    run(&mut l, &mut s, &mut ui, root, 120., 40., &mut m);
    close(s.node(a).unwrap().w, 60.);
    close(s.node(b).unwrap().w, 60.);
    s.node_mut(root).unwrap().s.wrap = 1;
    s.node_mut(root).unwrap().s.gap = 4.;
    s.invalidate(root, &mut ui);
    run(&mut l, &mut s, &mut ui, root, 120., 40., &mut m);
    close(s.node(b).unwrap().x, 0.);
    close(s.node(b).unwrap().y, 14.);
    s.node_mut(root).unwrap().s.wrap = 0;
    s.node_mut(root).unwrap().s.justify = 1;
    for id in [a, b] {
        let n = s.node_mut(id).unwrap();
        n.s.width = px(20.);
        n.s.shrink = 0.;
    }
    s.invalidate(root, &mut ui);
    run(&mut l, &mut s, &mut ui, root, 120., 40., &mut m);
    close(s.node(a).unwrap().x, 38.);
    close(s.node(b).unwrap().x, 62.);
}
#[test]
fn logical_edges_and_absolute_percent_insets_follow_direction() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let child = node(&mut s, &mut ui, 0, root);
    let n = s.node_mut(root).unwrap();
    n.s.border_width = 2.;
    n.s.padding = [3.; 4];
    n.s.row = 1;
    let n = s.node_mut(child).unwrap();
    n.s.absolute = 1;
    n.s.width = pct(50.);
    n.s.height = px(10.);
    assert!(n.s.set_number("start", 12.));
    assert!(n.s.set_number("marginStart", 5.));
    run(&mut l, &mut s, &mut ui, root, 104., 44., &mut m);
    close(s.node(child).unwrap().x, 19.);
    close(s.node(child).unwrap().w, 50.);
    ui.rtl = 1;
    run(&mut l, &mut s, &mut ui, root, 104., 44., &mut m);
    close(s.node(child).unwrap().x, 40.); // right inset wins, physical left margin is zero
    let n = s.node_mut(child).unwrap();
    n.s.border_width = 1.;
    n.s.radius = 2.;
    for (key, value) in [
        ("borderLeftWidth", 3.),
        ("borderStartWidth", 7.),
        ("borderTopStartRadius", 9.),
    ] {
        assert!(n.s.set_number(key, value));
    }
    edges::resolve(n);
    let x = unsafe { &*n.s.ext };
    assert_eq!(x.r.bw, [3., 1., 7., 1.]);
    assert_eq!(x.r.rad, [2., 9., 2., 2.]);
    assert_eq!(x.r.plain, 0);
    assert_eq!(Edges::of(n).margin, [0., 0., 5., 0.]);
}
#[test]
fn text_cache_reuses_work_and_font_generation_invalidates_all_roots() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let mut roots = Vec::new();
    for _ in 0..2 {
        let root = node(&mut s, &mut ui, 0, 0);
        let text = node(&mut s, &mut ui, 1, root);
        let raw = node(&mut s, &mut ui, 3, text);
        s.set_text(raw, c"abcd", &mut ui);
        roots.push((root, text));
    }
    for &(root, _) in &roots {
        run(&mut l, &mut s, &mut ui, root, 100., 30., &mut m);
    }
    let calls = m.calls;
    for _ in 0..5 {
        for &(root, _) in &roots {
            run(&mut l, &mut s, &mut ui, root, 100., 30., &mut m);
        }
    }
    assert_eq!(calls, m.calls);
    m.generation = 1;
    m.unit = 3.;
    for &(root, _) in &roots {
        run(&mut l, &mut s, &mut ui, root, 100., 30., &mut m);
    }
    assert_eq!(m.calls, calls + 2);
    for &(_, text) in &roots {
        close(s.node(text).unwrap().measure_w, 12.);
    }
}
#[test]
fn growing_rows_remeasure_wrapped_text_after_the_sixteenth_child() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let row = node(&mut s, &mut ui, 0, root);
    s.node_mut(row).unwrap().s.row = 1;
    s.node_mut(row).unwrap().s.width = px(240.);
    for _ in 0..24 {
        let item = node(&mut s, &mut ui, 0, row);
        let text = node(&mut s, &mut ui, 1, item);
        let raw = node(&mut s, &mut ui, 3, text);
        s.set_text(raw, c"0123456789012345678901234567890123456789", &mut ui);
        let n = s.node_mut(item).unwrap();
        n.s.grow = 1.;
        n.s.basis = px(0.);
    }
    run(&mut l, &mut s, &mut ui, root, 240., 100., &mut m);
    close(s.node(row).unwrap().h, 4.);
    let mut id = s.first(row);
    while id != 0 {
        close(s.node(id).unwrap().w, 10.);
        close(s.node(id).unwrap().h, 4.);
        id = s.next(id);
    }
}
#[test]
fn every_sibling_is_laid_out_past_the_old_fixed_limit() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let total = if cfg!(miri) { 40 } else { 1400 };
    for i in 0..total {
        let id = node(&mut s, &mut ui, 0, root);
        let n = s.node_mut(id).unwrap();
        n.s.width = px(10.);
        n.s.height = px(3.);
        n.term = i;
    }
    run(&mut l, &mut s, &mut ui, root, 100., 9000., &mut m);
    let mut id = s.first(root);
    while id != 0 {
        let n = s.node(id).unwrap();
        close(n.y, n.term as f32 * 3.);
        close(n.h, 3.);
        id = s.next(id);
    }
}
#[test]
fn deep_layout_measurement_and_placement_use_explicit_stacks() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let depth = if cfg!(miri) { 40 } else { 8192 };
    let ids: Vec<_> = (0..depth)
        .map(|_| {
            let id = node(&mut s, &mut ui, 0, 0);
            let n = s.node_mut(id).unwrap();
            n.s.width = px(10.);
            n.s.height = px(10.);
            id
        })
        .collect();
    for i in (0..depth - 1).rev() {
        assert!(s.insert(ids[i], ids[i + 1], 0, &mut ui));
    }
    run(&mut l, &mut s, &mut ui, ids[0], 10., 10., &mut m);
    close(s.node(*ids.last().unwrap()).unwrap().w, 10.);
    assert!(
        l.scratch_bytes() < 4096,
        "fixed-size chains do not need depth-sized work buffers"
    );
    // Also exercise the intrinsic measurement stack, with every inner width/height auto.
    // Limit this quadratic workload; the much deeper fixed-size case above tests traversal.
    let intrinsic = if cfg!(miri) { 20 } else { 200 };
    for &id in &ids[..intrinsic] {
        let n = s.node_mut(id).unwrap();
        n.s.width = Dim::default();
        n.s.height = Dim::default();
    }
    s.invalidate(ids[intrinsic - 1], &mut ui);
    run(&mut l, &mut s, &mut ui, ids[0], 10., 10., &mut m);
    assert!(s.node(ids[intrinsic - 1]).unwrap().w.is_finite());
}
#[test]
fn allocation_failure_keeps_geometry_and_warm_layout_needs_no_new_reservations() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    s.node_mut(root).unwrap().w = 71.;
    {
        let _fail = AllocationBudget::new(0);
        assert!(!l.run(&mut s, &mut ui, root, 100., 100., &mut m));
        close(s.node(root).unwrap().w, 71.);
    }
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    {
        let _fail = AllocationBudget::new(0);
        for _ in 0..5 {
            run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
        }
    }
    let child = node(&mut s, &mut ui, 0, root);
    s.node_mut(child).unwrap().s.width = px(10.);
    s.node_mut(child).unwrap().s.height = px(3.);
    {
        let _fail = AllocationBudget::new(0);
        assert!(!l.run(&mut s, &mut ui, root, 200., 100., &mut m));
        close(s.node(root).unwrap().w, 100.);
    }
    run(&mut l, &mut s, &mut ui, root, 200., 100., &mut m);
    close(s.node(child).unwrap().h, 3.);
}
#[test]
fn intrinsic_images_keep_aspect_ratio_inside_padding_and_borders() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let image = node(&mut s, &mut ui, 2, root);
    let n = s.node_mut(image).unwrap();
    n.img = ptr::dangling_mut(); // opaque test image handled only by TestMetrics
    n.s.padding = [2.; 4];
    n.s.border_width = 1.;
    n.s.width = px(40.);
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    close(s.node(image).unwrap().w, 40.);
    close(s.node(image).unwrap().h, 23.);
    let n = s.node_mut(image).unwrap();
    n.s.width = Dim::default();
    n.s.height = px(26.);
    n.s.align_self = 2;
    s.invalidate(image, &mut ui);
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    close(s.node(image).unwrap().w, 46.);
    close(s.node(image).unwrap().h, 26.);
}
#[test]
fn affine_roundtrip_composition_bounds_and_singular_output_are_stable() {
    let a = Affine {
        a: 2.,
        d: 3.,
        e: 7.,
        f: -2.,
        ..Affine::default()
    };
    let b = Affine {
        a: 1.,
        c: 0.5,
        d: 1.,
        e: 9.,
        ..Affine::default()
    };
    for i in -20..20 {
        let (x, y) = (i as f32, i as f32 / 3.);
        let p = b.point(x, y);
        let p = a.point(p.0, p.1);
        let q = a.compose(b).point(x, y);
        close(p.0, q.0);
        close(p.1, q.1);
        let p = a.inverse().unwrap().point(a.point(x, y).0, a.point(x, y).1);
        close(p.0, x);
        close(p.1, y);
    }
    assert!(Affine::default().inverse().is_none());
    let mut uninit = std::mem::MaybeUninit::<Affine>::uninit();
    assert_eq!(
        unsafe { super::transform::affine_invert(a, uninit.as_mut_ptr()) },
        1
    );
    close(unsafe { uninit.assume_init() }.a, 0.5);
    assert_eq!(
        a.bounds(Rect {
            x0: 0.,
            y0: 0.,
            x1: 10.,
            y1: 20.
        }),
        Rect {
            x0: 7.,
            y0: -2.,
            x1: 27.,
            y1: 58.
        }
    );
}
#[test]
fn placement_composes_ancestors_and_handles_zero_scale_or_allocation_failure() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let child = node(&mut s, &mut ui, 0, root);
    for id in [root, child] {
        let n = s.node_mut(id).unwrap();
        n.s.width = px(20.);
        n.s.height = px(20.);
    }
    let n = s.node_mut(root).unwrap();
    n.s.set_number("scaleX", 2.);
    n.s.translate_x = 10.;
    run(&mut l, &mut s, &mut ui, root, 20., 20., &mut m);
    assert_eq!(s.node(child).unwrap().has_tf, 1);
    let parent = unsafe { (*s.node(root).unwrap().tf).world };
    let inherited = unsafe { (*s.node(child).unwrap().tf).world };
    close(parent.point(3., 4.).0, inherited.point(3., 4.).0);
    s.node_mut(root).unwrap().s.set_number("scaleX", 0.);
    run(&mut l, &mut s, &mut ui, root, 20., 20., &mut m);
    let inv = unsafe { (*s.node(child).unwrap().tf).inv };
    close(inv.e, -1e9);
    // A fresh descendant must gracefully fall back when allocating its transform fails.
    let fresh = node(&mut s, &mut ui, 0, child);
    run(&mut l, &mut s, &mut ui, root, 20., 20., &mut m); // reserve scratch before injecting OOM
    unsafe {
        drop(Box::from_raw(s.node(fresh).unwrap().tf));
    }
    s.node_mut(fresh).unwrap().tf = ptr::null_mut();
    let _fail = AllocationBudget::new(0);
    run(&mut l, &mut s, &mut ui, root, 20., 20., &mut m);
    assert_eq!(s.node(fresh).unwrap().has_tf, 0);
    assert!(s.node(fresh).unwrap().tf.is_null());
}
