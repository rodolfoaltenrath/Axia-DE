//! Data-only vectors retained from Monocypher 4.0.2. The original header says
//! they include official vectors and cases generated with libsodium. No C is
//! compiled by this suite. Extra long-AD cases were recorded from the old binary.
use axia_crypto::*;
use serde_json::Value;
use std::ptr;
fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/monocypher-4.0.2.json")).unwrap()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn rows(v: &Value, name: &str) -> Vec<Vec<Vec<u8>>> {
    v[name]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|s| unhex(s.as_str().unwrap()))
                .collect()
        })
        .collect()
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
#[test]
fn blake2b_variable_output_keyed_and_split_messages() {
    unsafe {
        for (index, r) in rows(&fixtures(), "blake2b").iter().enumerate() {
            let (msg, key, expected) = (&r[0], &r[1], &r[2]);
            let mut out = vec![0; expected.len()];
            for split in [0, msg.len() / 2, msg.len()] {
                assert_eq!(
                    axia_crypto_blake2b(
                        out.as_mut_ptr(),
                        out.len(),
                        key.as_ptr(),
                        key.len(),
                        msg.as_ptr(),
                        split,
                        msg[split..].as_ptr(),
                        msg.len() - split
                    ),
                    0
                );
                assert_eq!(&out, expected, "vector {index}, split {split}");
            }
        }
    }
}
#[test]
fn xchacha20poly1305_legacy_ciphertexts_and_in_place() {
    unsafe {
        for (index, r) in rows(&fixtures(), "aead_ietf").iter().enumerate() {
            let (key, nonce, ad, msg, expected) = (&r[0], &r[1], &r[2], &r[3], &r[4]);
            let mut out = vec![0; msg.len()];
            let mut tag = [0; 16];
            assert_eq!(
                axia_crypto_seal(
                    out.as_mut_ptr(),
                    tag.as_mut_ptr(),
                    key.as_ptr(),
                    nonce.as_ptr(),
                    ad.as_ptr(),
                    ad.len(),
                    msg.as_ptr(),
                    msg.len()
                ),
                0
            );
            assert_eq!(tag, expected[..16], "vector {index}");
            assert_eq!(out, expected[16..], "vector {index}");
            let p = out.as_mut_ptr();
            assert_eq!(
                axia_crypto_open(
                    p,
                    tag.as_ptr(),
                    key.as_ptr(),
                    nonce.as_ptr(),
                    ad.as_ptr(),
                    ad.len(),
                    p,
                    out.len()
                ),
                0
            );
            assert_eq!(&out, msg);
            tag[0] ^= 1;
            out.fill(0xa5);
            assert_eq!(
                axia_crypto_open(
                    out.as_mut_ptr(),
                    tag.as_ptr(),
                    key.as_ptr(),
                    nonce.as_ptr(),
                    ad.as_ptr(),
                    ad.len(),
                    expected[16..].as_ptr(),
                    out.len()
                ),
                -1
            );
            assert!(out.iter().all(|b| *b == 0));
        }
    }
}
#[test]
fn chacha20_ietf_counters_and_keystream() {
    unsafe {
        for (index, r) in rows(&fixtures(), "ietf_chacha20").iter().enumerate() {
            let (key, nonce, msg, counter, expected) = (&r[0], &r[1], &r[2], u32le(&r[3]), &r[4]);
            let mut out = vec![0; msg.len()];
            assert_eq!(
                axia_crypto_chacha20(
                    out.as_mut_ptr(),
                    msg.as_ptr(),
                    msg.len(),
                    key.as_ptr(),
                    nonce.as_ptr(),
                    counter
                ),
                0,
                "vector {index}"
            );
            assert_eq!(&out, expected, "vector {index}");
            let p = out.as_mut_ptr();
            assert_eq!(
                axia_crypto_chacha20(p, p, out.len(), key.as_ptr(), nonce.as_ptr(), counter),
                0
            );
            assert_eq!(&out, msg);
            assert_eq!(
                axia_crypto_chacha20(
                    out.as_mut_ptr(),
                    ptr::null(),
                    out.len(),
                    key.as_ptr(),
                    nonce.as_ptr(),
                    counter
                ),
                0
            );
            assert!(out
                .iter()
                .zip(msg.iter().zip(expected))
                .all(|(stream, (plain, cipher))| *stream == plain ^ cipher));
        }
    }
}
#[test]
fn argon2_all_algorithms_lanes_rounding_and_long_associated_data() {
    unsafe {
        for (index, r) in rows(&fixtures(), "argon2").iter().enumerate() {
            let cfg = ArgonConfig {
                algorithm: u32le(&r[0]),
                memory_kib: u32le(&r[1]),
                passes: u32le(&r[2]),
                lanes: u32le(&r[3]),
            };
            let input = ArgonInputs {
                password: r[4].as_ptr(),
                salt: r[5].as_ptr(),
                password_len: r[4].len() as u32,
                salt_len: r[5].len() as u32,
            };
            let extras = ArgonExtras {
                secret: r[6].as_ptr(),
                ad: r[7].as_ptr(),
                secret_len: r[6].len() as u32,
                ad_len: r[7].len() as u32,
            };
            // Deliberately unaligned, as C's malloc/smem is not guaranteed 64-aligned.
            let mut work = vec![0xa5; axia_crypto_argon_work_size(cfg.memory_kib) + 1];
            let mut out = vec![0; r[8].len()];
            assert_eq!(
                axia_crypto_argon2(
                    out.as_mut_ptr(),
                    out.len(),
                    work.as_mut_ptr().add(1),
                    work.len() - 1,
                    cfg,
                    input,
                    extras
                ),
                0,
                "vector {index}"
            );
            assert_eq!(out, r[8], "vector {index}");
            let offset = work.as_ptr().add(1).align_offset(64) + 1;
            let count = (cfg.memory_kib / (4 * cfg.lanes) * (4 * cfg.lanes)) as usize;
            assert!(
                work[offset..offset + count * 1024].iter().all(|b| *b == 0),
                "scratch retained secrets"
            );
        }
    }
}
#[test]
fn rejects_short_scratch_and_overlapping_message_buffers() {
    unsafe {
        let cfg = ArgonConfig {
            algorithm: 2,
            memory_kib: 32,
            passes: 1,
            lanes: 1,
        };
        let pass = b"password";
        let salt = b"1234567890123456";
        let input = ArgonInputs {
            password: pass.as_ptr(),
            salt: salt.as_ptr(),
            password_len: 8,
            salt_len: 16,
        };
        let extras = ArgonExtras {
            secret: ptr::null(),
            ad: ptr::null(),
            secret_len: 0,
            ad_len: 0,
        };
        let mut work = vec![0; 1024];
        let mut out = [0xa5; 32];
        assert_eq!(
            axia_crypto_argon2(
                out.as_mut_ptr(),
                32,
                work.as_mut_ptr(),
                work.len(),
                cfg,
                input,
                extras
            ),
            -1
        );
        let mut data = [0u8; 65];
        let p = data.as_mut_ptr();
        let mut tag = [0u8; 16];
        assert_eq!(
            axia_crypto_seal(
                p.add(1),
                tag.as_mut_ptr(),
                [0; 32].as_ptr(),
                [0; 24].as_ptr(),
                ptr::null(),
                0,
                p,
                64
            ),
            -1
        );
    }
}
