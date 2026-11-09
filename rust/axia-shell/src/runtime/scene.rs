//! Owning scene graph. Links are slot IDs, never dereferenced C pointers.
//! A native allocation is released to its owning record before any pointer is
//! published. Moving a slot never reborrows or moves the native node allocation.
use super::memory;
use super::types::{Node, Rect, Style, StyleExt, Ui};
use std::cell::Cell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

#[derive(Clone, Copy, Default)]
struct Links {
    parent: usize,
    first: usize,
    last: usize,
    next: usize,
    prev: usize,
}
struct OwnedNode {
    // Keep the original raw allocation provenance for every renderer borrow.
    // Retaining a Box here would retag its pointee when the slot is moved or
    // mutably borrowed, invalidating pointers previously published to C.
    view: NonNull<Node>,
    links: Links,
}
impl OwnedNode {
    fn new(id: usize, kind: i32) -> Option<Self> {
        let allocation = memory::try_box(Node::new(id as i32, kind))?;
        Some(Self {
            view: NonNull::new(Box::into_raw(allocation)).unwrap(),
            links: Links::default(),
        })
    }
}
impl Drop for OwnedNode {
    fn drop(&mut self) {
        unsafe {
            // Reclaim ownership only after the renderer's borrow has ended.
            let mut n = Box::from_raw(self.view.as_ptr());
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
    pub(super) layout_bounds: Option<(usize, super::layout::Bounds)>,
    slots: Vec<Option<OwnedNode>>,
    // Cell keeps the published pointer table writable without exclusive slice
    // reborrows invalidating the table pointer retained by the C host.
    views: Vec<Cell<*mut Node>>,
    vacant: BinaryHeap<Reverse<usize>>,
    count: usize,
}
impl Scene {
    pub fn ptr(&self, id: usize) -> *mut Node {
        self.views.get(id).map(Cell::get).unwrap_or(ptr::null_mut())
    }
    pub fn node(&self, id: usize) -> Option<&Node> {
        self.slots
            .get(id)?
            .as_ref()
            .map(|n| unsafe { n.view.as_ref() })
    }
    pub fn node_mut(&mut self, id: usize) -> Option<&mut Node> {
        self.slots
            .get_mut(id)?
            .as_mut()
            .map(|n| unsafe { n.view.as_mut() })
    }
    fn links(&self, id: usize) -> Links {
        self.slots[id].as_ref().unwrap().links
    }
    pub fn parent(&self, id: usize) -> usize {
        self.links(id).parent
    }
    pub fn first(&self, id: usize) -> usize {
        self.links(id).first
    }
    pub fn next(&self, id: usize) -> usize {
        self.links(id).next
    }
    pub fn forget_layout(&mut self) {
        for owned in self.slots.iter_mut().flatten() {
            let n = unsafe { owned.view.as_mut() };
            n.layout_ok = 0;
            n.measure_valid = 0;
        }
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
            self.views.as_ptr().cast_mut().cast()
        };
        ui.cap = self.views.len() as i32;
    }
    pub fn create(&mut self, kind: i32, ui: &mut Ui) -> usize {
        if !(0..=6).contains(&kind) {
            return 0;
        }
        let id = self
            .vacant
            .peek()
            .map(|Reverse(i)| *i)
            .unwrap_or(self.slots.len().max(1));
        if id >= i32::MAX as usize {
            return 0;
        }
        let capacity = if id >= self.views.len() {
            self.views.len().saturating_mul(2).max(1024)
        } else {
            self.views.len()
        };
        if capacity > i32::MAX as usize {
            return 0;
        }
        let Some(owned) = OwnedNode::new(id, kind) else {
            return 0;
        };
        let required = self.slots.len().max(id + 1);
        // Reserve all metadata before committing a node. Vacancies can then be
        // recorded during destruction without allocating while freeing memory.
        if self.slots.try_reserve(required - self.slots.len()).is_err()
            || self
                .vacant
                .try_reserve(required.saturating_sub(self.vacant.len()))
                .is_err()
        {
            return 0;
        }
        // This reservation may relocate the published table, so it is last:
        // everything after success is allocation-free and republishes its pointer.
        if self.views.try_reserve(capacity - self.views.len()).is_err() {
            return 0;
        }
        self.views
            .resize_with(capacity, || Cell::new(ptr::null_mut()));
        self.slots.resize_with(required, || None);
        self.vacant.pop();
        self.views[id].set(owned.view.as_ptr());
        self.slots[id] = Some(owned);
        self.count += 1;
        self.layout_bounds = None;
        self.publish(ui);
        ui.dirty = 1;
        id
    }
    // Zero marks the end; slot zero is never allocated.
    pub fn next_in_subtree(&self, root: usize, mut id: usize, descend: bool) -> usize {
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
    /// Visit live IDs before destruction; None visits the entire scene for reset.
    /// The visitor must not reenter the scene or keep any node pointers.
    pub fn visit_destroying(&self, root: Option<usize>, mut visit: impl FnMut(i32)) {
        if let Some(root) = root {
            let mut id = root;
            while id != 0 {
                visit(id as i32);
                id = self.next_in_subtree(root, id, true);
            }
        } else {
            for (id, slot) in self.slots.iter().enumerate() {
                if slot.is_some() {
                    visit(id as i32);
                }
            }
        }
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
        self.layout_bounds = None;
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
        if parent == child {
            return false;
        }
        let before = if self.node(before).is_some() && self.links(before).parent == parent {
            before
        } else {
            0
        };
        let current = self.links(child);
        if before == child || (current.parent == parent && current.next == before) {
            return true;
        }
        // Reordering siblings or inserting a leaf cannot introduce a cycle.
        if current.first != 0 && current.parent != parent {
            let mut ancestor = parent;
            while ancestor != 0 {
                if ancestor == child {
                    return false;
                }
                ancestor = self.links(ancestor).parent;
            }
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
        self.layout_bounds = None;
        self.detach(root, ui);
        let mut id = root;
        loop {
            let links = self.links(id);
            if links.first != 0 {
                id = links.first;
                continue;
            }
            // Remove a leaf, then visit its next sibling or its parent. No
            // recursion and no temporary worklist allocation, even under OOM.
            if links.parent != 0 {
                self.links_mut(links.parent).first = links.next;
                if links.next != 0 {
                    self.links_mut(links.next).prev = 0;
                } else {
                    self.links_mut(links.parent).last = 0;
                }
            }
            if ui.root == self.ptr(id) {
                ui.root = ptr::null_mut();
            }
            self.views[id].set(ptr::null_mut());
            self.slots[id] = None;
            self.vacant.push(Reverse(id));
            self.count -= 1;
            if id == root {
                break;
            }
            id = if links.next != 0 {
                links.next
            } else {
                links.parent
            };
        }
        ui.dirty = 1;
    }
    pub fn reset(&mut self, ui: &mut Ui) {
        self.layout_bounds = None;
        ui.root = ptr::null_mut();
        ui.nodes = ptr::null_mut();
        ui.cap = 0;
        self.slots = Vec::new();
        self.views = Vec::new();
        self.vacant = BinaryHeap::new();
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
        let Some(next) = memory::try_cstring(text) else {
            return;
        };
        let next = next.into_raw();
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
            if used == out.len() - 1 {
                break;
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
        let same_base = before.same_base(&after);
        if same_base && !ext_changed {
            return;
        }
        let mut moved = *before;
        moved.pos = after.pos;
        moved.translate_x = after.translate_x;
        moved.translate_y = after.translate_y;
        moved.opacity = after.opacity;
        let insets_changed = !before.pos.iter().zip(after.pos).all(|(a, b)| a.same(b));
        if !ext_changed
            && (!insets_changed || (after.absolute != 0 && before.absolute != 0))
            && moved.same_base(&after)
        {
            if before.opacity != after.opacity {
                let n = self.node_mut(id).unwrap();
                n.paint_dirty = 1;
                n.gen = n.gen.wrapping_add(1);
            }
            self.moved(id, ui);
            return;
        }
        let mut transform_before = *before;
        transform_before.opacity = after.opacity;
        // Most changes cannot use the transform shortcut. Avoid walking all
        // ancestors for ordinary colors, sizes, text and first-time extensions.
        if n.kind == 3
            || n.kind == 1
            || !transform_before.same_base(&after)
            || !ext
                .zip(ext_before)
                .is_some_and(|(x, b)| x.same_values(b, true))
        {
            self.invalidate(id, ui);
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
        if !text_ancestor && self.pure_layout(id) {
            let n = self.node_mut(id).unwrap();
            n.paint_dirty = 1;
            if before.opacity != after.opacity {
                n.gen = n.gen.wrapping_add(1);
            }
            unsafe {
                (*n.s.ext).r.ok = 0;
            }
            self.moved(id, ui);
        } else {
            self.invalidate(id, ui);
        }
    }
}
