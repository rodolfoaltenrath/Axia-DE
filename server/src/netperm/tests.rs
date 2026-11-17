use super::*;
#[allow(dead_code)]
#[path = "elf.rs"]
mod elf;
#[test]
fn policy_files_are_strict_and_atomic() {
    let good=br#"{"v":1,"mode":"deny","trusted":["desktop:ok"],"restricted":{"desktop:no":["no-network"]},"rules":[{"app":"desktop:ok","dst":"example.com","action":"allow","port":"443","proto":"tcp"}]}"#;
    let s = policy::Store::parse(good).unwrap();
    unsafe {
        assert_eq!(fwrules::fw_mode(s.0), 3);
        assert_eq!(fwrules::fw_rule_count(s.0), 1);
        assert_eq!(fwrules::fw_restricted(s.0, c"desktop:no".as_ptr()), 4)
    }
    assert!(policy::Store::parse(
        br#"{"v":1.0,"rules":[{"app":"x","dst":"any","action":"allow","port":443.0}]}"#
    )
    .is_ok());
    for bad in [
        br#"{"v":1,"mode":null}"#.as_slice(),
        br#"{"v":1,"rules":[{"app":"x","dst":"any","action":"allow","port":null}]}"#,
        br#"{"v":1,"mode":"deny","mode":"off"}"#.as_slice(),
        br#"{"v":1,"unexpected":1}"#,
        br#"{"v":1,"trusted":["x","x"]}"#,
        br#"{"v":1,"trusted":["x"],"restricted":{"x":["no-network"]}}"#,
        br#"{"v":1,"restricted":{"x":["no-network","no-network"]}}"#,
        br#"{"v":1,"rules":[{"app":"x","dst":"any","action":"allow","port":"01"}]}"#,
        br#"{"v":1,"rules":[{"app":"x","dst":"any","action":"allow","port":0}]}"#,
    ] {
        assert!(
            policy::Store::parse(bad).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}
#[test]
fn malformed_netlink_messages_and_attributes_are_bounded() {
    for n in 0..100 {
        let b = vec![0xff; n];
        let _ = diag::messages(&b);
        let _ = diag::decode(&b, 6);
    }
    let mut msg = vec![0; 100];
    msg[..4].copy_from_slice(&100u32.to_ne_bytes());
    msg[16] = 2;
    msg[17] = 1;
    msg[20..22].copy_from_slice(&443u16.to_be_bytes());
    msg[22..24].copy_from_slice(&1234u16.to_be_bytes());
    msg[88..90].copy_from_slice(&12u16.to_ne_bytes());
    msg[90..92].copy_from_slice(&21u16.to_ne_bytes());
    msg[92..100].copy_from_slice(&42u64.to_ne_bytes());
    let s = diag::decode(&msg, 6).unwrap();
    assert_eq!((s.sport, s.dport, s.cgid), (443, 1234, 42));
    for n in 0..msg.len() {
        let _ = diag::decode(&msg[..n], 6);
    }
}
#[test]
fn btf_lookup_checks_bounds_and_type_lengths() {
    let mut b = vec![0; 24];
    b[..4].copy_from_slice(&[0x9f, 0xeb, 1, 0]);
    b[4..8].copy_from_slice(&24u32.to_le_bytes());
    b[12..16].copy_from_slice(&12u32.to_le_bytes());
    b[16..20].copy_from_slice(&12u32.to_le_bytes());
    b[20..24].copy_from_slice(&6u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(12u32 << 24).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(b"\0hook\0");
    assert_eq!(bpf::btf(&b, "hook"), Some(1));
    assert_eq!(bpf::btf(&b, "hoo"), None);
    for n in 0..b.len() {
        assert_eq!(bpf::btf(&b[..n], "hook"), None)
    }
    b[28..32].copy_from_slice(&(4u32 << 24 | 1).to_le_bytes());
    assert_eq!(bpf::btf(&b, "hook"), None);
}
#[cfg(feature = "bpf")]
#[test]
fn embedded_elf_contains_every_hook_and_only_map_relocations() {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/kernel.o"));
    let p = elf::pack(bytes).unwrap();
    assert_eq!(p.len(), bpf::SPECS.len());
    for spec in bpf::SPECS {
        let p = p.iter().find(|p| p.section == spec.section).unwrap();
        assert!(!p.insns.is_empty());
        assert!(p.relocs.iter().all(|(i, _)| p.insns[*i] & 255 == 0x18));
    }
    for n in [0, 4, 20, 60, bytes.len() - 1] {
        assert!(elf::pack(&bytes[..n]).is_err())
    }
}
#[test]
fn malformed_elf_never_panics() {
    let mut state = 7u64;
    for len in 0..300 {
        let mut b = vec![0; len];
        for c in &mut b {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *c = state as u8;
        }
        assert!(elf::pack(&b).is_err());
    }
}
