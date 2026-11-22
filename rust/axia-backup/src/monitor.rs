//! A mutex with its condition variable whose timed waits follow the wall clock.
//!
//! Backups are scheduled in calendar time ("an hour after the last one"), so after a suspend or a
//! change of the clock the thread has to wake when the *real* time comes; `std`'s `Condvar` counts
//! monotonic time, which stops while the machine sleeps. This is the C service's
//! `pthread_cond_timedwait` (CLOCK_REALTIME), kept as it was.
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};

pub struct Monitor<T> {
    mutex: UnsafeCell<libc::pthread_mutex_t>,
    cond: UnsafeCell<libc::pthread_cond_t>,
    data: UnsafeCell<T>,
}

// The data is only reached through a `Guard`, i.e. with the mutex held.
unsafe impl<T: Send> Sync for Monitor<T> {}
unsafe impl<T: Send> Send for Monitor<T> {}

pub struct Guard<'a, T> {
    monitor: &'a Monitor<T>,
}

impl<T> Monitor<T> {
    pub const fn new(data: T) -> Self {
        Self { mutex: UnsafeCell::new(libc::PTHREAD_MUTEX_INITIALIZER), cond: UnsafeCell::new(libc::PTHREAD_COND_INITIALIZER), data: UnsafeCell::new(data) }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        unsafe { libc::pthread_mutex_lock(self.mutex.get()) };
        Guard { monitor: self }
    }

    /// Wakes every waiter (the mutex may be held or not).
    pub fn notify_all(&self) {
        unsafe { libc::pthread_cond_broadcast(self.cond.get()) };
    }
}

impl<T> Guard<'_, T> {
    /// Waits for a notification.
    pub fn wait(&mut self) {
        unsafe { libc::pthread_cond_wait(self.monitor.cond.get(), self.monitor.mutex.get()) };
    }

    /// Waits for a notification or until `when` (seconds since the epoch).
    pub fn wait_until(&mut self, when: i64) {
        let ts = libc::timespec { tv_sec: when as libc::time_t, tv_nsec: 0 };
        unsafe { libc::pthread_cond_timedwait(self.monitor.cond.get(), self.monitor.mutex.get(), &ts) };
    }

    /// Runs `f` with the mutex released (a long operation); the data is not reachable meanwhile.
    pub fn unlocked<R>(&mut self, f: impl FnOnce() -> R) -> R {
        unsafe { libc::pthread_mutex_unlock(self.monitor.mutex.get()) };
        let out = f();
        unsafe { libc::pthread_mutex_lock(self.monitor.mutex.get()) };
        out
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.monitor.data.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.monitor.data.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        unsafe { libc::pthread_mutex_unlock(self.monitor.mutex.get()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_notification_wakes_a_waiter_and_a_deadline_ends_a_wait() {
        static M: Monitor<i32> = Monitor::new(0);
        let waiter = std::thread::spawn(|| {
            let mut g = M.lock();
            while *g == 0 {
                g.wait();
            }
            *g
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        *M.lock() = 7;
        M.notify_all();
        assert_eq!(waiter.join().unwrap(), 7);
        // a deadline already in the past returns at once
        let start = std::time::Instant::now();
        M.lock().wait_until(1);
        assert!(start.elapsed().as_millis() < 500);
        // the data is not touched while released
        let shared = Arc::new(Monitor::new(1u8));
        let mut g = shared.lock();
        let other = shared.clone();
        let out = g.unlocked(|| *other.lock());
        assert_eq!(out, 1);
    }
}
