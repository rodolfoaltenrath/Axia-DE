//! Native scene ownership and styles. The renderer still borrows the C-layout
//! views synchronously; image decoding, text shaping and painting remain in C.
//!
//! FFI contract: UI thread only, no reentrant calls. Node pointers are null or
//! live views returned by node_new/node_get. C cannot free or relink views, text,
//! style extensions or transforms. Style snapshots never own their ext pointer.
mod damage;
mod edges;
mod layout;
#[cfg(test)]
mod layout_tests;
mod memory;
mod scene;
mod state;
mod style;
#[cfg(test)]
mod tests;
mod transform;
mod types;

use scene::Scene;
use std::{
    cell::RefCell,
    ffi::{c_char, c_void, CStr},
    ptr,
};
use types::*;
thread_local! { static SCENE: RefCell<Scene> = RefCell::new(Scene::default()); }
thread_local! { static LAYOUT: RefCell<layout::Layout> = RefCell::new(layout::Layout::default()); }

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn layout_root(root: *mut Node, w: f32, h: f32) {
    with_scene(|s, ui| {
        if let Some(root) = id(s, root) {
            LAYOUT.with(|l| {
                l.borrow_mut()
                    .run(s, ui, root, w, h, &mut layout::NativeMetrics);
            });
        }
    });
}

#[cfg(not(test))]
unsafe fn release_image(image: *mut c_void) {
    extern "C" {
        fn image_release(image: *mut c_void);
    }
    image_release(image);
}
#[cfg(test)]
unsafe fn release_image(_image: *mut c_void) {
    tests::released_image();
}

unsafe fn with_scene<T>(f: impl FnOnce(&mut Scene, &mut Ui) -> T) -> T {
    SCENE.with(|s| f(&mut s.borrow_mut(), &mut *ptr::addr_of_mut!(state::ui)))
}
unsafe fn id(scene: &Scene, node: *const Node) -> Option<usize> {
    let n = node.as_ref()?;
    let i = usize::try_from(n.id).ok()?;
    (i != 0 && scene.ptr(i).cast_const() == node).then_some(i)
}
#[no_mangle]
pub unsafe extern "C" fn style_defaults(s: *mut Style) {
    if !s.is_null() {
        // The C caller may pass an uninitialized stack record.
        ptr::write(s, Style::default());
    }
}
#[no_mangle]
pub unsafe extern "C" fn style_release(s: *mut Style) {
    if let Some(s) = s.as_mut() {
        s.release();
    }
}
unsafe fn key<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        None
    } else {
        CStr::from_ptr(p).to_str().ok()
    }
}
#[no_mangle]
pub unsafe extern "C" fn style_set_number(s: *mut Style, k: *const c_char, value: f64) -> i32 {
    s.as_mut()
        .zip(key(k))
        .map_or(-1, |(s, k)| if s.set_number(k, value) { 0 } else { -1 })
}
#[no_mangle]
pub unsafe extern "C" fn style_set_string(
    s: *mut Style,
    k: *const c_char,
    value: *const c_char,
) -> i32 {
    if value.is_null() {
        return -1;
    }
    s.as_mut().zip(key(k)).map_or(-1, |(s, k)| {
        if s.set_string(k, CStr::from_ptr(value)) {
            0
        } else {
            -1
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn style_set_gradient(s: *mut Style, angle: f64, from: f64, to: f64) -> i32 {
    s.as_mut().map_or(-1, |s| {
        if s.set_gradient(angle, from, to) {
            0
        } else {
            -1
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn style_reset(s: *mut Style, k: *const c_char) -> i32 {
    s.as_mut()
        .zip(key(k))
        .map_or(-1, |(s, k)| if s.reset(k) { 0 } else { -1 })
}
#[no_mangle]
pub unsafe extern "C" fn node_new(kind: i32) -> *mut Node {
    with_scene(|s, ui| {
        let id = s.create(kind, ui);
        s.ptr(id)
    })
}
#[no_mangle]
pub extern "C" fn node_get(i: i32) -> *mut Node {
    SCENE.with(|s| {
        if i > 0 {
            s.borrow().ptr(i as usize)
        } else {
            ptr::null_mut()
        }
    })
}
#[no_mangle]
pub extern "C" fn node_count() -> i32 {
    SCENE.with(|s| s.borrow().count() as i32)
}
#[no_mangle]
pub unsafe extern "C" fn node_destroy(n: *mut Node) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            if let Some(notify) = state::node_destroy_hook {
                s.visit_destroying(Some(i), |id| notify(id));
            }
            s.destroy(i, ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_append(p: *mut Node, n: *mut Node) {
    node_insert_before(p, n, ptr::null_mut());
}
#[no_mangle]
pub unsafe extern "C" fn node_insert_before(p: *mut Node, n: *mut Node, before: *mut Node) {
    with_scene(|s, ui| {
        if let Some((p, n)) = id(s, p).zip(id(s, n)) {
            s.insert(p, n, id(s, before).unwrap_or(0), ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_detach(n: *mut Node) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            s.detach(i, ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_set_text(n: *mut Node, text: *const c_char) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            s.set_text(
                i,
                if text.is_null() {
                    c""
                } else {
                    CStr::from_ptr(text)
                },
                ui,
            );
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_invalidate(n: *mut Node) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            s.invalidate(i, ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_moved(n: *mut Node) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            s.moved(i, ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn node_restyled(
    n: *mut Node,
    before: *const Style,
    ext_before: *const StyleExt,
) {
    if let Some(b) = before.as_ref() {
        let snapshot = (!ext_before.is_null()).then(|| StyleExt::copy_values(ext_before));
        with_scene(|s, ui| {
            if let Some(i) = id(s, n) {
                s.restyled(i, b, snapshot.as_ref(), ui);
            }
        });
    }
}
#[no_mangle]
pub unsafe extern "C" fn node_ensure_transform(n: *mut Node) -> *mut NodeTf {
    with_scene(|s, _| {
        let Some(i) = id(s, n) else {
            return ptr::null_mut();
        };
        let n = s.node_mut(i).unwrap();
        if n.tf.is_null() {
            let Some(transform) = memory::try_box(NodeTf::default()) else {
                return ptr::null_mut();
            };
            n.tf = Box::into_raw(transform);
        }
        n.tf
    })
}
#[no_mangle]
pub unsafe extern "C" fn node_gather_text(n: *const Node, out: *mut c_char, size: i32) -> i32 {
    if out.is_null() || size <= 0 {
        return 0;
    }
    SCENE.with(|s| {
        let s = s.borrow();
        let Some(i) = id(&s, n) else {
            *out = 0;
            return 0;
        };
        s.gather_text(i, std::slice::from_raw_parts_mut(out.cast(), size as usize)) as i32
    })
}
#[no_mangle]
pub unsafe extern "C" fn node_children_ordered(
    n: *mut Node,
    out: *mut *mut Node,
    size: i32,
) -> i32 {
    if out.is_null() || size <= 0 {
        return 0;
    }
    SCENE.with(|s| {
        let s = s.borrow();
        id(&s, n).map_or(0, |i| {
            s.children_ordered(i, std::slice::from_raw_parts_mut(out.cast(), size as usize)) as i32
        })
    })
}
#[no_mangle]
pub unsafe extern "C" fn damage_subtree(n: *mut Node) {
    with_scene(|s, ui| {
        if let Some(i) = id(s, n) {
            s.damage_subtree(i, ui);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn damage_add(b: Rect) {
    (*ptr::addr_of_mut!(state::ui)).add_damage(b);
}
#[no_mangle]
pub unsafe extern "C" fn dlist_add(l: *mut DamageList, b: Rect) {
    if let Some(l) = l.as_mut() {
        l.add(b);
    }
}
#[no_mangle]
pub extern "C" fn box_area(b: Rect) -> f32 {
    b.area()
}
#[no_mangle]
pub unsafe extern "C" fn ui_reset() {
    with_scene(|s, ui| {
        if let Some(notify) = state::node_destroy_hook {
            s.visit_destroying(None, |id| notify(id));
        }
        s.reset(ui);
        LAYOUT.with(|l| *l.borrow_mut() = layout::Layout::default());
    });
}
