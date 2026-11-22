//! The saved configurations, the DNS and the `.ovpn` embedding, on fake folders.
use axia_netstate::config::*;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axia-netstate-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn paths(d: &Path) -> Paths {
    let s = |n: &str| d.join(n).to_str().unwrap().to_string();
    Paths { if_dir: s("if"), vpn_dir: s("vpn"), wifi_dir: s("wifi"), resolv_conf: s("resolv.conf"), resolved_conf: s("resolved.conf"), sys_class_net: s("sys") }
}

fn j(v: Vec<u8>) -> Value {
    serde_json::from_slice(&v).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(&v)))
}

#[test]
fn an_interface_configuration_as_lordos_saved_it() {
    let d = dir("ifconf");
    let p = paths(&d);
    fs::create_dir_all(&p.if_dir).unwrap();
    fs::write(format!("{}/eth0.conf", p.if_dir), "method=static\naddress=10.0.0.2/24\naddress=fd00::2/64\ngateway=10.0.0.1\ndns=1.1.1.1\ndns=9.9.9.9\nother=x\n").unwrap();
    assert_eq!(
        j(if_config_json(&p, b"eth0").unwrap()),
        json!({ "method": "static", "addresses": ["10.0.0.2/24", "fd00::2/64"], "gateway": "10.0.0.1", "dns": ["1.1.1.1", "9.9.9.9"] })
    );
    assert_eq!(
        String::from_utf8(if_config_json(&p, b"eth0").unwrap()).unwrap(),
        r#"{"method":"static","addresses":["10.0.0.2/24","fd00::2/64"],"gateway":"10.0.0.1","dns":["1.1.1.1","9.9.9.9"]}"#,
        "the same text the C code wrote"
    );
    fs::write(format!("{}/wlan0.conf", p.if_dir), "method=dhcp\ngateway=a\"b\\c\n").unwrap();
    assert_eq!(j(if_config_json(&p, b"wlan0").unwrap()), json!({ "method": "dhcp", "addresses": [], "gateway": "a\"b\\c", "dns": [] }));
    assert_eq!(if_config_json(&p, b"nothere"), Some(b"null".to_vec()));
    for bad in [&b""[..], b"../etc/passwd", b"a b", b".."] {
        assert_eq!(if_config_json(&p, bad), None, "{bad:?}");
    }
    // a link in the place of the file is not followed
    std::os::unix::fs::symlink(format!("{}/eth0.conf", p.if_dir), format!("{}/evil.conf", p.if_dir)).unwrap();
    assert_eq!(if_config_json(&p, b"evil"), Some(b"null".to_vec()));
}

#[test]
fn dns_from_resolv_conf_or_from_resolved() {
    let d = dir("dns");
    let p = paths(&d);
    fs::write(&p.resolv_conf, "# comment\nnameserver 192.168.1.1\nnameserver 2001:db8::1\nsearch home.arpa lan\noptions edns0\nnameserver\nsearch  one.example \t two.example\n").unwrap();
    assert_eq!(j(dns_json(&p)), json!({ "source": "resolv.conf", "servers": ["192.168.1.1", "2001:db8::1"], "search": ["one.example", "two.example"] }), "the last search line wins");
    assert!(String::from_utf8(dns_json(&p)).unwrap().starts_with("{\"source\":\"resolv.conf\",\"servers\":[\"192.168.1.1\""));
    // resolv.conf is a link to systemd-resolved's stub: its own file is read instead
    fs::write(&p.resolved_conf, "nameserver 10.1.1.1\ndomain corp.example\n").unwrap();
    fs::remove_file(&p.resolv_conf).unwrap();
    std::os::unix::fs::symlink("../run/systemd/resolve/stub-resolv.conf", &p.resolv_conf).unwrap();
    assert_eq!(j(dns_json(&p)), json!({ "source": "systemd-resolved", "servers": ["10.1.1.1"], "search": ["corp.example"] }));
    // a link to something else is just resolv.conf (which then cannot be read here)
    fs::remove_file(&p.resolv_conf).unwrap();
    std::os::unix::fs::symlink("/nonexistent/x", &p.resolv_conf).unwrap();
    assert_eq!(j(dns_json(&p)), json!({ "source": "resolv.conf", "servers": [], "search": [] }));
    // nothing at all
    assert_eq!(j(dns_json(&paths(&dir("dns-none")))), json!({ "source": "resolv.conf", "servers": [], "search": [] }));
}

#[test]
fn public_metadata_of_vpns_and_known_wifi_networks() {
    let d = dir("meta");
    let p = paths(&d);
    fs::create_dir_all(&p.vpn_dir).unwrap();
    fs::write(format!("{}/b.json", p.vpn_dir), "{\"name\":\"b\",\"type\":\"openvpn\"}\nsecond line is dropped\n").unwrap();
    fs::write(format!("{}/a.json", p.vpn_dir), "{\"name\":\"a\"}").unwrap();
    fs::write(format!("{}/c.json", p.vpn_dir), "[1,2]").unwrap(); // not an object
    fs::write(format!("{}/.hidden.json", p.vpn_dir), "{\"x\":1}").unwrap();
    fs::write(format!("{}/d.conf", p.vpn_dir), "{\"x\":1}").unwrap();
    fs::write(format!("{}/.json", p.vpn_dir), "{\"x\":1}").unwrap();
    fs::write(format!("{}/credentials", p.vpn_dir), "secret").unwrap();
    assert_eq!(String::from_utf8(dir_json_array(&p.vpn_dir)).unwrap(), "[{\"name\":\"a\"},{\"name\":\"b\",\"type\":\"openvpn\"}]");
    assert_eq!(dir_json_array(&p.wifi_dir), b"[]", "no folder, no networks");
}

#[test]
fn an_ovpn_gets_the_files_it_names_embedded() {
    let d = dir("ovpn");
    fs::write(d.join("ca.crt"), "CA-PEM\n").unwrap();
    fs::write(d.join("client.key"), "KEY-PEM-NO-NEWLINE").unwrap();
    fs::write(d.join("ta.key"), "TA\n").unwrap();
    fs::write(d.join("empty.crt"), "").unwrap();
    let abs = d.join("abs.crt");
    fs::write(&abs, "ABS\n").unwrap();
    let cfg = format!(
        "client\nremote vpn.example 1194\nca ca.crt\nkey client.key\ntls-auth ta.key 1\ncert empty.crt\ncert {}\ncert [inline]\nauth-user-pass creds.txt\nverb 3\n\nauth-user-pass\ncacert x\n",
        abs.display()
    );
    let path = d.join("work.ovpn");
    fs::write(&path, cfg).unwrap();
    let out = String::from_utf8(ovpn_inline(path.to_str().unwrap().as_bytes(), 256).unwrap()).unwrap();
    assert_eq!(
        out,
        format!(
            "client\nremote vpn.example 1194\n<ca>\nCA-PEM\n</ca>\n<key>\nKEY-PEM-NO-NEWLINE\n</key>\n<tls-auth>\nTA\n</tls-auth>\nkey-direction 1\n<cert>\n\n</cert>\n<cert>\nABS\n</cert>\ncert [inline]\nauth-user-pass\nverb 3\n\nauth-user-pass\ncacert x\n"
        )
    );
    // a file that is not there: the code line says which
    fs::write(&path, "ca missing file.crt\n").unwrap();
    assert_eq!(ovpn_inline(path.to_str().unwrap().as_bytes(), 256), Err(b"ovpn.missingFile file=missing".to_vec()));
    // the .ovpn itself is not there
    let e = ovpn_inline(b"/nonexistent/x.ovpn", 256).unwrap_err();
    assert_eq!(e, b"sys.readFailed path=/nonexistent/x.ovpn");
    // an empty one gives nothing to use
    fs::write(&path, "").unwrap();
    assert_eq!(ovpn_inline(path.to_str().unwrap().as_bytes(), 256), Err(b"sys.outOfMemory".to_vec()));
    // a link in place of a named file is not followed
    fs::write(&path, "ca link.crt\n").unwrap();
    std::os::unix::fs::symlink(d.join("ca.crt"), d.join("link.crt")).unwrap();
    assert_eq!(ovpn_inline(path.to_str().unwrap().as_bytes(), 256), Err(b"ovpn.missingFile file=link.crt".to_vec()));
}
