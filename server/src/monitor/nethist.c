/* nethist.c — the Monitor's traffic history file (see nethist.h and docs/MONITOR.md).
 *
 * Only the minute in progress lives in memory (its totals and one entry per app that moved bytes).
 * When it ends one record is appended to the file; queries and compaction read the file through the
 * same strict reader, which checks every length and the CRC of every record and stops at the first
 * record that doesn't check out. */
#include "nethist.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum { NH_APP = 1, NH_MIN = 2, NH_HOUR = 3 };
#define NH_MAGIC "LNH1"
#define NH_VERSION 1
#define NH_HDR 8
#define NH_REC_MAX 65535
#define NH_COMPACT_EVERY (6 * 60)       /* minutes */

typedef struct { uint32_t idx; uint64_t rx, tx; } Acc;

struct NetHist {
    char path[PATH_MAX];
    int loaded;                         /* the app table was read from the file */
    char (*ids)[NH_ID_MAX];             /* index → app id; 0 is "other" */
    uint8_t *written;                   /* its NH_APP record is in the file */
    int napps;
    long long cur;                      /* the minute in progress (Unix time / 60), 0 = none */
    uint64_t rx, tx;
    Acc *acc;
    int nacc, cacc;
    int since_compact;
    long long last_compact;
};

/* ---- small helpers ------------------------------------------------------------------------------ */

static uint32_t crc_table[256];
static uint32_t crc32_of(const uint8_t *p, size_t n) {
    if (!crc_table[1])
        for (uint32_t i = 0; i < 256; i++) {
            uint32_t c = i;
            for (int k = 0; k < 8; k++) c = c & 1 ? 0xedb88320u ^ (c >> 1) : c >> 1;
            crc_table[i] = c;
        }
    uint32_t c = 0xffffffffu;
    for (size_t i = 0; i < n; i++) c = crc_table[(c ^ p[i]) & 255] ^ (c >> 8);
    return c ^ 0xffffffffu;
}

typedef struct { uint8_t *p; size_t n, cap; int bad; } Out;

static void out_need(Out *o, size_t k) {
    if (o->bad || o->n + k <= o->cap) return;
    size_t c = o->cap ? o->cap * 2 : 4096;
    while (c < o->n + k) c *= 2;
    uint8_t *q = realloc(o->p, c);
    if (!q) { o->bad = 1; return; }
    o->p = q;
    o->cap = c;
}
static void out_bytes(Out *o, const void *b, size_t k) { out_need(o, k); if (!o->bad) { memcpy(o->p + o->n, b, k); o->n += k; } }
static void out_u32(Out *o, uint32_t v) { uint8_t b[4] = { (uint8_t)v, (uint8_t)(v >> 8), (uint8_t)(v >> 16), (uint8_t)(v >> 24) }; out_bytes(o, b, 4); }
static void out_var(Out *o, uint64_t v) {
    uint8_t b[10];
    int k = 0;
    do { b[k] = (uint8_t)(v & 127); v >>= 7; if (v) b[k] |= 128; k++; } while (v);
    out_bytes(o, b, (size_t)k);
}

/* One record around the payload built in body. */
static void out_rec(Out *o, int type, const Out *body) {
    if (body->bad || body->n > NH_REC_MAX) { o->bad = 1; return; }
    size_t start = o->n;
    uint8_t h[3] = { (uint8_t)type, (uint8_t)body->n, (uint8_t)(body->n >> 8) };
    out_bytes(o, h, 3);
    out_bytes(o, body->p, body->n);
    if (!o->bad) out_u32(o, crc32_of(o->p + start, 3 + body->n));
}

static int valid_id(const char *s) {
    size_t n = strlen(s);
    if (!n || n >= NH_ID_MAX) return 0;
    for (size_t i = 0; i < n; i++) {
        char c = s[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strchr("._:@+~/-", c))) return 0;
    }
    return 1;
}

/* ---- reading -------------------------------------------------------------------------------------- */

typedef struct { const uint8_t *p; size_t n, i; int bad; } In;
static uint64_t in_var(In *r) {
    uint64_t v = 0;
    for (int s = 0; s < 70; s += 7) {
        if (r->i >= r->n) { r->bad = 1; return 0; }
        uint8_t b = r->p[r->i++];
        v |= (uint64_t)(b & 127) << s;
        if (!(b & 128)) return v;
    }
    r->bad = 1;
    return 0;
}
static uint32_t in_u32(In *r) {
    if (r->n - r->i < 4 || r->i > r->n) { r->bad = 1; return 0; }
    const uint8_t *b = r->p + r->i;
    r->i += 4;
    return (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 | (uint32_t)b[3] << 24;
}

/* The next record at *off: 1 with its type and payload, 0 at the end, -1 if it doesn't check out. */
static int rec_next(const uint8_t *buf, size_t len, size_t *off, int *type, const uint8_t **pl, size_t *pn) {
    size_t o = *off;
    if (o == len) return 0;
    if (len - o < 7) return -1;
    size_t n = (size_t)buf[o + 1] | (size_t)buf[o + 2] << 8;
    if (len - o - 7 < n) return -1;
    const uint8_t *c = buf + o + 3 + n;
    uint32_t want = (uint32_t)c[0] | (uint32_t)c[1] << 8 | (uint32_t)c[2] << 16 | (uint32_t)c[3] << 24;
    if (crc32_of(buf + o, 3 + n) != want) return -1;
    int t = buf[o];
    if (t != NH_APP && t != NH_MIN && t != NH_HOUR) return -1;
    *type = t;
    *pl = buf + o + 3;
    *pn = n;
    *off = o + 7 + n;
    return 1;
}

/* A minute or hour record: its time, totals and per-app entries (at most max kept in e). Returns the
 * number of entries, or -1 if the payload is malformed. */
static int parse_slot(const uint8_t *pl, size_t pn, uint32_t *t, uint64_t *rx, uint64_t *tx, Acc *e, int max) {
    In r = { pl, pn, 0, 0 };
    *t = in_u32(&r);
    *rx = in_var(&r);
    *tx = in_var(&r);
    uint64_t n = in_var(&r);
    if (r.bad || n > NH_APPS_MAX + 1) return -1;
    int k = 0;
    for (uint64_t i = 0; i < n; i++) {
        Acc a;
        a.idx = (uint32_t)in_var(&r);
        a.rx = in_var(&r);
        a.tx = in_var(&r);
        if (r.bad || a.idx > NH_APPS_MAX) return -1;
        if (k < max) e[k++] = a;
    }
    return r.i == r.n ? k : -1;
}

size_t nh_check(const uint8_t *buf, size_t len, int *records) {
    *records = 0;
    if (len < NH_HDR || memcmp(buf, NH_MAGIC, 4) || buf[4] != NH_VERSION) return len ? 1 : 0;
    size_t off = NH_HDR;
    int type, r;
    const uint8_t *pl;
    size_t pn;
    Acc e[NH_APPS_MAX + 1];
    while ((r = rec_next(buf, len, &off, &type, &pl, &pn)) == 1) {
        if (type == NH_APP) {
            In in = { pl, pn, 0, 0 };
            uint64_t idx = in_var(&in);
            char id[NH_ID_MAX];
            size_t k = pn - in.i;
            if (in.bad || idx == 0 || idx > NH_APPS_MAX || k == 0 || k >= NH_ID_MAX) return off - 7 - pn;
            memcpy(id, pl + in.i, k);
            id[k] = 0;
            if (!valid_id(id)) return off - 7 - pn;
        } else {
            uint32_t t;
            uint64_t a, b;
            if (parse_slot(pl, pn, &t, &a, &b, e, NH_APPS_MAX + 1) < 0) return off - 7 - pn;
        }
        (*records)++;
    }
    return r < 0 ? off : 0;
}

/* The whole file (up to NH_FILE_MAX), checked to be a regular file of this user's. NULL with *len = 0
 * if it doesn't exist; NULL with *len = 1 if it can't be used. */
static uint8_t *read_file(const char *path, size_t *len) {
    *len = 0;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
    if (fd < 0) { if (errno != ENOENT) *len = 1; return NULL; }
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_uid != getuid() || st.st_size > (off_t)NH_FILE_MAX) { close(fd); *len = 1; return NULL; }
    size_t n = (size_t)st.st_size;
    uint8_t *b = malloc(n ? n : 1);
    size_t got = 0;
    while (b && got < n) {
        ssize_t r = read(fd, b + got, n - got);
        if (r <= 0) { if (r < 0 && errno == EINTR) continue; break; }
        got += (size_t)r;
    }
    close(fd);
    if (!b) { *len = 1; return NULL; }
    *len = got;
    return b;
}

/* ---- the app table ---------------------------------------------------------------------------------- */

static int app_grow(NetHist *h, int n) {
    if (n <= h->napps) return 0;
    char (*ids)[NH_ID_MAX] = realloc(h->ids, (size_t)n * NH_ID_MAX);
    if (!ids) return -1;
    h->ids = ids;
    uint8_t *w = realloc(h->written, (size_t)n);
    if (!w) return -1;
    h->written = w;
    for (int i = h->napps; i < n; i++) { h->ids[i][0] = 0; h->written[i] = 0; }
    h->napps = n;
    return 0;
}

static int app_index(NetHist *h, const char *app) {
    for (int i = 1; i < h->napps; i++) if (!strcmp(h->ids[i], app)) return i;
    if (!valid_id(app) || h->napps > NH_APPS_MAX || app_grow(h, h->napps < 1 ? 2 : h->napps + 1)) return 0;   /* "other" */
    snprintf(h->ids[h->napps - 1], NH_ID_MAX, "%s", app);
    return h->napps - 1;
}

/* Reads the app table of the file (and compacts it if its tail is bad). */
static void load(NetHist *h, long long now) {
    if (h->loaded) return;
    h->loaded = 1;
    if (app_grow(h, 1)) return;
    size_t len;
    uint8_t *b = read_file(h->path, &len);
    if (!b) return;
    int recs;
    size_t bad = nh_check(b, len, &recs);
    if (len >= NH_HDR && !memcmp(b, NH_MAGIC, 4) && b[4] == NH_VERSION) {
        size_t off = NH_HDR, end = bad ? bad : len;
        int type;
        const uint8_t *pl;
        size_t pn;
        while (rec_next(b, end, &off, &type, &pl, &pn) == 1) {
            if (type != NH_APP) continue;
            In in = { pl, pn, 0, 0 };
            uint64_t idx = in_var(&in);
            if (in.bad || !idx || idx > NH_APPS_MAX || app_grow(h, (int)idx + 1)) continue;
            size_t k = pn - in.i;
            if (k >= NH_ID_MAX) continue;
            memcpy(h->ids[idx], pl + in.i, k);
            h->ids[idx][k] = 0;
            h->written[idx] = 1;
        }
    }
    free(b);
    if (bad) nh_compact(h, now);
}

/* ---- the minute in progress --------------------------------------------------------------------- */

NetHist *nh_new(const char *path) {
    if (!path || strlen(path) >= PATH_MAX - 8) return NULL;
    NetHist *h = calloc(1, sizeof *h);
    if (!h) return NULL;
    snprintf(h->path, sizeof h->path, "%s", path);
    /* its folder, 0700, one level at a time from the home folder's own */
    char dir[PATH_MAX];
    snprintf(dir, sizeof dir, "%s", path);
    for (char *p = dir + 1; *p; p++) {
        if (*p != '/') continue;
        *p = 0;
        mkdir(dir, 0700);
        *p = '/';
    }
    return h;
}

void nh_add(NetHist *h, long long now, const char *app, uint64_t rx, uint64_t tx) {
    if (!h || (!rx && !tx)) return;
    load(h, now);
    long long m = now / 60;
    if (h->cur && m != h->cur) nh_flush(h, now);
    if (!h->cur) h->cur = m;
    if (!app) { h->rx += rx; h->tx += tx; return; }
    uint32_t idx = (uint32_t)app_index(h, app);
    for (int i = 0; i < h->nacc; i++)
        if (h->acc[i].idx == idx) { h->acc[i].rx += rx; h->acc[i].tx += tx; return; }
    if (h->nacc == h->cacc) {
        int c = h->cacc ? h->cacc * 2 : 16;
        Acc *a = realloc(h->acc, (size_t)c * sizeof *a);
        if (!a) return;
        h->acc = a;
        h->cacc = c;
    }
    h->acc[h->nacc++] = (Acc){ idx, rx, tx };
}

static void slot_body(Out *b, uint32_t t, uint64_t rx, uint64_t tx, const Acc *e, int n) {
    out_u32(b, t);
    out_var(b, rx);
    out_var(b, tx);
    out_var(b, (uint64_t)n);
    for (int i = 0; i < n; i++) { out_var(b, e[i].idx); out_var(b, e[i].rx); out_var(b, e[i].tx); }
}

static void app_rec(Out *o, uint32_t idx, const char *id) {
    Out b = { 0 };
    out_var(&b, idx);
    out_bytes(&b, id, strlen(id));
    out_rec(o, NH_APP, &b);
    free(b.p);
}

/* Appends bytes with one write; the file is created with its header if it isn't there. */
static int append(NetHist *h, const Out *o) {
    int fd = open(h->path, O_WRONLY | O_APPEND | O_CREAT | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK, 0600);
    if (fd < 0) return -1;
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_uid != getuid() || st.st_size > (off_t)NH_FILE_MAX) { close(fd); return -1; }
    int r = 0;
    if (st.st_size == 0) {
        uint8_t hdr[NH_HDR] = { 'L', 'N', 'H', '1', NH_VERSION, 0, 0, 0 };
        if (write(fd, hdr, sizeof hdr) != (ssize_t)sizeof hdr) r = -1;
    }
    if (!r && write(fd, o->p, o->n) != (ssize_t)o->n) r = -1;
    close(fd);
    return r;
}

int nh_flush(NetHist *h, long long now) {
    if (!h) return 0;
    load(h, now);
    int r = 0;
    if (h->cur && now / 60 != h->cur && (h->rx || h->tx || h->nacc)) {
        Out o = { 0 }, b = { 0 };
        /* a new file (cleared, or deleted by hand): every app gets its record again */
        struct stat st;
        if (lstat(h->path, &st)) for (int i = 0; i < h->napps; i++) h->written[i] = 0;
        for (int i = 0; i < h->nacc; i++) {
            uint32_t x = h->acc[i].idx;
            if (x && !h->written[x]) { app_rec(&o, x, h->ids[x]); h->written[x] = 1; }
        }
        slot_body(&b, (uint32_t)h->cur, h->rx, h->tx, h->acc, h->nacc);
        out_rec(&o, NH_MIN, &b);
        r = o.bad ? -1 : append(h, &o);
        if (r) for (int i = 0; i < h->nacc; i++) h->written[h->acc[i].idx] = 0;
        free(o.p);
        free(b.p);
        h->since_compact++;
    }
    if (h->cur && now / 60 != h->cur) { h->cur = 0; h->rx = h->tx = 0; h->nacc = 0; }
    if (h->since_compact >= NH_COMPACT_EVERY || !h->last_compact) nh_compact(h, now);
    return r;
}

void nh_free(NetHist *h) {
    if (!h) return;
    if (h->cur) nh_flush(h, (h->cur + 1) * 60);       /* the minute in progress, as if it ended */
    free(h->ids);
    free(h->written);
    free(h->acc);
    free(h);
}

int nh_clear(NetHist *h) {
    if (!h) return -1;
    int r = unlink(h->path) && errno != ENOENT ? -1 : 0;
    h->cur = 0;
    h->rx = h->tx = 0;
    h->nacc = 0;
    for (int i = 0; i < h->napps; i++) h->written[i] = 0;
    return r;
}

/* ---- compaction ------------------------------------------------------------------------------------- */

typedef struct { uint32_t t; uint32_t idx; uint64_t rx, tx; } HEnt;   /* idx 0xffffffff: the total */
#define TOTAL_IDX 0xffffffffu

static int hent_cmp(const void *a, const void *b) {
    const HEnt *x = a, *y = b;
    if (x->t != y->t) return x->t < y->t ? -1 : 1;
    return x->idx < y->idx ? -1 : x->idx > y->idx;
}

int nh_compact(NetHist *h, long long now) {
    if (!h) return -1;
    h->last_compact = now ? now : 1;
    h->since_compact = 0;
    size_t len;
    uint8_t *b = read_file(h->path, &len);
    if (!b) return len ? -1 : 0;
    int recs;
    size_t bad = nh_check(b, len, &recs);
    if (len < NH_HDR || memcmp(b, NH_MAGIC, 4) || b[4] != NH_VERSION) bad = NH_HDR, len = NH_HDR;   /* not ours: start over */
    size_t end = bad ? bad : len;
    long long m_cut = now / 60 - NH_MIN_KEEP, h_cut = now / 3600 - NH_HOUR_KEEP, m_max = now / 60 + 24 * 60, h_max = now / 3600 + 24;
    /* old table → new: only apps that are still used, numbered again from 1 */
    int nold = h->napps > 1 ? h->napps : 1;
    char (*fids)[NH_ID_MAX] = calloc((size_t)NH_APPS_MAX + 1, NH_ID_MAX);
    uint32_t *remap = calloc((size_t)NH_APPS_MAX + 1, sizeof *remap);
    HEnt *he = NULL;
    size_t nhe = 0, che = 0;
    Out o = { 0 }, mins = { 0 };
    Acc *e = malloc(sizeof(Acc) * (NH_APPS_MAX + 1));
    int newn = 1, r = -1;
    if (!fids || !remap || !e) goto out;
    size_t off = NH_HDR;
    int type;
    const uint8_t *pl;
    size_t pn;
    /* the file's own ids (they agree with h->ids for the indexes it wrote) */
    while (rec_next(b, end, &off, &type, &pl, &pn) == 1) {
        if (type != NH_APP) continue;
        In in = { pl, pn, 0, 0 };
        uint64_t idx = in_var(&in);
        size_t k = pn - in.i;
        if (in.bad || !idx || idx > NH_APPS_MAX || k >= NH_ID_MAX) continue;
        memcpy(fids[idx], pl + in.i, k);
        fids[idx][k] = 0;
    }
#define MAP(x) ((x) == 0 || (x) > NH_APPS_MAX || !fids[x][0] ? 0u : remap[x] ? remap[x] : (remap[x] = (uint32_t)newn++))
    off = NH_HDR;
    while (rec_next(b, end, &off, &type, &pl, &pn) == 1) {
        if (type == NH_APP) continue;
        uint32_t t;
        uint64_t rx, tx;
        int n = parse_slot(pl, pn, &t, &rx, &tx, e, NH_APPS_MAX + 1);
        if (n < 0) continue;
        if (type == NH_MIN && (long long)t >= m_cut && (long long)t <= m_max) {
            for (int i = 0; i < n; i++) e[i].idx = MAP(e[i].idx);
            Out bd = { 0 };
            slot_body(&bd, t, rx, tx, e, n);
            out_rec(&mins, NH_MIN, &bd);
            free(bd.p);
            continue;
        }
        uint32_t hour = type == NH_MIN ? t / 60 : t;
        if ((long long)hour < h_cut || (long long)hour > h_max) continue;
        if (nhe + (size_t)n + 1 > che) {
            size_t c = che ? che * 2 : 1024;
            while (c < nhe + (size_t)n + 1) c *= 2;
            HEnt *q = realloc(he, c * sizeof *q);
            if (!q) goto out;
            he = q;
            che = c;
        }
        he[nhe++] = (HEnt){ hour, TOTAL_IDX, rx, tx };
        for (int i = 0; i < n; i++) he[nhe++] = (HEnt){ hour, MAP(e[i].idx), e[i].rx, e[i].tx };
    }
#undef MAP
    if (mins.bad) goto out;
    /* the new file: header, apps, minutes, hours (one record per hour, entries summed) */
    uint8_t hdr[NH_HDR] = { 'L', 'N', 'H', '1', NH_VERSION, 0, 0, 0 };
    out_bytes(&o, hdr, sizeof hdr);
    for (int x = 1; x <= NH_APPS_MAX; x++) if (remap[x]) app_rec(&o, remap[x], fids[x]);
    out_bytes(&o, mins.p ? mins.p : (const uint8_t *)"", mins.n);
    if (nhe) qsort(he, nhe, sizeof *he, hent_cmp);
    for (size_t i = 0; i < nhe;) {
        uint32_t t = he[i].t;
        uint64_t trx = 0, ttx = 0;
        int k = 0;
        for (; i < nhe && he[i].t == t; i++) {
            if (he[i].idx == TOTAL_IDX) { trx += he[i].rx; ttx += he[i].tx; continue; }
            if (k && e[k - 1].idx == he[i].idx) { e[k - 1].rx += he[i].rx; e[k - 1].tx += he[i].tx; }
            else if (k <= NH_APPS_MAX) e[k++] = (Acc){ he[i].idx, he[i].rx, he[i].tx };
        }
        Out bd = { 0 };
        slot_body(&bd, t, trx, ttx, e, k);
        out_rec(&o, NH_HOUR, &bd);
        free(bd.p);
    }
    if (o.bad) goto out;
    /* atomic: a temporary file next to it, then rename */
    char tmp[PATH_MAX + 8];
    snprintf(tmp, sizeof tmp, "%s.tmp", h->path);
    unlink(tmp);
    int fd = open(tmp, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (fd < 0) goto out;
    size_t done = 0;
    while (done < o.n) {
        ssize_t w = write(fd, o.p + done, o.n - done);
        if (w <= 0) { if (w < 0 && errno == EINTR) continue; break; }
        done += (size_t)w;
    }
    int ok = done == o.n && !fsync(fd);
    close(fd);
    if (!ok || rename(tmp, h->path)) { unlink(tmp); goto out; }
    /* the table in memory follows the file; apps only in the minute in progress are added after */
    {
        char (*ids)[NH_ID_MAX] = calloc((size_t)newn + 1, NH_ID_MAX);
        uint8_t *wr = calloc((size_t)newn + 1, 1);
        if (!ids || !wr) { free(ids); free(wr); goto out; }
        for (int x = 1; x <= NH_APPS_MAX; x++) if (remap[x]) { snprintf(ids[remap[x]], NH_ID_MAX, "%s", fids[x]); wr[remap[x]] = 1; }
        char (*oldids)[NH_ID_MAX] = h->ids;
        int oldn = h->napps;
        h->ids = ids;
        h->written = (free(h->written), wr);
        h->napps = newn;
        for (int i = 0; i < h->nacc; i++) {
            uint32_t x = h->acc[i].idx;
            h->acc[i].idx = x && (int)x < oldn && oldids[x][0] ? (uint32_t)app_index(h, oldids[x]) : 0;
        }
        free(oldids);
        (void)nold;
    }
    r = 0;
out:
    free(b);
    free(fids);
    free(remap);
    free(he);
    free(e);
    free(o.p);
    free(mins.p);
    return r;
}

/* ---- queries ------------------------------------------------------------------------------------------ */

static void spread(double *rx, double *tx, long long from, long long step, int n, long long s, long long dur, uint64_t r, uint64_t t) {
    long long a = s, b = s + dur, end = from + step * n;
    if (b <= from || a >= end) return;
    long long i0 = (a > from ? a - from : 0) / step, i1 = ((b < end ? b : end) - from - 1) / step;
    for (long long i = i0; i <= i1 && i < n; i++) {
        long long x0 = from + i * step, x1 = x0 + step;
        if (x0 < a) x0 = a;
        if (x1 > b) x1 = b;
        if (x1 <= x0) continue;
        double f = (double)(x1 - x0) / (double)dur;
        rx[i] += (double)r * f;
        tx[i] += (double)t * f;
    }
}

int nh_query(NetHist *h, long long now, long long from, long long step, int n, const char *app,
             double *rx, double *tx, NhTop *top, int max_top, int *ntop, long long *oldest) {
    if (!h || n < 1 || n > NH_BUCKETS_MAX || step < 1 || step > 86400LL * 31 || from < 0) return -1;
    load(h, now);
    for (int i = 0; i < n; i++) rx[i] = tx[i] = 0;
    if (ntop) *ntop = 0;
    if (oldest) *oldest = 0;
    long long end = from + step * n;
    int res = 0;
    char (*fids)[NH_ID_MAX] = calloc((size_t)NH_APPS_MAX + 1, NH_ID_MAX);
    uint64_t *sum = calloc((size_t)(NH_APPS_MAX + 1) * 2, sizeof *sum);
    Acc *e = malloc(sizeof(Acc) * (NH_APPS_MAX + 1));
    size_t len;
    uint8_t *b = read_file(h->path, &len);
    if (!fids || !sum || !e) goto done;
    if (b && len >= NH_HDR && !memcmp(b, NH_MAGIC, 4) && b[4] == NH_VERSION) {
        size_t off = NH_HDR;
        int type;
        const uint8_t *pl;
        size_t pn;
        while (rec_next(b, len, &off, &type, &pl, &pn) == 1) {
            if (type == NH_APP) {
                In in = { pl, pn, 0, 0 };
                uint64_t idx = in_var(&in);
                size_t k = pn - in.i;
                if (in.bad || !idx || idx > NH_APPS_MAX || k >= NH_ID_MAX) continue;
                memcpy(fids[idx], pl + in.i, k);
                fids[idx][k] = 0;
                continue;
            }
            uint32_t t;
            uint64_t trx, ttx;
            int k = parse_slot(pl, pn, &t, &trx, &ttx, e, NH_APPS_MAX + 1);
            if (k < 0) continue;
            long long dur = type == NH_MIN ? 60 : 3600, s = (long long)t * dur;
            if (oldest && (!*oldest || s < *oldest)) *oldest = s;
            if (s + dur <= from || s >= end) continue;
            if (!res || dur < res) res = (int)dur;
            if (!app) spread(rx, tx, from, step, n, s, dur, trx, ttx);
            for (int i = 0; i < k; i++) {
                uint32_t x = e[i].idx;
                if (x > NH_APPS_MAX) continue;
                sum[x * 2] += e[i].rx;
                sum[x * 2 + 1] += e[i].tx;
                if (app && x && !strcmp(fids[x], app)) spread(rx, tx, from, step, n, s, dur, e[i].rx, e[i].tx);
            }
        }
    }
    /* the minute in progress (not in the file yet), with the table in memory */
    if (h->cur && h->cur * 60 + 60 > from && h->cur * 60 < end) {
        if (!res) res = 60;
        if (oldest && (!*oldest || h->cur * 60 < *oldest)) *oldest = h->cur * 60;
        if (!app) spread(rx, tx, from, step, n, h->cur * 60, 60, h->rx, h->tx);
        for (int i = 0; i < h->nacc; i++) {
            uint32_t x = h->acc[i].idx;
            if (!x || (int)x >= h->napps) continue;
            /* the same id may have another index in the file: add it to that one */
            uint32_t fx = 0;
            for (uint32_t j = 1; j <= NH_APPS_MAX && !fx; j++) if (fids[j][0] && !strcmp(fids[j], h->ids[x])) fx = j;
            if (!fx) for (uint32_t j = NH_APPS_MAX; j >= 1 && !fx; j--) if (!fids[j][0]) { fx = j; snprintf(fids[j], NH_ID_MAX, "%s", h->ids[x]); }
            if (!fx) continue;
            sum[fx * 2] += h->acc[i].rx;
            sum[fx * 2 + 1] += h->acc[i].tx;
            if (app && !strcmp(h->ids[x], app)) spread(rx, tx, from, step, n, h->cur * 60, 60, h->acc[i].rx, h->acc[i].tx);
        }
    }
    if (top && ntop && max_top > 0) {
        for (int k = 0; k < max_top; k++) {
            int best = -1;
            uint64_t bv = 0;
            for (int x = 1; x <= NH_APPS_MAX; x++) {
                uint64_t v = sum[x * 2] + sum[x * 2 + 1];
                if (v > bv && fids[x][0]) { bv = v; best = x; }
            }
            if (best < 0) break;
            snprintf(top[k].id, NH_ID_MAX, "%s", fids[best]);
            top[k].rx = sum[best * 2];
            top[k].tx = sum[best * 2 + 1];
            sum[best * 2] = sum[best * 2 + 1] = 0;
            (*ntop)++;
        }
    }
done:
    free(b);
    free(fids);
    free(sum);
    free(e);
    return res;
}
