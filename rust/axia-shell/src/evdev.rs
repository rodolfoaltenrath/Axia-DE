//! Evdev discovery, seat-mediated descriptor lifecycle and synchronous dispatch.
//! The C layout remains temporary until the compositor owns these objects in Rust.
use crate::onscreen_keyboard::{osk_chassis_portable, osk_full_keyboard, osk_keyboard_counts};
use std::{
    ffi::{c_char, c_int, c_ulong, c_void, CString},
    mem::{size_of, zeroed},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr,
    sync::{Mutex, OnceLock},
};

const MAX_DEVICES: usize = 32;
const WORD_BITS: usize = c_ulong::BITS as usize;
const KEY_WORDS: usize = 0x300_usize.div_ceil(WORD_BITS);
const EV_WORDS: usize = 32_usize.div_ceil(WORD_BITS);
const ABS_WORDS: usize = 64_usize.div_ceil(WORD_BITS);
const REL_WORDS: usize = 16_usize.div_ceil(WORD_BITS);
const GRAB: c_ulong = libc::_IOW::<c_int>(b'E' as u32, 0x90) as c_ulong;

#[cfg(not(test))]
mod host {
    use super::*;
    extern "C" {
        pub fn seat_device_open(path: *const c_char, flags: c_int) -> c_int;
        pub fn seat_device_close(fd: c_int);
        pub fn tp_open(fd: c_int, name: *const c_char) -> *mut c_void;
        pub fn tp_free(device: *mut c_void);
        pub fn tp_kind(device: *const c_void) -> c_int;
    }
}
// Unit tests never open or grab host input devices. The native ABI fixture tests
// discovery/classification through mocked seat and kernel calls separately.
#[cfg(test)]
mod host {
    use super::*;
    pub unsafe fn seat_device_open(_: *const c_char, _: c_int) -> c_int {
        -1
    }
    pub unsafe fn seat_device_close(fd: c_int) {
        libc::close(fd);
    }
    pub unsafe fn tp_open(_: c_int, _: *const c_char) -> *mut c_void {
        ptr::null_mut()
    }
    pub unsafe fn tp_free(_: *mut c_void) {}
    pub unsafe fn tp_kind(_: *const c_void) -> c_int {
        0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputDevice {
    fd: c_int,
    has_abs: c_int,
    ax: libc::input_absinfo,
    ay: libc::input_absinfo,
    name: [u8; 80],
    tp: *mut c_void,
    wheel_hires: c_int,
    hwheel_hires: c_int,
    keyboard: c_int,
    bus: c_int,
    typed: c_int,
}
impl Default for InputDevice {
    fn default() -> Self {
        unsafe { zeroed() }
    }
}
#[repr(C)]
#[derive(Default)]
pub struct Input {
    dev: [InputDevice; MAX_DEVICES],
    count: c_int,
}
type Handler = unsafe extern "C" fn(*mut c_void, *const InputDevice, *const libc::input_event);

fn has(words: &[c_ulong], code: usize) -> bool {
    words
        .get(code / WORD_BITS)
        .is_some_and(|word| word & (1 << (code % WORD_BITS)) != 0)
}
fn device_name(device: &InputDevice) -> String {
    let end = device
        .name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(device.name.len());
    String::from_utf8_lossy(&device.name[..end]).into_owned()
}

unsafe fn add_device(input: &mut Input, fd: c_int, path: &str) {
    let mut ev = [0; EV_WORDS];
    let mut keys = [0; KEY_WORDS];
    let mut abs = [0; ABS_WORDS];
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; EV_WORDS]>(b'E' as u32, 0x20),
        ev.as_mut_ptr(),
    );
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; KEY_WORDS]>(b'E' as u32, 0x21),
        keys.as_mut_ptr(),
    );
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; ABS_WORDS]>(b'E' as u32, 0x23),
        abs.as_mut_ptr(),
    );
    let keyboard = has(&ev, 1) && has(&keys, 30) && has(&keys, 28);
    let mouse = has(&ev, 2) && has(&keys, 272);
    let mut tablet = has(&ev, 3) && has(&abs, 0) && has(&abs, 1);
    if (!keyboard && !mouse && !tablet) || input.count as usize >= MAX_DEVICES {
        host::seat_device_close(fd);
        return;
    }
    let device = &mut input.dev[input.count as usize];
    *device = InputDevice {
        fd,
        ..InputDevice::default()
    };
    input.count += 1;
    libc::ioctl(
        fd,
        libc::_IOR::<[u8; 80]>(b'E' as u32, 0x06),
        device.name.as_mut_ptr(),
    );
    // Treat kernel names as bounded strings even if a malformed device fills it.
    device.name[79] = 0;
    let mut id: libc::input_id = zeroed();
    libc::ioctl(fd, libc::_IOR::<libc::input_id>(b'E' as u32, 0x02), &mut id);
    device.bus = id.bustype as c_int;
    device.keyboard = (keyboard
        && osk_full_keyboard(keys.as_ptr(), keys.len(), device.name.as_ptr().cast()) != 0)
        as c_int;
    let mut rel = [0; REL_WORDS];
    libc::ioctl(
        fd,
        libc::_IOR::<[c_ulong; REL_WORDS]>(b'E' as u32, 0x22),
        rel.as_mut_ptr(),
    );
    device.wheel_hires = has(&rel, 11) as c_int;
    device.hwheel_hires = has(&rel, 12) as c_int;
    device.tp = host::tp_open(fd, device.name.as_ptr().cast());
    if !device.tp.is_null() {
        tablet = false;
    }
    if tablet {
        device.has_abs = 1;
        libc::ioctl(
            fd,
            libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x40),
            &mut device.ax,
        );
        libc::ioctl(
            fd,
            libc::_IOR::<libc::input_absinfo>(b'E' as u32, 0x41),
            &mut device.ay,
        );
    }
    if libc::ioctl(fd, GRAB, 1 as c_int) != 0 {
        eprintln!(
            "input: could not grab {}: {}",
            device_name(device),
            std::io::Error::last_os_error()
        );
    }
    let kind = if !device.tp.is_null() {
        if host::tp_kind(device.tp) == 2 {
            " [touchscreen]"
        } else {
            " [touchpad]"
        }
    } else if mouse {
        " [mouse]"
    } else if tablet {
        " [tablet]"
    } else {
        ""
    };
    eprintln!(
        "input: {path} -> {}{}{kind}",
        device_name(device),
        if keyboard { " [keyboard]" } else { "" }
    );
}

unsafe fn scan_devices(input: &mut Input) {
    for index in 0..64 {
        if input.count as usize >= MAX_DEVICES {
            break;
        }
        let path = format!("/dev/input/event{index}");
        let path_c = CString::new(path.as_bytes()).unwrap();
        let mut stat: libc::stat = zeroed();
        if libc::stat(path_c.as_ptr(), &mut stat) != 0 {
            continue;
        }
        let found = input.dev[..input.count as usize].iter().any(|device| {
            let mut existing: libc::stat = zeroed();
            libc::fstat(device.fd, &mut existing) == 0
                && existing.st_rdev == stat.st_rdev
                && existing.st_ino == stat.st_ino
        });
        if found {
            continue;
        }
        let fd = host::seat_device_open(
            path_c.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        );
        if fd >= 0 {
            add_device(input, fd, &path);
        }
    }
}

/// # Safety
/// `input` must point to writable Input storage with no live owned descriptors.
#[no_mangle]
pub unsafe extern "C" fn input_open(input: *mut Input) -> c_int {
    input.write(Input::default());
    scan_devices(&mut *input);
    if (*input).count > 0 {
        0
    } else {
        -1
    }
}
static WATCH: Mutex<Option<OwnedFd>> = Mutex::new(None);
#[no_mangle]
pub extern "C" fn input_watch_fd() -> c_int {
    let mut watch = WATCH.lock().unwrap();
    if let Some(fd) = watch.as_ref() {
        return fd.as_raw_fd();
    }
    unsafe {
        let fd = libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK);
        if fd < 0 {
            return -1;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        if libc::inotify_add_watch(
            fd.as_raw_fd(),
            c"/dev/input".as_ptr(),
            libc::IN_CREATE | libc::IN_MOVED_TO,
        ) < 0
        {
            return -1;
        }
        let raw = fd.as_raw_fd();
        *watch = Some(fd);
        raw
    }
}
/// # Safety
/// `input` must point to an exclusively accessible initialized Input.
#[no_mangle]
pub unsafe extern "C" fn input_watch_dispatch(input: *mut Input, active: c_int) -> c_int {
    {
        let watch = WATCH.lock().unwrap();
        if let Some(watch) = watch.as_ref() {
            let mut events = [0u8; 4096];
            while libc::read(watch.as_raw_fd(), events.as_mut_ptr().cast(), events.len()) > 0 {}
        }
    }
    let before = (*input).count;
    if active != 0 {
        scan_devices(&mut *input);
    }
    (before != (*input).count) as c_int
}
unsafe fn add_fds(input: &mut Input, fds: *const c_int, count: c_int, path: &str) {
    for index in 0..count.max(0) as usize {
        let fd = fds.add(index).read();
        libc::fcntl(
            fd,
            libc::F_SETFL,
            libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
        );
        add_device(input, fd, path);
    }
}
/// # Safety
/// `input` must be writable Input storage with no live descriptors. `fds` holds
/// `count` readable descriptors; ownership is transferred for each one supplied.
#[no_mangle]
pub unsafe extern "C" fn input_open_fds(
    input: *mut Input,
    fds: *const c_int,
    count: c_int,
) -> c_int {
    input.write(Input::default());
    add_fds(&mut *input, fds, count, "(lord-sessiond)");
    if (*input).count > 0 {
        0
    } else {
        -1
    }
}
/// # Safety
/// `input` must be initialized and exclusively accessible. `fds` holds `count`
/// readable descriptors; ownership is transferred for each one supplied.
#[no_mangle]
pub unsafe extern "C" fn input_add_fds(
    input: *mut Input,
    fds: *const c_int,
    count: c_int,
) -> c_int {
    let before = (*input).count;
    add_fds(&mut *input, fds, count, "(enchufado)");
    (*input).count - before
}
/// # Safety
/// `input` must be initialized and exclusively accessible.
#[no_mangle]
pub unsafe extern "C" fn input_remove(input: *mut Input, index: c_int) {
    if index < 0 || index >= (*input).count {
        return;
    }
    let device = &(*input).dev[index as usize];
    eprintln!("input: {} is gone; closing it", device_name(device));
    host::seat_device_close(device.fd);
    host::tp_free(device.tp);
    let slot = ptr::addr_of_mut!((*input).dev)
        .cast::<InputDevice>()
        .add(index as usize);
    ptr::copy(slot.add(1), slot, ((*input).count - index - 1) as usize);
    (*input).count -= 1;
}
/// # Safety
/// `input` must be initialized and exclusively accessible.
#[no_mangle]
pub unsafe extern "C" fn input_close(input: *mut Input) {
    for index in 0..(*input).count as usize {
        let device = &mut (*input).dev[index];
        libc::ioctl(device.fd, GRAB, 0 as c_int);
        host::seat_device_close(device.fd);
        host::tp_free(device.tp);
        device.tp = ptr::null_mut();
    }
    (*input).count = 0;
}
/// # Safety
/// `input` and `index` must identify a live device. The callback/context must be
/// valid until return and must not remove the device or mutate Input storage.
#[no_mangle]
pub unsafe extern "C" fn input_dispatch(
    input: *mut Input,
    index: c_int,
    callback: Handler,
    context: *mut c_void,
) {
    let device = ptr::addr_of_mut!((*input).dev)
        .cast::<InputDevice>()
        .add(index as usize);
    let mut events: [libc::input_event; 64] = zeroed();
    loop {
        let bytes = libc::read(
            (*device).fd,
            events.as_mut_ptr().cast(),
            size_of::<[libc::input_event; 64]>(),
        );
        if bytes <= 0 {
            break;
        }
        for event in &events[..bytes as usize / size_of::<libc::input_event>()] {
            if (*device).keyboard != 0
                && event.type_ == 1
                && event.value == 1
                && (16..=50).contains(&event.code)
            {
                (*device).typed = 1;
            }
            // Keep device access raw: the C callback can read the containing shell.
            callback(context, device, event);
        }
    }
}
static PORTABLE: OnceLock<c_int> = OnceLock::new();
fn portable_computer() -> c_int {
    *PORTABLE.get_or_init(|| {
        let chassis = std::fs::read_to_string("/sys/class/dmi/id/chassis_type")
            .ok()
            .and_then(|text| text.split_whitespace().next()?.parse().ok())
            .unwrap_or(0);
        osk_chassis_portable(chassis)
    })
}
/// # Safety
/// `input` must point to an initialized Input, without concurrent mutation.
#[no_mangle]
pub unsafe extern "C" fn input_has_keyboard(input: *const Input) -> c_int {
    for device in &(&(*input).dev)[..(*input).count as usize] {
        if device.keyboard != 0
            && osk_keyboard_counts(device.bus, portable_computer(), device.typed) != 0
        {
            return 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::IntoRawFd;

    unsafe fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [-1; 2];
        assert_eq!(
            libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC),
            0
        );
        (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
    }
    struct Received {
        count: usize,
        typed_after: usize,
    }
    unsafe extern "C" fn receive(
        context: *mut c_void,
        device: *const InputDevice,
        event: *const libc::input_event,
    ) {
        let received = &mut *(context as *mut Received);
        assert_eq!((*event).value, received.count as c_int);
        if (*device).typed != 0 {
            received.typed_after += 1;
        }
        received.count += 1;
    }

    #[test]
    fn dispatch_drains_multiple_read_batches_and_sets_typing_before_callback() {
        unsafe {
            let (reader, writer) = pipe();
            let mut input = Input::default();
            input.count = 1;
            input.dev[0].fd = reader.into_raw_fd();
            input.dev[0].keyboard = 1;
            let mut events: [libc::input_event; 100] = zeroed();
            for (index, event) in events.iter_mut().enumerate() {
                event.type_ = 1;
                event.code = 30;
                event.value = index as c_int;
            }
            assert_eq!(
                libc::write(
                    writer.as_raw_fd(),
                    events.as_ptr().cast(),
                    std::mem::size_of_val(&events)
                ),
                std::mem::size_of_val(&events) as isize
            );
            let mut received = Received {
                count: 0,
                typed_after: 0,
            };
            input_dispatch(
                &mut input,
                0,
                receive,
                &mut received as *mut Received as *mut c_void,
            );
            assert_eq!(received.count, 100);
            assert_eq!(received.typed_after, 99);
            assert_eq!(input.dev[0].typed, 1);
            let fd = input.dev[0].fd;
            input_close(&mut input);
            assert_eq!(input.count, 0);
            assert_eq!(libc::fcntl(fd, libc::F_GETFD), -1);
            input_close(&mut input);
        }
    }

    #[test]
    fn rejected_descriptors_are_closed_and_empty_lists_are_safe() {
        unsafe {
            let (reader, _writer) = pipe();
            let fd = reader.into_raw_fd();
            let mut input = Input::default();
            assert_eq!(input_open_fds(&mut input, &fd, 1), -1);
            assert_eq!(input.count, 0);
            assert_eq!(libc::fcntl(fd, libc::F_GETFD), -1);
            assert_eq!(input_open_fds(&mut input, ptr::null(), 0), -1);
            assert_eq!(input_add_fds(&mut input, ptr::null(), -1), 0);
            input_remove(&mut input, -1);
            input_remove(&mut input, 0);
        }
    }

    #[test]
    fn removing_one_device_preserves_the_other_descriptor_and_order() {
        unsafe {
            let (a, _aw) = pipe();
            let (b, _bw) = pipe();
            let a = a.into_raw_fd();
            let b = b.into_raw_fd();
            let mut input = Input::default();
            input.count = 2;
            input.dev[0].fd = a;
            input.dev[1].fd = b;
            input_remove(&mut input, 0);
            assert_eq!(input.count, 1);
            assert_eq!(input.dev[0].fd, b);
            assert_eq!(libc::fcntl(a, libc::F_GETFD), -1);
            assert!(libc::fcntl(b, libc::F_GETFD) >= 0);
            input_close(&mut input);
        }
    }
}
