//! Unit tests. The full suite (with the real TZif reader and a fuzzer) is
//! server/tests/mail_test.c, which links this crate through axia-codecs.

use crate::c::*;
use crate::*;
use core::ffi::c_char;

/// Stand-ins for server/src/ui/tzif.c: every zone is UTC.
pub(crate) mod tz {
    use core::ffi::{c_char, c_int};

    pub(crate) unsafe fn tz_zone_valid(z: *const c_char) -> c_int {
        let z = unsafe { crate::c::cs(z) };
        (!z.is_empty() && !z.starts_with(b"/") && !z.windows(2).any(|w| w == b"..")) as c_int
    }

    pub(crate) unsafe fn tzif_lookup(
        _buf: *const u8,
        _n: usize,
        _t: i64,
        out: *mut crate::ical::TzInfo,
    ) -> c_int {
        unsafe { (*out).utoff = 0 };
        0
    }
}

fn c(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

unsafe fn take(p: *mut c_char, n: usize) -> Vec<u8> {
    let v = unsafe { bytes(p, n) }.to_vec();
    unsafe { libc::free(p.cast()) };
    v
}

#[test]
fn layout_matches_c_headers() {
    // sizeof/offsetof measured from the C headers on LP64 (x86_64, aarch64);
    // server/tests/mail_test.c checks the same table against the headers.
    let want = [
        800, 204808, 416, 64, 28, 32, 88, 72, 468, 4000, 216, 32, 56, 768, 1288, 3680, 3740, 200,
        24, 40,
    ];
    for (i, w) in want.iter().enumerate() {
        assert_eq!(
            axia_mail_layout(i as core::ffi::c_int),
            *w,
            "layout entry {i}"
        );
    }
}

#[test]
fn mime_words_and_params() {
    let words = decode_words_test("=?UTF-8?B?w6lsw6h2ZQ==?= =?ISO-8859-1?Q?caf=E9?= x");
    // whitespace between two encoded words is dropped (RFC 2047)
    assert_eq!(words, "élèvecafé x");
    let mut out = [0 as c_char; 64];
    let v = c("attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf");
    let n = c("filename");
    assert_eq!(
        unsafe { mime::mime_param(v.as_ptr().cast(), n.as_ptr().cast(), out.as_mut_ptr(), 64) },
        1
    );
    assert_eq!(zc(&out), "résumé.pdf".as_bytes());
}

fn decode_words_test(s: &str) -> String {
    let mut out = [0 as c_char; 256];
    let i = c(s);
    unsafe { mime::mime_decode_words(i.as_ptr().cast(), out.as_mut_ptr(), out.len()) };
    String::from_utf8(zc(&out).to_vec()).unwrap()
}

#[test]
fn mime_tree() {
    let msg = b"Content-Type: multipart/mixed; boundary=xx\r\n\r\n--xx\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\r\nol\xe1\r\n--xx\r\nContent-Type: application/pdf; name=\"../../a.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--xx--\r\n";
    let mut m: Box<MimeMsg> = Box::new(unsafe { core::mem::zeroed() });
    unsafe { mime::mime_parse(msg.as_ptr().cast(), msg.len(), &mut *m) };
    assert_eq!(m.nparts, 3);
    assert_eq!(zc(&m.parts[1].charset), b"iso-8859-1");
    assert_eq!(zc(&m.parts[2].filename), b"a.pdf");
    let mut n = 0;
    let t = unsafe { take(mime::mime_part_text(&m.parts[1], &mut n), n) };
    assert_eq!(t, "olá".as_bytes());
    let b = unsafe { take(mime::mime_decode_body(&m.parts[2], &mut n), n) };
    assert_eq!(b, b"%PDF");
}

#[test]
fn mime_dates_and_addresses() {
    let d = c("Tue, 3 Jun 2025 10:22:01 -0300");
    assert_eq!(unsafe { mime::mime_date(d.as_ptr().cast()) }, 1748956921);
    let bad = c("Tue, 3 Jun 2025 25: -0300");
    assert_eq!(unsafe { mime::mime_date(bad.as_ptr().cast()) }, 0);
    let a = c("\"Doe, J\" <j@x.org>, plain@y.com (Plain), group: g@z.net;");
    let mut out: [MimeAddr; 4] = unsafe { core::mem::zeroed() };
    let n = unsafe { mime::mime_addrs(a.as_ptr().cast(), out.as_mut_ptr(), 4) };
    assert_eq!(n, 3);
    assert_eq!(zc(&out[0].name), b"Doe, J");
    assert_eq!(zc(&out[1].name), b"Plain");
    assert_eq!(zc(&out[2].email), b"g@z.net");
}

#[test]
fn xml_namespaces() {
    let x = b"<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"><d:response><d:href> /a&amp;b </d:href></d:response></d:multistatus>";
    let mut err = std::ptr::null();
    let root = unsafe { xml::mx_parse(x.as_ptr().cast(), x.len(), &mut err) };
    assert!(!root.is_null());
    let dav = c("DAV:");
    let href = c("href");
    let h = unsafe { xml::mx_find(root, dav.as_ptr().cast(), href.as_ptr().cast()) };
    assert!(!h.is_null());
    assert_eq!(unsafe { cs(xml::mx_text(h)) }, b" /a&b ");
    unsafe { xml::mx_free(root) };
    let d = b"<!DOCTYPE x [<!ENTITY a \"b\">]><x/>";
    assert!(unsafe { xml::mx_parse(d.as_ptr().cast(), d.len(), &mut err) }.is_null());
    assert_eq!(unsafe { cs(err) }, b"DOCTYPE not allowed");
}

#[test]
fn html_strips_and_links() {
    let h = b"<p style=\"color:#f00\">Hi &amp; <b>bye</b><script>x()</script><img src=\"https://t.example/p.gif\" width=1 height=1><a href=\"https://evil.example\">www.bank.com</a></p>";
    let mut st = HtmlStats::default();
    let mut n = 0;
    let j = unsafe {
        take(
            html::html_sanitize(
                h.as_ptr().cast(),
                h.len(),
                &mut n,
                &mut st,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            n,
        )
    };
    let j = String::from_utf8(j).unwrap();
    assert!(j.contains("\"x\":\"Hi &\""), "{j}");
    assert!(
        j.contains("\"x\":\" bye\",\"s\":{\"_\":0,\"b\":1,\"c\":\"#ff0000\"}"),
        "{j}"
    );
    assert!(!j.contains("x()"));
    assert_eq!(st.trackers, 1);
    assert_eq!(st.warn, 1);
    assert!(j.contains("\"w\":\"deceptive\""));
    let t = unsafe { take(html::html_to_text(h.as_ptr().cast(), h.len(), &mut n), n) };
    assert_eq!(t, b"Hi & byewww.bank.com");
}

#[test]
fn ical_events_and_rrule() {
    let ics = b"BEGIN:VCALENDAR\r\nMETHOD:request\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Stand\\, up\r\nDTSTART:20250106T090000Z\r\nDURATION:PT15M\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4\r\nEXDATE:20250108T090000Z\r\nBEGIN:VALARM\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut ev: *mut IcEvent = std::ptr::null_mut();
    let mut method = [0 as c_char; 16];
    let n = unsafe {
        ical::ical_events(
            ics.as_ptr().cast(),
            ics.len(),
            &mut ev,
            method.as_mut_ptr(),
            16,
        )
    };
    assert_eq!(n, 1);
    assert_eq!(zc(&method), b"REQUEST");
    let e = unsafe { &*ev };
    assert_eq!(zc(&e.summary), b"Stand, up");
    assert_eq!(e.end - e.start, 900);
    assert_eq!((e.nalarms, e.alarms[0]), (1, 10));
    let mut got: Vec<i64> = Vec::new();
    unsafe extern "C" fn push(u: *mut core::ffi::c_void, t: i64) {
        unsafe { (*u.cast::<Vec<i64>>()).push(t) }
    }
    let k = unsafe {
        ical::ical_expand(
            ev,
            0,
            i64::MAX / 2,
            100,
            Some(push),
            (&mut got as *mut Vec<i64>).cast(),
        )
    };
    // COUNT=4 occurrences, one removed by EXDATE
    assert_eq!(k, 3);
    // Mon 6, (Wed 8 excluded), Mon 13, Wed 15 January 2025 at 09:00 UTC
    assert_eq!(got, vec![1736154000, 1736758800, 1736931600]);
    unsafe { ical::ical_events_free(ev, n) };
}

#[test]
fn ical_build_folds_and_escapes() {
    let uid = c("u1");
    let sum = c(&"a;b,c ".repeat(20));
    let x = IcBuild {
        is_todo: 0,
        uid: uid.as_ptr().cast(),
        summary: sum.as_ptr().cast(),
        location: std::ptr::null(),
        description: std::ptr::null(),
        url: std::ptr::null(),
        rrule: std::ptr::null(),
        status: std::ptr::null(),
        categories: std::ptr::null(),
        start: 1736154000,
        end: 0,
        allday: 0,
        tzid: std::ptr::null(),
        alarms: std::ptr::null(),
        nalarms: 0,
        due: 0,
        due_allday: 0,
        priority: 0,
        completed: 0,
        flagged: 0,
        organizer: std::ptr::null(),
        organizer_name: std::ptr::null(),
        att: std::ptr::null(),
        natt: 0,
        sequence: 2,
        method: std::ptr::null(),
        parent_uid: std::ptr::null(),
        exdate: std::ptr::null(),
        nexdate: 0,
    };
    let mut n = 0;
    let out = String::from_utf8(unsafe { take(ical::ical_build(&x, &mut n), n) }).unwrap();
    assert!(out.contains("DTSTART:20250106T090000Z\r\nDTEND:20250106T100000Z\r\n"));
    assert!(out.contains("SUMMARY:a\\;b\\,c a\\;b"));
    assert!(out.lines().all(|l| l.len() <= 75));
    assert!(out.contains("\r\n "), "a long SUMMARY is folded");
}

#[test]
fn vcard_reads_and_writes() {
    let v = b"BEGIN:VCARD\r\nVERSION:2.1\r\nUID:urn:uuid:abc\r\nFN;ENCODING=QUOTED-PRINTABLE;CHARSET=ISO-8859-1:Jos=E9\r\nN:Doe;John;;;\r\nTEL;CELL:+1 555\r\nitem1.EMAIL;TYPE=INTERNET,WORK:j@x.org\r\nEND:VCARD\r\n";
    let mut b = JBuf::new();
    assert_eq!(
        unsafe { vcard::vcard_json(v.as_ptr().cast(), v.len(), &mut b) },
        1
    );
    let j = String::from_utf8(b.as_bytes().to_vec()).unwrap();
    b.free();
    assert!(j.starts_with("{\"uid\":\"abc\",\"fn\":\"José\""), "{j}");
    assert!(j.contains("\"phones\":[{\"type\":\"cell\",\"value\":\"+1 555\"}]"));
    assert!(j.contains("\"emails\":[{\"type\":\"work\",\"value\":\"j@x.org\"}]"));
    let mut uid = [0 as c_char; 32];
    unsafe { vcard::vcard_uid(v.as_ptr().cast(), v.len(), uid.as_mut_ptr(), 32) };
    assert_eq!(zc(&uid), b"abc");
    let mut n = 0;
    let out = String::from_utf8(unsafe {
        take(
            vcard::vcard_build(
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut n,
            ),
            n,
        )
    })
    .unwrap();
    assert!(out.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\n"));
    assert!(out.contains("UID:\r\nFN:\r\nN:;;;;\r\n"));
}

#[test]
fn hostile_inputs_do_not_panic() {
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let alphabet = b"<>&#;:=?\"' /\\-\r\nBEGIN:VCALENDAR END:VEVENT DTSTART RRULE=FREQ=YEARLY;BYDAY=-5SU xmlns:d=DAV: style=width:0 \xc3\xa9\xff\x00";
    for _ in 0..3000 {
        let n = (rnd() % 300) as usize;
        let s: Vec<u8> = (0..n)
            .map(|_| alphabet[(rnd() % alphabet.len() as u64) as usize])
            .collect();
        let p = s.as_ptr().cast();
        let mut m: Box<MimeMsg> = Box::new(unsafe { core::mem::zeroed() });
        unsafe {
            mime::mime_parse(p, s.len(), &mut *m);
            let mut l = 0;
            libc::free(
                html::html_sanitize(
                    p,
                    s.len(),
                    &mut l,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
                .cast(),
            );
            libc::free(html::html_to_text(p, s.len(), &mut l).cast());
            xml::mx_free(xml::mx_parse(p, s.len(), std::ptr::null_mut()));
            let mut ev: *mut IcEvent = std::ptr::null_mut();
            let k = ical::ical_events(p, s.len(), &mut ev, std::ptr::null_mut(), 0);
            for i in 0..k.max(0) as usize {
                ical::ical_expand(ev.add(i), 0, 1 << 40, 50, None, std::ptr::null_mut());
                let mut b = JBuf::new();
                ical::ical_event_json(&mut b, ev.add(i), 0);
                b.free();
            }
            if k >= 0 {
                ical::ical_events_free(ev, k);
            }
            let mut b = JBuf::new();
            vcard::vcard_json(p, s.len(), &mut b);
            b.free();
        }
    }
}
