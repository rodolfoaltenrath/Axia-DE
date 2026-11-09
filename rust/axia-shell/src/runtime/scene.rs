//! Owning scene graph. Links are slot IDs, never dereferenced C pointers.
//! Boxes keep renderer addresses stable when the slot/table vectors grow.
use super::types::{Node, Rect, Style, StyleExt, Ui};
use std::cell::UnsafeCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::ptr;

#[derive(Clone, Copy, Default)]
struct Links {
    parent: usize,
    first: usize,
    last: usize,
    next: usize,
    prev: usize,
}
struct OwnedNode {
    // C temporarily borrows this view to compute layout and paint. These borrows
    // end before calling back into Scene; access is confined to the UI thread.
    view: UnsafeCell<Node>,
    links: Links,
}
impl Drop for OwnedNode {
    fn drop(&mut self) {
        let n = self.view.get_mut();
        unsafe {
            // Image references are still owned by the C image cache. Everything
            // else here is allocated and released by Rust, including rare fields.
            if !n.img.is_null() {
                super::release_image(n.img);
            }
            if !n.text.is_null() {
                drop(CString::from_raw(n.text));
            }
            if !n.tf.is_null() {
                drop(Box::from_raw(n.tf));
            }
            n.s.release();
        }
    }
}
#[derive(Default)]
pub struct Scene {
    slots: Vec<Option<Box<OwnedNode>>>,
    views: Vec<*mut Node>,
    vacant: BinaryHeap<Reverse<usize>>,
    count: usize,
}
impl Scene {
    pub fn ptr(&self, id: usize) -> *mut Node {
        self.views.get(id).copied().unwrap_or(ptr::null_mut())
    }
    pub fn node(&self, id: usize) -> Option<&Node> {
        self.slots
            .get(id)?
            .as_ref()
            .map(|n| unsafe { &*n.view.get() })
    }
    pub fn node_mut(&mut self, id: usize) -> Option<&mut Node> {
        self.slots.get_mut(id)?.as_mut().map(|n| n.view.get_mut())
    }
    fn links(&self, id: usize) -> Links {
        self.slots[id].as_ref().unwrap().links
    }
    fn links_mut(&mut self, id: usize) -> &mut Links {
        &mut self.slots[id].as_mut().unwrap().links
    }
    pub fn count(&self) -> usize {
        self.count
    }
    fn sync(&mut self, id: usize) {
        if id == 0 {
            return;
        }
        let l = self.links(id);
        let links = [l.parent, l.first, l.last, l.next, l.prev].map(|i| self.ptr(i));
        let n = self.node_mut(id).unwrap();
        [n.parent, n.first, n.last, n.next, n.prev] = links;
    }
    fn publish(&mut self, ui: &mut Ui) {
        ui.nodes = if self.views.is_empty() {
            ptr::null_mut()
        } else {
            self.views.as_mut_ptr()
        };
        ui.cap = self.views.len() as i32;
    }
    pub fn create(&mut self, kind: i32, ui: &mut Ui) -> usize {
        if !(0..=6).contains(&kind) {
            return 0;
        }
        if self.slots.is_empty() {
            self.slots.push(None);
        }
        let id = self
            .vacant
            .pop()
            .map(|Reverse(i)| i)
            .unwrap_or(self.slots.len());
        if id >= i32::MAX as usize {
            return 0;
        }
        if id >= self.views.len() {
            let capacity = self.views.len().saturating_mul(2).max(1024);
            if capacity > i32::MAX as usize {
                return 0;
            }
            self.views.resize(capacity, ptr::null_mut());
        }
        let owned = Box::new(OwnedNode {
            view: UnsafeCell::new(Node::new(id as i32, kind)),
            links: Links::default(),
        });
        self.views[id] = owned.view.get();
        if id == self.slots.len() {
            self.slots.push(Some(owned));
        } else {
            self.slots[id] = Some(owned);
        }
        self.count += 1;
        self.publish(ui);
        ui.dirty = 1;
        id
    }
    // Zero marks the end; slot zero is never allocated.
    fn next_in_subtree(&self, root: usize, mut id: usize, descend: bool) -> usize {
        if descend && self.links(id).first != 0 {
            return self.links(id).first;
        }
        while id != root {
            let l = self.links(id);
            if l.next != 0 {
                return l.next;
            }
            id = l.parent;
        }
        0
    }
    pub fn damage_subtree(&mut self, root: usize, ui: &mut Ui) {
        let mut id = root;
        while id != 0 {
            let next = self.next_in_subtree(root, id, true);
            let n = self.node_mut(id).unwrap();
            ui.add_damage(n.prev_box);
            n.prev_box = Rect::default();
            id = next;
        }
    }
    fn invalidate_measure(&mut self, mut id: usize, ui: &mut Ui) {
        while id != 0 {
            let n = self.node_mut(id).unwrap();
            n.measure_valid = 0;
            n.layout_ok = 0;
            id = self.links(id).parent;
        }
        ui.dirty = 1;
    }
    pub fn moved(&mut self, id: usize, ui: &mut Ui) {
        // The parent places this node again; its own cached layout stays valid.
        let mut id = self.links(id).parent;
        while id != 0 {
            self.node_mut(id).unwrap().layout_ok = 0;
            id = self.links(id).parent;
        }
        ui.dirty = 1;
    }
    pub fn invalidate(&mut self, id: usize, ui: &mut Ui) {
        let n = self.node_mut(id).unwrap();
        n.paint_dirty = 1;
        n.gen = n.gen.wrapping_add(1);
        if !n.s.ext.is_null() {
            unsafe {
                (*n.s.ext).r.ok = 0;
            }
        }
        if n.kind == 3 {
            let parent = self.links(id).parent;
            if parent != 0 {
                self.node_mut(parent).unwrap().paint_dirty = 1;
            }
        }
        self.invalidate_measure(id, ui);
    }
    pub fn detach(&mut self, id: usize, ui: &mut Ui) {
        let l = self.links(id);
        if l.parent == 0 {
            return;
        }
        self.damage_subtree(id, ui);
        if l.prev != 0 {
            self.links_mut(l.prev).next = l.next;
        } else {
            self.links_mut(l.parent).first = l.next;
        }
        if l.next != 0 {
            self.links_mut(l.next).prev = l.prev;
        } else {
            self.links_mut(l.parent).last = l.prev;
        }
        let c = self.links_mut(id);
        c.parent = 0;
        c.next = 0;
        c.prev = 0;
        for i in [id, l.parent, l.prev, l.next] {
            self.sync(i);
        }
        self.invalidate_measure(l.parent, ui);
    }
    pub fn insert(&mut self, parent: usize, child: usize, before: usize, ui: &mut Ui) -> bool {
        if self.node(parent).is_none() || self.node(child).is_none() {
            return false;
        }
        let mut ancestor = parent;
        while ancestor != 0 {
            if ancestor == child {
                return false;
            }
            ancestor = self.links(ancestor).parent;
        }
        let before = if self.node(before).is_some() && self.links(before).parent == parent {
            before
        } else {
            0
        };
        if before == child {
            return true;
        }
        self.detach(child, ui);
        let previous = if before != 0 {
            self.links(before).prev
        } else {
            self.links(parent).last
        };
        *self.links_mut(child) = Links {
            parent,
            prev: previous,
            next: before,
            ..self.links(child)
        };
        if previous != 0 {
            self.links_mut(previous).next = child;
        } else {
            self.links_mut(parent).first = child;
        }
        if before != 0 {
            self.links_mut(before).prev = child;
        } else {
            self.links_mut(parent).last = child;
        }
        for id in [parent, child, previous, before] {
            self.sync(id);
        }
        self.invalidate_measure(parent, ui);
        true
    }
    pub fn destroy(&mut self, root: usize, ui: &mut Ui) {
        self.detach(root, ui);
        let mut ids = Vec::new();
        let mut id = root;
        while id != 0 {
            ids.push(id);
            id = self.next_in_subtree(root, id, true);
        }
        // Iterative postorder: no stack growth with deeply nested UI trees.
        for id in ids.into_iter().rev() {
            if ui.root == self.ptr(id) {
                ui.root = ptr::null_mut();
            }
            self.views[id] = ptr::null_mut();
            self.slots[id] = None;
            self.vacant.push(Reverse(id));
            self.count -= 1;
        }
        ui.dirty = 1;
    }
    pub fn reset(&mut self, ui: &mut Ui) {
        self.slots.clear();
        self.slots.shrink_to_fit();
        self.views.clear();
        self.views.shrink_to_fit();
        self.vacant.clear();
        self.vacant.shrink_to_fit();
        self.count = 0;
        self.publish(ui);
        ui.root = ptr::null_mut();
        ui.damage = Rect {
            x0: 0.,
            y0: 0.,
            x1: ui.width,
            y1: ui.height,
        };
        ui.dirty = 1;
    }
    pub fn set_text(&mut self, id: usize, text: &CStr, ui: &mut Ui) {
        let n = self.node_mut(id).unwrap();
        if !n.text.is_null() && unsafe { CStr::from_ptr(n.text) } == text {
            return;
        }
        let next = text.to_owned().into_raw();
        if !n.text.is_null() {
            unsafe {
                drop(CString::from_raw(n.text));
            }
        }
        n.text = next;
        self.invalidate(id, ui);
    }
    pub fn gather_text(&self, root: usize, out: &mut [MaybeUninit<u8>]) -> usize {
        if out.is_empty() {
            return 0;
        }
        let mut used = 0;
        let mut id = self.links(root).first;
        while id != 0 {
            let n = self.node(id).unwrap();
            if n.kind == 3 && !n.text.is_null() {
                let text = unsafe { CStr::from_ptr(n.text) }.to_bytes();
                let len = text.len().min(out.len() - 1 - used);
                for (dst, &byte) in out[used..used + len].iter_mut().zip(&text[..len]) {
                    dst.write(byte);
                }
                used += len;
            }
            id = self.next_in_subtree(root, id, n.kind == 1);
        }
        out[used].write(0);
        used
    }
    pub fn children_ordered(&self, parent: usize, out: &mut [MaybeUninit<*mut Node>]) -> usize {
        let mut count = 0;
        let mut id = self.links(parent).first;
        while id != 0 && count < out.len() {
            out[count].write(self.ptr(id));
            count += 1;
            id = self.links(id).next;
        }
        // Stable and allocation-free for the small sibling lists used by paint.
        for i in 1..count {
            // Only the prefix written above is read, even when C passed an
            // uninitialized stack array as the destination.
            let value = unsafe { out[i].assume_init() };
            let z = unsafe { (*value).s.z_index };
            let mut j = i;
            while j > 0 && unsafe { (*out[j - 1].assume_init()).s.z_index } > z {
                out[j] = out[j - 1];
                j -= 1;
            }
            out[j].write(value);
        }
        count
    }
    fn pure_layout(&self, root: usize) -> bool {
        let mut id = root;
        let mut budget = 256;
        while id != 0 {
            let n = self.node(id).unwrap();
            if n.kind == 1 || n.kind == 2 || budget == 0 {
                return false;
            }
            budget -= 1;
            id = self.next_in_subtree(root, id, true);
        }
        true
    }
    pub fn restyled(
        &mut self,
        id: usize,
        before: &Style,
        ext_before: Option<&StyleExt>,
        ui: &mut Ui,
    ) {
        let n = self.node(id).unwrap();
        let after = n.s;
        let ext = unsafe { after.ext.as_ref() };
        let ext_changed = ext.is_some_and(|x| ext_before.is_none_or(|b| !x.same_values(b, false)));
        if before.same_base(&after) && !ext_changed {
            return;
        }
        let mut moved = *before;
        moved.pos = after.pos;
        if !ext_changed && after.absolute != 0 && before.absolute != 0 && moved.same_base(&after) {
            self.moved(id, ui);
            return;
        }
        let mut parent = self.links(id).parent;
        let mut text_ancestor = false;
        while parent != 0 {
            if self.node(parent).unwrap().kind == 1 {
                text_ancestor = true;
                break;
            }
            parent = self.links(parent).parent;
        }
        if n.kind != 3
            && n.kind != 1
            && before.same_base(&after)
            && !text_ancestor
            && ext
                .zip(ext_before)
                .is_some_and(|(x, b)| x.same_values(b, true))
            && self.pure_layout(id)
        {
            let n = self.node_mut(id).unwrap();
            n.paint_dirty = 1;
            unsafe {
                (*n.s.ext).r.ok = 0;
            }
            self.moved(id, ui);
        } else {
            self.invalidate(id, ui);
        }
    }
}
