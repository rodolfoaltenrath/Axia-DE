use super::ffi::*;
use super::*;
use std::ffi::CStr;

const AREAS: [SnapRect; 4] = [
    // a bottom taskbar (48 px) at 1280×800, an odd work area, a second display to the right, a left bar
    SnapRect::new(0.0, 0.0, 1280.0, 752.0),
    SnapRect::new(0.0, 0.0, 1365.0, 719.0),
    SnapRect::new(1280.0, 0.0, 1921.0, 1033.0),
    SnapRect::new(56.0, 0.0, 1224.0, 800.0),
];

fn area(r: &SnapRect) -> f64 {
    r.w * r.h
}
fn inside(r: &SnapRect, a: &SnapRect) -> bool {
    r.x >= a.x && r.y >= a.y && r.right() <= a.right() && r.bottom() <= a.bottom()
}
fn whole(r: &SnapRect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.fract() == 0.0)
}

#[test]
fn layouts_fill_the_area_without_gaps_or_overlaps() {
    use Zone::*;
    let layouts: [&[Zone]; 6] = [
        &[Left, Right],
        &[Top, Bottom],
        &[TopLeft, TopRight, BottomLeft, BottomRight],
        &[LeftThird, CenterThird, RightThird],
        &[LeftTwoThirds, RightThird],
        &[LeftThird, RightTwoThirds],
    ];
    for a in &AREAS {
        for l in layouts {
            let rs: Vec<SnapRect> = l.iter().map(|z| zone_rect(*z, *a)).collect();
            assert_eq!(rs.iter().map(area).sum::<f64>(), area(a), "{l:?} fills {a:?}");
            for i in 0..rs.len() {
                assert!(inside(&rs[i], a), "{:?} inside {a:?}", l[i]);
                for j in i + 1..rs.len() {
                    assert!(!rs[i].overlaps(&rs[j]), "{:?} and {:?} overlap in {a:?}", l[i], l[j]);
                }
            }
        }
        assert_eq!(zone_rect(Max, *a), *a);
        for z in Zone::ALL {
            assert!(whole(&zone_rect(z, *a)), "{z:?} whole pixels in {a:?}");
        }
    }
}

#[test]
fn halves_round_like_the_javascript_layouts() {
    // 1365 / 2 = 682.5 → 683, as Math.round did
    assert_eq!(zone_rect(Zone::Left, AREAS[1]).w, 683.0);
    assert_eq!(zone_rect(Zone::Right, AREAS[1]), SnapRect::new(683.0, 0.0, 682.0, 719.0));
    let t = zone_rect(Zone::Top, AREAS[0]);
    let b = zone_rect(Zone::Bottom, AREAS[0]);
    assert_eq!(t.bottom(), b.y);
    assert_eq!(t.h + b.h, AREAS[0].h);
}

#[test]
fn drop_zones_at_the_edges() {
    let a = AREAS[0];
    let at = |x, y| zone_at(x, y, a, false);
    assert_eq!(at(0.0, 400.0), Some(Zone::Left));
    assert_eq!(at(EDGE, 400.0), Some(Zone::Left));
    assert_eq!(at(EDGE + 1.0, 400.0), None);
    assert_eq!(at(1279.0, 400.0), Some(Zone::Right));
    assert_eq!(at(2.0, 40.0), Some(Zone::TopLeft));
    assert_eq!(at(1279.0, 740.0), Some(Zone::BottomRight));
    assert_eq!(at(0.0, 780.0), Some(Zone::BottomLeft), "over a bottom taskbar");
    assert_eq!(at(640.0, 0.0), Some(Zone::Max));
    assert_eq!(at(640.0, 400.0), None);
    assert_eq!(zone_at(20.0, 400.0, AREAS[3], false), Some(Zone::Left), "a left taskbar's strip");
    assert_eq!(zone_at(1281.0, 500.0, AREAS[2], false), Some(Zone::Left), "second display");
    assert_eq!(zone_at(f64::NAN, 0.0, a, false), None);
}

#[test]
fn shift_gives_the_thirds() {
    let a = AREAS[0];
    assert_eq!(zone_at(0.0, 400.0, a, true), Some(Zone::LeftThird));
    assert_eq!(zone_at(0.0, 20.0, a, true), Some(Zone::LeftThird), "no quarters with Shift");
    assert_eq!(zone_at(1279.0, 700.0, a, true), Some(Zone::RightThird));
    assert_eq!(zone_at(640.0, 0.0, a, true), Some(Zone::CenterThird));
    assert_eq!(zone_at(640.0, 400.0, a, true), None);
}

#[test]
fn super_arrows() {
    use Dir::*;
    use Next::{Minimize, Restore};
    let z = |z| Some(Next::Zone(z));
    assert_eq!(snap_next(None, Left), z(Zone::Left));
    assert_eq!(snap_next(None, Up), z(Zone::Max));
    assert_eq!(snap_next(None, Down), Some(Minimize));
    assert_eq!(snap_next(Some(Zone::Left), Up), z(Zone::TopLeft));
    assert_eq!(snap_next(Some(Zone::Left), Right), Some(Restore));
    assert_eq!(snap_next(Some(Zone::Left), Left), None);
    assert_eq!(snap_next(Some(Zone::TopLeft), Right), z(Zone::TopRight));
    assert_eq!(snap_next(Some(Zone::TopRight), Up), z(Zone::Max));
    assert_eq!(snap_next(Some(Zone::BottomRight), Down), Some(Minimize));
    assert_eq!(snap_next(Some(Zone::Max), Down), Some(Restore));
    assert_eq!(snap_next(Some(Zone::Max), Up), None);
    assert_eq!(snap_next(Some(Zone::CenterThird), Left), z(Zone::Left));
    assert_eq!(snap_next(Some(Zone::RightTwoThirds), Down), Some(Restore));
    assert_eq!(snap_next(Some(Zone::Top), Left), None);
}

#[test]
fn the_zone_follows_the_pointer_across_displays() {
    let displays = [
        SnapDisplay { bounds: SnapRect::new(0.0, 0.0, 1280.0, 800.0), work: AREAS[0] },
        SnapDisplay { bounds: SnapRect::new(1280.0, 0.0, 1921.0, 1080.0), work: AREAS[2] },
    ];
    let mut d = DragZones::new(&displays);
    assert!(!d.update(600.0, 400.0, false), "no zone, no change");
    assert!(d.update(1281.0, 500.0, false));
    assert_eq!(d.current(), Some((Zone::Left, zone_rect(Zone::Left, AREAS[2]))));
    assert!(!d.update(1282.0, 510.0, false), "the same zone");
    assert!(d.update(1282.0, 510.0, true), "Shift changes it in place");
    assert_eq!(d.current().map(|c| c.0), Some(Zone::LeftThird));
    assert!(d.update(-30.0, 400.0, false), "past the first display: the nearest one");
    assert_eq!(d.current().map(|c| c.0), Some(Zone::Left));
    assert!(d.update(640.0, 400.0, false));
    assert_eq!(d.current(), None);
    assert!(DragZones::new(&[]).current().is_none());
}

fn member(x: f64, y: f64, w: f64, h: f64) -> SnapMember {
    SnapMember { frame: SnapRect::new(x, y, w, h), min_w: 160.0, min_h: 100.0, max_w: 0.0, max_h: 0.0 }
}
const RIGHT: i32 = 8;
const LEFT: i32 = 4;
const BOTTOM: i32 = 2;

#[test]
fn halves_share_their_middle_line() {
    let a = AREAS[0];
    let ms = [member(0.0, 0.0, 640.0, 752.0), member(640.0, 0.0, 640.0, 752.0)];
    let l = LinkedResize::new(RIGHT, &ms, a).expect("linked");
    assert!(l.moves(1));
    let f = l.apply(100.0, 37.0);
    assert_eq!(f[0], SnapRect::new(0.0, 0.0, 740.0, 752.0));
    assert_eq!(f[1], SnapRect::new(740.0, 0.0, 540.0, 752.0));
    // the other window keeps its minimum width, and so does the dragged one
    assert_eq!(l.apply(5000.0, 0.0)[1].w, 160.0);
    assert_eq!(l.apply(-5000.0, 0.0)[0].w, 160.0);
    // dragging the right window's left edge is the same line
    let ms = [ms[1], ms[0]];
    let f = LinkedResize::new(LEFT, &ms, a).expect("linked").apply(-40.0, 0.0);
    assert_eq!((f[0].x, f[0].w, f[1].w), (600.0, 680.0, 600.0));
}

#[test]
fn a_line_moves_every_window_along_it() {
    let a = AREAS[0];
    let ms = [
        member(0.0, 0.0, 640.0, 376.0),
        member(640.0, 0.0, 640.0, 376.0),
        member(0.0, 376.0, 640.0, 376.0),
        member(640.0, 376.0, 640.0, 376.0),
    ];
    let l = LinkedResize::new(RIGHT | BOTTOM, &ms, a).expect("linked");
    assert!((0..4).all(|i| l.moves(i)));
    let f = l.apply(-40.0, 24.0);
    assert_eq!(f[0], SnapRect::new(0.0, 0.0, 600.0, 400.0));
    assert_eq!(f[1], SnapRect::new(600.0, 0.0, 680.0, 400.0));
    assert_eq!(f[2], SnapRect::new(0.0, 400.0, 600.0, 352.0));
    assert_eq!(f[3], SnapRect::new(600.0, 400.0, 680.0, 352.0));
}

#[test]
fn an_edge_on_the_border_resizes_only_the_window() {
    let a = AREAS[0];
    let ms = [member(0.0, 0.0, 640.0, 752.0), member(640.0, 0.0, 640.0, 752.0)];
    assert!(LinkedResize::new(LEFT, &ms, a).is_none(), "nobody behind the left border");
    assert!(LinkedResize::new(BOTTOM, &ms, a).is_none(), "nobody below the work area");
    let l = LinkedResize::new(RIGHT | BOTTOM, &ms, a).expect("the middle line is shared");
    let f = l.apply(10.0, -100.0);
    assert_eq!(f[0], SnapRect::new(0.0, 0.0, 650.0, 652.0));
    assert_eq!(f[1], SnapRect::new(650.0, 0.0, 630.0, 752.0), "only the shared line moves it");
    assert_eq!(l.apply(0.0, 300.0)[0].h, 752.0, "never past the work area");
}

#[test]
fn windows_off_the_line_stay() {
    let a = AREAS[0];
    let ms = [member(0.0, 0.0, 640.0, 752.0), member(640.0, 0.0, 640.0, 752.0), member(900.0, 100.0, 300.0, 200.0)];
    let l = LinkedResize::new(RIGHT, &ms, a).expect("linked");
    assert!(!l.moves(2));
    assert_eq!(l.apply(50.0, 0.0)[2], ms[2].frame);
    assert!(LinkedResize::new(RIGHT, &ms[..1], a).is_none(), "alone");
}

#[test]
fn assist_offers_the_rest_of_the_layout() {
    use Zone::*;
    assert_eq!(assist_zones(Left, &[], &[]), vec![Right]);
    assert_eq!(assist_zones(BottomRight, &[], &[]), vec![TopLeft, TopRight, BottomLeft]);
    assert_eq!(assist_zones(TopLeft, &[], &[Right]), vec![BottomLeft], "a right half takes both right quarters");
    assert_eq!(assist_zones(LeftThird, &[], &[]), vec![CenterThird, RightThird]);
    assert_eq!(assist_zones(LeftThird, &[], &[CenterThird]), vec![RightThird]);
    assert_eq!(assist_zones(Left, &[], &[Right]), Vec::<Zone>::new());
    assert!(assist_zones(Max, &[], &[]).is_empty());
}

#[test]
fn assist_follows_the_layout_picked() {
    use Zone::*;
    // the flyout's two-thirds layout: its right third is followed by the left two thirds
    assert_eq!(assist_zones(RightThird, &[LeftTwoThirds, RightThird], &[]), vec![LeftTwoThirds]);
    assert_eq!(assist_zones(RightThird, &[], &[]), vec![LeftThird, CenterThird], "without one, the thirds");
    assert_eq!(assist_zones(Left, &[TopLeft, TopRight], &[]), vec![Right], "a layout without the zone is ignored");
    assert_eq!(assist_zones(Left, &[Left, Right, Right], &[]), vec![Right]);
}

#[test]
fn assist_miniatures_fit_their_zone() {
    let cell = zone_rect(Zone::Right, AREAS[0]);
    for n in [1usize, 2, 3, 5, 12, 20] {
        let frames: Vec<SnapRect> = (0..n).map(|i| SnapRect::new(0.0, 0.0, 600.0 + 40.0 * i as f64, 400.0)).collect();
        let ps = assist_grid(cell, &frames);
        assert_eq!(ps.len(), n.min(ASSIST_MAX));
        for (i, p) in ps.iter().enumerate() {
            let r = SnapRect::new(p.x, p.y - 30.0, p.w, p.h + 30.0);
            assert!(inside(&r, &cell), "{n} candidates: {i} with its title inside {cell:?}: {r:?}");
            assert!(p.scale > 0.0 && p.scale <= 0.85);
            for q in &ps[i + 1..] {
                assert!(!SnapRect::new(p.x, p.y, p.w, p.h).overlaps(&SnapRect::new(q.x, q.y, q.w, q.h)));
            }
        }
    }
    assert!(assist_grid(cell, &[]).is_empty());
}

#[test]
fn c_abi() {
    unsafe {
        assert_eq!(axia_snap_zone_count(), 14);
        for z in Zone::ALL {
            let name = CStr::from_ptr(axia_snap_zone_name(z.index())).to_str().unwrap();
            assert_eq!(Zone::from_name(name), Some(z));
            assert_eq!(axia_snap_zone_index(axia_snap_zone_name(z.index())), z.index());
        }
        assert!(axia_snap_zone_name(14).is_null() && axia_snap_zone_name(-1).is_null());
        assert_eq!(axia_snap_zone_index(b"sideways\0".as_ptr().cast()), SNAP_NONE);
        assert_eq!(axia_snap_zone_index(std::ptr::null()), SNAP_NONE);
        let mut r = SnapRect::default();
        assert_eq!(axia_snap_rect(Zone::Right.index(), AREAS[0], &mut r), 1);
        assert_eq!(r, SnapRect::new(640.0, 0.0, 640.0, 752.0));
        assert_eq!(axia_snap_rect(99, AREAS[0], &mut r), 0);
        assert_eq!(axia_snap_rect(0, AREAS[0], std::ptr::null_mut()), 0);
        assert_eq!(axia_snap_next(SNAP_NONE, 3), SNAP_MINIMIZE);
        assert_eq!(axia_snap_next(Zone::Max.index(), 3), SNAP_RESTORE);
        assert_eq!(axia_snap_next(Zone::Left.index(), 0), SNAP_NONE);
        assert_eq!(axia_snap_next(SNAP_NONE, 9), SNAP_NONE);
        let mut out = [0i32; 4];
        let held = [Zone::Right.index(), 77];
        let none = std::ptr::null();
        assert_eq!(axia_snap_assist_zones(Zone::TopLeft.index(), none, 0, held.as_ptr(), 2, out.as_mut_ptr(), 4), 1);
        assert_eq!(out[0], Zone::BottomLeft.index());
        assert_eq!(axia_snap_assist_zones(Zone::BottomRight.index(), none, 0, none, 0, out.as_mut_ptr(), 2), 2);
        let layout = [Zone::LeftTwoThirds.index(), Zone::RightThird.index()];
        assert_eq!(axia_snap_assist_zones(Zone::RightThird.index(), layout.as_ptr(), 2, none, 0, out.as_mut_ptr(), 4), 1);
        assert_eq!(out[0], Zone::LeftTwoThirds.index());
        let d = axia_snap_drag_new(std::ptr::null(), 0);
        assert_eq!(axia_snap_drag_update(d, 0.0, 0.0, false), 0);
        assert_eq!(axia_snap_drag_zone(d, std::ptr::null_mut()), SNAP_NONE);
        axia_snap_drag_free(d);
        axia_snap_drag_free(std::ptr::null_mut());
        let ms = [member(0.0, 0.0, 640.0, 752.0), member(640.0, 0.0, 640.0, 752.0)];
        let l = axia_snap_resize_new(RIGHT, ms.as_ptr(), 2, AREAS[0]);
        assert!(!l.is_null());
        assert_eq!(axia_snap_resize_moves(l, 1), 1);
        assert_eq!(axia_snap_resize_moves(l, 5), 0);
        let mut f = [SnapRect::default(); 2];
        assert_eq!(axia_snap_resize_apply(l, 20.0, 0.0, f.as_mut_ptr(), 2), 2);
        assert_eq!(f[1].x, 660.0);
        axia_snap_resize_free(l);
        assert!(axia_snap_resize_new(LEFT, ms.as_ptr(), 2, AREAS[0]).is_null());
    }
}
