//! Rule-editor validation, independent of the host transport and firewall engine.
use serde_json::{json, Value};
use std::net::IpAddr;
pub fn destination(input: &str) -> Result<String, &'static str> {
    let text = input.trim();
    if text.is_empty() {
        return Err("empty");
    }
    if matches!(text, "any" | "lan" | "internet") {
        return Ok(text.into());
    }
    let (address, prefix) = text
        .split_once('/')
        .map(|(a, p)| (a, Some(p)))
        .unwrap_or((text, None));
    if let Ok(ip) = address.parse::<IpAddr>() {
        let (mut ip, mut bits) = (ip, if ip.is_ipv4() { 32 } else { 128 });
        let mut length = if let Some(p) = prefix {
            if p.len() > 1 && p.starts_with('0') {
                return Err("prefix");
            }
            p.parse::<u32>().map_err(|_| "prefix")?
        } else {
            bits
        };
        if length > bits {
            return Err("prefix");
        }
        if let IpAddr::V6(v6) = ip {
            if let Some(v4) = v6.to_ipv4_mapped() {
                if length >= 96 {
                    ip = IpAddr::V4(v4);
                    length -= 96;
                    bits = 32;
                }
            }
        }
        let raw = match ip {
            IpAddr::V4(a) => u32::from(a) as u128,
            IpAddr::V6(a) => u128::from(a),
        };
        let host = bits - length;
        if host == 128 {
            if raw != 0 {
                return Err("hostBits");
            }
        } else if host > 0 && raw & ((1u128 << host) - 1) != 0 {
            return Err("hostBits");
        }
        return Ok(if length == bits {
            ip.to_string()
        } else {
            format!("{ip}/{length}")
        });
    }
    if text.contains(':') || prefix.is_some() {
        return Err("address");
    }
    let wildcard = text.starts_with("*.");
    let domain = if wildcard { &text[2..] } else { text };
    let domain = domain
        .strip_suffix('.')
        .unwrap_or(domain)
        .to_ascii_lowercase();
    if domain.is_empty()
        || domain.len() > 253
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.split('.').any(|part| {
            part.is_empty()
                || part.len() > 63
                || part
                    .bytes()
                    .any(|b| !b.is_ascii_alphanumeric() && b != b'-' && b != b'_')
        })
        || domain
            .rsplit('.')
            .next()
            .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(if wildcard { "wildcard" } else { "domain" });
    }
    Ok(if wildcard {
        format!("*.{domain}")
    } else {
        domain
    })
}
pub fn port(input: &str) -> Result<String, &'static str> {
    let text = input.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let mut parts = text.split('-');
    let parse = |s: &str| {
        let s = s.trim();
        if s.starts_with('0') {
            return None;
        }
        s.parse::<u16>().ok().filter(|n| *n > 0)
    };
    let first = parse(parts.next().unwrap()).ok_or("port")?;
    let last = parts
        .next()
        .map(parse)
        .unwrap_or(Some(first))
        .ok_or("port")?;
    if first > last || parts.next().is_some() {
        return Err("port");
    }
    Ok(if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    })
}
pub fn validate(form: &Value, app: &str) -> Result<Value, &'static str> {
    if !(app == "lordos"
        || ["desktop:", "lpk:", "lordos:"]
            .iter()
            .any(|p| app.starts_with(p)))
    {
        return Err("app");
    }
    let dst = destination(form["dst"].as_str().unwrap_or(""))?;
    let port = port(form["port"].as_str().unwrap_or(""))?;
    let proto = form["proto"].as_str().unwrap_or("");
    let action = form["action"].as_str().unwrap_or("deny");
    let dir = form["dir"].as_str().unwrap_or("out");
    if !["", "tcp", "udp"].contains(&proto)
        || !["allow", "deny"].contains(&action)
        || !["in", "out"].contains(&dir)
    {
        return Err("invalid");
    };
    if dst == "any" && port.is_empty() && proto.is_empty() && dir == "out" {
        return Err("isDefault");
    };
    Ok(json!({"app":app,"dst":dst,"port":port,"proto":proto,"action":action,"dir":dir}))
}
