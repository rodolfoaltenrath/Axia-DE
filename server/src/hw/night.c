/* night.c — Night Light math (see night.h). */
#include "night.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define PI 3.14159265358979323846

/* Tanner Helland's fit of the blackbody curve (sRGB 0..255), good to a few percent between
 * 1000 K and 40000 K; only its ratios are used here. */
static void helland(double kelvin, double *r, double *g, double *b) {
    double t = kelvin / 100.0;
    *r = t <= 66 ? 255 : 329.698727446 * pow(t - 60, -0.1332047592);
    *g = t <= 66 ? 99.4708025861 * log(t) - 161.1195681661 : 288.1221695283 * pow(t - 60, -0.0755148492);
    *b = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * log(t - 10) - 305.0447927307;
}

static double clamp01(double v) { return v < 0 ? 0 : v > 1 ? 1 : v; }

void night_temp_rgb(int kelvin, double *r, double *g, double *b) {
    if (kelvin < 1000) kelvin = 1000;
    if (kelvin > 10000) kelvin = 10000;
    double r0, g0, b0, r1, g1, b1;
    helland(NIGHT_K_MAX, &r0, &g0, &b0);
    helland(kelvin, &r1, &g1, &b1);
    *r = clamp01(r1 / r0);
    *g = clamp01(g1 / g0);
    *b = clamp01(b1 / b0);
    if (kelvin == NIGHT_K_MAX) *r = *g = *b = 1;
}

void night_ramp(int n, double r, double g, double b, double bright, uint16_t *red, uint16_t *green, uint16_t *blue) {
    bright = clamp01(bright);
    for (int i = 0; i < n; i++) {
        double v = n > 1 ? (double)i / (n - 1) : 1;
        red[i] = (uint16_t)(clamp01(v * r * bright) * 65535 + 0.5);
        green[i] = (uint16_t)(clamp01(v * g * bright) * 65535 + 0.5);
        blue[i] = (uint16_t)(clamp01(v * b * bright) * 65535 + 0.5);
    }
}

static int day_of_year(int y, int m, int d) {
    static const int cum[12] = { 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334 };
    int leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    return cum[(m - 1) % 12] + d + (leap && m > 2);
}

static double fmod360(double v) { v = fmod(v, 360); return v < 0 ? v + 360 : v; }
static double rad(double d) { return d * PI / 180; }
static double deg(double r) { return r * 180 / PI; }

/* "Almanac for Computers" (US Naval Observatory, 1990) sunrise algorithm, official zenith
 * 90°50'. Accurate to about two minutes away from the poles. */
static int sun_event(int n, double lat, double lon, int rising, double *ut_min) {
    double lng_hour = lon / 15;
    double t = n + ((rising ? 6 : 18) - lng_hour) / 24;
    double m = 0.9856 * t - 3.289;
    double l = fmod360(m + 1.916 * sin(rad(m)) + 0.020 * sin(rad(2 * m)) + 282.634);
    double ra = fmod360(deg(atan(0.91764 * tan(rad(l)))));
    ra += floor(l / 90) * 90 - floor(ra / 90) * 90;
    ra /= 15;
    double sin_dec = 0.39782 * sin(rad(l));
    double cos_dec = cos(asin(sin_dec));
    double cos_h = (cos(rad(90.833)) - sin_dec * sin(rad(lat))) / (cos_dec * cos(rad(lat)));
    if (cos_h > 1 || cos_h < -1) return -1;
    double h = rising ? 360 - deg(acos(cos_h)) : deg(acos(cos_h));
    h /= 15;
    double local = h + ra - 0.06571 * t - 6.622;
    double ut = fmod(local - lng_hour, 24);
    if (ut < 0) ut += 24;
    *ut_min = ut * 60;
    return 0;
}

int night_sun_times(int y, int m, int d, double lat, double lon, double *rise_utc, double *set_utc) {
    int n = day_of_year(y, m, d);
    if (sun_event(n, lat, lon, 1, rise_utc) || sun_event(n, lat, lon, 0, set_utc)) return -1;
    return 0;
}

double night_amount(int now, int from, int to, int fade) {
    int len = ((to - from) % 1440 + 1440) % 1440;
    if (!len) return 0;
    int d = ((now - from) % 1440 + 1440) % 1440;
    if (d >= len) return 0;
    if (fade <= 0) return 1;
    double a = (double)d / fade, b = (double)(len - d) / fade;
    return clamp01(a < b ? a : b);
}

/* ±DDMM[SS] (lat) or ±DDDMM[SS] (lon): deg_digits = 2 or 3 */
static int coord(const char *s, int len, int deg_digits, double *out) {
    if (len != 1 + deg_digits + 2 && len != 1 + deg_digits + 4) return -1;
    if (s[0] != '+' && s[0] != '-') return -1;
    int v[7] = { 0 };
    for (int i = 1; i < len; i++) { if (s[i] < '0' || s[i] > '9') return -1; v[i] = s[i] - '0'; }
    int dg = 0;
    for (int i = 1; i <= deg_digits; i++) dg = dg * 10 + v[i];
    int mn = v[deg_digits + 1] * 10 + v[deg_digits + 2];
    int sc = len > 1 + deg_digits + 2 ? v[deg_digits + 3] * 10 + v[deg_digits + 4] : 0;
    if (mn > 59 || sc > 59) return -1;
    double r = dg + mn / 60.0 + sc / 3600.0;
    *out = s[0] == '-' ? -r : r;
    return 0;
}

int night_parse_coords(const char *s, double *lat, double *lon) {
    size_t n = strlen(s);
    size_t split = 1;
    while (split < n && s[split] != '+' && s[split] != '-') split++;
    if (split >= n) return -1;
    if (coord(s, (int)split, 2, lat) || coord(s + split, (int)(n - split), 3, lon)) return -1;
    if (*lat < -90 || *lat > 90 || *lon < -180 || *lon > 180) return -1;
    return 0;
}

int night_zone_coords(const char *tab, const char *zone, double *lat, double *lon) {
    size_t zl = strlen(zone);
    if (!zl) return -1;
    for (const char *line = tab; line && *line;) {
        const char *end = strchr(line, '\n');
        size_t ll = end ? (size_t)(end - line) : strlen(line);
        if (ll && line[0] != '#') {
            /* codes \t coordinates \t TZ [\t comments] */
            const char *t1 = memchr(line, '\t', ll);
            const char *t2 = t1 ? memchr(t1 + 1, '\t', (size_t)(line + ll - t1 - 1)) : NULL;
            if (t2) {
                const char *z = t2 + 1, *ze = memchr(z, '\t', (size_t)(line + ll - z));
                if (!ze) ze = line + ll;
                if ((size_t)(ze - z) == zl && !memcmp(z, zone, zl)) {
                    char c[24];
                    size_t cl = (size_t)(t2 - t1 - 1);
                    if (cl >= sizeof c) return -1;
                    memcpy(c, t1 + 1, cl);
                    c[cl] = 0;
                    return night_parse_coords(c, lat, lon);
                }
            }
        }
        line = end ? end + 1 : NULL;
    }
    return -1;
}
