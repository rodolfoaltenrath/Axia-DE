//! Borrow compositor fullscreen state synchronously on its UI thread, without JS facts.
#[cfg(not(test))]
pub(crate) fn current() -> (bool, i64) {
    extern "C" {
        fn wl_fullscreen_active() -> i32;
        fn wl_fullscreen_window() -> i32;
    }
    unsafe { (wl_fullscreen_active() != 0, i64::from(wl_fullscreen_window())) }
}

#[cfg(test)]
thread_local! { static SOURCE: std::cell::Cell<(bool,i64)> = const { std::cell::Cell::new((false,0)) }; }
#[cfg(test)]
pub(crate) fn current() -> (bool,i64) { SOURCE.with(std::cell::Cell::get) }
#[cfg(test)]
pub(super) struct Guard((bool,i64));
#[cfg(test)]
pub(super) fn set(fullscreen: bool, surface: i64) -> Guard {
    Guard(SOURCE.with(|source| source.replace((fullscreen,surface))))
}
#[cfg(test)]
impl Drop for Guard { fn drop(&mut self) { SOURCE.with(|source|source.set(self.0)); } }
