//! Fallible allocation at the host boundary. Running out of memory must not
//! turn a recoverable node/style operation into a process-wide allocation abort.
use std::alloc::{alloc, Layout};
use std::ffi::{CStr, CString};

pub fn try_box<T>(value: T) -> Option<Box<T>> {
    if !allowed() {
        return None;
    }
    let layout = Layout::new::<T>();
    if layout.size() == 0 {
        return Some(Box::new(value));
    }
    unsafe {
        let storage = alloc(layout).cast::<T>();
        if storage.is_null() {
            return None;
        }
        storage.write(value);
        // Same global allocator and layout that Box uses for destruction.
        Some(Box::from_raw(storage))
    }
}

pub fn try_cstring(value: &CStr) -> Option<CString> {
    if !allowed() {
        return None;
    }
    let bytes = value.to_bytes_with_nul();
    let mut storage = Vec::new();
    storage.try_reserve_exact(bytes.len()).ok()?;
    storage.extend_from_slice(bytes);
    // The source CStr already guarantees exactly one trailing NUL.
    Some(unsafe { CString::from_vec_with_nul_unchecked(storage) })
}

#[cfg(not(test))]
fn allowed() -> bool {
    true
}
#[cfg(test)]
thread_local! { static BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn allowed() -> bool {
    BUDGET.with(|b| match b.get() {
        None => true,
        Some(0) => false,
        Some(n) => {
            b.set(Some(n - 1));
            true
        }
    })
}

/// Scoped fault injection for these allocations only, confined to one test thread.
#[cfg(test)]
pub struct AllocationBudget(Option<usize>);
#[cfg(test)]
impl AllocationBudget {
    pub fn new(budget: usize) -> Self {
        Self(BUDGET.with(|b| b.replace(Some(budget))))
    }
}
#[cfg(test)]
impl Drop for AllocationBudget {
    fn drop(&mut self) {
        BUDGET.with(|b| b.set(self.0));
    }
}
