//! UI-thread state shared by native models and their consumers, without a wire
//! format or a polling timer. Subscribers receive invalidations after a write
//! borrow ends, then read the latest typed state. Several commits coalesce into
//! one pending revision; intermediate states need not be cloned or queued.
use std::{
    cell::{Cell, Ref, RefCell},
    rc::{Rc, Weak},
};

type Pending = Cell<Option<u64>>;

struct Inner<T> {
    value: RefCell<T>,
    revision: Cell<u64>,
    subscribers: RefCell<Vec<Weak<Pending>>>,
}

/// A shared state owner. Like the UI it belongs to one thread (`Rc`, not `Arc`).
pub struct Store<T>(Rc<Inner<T>>);

impl<T> Clone for Store<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Store<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Store<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(Inner {
            value: RefCell::new(value),
            revision: Cell::new(0),
            subscribers: RefCell::new(Vec::new()),
        }))
    }

    /// Borrow the authoritative state, without a clone, encoding or parsing.
    pub fn read(&self) -> Ref<'_, T> {
        self.0.value.borrow()
    }

    pub fn revision(&self) -> u64 {
        self.0.revision.get()
    }

    pub fn reader(&self) -> Reader<T> {
        Reader(self.0.clone())
    }

    /// One model operation. The model reports whether its externally observable
    /// state changed; read-only operations and no-ops must return `false`.
    /// This is a commit boundary, not a rollback journal: do not panic after
    /// mutating state. No callbacks run while a mutable state borrow is held.
    pub fn update<R>(&self, edit: impl FnOnce(&mut T) -> (R, bool)) -> R {
        let (result, changed) = edit(&mut self.0.value.borrow_mut());
        if changed {
            let revision = self
                .revision()
                .checked_add(1)
                .expect("state revision exhausted");
            self.0.revision.set(revision);
            self.0.subscribers.borrow_mut().retain(|weak| {
                let Some(pending) = weak.upgrade() else {
                    return false;
                };
                pending.set(Some(revision));
                true
            });
        }
        result
    }

    /// Observe subsequent commits. Read the initial state after subscribing;
    /// subscription itself does not pretend that the model has changed.
    pub fn subscribe(&self) -> Subscription {
        let pending = Rc::new(Cell::new(None));
        let mut subscribers = self.0.subscribers.borrow_mut();
        // Churn while idle must not leave an ever-growing list of dead slots.
        subscribers.retain(|weak| weak.strong_count() != 0);
        subscribers.push(Rc::downgrade(&pending));
        Subscription { pending }
    }
}

impl<T: PartialEq> Store<T> {
    /// Replace scalar/configuration state, suppressing equal writes.
    pub fn set(&self, value: T) -> bool {
        self.update(|current| {
            if *current == value {
                return (false, false);
            }
            *current = value;
            (true, true)
        })
    }
}

/// A consumer can read and observe state but cannot bypass the model's rules.
pub struct Reader<T>(Rc<Inner<T>>);
impl<T> Clone for Reader<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Reader<T> {
    pub fn read(&self) -> Ref<'_, T> {
        self.0.value.borrow()
    }
    pub fn revision(&self) -> u64 {
        self.0.revision.get()
    }
    pub fn subscribe(&self) -> Subscription {
        Store(self.0.clone()).subscribe()
    }
}

/// A cancellable invalidation receiver. Dropping it cancels the subscription;
/// it does not keep the state owner or any UI component alive.
pub struct Subscription {
    pending: Rc<Pending>,
}

impl Subscription {
    /// Check readiness without consuming it, for the native event-loop deadline.
    pub fn pending(&self) -> bool {
        self.pending.get().is_some()
    }
    /// The newest pending revision, at most once until the next commit.
    pub fn take(&self) -> Option<u64> {
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_readers_share_the_authoritative_state_and_equal_writes_are_silent() {
        let store = Store::new(String::from("before"));
        let other = store.clone();
        let changes = other.subscribe();
        assert_eq!(changes.take(), None);
        assert!(!store.set(String::from("before")));
        assert_eq!(store.revision(), 0);
        assert!(store.set(String::from("after")));
        assert!(changes.pending());
        assert_eq!(changes.take(), Some(1));
        assert!(!changes.pending());
        assert_eq!(&*other.read(), "after");
        assert_eq!(changes.take(), None);
    }

    #[test]
    fn readers_coalesce_bursts_independently_and_late_readers_only_see_future_writes() {
        let store = Store::new(0);
        let a = store.subscribe();
        let b = store.subscribe();
        store.set(1);
        assert_eq!(a.take(), Some(1));
        let late = store.subscribe();
        store.set(2);
        store.set(3);
        assert_eq!(a.take(), Some(3));
        assert_eq!(b.take(), Some(3));
        assert_eq!(late.take(), Some(3));
        assert_eq!(*store.read(), 3);
        assert_eq!(b.take(), None);
    }

    #[test]
    fn one_operation_publishes_one_revision_after_all_fields_change() {
        let store = Store::new((0, String::new()));
        let changes = store.subscribe();
        let result = store.update(|value| {
            value.0 = 4;
            value.1 = "ready".into();
            (42, true)
        });
        assert_eq!(result, 42);
        assert_eq!(changes.take(), Some(1));
        assert_eq!(&*store.read(), &(4, String::from("ready")));
        store.update(|_| ((), false));
        assert_eq!(changes.take(), None);
    }

    #[test]
    fn cancelled_readers_do_not_accumulate_during_idle_or_retain_the_owner() {
        let store = Store::new(0);
        let weak_owner = Rc::downgrade(&store.0);
        for _ in 0..1000 {
            drop(store.subscribe());
        }
        assert_eq!(store.0.subscribers.borrow().len(), 1);
        store.set(1);
        assert!(store.0.subscribers.borrow().is_empty());
        let changes = store.subscribe();
        drop(store);
        assert!(weak_owner.upgrade().is_none());
        assert_eq!(changes.take(), None);
    }
}
