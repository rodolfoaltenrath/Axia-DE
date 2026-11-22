//! `axia-fetch`: the Axia mark and a short summary of this computer for a terminal, from the same
//! collector as System Information, plus a few live facts (uptime, memory and disk in use, packages,
//! shell). The user and host names appear only on screen and only without --share, so a screenshot
//! made for sharing carries neither. The mark is pixel art drawn two pixels per character cell with half
//! blocks (▀ ▄), in the violet of the app icon (cargo xtask gen-brand); without color it is plain ASCII.
use crate::collect::Report;
use crate::fsutil::{atoi, fit, read_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Plain,
    C256,
    TrueColor,
}

#[derive(Clone, Debug, Default)]
pub struct Live {
    /// Empty with --share.
    pub user: String,
    pub host: String,
    /// -1 unknown.
    pub uptime_s: i64,
    /// MemTotal − MemAvailable.
    pub mem_used_kb: u64,
    /// The root file system, bytes.
    pub disk_used: u64,
    pub disk_total: u64,
    /// RPM packages installed, -1 unknown.
    pub packages: i32,
    /// The login shell's name.
    pub shell: String,
    /// "Wayland", "X11", "".
    pub session: String,
}

/// Installed RPM packages under `root`: one line per package from `rpm -qa`, run without a shell. The RPM
/// database is SQLite, so the count comes from rpm itself. -1 if rpm is missing or slow.
fn rpm_packages(root: &str) -> i32 {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::process::{Command, Stdio};
    let mut child = match Command::new("rpm")
        .args(["--root", if root.is_empty() { "/" } else { root }, "-qa", "--qf", "\n"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let mut out = child.stdout.take().unwrap();
    let fd = out.as_raw_fd();
    let t0 = std::time::Instant::now();
    let (mut n, mut timed_out) = (0, false);
    let mut buf = [0u8; 4096];
    loop {
        let left = 5000 - t0.elapsed().as_millis().min(5000) as i32;
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: a valid pollfd.
        if left <= 0 || unsafe { libc::poll(&mut p, 1, left) } <= 0 {
            timed_out = true;
            break;
        }
        match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += buf[..k].iter().filter(|&&c| c == b'\n').count() as i32,
        }
    }
    drop(out);
    if timed_out {
        let _ = child.kill();
    }
    let st = child.wait();
    if !timed_out && st.is_ok_and(|s| s.success()) { n } else { -1 }
}

// ---- the mark ----

/// 31 × 22 pixels: the Axia mark, an "A" whose crossbar runs past both legs, in the icon's violet from light (P)
/// at the apex to deep (B) at the feet; '.' nothing.
const MARK: [&str; 22] = [
    "..............PPP..............",
    ".............PPPPP.............",
    ".............PPPPP.............",
    "............PPPPPPP............",
    "............PPP.PPP............",
    "............PPP.PPP............",
    "...........PPP...PPP...........",
    "...........VVV...VVV...........",
    "..........VVV.....VVV..........",
    "..........VVV.....VVV..........",
    ".........VVV.......VVV.........",
    ".........VVV.......VVV.........",
    "........VVV.........VVV........",
    "..VVVVVVVVVVVVVVVVVVVVVVVVVVV..",
    "..VVVVVVVVVVVVVVVVVVVVVVVVVVV..",
    ".......BBB...........BBB.......",
    "......BBB.............BBB......",
    "......BBB.............BBB......",
    ".....BBBB.............BBBB.....",
    ".....BBB...............BBB.....",
    "....BBBB...............BBBB....",
    "....BBB.................BBB....",
];
const MARK_H: usize = MARK.len();
const MARK_W: usize = 31;

const PLAIN: [&str; 9] = [
    "               /\\              ",
    "              /  \\             ",
    "             /    \\            ",
    "            /      \\           ",
    "           /        \\          ",
    "    ======/==========\\======   ",
    "         /            \\        ",
    "        /              \\       ",
    "       /                \\      ",
];

#[derive(Clone, Copy)]
struct Pal {
    key: u8,
    r: u8,
    g: u8,
    b: u8,
    idx: u8,
}

const PAL: [Pal; 3] = [Pal { key: b'P', r: 0xb4, g: 0x6c, b: 0xff, idx: 135 }, Pal { key: b'V', r: 0x7c, g: 0x5c, b: 0xff, idx: 99 }, Pal { key: b'B', r: 0x4f, g: 0x46, b: 0xe5, idx: 62 }];

fn pal(k: u8) -> Option<Pal> {
    PAL.iter().find(|p| p.key == k).copied()
}

fn fg(b: &mut String, p: Pal, c: Color) {
    if c == Color::TrueColor {
        b.push_str(&format!("\x1b[38;2;{};{};{}m", p.r, p.g, p.b));
    } else {
        b.push_str(&format!("\x1b[38;5;{}m", p.idx));
    }
}

fn bg(b: &mut String, p: Pal, c: Color) {
    if c == Color::TrueColor {
        b.push_str(&format!("\x1b[48;2;{};{};{}m", p.r, p.g, p.b));
    } else {
        b.push_str(&format!("\x1b[48;5;{}m", p.idx));
    }
}

/// One text row of the mark: pixel rows 2y and 2y+1.
fn mark_row(b: &mut String, y: usize, c: Color) {
    if c == Color::Plain {
        b.push_str(&format!("{:<w$}", PLAIN.get(y).copied().unwrap_or(""), w = MARK_W));
        return;
    }
    for x in 0..MARK_W {
        let t = if 2 * y < MARK_H { pal(MARK[2 * y].as_bytes()[x]) } else { None };
        let u = if 2 * y + 1 < MARK_H { pal(MARK[2 * y + 1].as_bytes()[x]) } else { None };
        match (t, u) {
            (None, None) => {
                b.push(' ');
                continue;
            }
            (Some(t), Some(u)) if t.key == u.key => {
                fg(b, t, c);
                b.push('█');
            }
            (Some(t), Some(u)) => {
                fg(b, t, c);
                bg(b, u, c);
                b.push('▀');
            }
            (Some(t), None) => {
                fg(b, t, c);
                b.push('▀');
            }
            (None, Some(u)) => {
                fg(b, u, c);
                b.push('▄');
            }
        }
        b.push_str("\x1b[0m");
    }
}

// ---- live facts ----

fn meminfo_kb(txt: &str, key: &str) -> u64 {
    txt.find(key).map_or(0, |i| {
        let rest = txt[i + key.len() + 1..].trim_start();
        rest.bytes().take_while(u8::is_ascii_digit).fold(0u64, |a, c| a.saturating_mul(10).saturating_add((c - b'0') as u64))
    })
}

/// Printable, no spaces: a user or host name for the screen.
fn clean(s: &str) -> String {
    s.bytes().map(|c| if c <= b' ' || c >= 0x7f { '?' } else { c as char }).collect()
}

/// The live facts from `root` (proc, the RPM database) and this process (user, shell, disk). `share`: no user
/// or host name.
pub fn live(root: &str, share: bool) -> Live {
    let mut l = Live { uptime_s: -1, packages: -1, ..Live::default() };
    if !share {
        // SAFETY: getpwuid returns a pointer into static storage, read at once.
        unsafe {
            let pw = libc::getpwuid(libc::getuid());
            if !pw.is_null() && !(*pw).pw_name.is_null() {
                l.user = fit(&std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy(), 64);
            }
        }
        if let Some(h) = crate::fsutil::read_file_following(&format!("{root}/proc/sys/kernel/hostname"), 64) {
            let h = String::from_utf8_lossy(&h).into_owned();
            l.host = fit(h.split('\n').next().unwrap_or(""), 64);
        }
        l.user = clean(&l.user);
        l.host = clean(&l.host);
    }
    if let Some(t) = crate::fsutil::read_file_following(&format!("{root}/proc/uptime"), 128) {
        l.uptime_s = atoi(&String::from_utf8_lossy(&t));
    }
    let t = crate::fsutil::read_file_following(&format!("{root}/proc/meminfo"), 16384).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let (total, avail) = (meminfo_kb(&t, "MemTotal:"), meminfo_kb(&t, "MemAvailable:"));
    l.mem_used_kb = total.saturating_sub(avail);
    if let Ok(c) = std::ffi::CString::new(format!("{root}/")) {
        // SAFETY: statvfs fills the struct.
        unsafe {
            let mut vs: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut vs) == 0 {
                l.disk_total = vs.f_blocks as u64 * vs.f_frsize as u64;
                l.disk_used = l.disk_total.wrapping_sub(vs.f_bfree as u64 * vs.f_frsize as u64);
            }
        }
    }
    l.packages = rpm_packages(root);
    if let Ok(sh) = std::env::var("SHELL") {
        if !sh.is_empty() {
            l.shell = clean(&fit(sh.rsplit('/').next().unwrap_or(&sh), 32));
        }
    }
    l.session = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "Wayland"
    } else if std::env::var_os("DISPLAY").is_some() {
        "X11"
    } else {
        ""
    }
    .into();
    l
}

/// How to color stdout: --color=always|never|auto (auto: a terminal, no NO_COLOR, TERM not "dumb"; true color when
/// COLORTERM says so).
pub fn color_mode(opt: Option<&str>, is_tty: bool) -> Color {
    let ct = std::env::var("COLORTERM").unwrap_or_default();
    let full = if ct.contains("truecolor") || ct.contains("24bit") { Color::TrueColor } else { Color::C256 };
    match opt {
        Some("never") => return Color::Plain,
        Some("always") => return full,
        _ => {}
    }
    let term = std::env::var("TERM").ok();
    if !is_tty || std::env::var_os("NO_COLOR").is_some() || term.as_deref() == Some("dumb") {
        return Color::Plain;
    }
    full
}

// ---- the summary ----

const LINES_MAX: usize = 24;

fn bytes_text(v: u64) -> String {
    if v >= 1_000_000_000_000 {
        format!("{:.1} TB", v as f64 / 1e12)
    } else if v >= 10_000_000_000 {
        format!("{:.0} GB", v as f64 / 1e9)
    } else {
        format!("{:.1} GB", v as f64 / 1e9)
    }
}

fn add(ls: &mut Vec<(String, String)>, label: &str, value: &str) {
    if ls.len() < LINES_MAX && !value.is_empty() {
        ls.push((fit(label, 16), fit(value, 256)));
    }
}

/// The whole output: the mark beside the summary.
pub fn render(r: &Report, l: &Live, c: Color) -> String {
    let mut ls: Vec<(String, String)> = Vec::new();
    add(&mut ls, "OS", &fit(&format!("Axia DE {}{}{}{}", r.sw.lordos, if r.sw.os.is_empty() { "" } else { " (" }, r.sw.os, if r.sw.os.is_empty() { "" } else { ")" }), 256));
    let m = &r.machine;
    let mut v = format!("{}{}{}", m.vendor, if !m.vendor.is_empty() && !m.product.is_empty() { " " } else { "" }, m.product);
    if v.is_empty() {
        v = m.model.clone();
    }
    if !m.virt.is_empty() {
        v.push_str(&format!("{}virtual machine ({})", if v.is_empty() { "" } else { " · " }, m.virt));
    }
    add(&mut ls, "Computer", &v);
    add(&mut ls, "Kernel", &r.sw.kernel);
    if l.uptime_s >= 0 {
        let (d, h, mi) = (l.uptime_s / 86400, l.uptime_s % 86400 / 3600, l.uptime_s % 3600 / 60);
        let v = if d != 0 {
            format!("{d} d {h} h {mi} min")
        } else if h != 0 {
            format!("{h} h {mi} min")
        } else {
            format!("{mi} min")
        };
        add(&mut ls, "Uptime", &v);
    }
    if l.packages > 0 {
        add(&mut ls, "Packages", &format!("{} (rpm)", l.packages));
    }
    add(&mut ls, "Shell", &format!("{}{}{}", l.shell, if l.session.is_empty() { "" } else { " · " }, l.session));
    for x in &r.display {
        let a = if x.pref_w != 0 { format!("{} × {}", x.pref_w, x.pref_h) } else { x.mode.clone() };
        let mut v = format!("{}{}{}", if x.name.is_empty() { &x.connector } else { &x.name }, if a.is_empty() { "" } else { ", " }, a);
        if x.diag_tenths != 0 {
            v.push_str(&format!(", {}\"", (x.diag_tenths + 5) / 10));
        }
        add(&mut ls, "Display", &v);
    }
    let cpu = &r.cpu;
    let mut v = format!("{} ({})", if cpu.model.is_empty() { &cpu.arch } else { &cpu.model }, cpu.threads);
    if cpu.max_mhz != 0 {
        v.push_str(&format!(" @ {:.2} GHz", cpu.max_mhz as f64 / 1000.0));
    }
    add(&mut ls, "CPU", &v);
    for g in &r.gpu {
        if g.connectors == 0 && !g.render_node {
            continue;
        }
        let a = if !g.device.is_empty() { &g.device } else if !g.vendor.is_empty() { &g.vendor } else { &g.card };
        add(&mut ls, "GPU", &format!("{}{}{}{}", a, if g.driver.is_empty() { "" } else { " (" }, g.driver, if g.driver.is_empty() { "" } else { ")" }));
    }
    if r.mem.total_kb != 0 {
        add(&mut ls, "Memory", &format!("{} / {} ({} %)", bytes_text(l.mem_used_kb * 1024), bytes_text(r.mem.total_kb * 1024), l.mem_used_kb * 100 / r.mem.total_kb));
    }
    if l.disk_total != 0 {
        add(&mut ls, "Disk (/)", &format!("{} / {} ({} %)", bytes_text(l.disk_used), bytes_text(l.disk_total), l.disk_used * 100 / l.disk_total));
    }
    // what keeps apps in their place (docs/PRIVACY.md, docs/FIREWALL.md)
    let mut v = String::new();
    if r.sw.landlock_abi > 0 {
        v = format!("Landlock {}", r.sw.landlock_abi);
    }
    if r.sw.netperm {
        v.push_str(&format!("{}app firewall", if v.is_empty() { "" } else { " · " }));
    }
    if r.sw.cgroup2 {
        v.push_str(&format!("{}cgroup v2", if v.is_empty() { "" } else { " · " }));
    }
    add(&mut ls, "Isolation", &v);

    let n = ls.len();
    let mut b = String::new();
    let violet = pal(b'V').unwrap();
    // the title: user@host (or the product) and a rule under it
    let title = if !l.user.is_empty() && !l.host.is_empty() { format!("{}@{}", l.user, l.host) } else { "Axia DE".to_string() };
    // title, rule, the lines, and with color a blank line and the palette; at least the mark's height
    let mut rows = n + if c == Color::Plain { 2 } else { 4 };
    rows = rows.max(MARK_H.div_ceil(2)).max(PLAIN.len());
    for y in 0..rows {
        b.push(' ');
        mark_row(&mut b, y, c);
        b.push_str("   ");
        if y == 0 {
            if c != Color::Plain {
                b.push_str("\x1b[1m");
                fg(&mut b, violet, c);
            }
            b.push_str(&title);
            if c != Color::Plain {
                b.push_str("\x1b[0m");
            }
        } else if y == 1 {
            for _ in 0..title.len() {
                b.push_str(if c == Color::Plain { "-" } else { "─" });
            }
        } else if y - 2 < n {
            let (label, value) = &ls[y - 2];
            if c != Color::Plain {
                b.push_str("\x1b[1m");
                fg(&mut b, violet, c);
            }
            b.push_str(label);
            if c != Color::Plain {
                b.push_str("\x1b[0m");
            }
            b.push_str(&" ".repeat(10usize.saturating_sub(label.len())));
            b.push_str(value);
        } else if y - 2 == n + 1 && c != Color::Plain {
            for k in b"PVB" {
                bg(&mut b, pal(*k).unwrap(), c);
                b.push_str("   ");
            }
            b.push_str("\x1b[0m");
        }
        b.push('\n');
    }
    b
}

/// Reads a text file under a root (the tests use it).
pub fn read_under(root: &str, path: &str) -> Option<String> {
    read_text(&format!("{root}{path}"), 4096)
}
