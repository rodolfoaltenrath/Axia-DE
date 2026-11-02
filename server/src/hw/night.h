/* night.h — Night Light math for Settings → Displays: color temperature → RGB gains, the gamma
 * ramp (KMS gamma LUT, or the software LUT when the card has none), sunrise/sunset and the
 * schedule blend. No location services: the coordinates come from the chosen time zone
 * (zone1970.tab), never from the network. Unit-tested in tests/night_test.c. */
#ifndef LORD_HW_NIGHT_H
#define LORD_HW_NIGHT_H

#include <stdint.h>

#define NIGHT_K_MIN   1900
#define NIGHT_K_MAX   6500             /* neutral: gains (1, 1, 1) */

/* White point of a blackbody at kelvin, normalized so 6500 K is (1, 1, 1). Clamped to
 * [1000, 10000] K. Gains are in [0, 1]. */
void night_temp_rgb(int kelvin, double *r, double *g, double *b);
/* Gamma ramp of n entries per channel (0..65535) for these gains and brightness (0..1). */
void night_ramp(int n, double r, double g, double b, double bright, uint16_t *red, uint16_t *green, uint16_t *blue);

/* Sunrise and sunset for a date (y, m 1-12, d) at lat/lon (degrees, east positive), in
 * minutes after midnight UTC (may be < 0 or ≥ 1440). Returns 0, or -1 for polar day/night. */
int  night_sun_times(int y, int m, int d, double lat, double lon, double *rise_utc, double *set_utc);

/* How much of the night tint applies at minute `now` of the local day (0..1439) for a night
 * that starts at `from` and ends at `to` (local minutes; may cross midnight), fading over
 * `fade` minutes on each side. 0 = day, 1 = full night. */
double night_amount(int now, int from, int to, int fade);

/* ISO 6709 coordinates as in zone1970.tab ("+4230+00131" or "+404251-0740023"). 0 if valid. */
int  night_parse_coords(const char *s, double *lat, double *lon);
/* Looks up a zone ("America/Argentina/Buenos_Aires") in a zone1970.tab text. 0 if found. */
int  night_zone_coords(const char *tab, const char *zone, double *lat, double *lon);

#endif
