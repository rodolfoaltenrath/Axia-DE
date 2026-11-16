//! OSC and DCS strings: titles, palette, hyperlinks, shell integration, inline images and sixel.
use super::*;

extern "C" {
    fn atof(s: *const c_char) -> f64;
}

fn hexval(c: i32) -> i32 {
    match c {
        0x30..=0x39 => c - 0x30,
        0x61..=0x66 => c - 0x61 + 10,
        0x41..=0x46 => c - 0x41 + 10,
        _ => -1,
    }
}

/// "rgb:rr/gg/bb" (1 to 4 digits per channel) or "#rrggbb".
unsafe fn parse_color(mut s: *const c_char, out: &mut u32) -> bool {
    if *s as u8 == b'#' && libc::strlen(s) == 7 {
        let mut v: u32 = 0;
        for i in 1..7 {
            let h = hexval(*s.add(i) as u8 as i32);
            if h < 0 {
                return false;
            }
            v = v << 4 | h as u32;
        }
        *out = 0xff00_0000 | v;
        return true;
    }
    if libc::strncmp(s, c"rgb:".as_ptr(), 4) != 0 {
        return false;
    }
    s = s.add(4);
    let mut ch = [0u32; 3];
    for k in 0..3 {
        let mut v: u32 = 0;
        let mut digits = 0;
        while hexval(*s as u8 as i32) >= 0 {
            v = v << 4 | hexval(*s as u8 as i32) as u32;
            s = s.add(1);
            digits += 1;
        }
        if digits == 0 || digits > 4 {
            return false;
        }
        ch[k] = match digits {
            1 => v * 17,
            2 => v,
            3 => v >> 4,
            _ => v >> 8,
        };
        if k < 2 {
            if *s as u8 != b'/' {
                return false;
            }
            s = s.add(1);
        }
    }
    *out = 0xff00_0000 | ch[0] << 16 | ch[1] << 8 | ch[2];
    true
}

unsafe fn reply_color(t: *mut Term, prefix: &str, c: u32) {
    let (r, g, b) = (c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff);
    reply!(t, "\x1b]{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\", prefix, r, r, g, g, b, b);
}

fn b64_val(c: u8) -> i32 {
    match c {
        b'A'..=b'Z' => (c - b'A') as i32,
        b'a'..=b'z' => (c - b'a') as i32 + 26,
        b'0'..=b'9' => (c - b'0') as i32 + 52,
        b'+' => 62,
        b'/' => 63,
        _ => -1,
    }
}

/// Decodes base64 (ignores spaces and '='); `out` needs n*3/4+3 bytes.
#[no_mangle]
pub unsafe extern "C" fn base64_decode(inp: *const c_char, n: usize, out: *mut u8) -> usize {
    let mut acc: u32 = 0;
    let mut bits = 0;
    let mut o = 0usize;
    for i in 0..n {
        let v = b64_val(*inp.add(i) as u8);
        if v < 0 {
            continue;
        }
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            *out.add(o) = (acc >> bits) as u8;
            o += 1;
        }
    }
    o
}

unsafe fn b64_text(inp: *const c_char, max: usize) -> *mut c_char {
    let n = libc::strlen(inp);
    let out = libc::malloc(n * 3 / 4 + 4) as *mut c_char;
    if out.is_null() {
        return null_mut();
    }
    let mut m = base64_decode(inp, n, out.cast());
    if m > max {
        m = max;
    }
    *out.add(m) = 0;
    out
}

unsafe fn copy_str(dst: *mut c_char, size: usize, src: *const c_char) {
    let mut i = 0;
    while *src.add(i) != 0 && i + 1 < size {
        let c = *src.add(i) as u8;
        *dst.add(i) = if c < 0x20 { b' ' as c_char } else { *src.add(i) };
        i += 1;
    }
    *dst.add(i) = 0;
}

unsafe fn url_decode(dst: *mut c_char, size: usize, src: *const c_char) {
    let mut o = 0usize;
    let mut p = src;
    while *p != 0 && o + 1 < size {
        if *p as u8 == b'%' && hexval(*p.add(1) as u8 as i32) >= 0 && hexval(*p.add(2) as u8 as i32) >= 0 {
            *dst.add(o) = (hexval(*p.add(1) as u8 as i32) << 4 | hexval(*p.add(2) as u8 as i32)) as c_char;
            o += 1;
            p = p.add(2);
        } else {
            *dst.add(o) = *p;
            o += 1;
        }
        p = p.add(1);
    }
    *dst.add(o) = 0;
}

unsafe fn add_link(t: *mut Term, uri: *const c_char) -> c_int {
    for i in 0..(*t).nlinks {
        if libc::strcmp(*(*t).links.add(i as usize), uri) == 0 {
            return i + 1;
        }
    }
    if (*t).links.is_null() {
        (*t).links = libc::calloc(MAX_LINKS, std::mem::size_of::<*mut c_char>()) as *mut *mut c_char;
    }
    if (*t).links.is_null() {
        return 0;
    }
    if (*t).nlinks as usize >= MAX_LINKS {
        for i in 0..(*t).nlinks {
            libc::free((*(*t).links.add(i as usize)).cast());
        }
        (*t).nlinks = 0;
    }
    *(*t).links.add((*t).nlinks as usize) = libc::strdup(uri);
    if (*(*t).links.add((*t).nlinks as usize)).is_null() {
        0
    } else {
        (*t).nlinks += 1;
        (*t).nlinks
    }
}

/// OSC 1337 image dimension: "N" cells, "Npx", "N%" or "auto".
unsafe fn image_dim(v: *const c_char, cell: c_int, avail_cells: c_int, px: &mut c_int) -> c_int {
    *px = 0;
    if v.is_null() || *v == 0 || c_eq(v, b"auto") {
        return -1;
    }
    let d = atof(v);
    let n = libc::strlen(v);
    if n > 2 && c_eq(v.add(n - 2), b"px") {
        *px = d as c_int;
        return ((d + cell as f64 - 1.0) / cell as f64) as c_int;
    }
    if *v.add(n - 1) as u8 == b'%' {
        return (avail_cells as f64 * d / 100.0) as c_int;
    }
    d as c_int
}

unsafe fn place_image(t: *mut Term, args: *const c_char, data: *const c_char) {
    if (*t).buf != &mut (*t).main as *mut Buffer {
        return;
    }
    let mut width = [0 as c_char; 32];
    let mut height = [0 as c_char; 32];
    let mut inline_ = 0;
    let mut keep = 1;
    let mut tmp = [0 as c_char; 1024];
    libc::snprintf(tmp.as_mut_ptr(), tmp.len(), c"%s".as_ptr(), args);
    let mut tok = libc::strtok(tmp.as_mut_ptr(), c";".as_ptr());
    while !tok.is_null() {
        let eq = libc::strchr(tok, b'=' as c_int);
        if !eq.is_null() {
            *eq = 0;
            if c_eq(tok, b"width") {
                libc::snprintf(width.as_mut_ptr(), width.len(), c"%s".as_ptr(), eq.add(1));
            } else if c_eq(tok, b"height") {
                libc::snprintf(height.as_mut_ptr(), height.len(), c"%s".as_ptr(), eq.add(1));
            } else if c_eq(tok, b"inline") {
                inline_ = libc::atoi(eq.add(1));
            } else if c_eq(tok, b"preserveAspectRatio") {
                keep = libc::atoi(eq.add(1));
            }
        }
        tok = libc::strtok(null_mut(), c";".as_ptr());
    }
    if inline_ == 0 {
        return;
    }
    let n = libc::strlen(data);
    let raw = libc::malloc(n * 3 / 4 + 4) as *mut u8;
    if raw.is_null() {
        return;
    }
    let len = base64_decode(data, n, raw);
    let (mut w, mut h): (c_int, c_int) = (0, 0);
    let rgba = crate::axia_image_decode(raw, len, &mut w, &mut h, 4, 8192, 8192u64 * 8192);
    libc::free(raw.cast());
    if rgba.is_null() {
        return;
    }
    if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
        libc::free(rgba.cast());
        return;
    }
    let px = libc::malloc(w as usize * h as usize * 4) as *mut u32;
    if px.is_null() {
        libc::free(rgba.cast());
        return;
    }
    for i in 0..(w as usize * h as usize) {
        let (r, g, b, a) = (*rgba.add(i * 4) as u32, *rgba.add(i * 4 + 1) as u32, *rgba.add(i * 4 + 2) as u32, *rgba.add(i * 4 + 3) as u32);
        *px.add(i) = a << 24 | (r * a / 255) << 16 | (g * a / 255) << 8 | (b * a / 255);
    }
    libc::free(rgba.cast());

    let cw: f32 = if (*t).cell_w > 0.0 { (*t).cell_w } else { 8.0 };
    let ch: f32 = if (*t).cell_h > 0.0 { (*t).cell_h } else { 16.0 };
    let (mut pxw, mut pxh) = (0, 0);
    let mut wc = image_dim(width.as_ptr(), cw as c_int, (*t).cols - (*t).cx, &mut pxw);
    let mut hc = image_dim(height.as_ptr(), ch as c_int, (*t).rows, &mut pxh);
    if wc < 0 && hc < 0 {
        wc = ((w as f32 + cw - 1.0) / cw) as c_int;
        hc = ((h as f32 + ch - 1.0) / ch) as c_int;
    } else if wc < 0 {
        wc = if keep != 0 { ((hc as f32 * ch * w as f32 / h as f32 / cw) as f64 + 0.999) as c_int } else { ((w as f32 + cw - 1.0) / cw) as c_int };
    } else if hc < 0 {
        hc = if keep != 0 { ((wc as f32 * cw * h as f32 / w as f32 / ch) as f64 + 0.999) as c_int } else { ((h as f32 + ch - 1.0) / ch) as c_int };
    }
    if wc > (*t).cols - (*t).cx {
        let nw = (*t).cols - (*t).cx;
        if keep != 0 && wc > 0 {
            hc = hc * nw / wc;
        }
        wc = nw;
    }
    if wc < 1 {
        wc = 1;
    }
    if hc < 1 {
        hc = 1;
    }
    if hc > 4 * (*t).rows {
        hc = 4 * (*t).rows;
    }
    add_image(t, px, w, h, wc, hc);
}

unsafe fn add_image(t: *mut Term, px: *mut u32, w: c_int, h: c_int, wc: c_int, hc: c_int) {
    if (*t).nimages as usize >= MAX_IMAGES {
        image_free((*t).images);
        (*t).nimages -= 1;
        std::ptr::copy((*t).images.add(1), (*t).images, (*t).nimages as usize);
    }
    if (*t).images.is_null() {
        (*t).images = libc::calloc(MAX_IMAGES, std::mem::size_of::<TermImage>()) as *mut TermImage;
    }
    if (*t).images.is_null() {
        libc::free(px.cast());
        return;
    }
    let col = (*t).cx;
    for _ in 0..hc - 1 {
        index_down(t);
    }
    let line = vt_cursor_abs(t) - (hc as i64 - 1);
    *(*t).images.add((*t).nimages as usize) = TermImage { line, col, wc, hc, pw: w, ph: h, px, scaled: null_mut(), sw: 0, sh: 0 };
    (*t).nimages += 1;
    (*t).cx = if col + wc < (*t).cols { col + wc } else { (*t).cols - 1 };
    (*t).wrap_pending = 0;
    dirty_all(t);
}

// ---- Sixel ----

fn hls(h: c_int, l: c_int, sat: c_int) -> u32 {
    let hh = ((h + 240) % 360) as f32 / 360.0;
    let ll = l as f32 / 100.0;
    let ss = sat as f32 / 100.0;
    let q = if ll < 0.5 { ll * (1.0 + ss) } else { ll + ss - ll * ss };
    let p = 2.0 * ll - q;
    let mut c = [0f32; 3];
    for i in 0..3 {
        let mut tt = hh + (1 - i as c_int) as f32 / 3.0;
        if tt < 0.0 {
            tt += 1.0;
        }
        if tt > 1.0 {
            tt -= 1.0;
        }
        c[i] = if tt < 1.0 / 6.0 {
            p + (q - p) * 6.0 * tt
        } else if tt < 0.5 {
            q
        } else if tt < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - tt) * 6.0
        } else {
            p
        };
    }
    0xff00_0000 | ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32
}

unsafe fn sixel_num(p: &mut *const u8) -> c_int {
    let mut v: c_int = 0;
    while (**p).is_ascii_digit() {
        if v < 100000 {
            v = v * 10 + (**p - b'0') as c_int;
        }
        *p = p.add(1);
    }
    v
}

unsafe fn sixel(t: *mut Term, s: *const c_char) {
    if (*t).buf != &mut (*t).main as *mut Buffer {
        return;
    }
    let mut p = s as *const u8;
    while *p != 0 && *p != b'q' {
        p = p.add(1);
    }
    if *p == 0 {
        return;
    }
    p = p.add(1);
    const VT340: [[u8; 3]; 16] = [
        [0, 0, 0],
        [20, 20, 80],
        [80, 13, 13],
        [20, 80, 20],
        [80, 20, 80],
        [20, 80, 80],
        [80, 80, 20],
        [53, 53, 53],
        [26, 26, 26],
        [33, 33, 60],
        [60, 26, 26],
        [33, 60, 33],
        [60, 33, 60],
        [33, 60, 60],
        [60, 60, 33],
        [80, 80, 80],
    ];
    let mut pal = [0u32; 256];
    for i in 0..256 {
        let c = VT340[i % 16];
        pal[i] = 0xff00_0000 | (c[0] as u32 * 255 / 100) << 16 | (c[1] as u32 * 255 / 100) << 8 | (c[2] as u32 * 255 / 100);
    }
    let (mut cw, mut ch, mut w, mut h, mut x, mut y, mut color): (c_int, c_int, c_int, c_int, c_int, c_int, c_int) = (0, 0, 0, 0, 0, 0, 0);
    let mut px: *mut u32 = null_mut();
    while *p != 0 {
        let mut c = *p;
        if c == b'"' {
            p = p.add(1);
            sixel_num(&mut p);
            if *p == b';' {
                p = p.add(1);
            }
            sixel_num(&mut p);
            if *p == b';' {
                p = p.add(1);
            }
            let rw = sixel_num(&mut p);
            if *p == b';' {
                p = p.add(1);
            }
            let rh = sixel_num(&mut p);
            if rw > 0 && rh > 0 && rw <= 4096 && rh <= 4096 && px.is_null() {
                cw = rw;
                ch = (rh + 5) / 6 * 6;
                px = libc::calloc(cw as usize * ch as usize, 4) as *mut u32;
                if px.is_null() {
                    return;
                }
            }
        } else if c == b'#' {
            p = p.add(1);
            let n = sixel_num(&mut p) & 255;
            if *p == b';' {
                p = p.add(1);
                let mode = sixel_num(&mut p);
                if *p == b';' {
                    p = p.add(1);
                }
                let a = sixel_num(&mut p);
                if *p == b';' {
                    p = p.add(1);
                }
                let b = sixel_num(&mut p);
                if *p == b';' {
                    p = p.add(1);
                }
                let d = sixel_num(&mut p);
                pal[n as usize] = if mode == 1 {
                    hls(a, b, d)
                } else {
                    0xff00_0000 | ((a * 255 / 100) as u32) << 16 | ((b * 255 / 100) as u32) << 8 | (d * 255 / 100) as u32
                };
            }
            color = n;
        } else if c == b'$' {
            x = 0;
            p = p.add(1);
        } else if c == b'-' {
            x = 0;
            y += 6;
            p = p.add(1);
        } else if c == b'!' || (b'?'..=b'~').contains(&c) {
            let mut rep: c_int = 1;
            if c == b'!' {
                p = p.add(1);
                rep = sixel_num(&mut p);
                if rep < 1 {
                    rep = 1;
                }
                c = *p;
                if !(b'?'..=b'~').contains(&c) {
                    continue;
                }
            }
            let bits = (c - b'?') as c_int;
            p = p.add(1);
            if x + rep > cw || y + 6 > ch {
                let (mut nw, mut nh) = (cw, ch);
                while x + rep > nw {
                    nw = if nw != 0 { nw * 2 } else { 256 };
                }
                while y + 6 > nh {
                    nh = if nh != 0 { nh * 2 } else { 96 };
                }
                if nw > 4096 {
                    nw = 4096;
                }
                if nh > 4096 {
                    nh = 4096;
                }
                if x + rep > nw || y + 6 > nh {
                    rep = nw - x;
                    if rep <= 0 || y + 6 > nh {
                        continue;
                    }
                }
                let n2 = libc::calloc(nw as usize * nh as usize, 4) as *mut u32;
                if n2.is_null() {
                    break;
                }
                for r in 0..ch {
                    if !px.is_null() {
                        std::ptr::copy_nonoverlapping(px.add(r as usize * cw as usize), n2.add(r as usize * nw as usize), cw as usize);
                    }
                }
                libc::free(px.cast());
                px = n2;
                cw = nw;
                ch = nh;
            }
            for _ in 0..rep {
                for b in 0..6 {
                    if bits & (1 << b) != 0 {
                        *px.add((y + b) as usize * cw as usize + x as usize) = pal[color as usize];
                    }
                }
                x += 1;
            }
            if x > w {
                w = x;
            }
            if y + 6 > h {
                h = y + 6;
            }
        } else {
            p = p.add(1);
        }
    }
    if px.is_null() || w == 0 || h == 0 {
        libc::free(px.cast());
        return;
    }
    let img = libc::malloc(w as usize * h as usize * 4) as *mut u32;
    if img.is_null() {
        libc::free(px.cast());
        return;
    }
    for r in 0..h {
        std::ptr::copy_nonoverlapping(px.add(r as usize * cw as usize), img.add(r as usize * w as usize), w as usize);
    }
    libc::free(px.cast());
    let fw: f32 = if (*t).cell_w > 0.0 { (*t).cell_w } else { 8.0 };
    let fh: f32 = if (*t).cell_h > 0.0 { (*t).cell_h } else { 16.0 };
    let mut wc = ((w as f32 + fw - 1.0) / fw) as c_int;
    let hc = ((h as f32 + fh - 1.0) / fh) as c_int;
    if wc > (*t).cols - (*t).cx {
        wc = (*t).cols - (*t).cx;
    }
    add_image(t, img, w, h, if wc < 1 { 1 } else { wc }, if hc < 1 { 1 } else { hc });
}

unsafe fn osc_1337(t: *mut Term, s: *mut c_char) {
    let eq = libc::strchr(s, b'=' as c_int);
    let val: *mut c_char = if eq.is_null() { c"".as_ptr() as *mut c_char } else { eq.add(1) };
    if !eq.is_null() {
        *eq = 0;
    }
    if c_eq(s, b"SetMark") {
        vt_set_mark(t);
    } else if c_eq(s, b"CurrentDir") {
        copy_str((*t).cwd.as_mut_ptr(), (*t).cwd.len(), val);
        (*t).events |= EV_CWD;
    } else if c_eq(s, b"RemoteHost") {
        if libc::strcmp((*t).host.as_ptr(), val) != 0 {
            copy_str((*t).host.as_mut_ptr(), (*t).host.len(), val);
            (*t).events |= EV_HOST;
        }
    } else if c_eq(s, b"SetBadgeFormat") {
        let b = b64_text(val, (*t).badge.len() - 1);
        if !b.is_null() {
            copy_str((*t).badge.as_mut_ptr(), (*t).badge.len(), b);
            libc::free(b.cast());
            (*t).events |= EV_BADGE;
        }
    } else if c_eq(s, b"File") {
        let colon = libc::strchr(val, b':' as c_int);
        if !colon.is_null() {
            *colon = 0;
            place_image(t, val, colon.add(1));
        }
    } else if c_eq(s, b"ClearScrollback") {
        vt_clear_scrollback(t);
    } else if c_eq(s, b"CursorShape") {
        let v = libc::atoi(val);
        (*t).cursor_shape = if v == 1 {
            6
        } else if v == 2 {
            4
        } else {
            2
        };
        vt_mark_dirty(t, (*t).cy);
    } else if c_eq(s, b"SetProfile") {
        copy_str((*t).profile_req.as_mut_ptr(), (*t).profile_req.len(), val);
        (*t).events |= EV_PROFILE;
    } else if c_eq(s, b"RequestAttention") {
        (*t).events |= EV_ATTENTION;
    } else if c_eq(s, b"ReportCellSize") {
        reply!(t, "\x1b]1337;ReportCellSize={:.1};{:.1}\x07", (*t).cell_h as f64, (*t).cell_w as f64);
    } else if c_eq(s, b"Copy") {
        let d = libc::strchr(val, b':' as c_int);
        let b = b64_text(if d.is_null() { val } else { d.add(1) }, 1 << 20);
        if !b.is_null() {
            clipboard_set(b);
            libc::free(b.cast());
            (*t).events |= EV_COPY;
        }
    } else if c_eq(s, b"HighlightCursorLine") {
        (*(*t).prof).cursor_guide = (c_eq(val, b"yes") || c_eq(val, b"true")) as c_int;
        dirty_all(t);
    }
}

unsafe fn clipboard_set(b: *const c_char) {
    if let Some(f) = CLIPBOARD {
        f(b, libc::strlen(b));
    }
}

pub(crate) unsafe fn osc(t: *mut Term) {
    let mut s = (*t).str_;
    let mut num: c_int = 0;
    while (*s as u8).is_ascii_digit() {
        num = num.wrapping_mul(10).wrapping_add((*s as u8 - b'0') as c_int);
        s = s.add(1);
    }
    if *s as u8 == b';' {
        s = s.add(1);
    } else if *s != 0 {
        return;
    }
    match num {
        0 => {
            copy_str((*t).title.as_mut_ptr(), (*t).title.len(), s);
            copy_str((*t).icon_title.as_mut_ptr(), (*t).icon_title.len(), s);
            (*t).events |= EV_TITLE;
        }
        1 => copy_str((*t).icon_title.as_mut_ptr(), (*t).icon_title.len(), s),
        2 => {
            copy_str((*t).title.as_mut_ptr(), (*t).title.len(), s);
            (*t).events |= EV_TITLE;
        }
        4 => {
            let mut p = s;
            while !p.is_null() && *p != 0 {
                let semi = libc::strchr(p, b';' as c_int);
                if semi.is_null() {
                    break;
                }
                let idx = libc::atoi(p);
                let spec = semi.add(1);
                let next = libc::strchr(spec, b';' as c_int);
                if !next.is_null() {
                    *next = 0;
                }
                if (0..256).contains(&idx) {
                    if c_eq(spec, b"?") {
                        reply_color(t, &format!("4;{}", idx), (*t).palette[idx as usize]);
                    } else {
                        let mut c = 0u32;
                        if parse_color(spec, &mut c) {
                            (*t).palette[idx as usize] = c;
                            dirty_all(t);
                        }
                    }
                }
                p = if next.is_null() { null_mut() } else { next.add(1) };
            }
        }
        7 => {
            if c_starts(s, b"file://") {
                let path = libc::strchr(s.add(7), b'/' as c_int);
                if !path.is_null() {
                    url_decode((*t).cwd.as_mut_ptr(), (*t).cwd.len(), path);
                    (*t).events |= EV_CWD;
                }
                let hl = if path.is_null() { 0 } else { path as usize - s.add(7) as usize };
                if hl != 0 && hl < 256 && libc::strchr((*t).host.as_ptr(), b'@' as c_int).is_null() {
                    let mut host = [0 as c_char; 256];
                    std::ptr::copy_nonoverlapping(s.add(7), host.as_mut_ptr(), hl);
                    host[hl] = 0;
                    if libc::strcmp((*t).host.as_ptr(), host.as_ptr()) != 0 {
                        copy_str((*t).host.as_mut_ptr(), (*t).host.len(), host.as_ptr());
                        (*t).events |= EV_HOST;
                    }
                }
            }
        }
        8 => {
            let semi = libc::strchr(s, b';' as c_int);
            if semi.is_null() {
                return;
            }
            (*t).pen.link = if *semi.add(1) != 0 { add_link(t, semi.add(1)) as u16 } else { 0 };
        }
        9 => {
            if !((*s as u8).is_ascii_digit() && *s.add(1) as u8 == b';') {
                copy_str((*t).notify.as_mut_ptr(), (*t).notify.len(), s);
                (*t).events |= EV_NOTIFY;
            }
        }
        10 | 11 | 12 => {
            let cur = if num == 10 {
                vt_resolve(t, 0, 1)
            } else if num == 11 {
                vt_resolve(t, 0, 0)
            } else if (*t).cursor_set != 0 {
                (*t).cursor_color
            } else {
                (*(*t).prof).cursor | 0xff00_0000
            };
            if c_eq(s, b"?") {
                reply_color(t, &format!("{}", num), cur);
            } else {
                let mut c = 0u32;
                if parse_color(s, &mut c) {
                    if num == 10 {
                        (*t).fg = c;
                        (*t).fg_set = 1;
                    } else if num == 11 {
                        (*t).bg = c;
                        (*t).bg_set = 1;
                    } else {
                        (*t).cursor_color = c;
                        (*t).cursor_set = 1;
                    }
                    dirty_all(t);
                }
            }
        }
        52 => {
            let semi = libc::strchr(s, b';' as c_int);
            if semi.is_null() || c_eq(semi.add(1), b"?") {
                return;
            }
            let b = b64_text(semi.add(1), 1 << 20);
            if !b.is_null() {
                clipboard_set(b);
                libc::free(b.cast());
                (*t).events |= EV_COPY;
            }
        }
        104 => {
            if *s == 0 {
                vt_apply_palette(t);
            } else {
                let mut p = s;
                while !p.is_null() {
                    let i = libc::atoi(p);
                    if (0..16).contains(&i) {
                        (*t).palette[i as usize] = (*(*t).prof).palette[i as usize] | 0xff00_0000;
                    }
                    let semi = libc::strchr(p, b';' as c_int);
                    p = if semi.is_null() { null_mut() } else { semi.add(1) };
                }
            }
            dirty_all(t);
        }
        110 => {
            (*t).fg_set = 0;
            dirty_all(t);
        }
        111 => {
            (*t).bg_set = 0;
            dirty_all(t);
        }
        112 => (*t).cursor_set = 0,
        133 => osc_133(t, s),
        1337 => osc_1337(t, s),
        _ => {}
    }
}

unsafe fn osc_133(t: *mut Term, s: *mut c_char) {
    let l = lrow(t, (*t).cy);
    let k = *s as u8;
    if k == b'A' {
        (*l).flags |= LINE_PROMPT;
        (*l).flags &= !(LINE_OK | LINE_FAIL);
        vt_mark_dirty(t, (*t).cy);
    } else if k == b'B' {
        (*t).cmd_line = vt_cursor_abs(t);
        (*t).cmd_col = (*t).cx;
    } else if k == b'C' {
        let end = vt_cursor_abs(t) - if (*t).cx == 0 { 1 } else { 0 };
        if (*t).cmd_line >= vt_first_abs(t) && (*t).cmd_line <= end && end - (*t).cmd_line < 50 {
            let mut txt = Str::default();
            let mut a = (*t).cmd_line;
            while a <= end {
                let cl = vt_abs_line(t, a);
                line_text(&mut txt, cl, if a == (*t).cmd_line { (*t).cmd_col } else { 0 }, (*t).cols - 1, !cl.is_null() && (*cl).flags & LINE_WRAPPED != 0);
                a += 1;
            }
            if !txt.s.is_null() {
                let mut p = txt.s;
                while *p as u8 == b' ' {
                    p = p.add(1);
                }
                libc::snprintf((*t).run_cmd.as_mut_ptr(), (*t).run_cmd.len(), c"%s".as_ptr(), p);
                if (*t).run_cmd[0] != 0 {
                    (*t).events |= EV_RUN;
                }
                libc::free(txt.s.cast());
            }
        }
        (*t).cmd_line = -1;
        (*t).cmd_prompt = -1;
        let mut a = vt_cursor_abs(t);
        while a >= vt_first_abs(t) {
            let p = vt_abs_line(t, a);
            if !p.is_null() && (*p).flags & LINE_PROMPT != 0 {
                (*t).cmd_prompt = a;
                break;
            }
            a -= 1;
        }
        (*l).flags |= LINE_OUTPUT;
    } else if k == b'D' {
        let p = if (*t).cmd_prompt >= 0 { vt_abs_line(t, (*t).cmd_prompt) } else { null_mut() };
        if !p.is_null() {
            let code = if *s.add(1) as u8 == b';' { libc::atoi(s.add(2)) } else { 0 };
            (*p).flags |= if code != 0 { LINE_FAIL } else { LINE_OK };
            libc::snprintf((*t).command.as_mut_ptr(), (*t).command.len(), c"%d".as_ptr(), code);
            (*t).events |= EV_COMMAND;
            dirty_all(t);
        }
        (*t).cmd_prompt = -1;
    }
}

pub(crate) unsafe fn dcs(t: *mut Term) {
    let mut p = (*t).str_;
    while (*p as u8).is_ascii_digit() || *p as u8 == b';' {
        p = p.add(1);
    }
    if *p as u8 == b'q' {
        sixel(t, (*t).str_);
        return;
    }
    if c_starts((*t).str_, b"$q") {
        reply!(t, "\x1bP0$r\x1b\\");
    } else if c_starts((*t).str_, b"+q") {
        reply!(t, "\x1bP0+r\x1b\\");
    }
}
