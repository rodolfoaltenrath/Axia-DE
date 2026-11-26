use super::*;

/// Writes the D-Bus wire format, little or big endian.
#[derive(Default)]
struct Body {
    bytes: Vec<u8>,
    big: bool,
}
impl Body {
    fn align(&mut self, to: usize) {
        while self.bytes.len() % to != 0 {
            self.bytes.push(0);
        }
    }
    fn u32(&mut self, v: u32) {
        self.align(4);
        let b = if self.big { v.to_be_bytes() } else { v.to_le_bytes() };
        self.bytes.extend(b);
    }
    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.bytes.extend(s.as_bytes());
        self.bytes.push(0);
    }
    fn signature(&mut self, s: &str) {
        self.bytes.push(s.len() as u8);
        self.bytes.extend(s.as_bytes());
        self.bytes.push(0);
    }
    fn array(&mut self, to: usize, inside: impl FnOnce(&mut Self)) {
        self.u32(0);
        let length = self.bytes.len() - 4;
        self.align(to);
        let start = self.bytes.len();
        inside(self);
        let n = (self.bytes.len() - start) as u32;
        let b = if self.big { n.to_be_bytes() } else { n.to_le_bytes() };
        self.bytes[length..length + 4].copy_from_slice(&b);
    }
    fn option(&mut self, key: &str, signature: &str, value: impl FnOnce(&mut Self)) {
        self.align(8);
        self.string(key);
        self.signature(signature);
        value(self);
    }
    fn filter(&mut self, name: &str, patterns: &[(u32, String)]) {
        self.align(8);
        self.string(name);
        self.array(8, |b| {
            for (kind, value) in patterns {
                b.align(8);
                b.u32(*kind);
                b.string(value);
            }
        });
    }
}
fn request(big: bool, title: &str, options: impl FnOnce(&mut Body)) -> Vec<u8> {
    let mut body = Body { big, ..Body::default() };
    body.string("");
    body.string(title);
    body.array(8, options);
    body.bytes
}
fn asked(member: &str, body: &[u8], big: bool) -> Box<Chooser> {
    let mut out: Box<Chooser> = Box::new(unsafe { std::mem::zeroed() });
    let member = std::ffi::CString::new(member).unwrap();
    unsafe { axia_portal_chooser(member.as_ptr(), body.as_ptr(), body.len(), c_int::from(big), &mut *out) };
    out
}
fn text(field: &[u8]) -> &str {
    CStr::from_bytes_until_nul(field).expect("terminated").to_str().unwrap()
}
fn globs(list: &[&str]) -> Vec<(u32, String)> {
    list.iter().map(|s| (0, (*s).to_owned())).collect()
}

/// What PhotoCraft sends through zenity: 35 filters, the first with every format in three
/// spellings. The portal used to refuse it for having more than 128 patterns in one filter.
#[test]
fn a_big_filter_list_still_opens_with_every_format() {
    let extensions: Vec<String> = (0..71).map(|i| format!("ext{i}")).collect();
    let every: Vec<(u32, String)> = extensions
        .iter()
        .flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_uppercase()), format!("*.[eE][xX]{}", &e[2..])])
        .map(|p| (0, p))
        .collect();
    assert_eq!(every.len(), 213);
    for big in [false, true] {
        let body = request(big, "Open", |b| {
            b.option("handle_token", "s", |b| b.string("gtk123"));
            b.option("multiple", "b", |b| b.u32(1));
            b.option("modal", "b", |b| b.u32(1));
            b.option("filters", "a(sa(us))", |b| {
                b.array(8, |b| {
                    b.filter("All Formats", &every);
                    for i in 0..34 {
                        b.filter(&format!("F{i}"), &every[i * 6..i * 6 + 6]);
                    }
                })
            });
        });
        let out = asked("OpenFile", &body, big);
        assert_eq!((text(&out.token), text(&out.mode), text(&out.title)), ("gtk123", "open", "Open"));
        assert_eq!((out.multiple, out.filters, out.ignored), (1, 35, 0));
        let lines: Vec<&str> = text(&out.patterns).lines().collect();
        assert_eq!(lines.len(), 213, "every pattern once");
        assert_eq!(lines[0], "g:*.ext0");
    }
}

#[test]
fn the_current_filter_wins_and_an_all_files_filter_shows_everything() {
    let body = request(false, "Save", |b| {
        b.option("filters", "a(sa(us))", |b| b.array(8, |b| {
            b.filter("PNG", &globs(&["*.png"]));
            b.filter("JPEG", &globs(&["*.jpg"]));
        }));
        b.option("current_filter", "(sa(us))", |b| b.filter("JPEG", &[(0, "*.jpg".into()), (1, "image/jpeg".into())]));
        b.option("current_name", "s", |b| b.string("photo.jpg"));
        b.option("current_folder", "ay", |b| b.array(1, |b| b.bytes.extend(b"/home/u/Imagens\0")));
    });
    let out = asked("SaveFile", &body, false);
    assert_eq!(text(&out.mode), "save");
    assert_eq!(text(&out.patterns), "g:*.jpg\nm:image/jpeg\n");
    assert_eq!((text(&out.name), text(&out.folder)), ("photo.jpg", "/home/u/Imagens"));
    for everything in [(0, "*"), (0, "*.*"), (1, "*/*"), (1, "application/octet-stream")] {
        let body = request(false, "", |b| {
            b.option("filters", "a(sa(us))", |b| b.array(8, |b| {
                b.filter("PNG", &globs(&["*.png"]));
                b.filter("All", &[(everything.0, everything.1.into())]);
            }));
        });
        assert_eq!(text(&asked("OpenFile", &body, false).patterns), "", "{everything:?}");
    }
}

#[test]
fn what_cannot_be_honored_is_left_out_and_the_dialog_still_opens() {
    let many: Vec<(u32, String)> = (0..MOST_PATTERNS + 1).map(|i| (0, format!("*.{i}"))).collect();
    let long = "ç".repeat(2000);
    let body = request(false, &long, |b| {
        b.option("handle_token", "s", |b| b.string("bad/token"));
        b.option("current_name", "s", |b| b.string("../../etc/passwd"));
        b.option("current_folder", "ay", |b| b.array(1, |b| b.bytes.extend(b"relative\0")));
        b.option("filters", "a(sa(us))", |b| b.array(8, |b| b.filter("Too many", &many)));
        // something newer than this code, with a nested type
        b.option("future", "a{s(ib)}", |b| b.array(8, |b| {
            b.align(8);
            b.string("k");
            b.align(8);
            b.u32(4);
            b.u32(1);
        }));
        b.option("multiple", "b", |b| b.u32(1));
    });
    let out = asked("OpenFile", &body, false);
    assert_eq!(text(&out.token), "");
    assert_eq!(text(&out.name), "");
    assert_eq!(text(&out.folder), "");
    assert_eq!(text(&out.patterns), "", "a filter that does not fit shows every file");
    assert_eq!((out.multiple, out.ignored), (1, 1), "what came after the unknown option was read");
    assert!(text(&out.title).len() <= 1024 && text(&out.title).chars().all(|c| c == 'ç'));
}

#[test]
fn application_choices_are_answered_with_their_defaults() {
    let body = request(false, "", |b| {
        b.option("choices", "a(ssa(ss)s)", |b| b.array(8, |b| {
            for (id, options, initial) in [
                ("encoding", vec![("utf8", "UTF-8"), ("latin1", "Latin 1")], "latin1"),
                ("ending", vec![("unix", "Unix")], ""),
                ("backup", vec![], ""),
            ] {
                b.align(8);
                b.string(id);
                b.string("Label");
                b.array(8, |b| {
                    for (option, label) in &options {
                        b.align(8);
                        b.string(option);
                        b.string(label);
                    }
                });
                b.string(initial);
            }
        }));
        b.option("directory", "b", |b| b.u32(1));
    });
    let out = asked("OpenFile", &body, false);
    assert_eq!(text(&out.choices), "encoding\tlatin1\nending\tunix\nbackup\tfalse\n");
    assert_eq!(text(&out.mode), "folder");
}

#[test]
fn several_files_to_save_and_a_current_file() {
    let body = request(false, "", |b| {
        b.option("files", "aay", |b| b.array(4, |b| {
            for name in [&b"a.txt\0"[..], b"../b\0", b"c.txt\0"] {
                b.array(1, |b| b.bytes.extend(name));
            }
        }));
    });
    let out = asked("SaveFiles", &body, false);
    assert_eq!((text(&out.mode), text(&out.name), out.ignored), ("saveFiles", "a.txt\nc.txt", 1));
    let body = request(false, "", |b| b.option("current_file", "ay", |b| b.array(1, |b| b.bytes.extend(b"/home/u/doc.psd\0"))));
    let out = asked("SaveFile", &body, false);
    assert_eq!((text(&out.folder), text(&out.name)), ("/home/u", "doc.psd"));
}

#[test]
fn broken_or_hostile_bodies_neither_crash_nor_loop() {
    let good = request(false, "Open", |b| {
        b.option("handle_token", "s", |b| b.string("t1"));
        b.option("filters", "a(sa(us))", |b| b.array(8, |b| b.filter("PNG", &globs(&["*.png"]))));
    });
    for cut_at in 0..good.len() {
        let out = asked("OpenFile", &good[..cut_at], false);
        assert_eq!(text(&out.mode), "open");
    }
    for i in 0..good.len() {
        for value in [0, 1, 0x7f, 0xff] {
            let mut bad = good.clone();
            bad[i] = value;
            asked("SaveFile", &bad, i % 2 == 0);
        }
    }
    let deep = request(false, "", |b| b.option("x", &"a".repeat(200), |b| b.u32(0)));
    asked("OpenFile", &deep, false);
    asked("OpenFile", &[], false);
}
