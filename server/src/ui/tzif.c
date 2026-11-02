/* tzif.c — time zones with our own code: TZif files (RFC 8536, versions 1 to 4) and POSIX TZ
 * rules (the TZif footer, for dates after the last transition). See tzif.h.
 * Everything is validated: a short file or out-of-range indices return -1, it never over-reads. */
#include "tzif.h"

#include <ctype.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static uint32_t be32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }
static int64_t be64(const uint8_t *p) { return (int64_t)((uint64_t)be32(p) << 32 | be32(p + 4)); }

/* ---- dates ---- */

/* days since 1970-01-01 (Howard Hinnant's algorithm) */
static int64_t days_from_civil(int64_t y, int m, int d) {
    y -= m <= 2;
    int64_t era = (y >= 0 ? y : y - 399) / 400;
    int64_t yoe = y - era * 400;
    int64_t doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + doe - 719468;
}

static int64_t year_of(int64_t t) {
    int64_t z = (t >= 0 ? t : t - 86399) / 86400 + 719468;
    int64_t era = (z >= 0 ? z : z - 146096) / 146097;
    int64_t doe = z - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    int64_t mp = (5 * doy + 2) / 153;
    int m = (int)(mp < 10 ? mp + 3 : mp - 9);
    return yoe + era * 400 + (m <= 2);
}

static int leap(int64_t y) { return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0; }
static int mdays(int64_t y, int m) {
    static const int d[] = { 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 };
    return m == 2 && leap(y) ? 29 : d[m - 1];
}

/* ---- POSIX rules ---- */

typedef struct { char kind; int a, b, c; int32_t time; } TzDate;   /* 'J' n, 'D' n, 'M' m.w.d */

static const char *p_name(const char *s, char *out, size_t n) {
    size_t k = 0;
    if (*s == '<') {
        s++;
        while (*s && *s != '>') { if (k + 1 < n) out[k++] = *s; s++; }
        if (*s != '>') return NULL;
        s++;
    } else {
        while (isalpha((unsigned char)*s)) { if (k + 1 < n) out[k++] = *s; s++; }
    }
    out[k] = 0;
    return k >= 1 ? s : NULL;
}

/* [+-]hh[:mm[:ss]]; max: allowed hours (24 for offsets, 167 for rule times) */
static const char *p_time(const char *s, int32_t *out, int max) {
    int sign = 1;
    if (*s == '+') s++;
    else if (*s == '-') { sign = -1; s++; }
    if (!isdigit((unsigned char)*s)) return NULL;
    long h = 0, m = 0, sec = 0;
    while (isdigit((unsigned char)*s)) { h = h * 10 + (*s++ - '0'); if (h > max) return NULL; }
    if (*s == ':') {
        s++;
        if (!isdigit((unsigned char)*s)) return NULL;
        while (isdigit((unsigned char)*s)) { m = m * 10 + (*s++ - '0'); if (m > 59) return NULL; }
        if (*s == ':') {
            s++;
            if (!isdigit((unsigned char)*s)) return NULL;
            while (isdigit((unsigned char)*s)) { sec = sec * 10 + (*s++ - '0'); if (sec > 59) return NULL; }
        }
    }
    *out = (int32_t)(sign * (h * 3600 + m * 60 + sec));
    return s;
}

static const char *p_num(const char *s, int *out, int lo, int hi) {
    if (!isdigit((unsigned char)*s)) return NULL;
    long v = 0;
    while (isdigit((unsigned char)*s)) { v = v * 10 + (*s++ - '0'); if (v > hi) return NULL; }
    if (v < lo) return NULL;
    *out = (int)v;
    return s;
}

static const char *p_date(const char *s, TzDate *d) {
    d->time = 7200;
    if (*s == 'J') { d->kind = 'J'; s = p_num(s + 1, &d->a, 1, 365); }
    else if (*s == 'M') {
        d->kind = 'M';
        s = p_num(s + 1, &d->a, 1, 12);
        if (s && *s == '.') s = p_num(s + 1, &d->b, 1, 5); else s = NULL;
        if (s && *s == '.') s = p_num(s + 1, &d->c, 0, 6); else s = NULL;
    } else { d->kind = 'D'; s = p_num(s, &d->a, 0, 365); }
    if (s && *s == '/') s = p_time(s + 1, &d->time, 167);
    return s;
}

/* Seconds since 1970 of the start (local 00:00) of the day the rule names in year y. */
static int64_t rule_day(const TzDate *d, int64_t y) {
    int64_t jan1 = days_from_civil(y, 1, 1);
    if (d->kind == 'J') return (jan1 + d->a - 1 + (leap(y) && d->a >= 60)) * 86400;
    if (d->kind == 'D') return (jan1 + d->a) * 86400;
    int64_t first = days_from_civil(y, d->a, 1);
    int wd = (int)(((first + 4) % 7 + 7) % 7);          /* 1970-01-01 was a Thursday */
    int day = 1 + (d->c - wd + 7) % 7 + (d->b - 1) * 7;
    while (day > mdays(y, d->a)) day -= 7;
    return (first + day - 1) * 86400;
}

int tz_posix(const char *rule, int64_t t, TzInfo *out) {
    char std[16], dst[16] = "";
    int32_t stdoff, dstoff;
    const char *s = rule ? p_name(rule, std, sizeof std) : NULL;
    if (!s || !(s = p_time(s, &stdoff, 24))) return -1;
    stdoff = -stdoff;                                   /* POSIX counts westwards */
    if (!*s) { *out = (TzInfo){ .utoff = stdoff }; snprintf(out->abbr, sizeof out->abbr, "%s", std); return 0; }
    if (!(s = p_name(s, dst, sizeof dst))) return -1;
    dstoff = stdoff + 3600;
    if (*s && *s != ',') { if (!(s = p_time(s, &dstoff, 24))) return -1; dstoff = -dstoff; }
    TzDate a = { 'M', 3, 2, 0, 7200 }, b = { 'M', 11, 1, 0, 7200 };   /* default: the US rule */
    if (*s == ',') {
        if (!(s = p_date(s + 1, &a)) || *s != ',' || !(s = p_date(s + 1, &b))) return -1;
    }
    if (*s) return -1;
    int64_t y = year_of(t + stdoff);
    int in = 0;
    for (int64_t yy = y - 1; yy <= y + 1 && !in; yy++) {
        /* the start is given in standard time and the end in daylight time */
        int64_t start = rule_day(&a, yy) + a.time - stdoff;
        int64_t end = rule_day(&b, yy) + b.time - dstoff;
        if (start < end) in = t >= start && t < end;
        else {
            /* southern hemisphere: summer spans the new year; check this year's span */
            int64_t next_end = rule_day(&b, yy + 1) + b.time - dstoff;
            in = t >= start && t < next_end;
        }
    }
    *out = (TzInfo){ .utoff = in ? dstoff : stdoff, .isdst = in };
    snprintf(out->abbr, sizeof out->abbr, "%s", in ? dst : std);
    return 0;
}

/* ---- TZif ---- */

typedef struct { uint32_t isut, isstd, leap, time, type, chars; } Counts;

static int counts(const uint8_t *p, size_t n, Counts *c) {
    if (n < 44 || memcmp(p, "TZif", 4)) return -1;
    c->isut = be32(p + 20); c->isstd = be32(p + 24); c->leap = be32(p + 28);
    c->time = be32(p + 32); c->type = be32(p + 36); c->chars = be32(p + 40);
    if (c->type == 0 || c->type > 256 || c->chars == 0 || c->chars > 4096 || c->time > 1 << 20 || c->leap > 1 << 16) return -1;
    if ((c->isut && c->isut != c->type) || (c->isstd && c->isstd != c->type)) return -1;
    return 0;
}

static size_t block_len(const Counts *c, int v2) {
    size_t ts = v2 ? 8 : 4;
    return (size_t)c->time * ts + c->time + (size_t)c->type * 6 + c->chars + (size_t)c->leap * (ts + 4) + c->isstd + c->isut;
}

static void fill(const uint8_t *types, const uint8_t *chars, uint32_t nchars, unsigned idx, TzInfo *out) {
    const uint8_t *tt = types + idx * 6;
    out->utoff = (int32_t)be32(tt);
    out->isdst = tt[4] != 0;
    unsigned di = tt[5];
    size_t k = 0;
    if (di < nchars) while (di + k < nchars && chars[di + k] && k + 1 < sizeof out->abbr) { out->abbr[k] = (char)chars[di + k]; k++; }
    out->abbr[k] = 0;
}

int tzif_lookup(const uint8_t *buf, size_t n, int64_t t, TzInfo *out) {
    Counts c;
    if (counts(buf, n, &c)) return -1;
    int v2 = buf[4] >= '2';
    const uint8_t *p = buf + 44;
    size_t rem = n - 44, len = block_len(&c, 0);
    if (len > rem) return -1;
    if (v2) {                                   /* skip the 32-bit block */
        p += len; rem -= len;
        if (counts(p, rem, &c)) return -1;
        p += 44; rem -= 44;
        len = block_len(&c, 1);
        if (len > rem) return -1;
    }
    size_t ts = v2 ? 8 : 4;
    const uint8_t *times = p, *idx = p + (size_t)c.time * ts, *types = idx + c.time, *chars = types + (size_t)c.type * 6;
    for (uint32_t i = 0; i < c.time; i++) if (idx[i] >= c.type) return -1;
    const char *footer = NULL;
    char foot[128];
    if (v2 && rem > len + 1 && p[len] == '\n') {
        const uint8_t *f = p + len + 1, *e = memchr(f, '\n', rem - len - 1);
        if (e && (size_t)(e - f) < sizeof foot) { memcpy(foot, f, (size_t)(e - f)); foot[e - f] = 0; footer = foot; }
    }
    #define TT(i) (v2 ? be64(times + (size_t)(i) * 8) : (int64_t)(int32_t)be32(times + (size_t)(i) * 4))
    if (c.time == 0 || t < TT(0)) {
        if (c.time == 0 && footer && footer[0] && !tz_posix(footer, t, out)) return 0;
        fill(types, chars, c.chars, 0, out);
        return 0;
    }
    if (t >= TT(c.time - 1) && footer && footer[0] && !tz_posix(footer, t, out)) return 0;
    uint32_t lo = 0, hi = c.time - 1;           /* the last transition <= t */
    while (lo < hi) {
        uint32_t mid = lo + (hi - lo + 1) / 2;
        if (TT(mid) <= t) lo = mid; else hi = mid - 1;
    }
    #undef TT
    fill(types, chars, c.chars, idx[lo], out);
    return 0;
}

int tz_zone_valid(const char *zone) {
    size_t n = zone ? strlen(zone) : 0;
    if (!n || n > 64 || zone[0] == '/' || strstr(zone, "..") || strstr(zone, "//")) return 0;
    for (const char *p = zone; *p; p++) if (!isalnum((unsigned char)*p) && !strchr("/_+-", *p)) return 0;
    return 1;
}

int tz_zone_offset(const char *zone, int64_t t, TzInfo *out) {
    if (!tz_zone_valid(zone)) return -1;
    char path[160];
    snprintf(path, sizeof path, "/usr/share/zoneinfo/%s", zone);
    /* tzdata uses links: follow them, but never outside zoneinfo */
    char real[PATH_MAX];
    if (!realpath(path, real) || strncmp(real, "/usr/share/zoneinfo/", 20)) return -1;
    int fd = open(real, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size < 44 || st.st_size > (1 << 20)) { close(fd); return -1; }
    size_t n = (size_t)st.st_size;
    uint8_t *buf = malloc(n);
    size_t got = 0;
    while (buf && got < n) {
        ssize_t r = read(fd, buf + got, n - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    close(fd);
    int rc = buf && got == n ? tzif_lookup(buf, n, t, out) : -1;
    free(buf);
    return rc;
}
