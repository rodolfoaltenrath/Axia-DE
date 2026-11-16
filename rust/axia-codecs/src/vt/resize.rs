//! Resizing with text reflow on the main screen.
use super::*;

struct LineVec {
    v: *mut Line,
    n: c_int,
    cap: c_int,
}

unsafe fn vec_push(v: &mut LineVec) -> *mut Line {
    if v.n == v.cap {
        let cap = if v.cap != 0 { v.cap * 2 } else { 256 };
        let nv = libc::realloc(v.v.cast(), cap as usize * std::mem::size_of::<Line>()) as *mut Line;
        if nv.is_null() {
            return null_mut();
        }
        v.v = nv;
        v.cap = cap;
    }
    let l = v.v.add(v.n as usize);
    v.n += 1;
    std::ptr::write_bytes(l, 0, 1);
    l
}

unsafe fn reflow_main(t: *mut Term, ncols: c_int, nrows: c_int, ccx: *mut c_int, ccy: *mut c_int) {
    let b = &mut (*t).main as *mut Buffer;
    let ocols = (*t).cols;
    let mut count = (*b).count;
    let cur = count - (*t).rows + *ccy;
    while count - 1 > cur && (*bl(b, count - 1)).len == 0 && (*bl(b, count - 1)).flags == 0 {
        count -= 1;
    }

    let mut out = LineVec { v: null_mut(), n: 0, cap: 0 };
    let map = libc::malloc(count as usize * 4) as *mut c_int;
    let mut cells_cap = ocols as usize * 64;
    let mut cells = libc::malloc(cells_cap * std::mem::size_of::<Cell>()) as *mut Cell;
    let (mut new_cur, mut new_col) = (0, 0);
    if map.is_null() || cells.is_null() {
        libc::free(map.cast());
        libc::free(cells.cast());
        return;
    }

    let mut i: c_int = 0;
    while i < count {
        let mut j = i;
        while j < count - 1 && (*bl(b, j)).flags & LINE_WRAPPED != 0 {
            j += 1;
        }
        let mut total: usize = 0;
        for k in i..=j {
            total += if k < j { ocols as usize } else { (*bl(b, k)).len as usize };
        }
        if total + 1 > cells_cap {
            let nc = libc::realloc(cells.cast(), (total + 1) * std::mem::size_of::<Cell>()) as *mut Cell;
            if nc.is_null() {
                break;
            }
            cells = nc;
            cells_cap = total + 1;
        }
        let mut pos = 0usize;
        for k in i..=j {
            let l = bl(b, k);
            let mut w = if k < j { ocols } else { (*l).len as c_int };
            if k < j && (((*l).len as c_int) < ocols || ((*(*l).c.add(ocols as usize - 1)).cp == 0 && (*(*l).c.add(ocols as usize - 1)).attr == 0)) {
                let nx = bl(b, k + 1);
                if (*nx).len != 0 && (*(*nx).c).attr & A_WIDE != 0 {
                    w = ocols - 1;
                }
            }
            for x in 0..w {
                *cells.add(pos) = if x < (*l).len as c_int { *(*l).c.add(x as usize) } else { Cell::default() };
                pos += 1;
            }
        }
        // A wide character that did not fit at the end of a row leaves a padding cell that is counted in
        // `total` but was never written: the C code read stale malloc contents there (a phantom character
        // after a reflow). It is blank.
        while pos < total {
            *cells.add(pos) = Cell::default();
            pos += 1;
        }
        while total > 0 && cell_blank(*cells.add(total - 1)) {
            total -= 1;
        }
        let cur_off: i64 = if cur >= i && cur <= j { (cur - i) as i64 * ocols as i64 + *ccx as i64 } else { -1 };
        let mut cur_off = cur_off;

        let first_flags = (*bl(b, i)).flags & !LINE_WRAPPED;
        let time0 = (*bl(b, i)).time;
        let first_new = out.n;
        let mut start: usize = 0;
        loop {
            let mut len = if total - start > ncols as usize { ncols as usize } else { total - start };
            if len == ncols as usize && len > 1 && (*cells.add(start + len - 1)).attr & A_WIDE != 0 {
                len -= 1;
            }
            let mut nl = vec_push(&mut out);
            if nl.is_null() {
                break;
            }
            (*nl).flags = if start == 0 { first_flags } else { 0 };
            (*nl).time = time0;
            if len != 0 {
                line_reserve(nl, len as c_int);
                std::ptr::copy_nonoverlapping(cells.add(start), (*nl).c, len);
                (*nl).len = len as u16;
            }
            if cur_off >= 0 && cur_off as usize >= start && ((cur_off as usize) < start + len || start + len >= total) {
                let mut col = cur_off - start as i64;
                while col >= ncols as i64 {
                    (*nl).flags |= LINE_WRAPPED;
                    nl = vec_push(&mut out);
                    if nl.is_null() {
                        break;
                    }
                    col -= ncols as i64;
                }
                new_cur = out.n - 1;
                new_col = col as c_int;
                cur_off = -1;
            }
            start += len;
            if start < total {
                (*nl).flags |= LINE_WRAPPED;
            }
            if start >= total {
                break;
            }
        }
        for k in i..=j {
            let off = (k - i) as i64 * ocols as i64 / ncols as i64;
            let lim = (out.n - first_new) as i64;
            *map.add(k as usize) = first_new + if off < lim { off } else { lim - 1 } as c_int;
        }
        i = j + 1;
    }
    libc::free(cells.cast());

    while out.n - new_cur > nrows {
        out.n -= 1;
        libc::free((*out.v.add(out.n as usize)).c.cast());
    }
    while out.n < nrows {
        vec_push(&mut out);
    }

    let cap = (*b).cap - (*t).rows + nrows;
    let drop = if out.n > cap { out.n - cap } else { 0 };
    for i in 0..drop {
        libc::free((*out.v.add(i as usize)).c.cast());
    }
    for i in 0..(*b).cap {
        libc::free((*(*b).ring.add(i as usize)).c.cast());
    }
    libc::free((*b).ring.cast());
    (*b).ring = libc::calloc(cap as usize, std::mem::size_of::<Line>()) as *mut Line;
    (*b).cap = cap;
    (*b).head = 0;
    (*b).count = out.n - drop;
    let old_dropped = (*b).dropped;
    (*b).dropped += drop as i64;
    for i in 0..(*b).count {
        std::ptr::copy_nonoverlapping(out.v.add((drop + i) as usize), (*b).ring.add(i as usize), 1);
    }
    libc::free(out.v.cast());

    for k in 0..(*t).nimages {
        let im = (*t).images.add(k as usize);
        let oi = (*im).line - old_dropped;
        (*im).line = if oi >= 0 && oi < count as i64 { (*b).dropped + *map.add(oi as usize) as i64 - drop as i64 } else { -1000000 };
        libc::free((*im).scaled.cast());
        (*im).scaled = null_mut();
    }
    libc::free(map.cast());
    *ccy = new_cur - drop - ((*b).count - nrows);
    if *ccy < 0 {
        *ccy = 0;
    }
    *ccx = if new_col < ncols { new_col } else { ncols - 1 };
}

unsafe fn resize_alt(t: *mut Term, _ncols: c_int, nrows: c_int) {
    let b = &mut (*t).alt as *mut Buffer;
    let ring = libc::calloc(nrows as usize, std::mem::size_of::<Line>()) as *mut Line;
    if ring.is_null() {
        return;
    }
    let keep = if (*t).rows < nrows { (*t).rows } else { nrows };
    for r in 0..(*t).rows {
        let l = bl(b, r);
        if r < keep {
            std::ptr::copy_nonoverlapping(l, ring.add(r as usize), 1);
            if (*ring.add(r as usize)).len as c_int > _ncols {
                (*ring.add(r as usize)).len = _ncols as u16;
            }
            (*l).c = null_mut();
        }
    }
    buf_free(b);
    (*b).ring = ring;
    (*b).cap = nrows;
    (*b).count = nrows;
    (*b).head = 0;
}

#[no_mangle]
pub unsafe extern "C" fn vt_resize(t: *mut Term, mut ncols: c_int, mut nrows: c_int) {
    if ncols < 2 {
        ncols = 2;
    }
    if nrows < 1 {
        nrows = 1;
    }
    if ncols > TERM_MAX_COLS {
        ncols = TERM_MAX_COLS;
    }
    if nrows > TERM_MAX_ROWS {
        nrows = TERM_MAX_ROWS;
    }
    if ncols == (*t).cols && nrows == (*t).rows {
        return;
    }
    let tabs = libc::realloc((*t).tabs.cast(), ncols as usize) as *mut u8;
    let dirty = libc::realloc((*t).dirty.cast(), nrows as usize) as *mut u8;
    if tabs.is_null() || dirty.is_null() {
        return;
    }
    (*t).tabs = tabs;
    (*t).dirty = dirty;
    let in_alt = (*t).buf == &mut (*t).alt as *mut Buffer;
    let mut mcx = if in_alt { (*t).saved_alt.cx } else { (*t).cx };
    let mut mcy = if in_alt { (*t).saved_alt.cy } else { (*t).cy };
    reflow_main(t, ncols, nrows, &mut mcx, &mut mcy);
    resize_alt(t, ncols, nrows);
    let ocols = (*t).cols;
    (*t).cols = ncols;
    (*t).rows = nrows;
    if in_alt {
        (*t).saved_alt.cx = mcx;
        (*t).saved_alt.cy = mcy;
        (*t).cx = if (*t).cx < ncols { (*t).cx } else { ncols - 1 };
        (*t).cy = if (*t).cy < nrows { (*t).cy } else { nrows - 1 };
    } else {
        (*t).cx = mcx;
        (*t).cy = mcy;
    }
    (*t).wrap_pending = 0;
    if (*t).saved.cx >= ncols {
        (*t).saved.cx = ncols - 1;
    }
    if (*t).saved.cy >= nrows {
        (*t).saved.cy = nrows - 1;
    }
    (*t).top = 0;
    (*t).bot = nrows - 1;
    reset_tabs(t, if ocols < ncols { ocols } else { ncols });
    if ocols > ncols {
        reset_tabs(t, 0);
    }
    std::ptr::write_bytes((*t).dirty, 1, nrows as usize);
    (*t).view = 0;
    (*t).sel_active = 0;
    vt_search_clear(t);
    dirty_all(t);
}
