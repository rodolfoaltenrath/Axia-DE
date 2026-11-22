/* fancurve.h — what lord.fan (ui/js_fan.c) shares with axia-fand: the limits of a request and two
 * validations. axia-fand itself (profiles, curves with hysteresis, fan.json, the sysfs side and
 * the socket loop) is Rust: rust/axia-fan; this is its C ABI (src/ffi.rs). docs/FANS.md. */
#ifndef LORD_FAN_CURVE_H
#define LORD_FAN_CURVE_H

#include <stddef.h>

#define FAN_MAX_POINTS    8       /* points of a custom curve */
#define FAN_REQ_MAX       256     /* longest request line accepted on the socket */
#define FAN_FORCE_MAX_MIN (24 * 60)

/* The canonical name of a profile (auto|quiet|balanced|performance|always|custom) copied to out;
 * 0, or -1 if s is not one or the name does not fit in n bytes. */
int fan_profile_canonical(const char *s, char *out, size_t n);

/* 0 if a user curve of n points is acceptable: 1..FAN_MAX_POINTS points, 30..90 °C strictly
 * increasing, 0..100 % never decreasing. -1 otherwise. */
int fan_curve_points_valid(const int *temps, const int *pcts, int n);

#endif
