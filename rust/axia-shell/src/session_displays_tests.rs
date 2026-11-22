use super::*;
use std::ffi::CString;

#[test]
fn compositor_events_publish_only_physical_changes_and_ignore_invalid_ffi_input() {
    let source = reader();
    let event = json!({"displays":[{"id":7,"name":"Panel","connector":"DP-1",
        "x":-800,"y":0,"w":800,"h":600,"scale":1}]});
    let input = CString::new(event.to_string()).unwrap();
    assert_eq!(unsafe { axia_session_displays_update(input.as_ptr()) }, 1);
    let revision = source.revision();
    let changes = source.subscribe(); changes.take();
    let mut equivalent = event.clone(); equivalent["displays"][0]["scale"] = json!(2);
    assert_eq!(update(&equivalent), Ok(false));
    assert!(!changes.pending());
    for bytes in [vec![0xff,0], b"{bad}\0".to_vec(), vec![b'x';65_538]] {
        let mut bytes = bytes; if *bytes.last().unwrap()!=0 { bytes.push(0); }
        assert_eq!(unsafe { axia_session_displays_update(bytes.as_ptr().cast()) }, -1);
    }
    assert_eq!(unsafe { axia_session_displays_update(std::ptr::null()) }, -1);
    assert_eq!(source.revision(), revision);
    let mut main = event.clone(); main["displays"][0]["main"] = json!(true);
    assert_eq!(update(&main), Ok(true));
    assert!(changes.pending()); changes.take();
    assert_eq!(source.read()[0]["main"], true);
    assert_eq!(update(&main), Ok(false));
    assert!(!changes.pending());
    let mut moved = event; moved["displays"][0]["x"] = json!(0);
    assert_eq!(update(&moved), Ok(true));
    assert!(changes.pending());
    assert_eq!(source.read()[0]["x"], 0.);
    assert_eq!(source.read()[0]["name"], "Panel");
    assert!(source.read()[0].get("scale").is_none());
}

#[test]
fn physical_source_bounds_display_count_and_rejects_unusable_frames() {
    let mut displays:Vec<_> = (0..32).map(|i|json!({"id":i,"x":i*800,"y":0,"w":800,"h":600})).collect();
    displays[2]["w"] = json!(0);
    displays[3]["x"] = Value::Null;
    assert!(update(&json!({"displays":displays})).is_ok());
    let source = reader();
    assert_eq!(source.read().len(), 14);
    assert_eq!(source.read().last().unwrap()["id"], 15);
    let revision = source.revision();
    assert_eq!(update(&json!({"displays":"invalid"})), Err(()));
    assert_eq!(source.revision(), revision);
    assert_eq!(update(&json!({"displays":[]})), Ok(true));
    assert!(source.read().is_empty());
}
