//! Safe scene access for trees built in Rust (native views). UI thread only, like
//! the rest of the runtime: these are the operations the QuickJS bridge performs
//! for React (`lord.createNode`, `setStyle`, `setText`, `setProp`), without JS.
use super::{types::*, with_scene};
use std::ffi::{CStr, CString};
pub const VIEW: i32 = 0;
pub const TEXT: i32 = 1;
pub const IMAGE: i32 = 2;
const RAW: i32 = 3;
/// A converted style value, as `runtime/style.js` sends it to `lord.setStyle`.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Num(f64),
    Str(String),
    /// `linear-gradient`: angle and the two ARGB stops.
    Gradient(f64, f64, f64),
}
fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}
/// The scene still exists: thread-local storage is destroyed in no fixed order at
/// exit, and a view dropped then must not touch it.
pub fn alive() -> bool {
    super::SCENE.try_with(|_| ()).is_ok()
}
/// A new detached node; 0 when the scene is out of memory.
pub fn create(kind: i32) -> i32 {
    unsafe { with_scene(|s, ui| s.create(kind, ui) as i32) }
}
pub fn exists(id: i32) -> bool {
    id > 0 && unsafe { with_scene(|s, _| s.node(id as usize).is_some()) }
}
/// Destroys the node and its subtree (their images are released by the scene).
pub fn destroy(id: i32) {
    if exists(id) {
        unsafe { super::node_destroy(super::node_get(id)) }
    }
}
/// Inserts `child` into `parent` before `before` (0: at the end).
pub fn insert(parent: i32, child: i32, before: i32) -> bool {
    if parent <= 0 || child <= 0 {
        return false;
    }
    unsafe {
        with_scene(|s, ui| {
            let before = if before > 0 && s.parent(before as usize) == parent as usize {
                before as usize
            } else {
                0
            };
            s.insert(parent as usize, child as usize, before, ui)
        })
    }
}
/// The string of a TEXT node. As React's text instances, it lives in a RAW child:
/// layout, painting and `gather_text` read those.
pub fn set_label(id: i32, text: &str) {
    let raw = unsafe {
        with_scene(|s, _| {
            let first = s.first(id as usize);
            (first != 0 && s.node(first).is_some_and(|n| n.kind == RAW)).then_some(first as i32)
        })
    };
    let raw = match raw {
        Some(raw) => raw,
        None => {
            let raw = create(RAW);
            if raw == 0 || !insert(id, raw, 0) {
                destroy(raw);
                return;
            }
            raw
        }
    };
    set_text(raw, text);
}
fn set_text(id: i32, text: &str) {
    let text = cstring(text);
    unsafe {
        with_scene(|s, ui| {
            if s.node(id as usize).is_some() {
                s.set_text(id as usize, &text, ui)
            }
        })
    }
}
/// Applies changed properties (`None` resets one) and repaints or relayouts as needed.
pub fn set_style(id: i32, props: &[(&str, Option<&Prop>)]) {
    if props.is_empty() || !exists(id) {
        return;
    }
    unsafe {
        with_scene(|s, ui| {
            let Some(n) = s.node_mut(id as usize) else {
                return;
            };
            let before = n.s;
            let ext_before = (!n.s.ext.is_null()).then(|| StyleExt::copy_values(n.s.ext));
            for (key, value) in props {
                let _ = match value {
                    None if *key == "gradient" => {
                        n.s.has_gradient = 0;
                        true
                    }
                    None => n.s.reset(key),
                    Some(Prop::Num(v)) => n.s.set_number(key, *v),
                    Some(Prop::Str(v)) => n.s.set_string(key, &cstring(v)),
                    Some(Prop::Gradient(a, from, to)) => n.s.set_gradient(*a, *from, *to),
                };
            }
            s.restyled(id as usize, &before, ext_before.as_ref(), ui);
        })
    }
}
/// The node's box after the last layout: absolute x, y, width and height.
pub fn frame(id: i32) -> Option<(f32, f32, f32, f32)> {
    unsafe { with_scene(|s, _| s.node(id as usize).map(|n| (n.ax, n.ay, n.w, n.h))) }
}
pub fn parent(id: i32) -> i32 {
    if !exists(id) {
        return 0;
    }
    unsafe { with_scene(|s, _| s.parent(id as usize) as i32) }
}
pub fn next_sibling(id: i32) -> i32 {
    if !exists(id) {
        return 0;
    }
    unsafe { with_scene(|s, _| s.next(id as usize) as i32) }
}
/// Is `id` the node `ancestor` or inside it?
pub fn within(mut id: i32, ancestor: i32) -> bool {
    for _ in 0..4096 {
        if id <= 0 {
            return false;
        }
        if id == ancestor {
            return true;
        }
        id = parent(id);
    }
    false
}
/// An image node shows a file (as `setProp(id, 'src', path)`).
#[cfg(not(test))]
pub fn set_image_path(id: i32, path: &str) {
    let path = cstring(path);
    unsafe {
        let n = super::node_get(id);
        if n.is_null() {
            return;
        }
        super::image::image_release((*n).img.cast());
        (*n).img = super::image::image_get(path.as_ptr()).cast();
        super::node_invalidate(n);
    }
}
#[cfg(test)]
pub fn set_image_path(_id: i32, _path: &str) {}
/// An image node shows RGBA pixels (as `setProp(id, 'pixels', …)`), reusing its image.
pub fn set_image_pixels(id: i32, width: usize, height: usize, rgba: &[u8]) {
    if width == 0 || height == 0 || width > 4096 || height > 4096 || rgba.len() < width * height * 4
    {
        return;
    }
    unsafe {
        let n = super::node_get(id);
        if n.is_null() || (*n).kind != IMAGE {
            return;
        }
        let image =
            super::image::image_pixels((*n).img.cast(), width as i32, height as i32, rgba.as_ptr());
        if !image.is_null() {
            (*n).img = image.cast();
        }
        super::node_invalidate(n);
    }
}
/// The strings in a subtree, one per line in tree order (tests and accessibility).
/// Unlike `node_gather_text`, which joins one TEXT node's runs, this descends views.
pub fn gather_text(id: i32) -> String {
    if !exists(id) {
        return String::new();
    }
    unsafe {
        with_scene(|s, _| {
            let root = id as usize;
            let mut lines = Vec::new();
            let mut n = s.first(root);
            while n != 0 && lines.len() < 65536 {
                let node = s.node(n).unwrap();
                if node.kind == RAW && !node.text.is_null() {
                    lines.push(CStr::from_ptr(node.text).to_string_lossy().into_owned());
                }
                n = s.next_in_subtree(root, n, true);
            }
            lines.join("\n")
        })
    }
}
