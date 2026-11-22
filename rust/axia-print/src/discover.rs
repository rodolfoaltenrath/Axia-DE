//! `discover`: the printers and the scanners that announce themselves on the local network, as
//! the JSON `lord.print.discover()` gives JS.
use crate::browse::browse;
use crate::{http, lossy};
use axia_codecs::mdns::MdnsSvc;
use serde_json::{json, Value};

/// A TXT value as `mdns_txt` copies it into a buffer of `n` bytes (one is for the NUL).
fn txt(s: &MdnsSvc, key: &str, n: usize) -> String {
    let v = s.txt_value(key.as_bytes());
    lossy(&v[..v.len().min(n - 1)])
}

/// The JSON of what was found: {printers: [...], scanners: [...]}.
pub fn to_json(svc: &[MdnsSvc]) -> Value {
    let mut printers = Vec::new();
    for s in svc {
        if !s.kind().starts_with(b"_ipp") || s.addr().is_empty() {
            continue;
        }
        let tls = s.kind() == b"_ipps._tcp";
        // the same printer usually announces itself over ipp and ipps: shown once (ipp)
        if tls && svc.iter().any(|o| o.kind() == b"_ipp._tcp" && o.name() == s.name()) {
            continue;
        }
        let rp = txt(s, "rp", 256);
        let addr = lossy(s.addr());
        let uri = format!("{}://{addr}:{}/{}", if tls { "ipps" } else { "ipp" }, s.port(), if rp.is_empty() { "ipp/print" } else { &rp });
        let field = |key: &str| txt(s, key, 512);
        printers.push(json!({
            "name": lossy(s.name()),
            "host": lossy(s.host()),
            "addr": addr,
            "port": s.port(),
            "tls": tls,
            "uri": uri,
            "model": field("ty"),
            "note": field("note"),
            "uuid": field("UUID"),
            "pdl": field("pdl"),
            "color": field("Color"),
            "duplex": field("Duplex"),
            "kind": field("kind"),
        }));
    }
    let mut scanners = Vec::new();
    for s in svc {
        if !s.kind().starts_with(b"_uscan") || s.addr().is_empty() {
            continue;
        }
        let rs = txt(s, "rs", 512);
        let addr = lossy(s.addr());
        scanners.push(json!({
            "name": lossy(s.name()),
            "lan": http::lan_ipv4(&addr),
            "addr": addr,
            "port": s.port(),
            "tls": s.kind() == b"_uscans._tcp",
            "rs": if rs.is_empty() { "eSCL".to_string() } else { rs },
            "model": txt(s, "ty", 512),
            "uuid": txt(s, "UUID", 512),
        }));
    }
    json!({ "printers": printers, "scanners": scanners })
}

/// Asks the network for 1.8 s.
pub fn discover() -> Value {
    let found = browse(&["_ipp._tcp", "_ipps._tcp", "_uscan._tcp", "_uscans._tcp"], 1800, 48);
    to_json(&found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axia_codecs::mdns::{parse, MdnsCache};

    fn name(out: &mut Vec<u8>, s: &str) {
        for l in s.split('.') {
            out.push(l.len() as u8);
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
    }

    fn rr(out: &mut Vec<u8>, owner: &str, ty: u16, rd: &[u8]) {
        name(out, owner);
        out.extend_from_slice(&ty.to_be_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0, 120]);
        out.extend_from_slice(&(rd.len() as u16).to_be_bytes());
        out.extend_from_slice(rd);
    }

    fn txt_rd(kv: &[&str]) -> Vec<u8> {
        kv.iter().flat_map(|s| std::iter::once(s.len() as u8).chain(s.bytes())).collect()
    }

    /// A reply announcing `inst` of `ty` on `host` with TXT `kv`, at `addr`.
    fn announce(inst: &str, ty: &str, host: &str, port: u16, addr: Option<[u8; 4]>, kv: &[&str]) -> Vec<u8> {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, if addr.is_some() { 4 } else { 3 }, 0, 0, 0, 0];
        let full = format!("{inst}.{ty}.local");
        let mut ptr = Vec::new();
        name(&mut ptr, &full);
        rr(&mut p, &format!("{ty}.local"), 12, &ptr);
        let mut srv = vec![0, 0, 0, 0, (port >> 8) as u8, port as u8];
        name(&mut srv, host);
        rr(&mut p, &full, 33, &srv);
        rr(&mut p, &full, 16, &txt_rd(kv));
        if let Some(a) = addr {
            rr(&mut p, host, 1, &a);
        }
        p
    }

    fn services(packets: &[Vec<u8>], types: &[&[u8]]) -> Vec<MdnsSvc> {
        let mut c = MdnsCache::empty();
        for p in packets {
            assert_eq!(parse(p, &mut c), 0);
        }
        let mut out: Vec<MdnsSvc> = (0..16).map(|_| MdnsSvc::zeroed()).collect();
        let n = axia_codecs::mdns::collect(&c, types, &mut out);
        out.truncate(n);
        out
    }

    #[test]
    fn printers_and_scanners_become_the_json_js_reads() {
        let kinds: [&[u8]; 4] = [b"_ipp._tcp", b"_ipps._tcp", b"_uscan._tcp", b"_uscans._tcp"];
        let svc = services(
            &[
                announce("Oficina", "_ipp._tcp", "impre.local", 631, Some([192, 168, 1, 20]), &["rp=ipp/print", "ty=Impresora de prueba", "Color=T", "UUID=u-1"]),
                announce("Oficina", "_ipps._tcp", "impre.local", 443, Some([192, 168, 1, 20]), &["rp=ipp/print"]),
                announce("Solo TLS", "_ipps._tcp", "tls.local", 443, Some([192, 168, 1, 21]), &[]),
                announce("Scan", "_uscan._tcp", "scan.local", 80, Some([8, 8, 8, 8]), &["rs=eSCL", "ty=Scanner X", "UUID=u-2"]),
                announce("Scan 2", "_uscan._tcp", "scan2.local", 8080, Some([10, 0, 0, 9]), &[]),
            ],
            &kinds,
        );
        assert_eq!(svc.len(), 5);
        let j = to_json(&svc);
        let p = j["printers"].as_array().unwrap();
        assert_eq!(p.len(), 2, "the ipps twin of an ipp printer is hidden: {j}");
        assert_eq!((p[0]["name"].as_str(), p[0]["tls"].as_bool(), p[0]["uri"].as_str()), (Some("Oficina"), Some(false), Some("ipp://192.168.1.20:631/ipp/print")));
        assert_eq!((p[0]["model"].as_str(), p[0]["color"].as_str(), p[0]["uuid"].as_str(), p[0]["host"].as_str()), (Some("Impresora de prueba"), Some("T"), Some("u-1"), Some("impre.local")));
        assert_eq!((p[1]["name"].as_str(), p[1]["tls"].as_bool(), p[1]["uri"].as_str()), (Some("Solo TLS"), Some(true), Some("ipps://192.168.1.21:443/ipp/print")));
        let s = j["scanners"].as_array().unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0]["rs"].as_str(), s[0]["lan"].as_bool(), s[0]["model"].as_str(), s[0]["port"].as_i64()), (Some("eSCL"), Some(false), Some("Scanner X"), Some(80)));
        assert_eq!((s[1]["rs"].as_str(), s[1]["lan"].as_bool(), s[1]["tls"].as_bool()), (Some("eSCL"), Some(true), Some(false)));
    }

    #[test]
    fn a_service_without_an_address_is_not_listed() {
        let p = announce("Sin IP", "_ipp._tcp", "x.local", 631, None, &[]);
        let svc = services(&[p], &[b"_ipp._tcp"]);
        assert_eq!(svc.len(), 1);
        assert!(svc[0].addr().is_empty());
        assert_eq!(to_json(&svc), json!({ "printers": [], "scanners": [] }));
    }
}
