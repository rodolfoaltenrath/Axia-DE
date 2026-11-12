use super::{ffi::*, wire::*};
use std::ptr;

#[test]
fn requests_ownership_and_bounded_failure() {
    let mut out = Output::default();
    out.begin(16, 0);
    out.append(&5u16.to_le_bytes());
    out.pad(2);
    out.bytes(b"WM_S0");
    out.end();
    assert_eq!(out.bytes, b"\x10\0\x04\0\x05\0\0\0WM_S0\0\0\0");
    out.begin(8, 0);
    out.append(&0x400001u32.to_le_bytes());
    out.end();
    out.drop_prefix(16);
    assert_eq!(out.bytes, [8, 0, 2, 0, 1, 0, 64, 0]);
    let cap = out.bytes.capacity();
    out.drop_prefix(usize::MAX);
    assert_eq!(out.bytes.len(), 0);
    assert_eq!(out.bytes.capacity(), cap);
    assert!(!out.room(OUTPUT_LIMIT + 1));
    assert!(out.failed);
    out.append(b"no");
    assert!(out.bytes.is_empty());
    let mut overflow = Output::default();
    overflow.append(b"a");
    assert!(!overflow.room(usize::MAX));
    assert_eq!(overflow.bytes, b"a");
}

#[test]
fn opaque_abi_reset_and_failure() {
    // This test runs under Miri too: no native syscall or compositor is involved.
    let mut out = unsafe { std::mem::zeroed::<XOut>() };
    unsafe {
        assert_eq!(xo_len(&out), 0);
        assert_eq!(xo_failed(&out), 0);
        xo_begin(&mut out, 8, 0);
        xo_u32(&mut out, 55);
        xo_end(&mut out);
        assert_eq!(
            std::slice::from_raw_parts(xo_data(&out), xo_len(&out)),
            [8, 0, 2, 0, 55, 0, 0, 0]
        );
        xo_fail(&mut out);
        xo_u8(&mut out, 1);
        assert_eq!(xo_len(&out), 8);
        assert_eq!(xo_failed(&out), 1);
        xo_free(&mut out);
        xo_free(&mut out);
        assert_eq!(xo_len(&out), 0);
        assert_eq!(xo_failed(&out), 0);
        xo_bytes(&mut out, ptr::null(), usize::MAX);
        assert_eq!(xo_failed(&out), 1);
        xo_free(&mut out);
    }
}

#[test]
fn title_decoder_preserves_legacy_invalid_byte_and_capacity_policy() {
    for (input, latin, expected) in [
        (&b"a\xffb\xc0\xafc"[..], false, "a�b��c"),
        (&b"Caf\xe9"[..], true, "Café"),
        (&b"\xed\xa0\x80ok"[..], false, "ok"), // decoded surrogate is dropped
        (&b"\xe0\x80\x80x"[..], false, "���x"),
        (&b"a\x00tail"[..], false, "a"),
        (&b"line\nbreak\ttab\x7f\xc2\x80"[..], false, "linebreaktab"),
    ] {
        let mut out = [0xcc; 100];
        title(input, latin, &mut out);
        let end = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[..end], expected.as_bytes());
    }
    let mut small = [0x55; 3];
    title("€a".as_bytes(), false, &mut small);
    assert_eq!(small, [b'a', 0, 0x55]); // a code point that does not fit is skipped
    let mut guard = [0x55; 3];
    title(b"abc", false, &mut guard[..0]);
    assert_eq!(guard, [0x55; 3]);
}

#[test]
fn setup_and_packet_lengths_and_sequence_wrap() {
    assert_eq!(setup_request()[..4], [b'l', 0, 11, 0]);
    let mut reply = [0u8; 80];
    reply[0] = 1;
    reply[6..8].copy_from_slice(&18u16.to_le_bytes());
    reply[16..20].copy_from_slice(&0x1fffffu32.to_le_bytes());
    reply[28] = 1;
    reply[40..44].copy_from_slice(&0x221u32.to_le_bytes());
    reply[60..62].copy_from_slice(&2560u16.to_le_bytes());
    reply[62..64].copy_from_slice(&1440u16.to_le_bytes());
    assert_eq!(parse_setup(&reply[..79]).status, 0);
    let s = parse_setup(&reply);
    assert_eq!(s.status, 1);
    assert_eq!(s.used, Some(80));
    assert_eq!(s.setup.unwrap().width, 2560);
    reply[28] = 0;
    let s = parse_setup(&reply);
    assert_eq!(s.status, -1);
    assert_eq!(s.setup.unwrap().id_mask, 0x1fffff);
    assert_eq!(packet_len(&reply[..31]), 0);
    assert_eq!(seq_widen(0x10003, 0xfffe), 0xfffe);
    assert_eq!(seq_widen(0, 0xffff), 0xffff_ffff);
}
