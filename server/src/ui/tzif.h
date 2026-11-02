/* tzif.h — our own time zone reader (tzdata TZif files, RFC 8536, and POSIX TZ rules).
 * Used by lord.time.zoneOffset (Clock app, docs/SYSTEM.md) to get another city's time without
 * touching the process time zone (setenv TZ would change the Date of the whole theme). */
#ifndef LORD_TZIF_H
#define LORD_TZIF_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    int32_t utoff;          /* seconds east of UTC (Buenos Aires: -10800) */
    int isdst;
    char abbr[16];          /* "CEST", "-03"… */
} TzInfo;

/* Local time at instant t (Unix seconds) according to an already loaded TZif file. 0 on success. */
int tzif_lookup(const uint8_t *buf, size_t n, int64_t t, TzInfo *out);
/* Same, from a POSIX rule ("CET-1CEST,M3.5.0,M10.5.0/3"): the one in the TZif footer. */
int tz_posix(const char *rule, int64_t t, TzInfo *out);
/* Valid zone name: "Europe/Madrid", "UTC" (no "..", no leading "/", [A-Za-z0-9/_+-]). */
int tz_zone_valid(const char *zone);
/* Reads /usr/share/zoneinfo/<zone> (a regular file up to 1 MB) and looks up t. 0 on success. */
int tz_zone_offset(const char *zone, int64_t t, TzInfo *out);

#endif
