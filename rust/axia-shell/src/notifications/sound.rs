//! Native notification audio uses the existing nonblocking session audio client.
#[cfg(not(test))]
pub(super) fn notify() {
    extern "C" {
        fn snd_play(name: *const std::ffi::c_char) -> i32;
    }
    // A disconnected server drops this attempt, matching the public sound API.
    // The notification scheduler owns future pulses; audio failures add no retry timer.
    unsafe {
        snd_play(c"notify".as_ptr());
    }
}

#[cfg(test)]
thread_local! { static ATTEMPTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn notify() {
    ATTEMPTS.with(|n| n.set(n.get() + 1));
}
#[cfg(test)]
pub(super) fn attempts() -> usize {
    ATTEMPTS.with(std::cell::Cell::get)
}
