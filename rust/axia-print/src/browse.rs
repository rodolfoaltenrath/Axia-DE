//! The mDNS lookup on the socket: DNS-SD (RFC 6762/6763) asked from an arbitrary port («legacy
//! unicast»), so it needs neither port 5353 nor avahi and changes nothing on the network. Reading
//! replies, building queries and turning the cache into services is `axia_codecs::mdns`.
use axia_codecs::mdns::{self, MdnsCache, MdnsSvc};
use std::os::fd::AsRawFd;
use std::time::Instant;

const T_PTR: i32 = 12;

/// Asks for the given service types (`"_ipp._tcp"` …) for `ms` milliseconds and returns what
/// answered (at most `max`).
pub fn browse(types: &[&str], ms: u64, max: usize) -> Vec<MdnsSvc> {
    // the printers' own socket (sec/netconn.h)
    let Ok(fd) = crate::net::socket(libc::SOCK_DGRAM, None) else { return Vec::new() };
    let raw = fd.as_raw_fd();
    let (ttl, lp) = (255u8, 1u8);
    // SAFETY: plain setsockopt calls with valid pointers.
    unsafe {
        libc::setsockopt(raw, libc::IPPROTO_IP, libc::IP_MULTICAST_TTL, (&ttl as *const u8).cast(), 1);
        libc::setsockopt(raw, libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP, (&lp as *const u8).cast(), 1);
    }
    // SAFETY: a plain C struct; zero is valid.
    let mut to: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    to.sin_family = libc::AF_INET as libc::sa_family_t;
    to.sin_port = 5353u16.to_be();
    to.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::new(224, 0, 0, 251)).to_be();
    let send = |q: &[u8]| {
        // SAFETY: `q` and `to` are valid for the call.
        unsafe { libc::sendto(raw, q.as_ptr().cast(), q.len(), 0, (&to as *const libc::sockaddr_in).cast(), size_of::<libc::sockaddr_in>() as libc::socklen_t) };
    };
    let mut cache = MdnsCache::empty();
    let full: Vec<Vec<u8>> = types.iter().take(8).map(|t| format!("{t}.local").bytes().take(63).collect()).collect();
    let ask: Vec<(&[u8], i32)> = full.iter().map(|n| (n.as_slice(), T_PTR)).collect();
    let mut pkt = [0u8; 1500];
    let id = (std::process::id() ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32)) as u16;
    let start = Instant::now();
    let elapsed = || start.elapsed().as_secs_f64() * 1e3;
    let (mut next_send, mut sends, mut followed) = (0.0f64, 0, false);
    let ms_f = ms as f64;
    while elapsed() < ms_f {
        if elapsed() >= next_send && sends < 3 {
            let n = mdns::query(&mut pkt, &ask, id);
            if n > 0 {
                send(&pkt[..n]);
            }
            sends += 1;
            next_send = elapsed() + if sends == 1 { 350.0 } else { 700.0 };
        }
        // halfway: the ones that still have no SRV or address are asked one by one
        if !followed && elapsed() > ms_f / 2.0 {
            followed = true;
            let more = cache.followups();
            let qs: Vec<(&[u8], i32)> = more.iter().map(|(n, t)| (n.as_slice(), *t)).collect();
            if !qs.is_empty() {
                let n = mdns::query(&mut pkt, &qs, id);
                if n > 0 {
                    send(&pkt[..n]);
                }
            }
        }
        let left = (ms_f - elapsed()) as i64;
        let mut w = (next_send - elapsed()) as i64;
        if sends >= 3 || w > left {
            w = left;
        }
        if !followed && w > (ms_f / 2.0 - elapsed()) as i64 {
            w = (ms_f / 2.0 - elapsed()) as i64 + 1;
        }
        let mut p = libc::pollfd { fd: raw, events: libc::POLLIN, revents: 0 };
        // SAFETY: a valid pollfd.
        if unsafe { libc::poll(&mut p, 1, w.clamp(1, i32::MAX as i64) as i32) } <= 0 {
            continue;
        }
        loop {
            // SAFETY: the buffer is ours.
            let r = unsafe { libc::recv(raw, pkt.as_mut_ptr().cast(), pkt.len(), 0) };
            if r <= 0 {
                break;
            }
            mdns::parse(&pkt[..r as usize], &mut cache);
        }
    }
    drop(fd);
    let kinds: Vec<&[u8]> = types.iter().map(|t| t.as_bytes()).collect();
    let mut out: Vec<MdnsSvc> = (0..max).map(|_| MdnsSvc::zeroed()).collect();
    let n = mdns::collect(&cache, &kinds, &mut out);
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_short_lookup_with_nobody_answering_finds_nothing_and_ends_on_time() {
        let t = Instant::now();
        let found = browse(&["_ipp._tcp", "_uscan._tcp"], 300, 8);
        assert!(found.is_empty() || found.len() <= 8);
        assert!(t.elapsed() < Duration::from_millis(1500), "took {:?}", t.elapsed());
    }
}
