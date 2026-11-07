use super::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

const MAX_APPS: usize = 1024;
#[repr(C)]
#[derive(Clone, PartialEq)]
pub struct DesktopApp {
    pub id: [c_char; 160],
    pub name: [c_char; 160],
    pub generic: [c_char; 160],
    pub comment: [c_char; 320],
    pub keywords: [c_char; 320],
    pub categories: [c_char; 256],
    pub name_c: [c_char; 160],
    pub wmclass: [c_char; 160],
    pub mime: [c_char; 1024],
    pub icon: [c_char; 4096],
    pub file: [c_char; 4096],
    pub bin: [c_char; 128],
    pub user: i32,
}
impl Default for DesktopApp {
    fn default() -> Self {
        Self {
            id: [0; 160],
            name: [0; 160],
            generic: [0; 160],
            comment: [0; 320],
            keywords: [0; 320],
            categories: [0; 256],
            name_c: [0; 160],
            wmclass: [0; 160],
            mime: [0; 1024],
            icon: [0; 4096],
            file: [0; 4096],
            bin: [0; 128],
            user: 0,
        }
    }
}
#[derive(Default)]
pub(super) struct Action {
    pub name: Vec<u8>,
    pub label: Vec<u8>,
    pub exec: Vec<u8>,
    rank: i32,
    has_label: bool,
}
#[derive(Default)]
pub(super) struct Entry {
    pub app: DesktopApp,
    pub exec: Vec<u8>,
    pub tryexec: Vec<u8>,
    pub icon: Vec<u8>,
    pub cwd: Vec<u8>,
    only: Vec<u8>,
    not: Vec<u8>,
    actions: Vec<u8>,
    pub act: Vec<Action>,
    pub hidden: bool,
    nodisplay: bool,
    pub terminal: bool,
    pub is_app: bool,
    has_name: bool,
}
pub(super) fn action_valid(s: &[u8]) -> bool {
    !s.is_empty() && s.len() < 64 && s.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}
pub(super) fn id_valid(s: &[u8]) -> bool {
    !s.is_empty()
        && s.len() <= 62
        && !b".-".contains(&s[0])
        && !s.starts_with(b"lpk-")
        && !s.windows(2).any(|w| w == b"..")
        && s.iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
}
fn rank(loc: Option<&[u8]>, lang: &[u8]) -> i32 {
    let Some(loc) = loc else {
        return 1;
    };
    // Preserve the C reader's language/country prefixes, including locales such
    // as en.UTF-8 and pt_BR.UTF-8 supplied by existing callers.
    let len = lang
        .iter()
        .take_while(|b| b.is_ascii_lowercase())
        .count()
        .min(7);
    let ll = &lang[..len];
    let rest = &lang[len..];
    let separators = rest.iter().take_while(|b| b"-_".contains(b)).count();
    let cc = if separators > 0 {
        &rest[separators..]
    } else {
        b""
    };
    let cc = &cc[..cc
        .iter()
        .take_while(|b| b.is_ascii_alphabetic())
        .count()
        .min(7)];
    let mut want = ll.to_vec();
    want.push(b'_');
    want.extend_from_slice(cc);
    if !cc.is_empty() && loc == want {
        3
    } else if loc == ll {
        2
    } else {
        0
    }
}
fn bounded(s: &[u8], n: usize) -> Vec<u8> {
    s[..s.len().min(n - 1)].to_vec()
}
fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' && i + 1 < s.len() {
            i += 1;
            out.push(match s[i] {
                b's' => b' ',
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                b => b,
            });
        } else {
            out.push(s[i]);
        }
        i += 1;
    }
    out
}
pub(super) fn parse(text: &[u8], file: &[u8], lang: &[u8]) -> Entry {
    let mut e = Entry::default();
    let mut main = false;
    let mut action = None;
    let mut ranks = [0; 4];
    // Match fgets' original 2047-byte chunks and C string boundaries.
    for raw in text.split_inclusive(|b| *b == b'\n') {
        for chunk in raw.chunks(2047) {
            let line = &chunk[..chunk
                .iter()
                .position(|b| b"\r\n\0".contains(b))
                .unwrap_or(chunk.len())];
            if line.starts_with(b"[") {
                main = line == b"[Desktop Entry]";
                action = None;
                if let Some(name) = line
                    .strip_prefix(b"[Desktop Action ")
                    .and_then(|v| v.strip_suffix(b"]"))
                {
                    if action_valid(name)
                        && e.act.len() < 16
                        && !e.act.iter().any(|a| a.name == name)
                    {
                        e.act.push(Action {
                            name: name.to_vec(),
                            ..Action::default()
                        });
                        action = Some(e.act.len() - 1);
                    }
                }
                continue;
            }
            if (!main && action.is_none()) || line.starts_with(b"#") || line.is_empty() {
                continue;
            }
            let Some(eq) = line.iter().position(|b| *b == b'=') else {
                continue;
            };
            let k = &line[..eq];
            let k = &k[..k.iter().rposition(|b| *b != b' ').map_or(0, |i| i + 1)];
            let v = &line[eq + 1..];
            let v = &v[v.iter().take_while(|b| **b == b' ').count()..];
            let (k, loc) = if let Some(i) = k.iter().position(|b| *b == b'[') {
                let l = &k[i + 1..];
                let l = &l[..l.iter().position(|b| b"]@.".contains(b)).unwrap_or(l.len())];
                (&k[..i], Some(l))
            } else {
                (k, None)
            };
            let r = rank(loc, lang);
            if let Some(i) = action {
                let a = &mut e.act[i];
                if k == b"Name" && (r != 0 || loc.is_none()) && r >= a.rank {
                    a.rank = r;
                    a.label = bounded(v, 160);
                    a.has_label = true;
                } else if k == b"Exec" && loc.is_none() {
                    a.exec = bounded(v, 1024);
                }
                continue;
            }
            let slot = match k {
                b"Name" => Some(0),
                b"GenericName" => Some(1),
                b"Comment" => Some(2),
                b"Keywords" => Some(3),
                _ => None,
            };
            if let Some(i) = slot {
                if k == b"Name" {
                    e.has_name = true;
                    if loc.is_none() {
                        e.app.name_c = field(v);
                    }
                }
                if (r != 0 || loc.is_none()) && r >= ranks[i] {
                    ranks[i] = r;
                    match i {
                        0 => e.app.name = field(v),
                        1 => e.app.generic = field(v),
                        2 => e.app.comment = field(v),
                        _ => e.app.keywords = field(v),
                    }
                }
                continue;
            }
            if loc.is_some() {
                continue;
            }
            match k {
                b"Type" => e.is_app = v == b"Application",
                b"Exec" => e.exec = bounded(v, 1024),
                b"TryExec" => e.tryexec = bounded(v, 256),
                b"Icon" => e.icon = bounded(v, 256),
                b"Path" => e.cwd = bounded(v, 512),
                b"Categories" => e.app.categories = field(v),
                b"MimeType" => e.app.mime = field(v),
                b"StartupWMClass" => e.app.wmclass = field(v),
                b"OnlyShowIn" => e.only = bounded(v, 256),
                b"NotShowIn" => e.not = bounded(v, 256),
                b"Hidden" => e.hidden = v == b"true",
                b"NoDisplay" => e.nodisplay = v == b"true",
                b"Terminal" => e.terminal = v == b"true",
                b"Actions" => e.actions = bounded(v, 512),
                _ => {}
            }
        }
    }
    e.app.name = field(&unescape(value(&e.app.name)));
    e.app.name_c = field(&unescape(value(&e.app.name_c)));
    e.app.generic = field(&unescape(value(&e.app.generic)));
    e.app.comment = field(&unescape(value(&e.app.comment)));
    for a in &mut e.act {
        a.label = unescape(&a.label);
    }
    e.app.file = field(file);
    e
}
pub(super) fn read(file: &[u8], lang: &[u8]) -> Option<Entry> {
    // Cap malicious .desktop input while leaving normal entries unchanged.
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(Path::new(OsStr::from_bytes(file)))
        .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = Vec::new();
    f.take(1024 * 1024 + 1).read_to_end(&mut text).ok()?;
    if text.len() > 1024 * 1024 {
        return None;
    }
    Some(parse(&text, file, lang))
}
impl Entry {
    pub fn action(&self, name: &[u8]) -> Option<&Action> {
        if !action_valid(name) || !list_has(&self.actions, name) {
            return None;
        }
        self.act
            .iter()
            .find(|a| a.name == name && a.has_label && !a.exec.is_empty())
    }
    pub fn ordered_actions(&self) -> Vec<&Action> {
        let mut names = HashSet::new();
        self.actions
            .split(|b| *b == b';')
            .filter(|s| names.insert(*s))
            .filter_map(|s| self.action(s))
            .collect()
    }
}
pub(super) fn program(prog: &[u8], user: bool) -> Option<Vec<u8>> {
    if prog.is_empty() {
        return None;
    }
    if prog.contains(&b'/') {
        return (prog[0] == b'/'
            && unsafe { libc::access(cstring(prog).as_ptr(), libc::X_OK) } == 0)
            .then(|| prog.to_vec());
    }
    let mut dirs: Vec<PathBuf> = [
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/local/games",
        "/usr/games",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if user {
        if let Some(h) = defaults::home() {
            dirs.push(h.join(".local/bin"));
        }
    }
    dirs.into_iter()
        .map(|d| d.join(OsStr::from_bytes(prog)))
        .find(|p| {
            p.is_file()
                && unsafe { libc::access(cstring(p.as_os_str().as_bytes()).as_ptr(), libc::X_OK) }
                    == 0
        })
        .map(|p| p.as_os_str().as_bytes().to_vec())
}
fn exec_bin(exec: &[u8]) -> Vec<u8> {
    let mut p = exec;
    loop {
        p = p.trim_ascii_start();
        if p.is_empty() {
            return Vec::new();
        }
        let quoted = p[0] == b'"';
        if quoted {
            p = &p[1..];
        }
        let n = p
            .iter()
            .position(|b| if quoted { *b == b'"' } else { *b == b' ' })
            .unwrap_or(p.len());
        let part = &p[..n];
        if part != b"env" && !part.contains(&b'=') {
            return part
                .rsplit(|b| *b == b'/')
                .next()
                .unwrap_or_default()
                .to_vec();
        }
        p = &p[(n + 1).min(p.len())..];
    }
}
fn icon(name: &[u8]) -> Vec<u8> {
    if name.is_empty() || name.windows(2).any(|s| s == b"..") {
        return Vec::new();
    }
    let is_png = |p: &Path| p.as_os_str().as_bytes().ends_with(b".png") && p.is_file();
    if name[0] == b'/' {
        return if is_png(Path::new(OsStr::from_bytes(name))) {
            name.to_vec()
        } else {
            Vec::new()
        };
    }
    let mut roots = Vec::new();
    if let Some(h) = defaults::home() {
        roots.push(h.join(".local/share/icons/hicolor"));
    }
    roots.push(PathBuf::from("/usr/share/icons/hicolor"));
    let mut filename = name.to_vec();
    filename.extend_from_slice(b".png");
    for root in roots {
        for size in [64, 48, 128, 96, 256, 72, 32, 512] {
            let p = root
                .join(format!("{size}x{size}/apps"))
                .join(OsStr::from_bytes(&filename));
            if is_png(&p) {
                return p.as_os_str().as_bytes().to_vec();
            }
        }
    }
    let p = Path::new("/usr/share/pixmaps").join(OsStr::from_bytes(&filename));
    if is_png(&p) {
        return p.as_os_str().as_bytes().to_vec();
    }
    let mut suffix = vec![b'_'];
    suffix.extend_from_slice(&filename);
    if let Ok(entries) = fs::read_dir("/var/lib/lordos/pkg/icons/64") {
        for e in entries.flatten() {
            if e.file_name().as_bytes().ends_with(&suffix) && is_png(&e.path()) {
                return e.path().as_os_str().as_bytes().to_vec();
            }
        }
    }
    Vec::new()
}
#[derive(Default)]
struct Catalog {
    apps: Vec<DesktopApp>,
    lang: Vec<u8>,
    dirs: [PathBuf; 2],
    mt: [i128; 2],
    ready: bool,
    /// Bumped only when a re-read changed the list, so the shell re-syncs only then.
    generation: u64,
}
thread_local! {static CATALOG:RefCell<Catalog>=RefCell::new(Catalog::default());}
pub(super) fn invalidate() {
    CATALOG.with(|c| c.borrow_mut().ready = false);
}
pub(super) fn language() -> Vec<u8> {
    CATALOG.with(|c| {
        let c = c.borrow();
        if c.lang.is_empty() {
            b"en".to_vec()
        } else {
            c.lang.clone()
        }
    })
}
fn refresh(c: &mut Catalog, lang: &[u8]) {
    let dirs = [
        defaults::home()
            .unwrap_or_else(|| PathBuf::from("/nonexistent"))
            .join(".local/share/applications"),
        defaults::system(b"/usr/share/applications"),
    ];
    let mt = dirs.each_ref().map(|d| {
        fs::metadata(d).map_or(0, |m| {
            m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128
        })
    });
    if c.ready && c.lang == lang && c.dirs == dirs && c.mt == mt {
        return;
    }
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for (d, dir) in dirs.iter().enumerate() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for f in entries.flatten() {
            if apps.len() >= MAX_APPS {
                break;
            }
            let name = f.file_name();
            let Some(stem) = name.as_bytes().strip_suffix(b".desktop") else {
                continue;
            };
            let id: Vec<u8> = stem
                .iter()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"._-".contains(b) {
                        *b
                    } else {
                        b'-'
                    }
                })
                .collect();
            if !id_valid(&id) || seen.contains(&id) {
                continue;
            }
            // Match the old bounded shadow table, including hidden user entries.
            if seen.len() < MAX_APPS * 2 {
                seen.insert(id.clone());
            }
            let file = f.path();
            if file.as_os_str().as_bytes().len() >= 4096 {
                continue;
            }
            let Some(mut e) = read(file.as_os_str().as_bytes(), lang) else {
                continue;
            };
            if !e.is_app
                || e.hidden
                || e.nodisplay
                || e.terminal
                || !e.has_name
                || e.exec.is_empty()
            {
                continue;
            }
            if !e.only.is_empty() && !list_has(&e.only, b"Axia") {
                continue;
            }
            if list_has(&e.not, b"Axia") {
                continue;
            }
            if !e.tryexec.is_empty() && program(&e.tryexec, d == 0).is_none() {
                continue;
            }
            e.app.id = field(&id);
            e.app.user = (d == 0) as i32;
            e.app.icon = field(&icon(&e.icon));
            e.app.bin = field(&exec_bin(&e.exec));
            apps.push(e.app);
        }
    }
    apps.sort_by(|a, b| {
        value(&a.name)
            .to_ascii_lowercase()
            .cmp(&value(&b.name).to_ascii_lowercase())
    });
    let generation = if c.ready && c.apps == apps {
        c.generation
    } else {
        c.generation + 1
    };
    *c = Catalog {
        apps,
        lang: lang.to_vec(),
        dirs,
        mt,
        ready: true,
        generation,
    };
}
/// The catalog's generation for `lang`, re-reading the folders if their mtime changed. Packages are
/// installed by dnf or GNOME Software outside the session, so the shell asks this cheaply (two
/// stats when nothing changed) and only fetches the list when the number moved.
pub(super) fn generation(lang: &[u8]) -> u64 {
    CATALOG.with(|c| {
        let mut c = c.borrow_mut();
        refresh(&mut c, lang);
        c.generation
    })
}
pub(super) fn list(lang: &[u8]) -> Vec<DesktopApp> {
    CATALOG.with(|c| {
        let mut c = c.borrow_mut();
        refresh(&mut c, lang);
        c.apps.clone()
    })
}
pub(super) fn find(id: &[u8]) -> Option<DesktopApp> {
    if !id_valid(id) {
        return None;
    }
    let lang = language();
    CATALOG.with(|c| {
        let mut c = c.borrow_mut();
        refresh(&mut c, &lang);
        c.apps.iter().find(|a| value(&a.id) == id).cloned()
    })
}
#[no_mangle]
pub unsafe extern "C" fn apps_generation(lang: *const c_char) -> u64 {
    generation(bytes(lang))
}
#[no_mangle]
pub unsafe extern "C" fn apps_list(lang: *const c_char, out: *mut *const DesktopApp) -> i32 {
    if out.is_null() {
        return 0;
    }
    CATALOG.with(|c| {
        let mut c = c.borrow_mut();
        refresh(&mut c, bytes(lang));
        *out = c.apps.as_ptr();
        c.apps.len() as i32
    })
}
#[repr(C)]
pub struct AppAction {
    name: [c_char; 64],
    label: [c_char; 160],
}
#[no_mangle]
pub unsafe extern "C" fn apps_actions(
    id: *const c_char,
    lang: *const c_char,
    out: *mut AppAction,
    max: i32,
) -> i32 {
    if out.is_null() || max <= 0 {
        return 0;
    }
    let Some(a) = find(bytes(id)) else {
        return 0;
    };
    let lang = bytes(lang);
    let Some(e) = read(value(&a.file), if lang.is_empty() { b"en" } else { lang }) else {
        return 0;
    };
    let acts = e.ordered_actions();
    let n = acts.len().min(max as usize);
    for (i, a) in acts.iter().take(n).enumerate() {
        out.add(i).write(AppAction {
            name: field(&a.name),
            label: field(&a.label),
        });
    }
    n as i32
}
