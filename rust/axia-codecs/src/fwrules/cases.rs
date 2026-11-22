//! Per-app network rules (docs/FIREWALL.md): ids, destinations, LAN, precedence, v4/v6 networks, wildcards,
//! durations, restrictions, the JSON file (round trip and rejection) and a random test against an obvious
//! linear version of the decider. This is what server/tests/fw_rules_test.c checked through the C ABI.
use super::*;
use crate::json;
use std::ffi::{CStr, CString};
use std::net::IpAddr;

const A: &str = "desktop:org.chromium.Chromium";

fn mk(app: &str, dir: c_int, dst: &str, act: c_int) -> FwRule {
    let mut r = zeroed_rule();
    set(&mut r.app, app.as_bytes());
    r.dir = dir;
    r.action = act;
    assert!(parse_dest(dst.as_bytes(), &mut r), "invalid destination in the test: {dst}");
    r
}

fn put(s: &mut FwStore, r: &FwRule) -> Option<u32> {
    validate(r).and_then(|v| s.add(v))
}

fn add(s: &mut FwStore, app: &str, dir: c_int, dst: &str, act: c_int) -> u32 {
    put(s, &mk(app, dir, dst, act)).unwrap_or_else(|| panic!("could not add {app} {dst}"))
}

fn qry(app: &str, dir: c_int, ip: Option<&str>, port: u16) -> FwQuery {
    let mut q = FwQuery { app: ptr::null(), dir, proto: FW_PROTO_TCP, fam: 0, addr: [0; 16], port, hosts: ptr::null(), nhosts: 0, listed: 0 };
    let _ = app;
    match ip.map(|i| i.parse::<IpAddr>().unwrap()) {
        Some(IpAddr::V4(a)) => {
            q.fam = 4;
            q.addr[..4].copy_from_slice(&a.octets());
        }
        Some(IpAddr::V6(a)) => {
            q.fam = 6;
            q.addr = a.octets();
        }
        None => {}
    }
    q
}

/// What fw_decide does with the app (an invalid one is denied) and the names.
fn decide(s: &mut FwStore, app: &str, q: &FwQuery, hosts: &[&str]) -> FwDecision {
    let Some(c) = app_canon(app.as_bytes()) else {
        return FwDecision { verdict: FW_V_DENY, reason: FW_BY_RESTRICTED, rule_id: 0, flag: FW_R_NO_NETWORK, record: 0 };
    };
    let h: Vec<Option<&[u8]>> = hosts.iter().map(|x| Some(x.as_bytes())).collect();
    s.decide(q, &c, &h)
}

fn dec(s: &mut FwStore, app: &str, ip: Option<&str>, port: u16, host: Option<&str>) -> FwDecision {
    let q = qry(app, FW_OUT, ip, port);
    let hosts: Vec<&str> = host.into_iter().collect();
    decide(s, app, &q, &hosts)
}

fn decl(s: &mut FwStore, app: &str, ip: &str, host: Option<&str>, listed: c_int) -> FwDecision {
    let mut q = qry(app, FW_OUT, Some(ip), 443);
    q.listed = listed;
    let hosts: Vec<&str> = host.into_iter().collect();
    decide(s, app, &q, &hosts)
}

fn canon_of(inp: &str, n: usize) -> Option<String> {
    let c = CString::new(inp).unwrap();
    let mut out = vec![0 as c_char; n.max(1)];
    // SAFETY: `out` has room for `n` bytes.
    let r = unsafe { fw_app_canon(c.as_ptr(), out.as_mut_ptr(), n) };
    (r == 0).then(|| unsafe { CStr::from_ptr(out.as_ptr()) }.to_str().unwrap().to_string())
}

fn scope_of(inp: &str) -> Option<String> {
    let c = CString::new(inp).unwrap();
    let mut out = [0 as c_char; FW_APP_MAX];
    // SAFETY: `out` has room for FW_APP_MAX bytes.
    let r = unsafe { fw_component_scope(c.as_ptr(), out.as_mut_ptr(), out.len()) };
    (r == 0).then(|| unsafe { CStr::from_ptr(out.as_ptr()) }.to_str().unwrap().to_string())
}


// ---- ids, destinations, LAN ----

#[test]
fn app_ids() {
    for (inp, want) in [
        ("lordos", "lordos"),
        ("desktop:org.chromium.Chromium", "desktop:org.chromium.Chromium"),
        ("app-lordos-lpk-sudoku.scope", "lpk:sudoku"),
        ("app-lordos-org.mozilla.firefox.scope", "desktop:org.mozilla.firefox"),
        ("/usr/bin/curl", "/usr/bin/curl"),
        ("lordos:store", "lordos:store"),
    ] {
        assert_eq!(canon_of(inp, FW_APP_MAX).as_deref(), Some(want), "{inp}");
    }
    for bad in ["", "desktop:", ":x", "a b", "a\"b", "/usr/../bin/sh", "/usr//bin", "/usr/bin/", "rel/path", "app-lordos-lpk-a:b.scope", "x\n", "\u{e1}"] {
        assert_eq!(canon_of(bad, FW_APP_MAX), None, "{bad:?}");
    }
    // the components' scopes
    assert_eq!(scope_of("lordos-net-mail.scope").as_deref(), Some("lordos:mail"));
    assert_eq!(scope_of("lordos-net-terminal.scope").as_deref(), Some("lordos:terminal"));
    for bad in [
        "lordos-net-evil.scope", "lordos-net-.scope", "lordos-net-mail", "lordos-net-mail.scope.scope", "lordos-maild.scope",
        "app-lordos-mail.scope", "lordos-net-Mail.scope",
    ] {
        assert_eq!(scope_of(bad), None, "{bad}");
    }
    assert!(component_ok(b"lists") && !component_ok(b"") && !component_ok(b"vpn"));
    assert_eq!(unsafe { fw_component_ok(ptr::null()) }, 0);
    assert_eq!(canon_of(&"a".repeat(199), FW_APP_MAX), None);
    assert_eq!(canon_of("lordos", 3), None);
}

#[test]
fn destinations() {
    for (inp, want) in [
        ("any", "any"), ("lan", "lan"), ("internet", "internet"),
        ("Example.COM.", "example.com"), ("*.example.com", "*.example.com"), ("_dmarc.x.org", "_dmarc.x.org"),
        ("10.0.0.0/8", "10.0.0.0/8"), ("192.0.2.7", "192.0.2.7"), ("192.0.2.7/32", "192.0.2.7"),
        ("0.0.0.0/0", "0.0.0.0/0"), ("2001:db8::/32", "2001:db8::/32"), ("2001:DB8::1", "2001:db8::1"),
        ("::ffff:10.1.2.0/120", "10.1.2.0/24"), ("::/0", "::/0"),
    ] {
        let mut r = zeroed_rule();
        assert!(parse_dest(inp.as_bytes(), &mut r), "{inp}");
        assert_eq!(format_dest(&r).unwrap(), want.as_bytes(), "{inp}");
    }
    for bad in [
        "", "*", "*.", "*example.com", "a..b", ".a", "a.b.123", "1.2.3.400", "1.2.3", "10.0.0.1/8", "10.0.0.0/33", "10.0.0.0/08", "10.0.0.0/",
        "2001:db8::1/129", "2001:db8::1/64", "ex ample.com", "a.-b.c\x01", "*.*.com", "\u{f1}.com", "1.2.3.4/-1", "::1::2",
    ] {
        let mut r = zeroed_rule();
        assert!(!parse_dest(bad.as_bytes(), &mut r), "accepted invalid destination: {bad:?}");
    }
    let mut r = zeroed_rule();
    assert!(!parse_dest(format!("{}.com", "a".repeat(64)).as_bytes(), &mut r), "64-byte label");
    let lab = "a".repeat(63);
    assert!(!parse_dest(format!("{lab}.{lab}.{lab}.{lab}").as_bytes(), &mut r), "255 > 253");
}

fn lan_of(ip: &str) -> bool {
    match ip.parse::<IpAddr>().unwrap() {
        IpAddr::V4(a) => {
            let mut b = [0u8; 16];
            b[..4].copy_from_slice(&a.octets());
            addr_is_lan(4, &b)
        }
        IpAddr::V6(a) => addr_is_lan(6, &a.octets()),
    }
}

#[test]
fn lan_addresses() {
    for ip in [
        "10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.1.1", "127.0.0.1", "169.254.3.4", "100.64.0.1", "224.0.0.251", "255.255.255.255", "0.0.0.0", "::1", "::",
        "fe80::1", "fd12:3456::1", "fc00::1", "ff02::fb", "::ffff:192.168.0.1",
    ] {
        assert!(lan_of(ip), "{ip}");
    }
    for ip in ["8.8.8.8", "172.15.255.255", "172.32.0.0", "192.169.0.1", "100.128.0.1", "1.1.1.1", "2001:4860:4860::8888", "fec0::1", "::ffff:8.8.8.8", "11.0.0.1", "223.255.255.255"] {
        assert!(!lan_of(ip), "{ip}");
    }
}

// ---- precedence ----

#[test]
fn mode_and_trusted() {
    let mut s = FwStore::new();
    assert_eq!(s.mode, FW_MODE_SILENT);
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_MODE && d.record == 1);
    s.mode = FW_MODE_ASK;
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_ASK && d.reason == FW_BY_MODE && d.record == 0);
    s.mode = FW_MODE_OFF;
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_MODE && d.record == 0);
    unsafe { fw_set_mode(&mut s, 7) }; // invalid: no change
    assert_eq!(s.mode, FW_MODE_OFF);

    s.mode = FW_MODE_ASK;
    assert!(s.set_trusted(b"desktop:a", true));
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_TRUSTED && d.record == 0);
    // an explicit rule still applies to a trusted app
    add(&mut s, "desktop:a", FW_OUT, "8.8.8.8", FW_DENY);
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_RULE);
    // trusting and restricting are mutually exclusive
    assert!(s.set_restricted(b"desktop:a", 1 << 4));
    let app = |s: &FwStore| s.apps.get(&b"desktop:a"[..]).map(|a| (a.trusted, a.restricted));
    assert_eq!(app(&s), Some((false, 1 << 4)));
    assert!(s.set_trusted(b"desktop:a", true));
    assert_eq!(app(&s), Some((true, 0)));
    let id = CString::new("desktop:a").unwrap();
    assert_eq!(unsafe { fw_set_restricted(&mut s, id.as_ptr(), 1 << 20) }, -1);
    let bad = CString::new("bad id").unwrap();
    assert_eq!(unsafe { fw_set_trusted(&mut s, bad.as_ptr(), 1) }, -1);
    // without a valid app: denied
    assert_eq!(dec(&mut s, "", Some("8.8.8.8"), 443, None).verdict, FW_V_DENY);
    // the cgroup scope is the same app
    let d = dec(&mut s, "app-lordos-a.scope", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_RULE);
}

#[test]
fn deny_mode_and_lists() {
    let mut s = FwStore::new();
    s.mode = FW_MODE_DENY;
    let d = dec(&mut s, "desktop:a", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_MODE && d.record == 1);
    let d = dec(&mut s, "desktop:a", Some("192.168.1.1"), 443, None);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_MODE);
    let d = dec(&mut s, "desktop:a", Some("127.0.0.53"), 53, None); // the resolver's stub: loopback stays
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_MODE);
    assert_eq!(dec(&mut s, "desktop:a", Some("::1"), 631, None).verdict, FW_V_ALLOW);
    assert_eq!(dec(&mut s, "desktop:a", Some("::ffff:127.0.0.1"), 631, None).verdict, FW_V_ALLOW);
    add(&mut s, "desktop:a", FW_OUT, "example.org", FW_ALLOW); // a rule still allows
    let d = dec(&mut s, "desktop:a", Some("203.0.113.5"), 443, Some("example.org"));
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_RULE);
    assert!(s.set_trusted(b"desktop:t", true)); // trusted apps keep the network
    let d = dec(&mut s, "desktop:t", Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_TRUSTED);
    // the mode survives the file
    let j = file::to_json(&s).unwrap();
    assert!(String::from_utf8_lossy(&j).contains("\"mode\":\"deny\""));
    let mut t = FwStore::new();
    file::from_json(&mut t, &j).unwrap();
    assert_eq!(t.mode, FW_MODE_DENY);

    // lists: deny before lan/internet/any rules, never before a rule naming the destination
    let mut s = FwStore::new();
    let a = "desktop:b";
    let d = decl(&mut s, a, "203.0.113.9", Some("tracker.example"), 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_LIST && d.record == 1);
    let d = decl(&mut s, a, "203.0.113.9", Some("tracker.example"), 0);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_MODE);
    add(&mut s, a, FW_OUT, "any", FW_ALLOW);
    add(&mut s, a, FW_OUT, "internet", FW_ALLOW);
    let d = decl(&mut s, a, "203.0.113.9", Some("tracker.example"), 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_LIST, "broad rules don't override a list");
    let mine = add(&mut s, a, FW_OUT, "tracker.example", FW_ALLOW);
    let d = decl(&mut s, a, "203.0.113.9", Some("tracker.example"), 1);
    assert!(d.verdict == FW_V_ALLOW && d.reason == FW_BY_RULE && d.rule_id == mine, "the owner's allow wins");
    let net = add(&mut s, a, FW_OUT, "198.51.100.0/24", FW_ALLOW);
    let d = decl(&mut s, a, "198.51.100.7", None, 1);
    assert!(d.verdict == FW_V_ALLOW && d.rule_id == net);
    add(&mut s, a, FW_OUT, "*.ads.example", FW_DENY);
    let d = decl(&mut s, a, "203.0.113.10", Some("x.ads.example"), 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_RULE, "the owner's own deny still says so");
    assert!(s.set_trusted(a.as_bytes(), true));
    let d = decl(&mut s, "desktop:c", "203.0.113.9", None, 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_LIST);
    assert!(s.set_trusted(b"desktop:c", true)); // lists apply to trusted apps too
    let d = decl(&mut s, "desktop:c", "203.0.113.9", None, 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_LIST);
    assert!(s.set_restricted(b"desktop:d", FW_R_NO_INTERNET)); // a restriction comes first
    let d = decl(&mut s, "desktop:d", "203.0.113.9", None, 1);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_RESTRICTED);
}

fn list_of(s: &FwStore, app: &str) -> Vec<FwRule> {
    let c = CString::new(app).unwrap();
    let mut out = vec![zeroed_rule(); 64];
    // SAFETY: `out` has room for 64 rules.
    let n = unsafe { fw_rule_list(s, c.as_ptr(), out.as_mut_ptr(), 64) };
    out.truncate(n.max(0) as usize);
    out
}

#[test]
fn precedence() {
    let mut s = FwStore::new();
    s.mode = FW_MODE_ASK;

    // allow and deny for the same destination: the last decision replaces it (same id)
    let i1 = add(&mut s, A, FW_OUT, "any", FW_ALLOW);
    let i2 = add(&mut s, A, FW_OUT, "any", FW_DENY);
    assert!(i1 == i2 && s.ids.len() == 1);
    assert_eq!(dec(&mut s, A, Some("1.1.1.1"), 80, None).verdict, FW_V_DENY);

    // the most specific wins: allowed domain over "deny all"
    let dom = add(&mut s, A, FW_OUT, "api.example.com", FW_ALLOW);
    let d = dec(&mut s, A, Some("93.184.216.34"), 443, Some("api.example.com"));
    assert!(d.verdict == FW_V_ALLOW && d.rule_id == dom);
    assert_eq!(dec(&mut s, A, Some("93.184.216.34"), 443, Some("www.example.com")).verdict, FW_V_DENY);

    // an exact domain beats the wildcard, and a longer suffix beats a shorter one
    let w1 = add(&mut s, A, FW_OUT, "*.example.com", FW_DENY);
    let w2 = add(&mut s, A, FW_OUT, "*.cdn.example.com", FW_ALLOW);
    let ip = Some("93.184.216.34");
    assert_eq!(dec(&mut s, A, ip, 443, Some("api.example.com")).rule_id, dom);
    assert_eq!(dec(&mut s, A, ip, 443, Some("x.example.com")).rule_id, w1);
    assert_eq!(dec(&mut s, A, ip, 443, Some("example.com")).rule_id, w1, "the wildcard covers the suffix");
    assert_eq!(dec(&mut s, A, ip, 443, Some("img.cdn.example.com")).rule_id, w2);
    assert_eq!(dec(&mut s, A, ip, 443, Some("API.Example.COM.")).rule_id, dom);
    assert_eq!(dec(&mut s, A, ip, 443, Some("notexample.com")).verdict, FW_V_DENY, "any");
    assert_eq!(dec(&mut s, A, ip, 443, Some("notexample.com")).rule_id, i1);

    // networks: the smallest wins; with a domain and a network at once, the domain wins
    let c8 = add(&mut s, A, FW_OUT, "10.0.0.0/8", FW_ALLOW);
    let c24 = add(&mut s, A, FW_OUT, "10.1.2.0/24", FW_DENY);
    let c32 = add(&mut s, A, FW_OUT, "10.1.2.3", FW_ALLOW);
    assert_eq!(dec(&mut s, A, Some("10.9.9.9"), 80, None).rule_id, c8);
    assert_eq!(dec(&mut s, A, Some("10.1.2.9"), 80, None).rule_id, c24);
    assert_eq!(dec(&mut s, A, Some("10.1.2.3"), 80, None).rule_id, c32);
    assert_eq!(dec(&mut s, A, Some("::ffff:10.1.2.9"), 80, None).rule_id, c24, "mapped IPv4");
    assert_eq!(dec(&mut s, A, Some("10.1.2.9"), 80, Some("api.example.com")).rule_id, dom);
    let v6a = add(&mut s, A, FW_OUT, "2001:db8::/32", FW_DENY);
    let v6b = add(&mut s, A, FW_OUT, "2001:db8:1::/48", FW_ALLOW);
    let v6c = add(&mut s, A, FW_OUT, "::/0", FW_ALLOW);
    assert_eq!(dec(&mut s, A, Some("2001:db8:2::1"), 80, None).rule_id, v6a);
    assert_eq!(dec(&mut s, A, Some("2001:db8:1::1"), 80, None).rule_id, v6b);
    assert_eq!(dec(&mut s, A, Some("2a00::1"), 80, None).rule_id, v6c);
    assert_eq!(dec(&mut s, A, Some("2a00::1"), 80, None).verdict, FW_V_ALLOW);

    // LAN / internet sit between the network and "any"
    let lan = add(&mut s, A, FW_OUT, "lan", FW_ALLOW);
    let inet = add(&mut s, A, FW_OUT, "internet", FW_DENY);
    assert_eq!(dec(&mut s, A, Some("192.168.1.5"), 80, None).rule_id, lan);
    assert_eq!(dec(&mut s, A, Some("8.8.8.8"), 80, None).rule_id, inet);
    assert_eq!(dec(&mut s, A, Some("10.9.9.9"), 80, None).rule_id, c8, "the network is more specific");
    assert_eq!(dec(&mut s, A, None, 80, None).rule_id, i1, "no address: only \"any\"");

    // at equal specificity, deny beats allow: two names for the same IP
    let mut t = FwStore::new();
    add(&mut t, A, FW_OUT, "good.example", FW_ALLOW);
    let no = add(&mut t, A, FW_OUT, "evil.example", FW_DENY);
    let mut q = qry(A, FW_OUT, Some("203.0.113.5"), 443);
    let d = decide(&mut t, A, &q, &["good.example", "evil.example"]);
    assert!(d.verdict == FW_V_DENY && d.rule_id == no);
    // a specific port beats any port, and a specific protocol beats "any"
    let mut r = mk(A, FW_OUT, "203.0.113.5", FW_ALLOW);
    r.port_lo = 443;
    r.port_hi = 443;
    let p443 = put(&mut t, &r).unwrap();
    r.port_lo = 0;
    r.port_hi = 0;
    r.action = FW_DENY;
    let pany = put(&mut t, &r).unwrap();
    assert_ne!(p443, pany);
    q.port = 443;
    assert_eq!(decide(&mut t, A, &q, &[]).rule_id, p443);
    q.port = 80;
    assert_eq!(decide(&mut t, A, &q, &[]).rule_id, pany);
    r.port_lo = 1;
    r.port_hi = 1024;
    r.action = FW_ALLOW;
    r.proto = FW_PROTO_UDP;
    let pudp = put(&mut t, &r).unwrap();
    q.proto = FW_PROTO_UDP;
    q.port = 53;
    assert_eq!(decide(&mut t, A, &q, &[]).rule_id, pudp);
    q.proto = FW_PROTO_TCP;
    q.port = 53;
    assert_eq!(decide(&mut t, A, &q, &[]).rule_id, pany);
    // direction (outbound/inbound) separates the rules
    let qi = qry(A, FW_IN, Some("203.0.113.5"), 443);
    assert_eq!(decide(&mut t, A, &qi, &[]).reason, FW_BY_MODE);

    // another app does not see these rules
    assert_eq!(dec(&mut s, "desktop:other", Some("10.1.2.3"), 80, None).reason, FW_BY_MODE);

    // removing rules
    assert!(s.remove(c32));
    assert_eq!(dec(&mut s, A, Some("10.1.2.3"), 80, None).rule_id, c24);
    assert!(!s.remove(c32));
    assert_eq!(s.ids.get(&c24).map(|r| r.prefix), Some(24));
    assert!(s.remove(c24) && s.remove(c8));
    assert_eq!(dec(&mut s, A, Some("10.1.2.3"), 80, None).rule_id, lan);
    let list = list_of(&s, A);
    assert_eq!(list.len(), s.ids.len());
    assert!(list.windows(2).all(|w| w[0].id < w[1].id));
    assert!(list_of(&s, "desktop:nobody").is_empty());
    let c = CString::new(A).unwrap();
    unsafe { fw_app_forget(&mut s, c.as_ptr()) };
    assert_eq!(s.ids.len(), 0);
    assert_eq!(dec(&mut s, A, Some("8.8.8.8"), 80, None).reason, FW_BY_MODE);
}

#[test]
fn restrictions() {
    let mut s = FwStore::new();
    let a = "lpk:notes";
    add(&mut s, a, FW_OUT, "any", FW_ALLOW);
    add(&mut s, a, FW_IN, "any", FW_ALLOW);
    assert_eq!(dec(&mut s, a, Some("8.8.8.8"), 443, None).verdict, FW_V_ALLOW);

    assert!(s.set_restricted(a.as_bytes(), FW_R_NO_INTERNET));
    let d = dec(&mut s, a, Some("8.8.8.8"), 443, None);
    assert!(d.verdict == FW_V_DENY && d.reason == FW_BY_RESTRICTED && d.flag == FW_R_NO_INTERNET);
    assert_eq!(dec(&mut s, a, Some("192.168.0.10"), 443, None).verdict, FW_V_ALLOW);
    assert_eq!(dec(&mut s, a, None, 443, Some("example.com")).verdict, FW_V_DENY, "no address = internet");

    assert!(s.set_restricted(a.as_bytes(), FW_R_NO_LAN));
    assert_eq!(dec(&mut s, a, Some("192.168.0.10"), 443, None).flag, FW_R_NO_LAN);
    assert_eq!(dec(&mut s, a, Some("127.0.0.1"), 443, None).flag, FW_R_NO_LAN);
    assert_eq!(dec(&mut s, a, Some("fe80::1"), 443, None).flag, FW_R_NO_LAN);
    assert_eq!(dec(&mut s, a, Some("8.8.8.8"), 443, None).verdict, FW_V_ALLOW);

    assert!(s.set_restricted(a.as_bytes(), FW_R_NO_INCOMING));
    let qi = qry(a, FW_IN, Some("192.168.0.10"), 8080);
    assert_eq!(decide(&mut s, a, &qi, &[]).flag, FW_R_NO_INCOMING);
    assert_eq!(dec(&mut s, a, Some("192.168.0.10"), 443, None).verdict, FW_V_ALLOW);

    const NO_MIC: u32 = 1 << 5;
    assert!(s.set_restricted(a.as_bytes(), FW_R_NO_NETWORK | NO_MIC));
    assert_eq!(dec(&mut s, a, Some("192.168.0.10"), 443, None).flag, FW_R_NO_NETWORK);
    assert_eq!(decide(&mut s, a, &qi, &[]).flag, FW_R_NO_NETWORK);
    let c = CString::new(a).unwrap();
    assert!(unsafe { fw_blocks(&s, c.as_ptr(), NO_MIC) } == 1 && unsafe { fw_blocks(&s, c.as_ptr(), 1 << 4) } == 0);

    // the non-network ones: camera, mic, screen, usb, bluetooth, files, notifications
    for bit in [4, 5, 6, 7, 8, 9, 10] {
        let flag = 1u32 << bit;
        assert!(s.set_restricted(a.as_bytes(), flag));
        assert_eq!(unsafe { fw_blocks(&s, c.as_ptr(), flag) }, 1);
        assert_eq!(dec(&mut s, a, Some("8.8.8.8"), 443, None).verdict, FW_V_ALLOW, "they don't touch the network");
        let name = fw_flag_name(flag);
        assert!(!name.is_null());
        assert_eq!(unsafe { fw_flag_from_name(name) }, flag);
    }
    assert!(s.set_restricted(a.as_bytes(), 0));
    assert_eq!(unsafe { fw_restricted(&s, c.as_ptr()) }, 0);
    let nosuch = CString::new("no-such").unwrap();
    assert_eq!(unsafe { fw_flag_from_name(nosuch.as_ptr()) }, 0);
    assert!(fw_flag_name(3).is_null());
}

#[test]
fn lifetimes() {
    let mut s = FwStore::new();
    s.mode = FW_MODE_ASK;
    let a = "desktop:x";
    let mut r = mk(a, FW_OUT, "example.org", FW_ALLOW);
    r.life = FW_LIFE_ONCE;
    let once = put(&mut s, &r).unwrap();
    let d = dec(&mut s, a, Some("198.51.100.1"), 443, Some("example.org"));
    assert!(d.verdict == FW_V_ALLOW && d.rule_id == once);
    assert!(!s.ids.contains_key(&once) && s.ids.is_empty(), "a once rule is used up");
    assert_eq!(dec(&mut s, a, Some("198.51.100.1"), 443, Some("example.org")).verdict, FW_V_ASK);

    r = mk(a, FW_OUT, "198.51.100.0/24", FW_DENY);
    r.life = FW_LIFE_UNTIL_QUIT;
    let q1 = put(&mut s, &r).unwrap();
    let keep = add(&mut s, a, FW_OUT, "lan", FW_ALLOW);
    r = mk("desktop:y", FW_OUT, "any", FW_DENY);
    r.life = FW_LIFE_UNTIL_QUIT;
    let qy = put(&mut s, &r).unwrap();
    assert_eq!(dec(&mut s, a, Some("198.51.100.1"), 443, None).rule_id, q1);
    assert_eq!(dec(&mut s, a, Some("198.51.100.1"), 443, None).rule_id, q1, "not used up");
    let json = String::from_utf8(file::to_json(&s).unwrap()).unwrap();
    assert!(!json.contains("198.51.100") && json.contains("\"lan\""), "only the forever rules are saved");
    let c = CString::new(a).unwrap();
    unsafe { fw_app_quit(&mut s, c.as_ptr()) };
    assert!(!s.ids.contains_key(&q1) && s.ids.contains_key(&keep) && s.ids.contains_key(&qy));
    assert_eq!(dec(&mut s, a, Some("198.51.100.1"), 443, None).verdict, FW_V_ASK);
    let y = CString::new("desktop:y").unwrap();
    unsafe { fw_app_quit(&mut s, y.as_ptr()) };
    assert_eq!(s.ids.len(), 1);

    r = mk(a, FW_OUT, "any", FW_ALLOW);
    r.life = 9;
    assert!(put(&mut s, &r).is_none());
    r.life = FW_LIFE_FOREVER;
    r.dir = 5;
    assert!(put(&mut s, &r).is_none());
    r.dir = FW_OUT;
    r.port_lo = 10;
    r.port_hi = 5;
    assert!(put(&mut s, &r).is_none());
    r.port_lo = 0;
    r.port_hi = 0;
    r.kind = FW_DST_CIDR;
    r.fam = 4;
    r.prefix = 8;
    r.addr[0] = 10;
    r.addr[3] = 1;
    assert!(put(&mut s, &r).is_none(), "host bits");
}

// ---- JSON ----

fn sample() -> FwStore {
    let mut s = FwStore::new();
    s.mode = FW_MODE_ASK;
    s.set_trusted(b"lordos", true);
    s.set_trusted(b"desktop:org.gnome.Maps", true);
    s.set_restricted(b"lpk:game", FW_R_NO_INTERNET | (1 << 4) | (1 << 10));
    add(&mut s, "desktop:org.chromium.Chromium", FW_OUT, "any", FW_ALLOW);
    add(&mut s, "desktop:org.chromium.Chromium", FW_OUT, "*.doubleclick.net", FW_DENY);
    let mut r = mk("/usr/bin/curl", FW_OUT, "2001:db8::/32", FW_DENY);
    r.port_lo = 8000;
    r.port_hi = 8080;
    r.proto = FW_PROTO_TCP;
    r.when = 1760000000;
    put(&mut s, &r).unwrap();
    r = mk("lpk:game", FW_IN, "lan", FW_ALLOW);
    r.port_lo = 27015;
    r.port_hi = 27015;
    r.proto = FW_PROTO_UDP;
    put(&mut s, &r).unwrap();
    add(&mut s, "app-lordos-lpk-game.scope", FW_OUT, "10.0.0.0/8", FW_ALLOW);
    s
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn the_file_round_trips_and_refuses_bad_input() {
    let s = sample();
    let j1 = file::to_json(&s).unwrap();
    let mut t = FwStore::new();
    file::from_json(&mut t, &j1).unwrap();
    assert_eq!(file::to_json(&t).unwrap(), j1);
    assert!(t.mode == FW_MODE_ASK && t.apps[&b"lordos"[..]].trusted && t.apps[&b"lpk:game"[..]].restricted & (1 << 4) != 0);
    let chrome = "desktop:org.chromium.Chromium";
    assert_eq!(dec(&mut t, chrome, Some("1.2.3.4"), 443, Some("ad.doubleclick.net")).verdict, FW_V_DENY);
    assert_eq!(dec(&mut t, chrome, Some("1.2.3.4"), 443, Some("example.com")).verdict, FW_V_ALLOW);
    assert_eq!(dec(&mut t, "lpk:game", Some("10.2.3.4"), 443, None).verdict, FW_V_ALLOW);

    // the file
    let dir = std::env::temp_dir().join(format!("fw-rules-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("firewall.json");
    let p = path.to_str().unwrap().as_bytes();
    assert!(file::save(&s, p));
    let mut u = FwStore::new();
    assert_eq!(file::load(&mut u, p), Ok(true));
    assert_eq!(file::to_json(&u).unwrap(), j1);
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(file::load(&mut u, p), Ok(false), "no file: stays the same");
    assert_eq!(u.ids.len(), s.ids.len());
    // a link, a directory and a file that is too big are refused
    std::fs::write(dir.join("real"), "{\"v\":1}").unwrap();
    std::os::unix::fs::symlink(dir.join("real"), &path).unwrap();
    assert!(file::load(&mut u, p).unwrap_err().starts_with("could not open"));
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, vec![b' '; (4 << 20) + 1]).unwrap();
    assert_eq!(file::load(&mut u, p).unwrap_err(), "not a regular file or too large");
    std::fs::remove_dir_all(&dir).unwrap();

    // bad input: rejected, and the store does not change
    let bad = [
        "", "[]", "null", "{", "{\"v\":2}", "{}", "{\"v\":\"1\"}", "{\"v\":1,\"x\":1}", "{\"v\":1}{}", "{\"v\":1,\"mode\":\"loud\"}", "{\"v\":1,\"mode\":1}",
        "{\"v\":1,\"trusted\":\"lordos\"}", "{\"v\":1,\"trusted\":[1]}", "{\"v\":1,\"trusted\":[\"a b\"]}", "{\"v\":1,\"trusted\":[\"lordos\",\"lordos\"]}",
        "{\"v\":1,\"trusted\":[\"app-lordos-x.scope\"]}", "{\"v\":1,\"restricted\":[]}", "{\"v\":1,\"restricted\":{\"lpk:a\":[]}}",
        "{\"v\":1,\"restricted\":{\"lpk:a\":[\"no-wifi\"]}}", "{\"v\":1,\"restricted\":{\"lpk:a\":[\"no-mic\",\"no-mic\"]}}",
        "{\"v\":1,\"restricted\":{\"lpk:a\":\"no-mic\"}}", "{\"v\":1,\"trusted\":[\"lpk:a\"],\"restricted\":{\"lpk:a\":[\"no-mic\"]}}",
        "{\"v\":1,\"rules\":{}}", "{\"v\":1,\"rules\":[1]}", "{\"v\":1,\"rules\":[{}]}", "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"maybe\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"10.0.0.1/8\",\"action\":\"allow\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"dir\":\"up\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"port\":\"0\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"port\":\"9-1\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"port\":\"65536\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"port\":1.5}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"proto\":\"icmp\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"when\":-1}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"life\":\"once\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\",\"app\":\"lordos\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"*.\",\"action\":\"allow\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lord\\u0000os\",\"dst\":\"any\",\"action\":\"allow\"}]}",
        "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"allow\"},]}",
    ];
    let before = t.ids.len();
    for (i, b) in bad.iter().enumerate() {
        let r = file::from_json(&mut t, b.as_bytes());
        assert!(r.as_ref().is_err_and(|e| !e.is_empty()), "accepted invalid JSON {i}: {b}");
    }
    assert!(file::from_json(&mut t, b"{\"v\":1}\0").is_err());
    assert!(t.ids.len() == before && t.mode == FW_MODE_ASK);
    // the minimum is valid and leaves everything empty
    file::from_json(&mut t, b"{\"v\":1}").unwrap();
    assert!(t.ids.is_empty() && t.mode == FW_MODE_SILENT);
    let num = "{\"v\":1,\"rules\":[{\"app\":\"lordos\",\"dst\":\"any\",\"action\":\"deny\",\"port\":443}]}";
    file::from_json(&mut t, num.as_bytes()).unwrap();
    assert_eq!(t.ids.len(), 1);
    assert_eq!(dec(&mut t, "lordos", Some("1.2.3.4"), 443, None).verdict, FW_V_DENY);
    assert_eq!(dec(&mut t, "lordos", Some("1.2.3.4"), 80, None).reason, FW_BY_MODE);
    // errors say where
    assert_eq!(file::from_json(&mut t, b"{\"v\":1,\"x\":1}").unwrap_err(), "unknown key \"x\"");
    assert_eq!(file::from_json(&mut t, b"{\"v\":1,\"rules\":[{}]}").unwrap_err(), "rule 0: invalid app");
    assert!(file::from_json(&mut t, b"{\"v\":1,\"a\":1,\"a\":2}").unwrap_err().starts_with("invalid JSON: repeated key (byte "));
}

use std::os::unix::fs::PermissionsExt;

#[test]
fn random_mutations_of_the_good_file_never_break_it() {
    let j1 = file::to_json(&sample()).unwrap();
    let mut rng = Rng(1234);
    let mut accepted = 0;
    for _ in 0..20000 {
        let mut m = j1.clone();
        for _ in 0..1 + rng.below(4) {
            if m.is_empty() {
                break;
            }
            let p = rng.below(m.len());
            match rng.below(3) {
                0 => m[p] = rng.next() as u8,
                1 => m[p] = b"{}[]\",:0a-*./\\"[rng.below(14)],
                _ => m.truncate(p),
            }
        }
        let mut f = FwStore::new();
        if file::from_json(&mut f, &m).is_ok() {
            accepted += 1;
            // accepted: it is written and read back the same
            let again = file::to_json(&f).unwrap();
            let mut g = FwStore::new();
            file::from_json(&mut g, &again).unwrap();
            assert_eq!(file::to_json(&g).unwrap(), again);
        }
    }
    assert!(accepted > 0);
}

#[test]
fn the_json_reader_is_strict() {
    assert!(json::parse(b" {\"a\":[1,2.5,-3e2,true,false,null,\"x\\u00e9\\ud83d\\ude00\"]} \n").is_ok());
    for bad in ["", "{", "[1,]", "{\"a\":1,\"a\":2}", "\"\\u0000\"", "\"\\ud800\"", "\"\\udc00\"", "01", "1.", "1e", "tru", "[1 2]", "1 2", "\"\x01\"", "\"abc", "{,}"] {
        assert!(json::parse(bad.as_bytes()).is_err(), "{bad:?}");
    }
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(json::parse(deep(32).as_bytes()).is_ok());
    assert_eq!(json::parse(deep(33).as_bytes()).unwrap_err().why, "nested too deeply");
    assert_eq!(json::parse(b"{\"a\" 1}").unwrap_err().message(), "expected ':' (byte 5)");
    assert_eq!(json::parse(b"[1] x").unwrap_err().message(), "trailing garbage (byte 4)");
    assert_eq!(json::parse(b"").unwrap_err().message(), "missing value (byte 0)");
    let v = json::parse(b"{\"s\":\"a\\n\\u00e9\",\"n\":[1,2]}").unwrap();
    assert_eq!(v.get(b"s"), Some(&json::Json::Str("a\n\u{e9}".as_bytes().to_vec())));
    assert_eq!(v.get(b"n"), Some(&json::Json::Arr(vec![json::Json::Num(1.0), json::Json::Num(2.0)])));
    assert_eq!(v.get(b"missing"), None);
    let mut q = Vec::new();
    json::quote(b"a\"b\\c\n\x01", &mut q);
    assert_eq!(q, b"\"a\\\"b\\\\c\\u000a\\u0001\"");
}

// ---- random against a linear version ----

fn labels_of(d: &[u8]) -> i32 {
    1 + d.iter().filter(|&&c| c == b'.').count() as i32
}

fn ref_dst(r: &FwRule, q: &FwQuery, hosts: &[&str], lan: i32) -> i32 {
    match r.kind {
        FW_DST_ANY => 0,
        FW_DST_LAN => if lan == 1 { 500 } else { -1 },
        FW_DST_INTERNET => if lan == 0 { 500 } else { -1 },
        FW_DST_CIDR => {
            if r.fam != q.fam {
                return -1;
            }
            for i in 0..r.prefix as usize {
                if (r.addr[i >> 3] ^ q.addr[i >> 3]) >> (7 - (i & 7)) & 1 != 0 {
                    return -1;
                }
            }
            1000 + r.prefix
        }
        FW_DST_DOMAIN => {
            let dom = cs(&r.domain);
            let mut best = -1;
            for h in hosts {
                let n = h.as_bytes();
                if r.wildcard == 0 {
                    if n == dom && 3000 + labels_of(n) > best {
                        best = 3000 + labels_of(n);
                    }
                } else if n == dom || (n.len() > dom.len() && n[n.len() - dom.len() - 1] == b'.' && &n[n.len() - dom.len()..] == dom) {
                    best = best.max(2000 + labels_of(dom));
                }
            }
            best
        }
        _ => -1,
    }
}

fn ref_decide(rules: &[FwRule], q: &FwQuery, app: &str, hosts: &[&str]) -> u32 {
    let lan = if q.fam != 0 { addr_is_lan(q.fam, &q.addr) as i32 } else { -1 };
    let mut best: Option<&FwRule> = None;
    let mut bs = [0i64; 5];
    for r in rules {
        if cs(&r.app) != app.as_bytes() || r.dir != q.dir {
            continue;
        }
        if r.proto != FW_PROTO_ANY && r.proto != q.proto {
            continue;
        }
        if q.port < r.port_lo || q.port > r.port_hi {
            continue;
        }
        let d = ref_dst(r, q, hosts, lan);
        if d < 0 {
            continue;
        }
        let sc = [d as i64, 65535 - (r.port_hi as i64 - r.port_lo as i64), (r.proto != FW_PROTO_ANY) as i64, (r.action == FW_DENY) as i64, r.id as i64];
        let mut better = best.is_none();
        if !better {
            for k in 0..5 {
                if sc[k] != bs[k] {
                    better = sc[k] > bs[k];
                    break;
                }
            }
        }
        if better {
            best = Some(r);
            bs = sc;
        }
    }
    best.map_or(0, |r| r.id)
}

#[test]
fn random_decisions_match_a_linear_version() {
    const APPS: [&str; 3] = ["lordos", "desktop:a", "lpk:b"];
    const NAMES: [&str; 8] = ["a.com", "b.a.com", "c.b.a.com", "x.org", "y.x.org", "com", "org", "z.net"];
    let mut s = FwStore::new();
    s.mode = FW_MODE_ASK;
    let mut rng = Rng(42);
    let (mut mism, mut total) = (0, 0);
    for _ in 0..30000 {
        let op = rng.below(10);
        if op < 4 {
            let mut r = zeroed_rule();
            set(&mut r.app, APPS[rng.below(3)].as_bytes());
            r.dir = rng.below(2) as c_int;
            r.action = rng.below(2) as c_int;
            r.proto = rng.below(3) as c_int;
            if rng.below(2) == 1 {
                r.port_lo = (rng.below(4) * 100) as u16;
                r.port_hi = r.port_lo + (rng.below(3) * 100) as u16;
            }
            let dst = match rng.below(6) {
                0 => "any".to_string(),
                1 => (if rng.below(2) == 1 { "lan" } else { "internet" }).to_string(),
                2 => NAMES[rng.below(8)].to_string(),
                3 => format!("*.{}", NAMES[rng.below(8)]),
                4 => {
                    let p = rng.below(33) as u32;
                    let mut a = (if rng.below(4) == 0 { 0x0a000000u32 } else { 0xc0a80000 }) | rng.below(65536) as u32;
                    if p < 32 {
                        a &= if p > 0 { !0u32 << (32 - p) } else { 0 };
                    }
                    format!("{}.{}.{}.{}/{p}", a >> 24, a >> 16 & 255, a >> 8 & 255, a & 255)
                }
                _ => {
                    let p = rng.below(129);
                    let mut a = [0u8; 16];
                    a[..4].copy_from_slice(&[0x20, 0x01, 0x0d, 0xb8]);
                    a[4] = rng.below(4) as u8;
                    a[15] = rng.below(4) as u8;
                    for i in p..128 {
                        a[i >> 3] &= !(0x80u8 >> (i & 7));
                    }
                    format!("{}/{p}", std::net::Ipv6Addr::from(a))
                }
            };
            assert!(parse_dest(dst.as_bytes(), &mut r), "dst {dst}");
            r.life = if rng.below(8) == 0 { FW_LIFE_ONCE } else { FW_LIFE_FOREVER };
            assert!(put(&mut s, &r).is_some());
        } else if op < 5 {
            let ids = s.sorted(None);
            if !ids.is_empty() {
                assert!(s.remove(ids[rng.below(ids.len())]));
            }
        } else {
            let all: Vec<FwRule> = s.sorted(None).iter().map(|id| *s.ids[id]).collect();
            let app = APPS[rng.below(3)];
            let mut q = qry(app, rng.below(2) as c_int, None, 0);
            q.proto = if rng.below(2) == 1 { FW_PROTO_TCP } else { FW_PROTO_UDP };
            q.port = rng.below(700) as u16;
            let hs = [NAMES[rng.below(8)], NAMES[rng.below(8)]];
            let hosts = &hs[..rng.below(3)];
            match rng.below(3) {
                1 => {
                    q.fam = 4;
                    let a = (if rng.below(3) == 0 { 0x0a000000u32 } else if rng.below(2) == 1 { 0xc0a80000 } else { 0x08080000 }) | rng.below(65536) as u32;
                    q.addr[..4].copy_from_slice(&a.to_be_bytes());
                }
                2 => {
                    q.fam = 6;
                    q.addr[..4].copy_from_slice(&[0x20, 0x01, 0x0d, 0xb8]);
                    q.addr[4] = rng.below(4) as u8;
                    q.addr[15] = rng.below(4) as u8;
                }
                _ => {}
            }
            let want = ref_decide(&all, &q, app, hosts);
            let d = decide(&mut s, app, &q, hosts);
            let got = if d.reason == FW_BY_RULE { d.rule_id } else { 0 };
            total += 1;
            if want != got {
                mism += 1;
            }
        }
    }
    assert_eq!(mism, 0, "{mism} of {total} decisions differ from the linear version");
}

#[test]
fn many_rules_decide_quickly() {
    let mut s = FwStore::new();
    for i in 0..50000 {
        let app = format!("desktop:app{}", i % 500);
        let dst = match i % 3 {
            0 => format!("h{i}.example{}.com", i % 7),
            1 => format!("*.s{i}.net"),
            _ => format!("10.{}.{}.0/24", i / 256 % 256, i % 256),
        };
        add(&mut s, &app, FW_OUT, &dst, if i % 2 == 1 { FW_ALLOW } else { FW_DENY });
    }
    let q = qry("desktop:app4", FW_OUT, Some("10.0.4.9"), 443);
    let first = decide(&mut s, "desktop:app4", &q, &["a.b.c.s4.net"]).rule_id;
    let t0 = std::time::Instant::now();
    let mut sum = 0u64;
    for _ in 0..100_000 {
        sum += decide(&mut s, "desktop:app4", &q, &["a.b.c.s4.net"]).rule_id as u64;
    }
    assert!(first != 0 && sum == 100_000 * first as u64);
    assert!(t0.elapsed().as_secs() < 30, "100,000 decisions among 50,000 rules took {:?}", t0.elapsed());
}
