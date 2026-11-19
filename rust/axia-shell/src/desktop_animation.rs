//! Native desktop selection and paint-only animation ownership; no JS completion callback.
use crate::{anim, runtime::native as scene, winmodel::WindowState};
use axia_state::Reader;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Binding {
    node: i32,
    generation: u64,
}
#[derive(Clone, Copy)]
struct Transition {
    start: f64,
    step: i32,
}
pub struct Animation {
    windows: Reader<WindowState>,
    active: Option<i64>,
    bindings: BTreeMap<String, Binding>,
    running: Vec<(Binding, u32)>,
    transition: Option<Transition>,
    reduce: bool,
    rtl: bool,
}
impl Animation {
    pub fn new(windows: Reader<WindowState>) -> Self {
        Self {
            windows,
            active: None,
            bindings: BTreeMap::new(),
            running: Vec::new(),
            transition: None,
            reduce: true,
            rtl: false,
        }
    }
    pub fn initialize(&mut self) {
        self.active = Some(self.windows.read().active_desktop());
    }
    fn valid(binding: Binding) -> bool {
        scene::generation(binding.node) == Some(binding.generation)
    }
    fn restore(binding: Binding, id: u32) {
        if !Self::valid(binding) {
            return;
        }
        if let Some(base) = anim::clear_desktop(binding.node, id) {
            if Self::valid(binding) {
                scene::apply_animation(binding.node, base);
            }
        }
    }
    fn cancel(&mut self) {
        for (binding, id) in self.running.drain(..) {
            Self::restore(binding, id);
        }
        self.transition = None;
    }
    fn animate(&mut self, binding: Binding, transition: Transition, now: f64) {
        if !Self::valid(binding) {
            return;
        }
        let Some(base) = scene::animation_props(binding.node) else {
            return;
        };
        if let Some((id, initial)) = anim::desktop_slide(
            binding.node,
            transition.start,
            now,
            transition.step,
            self.rtl,
            base,
        ) {
            scene::apply_animation(binding.node, initial);
            self.running.push((binding, id));
        }
    }
    pub fn configure(&mut self, reduce: bool, rtl: bool) {
        self.reduce = reduce;
        self.rtl = rtl;
    }
    pub fn sync(&mut self, now: f64) {
        let state = self.windows.read();
        let active = state.active_desktop();
        let switched = self.active.is_some_and(|old| old != active);
        let step = if state
            .desktops()
            .iter()
            .position(|d| Some(*d) == self.active)
            .unwrap_or(0)
            <= state
                .desktops()
                .iter()
                .position(|d| *d == active)
                .unwrap_or(0)
        {
            1
        } else {
            -1
        };
        if self.active.is_some() {
            self.active = Some(active);
        }
        let disabled = self.reduce || state.overview();
        let visible: Vec<_> = state.visible_windows().map(|w| w.id().to_owned()).collect();
        let exists: Vec<_> = state.windows().map(|w| w.id().to_owned()).collect();
        drop(state);
        self.bindings
            .retain(|id, binding| exists.contains(id) && Self::valid(*binding));
        if switched || disabled {
            self.cancel();
        }
        if switched && !disabled {
            let transition = Transition { start: now, step };
            self.transition = Some(transition);
            let bindings: Vec<_> = visible
                .iter()
                .filter_map(|id| self.bindings.get(id).copied())
                .collect();
            for binding in bindings {
                self.animate(binding, transition, now);
            }
        }
        let live: Vec<_> = visible
            .iter()
            .filter_map(|id| self.bindings.get(id).copied())
            .collect();
        self.running.retain(|(binding, id)| {
            let keep = live
                .iter()
                .any(|b| b.node == binding.node && b.generation == binding.generation);
            if !keep {
                Self::restore(*binding, *id);
            }
            keep
        });
    }
    pub fn bind(&mut self, id: &str, node: i32, now: f64) -> bool {
        self.sync(now);
        if let Some(previous) = self.bindings.remove(id) {
            self.running.retain(|(binding, animation)| {
                if binding.node == previous.node && binding.generation == previous.generation {
                    Self::restore(*binding, *animation);
                    false
                } else {
                    true
                }
            });
        }
        if node == 0 {
            return true;
        }
        let state = self.windows.read();
        if !state.windows().any(|w| w.id() == id) {
            return false;
        }
        let visible = state.visible_windows().any(|w| w.id() == id);
        drop(state);
        let Some(generation) = scene::generation(node) else {
            return false;
        };
        let binding = Binding { node, generation };
        self.bindings.insert(id.to_owned(), binding);
        if visible && !self.reduce {
            if let Some(transition) = self.transition {
                self.animate(binding, transition, now);
            }
        }
        true
    }
}
impl Drop for Animation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
#[path = "desktop_animation_tests.rs"]
mod tests;
