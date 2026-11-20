//! Protected settings I/O reuses the existing atomic C file writer.
#[cfg(not(test))]
mod host {
    use std::ffi::{c_char, CString};
    extern "C" {
        fn axia_settings_read(home: *const c_char, len: *mut usize) -> *mut c_char;
        fn axia_settings_write(
            home: *const c_char,
            text: *const c_char,
            len: usize,
            durable: i32,
        ) -> i32;
    }
    pub fn load(home: &str) -> Option<Vec<u8>> {
        let home = CString::new(home).ok()?;
        let mut len = 0;
        let bytes = unsafe { axia_settings_read(home.as_ptr(), &mut len) };
        if bytes.is_null() {
            return None;
        }
        let value = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) }.to_vec();
        unsafe {
            libc::free(bytes.cast());
        }
        Some(value)
    }
    pub fn write(home: &str, bytes: &[u8], durable: bool) -> bool {
        let Ok(home) = CString::new(home) else {
            return false;
        };
        unsafe {
            axia_settings_write(
                home.as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len(),
                durable as i32,
            ) != 0
        }
    }
}
#[cfg(test)]
pub(super) mod host {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };
    thread_local! {
        static FILES:RefCell<BTreeMap<String,Vec<u8>>>=RefCell::new(BTreeMap::new());
        static WRITES:RefCell<Vec<(String,Vec<u8>,bool)>>=const {RefCell::new(Vec::new())};
        static FAIL:Cell<bool>=const {Cell::new(false)};
    }
    pub fn load(home: &str) -> Option<Vec<u8>> {
        FILES.with(|f| f.borrow().get(home).cloned())
    }
    pub fn write(home: &str, bytes: &[u8], durable: bool) -> bool {
        WRITES.with(|w| w.borrow_mut().push((home.into(), bytes.into(), durable)));
        if FAIL.with(Cell::get) {
            return false;
        }
        FILES.with(|f| {
            f.borrow_mut().insert(home.into(), bytes.into());
        });
        true
    }
    pub fn set_file(home: &str, bytes: &[u8]) {
        FILES.with(|f| {
            f.borrow_mut().insert(home.into(), bytes.into());
        });
    }
    pub fn writes() -> Vec<(String, Vec<u8>, bool)> {
        WRITES.with(|w| w.borrow().clone())
    }
    pub fn fail(on: bool) {
        FAIL.with(|f| f.set(on));
    }
}
pub(super) use host::{load, write};
