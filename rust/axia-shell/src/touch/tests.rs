use super::*;
use std::mem::zeroed;
#[test]
fn acceleration_endpoints_and_missing_resolution_geometry_are_preserved() {
    assert_eq!(tp_accel(0.0, 0.5), 2.4);
    assert_eq!(tp_accel(300.0, 0.5), 15.0);
    assert_eq!(tp_accel(300.0, 1.0), 30.0);
    assert_eq!(tp_accel(300.0, -1.0), 7.5);
    assert!(tp_accel(60.0, 0.5) > tp_accel(5.0, 0.5));
    let caps = Caps {
        kind: TOUCHPAD,
        nslots: i32::MAX,
        ..Caps::default()
    };
    let device = Device::new(caps);
    assert_eq!(device.slots(), MAX_SLOTS);
    assert_eq!(device.wmm, 100.0);
    assert!(device.s.iter().all(|slot| slot.id == -1));
    let caps = Caps {
        kind: TOUCHSCREEN,
        nslots: -1,
        ..Caps::default()
    };
    let device = Device::new(caps);
    assert_eq!(device.slots(), 1);
    assert_eq!(device.wmm, 300.0);
}
struct Capture {
    device: *mut Device,
    buttons: usize,
}
unsafe extern "C" fn button(context: *mut c_void, _: i32, _: i32) {
    let capture = &mut *(context as *mut Capture);
    capture.buttons += 1;
    // Reentrant metadata access must not alias the state machine's mutable state.
    assert_eq!(tp_count(0), 1);
    assert_eq!((*tp_caps(capture.device)).kind, TOUCHPAD);
    let mut json = [0i8; 512];
    let size = tp_list_json(json.as_mut_ptr(), json.len() as i32);
    assert!(size > 0);
    assert_eq!(config().speed, 0.5);
}
unsafe fn event(device: *mut Device, type_: u16, code: u16, value: i32, time: f64) {
    let mut event: libc::input_event = zeroed();
    event.type_ = type_;
    event.code = code;
    event.value = value;
    tp_event_at(device, &event, time);
}
#[test]
fn process_contract() {
    if std::env::var_os("AXIA_TOUCH_DRIVER_TEST_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "touch::tests::process_contract", "--nocapture"])
            .env("AXIA_TOUCH_DRIVER_TEST_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    unsafe {
        let mut cfg = Config::default();
        cfg.speed = -10.0;
        cfg.secondary = 10;
        tp_set_config(&cfg);
        assert_eq!(config().speed, 0.0);
        assert_eq!(config().secondary, 0);
        cfg.speed = 10.0;
        tp_set_config(&cfg);
        assert_eq!(config().speed, 1.0);
        tp_set_config(&Config::default());
        let mut caps = Caps {
            kind: TOUCHPAD,
            nslots: 1,
            ..Caps::default()
        };
        caps.x.maximum = 10000;
        caps.y.maximum = 6000;
        caps.x.resolution = 100;
        caps.y.resolution = 100;
        caps.name[..8].copy_from_slice(b"a\"b\\c\x01d\0");
        let device = tp_new(&caps);
        assert!(!device.is_null());
        let stable_caps = tp_caps(device);
        let mut others = Vec::new();
        for _ in 1..MAX_DEVICES {
            others.push(tp_new(&caps));
        }
        assert!(tp_new(&caps).is_null());
        assert_eq!(tp_count(TOUCHPAD), 16);
        assert_eq!((*stable_caps).nslots, 1);
        for other in others {
            tp_free(other);
        }
        assert_eq!(tp_count(0), 1);
        let mut json = [0i8; 512];
        let length = tp_list_json(json.as_mut_ptr(), 512);
        let bytes = std::slice::from_raw_parts(json.as_ptr().cast::<u8>(), length as usize);
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value[0]["name"], "abcd");
        let mut capture = Capture { device, buttons: 0 };
        let sink = Sink {
            button: Some(button),
            ..Sink::default()
        };
        tp_set_sink(&sink, &mut capture as *mut Capture as *mut c_void);
        event(device, 3, 0x2f, 99, 1000.0);
        assert_eq!((*device).cur, 0);
        event(device, 3, 0x39, 1, 1000.0);
        event(device, 3, 0x35, 5000, 1000.0);
        event(device, 3, 0x36, 2000, 1000.0);
        event(device, 0, 0, 0, 1000.0);
        event(device, 0, 3, 0, 1010.0);
        event(device, 3, 0x35, 9000, 1010.0);
        event(device, 0, 0, 0, 1010.0);
        assert_eq!((*device).s[0].x, 5000);
        event(device, 3, 0x39, -1, 1020.0);
        event(device, 0, 0, 0, 1020.0);
        assert_eq!(capture.buttons, 1);
        assert_eq!(tp_timeout(1020.0), 180.0);
        tp_tick(1200.0);
        assert_eq!(capture.buttons, 2);
        assert_eq!(tp_timeout(1200.0), -1.0);
        tp_set_sink(ptr::null(), ptr::null_mut());
        tp_free(device);
        tp_free(ptr::null_mut());
        assert_eq!(tp_count(0), 0);
        assert!(device::tp_open(-1, ptr::null()).is_null());
    }
}
