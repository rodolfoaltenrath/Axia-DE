//! PulseAudio protocol serialization without a server: round trip of each type, property lists,
//! descriptors and truncated data (the old server/tests/sound_test.c), through the Rust API and the C ABI.

use axia_sound::pulse::*;
use std::ffi::{c_char, c_void, CStr};

#[test]
fn every_type_round_trips_and_skip_walks_the_same_way() {
    let mut w = Writer::new();
    w.u32(0xdead_beef);
    w.u8(7);
    w.u64(0x0102_0304_0506_0708);
    w.bool(true);
    w.bool(false);
    w.str(Some("hola"));
    w.str(None);
    w.arb(&[1, 2, 3]);
    w.ss(PA_SAMPLE_FLOAT32LE, 2, 48000);
    w.cmap(&[1, 2]);
    w.cvol(&[PA_VOLUME_NORM, PA_VOLUME_NORM / 2]);
    w.proplist(&[("application.name", "LordOS"), ("media.role", "event")]);
    w.format_pcm();
    let packet = w.finish();
    let (len, channel, offset, flags) = descriptor(&packet);
    assert_eq!((len as usize, channel, offset, flags), (packet.len() - PA_DESCRIPTOR_SIZE, 0xffff_ffff, 0, 0));
    let body = &packet[PA_DESCRIPTOR_SIZE..];

    let mut r = Reader::new(body);
    assert_eq!(r.u32(), 0xdead_beef);
    assert_eq!(r.u8(), 7);
    assert_eq!(r.u64(), 0x0102_0304_0506_0708);
    assert!(r.bool() && !r.bool());
    assert_eq!(r.str(), Some(&b"hola"[..]));
    assert_eq!((r.str(), r.err), (None, false));
    assert_eq!(r.arb(), Some(&[1u8, 2, 3][..]));
    assert_eq!(r.ss(), (PA_SAMPLE_FLOAT32LE, 2, 48000));
    let mut map = [0u8; PA_CHANNELS_MAX];
    assert_eq!((r.cmap(&mut map), map[1]), (2, 2));
    let mut vol = [0u32; PA_CHANNELS_MAX];
    assert_eq!((r.cvol(&mut vol), vol[0], vol[1]), (2, PA_VOLUME_NORM, PA_VOLUME_NORM / 2));
    let mut seen = Vec::new();
    r.proplist(|k, v, terminated| seen.push((k.to_vec(), v.to_vec(), terminated)));
    assert_eq!(seen, [(b"application.name".to_vec(), b"LordOS".to_vec(), true), (b"media.role".to_vec(), b"event".to_vec(), true)]);
    r.skip_format();
    assert!(!r.err && r.eof());

    // skip walks everything like the typed reads
    let mut r = Reader::new(body);
    let mut count = 0;
    while !r.eof() && !r.err {
        r.skip();
        count += 1;
    }
    assert!(!r.err);
    assert_eq!(count, 13);

    // truncated: every prefix fails without reading outside the buffer
    let mut errors = 0;
    for cut in 0..body.len() {
        let mut r = Reader::new(&body[..cut]);
        while !r.eof() && !r.err {
            r.skip();
        }
        errors += usize::from(r.err);
    }
    assert!(errors > 0);

    // wrong type
    let mut r = Reader::new(body);
    r.str();
    assert!(r.err);
    assert_eq!(r.u32(), 0, "after an error every read gives zero");
}

#[test]
fn data_packets_carry_their_channel_and_bytes() {
    let packet = data_packet(5, b"abcd");
    let (len, channel, offset, flags) = descriptor(&packet);
    assert_eq!((len, channel, offset, flags, packet.len()), (4, 5, 0, 0, PA_DESCRIPTOR_SIZE + 4));
    assert_eq!(&packet[PA_DESCRIPTOR_SIZE..], b"abcd");
}

#[test]
fn an_unterminated_property_is_reported_as_it_came_and_a_value_over_512_bytes_is_skipped() {
    let mut raw = vec![b'P'];
    for (key, value) in [(&b"k1"[..], &b"abc"[..]), (b"k2", &[b'x'; 600][..]), (b"k3", b"")] {
        raw.push(b't');
        raw.extend(key);
        raw.push(0);
        raw.push(b'L');
        raw.extend((value.len() as u32).to_be_bytes());
        raw.push(b'x');
        raw.extend((value.len() as u32).to_be_bytes());
        raw.extend(value);
    }
    raw.push(b'N');
    let mut seen = Vec::new();
    let mut r = Reader::new(&raw);
    r.proplist(|k, v, t| seen.push((k.to_vec(), v.len(), t)));
    assert!(!r.err);
    assert_eq!(seen, [(b"k1".to_vec(), 3, false), (b"k3".to_vec(), 0, false)]);
    // a length that disagrees with the payload is an error
    let mut bad = raw.clone();
    bad[6] ^= 1;
    let mut r = Reader::new(&bad);
    r.proplist(|_, _, _| {});
    assert!(r.err);
}

unsafe extern "C" fn collect(ud: *mut c_void, key: *const c_char, val: *const c_char, len: u32) {
    let seen = &mut *(ud as *mut Vec<(String, String, u32)>);
    seen.push((CStr::from_ptr(key).to_string_lossy().into_owned(), CStr::from_ptr(val).to_string_lossy().into_owned(), len));
}

#[test]
fn the_c_abi_writes_the_same_bytes_and_reads_them_back() {
    unsafe {
        let mut out = std::mem::zeroed::<TsOut>();
        ts_init(&mut out);
        ts_u32(&mut out, 0xdead_beef);
        ts_u8(&mut out, 7);
        ts_u64(&mut out, 0x0102_0304_0506_0708);
        ts_bool(&mut out, 1);
        ts_str(&mut out, c"hola".as_ptr());
        ts_str(&mut out, std::ptr::null());
        ts_arb(&mut out, [1u8, 2, 3].as_ptr() as *const c_void, 3);
        ts_ss(&mut out, PA_SAMPLE_FLOAT32LE, 2, 48000);
        ts_cmap(&mut out, 2, [1u8, 2].as_ptr());
        ts_cvol(&mut out, 2, [PA_VOLUME_NORM, PA_VOLUME_NORM / 2].as_ptr());
        let kv = [c"application.name".as_ptr(), c"LordOS".as_ptr(), c"media.role".as_ptr(), c"event".as_ptr()];
        ts_proplist(&mut out, kv.as_ptr(), 2);
        ts_format_pcm(&mut out);
        let total = ts_finish(&mut out);

        let mut w = Writer::new();
        w.u32(0xdead_beef);
        w.u8(7);
        w.u64(0x0102_0304_0506_0708);
        w.bool(true);
        w.str(Some("hola"));
        w.str(None);
        w.arb(&[1, 2, 3]);
        w.ss(PA_SAMPLE_FLOAT32LE, 2, 48000);
        w.cmap(&[1, 2]);
        w.cvol(&[PA_VOLUME_NORM, PA_VOLUME_NORM / 2]);
        w.proplist(&[("application.name", "LordOS"), ("media.role", "event")]);
        w.format_pcm();
        let expected = w.finish();
        let bytes = std::slice::from_raw_parts(out.d(), total);
        assert_eq!(bytes, &expected[..]);

        let mut t = std::mem::zeroed::<TsIn>();
        ts_in(&mut t, bytes.as_ptr().add(PA_DESCRIPTOR_SIZE), total - PA_DESCRIPTOR_SIZE);
        assert_eq!(ts_get_u32(&mut t), 0xdead_beef);
        assert_eq!(ts_get_u8(&mut t), 7);
        assert_eq!(ts_get_u64(&mut t), 0x0102_0304_0506_0708);
        assert_eq!(ts_get_bool(&mut t), 1);
        assert_eq!(CStr::from_ptr(ts_get_str(&mut t)).to_bytes(), b"hola");
        assert!(ts_get_str(&mut t).is_null());
        let mut n = 0u32;
        let a = ts_get_arb(&mut t, &mut n);
        assert_eq!((n, *a.add(2)), (3, 3));
        let (mut f, mut c, mut r) = (0u8, 0u8, 0u32);
        ts_get_ss(&mut t, &mut f, &mut c, &mut r);
        assert_eq!((f, c, r), (PA_SAMPLE_FLOAT32LE, 2, 48000));
        let mut map = [0u8; PA_CHANNELS_MAX];
        assert_eq!(ts_get_cmap(&mut t, map.as_mut_ptr()), 2);
        let mut vol = [0u32; PA_CHANNELS_MAX];
        assert_eq!(ts_get_cvol(&mut t, vol.as_mut_ptr()), 2);
        let mut seen: Vec<(String, String, u32)> = Vec::new();
        ts_get_proplist(&mut t, Some(collect), &mut seen as *mut _ as *mut c_void);
        assert_eq!(seen, [("application.name".into(), "LordOS".into(), 6), ("media.role".into(), "event".into(), 5)]);
        ts_skip_format(&mut t);
        assert_eq!(ts_eof(&t), 1);
        ts_free(&mut out);
    }
}
