//! C ABI of window snapping (server/src/ui/snap_ffi.h). UI thread only. Zones cross as their index
//! in `Zone::ALL`; -1 is "none". Callers keep ownership of every buffer they pass.
use super::*;
use std::ffi::{c_char, CStr};

pub const SNAP_NONE: i32 = -1;
pub const SNAP_RESTORE: i32 = 100;
pub const SNAP_MINIMIZE: i32 = 101;

const C_NAMES: [&[u8]; 14] = [
    b"left\0",
    b"right\0",
    b"top\0",
    b"bottom\0",
    b"top-left\0",
    b"top-right\0",
    b"bottom-left\0",
    b"bottom-right\0",
    b"max\0",
    b"left-third\0",
    b"center-third\0",
    b"right-third\0",
    b"left-two-thirds\0",
    b"right-two-thirds\0",
];

fn index(z: Option<Zone>) -> i32 {
    z.map_or(SNAP_NONE, Zone::index)
}

#[no_mangle]
pub extern "C" fn axia_snap_zone_count() -> i32 {
    Zone::ALL.len() as i32
}

/// The zone's name, a static NUL-terminated string; NULL for an unknown index.
#[no_mangle]
pub extern "C" fn axia_snap_zone_name(zone: i32) -> *const c_char {
    Zone::from_index(zone).map_or(std::ptr::null(), |z| C_NAMES[z as usize].as_ptr().cast())
}

/// The index of a zone's name, or -1.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_zone_index(name: *const c_char) -> i32 {
    if name.is_null() {
        return SNAP_NONE;
    }
    index(CStr::from_ptr(name).to_str().ok().and_then(Zone::from_name))
}

/// 1 and the zone's rectangle inside `area`; 0 for an unknown zone or an invalid area.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_rect(zone: i32, area: SnapRect, out: *mut SnapRect) -> i32 {
    match (Zone::from_index(zone), out.as_mut()) {
        (Some(z), Some(out)) if area.is_valid() => {
            *out = zone_rect(z, area);
            1
        }
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn axia_snap_zone_at(x: f64, y: f64, work: SnapRect, thirds: bool) -> i32 {
    index(zone_at(x, y, work, thirds))
}

/// Super+arrows. `at` a zone or -1 (free); `dir` 0 left, 1 right, 2 up, 3 down. Returns a zone,
/// SNAP_RESTORE, SNAP_MINIMIZE or -1 (nothing to do).
#[no_mangle]
pub extern "C" fn axia_snap_next(at: i32, dir: i32) -> i32 {
    let dir = match dir {
        0 => Dir::Left,
        1 => Dir::Right,
        2 => Dir::Up,
        3 => Dir::Down,
        _ => return SNAP_NONE,
    };
    match snap_next(Zone::from_index(at), dir) {
        Some(Next::Zone(z)) => z.index(),
        Some(Next::Restore) => SNAP_RESTORE,
        Some(Next::Minimize) => SNAP_MINIMIZE,
        None => SNAP_NONE,
    }
}

unsafe fn zones(list: *const i32, n: usize) -> Vec<Zone> {
    if list.is_null() || n == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(list, n.min(64)).iter().filter_map(|&i| Zone::from_index(i)).collect()
}

/// Snap Assist: the free zones of `zone`'s layout into `out` (at most `max`); returns how many.
/// `layout` (may be empty) is the layout picked, `occupied` the zones other windows hold. Unknown
/// indices are ignored.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_assist_zones(
    zone: i32,
    layout: *const i32,
    nlayout: usize,
    occupied: *const i32,
    n: usize,
    out: *mut i32,
    max: usize,
) -> usize {
    let Some(zone) = Zone::from_index(zone) else { return 0 };
    if out.is_null() {
        return 0;
    }
    let free = assist_zones(zone, &zones(layout, nlayout), &zones(occupied, n));
    let out = std::slice::from_raw_parts_mut(out, max);
    let k = free.len().min(max);
    for (o, z) in out.iter_mut().zip(&free[..k]) {
        *o = z.index();
    }
    k
}

#[no_mangle]
pub unsafe extern "C" fn axia_snap_drag_new(displays: *const SnapDisplay, n: usize) -> *mut DragZones {
    let list = if displays.is_null() || n == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(displays, n.min(drag::MAX_DISPLAYS))
    };
    Box::into_raw(Box::new(DragZones::new(list)))
}

#[no_mangle]
pub unsafe extern "C" fn axia_snap_drag_free(drag: *mut DragZones) {
    if !drag.is_null() {
        drop(Box::from_raw(drag));
    }
}

/// The pointer moved or Shift changed: 1 when the zone under it is a different one.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_drag_update(drag: *mut DragZones, x: f64, y: f64, thirds: bool) -> i32 {
    drag.as_mut().map_or(0, |d| d.update(x, y, thirds) as i32)
}

/// The zone the window would snap to now (and its rectangle into `rect`, if given), or -1.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_drag_zone(drag: *const DragZones, rect: *mut SnapRect) -> i32 {
    match drag.as_ref().and_then(DragZones::current) {
        Some((z, r)) => {
            if let Some(out) = rect.as_mut() {
                *out = r;
            }
            z.index()
        }
        None => SNAP_NONE,
    }
}

/// A linked resize, or NULL when no dragged edge of members[0] is shared with another member.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_resize_new(
    edges: i32,
    members: *const SnapMember,
    n: usize,
    area: SnapRect,
) -> *mut LinkedResize {
    if members.is_null() || n < 2 {
        return std::ptr::null_mut();
    }
    let list = std::slice::from_raw_parts(members, n.min(resize::MAX_MEMBERS));
    LinkedResize::new(edges, list, area).map_or(std::ptr::null_mut(), |r| Box::into_raw(Box::new(r)))
}

#[no_mangle]
pub unsafe extern "C" fn axia_snap_resize_free(link: *mut LinkedResize) {
    if !link.is_null() {
        drop(Box::from_raw(link));
    }
}

/// 1 if member `i` changes with this drag.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_resize_moves(link: *const LinkedResize, i: usize) -> i32 {
    link.as_ref().map_or(0, |l| (i < l.len() && l.moves(i)) as i32)
}

/// Every member's frame with the pointer moved (dx, dy) since the start, into `out` (at most `max`);
/// returns how many were written.
#[no_mangle]
pub unsafe extern "C" fn axia_snap_resize_apply(
    link: *const LinkedResize,
    dx: f64,
    dy: f64,
    out: *mut SnapRect,
    max: usize,
) -> usize {
    let Some(l) = link.as_ref() else { return 0 };
    if out.is_null() {
        return 0;
    }
    let frames = l.apply(dx, dy);
    let out = std::slice::from_raw_parts_mut(out, max);
    let k = frames.len().min(max);
    out[..k].copy_from_slice(&frames[..k]);
    k
}
