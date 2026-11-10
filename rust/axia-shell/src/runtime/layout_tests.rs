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
fn wrapped_lines_break_on_the_clamped_basis_in_measure_and_layout() {
    // `flex: 1` (basis 0) with a minimum width: lines must break where the minimum
    // no longer fits, and the measured height must match the lines laid out.
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let wrapper = node(&mut s, &mut ui, 0, root);
    {
        let w = s.node_mut(wrapper).unwrap();
        w.s.row = 1;
        w.s.wrap = 1;
        w.s.gap = 8.;
    }
    let items: Vec<_> = (0..5)
        .map(|_| {
            let id = node(&mut s, &mut ui, 0, wrapper);
            let n = s.node_mut(id).unwrap();
            n.s.grow = 1.;
            n.s.shrink = 1.;
            n.s.basis = px(0.);
            n.s.min_w = px(96.);
            n.s.height = px(10.);
            id
        })
        .collect();
    run(&mut l, &mut s, &mut ui, root, 260., 400., &mut m);
    // Two items per line (96 + 8 + 96 <= 260 < 304): three lines.
    close(s.node(wrapper).unwrap().h, 3. * 10. + 2. * 8.);
    let at = |s: &Scene, i: usize| {
        let n = s.node(items[i]).unwrap();
        (n.x, n.y, n.w)
    };
    for (i, (x, y, w)) in [
        (0., 0., 126.),
        (134., 0., 126.),
        (0., 18., 126.),
        (134., 18., 126.),
        (0., 36., 260.),
    ]
    .into_iter()
    .enumerate()
    {
        let (ax, ay, aw) = at(&s, i);
        close(ax, x);
        close(ay, y);
        close(aw, w);
    }
}
#[test]
fn main_axis_constraints_apply_after_shrink_and_when_the_line_exactly_fits() {
    for row in [0, 1] {
        let (mut s, mut ui, mut l, mut m) = (
            Scene::default(),
            Ui::empty(),
            Layout::default(),
            TestMetrics::default(),
        );
        let root = node(&mut s, &mut ui, 0, 0);
        let child = node(&mut s, &mut ui, 0, root);
        s.node_mut(root).unwrap().s.row = row;
        s.node_mut(root).unwrap().s.justify = 1;
        let cs = &mut s.node_mut(child).unwrap().s;
        cs.width = px(100.);
        cs.height = px(100.);
        cs.shrink = 1.;
        if row != 0 {
            cs.min_w = px(120.);
        } else {
            cs.min_h = px(120.);
        }
        run(&mut l, &mut s, &mut ui, root, 80., 80., &mut m);
        let n = s.node(child).unwrap();
        close(if row != 0 { n.w } else { n.h }, 120.);
        close(if row != 0 { n.x } else { n.y }, -20.);
        let cs = &mut s.node_mut(child).unwrap().s;
        if row != 0 {
            cs.min_w = Dim::default();
            cs.max_w = px(40.);
        } else {
            cs.min_h = Dim::default();
            cs.max_h = px(40.);
        }
        s.invalidate(child, &mut ui);
        run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
        let n = s.node(child).unwrap();
        close(if row != 0 { n.w } else { n.h }, 40.);
        close(if row != 0 { n.x } else { n.y }, 30.);
    }
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
fn affine_extremes_do_not_publish_invalid_inverses_or_overflow_rotation_angles() {
    let large = Affine {
        a: 1e20,
        d: 1e20,
        e: 1e20,
        f: -1e20,
        ..Affine::default()
    };
    let inverse = large.inverse().unwrap();
    let point = large.point(2., 3.);
    let back = inverse.point(point.0, point.1);
    close(back.0, 2.);
    close(back.1, 3.);
    for bad in [
        Affine {
            a: f32::NAN,
            ..Affine::identity()
        },
        Affine {
            e: f32::INFINITY,
            ..Affine::identity()
        },
        Affine {
            a: 1e-7,
            d: 1.,
            e: f32::MAX,
            ..Affine::default()
        },
        Affine {
            a: 1e20,
            b: 1e20,
            c: 1e20,
            d: 1e20,
            ..Affine::default()
        },
    ] {
        assert!(bad.inverse().is_none());
        let mut out = Affine::identity();
        assert_eq!(unsafe { super::transform::affine_invert(bad, &mut out) }, 0);
        assert_eq!(
            [out.a, out.b, out.c, out.d, out.e, out.f],
            [1., 0., 0., 1., 0., 0.]
        );
    }
    let mut style = Style::default();
    assert!(style.set_number("rotate", f32::MAX as f64));
    let matrix = style.transform(0., 0., 20., 20.);
    assert!(matrix.inverse().is_some());
    let point = matrix.point(2., 3.);
    assert!(point.0.is_finite() && point.1.is_finite());
    // Styles do not own their extensions outside a Scene owner.
    unsafe {
        drop(Box::from_raw(style.ext));
    }
}
#[test]
fn placement_composes_ancestors_and_handles_zero_scale() {
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
}

#[test]
fn enabling_transform_preflights_every_descendant_and_retries_without_partial_geometry() {
    for budget in 0..3 {
        let (mut s, mut ui, mut l, mut m) = (
            Scene::default(),
            Ui::empty(),
            Layout::default(),
            TestMetrics::default(),
        );
        let root = node(&mut s, &mut ui, 0, 0);
        let child = node(&mut s, &mut ui, 0, root);
        let leaf = node(&mut s, &mut ui, 0, child);
        // Allocate the extension before warming the layout, so enabling the
        // transform exercises the animation shortcut rather than invalidation.
        assert!(s.node_mut(root).unwrap().s.set_number("scaleX", 1.));
        run(&mut l, &mut s, &mut ui, root, 20., 20., &mut m);
        let before = s.node(root).unwrap().s;
        let ext = unsafe { *before.ext };
        assert!(s.node_mut(root).unwrap().s.set_number("scaleX", 2.));
        s.restyled(root, &before, Some(&ext), &mut ui);
        assert!(s.layout_bounds.is_none());
        ui.dirty = 0;
        let geometry = [root, child, leaf].map(|id| {
            let n = s.node(id).unwrap();
            (n.w, n.h, n.ax, n.ay, n.has_tf, n.own_tf)
        });
        {
            let _fail = AllocationBudget::new(budget);
            assert!(!l.run(&mut s, &mut ui, root, 40., 40., &mut m));
        }
        assert_eq!(ui.dirty, 1);
        for (&id, previous) in [root, child, leaf].iter().zip(geometry) {
            let n = s.node(id).unwrap();
            assert_eq!((n.w, n.h, n.ax, n.ay, n.has_tf, n.own_tf), previous);
        }
        run(&mut l, &mut s, &mut ui, root, 40., 40., &mut m);
        close(s.node(root).unwrap().w, 40.);
        for id in [root, child, leaf] {
            assert_eq!(s.node(id).unwrap().has_tf, 1);
            assert!(!s.node(id).unwrap().tf.is_null());
        }
        let _fail = AllocationBudget::new(0);
        run(&mut l, &mut s, &mut ui, root, 50., 40., &mut m);
    }
}

#[test]
#[cfg_attr(miri, ignore = "large workspace retention stress; exercised natively")]
fn closing_a_large_temporary_tree_releases_excess_scratch_and_keeps_warm_reuse() {
    let (mut s, mut ui, mut l, mut m) = (
        Scene::default(),
        Ui::empty(),
        Layout::default(),
        TestMetrics::default(),
    );
    let root = node(&mut s, &mut ui, 0, 0);
    let small = node(&mut s, &mut ui, 0, 0);
    let ids: Vec<_> = (0..1400).map(|_| node(&mut s, &mut ui, 0, root)).collect();
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    let large = l.scratch_bytes();
    run(&mut l, &mut s, &mut ui, small, 100., 100., &mut m);
    {
        let _fail = AllocationBudget::new(0);
        run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    }
    for &id in &ids[1..] {
        s.destroy(id, &mut ui);
    }
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
    assert!(l.scratch_bytes() < large / 2);
    let _fail = AllocationBudget::new(0);
    run(&mut l, &mut s, &mut ui, root, 100., 100., &mut m);
}
