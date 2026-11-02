/* tzif_test.c — test of the time zone reader (src/ui/tzif.c).
 *   1. POSIX rules at known instants (Europe, Australia, the US, zones without DST);
 *   2. a TZif v2 built here (transitions, footer) and every truncated prefix of it (no over-reads);
 *   3. if tzdata is installed (/usr/share/zoneinfo): compare against glibc (localtime_r with TZ)
 *      in many zones, from 1970 to 2045. */
#include "ui/tzif.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static int fails;

static void check(const char *what, int ok) {
    if (!ok) { printf("✗ %s\n", what); fails++; }
}

static void rule(const char *r, int64_t t, int32_t off, const char *abbr) {
    TzInfo z;
    char what[160];
    snprintf(what, sizeof what, "%s @%lld → %d %s", r, (long long)t, off, abbr);
    check(what, !tz_posix(r, t, &z) && z.utoff == off && !strcmp(z.abbr, abbr));
}

/* ---- hand-built TZif ---- */

static size_t put32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16); p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)v; return 4; }
static size_t put64(uint8_t *p, int64_t v) { put32(p, (uint32_t)((uint64_t)v >> 32)); put32(p + 4, (uint32_t)v); return 8; }

static size_t header(uint8_t *p, char ver, uint32_t timecnt, uint32_t typecnt, uint32_t charcnt) {
    memset(p, 0, 44);
    memcpy(p, "TZif", 4);
    p[4] = (uint8_t)ver;
    put32(p + 32, timecnt); put32(p + 36, typecnt); put32(p + 40, charcnt);
    return 44;
}

/* two types: "AAA" +1 h and "BBB" +2 h (DST); transitions at 1000 and 2000; footer "AAA-1" */
static size_t build(uint8_t *b) {
    static const int64_t tr[] = { 1000, 2000 };
    static const uint8_t ix[] = { 1, 0 };
    const char chars[] = "AAA\0BBB";
    size_t n = 0;
    for (int v2 = 0; v2 < 2; v2++) {
        n += header(b + n, '2', 2, 2, 8);
        for (int i = 0; i < 2; i++) n += v2 ? put64(b + n, tr[i]) : put32(b + n, (uint32_t)tr[i]);
        memcpy(b + n, ix, 2); n += 2;
        n += put32(b + n, 3600); b[n++] = 0; b[n++] = 0;
        n += put32(b + n, 7200); b[n++] = 1; b[n++] = 4;
        memcpy(b + n, chars, 8); n += 8;
    }
    memcpy(b + n, "\nAAA-1\n", 7);
    return n + 7;
}

static void synthetic(void) {
    uint8_t b[512];
    size_t n = build(b);
    TzInfo z;
    check("tzif before the first transition", !tzif_lookup(b, n, 0, &z) && z.utoff == 3600 && !strcmp(z.abbr, "AAA"));
    check("tzif at the first transition", !tzif_lookup(b, n, 1000, &z) && z.utoff == 7200 && z.isdst && !strcmp(z.abbr, "BBB"));
    check("tzif between transitions", !tzif_lookup(b, n, 1999, &z) && z.utoff == 7200);
    check("tzif after the last one (footer)", !tzif_lookup(b, n, 5000, &z) && z.utoff == 3600 && !z.isdst && !strcmp(z.abbr, "AAA"));
    /* truncated: any prefix that cuts the data must fail without over-reading */
    size_t data_end = n - 7;
    for (size_t k = 0; k < data_end; k++) {
        uint8_t *cut = malloc(k ? k : 1);
        memcpy(cut, b, k);
        TzInfo w;
        char what[64];
        snprintf(what, sizeof what, "tzif truncated to %zu bytes", k);
        check(what, tzif_lookup(cut, k, 1500, &w) != 0);
        free(cut);
    }
    /* out-of-range type index */
    uint8_t bad[512];
    memcpy(bad, b, n);
    size_t second = 44 + 2 * 4 + 2 + 12 + 8;           /* start of the v2 block */
    bad[second + 44 + 16] = 9;
    check("tzif with an invalid index", tzif_lookup(bad, n, 1500, &z) != 0);
    check("tzif without magic", tzif_lookup((const uint8_t *)"TZiX", 4, 0, &z) != 0);
}

/* ---- against glibc ---- */

static void against_glibc(void) {
    static const char *zones[] = {
        "UTC", "Europe/Madrid", "Europe/London", "Europe/Moscow", "America/Argentina/Buenos_Aires", "America/New_York",
        "America/Los_Angeles", "America/Sao_Paulo", "America/Santiago", "America/Mexico_City", "Asia/Tokyo", "Asia/Kolkata",
        "Asia/Kathmandu", "Asia/Tehran", "Australia/Sydney", "Australia/Lord_Howe", "Pacific/Auckland", "Pacific/Chatham",
        "Africa/Casablanca", "Asia/Gaza", "America/St_Johns", "Pacific/Kiritimati", "Atlantic/Azores", "Europe/Dublin",
    };
    FILE *probe = fopen("/usr/share/zoneinfo/Europe/Madrid", "r");
    if (!probe) { printf("tzif: no tzdata, skipping the glibc comparison\n"); return; }
    fclose(probe);
    int checked = 0, bad = 0;
    for (size_t zi = 0; zi < sizeof zones / sizeof *zones; zi++) {
        char env[96];
        snprintf(env, sizeof env, ":%s", zones[zi]);
        setenv("TZ", env, 1);
        tzset();
        for (int64_t t = 0; t < 2366841600LL; t += 86400 * 3 + 3607) {   /* 1970–2045, every ~3 days */
            time_t tt = (time_t)t;
            struct tm tm;
            if (!localtime_r(&tt, &tm)) continue;
            TzInfo z;
            if (tz_zone_offset(zones[zi], t, &z)) { bad++; if (bad < 10) printf("✗ %s: could not be read\n", zones[zi]); break; }
            checked++;
            if (z.utoff != tm.tm_gmtoff || strcmp(z.abbr, tm.tm_zone)) {
                if (bad++ < 10) printf("✗ %s @%lld: ours %d %s, glibc %ld %s\n", zones[zi], (long long)t, z.utoff, z.abbr, tm.tm_gmtoff, tm.tm_zone);
            }
        }
    }
    printf("tzif: %d instants compared with glibc, %d different\n", checked, bad);
    if (bad) fails++;
}

int main(void) {
    rule("CET-1CEST,M3.5.0,M10.5.0/3", 1774745999, 3600, "CET");
    rule("CET-1CEST,M3.5.0,M10.5.0/3", 1774746000, 7200, "CEST");
    rule("CET-1CEST,M3.5.0,M10.5.0/3", 1792889999, 7200, "CEST");
    rule("CET-1CEST,M3.5.0,M10.5.0/3", 1792890000, 3600, "CET");
    rule("AEST-10AEDT,M10.1.0,M4.1.0/3", 1775318399, 39600, "AEDT");
    rule("AEST-10AEDT,M10.1.0,M4.1.0/3", 1775318400, 36000, "AEST");
    rule("AEST-10AEDT,M10.1.0,M4.1.0/3", 1791043199, 36000, "AEST");
    rule("AEST-10AEDT,M10.1.0,M4.1.0/3", 1791043200, 39600, "AEDT");
    rule("AEST-10AEDT,M10.1.0,M4.1.0/3", 1768478400, 39600, "AEDT");
    rule("EST5EDT,M3.2.0,M11.1.0", 1772953199, -18000, "EST");
    rule("EST5EDT,M3.2.0,M11.1.0", 1772953200, -14400, "EDT");
    rule("EST5EDT,M3.2.0,M11.1.0", 1793512799, -14400, "EDT");
    rule("EST5EDT,M3.2.0,M11.1.0", 1793512800, -18000, "EST");
    rule("EST5EDT", 1772953200, -14400, "EDT");           /* no rule: the US one */
    rule("<-03>3", 1768478400, -10800, "-03");
    rule("<+0545>-5:45", 1768478400, 20700, "+0545");
    rule("UTC0", 0, 0, "UTC");
    TzInfo z;
    check("invalid rule", tz_posix("CET-1CEST,M13.5.0,M10.5.0", 0, &z) != 0);
    check("empty rule", tz_posix("", 0, &z) != 0);
    check("zone with ..", !tz_zone_valid("../etc/passwd") && !tz_zone_valid("/etc/localtime") && tz_zone_valid("America/Argentina/Buenos_Aires"));
    check("missing zone", tz_zone_offset("Nowhere/Nothing", 0, &z) != 0);
    synthetic();
    against_glibc();
    if (fails) { printf("tzif: %d failures\n", fails); return 1; }
    printf("tzif: ok\n");
    return 0;
}
