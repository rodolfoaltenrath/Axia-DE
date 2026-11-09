use super::{scene::Scene, types::*};
use std::{cell::Cell, mem::MaybeUninit, ptr};
thread_local! { static RELEASED_IMAGES: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn released_image() {
    RELEASED_IMAGES.with(|n| n.set(n.get() + 1));
}
fn create(s: &mut Scene, ui: &mut Ui, kind: i32) -> usize {
    s.create(kind, ui)
}
fn child_ids(s: &Scene, id: usize) -> Vec<i32> {
    let mut p = s.node(id).unwrap().first;
    let mut out = Vec::new();
    while !p.is_null() {
        assert!(out.len() <= s.count(), "cycle in renderer links");
        unsafe {
            out.push((*p).id);
            p = (*p).next;
        }
    }
    out
}
#[test]
fn growth_preserves_addresses_reuses_lowest_vacancy_and_releases_tables() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let a = create(&mut s, &mut ui, 0);
    let first = s.ptr(a);
    let extra = if cfg!(miri) { 64 } else { 3000 };
    for _ in 0..extra {
        create(&mut s, &mut ui, 0);
    }
    assert_eq!(s.count(), extra + 1);
    assert_eq!(s.ptr(a), first);
    assert!(ui.cap as usize >= extra + 2);
    assert_eq!(unsafe { *ui.nodes.add(a) }, first);
    s.destroy(9, &mut ui);
    s.destroy(3, &mut ui);
    assert_eq!(create(&mut s, &mut ui, 0), 3);
    assert_eq!(create(&mut s, &mut ui, 0), 9);
    assert_eq!(s.count(), extra + 1);
    assert_eq!(create(&mut s, &mut ui, 7), 0);
    s.reset(&mut ui);
    assert_eq!(s.count(), 0);
    assert_eq!(ui.cap, 0);
    assert!(ui.nodes.is_null());
    assert_eq!(create(&mut s, &mut ui, 0), 1);
}
#[test]
fn reorder_reparent_detach_and_cycle_rejection_keep_both_views_consistent() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let ids: Vec<_> = (0..5).map(|_| create(&mut s, &mut ui, 0)).collect();
    let (root, a, b, c, other) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    for id in [a, b, c] {
        assert!(s.insert(root, id, 0, &mut ui));
    }
    assert_eq!(child_ids(&s, root), vec![a as i32, b as i32, c as i32]);
    assert!(s.insert(root, c, a, &mut ui));
    assert_eq!(child_ids(&s, root), vec![c as i32, a as i32, b as i32]);
    assert!(s.insert(root, a, a, &mut ui)); // inserting before itself must be a no-op
    assert_eq!(child_ids(&s, root), vec![c as i32, a as i32, b as i32]);
    assert!(!s.insert(a, root, 0, &mut ui));
    assert!(!s.insert(a, a, 0, &mut ui));
    assert!(s.insert(other, a, 0, &mut ui));
    assert_eq!(child_ids(&s, root), vec![c as i32, b as i32]);
    assert_eq!(child_ids(&s, other), vec![a as i32]);
    s.detach(c, &mut ui);
    assert_eq!(child_ids(&s, root), vec![b as i32]);
    let n = s.node(c).unwrap();
    assert!(n.parent.is_null() && n.next.is_null() && n.prev.is_null());
    unsafe {
        assert_eq!((*s.ptr(b)).prev, ptr::null_mut());
        assert_eq!((*s.ptr(root)).last, s.ptr(b));
        assert_eq!((*s.ptr(a)).parent, s.ptr(other));
    }
}
#[test]
fn destroying_deep_subtrees_releases_resources_without_recursive_drop() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    ui.root = s.ptr(root);
    let mut parent = root;
    for _ in 0..if cfg!(miri) { 64 } else { 2048 } {
        let n = create(&mut s, &mut ui, 0);
        s.insert(parent, n, 0, &mut ui);
        parent = n;
    }
    let n = s.node_mut(parent).unwrap();
    n.s.set_number("scaleX", 2.);
    n.tf = Box::into_raw(Box::new(NodeTf::default()));
    n.img = ptr::dangling_mut(); // host image-cache reference, counted by the Rust test stub
    s.set_text(parent, c"owned text", &mut ui);
    RELEASED_IMAGES.with(|n| n.set(0));
    s.destroy(root, &mut ui);
    assert_eq!(s.count(), 0);
    assert!(ui.root.is_null());
    RELEASED_IMAGES.with(|n| assert_eq!(n.get(), 1));
    s.reset(&mut ui);
    RELEASED_IMAGES.with(|n| assert_eq!(n.get(), 1));
}

#[test]
fn mixed_tree_edits_preserve_renderer_links_and_single_parent_ownership() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let population = if cfg!(miri) { 16 } else { 64 };
    let rounds = if cfg!(miri) { 80 } else { 1200 };
    for _ in 0..population {
        create(&mut s, &mut ui, 0);
    }
    let mut seed = 0x24b1_784d_u32;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize % population + 1
    };
    for step in 0..rounds {
        let (p, c, b) = (random(), random(), random());
        match step % 11 {
            0 => {
                s.destroy(c, &mut ui);
                while s.count() < population {
                    create(&mut s, &mut ui, 0);
                }
            }
            1 => s.detach(c, &mut ui),
            _ => {
                s.insert(p, c, b, &mut ui);
            }
        }
        let mut attached = std::collections::HashSet::new();
        for id in 1..=population {
            let n = s.node(id).unwrap();
            let (mut child, mut previous) = (n.first, ptr::null_mut());
            while !child.is_null() {
                unsafe {
                    assert!(
                        attached.insert((*child).id),
                        "child appears twice at step {step}"
                    );
                    assert_eq!((*child).parent, s.ptr(id));
                    assert_eq!((*child).prev, previous);
                    previous = child;
                    child = (*child).next;
                }
            }
            assert_eq!(n.last, previous);
            let (mut ancestor, mut depth) = (n.parent, 0);
            while !ancestor.is_null() {
                depth += 1;
                assert!(depth < population, "cycle at step {step}");
                ancestor = unsafe { (*ancestor).parent };
            }
        }
        assert_eq!(
            attached.len(),
            (1..=population)
                .filter(|&id| !s.node(id).unwrap().parent.is_null())
                .count()
        );
        assert_eq!(s.count(), population);
    }
}
#[test]
fn gather_handles_nested_text_ignores_other_views_and_bounds_every_write() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 1);
    let nested = create(&mut s, &mut ui, 1);
    s.insert(root, nested, 0, &mut ui);
    let raw = create(&mut s, &mut ui, 3);
    s.insert(nested, raw, 0, &mut ui);
    s.set_text(raw, c"Olá", &mut ui);
    let hidden = create(&mut s, &mut ui, 0);
    s.insert(root, hidden, 0, &mut ui);
    let ignored = create(&mut s, &mut ui, 3);
    s.insert(hidden, ignored, 0, &mut ui);
    s.set_text(ignored, c"ignored", &mut ui);
    let end = create(&mut s, &mut ui, 3);
    s.insert(root, end, 0, &mut ui);
    s.set_text(end, c"!", &mut ui);
    let mut out = [MaybeUninit::new(42); 16];
    assert_eq!(s.gather_text(root, &mut out[..8]), 5);
    let bytes = out.map(|v| unsafe { v.assume_init() });
    assert_eq!(&bytes[..6], b"Ol\xc3\xa1!\0");
    assert_eq!(&bytes[8..], &[42; 8]);
    assert_eq!(s.gather_text(root, &mut out[..1]), 0);
    assert_eq!(unsafe { out[0].assume_init() }, 0);
    assert_eq!(s.gather_text(root, &mut []), 0);
    assert_eq!(s.gather_text(root, &mut out[..4]), 3);
    assert_eq!(unsafe { out[3].assume_init() }, 0);
    let mut uninitialized = [MaybeUninit::uninit(); 6];
    assert_eq!(s.gather_text(root, &mut uninitialized), 5);
    assert_eq!(
        uninitialized.map(|v| unsafe { v.assume_init() }),
        *b"Ol\xc3\xa1!\0"
    );
}
#[test]
fn unchanged_text_and_styles_preserve_cached_layout_but_real_changes_invalidate() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 1);
    let raw = create(&mut s, &mut ui, 3);
    s.insert(root, raw, 0, &mut ui);
    s.set_text(raw, c"text", &mut ui);
    let generation = s.node(raw).unwrap().gen;
    s.node_mut(root).unwrap().measure_valid = 1;
    ui.dirty = 0;
    s.set_text(raw, c"text", &mut ui);
    assert_eq!(s.node(raw).unwrap().gen, generation);
    assert_eq!(ui.dirty, 0);
    s.set_text(raw, c"changed", &mut ui);
    assert_eq!(s.node(raw).unwrap().gen, generation + 1);
    assert_eq!(s.node(root).unwrap().measure_valid, 0);
    assert_eq!(s.node(root).unwrap().paint_dirty, 1);
    let before = s.node(root).unwrap().s;
    ui.dirty = 0;
    s.restyled(root, &before, None, &mut ui);
    assert_eq!(ui.dirty, 0);
}
#[test]
fn moving_absolute_nodes_and_rotating_pure_subtrees_keep_their_measurements() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let id = create(&mut s, &mut ui, 0);
    let n = s.node_mut(id).unwrap();
    n.s.absolute = 1;
    n.measure_valid = 1;
    n.layout_ok = 1;
    let before = n.s;
    n.s.set_number("left", 42.);
    s.restyled(id, &before, None, &mut ui);
    assert_eq!(s.node(id).unwrap().measure_valid, 1);
    assert_eq!(s.node(id).unwrap().layout_ok, 1);
    let n = s.node_mut(id).unwrap();
    n.s.set_number("rotate", 0.);
    let before = n.s;
    let ext = unsafe { *n.s.ext };
    n.s.set_number("rotate", 90.);
    s.restyled(id, &before, Some(&ext), &mut ui);
    assert_eq!(s.node(id).unwrap().measure_valid, 1);
    assert_eq!(s.node(id).unwrap().paint_dirty, 1);
    assert_eq!(s.node(id).unwrap().layout_ok, 1);
    // Text descendants prevent the transform-only shortcut.
    let text = create(&mut s, &mut ui, 1);
    s.insert(id, text, 0, &mut ui);
    let n = s.node_mut(id).unwrap();
    n.measure_valid = 1;
    let before = n.s;
    let ext = unsafe { *n.s.ext };
    n.s.set_number("rotate", 180.);
    s.restyled(id, &before, Some(&ext), &mut ui);
    assert_eq!(s.node(id).unwrap().measure_valid, 0);
}
#[test]
#[cfg_attr(miri, ignore = "host libc numeric parsing is covered by native tests")]
fn style_values_cover_units_colors_enums_logical_edges_and_reset() {
    let mut s = Style::default();
    assert_eq!(s.opacity, 1.);
    assert_eq!(s.font_weight, 400.);
    assert_eq!(s.color, 0xff000000);
    assert!(s.set_string("width", c"37.5%"));
    assert!(s.width.same(Dim { v: 37.5, unit: 2 }));
    assert!(s.set_string("height", c"auto"));
    assert_eq!(s.height.unit, 0);
    assert!(s.set_number("backgroundColor", 0xff123456_u32 as f64));
    assert_eq!(s.bg, 0xff123456);
    assert!(s.set_string("pointerEvents", c"box-none"));
    assert_eq!(s.pointer_none, 2);
    assert!(!s.set_number("pointerEvents", 1.));
    assert!(!s.set_string("pointerEvents", c"invalid"));
    assert!(!s.set_number("unknown", 1.));
    assert!(s.ext.is_null());
    assert!(s.reset("marginStart"));
    assert!(s.ext.is_null());
    assert!(s.set_number("borderEndWidth", 3.));
    assert!(s.set_number("marginStart", 7.));
    unsafe {
        let x = &*s.ext;
        assert_eq!(x.border_side[5], 3.);
        assert_eq!(x.margin_inline[0], 7.);
        assert_eq!(x.edge_set, (1 << 5) | (1 << 12));
        assert_eq!(x.scale_x, 1.);
        assert!(x.origin_x.same(Dim { v: 50., unit: 2 }));
    }
    assert!(s.reset("borderEndWidth"));
    unsafe {
        assert_eq!((*s.ext).edge_set, 1 << 12);
    }
    assert!(s.set_number("numberOfLines", 999.));
    assert_eq!(s.line_clamp, 255);
    assert!(s.set_string("numberOfLines", c"-12"));
    assert_eq!(s.line_clamp, 0);
    assert!(s.reset("pointerEvents"));
    assert_eq!(s.pointer_none, 0);
    unsafe {
        s.release();
        s.release();
    }
    assert!(s.ext.is_null());
}
#[test]
fn style_comparison_ignores_padding_and_resolved_cache_but_preserves_float_bits() {
    let mut a = Style::default();
    a.set_number("rotate", 0.);
    let mut b = a;
    b.width.v = -0.;
    assert!(!a.same_base(&b));
    b.width.v = 0.;
    assert!(a.same_base(&b));
    let mut x = unsafe { *a.ext };
    let before = x;
    x.r.ok = 1;
    assert!(x.same_values(&before, false));
    x.rotate = 90.;
    assert!(!x.same_values(&before, false));
    assert!(x.same_values(&before, true));
    x.margin_inline[0] = 1.;
    assert!(!x.same_values(&before, true));
    unsafe {
        a.release();
    }
}

#[test]
fn c_style_snapshots_need_only_the_initialized_value_prefix() {
    let source = StyleExt {
        rotate: 27.,
        edge_set: 1 << 12,
        ..StyleExt::default()
    };
    let mut partial = MaybeUninit::<StyleExt>::uninit();
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::addr_of!(source).cast::<MaybeUninit<u8>>(),
            partial.as_mut_ptr().cast::<MaybeUninit<u8>>(),
            std::mem::offset_of!(StyleExt, r),
        );
        let snapshot = StyleExt::copy_values(partial.as_ptr());
        assert!(snapshot.same_values(&source, false));
        assert_eq!(snapshot.r.ok, 0);
        assert_eq!(snapshot.r.mg, [0.; 4]);
    }
}

#[test]
fn renderer_borrows_survive_slot_growth_edits_and_node_deletion() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    let child = create(&mut s, &mut ui, 0);
    let (p, c) = (s.ptr(root), s.ptr(child));
    s.insert(root, child, 0, &mut ui);
    for _ in 0..32 {
        create(&mut s, &mut ui, 0);
    }
    s.node_mut(root).unwrap().w = 42.;
    unsafe {
        assert_eq!((*p).first, c);
        assert_eq!((*p).w, 42.);
        (*c).x = 9.;
    }
    assert_eq!(s.node(child).unwrap().x, 9.);
    s.destroy(child, &mut ui);
    unsafe {
        assert!((*p).first.is_null());
        assert_eq!(*ui.nodes.add(root), p);
        assert!((*ui.nodes.add(child)).is_null());
    }
    assert_eq!(create(&mut s, &mut ui, 0), child);
    unsafe {
        assert_eq!((*(*ui.nodes.add(child))).id, child as i32);
    }
}

#[test]
fn failed_allocations_preserve_nodes_vacancies_text_and_style_ownership() {
    use super::memory::AllocationBudget;
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    let hole = create(&mut s, &mut ui, 0);
    s.set_text(root, c"kept", &mut ui);
    s.destroy(hole, &mut ui);
    ui.dirty = 0;
    let view = s.ptr(root);
    {
        let _failure = AllocationBudget::new(0);
        assert_eq!(create(&mut s, &mut ui, 0), 0);
        s.set_text(root, c"replacement", &mut ui);
        assert_eq!(s.count(), 1);
        assert_eq!(ui.dirty, 0);
        assert!(s.ptr(hole).is_null());
        unsafe {
            assert_eq!(std::ffi::CStr::from_ptr((*view).text), c"kept");
        }
        let mut style = Style::default();
        assert!(!style.set_number("borderStartWidth", 7.));
        assert!(style.ext.is_null());
        assert!(style.set_number("width", 7.)); // a base property needs no allocation
    }
    assert_eq!(create(&mut s, &mut ui, 0), hole);
    s.insert(root, hole, 0, &mut ui);
    {
        let _failure = AllocationBudget::new(0);
        s.destroy(root, &mut ui);
        assert_eq!(s.count(), 0);
        s.reset(&mut ui);
        assert!(ui.nodes.is_null());
    }
}

#[test]
#[cfg_attr(miri, ignore = "host libc numeric parsing is covered by native tests")]
fn invalid_style_values_do_not_allocate_or_change_overrides_and_caches() {
    let mut s = Style::default();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
        assert!(!s.set_number("borderStartWidth", value));
        assert!(s.ext.is_null());
        assert!(!s.set_number("width", value));
    }
    for value in [c"bad", c"nan", c"inf", c"1e999", c""] {
        assert!(!s.set_string("start", value));
        assert!(s.ext.is_null());
        assert!(!s.set_string("fontSize", value));
        assert_eq!(s.font_size, 14.);
    }
    assert!(!s.set_string("borderStartColor", c"unsupported"));
    assert!(s.ext.is_null());
    assert!(!s.set_string("numberOfLines", c"bad"));
    s.set_number("borderWidth", 3.);
    s.set_number("borderStartWidth", 5.);
    unsafe {
        (*s.ext).r.ok = 1;
        let before = *s.ext;
        assert!(!s.set_string("start", c"invalid"));
        assert!((*s.ext).same_values(&before, false));
        assert_eq!((*s.ext).r.ok, 1);
        s.release();
    }
}

#[test]
fn no_op_insertions_preserve_damage_and_layout_caches() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    let a = create(&mut s, &mut ui, 0);
    let b = create(&mut s, &mut ui, 0);
    s.insert(root, a, 0, &mut ui);
    s.insert(root, b, 0, &mut ui);
    ui.dirty = 0;
    s.node_mut(root).unwrap().layout_ok = 1;
    let previous = Rect {
        x0: 1.,
        y0: 2.,
        x1: 3.,
        y1: 4.,
    };
    s.node_mut(a).unwrap().prev_box = previous;
    assert!(s.insert(root, a, b, &mut ui));
    assert!(s.insert(root, b, 0, &mut ui));
    assert_eq!(ui.dirty, 0);
    assert_eq!(s.node(root).unwrap().layout_ok, 1);
    assert_eq!(s.node(a).unwrap().prev_box, previous);
    assert_eq!(ui.dlist.n, 0);
}

#[test]
fn gradient_validation_preserves_the_previous_style_on_invalid_input() {
    let mut s = Style::default();
    assert!(s.set_gradient(90., 0xff123456_u32 as f64, 0xffabcdef_u32 as f64));
    let previous = s;
    for (angle, from, to) in [
        (f64::NAN, 0., 0.),
        (f64::INFINITY, 0., 0.),
        (f64::MAX, 0., 0.),
        (90., f64::NAN, 0.),
        (90., 0., f64::INFINITY),
    ] {
        assert!(!s.set_gradient(angle, from, to));
        assert!(s.same_base(&previous));
    }
    assert_eq!(s.has_gradient, 1);
    assert_eq!(s.grad_angle, 90.);
    assert_eq!(s.grad_from, 0xff123456);
}

#[test]
fn host_ffi_handles_partial_snapshots_uninitialized_outputs_and_allocation_failure() {
    // This is the only runtime unit test using the UI-thread host singleton.
    // All other unit tests use independent local Scene/Ui instances.
    unsafe {
        super::ui_reset();
        let root = super::node_new(0);
        let raw = super::node_new(3);
        assert!(!root.is_null() && !raw.is_null());
        super::node_append(root, raw);
        super::node_set_text(raw, c"ffi".as_ptr());
        let mut text = [MaybeUninit::<u8>::uninit(); 4];
        assert_eq!(
            super::node_gather_text(root, text.as_mut_ptr().cast(), 4),
            3
        );
        assert_eq!(text.map(|b| b.assume_init()), *b"ffi\0");
        let mut children = [MaybeUninit::<*mut Node>::uninit(); 1];
        assert_eq!(
            super::node_children_ordered(root, children.as_mut_ptr().cast(), 1),
            1
        );
        assert_eq!(children[0].assume_init(), raw);
        let mut style = MaybeUninit::<Style>::uninit();
        super::style_defaults(style.as_mut_ptr());
        let mut style = style.assume_init();
        assert_eq!(style.opacity, 1.);
        super::style_release(&mut style);
        super::style_set_number(ptr::addr_of_mut!((*root).s), c"rotate".as_ptr(), 0.);
        let before = (*root).s;
        let mut prefix = MaybeUninit::<StyleExt>::uninit();
        ptr::copy_nonoverlapping(
            before.ext.cast::<MaybeUninit<u8>>(),
            prefix.as_mut_ptr().cast(),
            std::mem::offset_of!(StyleExt, r),
        );
        super::style_set_number(ptr::addr_of_mut!((*root).s), c"rotate".as_ptr(), 30.);
        super::node_restyled(root, &before, prefix.as_ptr());
        {
            let _failure = super::memory::AllocationBudget::new(0);
            assert!(super::node_new(0).is_null());
            assert!(super::node_ensure_transform(root).is_null());
            assert!((*root).tf.is_null());
            assert_eq!(super::node_count(), 2);
            super::node_destroy(root);
            assert_eq!(super::node_count(), 0);
        }
        assert!(super::node_get(0).is_null());
        assert!(super::node_get(-1).is_null());
        super::node_destroy(ptr::null_mut());
        assert_eq!(super::node_gather_text(ptr::null(), ptr::null_mut(), 0), 0);
        super::ui_reset();
    }
}
#[test]
fn child_paint_order_is_stable_and_does_not_change_tree_order() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    let mut ids = Vec::new();
    for z in [2., -1., 2., 0.] {
        let id = create(&mut s, &mut ui, 0);
        s.insert(root, id, 0, &mut ui);
        s.node_mut(id).unwrap().s.z_index = z;
        ids.push(id);
    }
    let mut out = [MaybeUninit::uninit(); 4];
    assert_eq!(s.children_ordered(root, &mut out), 4);
    assert_eq!(
        out.map(|v| unsafe { v.assume_init() }),
        [ids[1], ids[3], ids[0], ids[2]].map(|i| s.ptr(i))
    );
    assert_eq!(
        child_ids(&s, root),
        ids.iter().map(|&i| i as i32).collect::<Vec<_>>()
    );
    assert_eq!(s.children_ordered(root, &mut []), 0);
}
#[test]
fn damage_merges_transitive_overlaps_and_bounds_fragmented_regions() {
    let mut list = DamageList::default();
    let rect = |x0, y0, x1, y1| Rect { x0, y0, x1, y1 };
    list.add(rect(0., 0., 10., 10.));
    list.add(rect(20., 0., 30., 10.));
    list.add(rect(5., 0., 25., 10.));
    assert_eq!(list.n, 1);
    assert_eq!(list.r[0], rect(0., 0., 30., 10.));
    for i in 0..80 {
        let x = i as f32 * 100.;
        list.add(rect(x, 50., x + 10., 60.));
    }
    assert!(list.n <= 16);
    for i in 0..80 {
        let x = i as f32 * 100.;
        assert!(list.r[..list.n as usize]
            .iter()
            .any(|r| r.x0 <= x && r.x1 >= x + 10. && r.y0 <= 50. && r.y1 >= 60.));
    }
    let mut ui = Ui::empty();
    ui.add_damage(rect(1., 2., 3., 4.));
    ui.damage = rect(100., 100., 200., 200.);
    ui.add_damage(rect(300., 100., 400., 200.));
    assert_eq!(ui.dlist.n, 2);
    assert_eq!(ui.damage, rect(100., 100., 400., 200.));
}
#[test]
fn detach_repaints_the_entire_old_subtree_and_keeps_its_nodes_alive() {
    let (mut s, mut ui) = (Scene::default(), Ui::empty());
    let root = create(&mut s, &mut ui, 0);
    let a = create(&mut s, &mut ui, 0);
    let b = create(&mut s, &mut ui, 0);
    s.insert(root, a, 0, &mut ui);
    s.insert(a, b, 0, &mut ui);
    s.node_mut(a).unwrap().prev_box = Rect {
        x0: 10.,
        y0: 10.,
        x1: 20.,
        y1: 20.,
    };
    s.node_mut(b).unwrap().prev_box = Rect {
        x0: 30.,
        y0: 30.,
        x1: 40.,
        y1: 40.,
    };
    s.detach(a, &mut ui);
    assert_eq!(
        ui.damage,
        Rect {
            x0: 10.,
            y0: 10.,
            x1: 40.,
            y1: 40.
        }
    );
    assert_eq!(s.count(), 3);
    assert_eq!(s.node(b).unwrap().prev_box, Rect::default());
}

#[test]
#[ignore = "requires clang to inspect the existing C renderer; see docs/RUST.md"]
fn native_views_match_c_layout() {
    // Compile no new C test code: inspect the real renderer's existing records.
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = std::process::Command::new("clang")
        .current_dir(repo)
        .args([
            "-Xclang",
            "-fdump-record-layouts-complete",
            "-fsyntax-only",
            "-Iserver/src",
            "server/src/ui/layout.c",
        ])
        .output()
        .expect("clang is required for the explicit ABI check");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = String::from_utf8(output.stdout).unwrap();
    fn check<T>(dump: &str, tag: &str, fields: &[(&str, usize)]) {
        let key = format!("| struct {tag}\n");
        let record = dump
            .split_once(&key)
            .unwrap_or_else(|| panic!("missing {tag}"))
            .1
            .split("*** Dumping")
            .next()
            .unwrap();
        let size = record
            .split_once("[sizeof=")
            .unwrap()
            .1
            .split(',')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let align = record
            .split_once("align=")
            .unwrap()
            .1
            .split(']')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(std::mem::size_of::<T>(), size, "{tag} size");
        assert_eq!(std::mem::align_of::<T>(), align, "{tag} alignment");
        let members: std::collections::HashMap<_, _> = record
            .lines()
            .filter_map(|line| {
                let (offset, body) = line.split_once('|')?;
                if !body.starts_with("   ") || body.starts_with("    ") {
                    return None;
                }
                Some((
                    body.split_whitespace().last()?.trim_start_matches('*'),
                    offset.trim().parse::<usize>().ok()?,
                ))
            })
            .collect();
        assert_eq!(members.len(), fields.len(), "{tag}: field added or removed");
        for &(field, offset) in fields {
            let c_field = if field == "kind" { "type" } else { field };
            assert_eq!(members.get(c_field), Some(&offset), "{tag}.{field}");
        }
    }
    macro_rules! layout {
        ($ty:ident, $tag:literal, $($field:ident),+ $(,)?) => {
            check::<$ty>(&dump,$tag,&[$((stringify!($field),std::mem::offset_of!($ty,$field))),+]);
        };
    }
    layout!(Dim, "Dim", v, unit);
    layout!(Affine, "Affine", a, b, c, d, e, f);
    layout!(Rect, "Box", x0, y0, x1, y1);
    layout!(DamageList, "DamageList", r, n);
    layout!(NodeTf, "NodeTf", own, world, inv, sig, live, live_known);
    layout!(
        ResolvedEdges,
        "ResolvedEdges",
        mg,
        pd,
        bw,
        bc,
        rad,
        ok,
        rtl,
        plain
    );
    layout!(
        StyleExt,
        "StyleExt",
        edge_set,
        scale_x,
        scale_y,
        rotate,
        border_side,
        border_side_color,
        margin_inline,
        padding_inline,
        inset_inline,
        radius_corner,
        origin_x,
        origin_y,
        r
    );
    layout!(
        Style,
        "Style",
        display_none,
        absolute,
        row,
        justify,
        align_items,
        align_self,
        overflow_hidden,
        pointer_none,
        text_align,
        object_fit,
        nowrap,
        dir,
        wrap,
        mono,
        cursor,
        grow,
        shrink,
        basis,
        width,
        height,
        min_w,
        min_h,
        max_w,
        max_h,
        pos,
        margin,
        padding,
        gap,
        bg,
        border_color,
        color,
        shadow_color,
        has_gradient,
        text_overflow,
        line_clamp,
        grad_from,
        grad_to,
        grad_angle,
        radius,
        border_width,
        opacity,
        shadow_blur,
        shadow_x,
        shadow_y,
        backdrop_blur,
        font_size,
        line_height,
        font_weight,
        translate_x,
        translate_y,
        z_index,
        ext
    );
    layout!(
        Node,
        "Node",
        id,
        kind,
        s,
        text,
        img,
        term,
        protect,
        rtl,
        own_tf,
        has_tf,
        layout_ok,
        layout_rtl,
        parent,
        first,
        last,
        next,
        prev,
        x,
        y,
        w,
        h,
        ax,
        ay,
        measure_valid,
        measure_maxw,
        measure_w,
        measure_h,
        paint_dirty,
        gen,
        prev_box,
        tf
    );
    layout!(
        Ui,
        "Ui",
        nodes,
        cap,
        root,
        dirty,
        width,
        height,
        theme_dir,
        layout_ms,
        paint_ms,
        damage,
        dlist,
        dlast,
        damage_px,
        quit_requested,
        rtl,
        theme_request,
        theme_failed
    );
}
